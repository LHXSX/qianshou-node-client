<script setup lang="ts">
/**
 * 实时任务卡流 (替代 TaskKanban 表格 · 2026-05-21)
 *
 * 设计:
 *   - 纵向卡片流 · 最新在顶
 *   - 卡片信息密度: 谁发的(头像+名字) / 任务名 / 类型 chip / 进度条(running) / 大 reward(done) / 真实 stderr(failed 可展开)
 *   - 失败卡: 红色光晕 + chevron 展开看完整 stderr
 *   - 成功卡: hover 浮起 + 金色 reward 多巴胺
 *   - 运行中卡: 蓝色 pulse glow + 进度横条
 *   - 空态: 节点 ID + "等待平台派任务" 文案
 */
import { computed, ref } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { useTasks, type TaskPhasePayload } from "../composables/useTasks"
import { useConnection } from "../composables/useConnection"
import { useNav } from "../composables/useNav"
import Icon from "./Icon.vue"
import { iconForTaskType, iconForStatus } from "../icons/paths"

const { queued, running, verifying, done } = useTasks()
const { snap } = useConnection()
const { goto } = useNav()

const expanded = ref<Set<string>>(new Set())

function toggle(id: string) {
  if (expanded.value.has(id)) expanded.value.delete(id)
  else expanded.value.add(id)
}

// 合并 + 排序: running 在最前 · 再 verifying · queued · done · 同段按 started_at desc
const feed = computed<TaskPhasePayload[]>(() => [
  ...running.value,
  ...verifying.value,
  ...queued.value,
  ...done.value,
])

const stats = computed(() => ({
  running: running.value.length,
  verifying: verifying.value.length,
  queued: queued.value.length,
  done: done.value.length,
  fail: done.value.filter((t) => t.ok === false).length,
  reward: done.value
    .filter((t) => t.ok)
    .reduce((sum, t) => sum + (t.reward || 0), 0),
}))

function statusKey(t: TaskPhasePayload): "queued" | "running" | "verifying" | "done" | "failed" {
  if (t.phase === "done" && t.ok === false) return "failed"
  return t.phase
}

function statusLabel(t: TaskPhasePayload): string {
  switch (statusKey(t)) {
    case "queued": return "排队中"
    case "running": return "运行中"
    case "verifying": return "验证中"
    case "done": return "成功"
    case "failed": return "失败"
  }
}

function shortId(id: string): string {
  return id.slice(0, 6)
}

function avatarLetter(name?: string): string {
  return (name?.trim() || "?").charAt(0).toUpperCase()
}

function fmtTime(ms?: number): string {
  if (!ms) return ""
  const d = new Date(ms)
  return d.toLocaleTimeString("zh-CN", { hour12: false, hour: "2-digit", minute: "2-digit", second: "2-digit" })
}

