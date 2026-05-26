<script setup lang="ts">
import { computed } from "vue"
import type { EarningPoint } from "../composables/useEarnings"

const props = defineProps<{ series: EarningPoint[] }>()

const W = 600
const H = 140
const PAD_X = 28
const PAD_Y = 16

const maxEarnings = computed(() => {
  const m = Math.max(0.5, ...props.series.map((p) => p.earnings))
  return Math.ceil(m * 10) / 10  // 向上取整到 0.1
})

const n = computed(() => props.series.length)

function pointX(i: number): number {
  if (n.value <= 1) return W / 2
  return PAD_X + (i / (n.value - 1)) * (W - PAD_X * 2)
}

function pointY(v: number): number {
  if (maxEarnings.value === 0) return H - PAD_Y
  const norm = v / maxEarnings.value
  return H - PAD_Y - norm * (H - PAD_Y * 2)
}

const linePath = computed(() => {
  if (n.value === 0) return ""
  return props.series
    .map((p, i) => `${i === 0 ? "M" : "L"}${pointX(i).toFixed(1)},${pointY(p.earnings).toFixed(1)}`)
    .join(" ")
})

const areaPath = computed(() => {
  if (n.value === 0) return ""
  const lp = props.series
    .map((p, i) => `${i === 0 ? "M" : "L"}${pointX(i).toFixed(1)},${pointY(p.earnings).toFixed(1)}`)
    .join(" ")
  const lastX = pointX(n.value - 1)
  const firstX = pointX(0)
  return `${lp} L${lastX.toFixed(1)},${H - PAD_Y} L${firstX.toFixed(1)},${H - PAD_Y} Z`
})

function dateLabel(d: string): string {
  // "2026-05-11" → "5/11"
  const parts = d.split("-")
  if (parts.length === 3) return `${Number(parts[1])}/${Number(parts[2])}`
  return d
}

const totalEarnings = computed(() =>
  props.series.reduce((sum, p) => sum + p.earnings, 0),
)
const totalCount = computed(() =>
  props.series.reduce((sum, p) => sum + p.count, 0),
)
</script>

<template>
  <div class="chart-card">
    <div class="chart-head">
      <div>
        <div class="chart-title">收益走势</div>
        <div class="chart-sub">最近 {{ series.length }} 天 · 共 +{{ totalEarnings.toFixed(2) }} EDG · {{ totalCount }} 个任务</div>
      </div>
    </div>
    <svg
      :viewBox="`0 0 ${W} ${H}`"
      preserveAspectRatio="none"
      class="chart-svg"
      role="img"
      aria-label="earnings chart"
    >
      <defs>
        <linearGradient id="earn-grad" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="var(--c-accent)" stop-opacity="0.45" />
          <stop offset="100%" stop-color="var(--c-accent)" stop-opacity="0.02" />
        </linearGradient>
      </defs>
      <!-- 横向网格线 -->
      <line :x1="PAD_X" :y1="PAD_Y" :x2="W - PAD_X" :y2="PAD_Y" stroke="var(--c-border)" stroke-width="1" />
      <line :x1="PAD_X" :y1="(H - PAD_Y * 2) / 2 + PAD_Y" :x2="W - PAD_X" :y2="(H - PAD_Y * 2) / 2 + PAD_Y" stroke="var(--c-bg-card)" stroke-width="1" />
      <line :x1="PAD_X" :y1="H - PAD_Y" :x2="W - PAD_X" :y2="H - PAD_Y" stroke="var(--c-border)" stroke-width="1" />
      <!-- 面积 -->
      <path :d="areaPath" fill="url(#earn-grad)" v-if="n > 0" />
      <!-- 折线 -->
      <path :d="linePath" fill="none" stroke="var(--c-accent)" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" v-if="n > 0" />
      <!-- 点 + tooltip 标签 -->
      <g v-for="(p, i) in series" :key="p.date">
        <circle :cx="pointX(i)" :cy="pointY(p.earnings)" r="3" fill="var(--c-accent)" />
        <text
          v-if="p.earnings > 0"
          :x="pointX(i)"
          :y="pointY(p.earnings) - 6"
          fill="var(--c-accent)"
          font-size="9"
          text-anchor="middle"
          font-family="ui-monospace, SFMono-Regular, monospace"
        >
          {{ p.earnings.toFixed(1) }}
        </text>
      </g>
      <!-- X 轴日期 -->
      <text
        v-for="(p, i) in series"
        :key="`x-${p.date}`"
        :x="pointX(i)"
        :y="H - 2"
        fill="#666"
        font-size="9"
        text-anchor="middle"
        font-family="ui-monospace, SFMono-Regular, monospace"
      >
        {{ dateLabel(p.date) }}
      </text>
      <!-- Y 轴最大值 -->
      <text :x="PAD_X - 4" :y="PAD_Y + 4" fill="#666" font-size="9" text-anchor="end" font-family="ui-monospace, SFMono-Regular, monospace">
        {{ maxEarnings.toFixed(1) }}
      </text>
      <text :x="PAD_X - 4" :y="H - PAD_Y + 3" fill="#666" font-size="9" text-anchor="end" font-family="ui-monospace, SFMono-Regular, monospace">
        0
      </text>
    </svg>
  </div>
</template>

<style scoped>
.chart-card {
  background: var(--c-bg-card);
  border: 1px solid #222;
  border-radius: 12px;
  padding: 16px 20px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.chart-head {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
}
.chart-title {
  font-size: 14px;
  font-weight: 600;
}
.chart-sub {
  font-size: 14.5px;
  color: var(--c-mute);
  margin-top: 2px;
}
.chart-svg {
  width: 100%;
  height: 140px;
  display: block;
}
</style>
