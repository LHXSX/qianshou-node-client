import { ref } from "vue"

/** Layout 3.0 — Sidebar 当前页 ref。单例共享。 */
export type PageId =
  | "dashboard"
  | "earnings"
  | "throttle"
  | "device"
  | "market"
  | "history"
  | "toolbox"
  | "ai-capability"
  | "capabilities"
  | "settings"
  | "help"

const _page = ref<PageId>("dashboard")

export function useNav() {
  function goto(p: PageId) {
    _page.value = p
  }
  return { page: _page, goto }
}