function fmtDuration(ms?: number): string {
  if (!ms || ms < 0) return "—"
  if (ms < 1000) return `${ms}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  const m = Math.floor(ms / 60_000)
  const s = Math.round((ms % 60_000) / 1000)
  return `${m}m ${s}s`
}

// running 卡片进度: 用 timeout_s 当总长 · started_at_ms 算已用比例
function runningProgress(t: TaskPhasePayload): number {
  if (!t.started_at_ms || !t.timeout_s) return 0
  const elapsed = Date.now() - t.started_at_ms
  const pct = (elapsed / (t.timeout_s * 1000)) * 100
  return Math.max(3, Math.min(95, pct))
}

async function quickInstall(t: TaskPhasePayload) {
  if (!t.missing_dep || !t.missing_install_cmd) return
  try {
    await invoke("install_dep", {
      bundleId: "task-recovery",
      depName: t.missing_dep,
      installCmd: t.missing_install_cmd,
    })
    goto("toolbox")
  } catch (e) {
    console.error("快速安装失败:", e)
  }
}

const nodeIdShort = computed(() => snap.value.node_id?.slice(0, 12) || "—")
</script>

<template>
  <section class="run-feed">
    <!-- 头部统计条 -->
    <header class="feed-head">
      <div class="head-left">
        <h3 class="title">实时任务流</h3>
        <span class="sub">本节点 · {{ nodeIdShort }}</span>
      </div>
      <div class="head-right">
        <span v-if="stats.running" class="stat running">
          <Icon name="status-running" :size="13" /> {{ stats.running }} 运行
        </span>
        <span v-if="stats.verifying" class="stat verifying">
          <Icon name="status-verifying" :size="13" /> {{ stats.verifying }} 验证
        </span>
        <span v-if="stats.done" class="stat done">
          <Icon name="status-done" :size="13" /> {{ stats.done - stats.fail }} 成功
        </span>
        <span v-if="stats.fail" class="stat failed">
          <Icon name="status-failed" :size="13" /> {{ stats.fail }} 失败
        </span>
        <span v-if="stats.reward > 0" class="stat reward">
          <Icon name="coin" :size="13" /> +{{ stats.reward.toFixed(2) }}
        </span>
      </div>
    </header>

    <!-- 空态 (紧凑横排) -->
    <div v-if="feed.length === 0" class="empty">
      <div class="empty-pulse"><span /></div>
      <div>
        <span class="empty-title">已就绪 · 等待派单</span>
        <span class="empty-hint">平台一旦有匹配任务 · 会即时推送到此节点</span>
      </div>
    </div>

    <!-- 卡片流 -->
    <transition-group v-else tag="div" name="card" class="card-stream">
      <article
        v-for="t in feed" :key="t.task_id"
        :class="['feed-card', `s-${statusKey(t)}`, { open: expanded.has(t.task_id), failable: statusKey(t) === 'failed' }]"
        @click="statusKey(t) === 'failed' && toggle(t.task_id)"
      >
        <!-- 左: 发布人头像 -->
        <div class="fc-avatar" :title="t.requester_name || '未知发布人'">
          {{ avatarLetter(t.requester_name) }}
        </div>

        <!-- 中: 任务信息 -->
        <div class="fc-body">
          <div class="fc-line1">
            <span class="fc-title" :title="t.workload_name || '(未命名任务)'">
              {{ t.workload_name || (t.task_type ? `${t.task_type} 任务` : "未命名任务") }}
            </span>
            <span class="fc-type">
              <Icon :name="iconForTaskType(t.task_type)" :size="12" />
              {{ t.task_type || "—" }}
            </span>
            <span v-if="(t.total ?? 1) > 1" class="fc-shard">
              分片 {{ (t.index ?? 0) + 1 }}/{{ t.total }}
            </span>
          </div>
          <div class="fc-line2">
            <span class="fc-by">
              <Icon name="user" :size="11" /> {{ t.requester_name || "匿名" }}
            </span>
            <span class="dot">·</span>
            <span class="fc-id mono">#{{ shortId(t.task_id) }}</span>
            <span class="dot">·</span>
            <span class="fc-time mono">{{ fmtTime(t.started_at_ms) }}</span>
            <span v-if="t.elapsed_ms" class="dot">·</span>
            <span v-if="t.elapsed_ms" class="fc-dur mono">{{ fmtDuration(t.elapsed_ms) }}</span>
          </div>
          <!-- running 状态: 进度横条 -->
          <div v-if="statusKey(t) === 'running'" class="fc-bar">
            <span class="bar-fill" :style="{ width: runningProgress(t) + '%' }" />
          </div>
          <!-- failed 状态: 折叠真实错误 -->
          <div v-if="statusKey(t) === 'failed' && t.error && expanded.has(t.task_id)" class="fc-err" @click.stop>
            <div class="err-head">
              <Icon name="status-failed" :size="13" />
              失败原因
              <span v-if="t.missing_dep" class="missing-chip">
                缺依赖: {{ t.missing_dep }}
              </span>
            </div>
            <pre class="err-body">{{ t.error }}</pre>
            <div v-if="t.missing_install_cmd" class="err-actions">
              <button class="install-btn" @click.stop="quickInstall(t)">
                <Icon name="action-install" :size="13" />
                一键安装 {{ t.missing_dep }}
              </button>
            </div>
          </div>
        </div>

        <!-- 右: 状态徽章 / reward -->
        <div class="fc-right">
          <div :class="['st-pill', `s-${statusKey(t)}`]">
            <Icon :name="iconForStatus(t.phase, t.ok)" :size="13" />
            <span>{{ statusLabel(t) }}</span>
          </div>
          <div v-if="statusKey(t) === 'done' && t.reward" class="fc-reward">
            <span class="rw-num">+{{ t.reward.toFixed(2) }}</span>
            <span class="rw-unit">EDG</span>
          </div>
          <div v-else-if="statusKey(t) === 'running'" class="fc-running-tag">
            <span class="rl-dot" />运行中
          </div>
          <button
            v-if="statusKey(t) === 'failed'"
            class="fc-chev"
            :class="{ rot: expanded.has(t.task_id) }"
            @click.stop="toggle(t.task_id)"
            :title="expanded.has(t.task_id) ? '收起' : '查看错误'"
          >
            <Icon name="action-chevron" :size="14" />
          </button>
        </div>
      </article>
    </transition-group>
  </section>
</template>

<style scoped>
.run-feed {
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md);
  display: flex;
  flex-direction: column;
  min-height: 0;     /* 允许 flex 子项 overflow */
  overflow: hidden;  /* 保护圆角不被卡片溢出 */
  height: 100%;      /* 撑满 .w-feed grid 单元 */
}

/* head */
.feed-head {
  display: flex; align-items: center; justify-content: space-between;
  gap: var(--sp-5);
  padding: 10px var(--sp-6);
  border-bottom: 1px solid var(--c-line);
  min-height: 38px;
}
.head-left { display: flex; align-items: center; gap: var(--sp-5); }
.title {
  margin: 0;
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--c-fg-soft);
}
.sub {
  font-size: var(--fs-xs);
  color: var(--c-mute);
  font-family: ui-monospace, monospace;
}

.head-right { display: flex; gap: 6px; flex-wrap: wrap; }
.stat {
  display: inline-flex; align-items: center; gap: 4px;
  padding: 2px 8px;
  border-radius: var(--r-pill);
  font-size: var(--fs-xs);
  font-weight: var(--fw-medium);
  font-variant-numeric: tabular-nums;
  background: var(--c-bg-soft);
  color: var(--c-mute);
}
.stat.running   { background: var(--c-info-soft); color: var(--c-info); }
.stat.verifying { background: var(--c-warn-soft); color: var(--c-warn); }
.stat.done      { background: var(--c-ok-soft);   color: var(--c-ok); }
.stat.failed    { background: var(--c-err-soft);  color: var(--c-err); }
.stat.reward    { background: var(--c-warn-soft); color: var(--c-warn); font-weight: var(--fw-semibold); }

/* 空态 */
.empty {
  display: flex; align-items: center; justify-content: center;
  gap: var(--sp-5);
  padding: 24px var(--sp-6);
  color: var(--c-mute);
  min-height: 110px;
}
.empty-pulse {
  width: 36px; height: 36px;
  border-radius: 50%;
  background: radial-gradient(circle, var(--c-brand-soft) 0%, transparent 70%);
  display: flex; align-items: center; justify-content: center;
  animation: brand-pulse 2.4s ease-in-out infinite;
  flex-shrink: 0;
}
.empty-pulse span {
  width: 8px; height: 8px;
  background: var(--c-brand);
  border-radius: 50%;
  box-shadow: 0 0 10px var(--c-brand-glow);
}
.empty-title { font-size: var(--fs-sm); color: var(--c-fg-soft); font-weight: var(--fw-medium); display: block; }
.empty-hint  { font-size: var(--fs-xs); color: var(--c-mute); margin-top: 2px; display: block; }

/* 卡片流 · 内部纵向滚动 · 不让卡片溢出右侧 widget */
.card-stream {
  display: flex; flex-direction: column;
  padding: var(--sp-3);
  gap: 2px;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-width: thin;
  position: relative; /* transition-group leave 用 absolute 时 · 限定在容器内 */
}
.card-stream::-webkit-scrollbar { width: 6px; }
.card-stream::-webkit-scrollbar-thumb {
  background: var(--c-line-strong, rgba(120,120,128,0.35));
  border-radius: 3px;
}
.card-stream::-webkit-scrollbar-thumb:hover {
  background: var(--c-mute, rgba(120,120,128,0.55));
}
.card-stream::-webkit-scrollbar-track { background: transparent; }

.feed-card {
  display: grid;
  grid-template-columns: 36px 1fr auto;
  gap: var(--sp-5);
  align-items: center;
  padding: 10px var(--sp-5);
  border-radius: var(--r-sm);
  position: relative;
  transition: background var(--dur-base);
}
.feed-card:hover { background: var(--c-bg-soft); }
.feed-card.failable { cursor: pointer; }
.feed-card.s-failed { background: var(--c-err-soft); }
.feed-card.open { background: var(--c-err-soft); }

/* status 左边 2px 立条 */
.feed-card::before {
  content: "";
  position: absolute;
  left: 0; top: 8px; bottom: 8px;
  width: 2px;
  border-radius: 0 2px 2px 0;
  background: transparent;
  transition: background var(--dur-base);
}
.feed-card.s-queued::before    { background: var(--c-faint); }
.feed-card.s-running::before   { background: var(--c-info);  animation: brand-pulse 1.6s ease-in-out infinite; }
.feed-card.s-verifying::before { background: var(--c-warn); }
.feed-card.s-done::before      { background: var(--c-ok); }
.feed-card.s-failed::before    { background: var(--c-err); }

/* 头像 */
.fc-avatar {
  width: 32px; height: 32px;
  border-radius: var(--r-sm);
  display: flex; align-items: center; justify-content: center;
  background: linear-gradient(135deg, var(--c-brand), var(--c-brand-2));
  color: #fff;
  font-weight: var(--fw-semibold);
  font-size: var(--fs-md);
  flex-shrink: 0;
}
.feed-card.s-failed .fc-avatar { background: linear-gradient(135deg, var(--c-warn), var(--c-err)); }
.feed-card.s-done .fc-avatar { background: linear-gradient(135deg, #06b6d4, var(--c-ok)); }

/* body */
.fc-body { min-width: 0; display: flex; flex-direction: column; gap: 3px; }
.fc-line1 {
  display: flex; align-items: center; gap: var(--sp-4);
  font-size: var(--fs-sm);
  min-width: 0;
}
.fc-title {
  color: var(--c-fg);
  font-weight: var(--fw-semibold);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  max-width: 360px;
}
.fc-type {
  display: inline-flex; align-items: center; gap: 4px;
  font-size: var(--fs-2xs);
  font-weight: var(--fw-medium);
  padding: 1px 7px;
  background: var(--c-bg-soft);
  color: var(--c-mute);
  border-radius: var(--r-xs);
  flex-shrink: 0;
  font-family: ui-monospace, monospace;
}
.feed-card:hover .fc-type { background: var(--c-bg-card); }
.fc-shard {
  font-size: var(--fs-2xs);
  color: var(--c-mute);
  font-family: ui-monospace, monospace;
  padding: 1px 6px;
  border-radius: var(--r-xs);
  background: var(--c-bg-soft);
  flex-shrink: 0;
}
.fc-line2 {
  display: flex; align-items: center; gap: 5px;
  font-size: var(--fs-2xs);
  color: var(--c-mute);
  flex-wrap: wrap;
}
.fc-by { display: inline-flex; align-items: center; gap: 3px; }
.fc-line2 .dot { opacity: 0.5; }
.mono { font-family: ui-monospace, monospace; font-variant-numeric: tabular-nums; }

/* running 进度条 */
.fc-bar {
  margin-top: 6px;
  height: 3px;
  background: var(--c-bg-soft);
  border-radius: var(--r-pill);
  overflow: hidden;
}
.bar-fill {
  display: block; height: 100%;
  background: linear-gradient(90deg, var(--c-brand), var(--c-info));
  border-radius: var(--r-pill);
  transition: width 0.8s ease;
}

/* 错误展开块 */
.fc-err {
  grid-column: 1 / -1;
  margin-top: var(--sp-4);
  padding: var(--sp-5);
  background: var(--c-bg);
  border: 1px solid var(--c-err);
  border-radius: var(--r-sm);
  cursor: default;
}
.err-head {
  display: flex; align-items: center; gap: 6px;
  font-size: var(--fs-xs);
  font-weight: var(--fw-semibold);
  color: var(--c-err);
  margin-bottom: var(--sp-3);
}
.missing-chip {
  font-size: var(--fs-2xs);
  padding: 1px 7px;
  background: var(--c-warn-soft);
  color: var(--c-warn);
  border-radius: var(--r-pill);
  margin-left: 4px;
}
.err-body {
  margin: 0;
  padding: var(--sp-4) var(--sp-5);
  background: var(--c-bg-card);
  border-radius: var(--r-xs);
  font-family: ui-monospace, monospace;
  font-size: var(--fs-xs);
  line-height: 1.55;
  color: var(--c-fg-soft);
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 240px;
  overflow: auto;
}
.err-actions { margin-top: var(--sp-3); display: flex; }
.install-btn {
  display: inline-flex; align-items: center; gap: 5px;
  background: var(--c-err);
  color: #fff;
  border: none;
  padding: 5px 12px;
  border-radius: var(--r-sm);
  font-size: var(--fs-xs);
  font-weight: var(--fw-medium);
  cursor: pointer;
  font-family: inherit;
}
.install-btn:hover { background: #ef4444; }

/* 右侧 */
.fc-right {
  display: flex; align-items: center; gap: var(--sp-5);
  flex-shrink: 0;
}
.st-pill {
  display: inline-flex; align-items: center; gap: 4px;
  padding: 3px 8px;
  border-radius: var(--r-pill);
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  background: var(--c-bg-soft);
  color: var(--c-mute);
  white-space: nowrap;
}
.st-pill.s-queued    { color: var(--c-mute); }
.st-pill.s-running   { background: var(--c-info-soft); color: var(--c-info); }
.st-pill.s-verifying { background: var(--c-warn-soft); color: var(--c-warn); }
.st-pill.s-done      { background: var(--c-ok-soft);   color: var(--c-ok); }
.st-pill.s-failed    { background: var(--c-err-soft);  color: var(--c-err); }

/* reward */
.fc-reward {
  display: flex; align-items: baseline; gap: 3px;
}
.rw-num {
  font-size: var(--fs-lg);
  font-weight: var(--fw-bold);
  color: var(--c-warn);
  font-family: ui-monospace, monospace;
  letter-spacing: -0.02em;
}
.rw-unit { font-size: var(--fs-2xs); color: var(--c-mute); font-weight: var(--fw-medium); }

.fc-running-tag {
  display: inline-flex; align-items: center; gap: 5px;
  font-size: var(--fs-2xs);
  color: var(--c-info);
  font-weight: var(--fw-medium);
}
.rl-dot {
  width: 6px; height: 6px;
  background: var(--c-info);
  border-radius: 50%;
  animation: brand-pulse 1.2s ease-in-out infinite;
}

.fc-chev {
  background: transparent;
  border: 1px solid var(--c-line);
  color: var(--c-mute);
  width: 22px; height: 22px;
  border-radius: var(--r-xs);
  display: flex; align-items: center; justify-content: center;
  cursor: pointer;
  transition: all var(--dur-base);
}
.fc-chev:hover { color: var(--c-err); border-color: var(--c-err); }
.fc-chev.rot   { transform: rotate(180deg); }

/* enter/leave 动画 */
.card-enter-active { transition: all var(--dur-slow) var(--ease-out); }
.card-leave-active { transition: all var(--dur-base) ease; position: absolute; }
.card-enter-from { opacity: 0; transform: translateY(-6px); }
.card-leave-to   { opacity: 0; transform: translateY(6px); }
.card-move { transition: transform var(--dur-base) ease; }
</style>
