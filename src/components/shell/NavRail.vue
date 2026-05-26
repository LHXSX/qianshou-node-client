<script setup lang="ts">
/**
 * 左侧导航 Rail · 56px 收起 / 200px hover 展开 (向右浮出)
 *
 * 设计:
 *   - 默认只显示图标 (居中 · 17px svg · 文字隐藏)
 *   - hover 整个 rail 横向展开 (overlay 浮出 · 不挤压主区)
 *   - active 项: 左侧 2px brand 立条 · brand-soft 背景
 *   - 有 task 进行中时 dashboard 项右上角红点 + 数字
 *   - 底部小 footer 显示版本 · 极简
 */
import { useNav, type PageId } from "../../composables/useNav"
import { useTasks } from "../../composables/useTasks"
import { useConnection } from "../../composables/useConnection"
import Icon from "../Icon.vue"
import type { IconName } from "../../icons/paths"

const { page, goto } = useNav()
const { running, verifying } = useTasks()
const { snap } = useConnection()

interface NavItem {
  id: PageId
  icon: IconName
  label: string
}

interface NavGroup {
  label: string
  items: NavItem[]
}

const groups: NavGroup[] = [
  {
    label: "运行",
    items: [
      { id: "dashboard",     icon: "nav-dashboard", label: "算力驾舱" },
      { id: "history",       icon: "nav-history",   label: "任务历史" },
      { id: "earnings",      icon: "nav-earnings",  label: "收益统计" },
    ],
  },
  {
    label: "能力",
    items: [
      { id: "capabilities",  icon: "task-render",   label: "能力中心" },
      { id: "ai-capability", icon: "nav-ai",        label: "智能能力" },
      { id: "toolbox",       icon: "nav-toolbox",   label: "工具管理" },
      { id: "market",        icon: "nav-market",    label: "任务市场" },
    ],
  },
  {
    label: "节点",
    items: [
      { id: "device",        icon: "nav-device",    label: "设备信息" },
      { id: "throttle",      icon: "nav-throttle",  label: "算力调节" },
    ],
  },
  {
    label: "系统",
    items: [
      { id: "settings",      icon: "nav-settings",  label: "系统设置" },
      { id: "help",          icon: "nav-help",      label: "帮助中心" },
    ],
  },
]

const activeBadge = (id: PageId) => {
  if (id === "dashboard") return running.value.length + verifying.value.length
  return 0
}
</script>

<template>
  <aside class="nav-rail">
    <nav class="nr-list">
      <div class="nr-group" v-for="g in groups" :key="g.label">
        <div class="nr-group-label">{{ g.label }}</div>
        <button
          v-for="it in g.items" :key="it.id"
          class="nr-item"
          :class="{ active: page === it.id }"
          @click="goto(it.id)"
        >
          <span class="nr-rail-bar" />
          <span class="nr-icon-wrap">
            <Icon :name="it.icon" :size="18" />
          </span>
          <span class="nr-label">{{ it.label }}</span>
          <span v-if="activeBadge(it.id)" class="nr-badge">{{ activeBadge(it.id) }}</span>
        </button>
      </div>
    </nav>
    <div class="nr-foot">
      <div class="nr-brand">
        <span class="nr-brand-dot" />
        <span class="nr-brand-name">千手 EdgeCompute</span>
      </div>
      <div class="nr-meta">
        <span class="mono">v{{ snap.client_version }}</span>
        <span class="sep">·</span>
        <span>分布式算力</span>
      </div>
    </div>
  </aside>
</template>

<style scoped>
.nav-rail {
  width: var(--rail-w);
  background: var(--c-bg-elev-1);
  border-right: 1px solid var(--c-line);
  display: flex;
  flex-direction: column;
  padding: var(--sp-6) var(--sp-5);
  flex-shrink: 0;
  position: relative;
  z-index: 5;
  overflow: hidden;
}

/* list */
.nr-list { display: flex; flex-direction: column; gap: var(--sp-7); flex: 1; overflow-y: auto; }

.nr-group { display: flex; flex-direction: column; gap: 2px; }
.nr-group-label {
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  text-transform: uppercase;
  letter-spacing: 0.1em;
  color: var(--c-faint);
  padding: 0 var(--sp-4) var(--sp-3);
}

.nr-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: var(--sp-5);
  height: 38px;
  padding: 0 var(--sp-4);
  color: var(--c-fg-soft);
  border-radius: var(--r-sm);
  transition:
    background var(--dur-base),
    color var(--dur-base);
  white-space: nowrap;
  text-align: left;
  font-size: var(--fs-sm);
  font-weight: var(--fw-medium);
}
.nr-item:hover { background: var(--c-bg-soft); color: var(--c-fg); }
.nr-item.active {
  color: var(--c-brand-2);
  background: var(--c-brand-soft);
}

.nr-rail-bar {
  position: absolute;
  left: calc(-1 * var(--sp-5) - 1px);
  top: 8px; bottom: 8px;
  width: 3px;
  border-radius: 0 3px 3px 0;
  background: transparent;
  transition: background var(--dur-base);
}
.nr-item.active .nr-rail-bar { background: var(--c-brand); box-shadow: 0 0 8px var(--c-brand-glow); }

.nr-icon-wrap {
  width: 20px; height: 20px;
  display: flex; align-items: center; justify-content: center;
  flex-shrink: 0;
  opacity: 0.78;
  transition: opacity var(--dur-base);
}
.nr-item:hover .nr-icon-wrap { opacity: 1; }
.nr-item.active .nr-icon-wrap { opacity: 1; color: var(--c-brand); }

.nr-label {
  flex: 1;
  letter-spacing: 0.01em;
}

.nr-badge {
  background: var(--c-brand);
  color: #fff;
  font-size: var(--fs-2xs);
  font-weight: var(--fw-bold);
  font-family: ui-monospace, monospace;
  min-width: 18px;
  height: 18px;
  padding: 0 5px;
  border-radius: var(--r-pill);
  display: flex; align-items: center; justify-content: center;
  flex-shrink: 0;
}

/* footer */
.nr-foot {
  padding-top: var(--sp-5);
  margin-top: var(--sp-4);
  border-top: 1px solid var(--c-line);
  display: flex; flex-direction: column; gap: 4px;
}
.nr-brand { display: flex; align-items: center; gap: 7px; }
.nr-brand-dot {
  width: 8px; height: 8px;
  background: var(--c-brand);
  border-radius: 50%;
  box-shadow: 0 0 8px var(--c-brand-glow);
}
.nr-brand-name {
  font-size: var(--fs-sm);
  font-weight: var(--fw-semibold);
  color: var(--c-fg);
  letter-spacing: -0.01em;
}
.nr-meta {
  font-size: var(--fs-2xs);
  color: var(--c-mute);
  display: flex; gap: 5px; align-items: center;
}
.nr-meta .sep { color: var(--c-faint); }
</style>
