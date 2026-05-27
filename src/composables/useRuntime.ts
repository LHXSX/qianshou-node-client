/**
 * useRuntime · 客户端运行时管理 (venv + 公共镜像 · 后端动态下发清单)
 *
 * 核心特性 (2026-05-20):
 *   - 后端 /api/v8/runtime/manifest 一次返回 mirrors + tiers + smoke_test
 *   - 后台动态新增稳定源后, 客户端无需发版
 *   - install_tier(tier) 走 Rust runtime::installer (venv + pip + 自检)
 *   - 安装日志通过 listen("runtime_install_log") 实时流回
 *   - 安装完成通过 listen("runtime_install_done") 通知
 */
import { ref, reactive, computed, onBeforeUnmount } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

export interface RuntimeMirror {
  label: string
  index_url: string
  trusted_host?: string
}

export interface BinarySpec {
  name: string
  url: string
  sha256?: string
  archive?: "tar.gz" | "tar" | "zip"
  extract_to?: string
  bin_dir?: string
  executables?: string[]
}

export interface RuntimeTierSpec {
  required: boolean
  /** V8.1 · 客户端首次启动自动装此 tier */
  auto_install?: boolean
  description: string
  packages: string[]
  pip_args?: string[]
  smoke_test?: string
  software: string[]
  task_types: string[]
  skills?: string[]
  /** 2026-05-24 · 静态二进制 tier (ffmpeg) */
  binaries?: BinarySpec[]
  /** 2026-05-24 · 依赖的其它 tier · UI 提示先装 */
  depends_on?: string[]
  /** 2026-05-24 · 系统命令依赖 (如 blender) · 安装时 which 探测 · 缺则失败 */
  system_commands?: string[]
  /** 2026-05-24 · 探测失败时显示的安装指引 · key 为 macos/linux/windows */
  install_hint?: Partial<Record<"macos" | "linux" | "windows", string>>
}

export interface RuntimeManifest {
  ok: boolean
  platform: string
  schema_version: string
  install_mode: string
  python?: { min_version?: string; preferred_versions?: string[] }
  mirrors: RuntimeMirror[]
  tiers: Record<string, RuntimeTierSpec>
}

export interface InstalledTier {
  ok: boolean
  python: string
  packages: string[]
  software: string[]
  mirror_label: string
  installed_at: string
  last_message: string
  binaries: Record<string, string>
}

export interface InstalledMeta {
  schema_version: string
  install_mode: string
  platform: string
  host_python?: string
  tiers: Record<string, InstalledTier>
}

export interface HostPythonInfo {
  path: string
  version: string
  ok: boolean
  message: string
}

export interface InstallLog {
  job_id: string
  tier: string
  line: string
  is_stderr: boolean
}

export interface InstallDone {
  job_id: string
  tier: string
  success: boolean
  error: string
  used_mirror: string
  venv_python: string
}

// ───────────── 全局 (单例) 状态 ─────────────
const manifest = ref<RuntimeManifest | null>(null)
const installed = ref<InstalledMeta>({
  schema_version: "",
  install_mode: "",
  platform: "",
  tiers: {},
})
const hostPython = ref<HostPythonInfo | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)

// 每个 tier 的当前 job id (供 UI 显示进度)
const tierJob = reactive<Record<string, string>>({})
// 每个 tier 的最后一次 job id (包括已结束的 · 供失败后查看日志)
const lastJobByTier = reactive<Record<string, string>>({})
// 每个 job 的日志 + 状态
const jobLogs = reactive<Record<string, { tier: string; lines: { line: string; err: boolean }[]; running: boolean; ok?: boolean; mirror?: string; error_msg?: string }>>({})

let _initOnce = false
let _unlistenLog: UnlistenFn | null = null
let _unlistenDone: UnlistenFn | null = null

