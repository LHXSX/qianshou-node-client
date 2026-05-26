<script setup lang="ts">
import { computed, ref, onMounted, onBeforeUnmount } from "vue"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { useConnection } from "../composables/useConnection"
import { useAccount } from "../composables/useAccount"
import { useUpdater } from "../composables/useUpdater"
import { useEarnings } from "../composables/useEarnings"
import DeviceCard from "../components/DeviceCard.vue"
import EarningsChart from "../components/EarningsChart.vue"
import SettingsPanel from "../components/SettingsPanel.vue"
import AgentCockpit from "../pages/AgentCockpit.vue"
import { useNav } from "../composables/useNav"

const settingsOpen = ref(false)

const { snap, authLogout, setThrottle } = useConnection()
const { account, history } = useAccount()
const { updateInfo, installing, dismissed, install: installUpdate, dismiss: dismissUpdate } = useUpdater()
const { series: earningsSeries } = useEarnings(7)
const { page } = useNav()

const isPaused = computed(() => snap.value.mode === "paused")
const isThrottled = computed(() => snap.value.mode === "throttled")
const throttlePct = computed({
  get: () => snap.value.throttle_pct ?? 100,
  set: (v: number) => {
    setThrottle(v).catch((e) => console.error("set_throttle failed:", e))
  },
})

const throttleLabel = computed(() => {
  const p = throttlePct.value
  if (p === 0) return "已暂停"
  if (p === 100) return "全速贡献"
  return `${p}% 限速`
})

function fmtTime(s?: string): string {
  if (!s) return ""
  try {
    return new Date(s).toLocaleString("zh-CN", { hour12: false })
  } catch {
    return s
  }
}

// 任务运行状态 + 最近完成的任务（toast）
interface TaskAssign {
  task_id: string
  task_type: string
  runner: string
  args: any
  timeout_s: number
  reward: number
}
interface TaskResult {
  task_id: string
  ok: boolean
  elapsed_ms: number
  output: string
  error?: string
  exit_code?: number | null
}
const currentTask = ref<TaskAssign | null>(null)
const lastResult = ref<TaskResult | null>(null)
const showResultToast = ref(false)
let toastTimer: any = null

let unlistenAssign: UnlistenFn | null = null
let unlistenDone: UnlistenFn | null = null

onMounted(async () => {
  unlistenAssign = await listen<TaskAssign>("task_assigned", (e) => {
    currentTask.value = e.payload
  })
  unlistenDone = await listen<TaskResult>("task_completed", (e) => {
    currentTask.value = null
    lastResult.value = e.payload
    showResultToast.value = true
    if (toastTimer) clearTimeout(toastTimer)
    toastTimer = setTimeout(() => {
      showResultToast.value = false
    }, 6000)
  })
})

onBeforeUnmount(() => {
  if (unlistenAssign) unlistenAssign()
  if (unlistenDone) unlistenDone()
  if (toastTimer) clearTimeout(toastTimer)
})

const dotColor = computed(() => {
  switch (snap.value.connection_state) {
    case "registered":
      return "var(--c-ok)"
    case "connecting":
    case "authenticating":
    case "reconnecting":
      return "var(--c-warn)"
    default:
      return "var(--c-mute)"
  }
})

const loggingOut = ref(false)

async function onLogout() {
  if (loggingOut.value) return
  loggingOut.value = true
  try {
    console.log("auth_logout invoke...")
    await authLogout()
    console.log("auth_logout done")
  } catch (e) {
    console.error("auth_logout failed:", e)
    alert(`退出失败: ${e}`)
  } finally {
    loggingOut.value = false
  }
}
</script>

