import { ref, computed, readonly } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { API as Paths, apiUrl } from "@shared"
import { useConnection } from "./useConnection"

/**
 * useHistory · 节点本机任务历史 (跨组件单例 · 切页面不丢失)
 *
 * 设计 (2026-05-21):
 *   - 永远只拉本机 worker 的任务历史 (跨设备走企业端「全局看板」)
 *   - 走 /api/v8/workers/{node_id}/history (workers.py:191)
 *   - 模块级 ref · App 生命周期保留缓存 · 切到别的页面再回来不会闪空白
 *   - 失败任务推断缺失依赖 (复用 useTasks 的 detectMissingDep)
 */
export interface HistoryItem {
  task_id: string
  workload_id: string
  task_type: string
  task_name: string
  status: "ok" | "failed" | "dispatched" | "running" | string
  attempts: number
  error: string
  elapsed_ms: number
  reward: number
  dispatched_at: string
  completed_at: string
  output_preview: string
}

const _items = ref<HistoryItem[]>([])
const _loading = ref(false)
const _error = ref<string | null>(null)
const _lastWorkerId = ref<string>("")
const _lastLoadedAt = ref<number>(0)

const STALE_MS = 30 * 1000 // 30s 内不重复打后端

function mapItem(raw: any): HistoryItem {
  const status = String(raw.status || "").toLowerCase()
  return {
    task_id: String(raw.task_id || ""),
    workload_id: String(raw.workload_id || ""),
    task_type: String(raw.task_type || ""),
    task_name: String(raw.task_name || ""),
    status: status === "done" ? "ok" : status,
    attempts: Number(raw.attempts || 0),
    error: String(raw.error || ""),
    elapsed_ms: Number(raw.elapsed_ms || 0),
    reward: Number(raw.reward || 0),
    dispatched_at: String(raw.dispatched_at || ""),
    completed_at: String(raw.completed_at || ""),
    output_preview: String(raw.output_preview || ""),
  }
}

async function load(force = false): Promise<void> {
  const { snap } = useConnection()
  const wid = snap.value.node_id
  if (!wid) {
    _error.value = "尚未连接 · 无法获取本机 worker_id"
    return
  }
  // 节点 id 没变 + 数据新鲜 → 跳过
  if (
    !force &&
    wid === _lastWorkerId.value &&
    _items.value.length > 0 &&
    Date.now() - _lastLoadedAt.value < STALE_MS
  ) {
    return
  }
  _loading.value = true
  _error.value = null
  try {
    const url = apiUrl(`${Paths.workers.workerHistory(wid)}?limit=500`)
    const body = await invoke<string>("api_get", { url })
    if (!body || !body.trim()) {
      _error.value = "服务端返回空响应 · 请点右上角刷新重试"
      return
    }
    let j: any
    try {
      j = JSON.parse(body)
    } catch {
      _error.value = "数据解析失败 · 服务端可能瞬时异常 · 请刷新重试"
      return
    }
    const items = (j?.items || []).map(mapItem)
    _items.value = items
    _lastWorkerId.value = wid
    _lastLoadedAt.value = Date.now()
  } catch (e: any) {
    _error.value = String(e?.message ?? e)
  } finally {
    _loading.value = false
  }
}

/** 后台静默刷新 · 不显示 loading */
async function refreshQuiet(): Promise<void> {
  const { snap } = useConnection()
  const wid = snap.value.node_id
  if (!wid) return
  try {
    const url = apiUrl(`${Paths.workers.workerHistory(wid)}?limit=500`)
    const body = await invoke<string>("api_get", { url })
    if (!body || !body.trim()) return
    let j: any
    try { j = JSON.parse(body) } catch { return }
    const items = (j?.items || []).map(mapItem)
    _items.value = items
    _lastWorkerId.value = wid
    _lastLoadedAt.value = Date.now()
    _error.value = null
  } catch (e) {
    // 静默忽略
  }
}

const stats = computed(() => {
  const total = _items.value.length
  const ok = _items.value.filter((i) => i.status === "ok").length
  const failed = _items.value.filter((i) => i.status === "failed").length
  const elapsedAvg = ok > 0
    ? Math.round(
        _items.value
          .filter((i) => i.status === "ok")
          .reduce((s, i) => s + (i.elapsed_ms || 0), 0) / ok,
      )
    : 0
  const totalReward = _items.value
    .filter((i) => i.status === "ok")
    .reduce((s, i) => s + (i.reward || 0), 0)
  return { total, ok, failed, elapsedAvg, totalReward }
})

export function useHistory() {
  return {
    items: readonly(_items),
    loading: readonly(_loading),
    error: readonly(_error),
    stats,
    load,
    refreshQuiet,
  }
}
