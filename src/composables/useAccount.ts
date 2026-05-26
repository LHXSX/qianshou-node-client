import { ref, onMounted, onBeforeUnmount } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

export interface AccountSummary {
  id: number
  username: string
  email: string
  balance: number
  total_earnings: number
  completed_tasks: number
  status: string
}

export interface HistoryItem {
  task_id: string
  node_id: string
  owner_id: number
  cmd?: string
  reward: number
  assigned_at?: string
  completed_at?: string
  status: string
  elapsed_ms: number
  output: string
  error: string
}

export function useAccount() {
  const account = ref<AccountSummary | null>(null)
  const history = ref<HistoryItem[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)

  let unlistenDone: UnlistenFn | null = null
  let unlistenState: UnlistenFn | null = null
  let pollTimer: ReturnType<typeof setInterval> | null = null

  async function refresh() {
    loading.value = true
    error.value = null
    try {
      const [a, h] = await Promise.all([
        invoke<AccountSummary>("get_my_account"),
        invoke<{ total: number; items: HistoryItem[] }>("get_my_history", { limit: 20 }),
      ])
      account.value = a
      history.value = h.items
    } catch (e: any) {
      error.value = String(e)
    } finally {
      loading.value = false
    }
  }

  onMounted(async () => {
    // 任务完成事件 → 立即刷新（看见余额跳动）
    unlistenDone = await listen("task_completed", () => {
      refresh().catch(() => {})
    })
    // 状态变成 Registered 才有意义查 account；Disconnected 时跳过
    unlistenState = await listen("connection_state_changed", (_e: any) => {
      const conn = (_e as any).payload?.connection_state
      if (conn === "registered" && !account.value) {
        refresh().catch(() => {})
      }
    })
    // 轮询兜底（每 30s 再拉一次防漏 event）
    pollTimer = setInterval(() => {
      refresh().catch(() => {})
    }, 30000)
    // 初始拉一次（可能此时还没 token，会 fail；下次 state→registered 再拉）
    refresh().catch(() => {})
  })

  onBeforeUnmount(() => {
    if (unlistenDone) unlistenDone()
    if (unlistenState) unlistenState()
    if (pollTimer) clearInterval(pollTimer)
  })

  return { account, history, loading, error, refresh }
}
