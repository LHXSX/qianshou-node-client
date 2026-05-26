<script setup lang="ts">
/**
 * 通用 SVG icon · 一处定义 · 全 app 共用
 *
 * 用法:
 *   <Icon name="status-running" />
 *   <Icon name="nav-dashboard" :size="20" />
 *   <Icon name="task-image" color="var(--c-accent)" />
 *
 * 颜色: 默认 currentColor (跟父级文字色) · color prop 覆盖
 * 大小: 默认 16px · size prop 覆盖
 */
import { ICON_PATHS, type IconName } from "../icons/paths"

const props = withDefaults(defineProps<{
  name: IconName
  size?: number | string
  color?: string
  strokeWidth?: number
}>(), {
  size: 16,
  strokeWidth: 1.6,
})

const pathHtml = ICON_PATHS[props.name] || ""
</script>

<template>
  <svg
    :width="size"
    :height="size"
    viewBox="0 0 24 24"
    fill="none"
    :stroke="color || 'currentColor'"
    :stroke-width="strokeWidth"
    stroke-linecap="round"
    stroke-linejoin="round"
    class="qs-icon"
    aria-hidden="true"
    v-html="pathHtml"
  />
</template>

<style scoped>
.qs-icon {
  display: inline-block;
  vertical-align: middle;
  flex-shrink: 0;
}
</style>