<template>
  <div class="dashboard">
    <!-- ── Header ─────────────────────────────────────── -->
    <header class="topbar">
      <div class="brand">
        <h1>千手</h1>
        <span class="badge">v{{ snap.client_version }}</span>
        <span
          class="conn-pill"
          :class="snap.connection_state"
          :title="snap.last_error || snap.state_label"
        >
          <span class="conn-dot" :style="{ background: dotColor }"></span>
          {{ snap.state_label }}
          <span v-if="snap.node_id && snap.connection_state === 'registered'" class="conn-node">
            · {{ snap.node_id }}
          </span>
        </span>
      </div>
      <div class="user-block" v-if="snap.user">
        <div class="user-info">
          <div class="username">{{ snap.user.username }}</div>
          <div class="email">{{ snap.user.email || `#${snap.owner_id}` }}</div>
        </div>
        <button class="btn icon-btn" @click="settingsOpen = true" title="设置">⚙</button>
        <button class="btn ghost" @click="onLogout">退出</button>
      </div>
    </header>

    <!-- ── 更新 banner（全宽） ───────────────────────── -->
    <transition name="slide">
      <section
        v-if="updateInfo?.available && !dismissed"
        class="update-banner"
      >
        <div class="ub-icon">↑</div>
        <div class="ub-body">
          <div class="ub-title">发现新版本 v{{ updateInfo.version }}</div>
          <div class="ub-notes" v-if="updateInfo.notes">{{ updateInfo.notes }}</div>
        </div>
        <div class="ub-actions">
          <button class="btn ghost" @click="dismissUpdate">稍后</button>
          <button class="btn primary" :disabled="installing" @click="installUpdate">
            {{ installing ? "安装中…" : "立即更新" }}
          </button>
        </div>
      </section>
    </transition>

    <!-- ── 重连错误 ──────────────────────────────────── -->
    <section
      v-if="snap.last_error && snap.connection_state !== 'registered'"
      class="alert-banner"
    >
      ⚠ {{ snap.state_label }}：{{ snap.last_error }}
    </section>

    <!-- ── 主网格 ────────────────────────────────────── -->
    <main class="grid" v-if="snap.user">
      <!-- 余额 hero（占 2 列） -->
      <section class="card balance-card" v-if="account">
        <div class="bc-row">
          <div class="bc-label">余额</div>
          <div class="bc-pill" v-if="account.completed_tasks">
            已完成 {{ account.completed_tasks }} 个任务
          </div>
        </div>
        <div class="bc-value">
          {{ account.balance.toFixed(2) }}
          <span class="bc-unit">EDG</span>
        </div>
        <div class="bc-sub">
          累计收益 <strong>+{{ account.total_earnings.toFixed(2) }}</strong> EDG
        </div>
      </section>

      <!-- 算力滑杆（占 1 列） -->
      <section
        class="card throttle-card"
        :class="{ paused: isPaused, throttled: isThrottled }"
      >
        <div class="th-head">
          <div class="th-title">算力贡献</div>
          <span class="th-label">{{ throttleLabel }}</span>
        </div>
        <input
          type="range"
          min="0"
          max="100"
          step="5"
          class="th-slider"
          :value="throttlePct"
          :style="{ '--pct': throttlePct + '%' }"
          @input="(e: any) => throttlePct = Number((e.target as HTMLInputElement).value)"
        />
        <div class="th-presets">
          <button class="th-preset" :class="{ active: throttlePct === 0 }" @click="throttlePct = 0">暂停</button>
          <button class="th-preset" :class="{ active: throttlePct === 50 }" @click="throttlePct = 50">50%</button>
          <button class="th-preset" :class="{ active: throttlePct === 100 }" @click="throttlePct = 100">全速</button>
        </div>
      </section>

      <!-- 当前任务（占 3 列 / 全宽，仅有任务时显示） -->
      <section class="card task-card span-3" v-if="currentTask">
        <div class="task-row">
          <span class="task-pulse" />
          <strong>正在执行</strong>
          <span class="task-id">#{{ currentTask.task_id.slice(0, 8) }}</span>
          <span class="task-spacer"></span>
          <span class="task-reward">+{{ currentTask.reward }} EDG</span>
        </div>
        <div class="task-cmd">
          <code>{{ currentTask.args?.cmd || "(无 cmd)" }}</code>
        </div>
        <div class="task-meta">
          <span>类型 <code>{{ currentTask.task_type }}</code></span>
          <span>超时 {{ currentTask.timeout_s }}s</span>
        </div>
      </section>

      <!-- 收益曲线（占 3 列 / 全宽） -->
      <div class="span-3">
        <EarningsChart v-if="account" :series="earningsSeries" />
      </div>

      <!-- 本机（占 1 列） -->
      <div class="span-1">
        <DeviceCard />
      </div>

      <!-- 最近任务（占 2 列） -->
      <section class="card history-card span-2">
        <div class="history-head">
          <h3>最近任务</h3>
          <span class="muted">{{ history.length ? `${history.length} 条` : '' }}</span>
        </div>
        <div class="history-empty" v-if="history.length === 0">
          <div class="he-title">节点已就绪，等待任务派发</div>
          <div class="he-hint">
            开发期间可用 <code>POST /api/v8/workloads</code> 提交任务（节点 ID: <code>{{ snap.node_id }}</code>），引擎会自动派发到符合条件的在线节点。
          </div>
        </div>
        <ul class="history-list" v-else>
          <li v-for="h in history.slice(0, 8)" :key="h.task_id" class="history-item">
            <div class="hi-row">
              <span class="hi-icon" :class="h.status">{{ h.status === 'ok' ? '✓' : '✗' }}</span>
              <code class="hi-cmd">{{ h.cmd || '(no cmd)' }}</code>
              <span class="hi-reward" v-if="h.status === 'ok'">+{{ h.reward.toFixed(2) }}</span>
            </div>
            <div class="hi-meta">
              <span>{{ fmtTime(h.completed_at) }}</span>
              <span>{{ h.elapsed_ms }}ms</span>
              <span class="hi-id">#{{ h.task_id.slice(0, 8) }}</span>
            </div>
          </li>
        </ul>
      </section>
    </main>

    <!-- M3.5.5 设置 + 诊断 modal -->
    <SettingsPanel v-if="settingsOpen" @close="settingsOpen = false" />

    <!-- 任务完成 Toast -->
    <transition name="toast">
      <div v-if="showResultToast && lastResult" class="toast" :class="{ ok: lastResult.ok, fail: !lastResult.ok }">
        <div class="toast-row">
          <span class="toast-icon">{{ lastResult.ok ? "✓" : "✗" }}</span>
          <strong>任务 #{{ lastResult.task_id.slice(0, 8) }}</strong>
          <span class="toast-time">{{ lastResult.elapsed_ms }}ms</span>
        </div>
        <div class="toast-body">
          {{ lastResult.ok ? "执行成功" : (lastResult.error || "执行失败") }}
        </div>
      </div>
    </transition>
  </div>
