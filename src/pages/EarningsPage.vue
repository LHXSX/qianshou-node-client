<script setup lang="ts">
/**
 * 收益统计 · 次时代重设计 (2026-05-21)
 *
 * - 顶部 4 张 KPI (余额 / 累计 / 今日 / 本周)
 * - 7日折线 panel
 * - 任务流水 list (复用 history 视觉)
 */
import { computed } from "vue"
import EarningsChart from "../components/EarningsChart.vue"
import StatCard from "../components/dashboard/StatCard.vue"
import MiniSpark from "../components/dashboard/MiniSpark.vue"
import Icon from "../components/Icon.vue"
import { useEarnings } from "../composables/useEarnings"
import { useAccount } from "../composables/useAccount"

const { series } = useEarnings(7)
const { account, history } = useAccount()

const sevenSeries = computed(() => series.value.map((p) => Number(p.earnings || 0)))
const todayEarn = computed(() => sevenSeries.value[sevenSeries.value.length - 1] || 0)
const weekEarn = computed(() => sevenSeries.value.reduce((a, b) => a + b, 0))
const yesterdayEarn = computed(() => sevenSeries.value[sevenSeries.value.length - 2] || 0)
const todayTrend = computed(() => {
  if (!yesterdayEarn.value) return 0
  return ((todayEarn.value - yesterdayEarn.value) / yesterdayEarn.value) * 100
})

function fmtTime(s?: string): string {
  if (!s) return "—"
  try {
    return new Date(s).toLocaleString("zh-CN", { hour12: false, month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })
  } catch { return s }
}
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1 class="page-title">收益统计</h1>
        <p class="page-sub">本账户全部收益流水 · 实时更新</p>
      </div>
    </header>

    <!-- KPI 行 -->
    <div class="kpi-row">
      <StatCard
        label="可用余额"
        :value="account?.balance?.toFixed(2) ?? '—'"
        unit="EDG"
        accent="brand"
        icon="coin"
        hint="可立即提现"
      />
      <StatCard
        label="累计收益"
        :value="account?.total_earnings?.toFixed(2) ?? '—'"
        unit="EDG"
        accent="ok"
        icon="status-done"
        :hint="account ? `完成 ${account.completed_tasks} 个任务` : ''"
      />
      <StatCard
        label="今日收益"
        :value="todayEarn.toFixed(2)"
        unit="EDG"
        accent="warn"
        icon="spark"
        :trend="todayTrend"
      />
      <StatCard
        label="近 7 日"
        :value="weekEarn.toFixed(2)"
        unit="EDG"
        accent="brand"
        icon="nav-earnings"
      >
        <MiniSpark v-if="sevenSeries.length" :series="sevenSeries" :width="100" :height="24" />
      </StatCard>
    </div>

    <!-- 图表 -->
    <section class="panel">
      <header class="p-head">
        <span class="p-title">7 日收益曲线</span>
        <span class="p-meta mono">{{ series.length }} 个数据点</span>
      </header>
      <div class="p-body">
        <EarningsChart :series="series" />
      </div>
    </section>

    <!-- 流水列表 -->
    <section class="panel">
      <header class="p-head">
        <span class="p-title">任务流水</span>
        <span class="p-meta">最近 {{ history.length }} 条</span>
      </header>
      <div v-if="history.length === 0" class="empty">
        <Icon name="nav-earnings" :size="32" />
        <div>暂无任务流水</div>
      </div>
      <table v-else class="hist-tbl">
        <thead>
          <tr>
            <th class="c-st"></th>
            <th class="c-cmd">命令</th>
            <th class="c-time">完成时间</th>
            <th class="c-dur num-col">耗时</th>
            <th class="c-rw num-col">奖励</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="h in history" :key="h.task_id" class="hist-row">
            <td class="c-st"><span :class="['st-dot', h.status === 'ok' ? 'st-done' : 'st-fail']" /></td>
            <td class="c-cmd"><code class="mono">{{ h.cmd || "(no cmd)" }}</code></td>
            <td class="c-time mono">{{ fmtTime(h.completed_at) }}</td>
            <td class="c-dur mono num-col">{{ h.elapsed_ms }}ms</td>
            <td class="c-rw mono num-col" :class="{ 'rw-pos': h.status === 'ok' }">
              {{ h.status === "ok" ? `+${h.reward.toFixed(3)}` : "—" }}
            </td>
          </tr>
        </tbody>
      </table>
    </section>
  </div>
</template>

<style scoped>
.page { display: flex; flex-direction: column; gap: var(--sp-6); }

.page-head { display: flex; align-items: flex-end; justify-content: space-between; }
.page-title { margin: 0; font-size: var(--fs-xl); font-weight: var(--fw-semibold); letter-spacing: -0.02em; color: var(--c-fg); }
.page-sub { margin: 2px 0 0; font-size: var(--fs-sm); color: var(--c-mute); }

.kpi-row {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--sp-5);
}
@media (max-width: 1100px) { .kpi-row { grid-template-columns: repeat(2, 1fr); } }

.panel {
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md);
  overflow: hidden;
}
.p-head {
  display: flex; align-items: center; justify-content: space-between;
  padding: 10px var(--sp-5);
  border-bottom: 1px solid var(--c-line);
}
.p-title {
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--c-fg-soft);
}
.p-meta { font-size: var(--fs-xs); color: var(--c-mute); }
.p-body { padding: var(--sp-5); }

.empty {
  padding: 56px;
  text-align: center;
  color: var(--c-mute);
  font-size: var(--fs-sm);
  display: flex; flex-direction: column; align-items: center; gap: 8px;
}
.empty :deep(.qs-icon) { color: var(--c-faint); }

/* table 复用 history 风格 */
.hist-tbl { width: 100%; border-collapse: collapse; font-size: var(--fs-sm); }
.hist-tbl thead { background: var(--c-bg-soft); }
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
.hist-row td {
  padding: 8px var(--sp-5);
  vertical-align: middle;
  border-bottom: 1px solid var(--c-line);
}
.hist-row:hover { background: var(--c-bg-soft); }
.num-col { text-align: right; font-variant-numeric: tabular-nums; }

.c-st { width: 14px; padding-right: 0 !important; }
.st-dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; background: var(--c-faint); }
.st-dot.st-done { background: var(--c-ok); box-shadow: 0 0 6px rgba(63,185,80,0.4); }
.st-dot.st-fail { background: var(--c-err); box-shadow: 0 0 6px rgba(248,81,73,0.4); }

.c-cmd code {
  font-family: ui-monospace, monospace;
  color: var(--c-fg);
  display: inline-block;
  max-width: 420px;
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.c-time { color: var(--c-mute); font-size: var(--fs-xs); }
.c-dur  { color: var(--c-fg-soft); }
.c-rw   { color: var(--c-mute); }
.c-rw.rw-pos { color: var(--c-warn); font-weight: var(--fw-semibold); }
</style>
