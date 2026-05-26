import { ref, onMounted, onBeforeUnmount } from "vue"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

/**
 * Layout 3.0 — 任务生命周期 4 阶段：queued / running / verifying / done。
 * 后端 `task_phase` 事件携带：{ task_id, phase, task_type, runtime, cmd, reward, ... }
 * 前端订阅后维护 4 个 reactive 列表，Kanban 列实时渲染。
 */
export interface TaskPhasePayload {
  task_id: string
  workload_id?: string
  phase: "queued" | "running" | "verifying" | "done"
  task_type: string
  runtime: string
  cmd: string
  reward: number
  timeout_s: number
  started_at_ms: number
  ok?: boolean
  elapsed_ms?: number
  error?: string
  /** 前端推断：失败原因是缺哪个依赖（用于 UI 显示「⚠ 缺 Pillow [安装]」） */
  missing_dep?: string
  missing_install_cmd?: string
  // 2026-05-21 · 后端补字段 (broker.dispatch_assignments 填)
  workload_name?: string
  requester_name?: string
  requester_avatar?: string
  created_at_ms?: number
  index?: number
  total?: number
}

/**
 * 扫描 stderr / error 文本，识别常见依赖缺失模式。
 * 返回 [缺失名, 推荐安装命令] —— 找不到则返回 null。
 */
export function detectMissingDep(error: string | undefined): { name: string; install: string } | null {
  if (!error) return null
  const e = error.toString()

  // Python: ModuleNotFoundError / ImportError
  const m1 = e.match(/(?:No module named|cannot import name)\s+['"]?(\w+)['"]?/i)
  if (m1) {
    const mod = m1[1]
    const map: Record<string, string> = {
      PIL: "Pillow",
      pil: "Pillow",
      cv2: "opencv-python",
      yaml: "PyYAML",
      bs4: "beautifulsoup4",
    }
    const pkg = map[mod] || mod
    return { name: pkg, install: `pip3 install --user ${pkg}` }
  }

  // 系统命令缺失
  const cmdPatterns: { rx: RegExp; name: string; install: string }[] = [
    { rx: /tesseract:\s*command not found|'tesseract' is not recognized/i, name: "tesseract", install: "brew install tesseract tesseract-lang" },
    { rx: /ffmpeg:\s*command not found|'ffmpeg' is not recognized/i, name: "ffmpeg", install: "brew install ffmpeg" },
    { rx: /python3?:\s*command not found/i, name: "python3", install: "brew install python3" },
    { rx: /node:\s*command not found/i, name: "node", install: "brew install node" },
    { rx: /onnxruntime/i, name: "onnxruntime", install: "pip3 install --user onnxruntime" },
  ]
  for (const p of cmdPatterns) {
    if (p.rx.test(e)) return { name: p.name, install: p.install }
  }

  return null
}

const queued = ref<TaskPhasePayload[]>([])
const running = ref<TaskPhasePayload[]>([])
const verifying = ref<TaskPhasePayload[]>([])
const done = ref<TaskPhasePayload[]>([])

const DONE_MAX = 30
let _initialized = false
let _unlisten: UnlistenFn | null = null

function removeFrom(list: typeof queued, task_id: string) {
  const i = list.value.findIndex((t) => t.task_id === task_id)
  if (i >= 0) list.value.splice(i, 1)
}

function ingest(t: TaskPhasePayload) {
  // 失败任务推断缺失依赖
  if (t.phase === "done" && t.ok === false && t.error && !t.missing_dep) {
    const detected = detectMissingDep(t.error)
    if (detected) {
      t.missing_dep = detected.name
      t.missing_install_cmd = detected.install
    }
  }
  // 从所有列表先剔除同 task_id
  removeFrom(queued, t.task_id)
  removeFrom(running, t.task_id)
  removeFrom(verifying, t.task_id)
  // 按 phase 分发
  switch (t.phase) {
    case "queued":
      queued.value.unshift(t)
      break
    case "running":
      running.value.unshift(t)
      break
    case "verifying":
      verifying.value.unshift(t)
      break
    case "done":
      // done 也从 done 列表去重再 unshift
      removeFrom(done, t.task_id)
      done.value.unshift(t)
      if (done.value.length > DONE_MAX) done.value.length = DONE_MAX
      break
  }
}

export function useTasks() {
  onMounted(async () => {
    if (_initialized) return
    _initialized = true
    _unlisten = await listen<TaskPhasePayload>("task_phase", (e) => {
      ingest(e.payload)
    })
  })

  onBeforeUnmount(() => {
    // 单例：整 app 生命周期不释放
  })

  return {
    queued,
    running,
    verifying,
    done,
  }
}

export function cleanupTasks() {
  if (_unlisten) {
    _unlisten()
    _unlisten = null
  }
  _initialized = false
}
