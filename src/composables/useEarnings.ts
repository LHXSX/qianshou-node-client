import { ref, onMounted, onBeforeUnmount } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

export interface EarningPoint {
  date: string
  earnings: number
  count: number
}

export interface EarningSeries {
  days: number
  series: EarningPoint[]
}

export function useEarnings(daysDefault = 7) {
  const series = ref<EarningPoint[]>([])
  const days = ref(daysDefault)
  const loading = ref(false)
  const error = ref<string | null>(null)

  let unlistenDone: UnlistenFn | null = null

  async function refresh(d?: number) {
    if (d) days.value = d
    loading.value = true
    error.value = null
    try {
      const r = await invoke<EarningSeries>("get_my_earnings", { days: days.value })
      series.value = r.series
    } catch (e: any) {
      error.value = String(e)
    } finally {
      loading.value = false
    }
  }

  onMounted(async () => {
    unlistenDone = await listen("task_completed", () => {
      refresh().catch(() => {})
    })
    refresh().catch(() => {})
  })

  onBeforeUnmount(() => {
    if (unlistenDone) unlistenDone()
  })

  return { series, days, loading, error, refresh }
}
