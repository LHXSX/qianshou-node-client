<script setup lang="ts">
/**
 * 通用广告位组件 · 一套样式适配多位置
 *
 * 用法:
 *   <AdSlot slot-key="banner" layout="banner" />        Dashboard 内大 banner
 *   <AdSlot slot-key="notice" layout="strip" />         StatusRail 下方滚动条
 *   <AdSlot slot-key="activity" layout="card" />        右栏卡片
 *
 * layout 决定视觉模板:
 *   - banner: 横向大图 + 标题 + CTA · 适合 KPI 下方主推位
 *   - strip:  紧凑横条 · 文字为主 + 小 icon · 适合公告条
 *   - card:   方形卡片 · 适合活动入口 / 侧边推广
 *
 * 数据源: useOpSlots() · 对应 slot_key 拉到的列表 (按 priority 排序)
 *         默认只渲染第一条 · 用 max-items 可控
 *
 * 点击行为: 跟 SplashAdModal 一致 · external/internal/download/qr
 */
import { computed, watch } from "vue"
import {
  useOpSlots,
  reportSlotEvent,
  type OpSlotItem,
  type OpSlotKey,
} from "../../composables/useOpSlots"
// 2026-05-25 8.0.9 · 点击行为统一走 useAdAction · 支持 modal / embed_url / qr 等
import { useAdAction } from "../../composables/useAdAction"

type LayoutKind = "banner" | "strip" | "card"

const props = withDefaults(
  defineProps<{
    slotKey: OpSlotKey
    layout?: LayoutKind
    /** 最多显示几条 · 默认 1 */
    maxItems?: number
    /** 是否允许用户关闭这条 (仅 strip/notice 类有意义) */
    dismissible?: boolean
  }>(),
  {
    layout: "banner",
    maxItems: 1,
    dismissible: false,
  },
)

const { banner, notice, activity, dismissNotice } = useOpSlots()
const { dispatchAction } = useAdAction()

const list = computed<OpSlotItem[]>(() => {
  const all = (() => {
    switch (props.slotKey) {
      case "banner":
        return banner.value
      case "notice":
        return notice.value
      case "activity":
        return activity.value
      default:
        return []
    }
  })()
  return all.slice(0, props.maxItems)
})

async function onClick(item: OpSlotItem) {
  // 2026-05-25 8.0.9 · 统一走 dispatcher (内部处理 7 种 action + 埋点 + 弹窗)
  if (item.action_type === "none") return
  await dispatchAction(item)
}

function onDismiss(item: OpSlotItem, ev: Event) {
  ev.stopPropagation()
  void reportSlotEvent(item.id, "dismiss")
  if (props.slotKey === "notice") {
    dismissNotice(item.id)
  }
}

// ── impression 埋点 · 列表变化时给新出现的 slot 报一次 ──
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
  <!-- 无内容 · 不渲染占位 (避免空白) -->
  <template v-if="list.length === 0" />

  <!-- banner 模式 · 横向大图 + 标题 + CTA -->
  <section v-else-if="layout === 'banner'" class="ad-banner-list">
    <article
      v-for="item in list"
      :key="item.id"
      class="ad-banner"
      :class="{ clickable: item.action_type !== 'none' }"
      @click="onClick(item)"
    >
      <div v-if="item.image_url" class="ad-banner-img">
        <img :src="item.image_url" :alt="item.title" />
      </div>
      <div class="ad-banner-body">
        <span class="ad-tag">推广</span>
        <h4 v-if="item.title" class="ad-title">{{ item.title }}</h4>
        <p v-if="item.subtitle" class="ad-sub">{{ item.subtitle }}</p>
      </div>
      <div v-if="item.action_type !== 'none'" class="ad-banner-cta">
        <span class="ad-cta-text">
          {{ item.action_label || "查看" }}
        </span>
        <span class="ad-arrow">→</span>
      </div>
    </article>
  </section>

  <!-- strip 模式 · 紧凑公告条 -->
  <section v-else-if="layout === 'strip'" class="ad-strip-list">
    <div
      v-for="item in list"
      :key="item.id"
      class="ad-strip"
      :class="{ clickable: item.action_type !== 'none' }"
      @click="onClick(item)"
    >
      <span class="ad-strip-icon">ⓘ</span>
      <span class="ad-strip-text">
        <b v-if="item.title">{{ item.title }}</b>
        <span v-if="item.subtitle"> · {{ item.subtitle }}</span>
      </span>
      <span v-if="item.action_type !== 'none'" class="ad-strip-cta">
        {{ item.action_label || "查看 →" }}
      </span>
      <button
        v-if="dismissible || item.closable"
        class="ad-strip-close"
        aria-label="关闭"
        @click="onDismiss(item, $event)"
      >
        ×
      </button>
    </div>
  </section>

  <!-- card 模式 · 方形卡片 -->
  <section v-else-if="layout === 'card'" class="ad-card-list">
    <article
      v-for="item in list"
      :key="item.id"
      class="ad-card"
      :class="{ clickable: item.action_type !== 'none' }"
      @click="onClick(item)"
    >
      <div v-if="item.image_url" class="ad-card-img">
        <img :src="item.image_url" :alt="item.title" />
      </div>
      <div class="ad-card-body">
        <h5 v-if="item.title" class="ad-card-title">{{ item.title }}</h5>
        <p v-if="item.subtitle" class="ad-card-sub">{{ item.subtitle }}</p>
      </div>
      <footer v-if="item.action_type !== 'none'" class="ad-card-foot">
        {{ item.action_label || "立即查看" }}
        <span class="ad-arrow">→</span>
      </footer>
    </article>
  </section>
