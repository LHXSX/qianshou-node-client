<script setup lang="ts">
/**
 * 任务历史 · 次时代重设计 (2026-05-21)
 *
 * 布局:
 *   - 顶部 5 张紧凑 KPI (StatCard)
 *   - 筛选 + 搜索条 (panel head 样式)
 *   - 表格式列表 (高 44px 一行) · 点击展开 detail
 *   - 详情 panel: 错误 / 输出预览 / 元数据 grid
 *
 * 数据全 from useHistory · 本节点真实 history
 */
import { computed, onMounted, ref } from "vue"
import { useHistory, type HistoryItem } from "../composables/useHistory"
import { useConnection } from "../composables/useConnection"
import Icon from "../components/Icon.vue"
import StatCard from "../components/dashboard/StatCard.vue"
import { iconForTaskType } from "../icons/paths"

const { items, loading, error, stats, load, refreshQuiet } = useHistory()
const { snap } = useConnection()

const filter = ref<"all" | "ok" | "failed">("all")
const search = ref("")
const expanded = ref<Set<string>>(new Set())

const filteredItems = computed<HistoryItem[]>(() => {
  let list = items.value as readonly HistoryItem[]
  if (filter.value === "ok") list = list.filter((i) => i.status === "ok")
  else if (filter.value === "failed") list = list.filter((i) => i.status !== "ok")
  if (search.value) {
    const q = search.value.toLowerCase()
    list = list.filter((i) =>
      (i.task_type || "").toLowerCase().includes(q) ||
      (i.task_name || "").toLowerCase().includes(q) ||
      (i.task_id || "").toLowerCase().includes(q),
    )
  }
  return list as HistoryItem[]
})

const nodeIdShort = computed(() => snap.value.node_id?.slice(0, 14) || "—")

function statusLabel(it: HistoryItem): string {
  switch (it.status) {
    case "ok": return "完成"
    case "failed": return "失败"
    case "running": return "运行"
    case "dispatched": return "待执行"
    default: return it.status || "—"
  }
}

function statusCls(it: HistoryItem): string {
  switch (it.status) {
    case "ok": return "st-done"
    case "failed": return "st-fail"
    case "running": return "st-running"
    case "dispatched": return "st-queued"
    default: return "st-queued"
  }
}

function fmtTime(s?: string): string {
  if (!s) return "—"
  try {
    const d = new Date(s)
    const now = new Date()
    const diff = now.getTime() - d.getTime()
    if (diff > 0 && diff < 60 * 60 * 1000) {
      const m = Math.floor(diff / 60_000)
      return m <= 0 ? "刚刚" : `${m}m前`
    }
    if (diff > 0 && diff < 24 * 60 * 60 * 1000) {
      return d.toLocaleString("zh-CN", { hour12: false, hour: "2-digit", minute: "2-digit" })
    }
    return d.toLocaleString("zh-CN", { hour12: false, month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })
  } catch { return s }
}

function fmtFull(s?: string): string {
  if (!s) return "—"
  try { return new Date(s).toLocaleString("zh-CN", { hour12: false }) } catch { return s }
}

