<script setup lang="ts">
import { computed } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { useTasks, type TaskPhasePayload } from "../composables/useTasks"
import { useConnection } from "../composables/useConnection"
import { useBundles } from "../composables/useBundles"
import { useNav } from "../composables/useNav"

const { queued, running, verifying, done } = useTasks()
const { snap } = useConnection()
const { bundleForTaskType, canRunTaskType, missingDepsForTaskType } = useBundles()
const { goto } = useNav()

// 合并 4 个状态 → 1 张表，按时间倒序（最近的在前）
// running > verifying > queued > done 之间用 phase 区分；同 phase 内按 started_at 倒序
const allTasks = computed<TaskPhasePayload[]>(() => {
  const list = [
    ...running.value,
    ...verifying.value,
    ...queued.value,
    ...done.value,
  ]
  // 同 phase 之间已经按 unshift 顺序在 useTasks 里维护好了；不再二次排序避免抖动
  return list
})

async function quickInstall(missingDep: string, installCmd: string) {
  try {
    await invoke("install_dep", {
      bundleId: "task-recovery",
      depName: missingDep,
      installCmd: installCmd,
    })
    goto("toolbox") // 跳到工具箱看实时日志
  } catch (e) {
    console.error("快速安装失败:", e)
  }
}

type Phase = "queued" | "running" | "verifying" | "done"

const phaseMeta: Record<Phase, { label: string; icon: string; cls: string }> = {
  queued: { label: "待处理", icon: "○", cls: "st-queued" },
  running: { label: "运行中", icon: "●", cls: "st-running" },
  verifying: { label: "验证中", icon: "✓", cls: "st-verifying" },
  done: { label: "已完成", icon: "▣", cls: "st-done" },
}

function phaseLabel(t: TaskPhasePayload): string {
  if (t.phase === "done" && t.ok === false) return "失败"
  return phaseMeta[t.phase].label
}

function phaseCls(t: TaskPhasePayload): string {
  if (t.phase === "done" && t.ok === false) return "st-fail"
  return phaseMeta[t.phase].cls
}

function phaseIcon(t: TaskPhasePayload): string {
  if (t.phase === "done" && t.ok === false) return "✗"
  return phaseMeta[t.phase].icon
}

// 统计四种状态各有多少 (表头右上角小徽章)
const stats = computed(() => ({
  running: running.value.length,
  verifying: verifying.value.length,
  queued: queued.value.length,
  done: done.value.length,
}))

function shortId(id: string): string {
  return "#" + id.slice(0, 4)
}

function fmtTime(ms: number): string {
  if (!ms) return ""
  const d = new Date(ms)
  return d.toLocaleTimeString("zh-CN", { hour12: false, hour: "2-digit", minute: "2-digit" })
}