</template>

<style scoped>
/* ═══════════════════════════════════════════════════════
   Dashboard 2.0 ─ 紧凑顶栏 + 3 列网格
   ═══════════════════════════════════════════════════════ */
.dashboard {
  display: flex;
  flex-direction: column;
  min-height: 100vh;
  padding: 20px 28px 32px;
  gap: 14px;
  max-width: 1140px;
  margin: 0 auto;
  box-sizing: border-box;
}

/* ── Topbar ───────────────────────────────────────────── */
.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 0 14px;
  border-bottom: 1px solid var(--c-border);
}
.brand {
  display: flex;
  align-items: center;
  gap: 12px;
}
.brand h1 {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
  letter-spacing: -0.01em;
  background: linear-gradient(135deg, var(--c-accent), var(--c-accent-2));
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
}
.badge {
  font-size: 14px;
  color: var(--c-mute);
  font-family: ui-monospace, SFMono-Regular, monospace;
  padding: 2px 6px;
  border: 1px solid #222;
  border-radius: 4px;
}
.conn-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  background: rgba(48, 209, 88, 0.08);
  border: 1px solid rgba(48, 209, 88, 0.3);
  border-radius: 999px;
  font-size: 14.5px;
  color: var(--c-ok);
  margin-left: 6px;
}
.conn-pill.reconnecting,
.conn-pill.connecting,
.conn-pill.authenticating {
  background: rgba(255, 214, 10, 0.08);
  border-color: rgba(255, 214, 10, 0.3);
  color: var(--c-warn);
}
.conn-pill.disconnected {
  background: rgba(255, 69, 58, 0.08);
  border-color: rgba(255, 69, 58, 0.3);
  color: var(--c-err);
}
.conn-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
}
.conn-node {
  font-family: ui-monospace, SFMono-Regular, monospace;
  color: var(--c-mute);
}
.user-block {
  display: flex;
  align-items: center;
  gap: 12px;
}
.user-info {
  text-align: right;
  line-height: 1.25;
}
.username {
  font-size: 14.5px;
  font-weight: 500;
}
.email {
  color: var(--c-mute);
  font-size: 14.5px;
}

