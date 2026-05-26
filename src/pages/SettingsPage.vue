<script setup lang="ts">
import SettingsPanel from "../components/SettingsPanel.vue"
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1 class="page-title">系统设置</h1>
        <p class="page-sub">开机自启 · 更新检查 · 数据目录 · 诊断信息</p>
      </div>
    </header>
    <SettingsPanel inline />
  </div>
</template>

<style scoped>
.page { display: flex; flex-direction: column; gap: var(--sp-6); }
.page-head { display: flex; align-items: flex-end; justify-content: space-between; }
.page-title { margin: 0; font-size: var(--fs-xl); font-weight: var(--fw-semibold); letter-spacing: -0.02em; color: var(--c-fg); }
.page-sub { margin: 2px 0 0; font-size: var(--fs-sm); color: var(--c-mute); }
</style>
