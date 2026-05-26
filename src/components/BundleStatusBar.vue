<script setup lang="ts">
import { computed, onMounted, ref } from "vue"
import { useBundles } from "../composables/useBundles"
import { useNav } from "../composables/useNav"

const { stats, loaded, detecting, load, bundles, bundleStatus } = useBundles()
const { goto } = useNav()

const dismissed = ref(false)

onMounted(() => {
  load()
})

const visible = computed(() => {
  if (dismissed.value) return false
  if (!loaded.value) return false
  return stats.value.missing > 0
})

const missingBundleNames = computed(() => {
  return (bundles.value as any[])
    .filter((b: any) => bundleStatus(b) !== "ready")
    .map((b: any) => `${b.icon} ${b.name}`)
    .join(" · ")
})
</script>

<template>
  <div v-if="detecting && !loaded" class="bundle-bar detecting">
    <span class="b-spin">⟳</span>
    <span class="b-text">正在探测本机环境...</span>
  </div>

  <div v-else-if="visible" class="bundle-bar warn">
    <span class="b-icon">⚠️</span>
    <div class="b-content">
      <div class="b-title">
        未安装 <strong>{{ stats.missing }}</strong> 个工具包，
        当前仅能接 <strong>{{ stats.ttReady }} / {{ stats.ttTotal }}</strong> 种任务类型
      </div>
      <div class="b-sub">缺失：{{ missingBundleNames }}</div>
    </div>
    <button class="b-btn-primary" @click="goto('toolbox')">
      前往安装
      <span class="arrow">→</span>
    </button>
    <button class="b-btn-close" @click="dismissed = true" title="本次不再提醒">×</button>
  </div>

  <div v-else-if="loaded && stats.missing === 0" class="bundle-bar ok">
    <span class="b-icon">✓</span>
    <span class="b-text">
      全部 {{ stats.total }} 个工具包已就绪 ·
      支持全部 <strong>{{ stats.ttTotal }}</strong> 种任务类型
    </span>
    <button class="b-btn-link" @click="goto('toolbox')">查看详情 →</button>
  </div>
</template>

<style scoped>
.bundle-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 11px 16px;
  border-radius: var(--r-md);
  font-size: 13.5px;
  margin-bottom: 14px;
  border: 1px solid transparent;
  animation: slide-in 0.25s ease-out;
}

@keyframes slide-in {
  from { transform: translateY(-6px); opacity: 0; }
  to { transform: translateY(0); opacity: 1; }
}

.bundle-bar.warn {
  background: linear-gradient(90deg, var(--c-warn-soft) 0%, var(--c-bg-card) 100%);
  border-color: var(--c-warn);
  color: var(--c-fg);
}
.bundle-bar.ok {
  background: linear-gradient(90deg, var(--c-ok-soft) 0%, var(--c-bg-card) 100%);
  border-color: var(--c-ok-soft);
  color: var(--c-fg-soft);
}
.bundle-bar.detecting {
  background: var(--c-bg-card);
  border-color: var(--c-border);
  color: var(--c-mute);
}

.b-icon {
  font-size: 18px;
  flex-shrink: 0;
}
.b-spin {
  font-size: 16px;
  animation: spin 1s linear infinite;
}
@keyframes spin {
  to { transform: rotate(360deg); }
}

.b-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.b-title {
  font-size: 13.5px;
  font-weight: 500;
}
.b-title strong {
  color: var(--c-warn);
  font-weight: 700;
}
.b-sub {
  font-size: 12px;
  color: var(--c-mute);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.b-text {
  flex: 1;
  font-size: 13.5px;
}
.b-text strong {
  color: var(--c-ok);
  font-weight: 700;
}

.b-btn-primary {
  background: var(--c-warn);
  color: #fff;
  border: none;
  padding: 7px 14px;
  border-radius: var(--r-sm);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  white-space: nowrap;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  transition: opacity 0.15s;
}
.b-btn-primary:hover { opacity: 0.9; }
.b-btn-primary .arrow { font-size: 14px; }

.b-btn-link {
  background: transparent;
  color: var(--c-ok);
  border: none;
  font-size: 13px;
  cursor: pointer;
  padding: 4px 8px;
}
.b-btn-link:hover { text-decoration: underline; }

.b-btn-close {
  background: transparent;
  color: var(--c-mute);
  border: none;
  font-size: 18px;
  cursor: pointer;
  padding: 0 4px;
  line-height: 1;
}
.b-btn-close:hover { color: var(--c-fg); }
</style>