/* ── 通用按钮 ─────────────────────────────────────────── */
.btn {
  padding: 6px 12px;
  border-radius: 7px;
  font-size: 13.5px;
  font-weight: 500;
  border: none;
  cursor: pointer;
  transition: all 0.15s;
}
.btn.ghost {
  background: transparent;
  border: 1px solid var(--c-border-strong);
  color: var(--c-mute);
}
.btn.ghost:hover {
  color: var(--c-fg);
  border-color: var(--c-mute);
}
.btn.primary {
  background: var(--c-accent);
  color: #fff;
  border: 1px solid var(--c-accent);
}
.btn.primary:hover { background: var(--c-accent); }
.btn.primary:disabled { opacity: 0.5; cursor: not-allowed; }
.btn.icon-btn {
  background: transparent;
  border: 1px solid var(--c-border-strong);
  color: var(--c-mute);
  width: 30px;
  height: 30px;
  padding: 0;
  font-size: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
}
.btn.icon-btn:hover { color: var(--c-fg); border-color: var(--c-mute); }

/* ── 全宽 banner ─────────────────────────────────────── */
.alert-banner {
  background: rgba(255, 69, 58, 0.1);
  border: 1px solid rgba(255, 69, 58, 0.3);
  color: var(--c-err);
  border-radius: 8px;
  padding: 10px 14px;
  font-size: 13.5px;
}
.update-banner {
  display: flex;
  align-items: center;
  gap: 14px;
  background: linear-gradient(135deg, rgba(10, 132, 255, 0.15), rgba(90, 60, 255, 0.15));
  border: 1px solid var(--c-accent);
  border-radius: 10px;
  padding: 12px 16px;
}
.ub-icon {
  width: 32px;
  height: 32px;
  border-radius: 50%;
  background: var(--c-accent);
  color: #fff;
  font-size: 16px;
  font-weight: 700;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}
.ub-body { flex: 1; min-width: 0; }
.ub-title { font-size: 14.5px; font-weight: 600; margin-bottom: 2px; }
.ub-notes {
  font-size: 14.5px;
  color: var(--c-mute);
  white-space: pre-wrap;
  line-height: 1.4;
  max-height: 40px;
  overflow: hidden;
}
.ub-actions { display: flex; gap: 8px; flex-shrink: 0; }
.slide-enter-active, .slide-leave-active { transition: all 0.3s ease; }
.slide-enter-from, .slide-leave-to { opacity: 0; transform: translateY(-8px); }

/* ── 主网格（3 列） ──────────────────────────────────── */
.grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  grid-auto-rows: min-content;
  gap: 14px;
}
.span-1 { grid-column: span 1; }
.span-2 { grid-column: span 2; }
.span-3 { grid-column: span 3; }

/* ── 通用 card ───────────────────────────────────────── */
.card {
  background: var(--c-bg-card);
  border: 1px solid var(--c-border);
  border-radius: 12px;
  padding: 16px 20px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  box-sizing: border-box;
}
.muted {
  color: var(--c-mute);
  font-size: 13.5px;
}

