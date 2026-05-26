/**
 * NCE 节点能力评估 · 实时拉取节点的 hw_tier / rep_main / 子分 / 改进建议
 *
 * 数据源: /api/v8/my/workers/{node_id}/nce (后端 my_nce.py)
 * 触发:
 *   - WS 状态变 registered → 立即拉
 *   - task_completed → 60s 节流后重拉
 *   - 每 5min 兜底轮询
 */
import { ref, onMounted, onBeforeUnmount, computed } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { PRIMARY_DOMAIN } from "@shared"
import { useConnection } from "./useConnection"

const API_BASE = `${PRIMARY_DOMAIN}/api/v8`

export interface NceSubScores {
  rep_stability: number
  rep_correctness: number
  rep_speed: number
  rep_resource: number
}

export interface NceProfile {
  worker_id: string
  name: string
  status: string
  compatibility_mode: boolean
  hardware: {
    hw_tier: string // S / A / B / C / D
    hw_score: number
    sub_scores: Record<string, number>
  }
  reputation: {
    rep_main: number
    rep_stability: number
    rep_correctness: number
    rep_speed: number
    rep_resource: number
  }
  runtime: {
    load: number
    active_shards: number
    onboarding_status: string
  }
  suggestions: Array<{
    category: string
    level: "info" | "praise" | "warn" | "err"
    message: string
    advice: string
  }>
}

// 档位 → 月预估收入 (粗略基线 · 真实数据待后端 api 给)
const INCOME_TABLE: Record<string, Record<string, number>> = {
  S: { 钻石: 10000, 黄金: 6000, 白银: 3500, 青铜: 1800, 预警: 600 },
  A: { 钻石: 5000, 黄金: 3000, 白银: 1800, 青铜: 900, 预警: 300 },
  B: { 钻石: 2000, 黄金: 1200, 白银: 700, 青铜: 350, 预警: 120 },
  C: { 钻石: 800, 黄金: 500, 白银: 280, 青铜: 140, 预警: 50 },
  D: { 钻石: 300, 黄金: 180, 白银: 100, 青铜: 50, 预警: 20 },
}

export function tierLabel(repMain: number | null | undefined): { name: string; emoji: string } {
  // 2026-05-26 fix · null / 0 / 无数据 · 显示 "评估中" 而非误报 "封禁"
  // 之前 rep_main ?? 0 → tierLabel(0) → 0<30 → 封禁 · 让新节点用户惊吓
  if (repMain === null || repMain === undefined || repMain <= 0) {
    return { name: "评估中", emoji: "⏳" }
  }
  if (repMain >= 92) return { name: "钻石", emoji: "💎" }
  if (repMain >= 77) return { name: "黄金", emoji: "🥇" }
  if (repMain >= 62) return { name: "白银", emoji: "🥈" }
  if (repMain >= 47) return { name: "青铜", emoji: "🥉" }
  if (repMain >= 30) return { name: "预警", emoji: "⚠️" }
  return { name: "封禁", emoji: "🚫" }
}

export function tierEmoji(hwTier: string): string {
  return { S: "🚀", A: "💪", B: "⚡", C: "📦", D: "🌱" }[hwTier] || "⚡"
}

export function estimateMonthlyIncome(hwTier: string, repMain: number): number {
  const repName = tierLabel(repMain).name
  return INCOME_TABLE[hwTier]?.[repName] ?? 0
}

const _profile = ref<NceProfile | null>(null)
const _loading = ref(false)
const _error = ref<string | null>(null)
let _lastFetch = 0

export function useNceProfile() {
  const { snap } = useConnection()
  let unlistenDone: UnlistenFn | null = null
  let unlistenState: UnlistenFn | null = null
  let pollTimer: ReturnType<typeof setInterval> | null = null

  async function refresh(force = false) {
    const nodeId = snap.value.node_id
    if (!nodeId || snap.value.connection_state !== "registered") return
    // 节流 60s · 防 task_completed 频发
    const now = Date.now()
    if (!force && now - _lastFetch < 60_000) return
    _lastFetch = now
    _loading.value = true
    try {
      const url = `${API_BASE}/my/workers/${nodeId}/nce`
      const body = await invoke<string>("api_get", { url })
      const parsed = JSON.parse(body)
      if (parsed && parsed.ok !== false) {
        _profile.value = parsed as NceProfile
        _error.value = null
      }
    } catch (e: any) {
      _error.value = String(e)
    } finally {
      _loading.value = false
    }
  }

  const hwTier = computed(() => _profile.value?.hardware.hw_tier ?? "—")
  const hwScore = computed(() => _profile.value?.hardware.hw_score ?? 0)
  // 2026-05-26 fix · 用 null 区分 "未拉到" vs "真 0 分" · 防误显示封禁
  const repMain = computed<number | null>(() => _profile.value?.reputation.rep_main ?? null)
  const tier = computed(() => tierLabel(repMain.value))
  const income = computed(() => estimateMonthlyIncome(hwTier.value, repMain.value ?? 60))
  const suggestions = computed(() => _profile.value?.suggestions ?? [])

  onMounted(async () => {
    unlistenDone = await listen("task_completed", () => {
      refresh(false).catch(() => {})
    })
    unlistenState = await listen("connection_state_changed", (e: any) => {
      const c = e?.payload?.connection_state
      if (c === "registered") refresh(true).catch(() => {})
    })
    pollTimer = setInterval(() => refresh(false).catch(() => {}), 5 * 60 * 1000)
    refresh(true).catch(() => {})
  })

  onBeforeUnmount(() => {
    unlistenDone?.()
    unlistenState?.()
    if (pollTimer) clearInterval(pollTimer)
  })

  return {
    profile: _profile,
    loading: _loading,
    error: _error,
    hwTier,
    hwScore,
    repMain,
    tier,
    income,
    suggestions,
    refresh,
  }
}
