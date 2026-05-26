import { ref, computed, readonly } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

/**
 * 全局单例 bundle 状态：
 * - 启动后 load() 拉清单 + 跑检测
 * - 任意页面 useBundles() 共享读取
 * - 提供：installed_bundles、missing_count、task_type 覆盖率
 * - 提供：根据 task_type 反查需要的 bundle (用于任务卡片 chip)
 * - 连接就绪后自动重试加载
 */

import { API as Paths, apiUrl } from "@shared"

export interface InstallSource {
  label: string
  cmd: string
  needs_password?: boolean   // 会弹出 macOS 系统密码框（支持 Touch ID）
  manual?: boolean           // 仅提示，需用户去终端手动跑
}

export interface DepSpec {
  name: string
  check: string
  install: string | null            // 兼容字段（单源）
  install_sources?: InstallSource[] // 多源 fallback（清华/阿里/默认/兜底）
  required: boolean
  note?: string
}
export interface DepStatus {
  name: string
  installed: boolean
  version: string
  error: string
}
export interface Bundle {
  id: string
  name: string
  icon: string
  desc: string
  task_types: string[]
  deps: DepSpec[]
  platform: string
  detection?: DepStatus[]
}

const bundles = ref<Bundle[]>([])
const loaded = ref(false)
const detecting = ref(false)
const error = ref<string | null>(null)

let _loadPromise: Promise<void> | null = null
let _unlistenConn: UnlistenFn | null = null
let _initialized = false

async function load(force = false) {
  if (loaded.value && !force) return
  if (_loadPromise && !force) return _loadPromise
  _loadPromise = (async () => {
    try {
      const body = await invoke<string>("api_get", { url: apiUrl(Paths.bundles.list()) })
      if (!body || !body.trim()) throw new Error("服务端返回空响应")
      let j: any
      try { j = JSON.parse(body) } catch { throw new Error("数据解析失败") }
      bundles.value = (j.bundles || []) as Bundle[]
      loaded.value = true
      error.value = null
      await detectAll()
    } catch (e: any) {
      error.value = String(e?.message || e)
      console.warn("[useBundles] bundles 拉取失败", e)
    } finally {
      _loadPromise = null
    }
  })()
  return _loadPromise
}

async function detectAll() {
  if (detecting.value) return
  detecting.value = true
  try {
    await Promise.all(
      bundles.value.map(async (b) => {
        try {
          const status: DepStatus[] = await invoke("detect_deps", { deps: b.deps })
          b.detection = status
        } catch (e) {
          console.error("[useBundles] detect_deps failed for", b.id, e)
        }
      })
    )
  } finally {
    detecting.value = false
  }
}

function bundleStatus(b: Bundle): "ready" | "partial" | "missing" | "unknown" {
  if (!b.detection || b.detection.length === 0) return "unknown"
  const required = b.deps.filter((d) => d.required)
  const allReqOk = required.every((rd) => {
    const s = b.detection!.find((x) => x.name === rd.name)
    return s?.installed
  })
  if (allReqOk) return "ready"
  return b.detection.some((d) => d.installed) ? "partial" : "missing"
}

const stats = computed(() => {
  const total = bundles.value.length
  const ready = bundles.value.filter((b) => bundleStatus(b) === "ready").length
  const missing = bundles.value.filter((b) => bundleStatus(b) !== "ready").length
  const ttReady = bundles.value
    .filter((b) => bundleStatus(b) === "ready")
    .reduce((s, b) => s + b.task_types.length, 0)
  const ttTotal = bundles.value.reduce((s, b) => s + b.task_types.length, 0)
  return { total, ready, missing, ttReady, ttTotal }
})

/** 反查：给定 task_type，返回该任务所需的 bundle（可能 0 或 1 个）。 */
function bundleForTaskType(taskType: string): Bundle | undefined {
  if (!taskType) return undefined
  return bundles.value.find((b) => b.task_types.includes(taskType))
}

/** 反查：task_type 是否当前节点能跑（对应 bundle ready）。 */
function canRunTaskType(taskType: string): boolean {
  const b = bundleForTaskType(taskType)
  if (!b) return true // 未知类型默认能跑（不阻拦）
  return bundleStatus(b) === "ready"
}

/** 给定 task_type，返回缺失的依赖列表（用于任务失败提示）。 */
function missingDepsForTaskType(taskType: string): DepStatus[] {
  const b = bundleForTaskType(taskType)
  if (!b || !b.detection) return []
  return b.detection.filter((d) => !d.installed)
}

export function useBundles() {
  if (!_initialized) {
    _initialized = true
    listen("connection_state_changed", (e: any) => {
      const conn = e.payload?.connection_state
      if (conn === "registered" && !loaded.value) {
        load().catch(() => {})
      }
    }).then((unlisten) => {
      _unlistenConn = unlisten
    })
  }

  return {
    bundles: readonly(bundles),
    stats,
    loaded: readonly(loaded),
    detecting: readonly(detecting),
    error: readonly(error),
    load,
    detectAll,
    bundleStatus,
    bundleForTaskType,
    canRunTaskType,
    missingDepsForTaskType,
  }
}