/* ── 余额 hero ───────────────────────────────────────── */
.balance-card {
  grid-column: span 2;
  background: linear-gradient(135deg, var(--c-accent) 0%, var(--c-accent-2) 100%);
  border-color: transparent;
  color: #fff;
  padding: 18px 24px;
  position: relative;
  overflow: hidden;
}
.balance-card::after {
  content: "";
  position: absolute;
  right: -30px;
  top: -30px;
  width: 140px;
  height: 140px;
  background: radial-gradient(circle, rgba(255, 255, 255, 0.08), transparent 70%);
  pointer-events: none;
}
.bc-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.bc-label {
  font-size: 14.5px;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: rgba(255, 255, 255, 0.8);
}
.bc-pill {
  background: rgba(0, 0, 0, 0.2);
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 999px;
  padding: 3px 10px;
  font-size: 14.5px;
  color: rgba(255, 255, 255, 0.9);
}
.bc-value {
  font-size: 38px;
  font-weight: 700;
  letter-spacing: -0.02em;
  line-height: 1.1;
  font-variant-numeric: tabular-nums;
}
.bc-unit {
  font-size: 14px;
  font-weight: 500;
  color: rgba(255, 255, 255, 0.7);
  margin-left: 4px;
}
.bc-sub {
  font-size: 13.5px;
  color: rgba(255, 255, 255, 0.85);
}
.bc-sub strong {
  font-weight: 600;
  color: #fff;
}

/* ── 算力滑杆 ────────────────────────────────────────── */
.throttle-card {
  background: linear-gradient(135deg, rgba(48, 209, 88, 0.08), rgba(10, 132, 255, 0.04));
  border: 1px solid rgba(48, 209, 88, 0.3);
  gap: 10px;
}
.throttle-card.throttled {
  border-color: rgba(255, 159, 10, 0.4);
  background: linear-gradient(135deg, rgba(255, 159, 10, 0.08), rgba(255, 214, 10, 0.04));
}
.throttle-card.paused {
  border-color: rgba(255, 214, 10, 0.4);
  background: rgba(255, 214, 10, 0.06);
}
.th-head {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
}
.th-title {
  font-size: 13.5px;
  font-weight: 600;
  color: var(--c-fg);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}
.th-label {
  font-size: 14.5px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  color: var(--c-ok);
}
.throttle-card.throttled .th-label { color: var(--c-warn); }
.throttle-card.paused .th-label { color: var(--c-warn); }
.th-slider {
  width: 100%;
  height: 6px;
  -webkit-appearance: none;
  margin: 6px 0 4px;
  background: linear-gradient(to right,
    var(--c-ok) 0%,
    var(--c-ok) var(--pct, 100%),
    var(--c-border) var(--pct, 100%),
    var(--c-border) 100%);
  border-radius: 3px;
  outline: none;
  cursor: pointer;
}
.throttle-card.throttled .th-slider {
  background: linear-gradient(to right,
    var(--c-warn) 0%,
    var(--c-warn) var(--pct, 100%),
    var(--c-border) var(--pct, 100%),
    var(--c-border) 100%);
}
.th-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  width: 16px;
  height: 16px;
  background: #fff;
  border-radius: 50%;
  border: 2px solid var(--c-ok);
  cursor: grab;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.4);
}
.throttle-card.throttled .th-slider::-webkit-slider-thumb { border-color: var(--c-warn); }
.throttle-card.paused .th-slider::-webkit-slider-thumb { border-color: var(--c-warn); }
.th-slider::-webkit-slider-thumb:active { cursor: grabbing; }
.th-presets {
  display: flex;
  gap: 5px;
}
.th-preset {
  flex: 1;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid var(--c-border-strong);
  border-radius: 5px;
  padding: 4px 0;
  font-size: 14.5px;
  color: var(--c-mute);
  cursor: pointer;
  transition: all 0.12s;
}
.th-preset:hover {
  color: var(--c-fg);
  border-color: var(--c-mute);
}
.th-preset.active {
  background: var(--c-accent);
  color: #fff;
  border-color: var(--c-accent);
}