function fmtElapsed(ms?: number): string {
  if (!ms) return ""
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(1)}s`
}

// 待处理列：客户端单机一般为空 → 显示「等待派发」placeholder
const queuedEmpty = computed(() => queued.value.length === 0)
const nodeIdShort = computed(() => snap.value.node_id?.slice(0, 14) || "")
</script>

<template>
  <div class="kanban">
    <div class="kanban-head">
      <h3 class="title">任务流水线</h3>
      <div class="head-right">
        <span class="stat-badge st-running" v-if="stats.running">● {{ stats.running }} 运行</span>
        <span class="stat-badge st-verifying" v-if="stats.verifying">✓ {{ stats.verifying }} 验证</span>
        <span class="stat-badge st-done" v-if="stats.done">▣ {{ stats.done }} 完成</span>
        <span class="sub">节点 {{ nodeIdShort }}</span>
      </div>
    </div>

    <!-- 空态：完全没任务 -->
    <div v-if="allTasks.length === 0" class="empty">
      <div class="empty-dot" />
      <div class="empty-text">已就绪</div>
      <div class="empty-hint">等待平台派发任务</div>
    </div>

    <!-- 表格 -->
    <div v-else class="tbl-wrap">
      <table class="tbl">
        <thead>
          <tr>
            <th class="th-status">状态</th>
            <th class="th-id">ID</th>
            <th class="th-bundle">工具包</th>
            <th class="th-cmd">命令</th>
            <th class="th-time">时间</th>
            <th class="th-elapsed">时长</th>
            <th class="th-reward">奖励</th>
            <th class="th-action"></th>
          </tr>
        </thead>
        <transition-group tag="tbody" name="row">
          <tr
            v-for="t in allTasks"
            :key="t.task_id"
            :class="['row', phaseCls(t), { pulse: t.phase === 'running' }]"
          >
            <td class="td-status">
              <span class="st-pill" :class="phaseCls(t)">
                <span class="st-icon">{{ phaseIcon(t) }}</span>
                <span class="st-label">{{ phaseLabel(t) }}</span>
              </span>
            </td>
            <td class="td-id mono">{{ shortId(t.task_id) }}</td>
            <td class="td-bundle">
              <span
                v-if="bundleForTaskType(t.task_type)"
                class="bundle-tag"
                :class="{ blocked: !canRunTaskType(t.task_type) }"
                :title="canRunTaskType(t.task_type)
                  ? `工具包 ${bundleForTaskType(t.task_type)?.name} 已就绪`
                  : `缺：${missingDepsForTaskType(t.task_type).map(d => d.name).join('、')}`"
              >
                <span>{{ bundleForTaskType(t.task_type)?.icon }}</span>
                {{ bundleForTaskType(t.task_type)?.name }}
                <span v-if="!canRunTaskType(t.task_type)">⚠</span>
              </span>
              <span v-else class="muted">—</span>
            </td>
            <td class="td-cmd mono" :title="t.cmd">
              <span class="runtime-tag" v-if="t.runtime">{{ t.runtime }}</span>
              <span class="cmd-text">{{ t.cmd || "—" }}</span>
            </td>
            <td class="td-time mono">
              {{ t.phase === 'done' ? fmtTime(t.started_at_ms + (t.elapsed_ms ?? 0)) : fmtTime(t.started_at_ms) }}
            </td>
            <td class="td-elapsed mono">{{ fmtElapsed(t.elapsed_ms) || "—" }}</td>
            <td class="td-reward mono">
              <span v-if="t.reward" class="reward">+{{ t.reward.toFixed(2) }}</span>
              <span v-else class="muted">—</span>
            </td>
            <td class="td-action">
              <button
                v-if="t.phase === 'done' && t.ok === false && t.missing_dep && t.missing_install_cmd"
                class="act-btn"
                :title="`缺 ${t.missing_dep}：点击安装`"
                @click.stop="quickInstall(t.missing_dep, t.missing_install_cmd)"
              >
                安装 {{ t.missing_dep }}
              </button>
            </td>
          </tr>
        </transition-group>
      </table>
    </div>
  </div>
</template>

<style scoped>
.kanban {
  background: var(--c-bg-card);
  border: 1px solid var(--c-border);
  border-radius: 10px;
  padding: 10px 14px 10px;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.kanban-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  margin-bottom: 6px;
}
.title {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--c-fg);
}
.sub {
  font-size: 13.5px;
  color: var(--c-mute);
  font-family: ui-monospace, SFMono-Regular, monospace;
}

/* 表头右侧统计徽章 */
.head-right {
  display: flex;
  align-items: center;
  gap: 8px;
}
.stat-badge {
  font-size: 12px;
  padding: 2px 8px;
  border-radius: 999px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  background: var(--c-border);
  color: var(--c-mute);
}
.stat-badge.st-running {
  background: rgba(10, 132, 255, 0.18);
  color: var(--c-accent);
}
.stat-badge.st-verifying {
  background: rgba(255, 159, 10, 0.18);
  color: var(--c-warn);
}
.stat-badge.st-done {
  background: rgba(48, 209, 88, 0.18);
  color: var(--c-ok);
}

/* 空态 */
.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 36px 0 30px;
  color: var(--c-mute);
}
.empty-dot {
  width: 8px;
  height: 8px;
  background: var(--c-mute);
  border-radius: 50%;
  margin-bottom: 8px;
  opacity: 0.45;
}
.empty-text {
  font-size: 14.5px;
  color: var(--c-fg);
  opacity: 0.8;
}
.empty-hint {
  font-size: 13px;
  color: var(--c-mute);
  margin-top: 3px;
  opacity: 0.7;
}

/* 表格容器：竖向滚动 */
.tbl-wrap {
  max-height: 320px;
  overflow-y: auto;
  border: 1px solid var(--c-border);
  border-radius: 8px;
  background: var(--c-bg-soft);
}
.tbl {
  width: 100%;
  border-collapse: collapse;
  font-size: 13.5px;
}
.tbl thead th {
  position: sticky;
  top: 0;
  z-index: 1;
  background: var(--c-bg-card);
  color: var(--c-mute);
  text-align: left;
  padding: 8px 10px;
  font-weight: 600;
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  border-bottom: 1px solid var(--c-border);
}
.tbl tbody tr {
  border-bottom: 1px solid var(--c-border);
  transition: background 0.18s;
}
.tbl tbody tr:last-child { border-bottom: 0; }
.tbl tbody tr:hover {
  background: rgba(255, 255, 255, 0.025);
}
.tbl td {
  padding: 8px 10px;
  vertical-align: middle;
  color: var(--c-fg);
}
.mono {
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-variant-numeric: tabular-nums;
}
.muted {
  color: var(--c-mute);
  opacity: 0.6;
}

/* 行颜色（按 phase 着色左边框） */
.row {
  border-left: 3px solid transparent;
}
.row.st-running {
  border-left-color: var(--c-accent);
  background: linear-gradient(90deg, rgba(10, 132, 255, 0.06), transparent 50%);
}
.row.st-verifying {
  border-left-color: var(--c-warn);
  background: linear-gradient(90deg, rgba(255, 159, 10, 0.06), transparent 50%);
}
.row.st-done {
  border-left-color: var(--c-ok);
}
.row.st-fail {
  border-left-color: var(--c-err);
  background: linear-gradient(90deg, rgba(255, 69, 58, 0.05), transparent 50%);
}
.row.pulse .st-icon {
  animation: pulse 1.1s ease-in-out infinite;
}

/* 状态徽章 */
.st-pill {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 2px 8px;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 600;
  background: var(--c-border);
  color: var(--c-mute);
  white-space: nowrap;
}
.st-pill.st-queued {
  background: rgba(160, 160, 160, 0.15);
  color: var(--c-mute);
}
.st-pill.st-running {
  background: rgba(10, 132, 255, 0.18);
  color: var(--c-accent);
}
.st-pill.st-verifying {
  background: rgba(255, 159, 10, 0.18);
  color: var(--c-warn);
}
.st-pill.st-done {
  background: rgba(48, 209, 88, 0.18);
  color: var(--c-ok);
}
.st-pill.st-fail {
  background: rgba(255, 69, 58, 0.18);
  color: var(--c-err);
}
.st-icon {
  font-size: 11px;
  line-height: 1;
}

/* 列宽控制 */
.th-status   { width: 92px; }
.th-id       { width: 64px; }
.th-bundle   { width: 140px; }
.th-cmd      { min-width: 200px; }
.th-time     { width: 70px; }
.th-elapsed  { width: 70px; }
.th-reward   { width: 80px; }
.th-action   { width: 120px; }

.td-id, .td-time, .td-elapsed { color: var(--c-mute); }

/* cmd 单元 */
.td-cmd {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 0; /* 跟 min-width 配合让 ellipsis 生效 */
}
.runtime-tag {
  background: rgba(255, 255, 255, 0.06);
  color: var(--c-mute);
  padding: 1px 5px;
  border-radius: 3px;
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  margin-right: 6px;
}
.cmd-text { color: var(--c-fg); }

/* 工具包 tag */
.bundle-tag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  background: var(--c-accent-soft);
  color: var(--c-accent);
  border-radius: var(--r-full);
  font-size: 11.5px;
  white-space: nowrap;
}
.bundle-tag.blocked {
  background: var(--c-err-soft);
  color: var(--c-err);
}

/* 奖励 */
.reward {
  color: var(--c-ok);
  font-weight: 600;
}

/* 一键安装按钮 */
.act-btn {
  background: var(--c-err);
  color: #fff;
  border: none;
  padding: 4px 10px;
  border-radius: var(--r-xs);
  font-size: 11.5px;
  cursor: pointer;
  white-space: nowrap;
}
.act-btn:hover { opacity: 0.9; }

/* 行进出动画 */
.row-enter-active,
.row-leave-active {
  transition: all 0.25s ease;
}
.row-enter-from {
  opacity: 0;
  transform: translateY(-4px);
}
.row-leave-to {
  opacity: 0;
  transform: translateY(4px);
}
.row-move {
  transition: transform 0.25s ease;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50%      { opacity: 0.35; }
}
</style>
