<script setup lang="ts">
/**
 * Hero 统计卡 · 全 Dashboard / Earnings / Device 通用
 *
 * 设计:
 *   - 顶部 label uppercase (mute)
 *   - 中央大数字 (mono 30px)
 *   - 数字旁可放 trend 或 unit
 *   - 底部 hint 行 (子指标 · mute)
 *   - 左边一条 3px 状态色立条 · 数据更新时闪一下
 */
import Icon from "../Icon.vue"
import type { IconName } from "../../icons/paths"

withDefaults(defineProps<{
  label: string
  value: string | number
  unit?: string
  hint?: string
  icon?: IconName
  trend?: number             // % 增长 (正绿负红)
  accent?: "brand" | "ok" | "warn" | "err" | "mute"
  loading?: boolean
}>(), {
  accent: "brand",
  loading: false,
})
</script>

<template>
  <div :class="['stat-card', `ac-${accent}`]">
    <div class="sc-bar" />
    <header class="sc-head">
      <span class="sc-label">{{ label }}</span>
      <Icon v-if="icon" :name="icon" :size="15" class="sc-icon" />
    </header>
    <div class="sc-value-row">
      <span class="sc-value mono">{{ loading ? "—" : value }}</span>
      <span v-if="unit" class="sc-unit">{{ unit }}</span>
      <span v-if="trend !== undefined && !loading" :class="['sc-trend', trend > 0 ? 'up' : trend < 0 ? 'dn' : 'flat']">
        {{ trend > 0 ? "↑" : trend < 0 ? "↓" : "·" }} {{ Math.abs(trend).toFixed(1) }}%
      </span>
    </div>
    <div v-if="hint || $slots.default" class="sc-hint">
      <slot>{{ hint }}</slot>
    </div>
  </div>
</template>

<style scoped>
.stat-card {
  position: relative;
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md);
  padding: var(--sp-6) var(--sp-6) var(--sp-5);
  overflow: hidden;
  transition: border-color var(--dur-base), transform var(--dur-base);
}
.stat-card:hover { border-color: var(--c-line-strong); }

.sc-bar {
  position: absolute;
  left: 0; top: 0; bottom: 0;
  width: 3px;
  background: var(--c-line);
  transition: background var(--dur-base);
}
.ac-brand .sc-bar { background: var(--c-brand); }
.ac-ok    .sc-bar { background: var(--c-ok); }
.ac-warn  .sc-bar { background: var(--c-warn); }
.ac-err   .sc-bar { background: var(--c-err); }
.ac-mute  .sc-bar { background: var(--c-line-strong); }

.sc-head {
  display: flex; align-items: center; justify-content: space-between;
  margin-bottom: var(--sp-5);
}
.sc-label {
  font-size: var(--fs-xs);
  font-weight: var(--fw-semibold);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--c-mute);
}
.sc-icon { color: var(--c-mute); }
.ac-brand .sc-icon { color: var(--c-brand); }
.ac-ok    .sc-icon { color: var(--c-ok); }
.ac-warn  .sc-icon { color: var(--c-warn); }
.ac-err   .sc-icon { color: var(--c-err); }

.sc-value-row {
  display: flex; align-items: baseline; gap: 8px;
  margin-bottom: var(--sp-3);
}
.sc-value {
  font-size: var(--fs-2xl);
  font-weight: var(--fw-bold);
  color: var(--c-fg);
  letter-spacing: -0.02em;
  line-height: 1.1;
}
.ac-ok    .sc-value { color: var(--c-ok); }
.ac-err   .sc-value { color: var(--c-err); }
.sc-unit {
  font-size: var(--fs-sm);
  color: var(--c-mute);
  font-weight: var(--fw-medium);
  letter-spacing: 0.04em;
}
.sc-trend {
  font-size: var(--fs-xs);
  font-weight: var(--fw-semibold);
  padding: 2px 7px;
  border-radius: var(--r-xs);
  font-family: ui-monospace, monospace;
}
.sc-trend.up   { color: var(--c-ok);   background: var(--c-ok-soft); }
.sc-trend.dn   { color: var(--c-err);  background: var(--c-err-soft); }
.sc-trend.flat { color: var(--c-mute); background: var(--c-bg-soft); }

.sc-hint {
  font-size: var(--fs-xs);
  color: var(--c-mute);
  line-height: 1.45;
  min-height: 18px;
}
</style>
