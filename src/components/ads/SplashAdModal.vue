<script setup lang="ts">
/**
 * 开屏广告 Modal
 *
 * 触发: App.vue 挂载后 dashboard 渲染完毕 · useOpSlots.activeSplash 有值时弹
 * 优先级: 必须在 ConsentMatrixModal 之后弹 (App.vue 控制顺序)
 *
 * 4 种内容载体自适应:
 *   - image_url   · 主流 · 图片 + 标题 + CTA 按钮
 *   - video_url   · mp4 自动播放 (muted/loop)
 *   - rich_html   · 富文本 (含简单 HTML)
 *   - 纯标题       · 仅文字 (无图无视频无富文本时)
 *
 * 关闭逻辑: closable=true 才显示 X · 关掉的 splash 调 dismissSplash(id)
 *           本会话不再弹 (跨会话由后端 cooldown_hours 控制 · MVP 不实现)
 */
import { computed, ref, watch } from "vue"
import {
  useOpSlots,
  reportSlotEvent,
  type OpSlotItem,
} from "../../composables/useOpSlots"
// 2026-05-25 8.0.9 · 点击行为统一走 useAdAction · 支持 modal / embed_url / qr 等
import { useAdAction } from "../../composables/useAdAction"

const props = defineProps<{
  /** 启动延迟 (ms) · 避开 ConsentMatrixModal */
  delayMs?: number
}>()

const { activeSplash, dismissSplash } = useOpSlots()
const shown = ref(false)
const closing = ref(false)

// 延迟弹出 (避开其他启动期 modal)
watch(
  activeSplash,
  (slot) => {
    if (!slot || shown.value) return
    setTimeout(() => {
      shown.value = true
    }, props.delayMs ?? 1200)
  },
  { immediate: true },
)

const slot = computed<OpSlotItem | null>(() => activeSplash.value)

/** 内容载体类型 · 自动判定 */
const mediaType = computed<"video" | "image" | "rich" | "text">(() => {
  if (!slot.value) return "text"
  if (slot.value.video_url) return "video"
  if (slot.value.image_url) return "image"
  if (slot.value.rich_html) return "rich"
  return "text"
})

/** CTA 按钮文案 · 优先 action_label · 否则按 action_type 智能填充 */
const ctaLabel = computed(() => {
  if (!slot.value) return ""
  if (slot.value.action_label) return slot.value.action_label
  switch (slot.value.action_type) {
    case "external":
      return "立即查看"
    case "internal":
      return "前往"
    case "download":
      return "下载"
    case "qr":
      return "查看二维码"
    case "modal":
      return "查看详情"
    case "embed_url":
      return "打开页面"
    default:
      return ""
  }
})

const { dispatchAction } = useAdAction()

// 2026-05-25 8.0.9 · 有 CTA 的条件放宽: modal 只需 rich_html · qr 只需以上任一个
const hasCTA = computed(() => {
  const s = slot.value
  if (!s || s.action_type === "none") return false
  if (s.action_type === "modal") return !!(s.rich_html || s.action_target)
  if (s.action_type === "qr") return !!s.action_target
  return !!s.action_target
})

async function onCtaClick() {
  if (!slot.value) return
  // 2026-05-25 8.0.9 · 统一走 dispatcher
  await dispatchAction(slot.value)
  onClose({ fromCta: true })
}

function onClose(opts: { fromCta?: boolean } = {}) {
  if (!slot.value) return
  closing.value = true
  const id = slot.value.id
  // CTA 点击触发的关闭不算 dismiss · 用户主动关掉才算
  if (!opts.fromCta) void reportSlotEvent(id, "dismiss")
  setTimeout(() => {
    dismissSplash(id)
    shown.value = false
    closing.value = false
  }, 200)
}

// ── impression 埋点 · shown 变 true 时报一次 ──
watch(shown, (v) => {
  if (v && slot.value) {
    void reportSlotEvent(slot.value.id, "impression")
  }
})

function onBackdropClick() {
  // 仅 closable 才允许点遮罩关闭
  if (slot.value?.closable) onClose()
}
</script>

<template>
  <Transition name="splash-fade">
    <div
      v-if="shown && slot"
      class="splash-backdrop"
      :class="{ 'is-closing': closing }"
      @click.self="onBackdropClick"
    >
      <div class="splash-modal" role="dialog" aria-modal="true">
        <!-- 关闭按钮 (closable 才显示) -->
        <button
          v-if="slot.closable"
          class="splash-close"
          aria-label="关闭"
          @click="onClose()"
        >
          <span class="close-icon">×</span>
        </button>

        <!-- 内容载体 -->
        <div class="splash-content" :data-type="mediaType">
          <!-- video -->
          <video
            v-if="mediaType === 'video' && slot.video_url"
            :src="slot.video_url"
            class="splash-media splash-video"
            autoplay
            muted
            loop
            playsinline
          />
          <!-- image -->
          <img
            v-else-if="mediaType === 'image' && slot.image_url"
            :src="slot.image_url"
            :alt="slot.title"
            class="splash-media splash-image"
          />
          <!-- rich html -->
          <div
            v-else-if="mediaType === 'rich' && slot.rich_html"
            class="splash-rich"
            v-html="slot.rich_html"
          />

          <!-- 标题 + 副标题 (所有载体都显示在底部) -->
          <div v-if="slot.title || slot.subtitle" class="splash-meta">
            <h3 v-if="slot.title" class="splash-title">{{ slot.title }}</h3>
            <p v-if="slot.subtitle" class="splash-sub">{{ slot.subtitle }}</p>
          </div>
        </div>

        <!-- CTA 按钮 -->
        <footer v-if="hasCTA" class="splash-foot">
          <button class="splash-cta" @click="onCtaClick">
            {{ ctaLabel }}
            <span class="cta-arrow">→</span>
          </button>
        </footer>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.splash-backdrop {
  position: fixed;
  inset: 0;
  background: var(--c-bg-overlay);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1500;
  padding: 24px;
}

