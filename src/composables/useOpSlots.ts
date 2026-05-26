/**
 * 运营位 (Op Slot) · 客户端 reactive 状态管理
 *
 * 一处获取 · 全局共享 · WS 推送实时热更新
 *
 * 链路:
 *   1. 启动时 fetchActive() · 调 GET /api/v8/op-slots/active?keys=splash,banner,notice,activity
 *   2. listen("op_slots:changed") · Rust 端 v8_ws 收到 op_slots_changed WS 帧后 emit
 *   3. 收到推送 → 立即重新 fetchActive() · 客户端瞬间生效
 *   4. dismissSplash(id) · 用户关掉的 splash 当前会话不再弹 (不持久化)
 *
 * 浏览器 dev preview 无 Tauri runtime · invoke/listen 静默失败 · 不阻塞渲染
 */
import { ref, computed, onMounted } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { apiUrl } from "@shared"

// ════════════════════════════════════════════════════════════════════
// 类型定义 (跟后端 api/v8/op_slots.py OpSlotItem 对齐)
// ════════════════════════════════════════════════════════════════════
export type OpSlotKey = "splash" | "banner" | "notice" | "activity"
// 2026-05-25 8.0.9 · 新增 modal (rich_html 弹窗) + embed_url (iframe 弹窗)
export type OpActionType = "none" | "external" | "internal" | "modal" | "embed_url" | "download" | "qr"

export interface OpSlotItem {
  id: number
  slot_key: string
  title: string
  subtitle: string
  image_url: string | null
  video_url: string | null
  rich_html: string | null
  action_type: OpActionType
  action_target: string | null
  action_label: string | null
  priority: number
  cooldown_hours: number
  show_once: boolean
  closable: boolean
  start_at: string | null
  end_at: string | null
}

interface ActiveSlotsResponse {
  ok: boolean
  server_time: string
  slots: Record<string, OpSlotItem[]>
}

interface ChangedEventPayload {
  affected_keys: string[]
  action: string
  ts: string
}

// ════════════════════════════════════════════════════════════════════
// 单例 reactive state (跨组件共享 · 避免重复拉取)
// ════════════════════════════════════════════════════════════════════
const slotsByKey = ref<Record<string, OpSlotItem[]>>({})
const loading = ref(false)
const lastFetchAt = ref(0)

/** 默认订阅的 slot_key 列表 · 加新 slot 时扩展这里 */
const SUBSCRIBED_KEYS: OpSlotKey[] = ["splash", "banner", "notice", "activity"]

/** Splash 类型用户关掉过的 id · session-only · 不持久化 */
const dismissedSplashIds = ref<Set<number>>(new Set())

/** Notice 类型用户关掉过的 id · session-only */
const dismissedNoticeIds = ref<Set<number>>(new Set())

let listenerRegistered = false

// ════════════════════════════════════════════════════════════════════
// 内部函数
// ════════════════════════════════════════════════════════════════════
/**
 * 收集客户端定向 tags · 用于后端 audience.tags_any/tags_all/tags_not 匹配
 * 零隐私设计: 仅设备级别 + 客户端已声明的能力 · 不涉及个人信息
 */
function collectClientTags(): string[] {
  const tags: string[] = []
  // 1. OS 探测 (navigator.userAgent 简单解析)
  try {
    const ua = navigator.userAgent.toLowerCase()
    if (ua.includes("mac")) tags.push("os:macos")
    else if (ua.includes("win")) tags.push("os:windows")
    else if (ua.includes("linux")) tags.push("os:linux")
  } catch {
    /* SSR / 非浏览器环境 · 静默 */
  }
  // 2. 客户端版本 (从 Vite 构建注入 · 后期可加)
  // 3. 已授权能力 tags (从 localStorage 的 capability_consent 取)
  try {
    const raw = localStorage.getItem("qs.capability_consent.v1")
    if (raw) {
      const parsed = JSON.parse(raw) as { consents?: Record<string, boolean> }
      const consents = parsed.consents || {}
      for (const [id, ok] of Object.entries(consents)) {
        if (ok) tags.push(`capability:${id}`)
      }
    }
  } catch {
    /* localStorage 损坏 · 静默 */
  }
  // 4. (后期扩展) skill:xxx · gpu:xxx · arch:xxx
  // 这些需要 Tauri invoke get_system_info · MVP 暂不实现
  return tags
}

