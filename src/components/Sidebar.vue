<script setup lang="ts">
import { useNav, type PageId } from "../composables/useNav"
import { useTasks } from "../composables/useTasks"
import Icon from "./Icon.vue"
import type { IconName } from "../icons/paths"

const { page, goto } = useNav()
const { running, verifying } = useTasks()

interface NavItem {
  id: PageId
  icon: IconName
  label: string
}

const items: NavItem[] = [
  { id: "dashboard",     icon: "nav-dashboard", label: "算力驾舱" },
  { id: "market",        icon: "nav-market",    label: "任务市场" },
  { id: "history",       icon: "nav-history",   label: "任务历史" },
  { id: "earnings",      icon: "nav-earnings",  label: "收益统计" },
  { id: "throttle",      icon: "nav-throttle",  label: "算力调节" },
  { id: "device",        icon: "nav-device",    label: "设备信息" },
  { id: "toolbox",       icon: "nav-toolbox",   label: "工具管理" },
  { id: "ai-capability", icon: "nav-ai",        label: "智能能力" },
  { id: "settings",      icon: "nav-settings",  label: "系统设置" },
  { id: "help",          icon: "nav-help",      label: "帮助中心" },
]
</script>

<template>
  <aside class="sidebar">
    <nav class="nav">
      <button
        v-for="it in items"
        :key="it.id"
        class="nav-item"
        :class="{ active: page === it.id }"
        @click="goto(it.id)"
      >
        <Icon :name="it.icon" :size="17" class="nav-icon" />
        <span class="nav-label">{{ it.label }}</span>
        <span
          v-if="it.id === 'dashboard' && (running.length + verifying.length) > 0"
          class="nav-badge"
        >
          {{ running.length + verifying.length }}
        </span>
      </button>
    </nav>
    <div class="footer">
      <div class="ec-line">千手 · EdgeCompute</div>
      <div class="ec-tip">分布式算力网络</div>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  width: 180px;
  background: var(--c-bg-soft);
  border-right: 1px solid var(--c-border);
  display: flex;
  flex-direction: column;
  padding: 12px 8px;
  box-sizing: border-box;
  flex-shrink: 0;
}
.nav {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.nav-item {
  display: flex;
  align-items: center;
  gap: 11px;
  padding: 10px 12px;
  background: transparent;
  border: 1px solid transparent;
  border-radius: 7px;
  color: var(--c-mute);
  font-size: 13.5px;
  cursor: pointer;
  text-align: left;
  font-family: inherit;
  transition: all 0.12s;
  position: relative;
}
.nav-item:hover {
  background: rgba(255, 255, 255, 0.03);
  color: var(--c-fg);
}
.nav-item.active {
  background: linear-gradient(135deg, rgba(10, 132, 255, 0.15), rgba(90, 60, 255, 0.1));
  border-color: rgba(10, 132, 255, 0.3);
  color: var(--c-fg);
}
.nav-item.active::before {
  content: "";
  position: absolute;
  left: -8px;
  top: 50%;
  transform: translateY(-50%);
  width: 3px;
  height: 16px;
  background: var(--c-accent);
  border-radius: 0 2px 2px 0;
}
.nav-icon {
  width: 17px;
  height: 17px;
  flex-shrink: 0;
  opacity: 0.78;
  transition: opacity 0.15s, color 0.15s;
}
.nav-item:hover .nav-icon { opacity: 1; }
.nav-item.active .nav-icon {
  opacity: 1;
  color: var(--c-accent);
}
.nav-label {
  flex: 1;
  letter-spacing: 0.02em;
}
.nav-badge {
  font-size: 14px;
  font-weight: 600;
  background: var(--c-accent);
  color: #fff;
  padding: 1px 5px;
  border-radius: 8px;
  min-width: 14px;
  text-align: center;
  font-variant-numeric: tabular-nums;
}
.footer {
  margin-top: auto;
  padding: 12px 10px 6px;
  border-top: 1px solid var(--c-border);
  font-size: 14.5px;
  line-height: 1.4;
}
.ec-line {
  color: var(--c-mute);
  font-weight: 500;
}
.ec-tip {
  color: var(--c-mute);
  opacity: 0.55;
  font-size: 14px;
  margin-top: 2px;
}
</style>