.splash-modal {
  position: relative;
  width: min(92vw, 540px);
  max-height: 86vh;
  background: var(--c-bg-card);
  border-radius: 16px;
  overflow: hidden;
  box-shadow:
    0 24px 64px rgba(0, 0, 0, 0.45),
    0 0 0 1px var(--c-line);
  display: flex;
  flex-direction: column;
}

.splash-close {
  position: absolute;
  top: 12px;
  right: 12px;
  width: 28px;
  height: 28px;
  border-radius: 14px;
  border: 1px solid var(--c-line);
  background: var(--c-bg-soft);
  color: var(--c-fg);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  z-index: 2;
  transition: transform 0.15s, background 0.15s, border-color 0.15s;
  font-size: 18px;
  line-height: 1;
  padding: 0;
}

.close-icon {
  display: block;
  font-weight: 300;
}

.cta-arrow {
  font-size: 14px;
  display: inline-block;
  transition: transform 0.18s;
}

.splash-cta:hover .cta-arrow {
  transform: translateX(2px);
}

.splash-close:hover {
  transform: scale(1.06);
  background: #fff;
}

.splash-content {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.splash-content[data-type="text"] {
  padding: 48px 32px 24px;
  background: linear-gradient(135deg, var(--c-brand, #7c3aed) 0%, #db2777 100%);
  color: #fff;
}

.splash-media {
  width: 100%;
  display: block;
  object-fit: cover;
  max-height: 60vh;
}

.splash-image {
  height: auto;
}

.splash-video {
  background: #000;
}

.splash-rich {
  padding: 32px 28px 16px;
  line-height: 1.65;
  font-size: 14px;
  color: var(--c-fg);
  overflow-y: auto;
  max-height: 60vh;
}

.splash-rich :deep(h1),
.splash-rich :deep(h2),
.splash-rich :deep(h3) {
  margin-top: 0;
  font-weight: 600;
}

.splash-rich :deep(a) {
  color: var(--c-brand, #7c3aed);
  text-decoration: underline;
}

.splash-meta {
  padding: 20px 24px 8px;
}

.splash-content[data-type="text"] .splash-meta {
  padding: 0;
  text-align: center;
}

.splash-title {
  margin: 0 0 6px;
  font-size: 18px;
  font-weight: 600;
  color: var(--c-fg, #111);
  letter-spacing: -0.01em;
}

.splash-content[data-type="text"] .splash-title {
  color: #fff;
  font-size: 22px;
  margin-bottom: 12px;
}

.splash-sub {
  margin: 0;
  font-size: 13px;
  color: var(--c-fg-soft);
  line-height: 1.55;
}

.splash-content[data-type="text"] .splash-sub {
  color: rgba(255, 255, 255, 0.85);
  font-size: 14px;
}

.splash-foot {
  padding: 16px 24px 20px;
  border-top: 1px solid var(--c-line);
  display: flex;
  justify-content: flex-end;
}

.splash-content[data-type="text"] + .splash-foot {
  border-top: none;
  background: var(--c-bg-card);
}

.splash-cta {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 10px 18px;
  border-radius: 8px;
  border: none;
  background: var(--c-brand);
  color: #fff;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: transform 0.15s, box-shadow 0.15s, background 0.15s;
}

.splash-cta:hover {
  transform: translateY(-1px);
  background: var(--c-brand-2);
  box-shadow: 0 6px 18px var(--c-brand-glow);
}

/* 入场动画 */
.splash-fade-enter-active,
.splash-fade-leave-active {
  transition: opacity 0.24s, backdrop-filter 0.24s;
}

.splash-fade-enter-active .splash-modal,
.splash-fade-leave-active .splash-modal {
  transition:
    transform 0.28s cubic-bezier(0.34, 1.56, 0.64, 1),
    opacity 0.24s;
}

.splash-fade-enter-from,
.splash-fade-leave-to {
  opacity: 0;
}

.splash-fade-enter-from .splash-modal,
.splash-fade-leave-to .splash-modal {
  transform: translateY(20px) scale(0.96);
  opacity: 0;
}
</style>
