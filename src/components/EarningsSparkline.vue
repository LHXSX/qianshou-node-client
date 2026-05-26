<script setup lang="ts">
import { computed } from "vue"
import type { EarningPoint } from "../composables/useEarnings"

const props = defineProps<{ series: EarningPoint[]; days?: number }>()

const W = 380
const H = 70
const PAD_X = 6
const PAD_Y = 6

const total = computed(() =>
  props.series.reduce((s, p) => s + p.earnings, 0)
)
const totalCount = computed(() =>
  props.series.reduce((s, p) => s + p.count, 0)
)

const points = computed(() => {
  const s = props.series
  if (!s.length) return []
  const maxE = Math.max(...s.map((p) => p.earnings), 0.001)
  const w = W - PAD_X * 2
  const h = H - PAD_Y * 2
  const step = s.length > 1 ? w / (s.length - 1) : 0
  return s.map((p, i) => ({
    x: PAD_X + i * step,
    y: PAD_Y + h * (1 - p.earnings / maxE),
    e: p.earnings,
    d: p.date,
  }))
})

const linePath = computed(() => {
  const pts = points.value
  if (!pts.length) return ""
  return pts.map((p, i) => `${i === 0 ? "M" : "L"}${p.x},${p.y}`).join(" ")
})
const areaPath = computed(() => {
  const pts = points.value
  if (!pts.length) return ""
  const line = pts.map((p, i) => `${i === 0 ? "M" : "L"}${p.x},${p.y}`).join(" ")
  return `${line} L${pts[pts.length - 1].x},${H - PAD_Y} L${pts[0].x},${H - PAD_Y} Z`
})
</script>

<template>
  <div class="spark">
    <div class="spark-head">
      <div class="title">
        <span class="icon">📈</span>
        <span class="t">收益趋势</span>
      </div>
      <div class="sub">
        最近 {{ props.days ?? props.series.length }} 天 ·
        <span class="hi">+{{ total.toFixed(2) }}</span> EDG ·
        {{ totalCount }} 个任务
      </div>
    </div>
    <svg
      :viewBox="`0 0 ${W} ${H}`"
      class="svg"
      preserveAspectRatio="none"
    >
      <defs>
        <linearGradient id="sparkGrad" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="rgb(10, 132, 255)" stop-opacity="0.35" />
          <stop offset="100%" stop-color="rgb(10, 132, 255)" stop-opacity="0" />
        </linearGradient>
      </defs>
      <path :d="areaPath" fill="url(#sparkGrad)" />
      <path :d="linePath" fill="none" stroke="rgb(10, 132, 255)" stroke-width="1.5" />
      <circle
        v-for="p in points"
        :key="p.d"
        :cx="p.x"
        :cy="p.y"
        r="2"
        fill="rgb(10, 132, 255)"
      >
        <title>{{ p.d }}: +{{ p.e.toFixed(2) }} EDG</title>
      </circle>
    </svg>
  </div>
</template>

<style scoped>
.spark {
  background: var(--c-bg-card);
  border: 1px solid var(--c-border);
  border-radius: 10px;
  padding: 10px 14px 8px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.spark-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13.5px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--c-fg);
}
.icon {
  opacity: 0.8;
}
.sub {
  font-size: 14px;
  color: var(--c-mute);
}
.sub .hi {
  color: var(--c-ok);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
.svg {
  width: 100%;
  height: 70px;
  display: block;
}
</style>