async function fetchActive(): Promise<void> {
  loading.value = true
  try {
    const tags = collectClientTags()
    const tagsParam = tags.length > 0 ? `&tags=${encodeURIComponent(tags.join(","))}` : ""
    // 2026-05-25 · apiUrl(base) 已自带 /api/v8 前缀 · path 不要再写 /api/v8 (否则路径重复 404)
    const url = apiUrl(
      `/op-slots/active?keys=${SUBSCRIBED_KEYS.join(",")}${tagsParam}`,
    )
    console.log("[op-slots] fetching:", url, "tags=", tags)
    const raw = await invoke<string>("api_get", { url })
    const resp = JSON.parse(raw) as ActiveSlotsResponse
    console.log("[op-slots] raw response:", resp)
    if (resp.ok) {
      slotsByKey.value = resp.slots || {}
      lastFetchAt.value = Date.now()
      const counts = Object.fromEntries(
        Object.entries(slotsByKey.value).map(([k, v]) => [k, v.length]),
      )
      console.log(
        "[op-slots] ✓ loaded · counts=",
        counts,
        "· splash="
          + (slotsByKey.value.splash?.[0]?.title || "—")
          + " · banner="
          + (slotsByKey.value.banner?.[0]?.title || "—"),
      )
    } else {
      console.warn("[op-slots] response ok=false:", resp)
    }
  } catch (e) {
    console.error("[op-slots] ✗ fetch failed:", e)
  } finally {
    loading.value = false
  }
}

async function ensureListener(): Promise<void> {
  if (listenerRegistered) return
  listenerRegistered = true
  try {
    await listen<ChangedEventPayload>("op_slots:changed", (event) => {
      console.info(
        "[op-slots] hot update received · keys=%o action=%s",
        event.payload.affected_keys,
        event.payload.action,
      )
      void fetchActive()
    })
    console.debug("[op-slots] WS push listener registered")
  } catch (e) {
    console.debug("[op-slots] listen failed (browser preview?):", e)
  }
}

// ════════════════════════════════════════════════════════════════════
// 公共 composable
// ════════════════════════════════════════════════════════════════════
export function useOpSlots() {
  // 首次挂载触发拉取 + 启 listener (单例 · 多次调用幂等)
  onMounted(() => {
    void ensureListener()
    // 30s 内已拉过就不重复 (路由切换 / 组件重挂时)
    if (Date.now() - lastFetchAt.value > 30_000) {
      void fetchActive()
    }
  })

  /** 开屏广告列表 · 已排除用户关掉的 · 按 priority 降序 */
  const splash = computed(() =>
    (slotsByKey.value.splash || []).filter((s) => !dismissedSplashIds.value.has(s.id)),
  )

  /** 首页 banner 广告位 · 通常取首条 */
  const banner = computed(() => slotsByKey.value.banner || [])

  /** 通知公告条 · 已排除关掉的 */
  const notice = computed(() =>
    (slotsByKey.value.notice || []).filter((n) => !dismissedNoticeIds.value.has(n.id)),
  )

  /** 活动入口 · 卡片式 */
  const activity = computed(() => slotsByKey.value.activity || [])

  /** 当前应该弹的开屏广告 (取最高 priority · null 表示无需弹) */
  const activeSplash = computed<OpSlotItem | null>(() => splash.value[0] || null)

  function dismissSplash(id: number) {
    dismissedSplashIds.value.add(id)
  }

  function dismissNotice(id: number) {
    dismissedNoticeIds.value.add(id)
  }

  return {
    splash,
    banner,
    notice,
    activity,
    activeSplash,
    loading,
    refresh: fetchActive,
    dismissSplash,
    dismissNotice,
  }
}

// ════════════════════════════════════════════════════════════════════
// 埋点上报 · 给后端 we_op_slot_events 写入 impression/click/dismiss
//
// 设计:
//   - 同一 slot_id × event_type 在本进程内仅上报一次 (impression)
//     · 避免组件多次 mount 重复计数
//   - click/dismiss 每次都报 · 操作即埋点
//   - 全部 best-effort · 网络 / Tauri 故障静默 · 不阻塞 UI
//   - 通过 Rust api_post 走 (而非 fetch) · 复用客户端鉴权 + 代理
// ════════════════════════════════════════════════════════════════════
type EventType = "impression" | "click" | "dismiss" | "close"

const reportedImpressions = new Set<string>() // `${slot_id}` set

interface ReportExtras {
  action_type?: string
  action_target?: string
}

export async function reportSlotEvent(
  slotId: number,
  eventType: EventType,
  extras: ReportExtras = {},
): Promise<void> {
  // 同设备同会话同 slot 仅报一次 impression (避免重复计数)
  if (eventType === "impression") {
    const key = `${slotId}`
    if (reportedImpressions.has(key)) return
    reportedImpressions.add(key)
  }
  try {
    const body: Record<string, unknown> = {
      client_tags: collectClientTags(),
      ...extras,
    }
    // 客户端版本 + os
    try {
      const ua = navigator.userAgent.toLowerCase()
      body.client_os = ua.includes("mac")
        ? "macos"
        : ua.includes("win")
          ? "windows"
          : ua.includes("linux")
            ? "linux"
            : null
    } catch {
      /* 静默 */
    }
    try {
      body.client_version = __APP_VERSION__
    } catch {
      /* 没注入版本号 · 静默 */
    }
    await invoke("api_post", {
      url: apiUrl(`/op-slots/${slotId}/${eventType}`),
      body,
    })
  } catch (e) {
    // 浏览器 dev preview 无 Tauri runtime · 静默
    console.debug("[op-slot-event] report failed:", eventType, slotId, e)
  }
}

// __APP_VERSION__ 由 vite.config.ts define 注入 · 类型在 vite-env.d.ts
declare const __APP_VERSION__: string
