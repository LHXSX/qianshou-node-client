/**
 * 广告 / 通知 / Splash 点击行为统一 dispatcher
 *
 * 2026-05-25 8.0.9 · 统一所有 op_slot 点击逻辑 ·
 *                    AdSlot · NoticeMarquee · SplashAdModal 都走这里
 *
 * 7 种 action_type:
 *   - none      不可点
 *   - external  系统浏览器开外链          (action_target = https://...)
 *   - internal  客户端内 hash 跳转        (action_target = #market / #earnings / ...)
 *   - modal     客户端弹窗显示 rich_html  (rich_html 必填)
 *   - embed_url 客户端 iframe 弹窗加载 URL (action_target = https://... 嵌入 webview)
 *   - download  浏览器下载文件            (action_target = .dmg/.exe 等 URL)
 *   - qr        弹窗显示二维码            (action_target = 二维码扫码后访问的 URL)
 *
 * 副作用: 任意点击都会 best-effort 上报 click 埋点。
 */
import { ref } from "vue"
import { open as shellOpen } from "@tauri-apps/plugin-shell"
import {
  reportSlotEvent,
  type OpSlotItem,
} from "./useOpSlots"

// ─────────────────────────────────────────────────────────────────
// 弹窗显示状态 (单例 · 整 app 共享 · AppShell 渲染 <AdActionModal>)
// ─────────────────────────────────────────────────────────────────
export type ActionModalKind = "rich_html" | "embed_url" | "qr"

export interface ActionModalState {
  open: boolean
  kind: ActionModalKind
  title: string
  subtitle: string
  /** rich_html 时 = HTML 字符串 · embed_url 时 = url · qr 时 = 二维码内容 */
  payload: string
  /** 点击 CTA 按钮时的目标 (qr 时可为空) */
  ctaUrl: string | null
  ctaLabel: string | null
  slotId: number | null
}

const modal = ref<ActionModalState>({
  open: false,
  kind: "rich_html",
  title: "",
  subtitle: "",
  payload: "",
  ctaUrl: null,
  ctaLabel: null,
  slotId: null,
})

function openModal(s: Partial<ActionModalState>) {
  modal.value = { ...modal.value, ...s, open: true }
}

function closeModal() {
  modal.value.open = false
}

// ─────────────────────────────────────────────────────────────────
// 工具函数
// ─────────────────────────────────────────────────────────────────
async function openExternal(url: string): Promise<void> {
  // 2026-05-26 fix · 用 @tauri-apps/plugin-shell open() · ACL shell:allow-open 已开
  // 之前调 invoke("open_external_url") 但该 IPC command 不存在 → 所有点击 silent 失败
  try {
    await shellOpen(url)
    console.log("[useAdAction] opened external:", url)
  } catch (e) {
    console.warn("[useAdAction] shell.open failed, fallback window.open:", e)
    try {
      window.open(url, "_blank")
    } catch (e2) {
      console.error("[useAdAction] window.open also failed:", e2)
    }
  }
}

function goInternal(target: string): void {
  if (target.startsWith("#")) {
    // hash 路由 (#market / #earnings) · App.vue 监听 hashchange → goto
    window.location.hash = target
  } else {
    // 非 hash · 走自定义事件 (App.vue 监听 qs:navigate)
    window.dispatchEvent(
      new CustomEvent("qs:navigate", { detail: { path: target } }),
    )
  }
}

// ─────────────────────────────────────────────────────────────────
// 主入口: 任意 op_slot 点击都调用 dispatchAction
// ─────────────────────────────────────────────────────────────────
async function dispatchAction(item: OpSlotItem): Promise<void> {
  const target = item.action_target ?? ""

  // 埋点 (异步 · 不 await)
  void reportSlotEvent(item.id, "click", {
    action_type: item.action_type,
    action_target: item.action_target ?? undefined,
  })

  switch (item.action_type) {
    case "none":
      return

    case "external":
      if (!target) return
      await openExternal(target)
      return

    case "internal":
      if (!target) return
      goInternal(target)
      return

    case "modal":
      // 在客户端内弹窗显示 rich_html (不离开 app)
      openModal({
        kind: "rich_html",
        title: item.title || "活动详情",
        subtitle: item.subtitle || "",
        payload: item.rich_html || item.subtitle || "(暂无内容)",
        ctaUrl: target || null,
        ctaLabel: item.action_label || (target ? "了解更多" : null),
        slotId: item.id,
      })
      return

    case "embed_url":
      // 在客户端内 iframe 弹窗加载 URL (用户停留在 app)
      if (!target) return
      openModal({
        kind: "embed_url",
        title: item.title || "活动页",
        subtitle: item.subtitle || "",
        payload: target,
        ctaUrl: target,
        ctaLabel: "在系统浏览器打开",
        slotId: item.id,
      })
      return

    case "download":
      if (!target) return
      // download 走外部浏览器 · 触发系统下载管理器
      await openExternal(target)
      return

    case "qr":
      // 弹窗显示二维码 (扫码进群/付款)
      openModal({
        kind: "qr",
        title: item.title || "扫码查看",
        subtitle: item.subtitle || "请用微信扫一扫",
        payload: target || "",
        ctaUrl: target || null,
        ctaLabel: target ? "在浏览器打开链接" : null,
        slotId: item.id,
      })
      return

    default:
      // 未知 action_type · 兼容性兜底
      console.warn("[useAdAction] unknown action_type:", item.action_type)
      return
  }
}

// ─────────────────────────────────────────────────────────────────
// public API
// ─────────────────────────────────────────────────────────────────
export function useAdAction() {
  return {
    /** 弹窗状态 (响应式 · 单例) */
    modal,
    /** 处理任一 op_slot 点击 · 自动 dispatch */
    dispatchAction,
    /** 关闭当前弹窗 */
    closeModal,
    /** 手动打开弹窗 (一般不直接用) */
    openModal,
  }
}

export default useAdAction