async function ensureListeners() {
  if (_unlistenLog) return
  _unlistenLog = await listen<InstallLog>("runtime_install_log", (e) => {
    const j = jobLogs[e.payload.job_id]
    if (j) {
      j.lines.push({ line: e.payload.line, err: e.payload.is_stderr })
    }
  })
  _unlistenDone = await listen<InstallDone>("runtime_install_done", async (e) => {
    const j = jobLogs[e.payload.job_id]
    if (j) {
      j.running = false
      j.ok = e.payload.success
      j.mirror = e.payload.used_mirror
      // 失败时将后端 error 追加到日志头 · 避免 UI 看不到原因
      if (!e.payload.success && e.payload.error) {
        j.error_msg = e.payload.error
        j.lines.unshift({ line: `✗ ${e.payload.error}`, err: true })
      }
    }
    // 任一 tier 装完 · 刷一下 installed
    await refreshInstalled()
    // 释放 tier→job 绑定 (UI 知道按钮恢复) · 但保留 lastJobByTier 供查看日志
    Object.keys(tierJob).forEach((tier) => {
      if (tierJob[tier] === e.payload.job_id) {
        lastJobByTier[tier] = e.payload.job_id
        delete tierJob[tier]
      }
    })
  })
}

async function refreshManifest(): Promise<void> {
  loading.value = true
  error.value = null
  try {
    manifest.value = await invoke<RuntimeManifest>("runtime_fetch_manifest")
  } catch (e: any) {
    error.value = String(e?.message ?? e)
  } finally {
    loading.value = false
  }
}

async function refreshInstalled(): Promise<void> {
  try {
    installed.value = await invoke<InstalledMeta>("runtime_get_installed")
  } catch (e) {
    // 忽略 (空 installed.json)
  }
}

async function refreshHostPython(): Promise<void> {
  try {
    hostPython.value = await invoke<HostPythonInfo>("runtime_host_python")
  } catch (e: any) {
    hostPython.value = {
      path: "",
      version: "",
      ok: false,
      message: String(e?.message ?? e),
    }
  }
}

async function installTier(tier: string): Promise<void> {
  await ensureListeners()
  try {
    const jobId: string = await invoke("runtime_install_tier", { tier })
    tierJob[tier] = jobId
    lastJobByTier[tier] = jobId
    jobLogs[jobId] = { tier, lines: [], running: true }
  } catch (e: any) {
    error.value = String(e?.message ?? e)
  }
}

async function uninstallTier(tier: string): Promise<void> {
  try {
    await invoke("runtime_uninstall_tier", { tier })
    await refreshInstalled()
  } catch (e: any) {
    error.value = String(e?.message ?? e)
  }
}

async function recheckTier(tier: string): Promise<InstalledTier> {
  return await invoke<InstalledTier>("runtime_recheck", { tier })
}

function statusOf(tier: string): "ready" | "installing" | "missing" | "failed" {
  if (tierJob[tier]) return "installing"
  const t = installed.value.tiers[tier]
  if (!t) return "missing"
  if (t.ok) return "ready"
  return "failed"
}

function logsForTier(tier: string) {
  // 优先返运行中的 job · 其次返最后一次已结束的 job (便于看失败原因)
  const jid = tierJob[tier] || lastJobByTier[tier]
  if (!jid) return null
  return jobLogs[jid]
}

const stats = computed(() => {
  const tiers = manifest.value?.tiers ?? {}
  const total = Object.keys(tiers).length
  const ready = Object.keys(tiers).filter((t) => statusOf(t) === "ready").length
  const required = Object.keys(tiers).filter((t) => tiers[t].required)
  const requiredReady = required.filter((t) => statusOf(t) === "ready").length
  return { total, ready, requiredTotal: required.length, requiredReady }
})

export function useRuntime() {
  if (!_initOnce) {
    _initOnce = true
    // 自动初始化一次
    refreshManifest().catch(() => {})
    refreshInstalled().catch(() => {})
    refreshHostPython().catch(() => {})
    ensureListeners().catch(() => {})
  }
  onBeforeUnmount(() => {
    // 模块单例 · 不真的卸载监听 · 避免别的页面失效
  })
  return {
    manifest,
    installed,
    hostPython,
    loading,
    error,
    stats,
    tierJob,
    jobLogs,
    refreshManifest,
    refreshInstalled,
    refreshHostPython,
    installTier,
    uninstallTier,
    recheckTier,
    statusOf,
    logsForTier,
  }
}