</template>

<style scoped>
/* ── banner ────────────────────────────────────────── */
.ad-banner-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin: var(--sp-6, 16px) 0;
}

.ad-banner {
  display: grid;
  grid-template-columns: 120px 1fr auto;
  gap: 14px;
  align-items: center;
  padding: 12px 16px;
  border-radius: 12px;
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  transition: transform 0.18s, box-shadow 0.18s, border-color 0.18s;
}

.ad-banner.clickable {
  cursor: pointer;
}

.ad-banner.clickable:hover {
  transform: translateY(-1px);
  box-shadow: 0 8px 24px var(--c-brand-glow, rgba(0, 0, 0, 0.25));
  border-color: var(--c-brand);
}

.ad-banner-img {
  width: 120px;
  height: 80px;
  border-radius: 8px;
  overflow: hidden;
  background: var(--c-bg-soft);
}

.ad-banner-img img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}

.ad-banner-body {
  min-width: 0;
}

.ad-tag {
  display: inline-block;
  padding: 2px 7px;
  border-radius: 4px;
  background: var(--c-brand-soft);
  color: var(--c-brand);
  font-size: 10px;
  letter-spacing: 0.04em;
  font-weight: 600;
  margin-bottom: 6px;
  text-transform: uppercase;
}

.ad-title {
  margin: 0 0 4px;
  font-size: 14px;
  font-weight: 600;
  color: var(--c-fg);
  letter-spacing: -0.005em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ad-sub {
  margin: 0;
  font-size: 12px;
  color: var(--c-fg-soft);
  line-height: 1.45;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.ad-banner-cta {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--c-brand);
  font-weight: 500;
  padding: 6px 10px;
  border-radius: 6px;
  background: var(--c-brand-soft);
}

.ad-arrow {
  font-size: 13px;
  display: inline-block;
  transition: transform 0.18s;
}

.ad-banner.clickable:hover .ad-arrow,
.ad-card.clickable:hover .ad-arrow {
  transform: translateX(2px);
}

.ad-strip-icon {
  color: var(--c-brand);
  flex-shrink: 0;
  font-size: 13px;
}

.ad-strip-close {
  font-size: 14px;
  line-height: 1;
  padding: 0;
  font-weight: 300;
}

/* ── strip ────────────────────────────────────────── */
.ad-strip-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 8px 0;
}

.ad-strip {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-radius: 8px;
  background: var(--c-brand-soft);
  border: 1px solid color-mix(in srgb, var(--c-brand) 25%, transparent);
  font-size: 12px;
  color: var(--c-fg);
}

.ad-strip.clickable {
  cursor: pointer;
  transition: background 0.18s, border-color 0.18s;
}

.ad-strip.clickable:hover {
  background: color-mix(in srgb, var(--c-brand) 18%, transparent);
  border-color: var(--c-brand);
}

.ad-strip-text {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ad-strip-cta {
  color: var(--c-brand);
  font-weight: 500;
  font-size: 11px;
  flex-shrink: 0;
}

.ad-strip-close {
  width: 18px;
  height: 18px;
  border-radius: 9px;
  border: none;
  background: transparent;
  color: var(--c-fg-soft);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0.6;
  transition: opacity 0.15s, background 0.15s;
}

.ad-strip-close:hover {
  opacity: 1;
  background: color-mix(in srgb, var(--c-fg) 12%, transparent);
}

/* ── card ────────────────────────────────────────── */
.ad-card-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 12px;
  margin: 12px 0;
}

.ad-card {
  display: flex;
  flex-direction: column;
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: 12px;
  overflow: hidden;
  transition: transform 0.18s, box-shadow 0.18s, border-color 0.18s;
}

.ad-card.clickable {
  cursor: pointer;
}

.ad-card.clickable:hover {
  transform: translateY(-2px);
  box-shadow: 0 12px 30px var(--c-brand-glow);
  border-color: var(--c-brand);
}

.ad-card-img {
  aspect-ratio: 16 / 9;
  background: var(--c-bg-soft);
}

.ad-card-img img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}

.ad-card-body {
  padding: 12px 14px 4px;
  flex: 1;
}

.ad-card-title {
  margin: 0 0 4px;
  font-size: 13px;
  font-weight: 600;
  color: var(--c-fg);
  line-height: 1.35;
}

.ad-card-sub {
  margin: 0;
  font-size: 11px;
  color: var(--c-fg-soft);
  line-height: 1.45;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.ad-card-foot {
  padding: 8px 14px 12px;
  font-size: 11px;
  color: var(--c-brand);
  font-weight: 500;
  display: flex;
  align-items: center;
  gap: 4px;
}
</style>
