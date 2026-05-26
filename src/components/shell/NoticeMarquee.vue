<script setup lang="ts">
/**
 * 通知公告滚动条 · 顶部 StatusRail 之下
 *
 * - 数据来自 useOpSlots.notice
 * - 多条 notice 横向 marquee 自动滚动
 * - 单条 notice 文字不动 · 仅显示
 * - 点击触发 action (跟 AdSlot 一致 · external/internal/...)
 * - 关闭按钮 (closable) · 用户关后本会话不再出现
 */
import { computed, watch } from "vue"
import {
  useOpSlots,
  reportSlotEvent,
  type OpSlotItem,
} from "../../composables/useOpSlots"
// 2026-05-25 8.0.9 · 点击行为统一走 useAdAction · 支持 modal / embed_url / qr 等
import { useAdAction } from "../../composables/useAdAction"

const { notice, dismissNotice } = useOpSlots()
const { dispatchAction } = useAdAction()

const list = computed(() => notice.value)
const hasNotice = computed(() => list.value.length > 0)
const shouldMarquee = computed(() => list.value.length > 1)

async function onClick(item: OpSlotItem) {
  // 2026-05-25 8.0.9 · 统一 dispatcher (含 7 种 action_type + 埋点)
  if (item.action_type === "none") return
  await dispatchAction(item)
}

function onDismiss(item: OpSlotItem, ev: Event) {
  ev.stopPropagation()
  void reportSlotEvent(item.id, "dismiss")
  dismissNotice(item.id)
}

// ── impression 埋点 ──
watch(
  list,
  (items) => {
    for (const it of items) {
      void reportSlotEvent(it.id, "impression")
    }
  },
  { immediate: true, deep: false },
)
</script>

<template>
  <div v-if="hasNotice" class="notice-marquee">
    <span class="nm-icon">📢</span>
    <div class="nm-track-wrap">
      <div class="nm-track" :class="{ animate: shouldMarquee }">
        <span
          v-for="item in list"
          :key="item.id"
          class="nm-item"
          :class="{ clickable: item.action_type !== 'none' }"
          @click="onClick(item)"
        >
          <b class="nm-title">{{ item.title }}</b>
          <span v-if="item.subtitle" class="nm-sub"> · {{ item.subtitle }}</span>
          <span v-if="item.action_type !== 'none'" class="nm-cta">
            {{ item.action_label || "查看" }} →
          </span>
        </span>
        <!-- 重复一遍用于无缝滚动 -->
        <template v-if="shouldMarquee">
          <span
            v-for="item in list"
            :key="`dup-${item.id}`"
            class="nm-item"
            :class="{ clickable: item.action_type !== 'none' }"
            @click="onClick(item)"
            aria-hidden="true"
          >
            <b class="nm-title">{{ item.title }}</b>
            <span v-if="item.subtitle" class="nm-sub"> · {{ item.subtitle }}</span>
          </span>
        </template>
      </div>
    </div>
    <!-- 第一条 closable 才显示关闭按钮 -->
    <button
      v-if="list[0]?.closable"
      class="nm-close"
      aria-label="关闭"
      @click="onDismiss(list[0], $event)"
    >
      ×
    </button>
  </div>
</template>

<style scoped>
.notice-marquee {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 28px;
  padding: 0 16px;
  background: linear-gradient(
    90deg,
    color-mix(in srgb, var(--c-brand) 14%, transparent),
    color-mix(in srgb, var(--c-brand) 6%, transparent)
  );
  border-bottom: 1px solid color-mix(in srgb, var(--c-brand) 22%, transparent);
  font-size: 12px;
  color: var(--c-fg);
  overflow: hidden;
  flex-shrink: 0;
}

.nm-icon {
  flex-shrink: 0;
  font-size: 14px;
  filter: drop-shadow(0 0 4px var(--c-brand-glow));
}

.nm-track-wrap {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  position: relative;
  mask-image: linear-gradient(90deg, transparent, #000 6%, #000 94%, transparent);
}

.nm-track {
  display: flex;
  gap: 32px;
  white-space: nowrap;
  align-items: center;
  width: max-content;
}

.nm-track.animate {
  animation: marquee 35s linear infinite;
}

.nm-track:hover {
  animation-play-state: paused;
}

@keyframes marquee {
  0% {
    transform: translateX(0);
  }
  100% {
    transform: translateX(-50%);
  }
}

.nm-item {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.nm-item.clickable {
  cursor: pointer;
  transition: opacity 0.18s;
}

.nm-item.clickable:hover {
  opacity: 0.85;
}

.nm-title {
  color: var(--c-fg);
  font-weight: 600;
}

.nm-sub {
  color: var(--c-fg-soft);
  font-weight: 400;
}

.nm-cta {
  color: var(--c-brand);
  font-weight: 500;
  margin-left: 4px;
}

.nm-close {
  flex-shrink: 0;
  width: 18px;
  height: 18px;
  border-radius: 9px;
  border: none;
  background: transparent;
  color: var(--c-fg-soft);
  cursor: pointer;
  font-size: 14px;
  line-height: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0.55;
  transition: opacity 0.15s, background 0.15s;
  padding: 0;
}

.nm-close:hover {
  opacity: 1;
  background: color-mix(in srgb, var(--c-fg) 12%, transparent);
}
</style>
