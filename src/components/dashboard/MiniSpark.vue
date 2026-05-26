<script setup lang="ts">
/**
 * 内嵌迷你折线 · 用于 StatCard 的 hint slot
 * 纯 SVG · 无库依赖
 */
import { computed } from "vue"

const _uid = Math.random().toString(36).slice(2, 8)

const props = withDefaults(defineProps<{
  series: number[]
  width?: number
  height?: number
  color?: string
  fill?: boolean
}>(), {
  width: 120,
  height: 28,
  fill: true,
})

const path = computed(() => {
  const n = props.series.length
  if (n === 0) return { line: "", area: "" }
  const max = Math.max(...props.series, 0.001)
  const min = Math.min(...props.series, 0)
  const range = max - min || 1
  const stepX = props.width / Math.max(n - 1, 1)
  const pts = props.series.map((v, i) => {
    const x = i * stepX
    const y = props.height - ((v - min) / range) * (props.height - 4) - 2
    return [x, y] as const
  })
  const line = pts.map(([x, y], i) => (i === 0 ? `M ${x} ${y}` : `L ${x} ${y}`)).join(" ")
  const area = `${line} L ${props.width} ${props.height} L 0 ${props.height} Z`
  return { line, area }
})
const lineColor = computed(() => props.color || "var(--c-brand)")
</script>

<template>
  <svg :width="width" :height="height" class="mini-spark" :viewBox="`0 0 ${width} ${height}`">
    <defs>
      <linearGradient :id="`spark-grad-${_uid}`" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" :stop-color="lineColor" stop-opacity="0.32" />
        <stop offset="100%" :stop-color="lineColor" stop-opacity="0" />
      </linearGradient>
    </defs>
    <path v-if="fill && path.area" :d="path.area" :fill="`url(#spark-grad-${_uid})`" />
    <path :d="path.line" fill="none" :stroke="lineColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round" />
  </svg>
</template>

<style scoped>
.mini-spark { display: block; }
</style>