function fmtElapsed(ms: number): string {
  if (!ms) return "—"
  if (ms < 1000) return `${ms}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  return `${Math.floor(ms / 60_000)}m${Math.round((ms % 60_000) / 1000)}s`
}

function fmtReward(n: number): string {
  if (!n) return "—"
  return n.toFixed(3)
}

function toggleExpand(id: string) {
  if (expanded.value.has(id)) expanded.value.delete(id)
  else expanded.value.add(id)
}

function fmtOutput(preview: string): string {
  if (!preview) return "(无输出)"
  try { return JSON.stringify(JSON.parse(preview), null, 2) } catch { return preview.slice(0, 2000) }
}

const avgElapsedLabel = computed(() => {
  const v = stats.value.elapsedAvg
  if (!v) return "—"
  return v < 1000 ? `${v}ms` : `${(v / 1000).toFixed(1)}s`
})

onMounted(async () => {
  if (items.value.length > 0) refreshQuiet()
  else await load()
})
</script>

<template>
  <div class="page">
    <!-- 页头 -->
    <header class="page-head">
      <div>
        <h1 class="page-title">任务历史</h1>
        <p class="page-sub">本节点 <code class="mono">{{ nodeIdShort }}</code> 处理过的所有任务</p>
      </div>
      <button class="btn-action" @click="load(true)" :disabled="loading">
        <Icon name="action-refresh" :size="14" :class="{ spin: loading }" />
        {{ loading ? "刷新中" : "刷新" }}
      </button>
    </header>

    <!-- 5 张 KPI -->
    <div class="kpi-row">
      <StatCard label="总任务" :value="stats.total" accent="brand" icon="nav-history" />
      <StatCard label="成功" :value="stats.ok" accent="ok" icon="status-done"
        :hint="stats.total ? `${((stats.ok/stats.total)*100).toFixed(0)}% 成功率` : ''" />
      <StatCard label="失败" :value="stats.failed" accent="err" icon="status-failed"
        :hint="stats.total && stats.failed ? `${((stats.failed/stats.total)*100).toFixed(0)}% 失败率` : '0 失败'" />
      <StatCard label="累计奖励" :value="stats.totalReward.toFixed(3)" unit="EDG" accent="warn" icon="coin" />
      <StatCard label="平均耗时" :value="avgElapsedLabel" accent="mute" icon="clock" />
    </div>

    <!-- 筛选 + 搜索 -->
    <section class="panel">
      <header class="p-head">
        <div class="seg">
          <button :class="{ on: filter === 'all' }" @click="filter = 'all'">全部 <span class="seg-n mono">{{ stats.total }}</span></button>
          <button :class="{ on: filter === 'ok' }" @click="filter = 'ok'">成功 <span class="seg-n mono">{{ stats.ok }}</span></button>
          <button :class="{ on: filter === 'failed' }" @click="filter = 'failed'">失败 <span class="seg-n mono">{{ stats.failed }}</span></button>
        </div>
        <div class="search-wrap">
          <Icon name="nav-help" :size="13" />
          <input v-model="search" placeholder="任务名 / 类型 / ID…" class="search-input" />
        </div>
      </header>

      <div v-if="error" class="state-msg err">{{ error }}</div>

      <!-- 空态 -->
      <div v-if="!loading && filteredItems.length === 0 && !error" class="empty">
        <div class="empty-icon"><Icon name="nav-history" :size="36" /></div>
        <div class="empty-title">{{ items.length === 0 ? '本节点尚未处理过任务' : '当前过滤无结果' }}</div>
        <div class="empty-hint" v-if="items.length === 0">平台一旦派单 · 任务完成后会出现在这里</div>
      </div>

      <!-- 列表表格 -->
      <table v-else-if="filteredItems.length > 0" class="hist-tbl">
        <thead>
          <tr>
            <th class="c-st"></th>
            <th class="c-task">任务</th>
            <th class="c-type">类型</th>
            <th class="c-time">完成于</th>
            <th class="c-dur num-col">耗时</th>
            <th class="c-rw num-col">奖励</th>
            <th class="c-chev"></th>
          </tr>
        </thead>
        <tbody>
          <template v-for="h in filteredItems" :key="h.task_id">
            <tr :class="['hist-row', { open: expanded.has(h.task_id) }]" @click="toggleExpand(h.task_id)">
              <td class="c-st"><span :class="['st-dot', statusCls(h)]" :title="statusLabel(h)" /></td>
              <td class="c-task">
                <div class="task-name" :title="h.task_name || h.task_type">
                  {{ h.task_name || `${h.task_type || '未命名'} 任务` }}
                </div>
                <div class="task-meta">
                  <span class="mono">#{{ h.task_id.slice(0, 8) }}</span>
                  <span v-if="h.attempts > 1" class="attempt">×{{ h.attempts }}</span>
                </div>
              </td>
              <td class="c-type">
                <span class="type-chip">
                  <Icon :name="iconForTaskType(h.task_type)" :size="12" />
                  {{ h.task_type || "—" }}
                </span>
              </td>
              <td class="c-time mono" :title="fmtFull(h.completed_at || h.dispatched_at)">{{ fmtTime(h.completed_at || h.dispatched_at) }}</td>
              <td class="c-dur mono num-col">{{ fmtElapsed(h.elapsed_ms) }}</td>
              <td :class="['c-rw mono num-col', h.reward > 0 ? 'rw-pos' : '']">
                {{ h.reward > 0 ? `+${fmtReward(h.reward)}` : "—" }}
              </td>
              <td class="c-chev">
                <Icon name="action-chevron" :size="13" :class="{ rot: expanded.has(h.task_id) }" />
              </td>
            </tr>
            <tr v-if="expanded.has(h.task_id)" class="detail-row">
              <td colspan="7">
                <div class="detail">
                  <div class="d-grid">
                    <div><span class="d-k">Task ID</span><span class="d-v mono">{{ h.task_id }}</span></div>
                    <div><span class="d-k">Workload</span><span class="d-v mono">{{ h.workload_id }}</span></div>
                    <div><span class="d-k">派发于</span><span class="d-v mono">{{ fmtFull(h.dispatched_at) }}</span></div>
                    <div><span class="d-k">完成于</span><span class="d-v mono">{{ fmtFull(h.completed_at) }}</span></div>
                    <div><span class="d-k">尝试次数</span><span class="d-v mono">{{ h.attempts }}</span></div>
                    <div><span class="d-k">状态</span><span :class="['d-v', statusCls(h)]">{{ statusLabel(h) }}</span></div>
                  </div>
                  <div v-if="h.error" class="d-block err-block">
                    <div class="d-block-head"><Icon name="status-failed" :size="12" /> 错误信息</div>
                    <pre>{{ h.error }}</pre>
                  </div>
                  <div v-if="h.output_preview" class="d-block">
                    <div class="d-block-head"><Icon name="action-copy" :size="12" /> 输出预览 (≤500B)</div>
                    <pre>{{ fmtOutput(h.output_preview) }}</pre>
                  </div>
                </div>
              </td>
            </tr>
          </template>
        </tbody>
      </table>
    </section>
  </div>
</template>

<style scoped>
.page { display: flex; flex-direction: column; gap: var(--sp-6); }

/* page head */
.page-head {
  display: flex; align-items: flex-end; justify-content: space-between; gap: var(--sp-5);
}
.page-title {
  margin: 0;
  font-size: var(--fs-xl);
  font-weight: var(--fw-semibold);
  letter-spacing: -0.02em;
  color: var(--c-fg);
}
.page-sub {
  margin: 2px 0 0;
  font-size: var(--fs-sm);
  color: var(--c-mute);
}
.page-sub code {
  font-family: ui-monospace, monospace;
  padding: 1px 6px;
  background: var(--c-bg-soft);
  border-radius: var(--r-xs);
  color: var(--c-fg-soft);
}

.btn-action {
  display: inline-flex; align-items: center; gap: 6px;
  padding: 6px 12px;
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-sm);
  color: var(--c-fg-soft);
  font-size: var(--fs-sm);
  font-weight: var(--fw-medium);
  transition: all var(--dur-base);
}
.btn-action:hover { border-color: var(--c-line-strong); color: var(--c-fg); background: var(--c-bg-soft); }
.btn-action:disabled { opacity: 0.5; cursor: not-allowed; }
.spin { animation: spin 1s linear infinite; }

.kpi-row {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: var(--sp-5);
}
@media (max-width: 1100px) { .kpi-row { grid-template-columns: repeat(3, 1fr); } }
@media (max-width: 700px)  { .kpi-row { grid-template-columns: repeat(2, 1fr); } }

/* panel */
.panel {
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md);
  overflow: hidden;
}
.p-head {
  display: flex; align-items: center; justify-content: space-between;
  padding: var(--sp-4) var(--sp-5);
  border-bottom: 1px solid var(--c-line);
  gap: var(--sp-5);
  flex-wrap: wrap;
}

/* seg control */
.seg {
  display: inline-flex;
  background: var(--c-bg-soft);
  border-radius: var(--r-sm);
  padding: 2px;
  gap: 2px;
}
.seg button {
  padding: 5px 12px;
  font-size: var(--fs-sm);
  font-weight: var(--fw-medium);
  color: var(--c-mute);
  border-radius: var(--r-xs);
  transition: all var(--dur-base);
  display: inline-flex; align-items: center; gap: 5px;
}
.seg button:hover { color: var(--c-fg-soft); }
.seg button.on { background: var(--c-bg-card); color: var(--c-fg); box-shadow: var(--sh-2); }
.seg-n {
  font-size: var(--fs-2xs);
  color: var(--c-faint);
  padding: 1px 5px;
  background: var(--c-bg);
  border-radius: var(--r-xs);
  font-variant-numeric: tabular-nums;
}
.seg button.on .seg-n { color: var(--c-mute); background: var(--c-bg-soft); }

.search-wrap {
  display: flex; align-items: center; gap: 6px;
  padding: 5px 10px;
  background: var(--c-bg-soft);
  border: 1px solid var(--c-line);
  border-radius: var(--r-sm);
  min-width: 240px;
  color: var(--c-mute);
}
.search-wrap:focus-within { border-color: var(--c-brand); }
.search-input {
  background: transparent; border: none; outline: none;
  color: var(--c-fg);
  flex: 1;
  font-size: var(--fs-sm);
}
.search-input::placeholder { color: var(--c-faint); }

/* 空 / 错 */
.state-msg {
  padding: var(--sp-5);
  text-align: center;
  font-size: var(--fs-sm);
  color: var(--c-mute);
}
.state-msg.err { color: var(--c-err); background: var(--c-err-soft); }
.empty {
  display: flex; flex-direction: column; align-items: center; justify-content: center;
  padding: 64px var(--sp-6);
  gap: 8px;
  color: var(--c-mute);
}
.empty-icon {
  width: 56px; height: 56px;
  display: flex; align-items: center; justify-content: center;
  border-radius: 50%;
  background: var(--c-bg-soft);
  color: var(--c-faint);
}
.empty-title { font-size: var(--fs-md); color: var(--c-fg-soft); font-weight: var(--fw-medium); }
.empty-hint  { font-size: var(--fs-xs); }

/* table */
.hist-tbl {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--fs-sm);
}
.hist-tbl thead {
  background: var(--c-bg-soft);
}
.hist-tbl th {
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--c-mute);
  padding: 8px var(--sp-5);
  text-align: left;
  border-bottom: 1px solid var(--c-line);
}
.hist-tbl th.num-col { text-align: right; }

.hist-row {
  cursor: pointer;
  transition: background var(--dur-base);
  border-bottom: 1px solid var(--c-line);
}
.hist-row:hover { background: var(--c-bg-soft); }
.hist-row.open { background: var(--c-bg-soft); }
.hist-row td {
  padding: 9px var(--sp-5);
  vertical-align: middle;
}
.num-col { text-align: right; font-variant-numeric: tabular-nums; }

.c-st { width: 14px; padding-right: 0 !important; }
.st-dot {
  display: inline-block;
  width: 8px; height: 8px;
  border-radius: 50%;
  background: var(--c-faint);
}
.st-dot.st-done    { background: var(--c-ok);   box-shadow: 0 0 6px rgba(63,185,80,0.4); }
.st-dot.st-fail    { background: var(--c-err);  box-shadow: 0 0 6px rgba(248,81,73,0.4); }
.st-dot.st-running { background: var(--c-info); animation: brand-pulse 1.6s ease-in-out infinite; }
.st-dot.st-queued  { background: var(--c-warn); }

.c-task { min-width: 220px; max-width: 360px; }
.task-name {
  font-weight: var(--fw-medium);
  color: var(--c-fg);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.task-meta {
  display: flex; align-items: center; gap: 6px;
  font-size: var(--fs-2xs);
  color: var(--c-mute);
  margin-top: 1px;
}
.attempt {
  padding: 1px 5px;
  background: var(--c-warn-soft);
  color: var(--c-warn);
  border-radius: var(--r-xs);
  font-weight: var(--fw-semibold);
}

.type-chip {
  display: inline-flex; align-items: center; gap: 4px;
  padding: 2px 8px;
  background: var(--c-bg-soft);
  border: 1px solid var(--c-line);
  border-radius: var(--r-pill);
  font-size: var(--fs-2xs);
  font-family: ui-monospace, monospace;
  color: var(--c-fg-soft);
}
.type-chip :deep(.qs-icon) { color: var(--c-brand); }

.c-time { color: var(--c-mute); font-size: var(--fs-xs); }
.c-dur  { color: var(--c-fg-soft); }
.c-rw   { color: var(--c-mute); }
.c-rw.rw-pos { color: var(--c-warn); font-weight: var(--fw-semibold); }

.c-chev { width: 22px; text-align: center; color: var(--c-faint); }
.c-chev :deep(.qs-icon) { transition: transform var(--dur-base); }
.c-chev :deep(.rot) { transform: rotate(180deg); color: var(--c-brand); }

/* detail row */
.detail-row td { padding: 0; background: var(--c-bg); }
.detail {
  padding: var(--sp-6);
  border-top: 1px solid var(--c-line);
  display: flex; flex-direction: column; gap: var(--sp-5);
  animation: slide-up var(--dur-slow) var(--ease-out);
}
.d-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: var(--sp-4) var(--sp-7);
}
.d-grid > div { display: flex; flex-direction: column; gap: 2px; }
.d-k {
  font-size: var(--fs-2xs);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--c-mute);
}
.d-v {
  font-size: var(--fs-sm);
  color: var(--c-fg-soft);
  word-break: break-all;
}
.d-v.st-done { color: var(--c-ok); }
.d-v.st-fail { color: var(--c-err); }

.d-block {
  border: 1px solid var(--c-line);
  border-radius: var(--r-sm);
  overflow: hidden;
}
.d-block-head {
  display: flex; align-items: center; gap: 6px;
  padding: 6px var(--sp-4);
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--c-mute);
  background: var(--c-bg-soft);
  border-bottom: 1px solid var(--c-line);
}
.d-block pre {
  margin: 0;
  padding: var(--sp-4) var(--sp-5);
  font-family: ui-monospace, monospace;
  font-size: var(--fs-xs);
  line-height: 1.55;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 240px;
  overflow: auto;
  color: var(--c-fg-soft);
}
.err-block { border-color: var(--c-err); }
.err-block .d-block-head { color: var(--c-err); background: var(--c-err-soft); border-color: var(--c-err); }
.err-block pre { color: #ffb3b0; }
</style>
