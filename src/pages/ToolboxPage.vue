<script setup lang="ts">
/**
 * 工具管理 v2 (2026-05-21)
 *
 * 只剩 RuntimePanel · 旧的 BundleTable 已移除
 * 装完自动点亮 · 不再表格 · 不再误判
 */
import RuntimePanel from "../components/RuntimePanel.vue"
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1 class="page-title">工具管理</h1>
        <p class="page-sub">一键安装运行时 · 装好自动点亮 · 后端镜像源动态分发</p>
      </div>
    </header>

    <RuntimePanel />
  </div>
</template>

<style scoped>
.page { display: flex; flex-direction: column; gap: var(--sp-6); }
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
</style>