/* ── 任务执行（运行时） ──────────────────────────────── */
.task-card {
  background: linear-gradient(135deg, rgba(10, 132, 255, 0.08), rgba(90, 60, 255, 0.08));
  border-color: rgba(10, 132, 255, 0.45);
}
.task-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14.5px;
}
.task-pulse {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--c-accent);
  animation: pulse 1.2s ease-in-out infinite;
}
@keyframes pulse {
  0%, 100% { opacity: 1; transform: scale(1); }
  50%      { opacity: 0.4; transform: scale(1.4); }
}
.task-id {
  color: var(--c-mute);
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 14.5px;
}
.task-spacer { flex: 1; }
.task-reward {
  font-size: 13.5px;
  font-weight: 600;
  color: var(--c-ok);
}
.task-cmd {
  background: var(--c-bg-soft);
  border: 1px solid var(--c-border);
  border-radius: 6px;
  padding: 8px 10px;
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 13.5px;
  color: var(--c-fg);
  word-break: break-all;
}
.task-meta {
  display: flex;
  gap: 16px;
  font-size: 14.5px;
  color: var(--c-mute);
}
.task-meta code {
  color: var(--c-fg);
  font-family: ui-monospace, SFMono-Regular, monospace;
}

/* ── 最近任务列表 ────────────────────────────────────── */
.history-card {
  padding: 16px 20px;
}
.history-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  margin-bottom: 10px;
}
.history-head h3 {
  margin: 0;
  font-size: 14.5px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--c-fg);
}
.history-empty {
  padding: 24px 0;
  text-align: center;
  line-height: 1.6;
}
.he-title {
  font-size: 14.5px;
  color: var(--c-fg);
  margin-bottom: 4px;
}
.he-hint {
  font-size: 14.5px;
  color: var(--c-mute);
}
.he-hint code {
  background: var(--c-bg-soft);
  border: 1px solid var(--c-border);
  padding: 1px 5px;
  border-radius: 4px;
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 14px;
}
.history-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 320px;
  overflow-y: auto;
}
.history-item {
  background: var(--c-bg-soft);
  border: 1px solid var(--c-border);
  border-radius: 7px;
  padding: 8px 12px;
}
.hi-row {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13.5px;
}
.hi-icon {
  display: inline-flex;
  width: 16px;
  height: 16px;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  font-weight: 700;
  font-size: 14px;
  flex-shrink: 0;
}
.hi-icon.ok { background: rgba(48, 209, 88, 0.15); color: var(--c-ok); }
.hi-icon.failed { background: rgba(255, 69, 58, 0.15); color: var(--c-err); }
.hi-cmd {
  flex: 1;
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 14.5px;
  color: var(--c-fg);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.hi-reward {
  color: var(--c-ok);
  font-size: 14.5px;
  font-weight: 600;
  flex-shrink: 0;
}
.hi-meta {
  display: flex;
  gap: 8px;
  margin-top: 2px;
  margin-left: 26px;
  font-size: 14px;
  color: var(--c-mute);
}
.hi-id {
  font-family: ui-monospace, SFMono-Regular, monospace;
}

/* ── Toast ───────────────────────────────────────────── */
.toast {
  position: fixed;
  bottom: 20px;
  right: 20px;
  background: var(--c-border);
  border: 1px solid var(--c-border-strong);
  border-radius: 10px;
  padding: 10px 14px;
  min-width: 240px;
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
  z-index: 100;
}
.toast.ok { border-color: rgba(48, 209, 88, 0.5); }
.toast.fail { border-color: rgba(255, 69, 58, 0.5); }
.toast-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13.5px;
  margin-bottom: 2px;
}
.toast-icon { font-weight: 700; font-size: 14px; }
.toast.ok .toast-icon { color: var(--c-ok); }
.toast.fail .toast-icon { color: var(--c-err); }
.toast-time {
  margin-left: auto;
  color: var(--c-mute);
  font-size: 14px;
  font-family: ui-monospace, SFMono-Regular, monospace;
}
.toast-body {
  color: var(--c-mute);
  font-size: 14.5px;
}
.toast-enter-active, .toast-leave-active { transition: all 0.3s ease; }
.toast-enter-from, .toast-leave-to { opacity: 0; transform: translateY(20px); }
</style>
