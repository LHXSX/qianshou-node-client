<script setup lang="ts">
/**
 * Panel · 全 app 通用面板容器
 *
 * 设计语言:
 *   - 1px 几乎不可见的 line border
 *   - 8px 圆角
 *   - 标题栏: uppercase label (mute) + 可选 actions slot 右对齐
 *   - 极简 · 不张扬 · 让数据自己说话
 *
 * 用法:
 *   <Panel title="任务流" hint="本节点实时">
 *     <template #actions> <button>...</button> </template>
 *     <内容/>
 *   </Panel>
 *   <Panel> 内容 </Panel>   // 无 head
 */
withDefaults(defineProps<{
  title?: string
  hint?: string
  noPad?: boolean      // body 是否取消内边距 (列表/表格自带 padding 时)
  bleed?: boolean      // body 满铺 (无 padding · 无 border)
}>(), {
  noPad: false,
  bleed: false,
})
</script>

<template>
  <section class="panel" :class="{ bleed }">
    <header v-if="title" class="p-head">
      <div class="p-title-wrap">
        <span class="p-title">{{ title }}</span>
        <span v-if="hint" class="p-hint">{{ hint }}</span>
      </div>
      <div class="p-actions">
        <slot name="actions" />
      </div>
    </header>
    <div class="p-body" :class="{ 'no-pad': noPad || !!$slots.default && false }">
      <slot />
    </div>
  </section>
</template>

<style scoped>
.panel {
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}
.panel.bleed { background: transparent; border: none; }

.p-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px var(--sp-5);
  border-bottom: 1px solid var(--c-line);
  min-height: 36px;
}
.p-title-wrap { display: flex; align-items: baseline; gap: var(--sp-4); min-width: 0; }
.p-title {
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--c-fg-soft);
  white-space: nowrap;
}
.p-hint {
  font-size: var(--fs-xs);
  color: var(--c-mute);
  white-space: nowrap;
  overflow: hidden; text-overflow: ellipsis;
}
.p-actions { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }

.p-body { padding: var(--sp-5); }
.p-body.no-pad { padding: 0; }
.panel.bleed .p-body { padding: 0; }
</style>
