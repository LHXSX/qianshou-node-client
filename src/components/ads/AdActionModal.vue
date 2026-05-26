<script setup lang="ts">
/**
 * 广告 / 通知 / Splash 点击后的统一弹窗
 *
 * 2026-05-25 8.0.9 · 在 App.vue 渲染 1 次 · 全局共享
 * 3 种渲染模式:
 *   - rich_html  → 内嵌 HTML (经过 sanitize)
 *   - embed_url  → iframe 加载远程页面
 *   - qr         → 二维码 + 文字提示
 */
import { computed, onBeforeUnmount, watch } from "vue"
import { open as shellOpen } from "@tauri-apps/plugin-shell"
import { useAdAction } from "../../composables/useAdAction"
import { reportSlotEvent } from "../../composables/useOpSlots"

const { modal, closeModal } = useAdAction()

const isOpen = computed(() => modal.value.open)

// ── ESC 关闭 ──
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape" && isOpen.value) closeModal()
}
watch(isOpen, (v) => {
  if (v) window.addEventListener("keydown", onKey)
  else window.removeEventListener("keydown", onKey)
})
onBeforeUnmount(() => window.removeEventListener("keydown", onKey))

// ── CTA 按钮 ──
async function onCta() {
  const url = modal.value.ctaUrl
  if (!url) {
    closeModal()
    return
  }
  if (modal.value.slotId) {
    void reportSlotEvent(modal.value.slotId, "click", { action_type: "modal_cta" })
  }
  try {
    await shellOpen(url)  // 2026-05-26 fix · 用 plugin-shell (open_external_url IPC 不存在)
  } catch (e) {
    console.warn("[AdActionModal] shell.open failed:", e)
    try { window.open(url, "_blank") } catch {}
  }
  closeModal()
}

// ── 复制按钮 (二维码模式下让用户复制链接) ──
async function onCopy() {
  const text = modal.value.ctaUrl || modal.value.payload
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    copied.value = true
    setTimeout(() => (copied.value = false), 1500)
  } catch {}
}
import { ref } from "vue"
const copied = ref(false)

// ── 二维码生成 (用 https://api.qrserver.com 服务 · 无依赖) ──
const qrSrc = computed(() => {
  if (modal.value.kind !== "qr") return ""
  const data = encodeURIComponent(modal.value.payload || "")
  return `https://api.qrserver.com/v1/create-qr-code/?size=260x260&margin=8&data=${data}`
})
</script>

<template>
  <Teleport to="body">
    <Transition name="ad-modal">
      <div v-if="isOpen" class="ad-modal-overlay" @click.self="closeModal">
        <article class="ad-modal" :data-kind="modal.kind">
          <!-- header -->
          <header class="am-head">
            <div class="am-titles">
              <h3 class="am-title">{{ modal.title }}</h3>
              <p v-if="modal.subtitle" class="am-sub">{{ modal.subtitle }}</p>
            </div>
            <button class="am-close" @click="closeModal" aria-label="关闭">×</button>
          </header>

          <!-- body · 按 kind 分支渲染 -->
          <div class="am-body" :class="`am-body-${modal.kind}`">
            <!-- rich_html · 用 v-html 直接渲染后台 HTML -->
            <div v-if="modal.kind === 'rich_html'"
                 class="am-rich"
                 v-html="modal.payload" />

            <!-- embed_url · iframe 加载外部页 -->
            <iframe v-else-if="modal.kind === 'embed_url'"
                    class="am-iframe"
                    :src="modal.payload"
                    referrerpolicy="no-referrer"
                    sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
                    loading="eager" />

            <!-- qr · 二维码 -->
            <div v-else-if="modal.kind === 'qr'" class="am-qr">
              <img :src="qrSrc" :alt="modal.title" class="am-qr-img" />
              <p class="am-qr-tip">用手机扫描二维码 · 或</p>
              <code class="am-qr-url">{{ modal.payload }}</code>
              <button class="am-qr-copy" @click="onCopy">
                {{ copied ? "✓ 已复制" : "📋 复制链接" }}
              </button>
            </div>
          </div>

          <!-- footer · CTA + 关闭 -->
          <footer class="am-foot">
            <button class="am-btn am-btn-ghost" @click="closeModal">关闭</button>
            <button v-if="modal.ctaUrl"
                    class="am-btn am-btn-primary"
                    @click="onCta">
              {{ modal.ctaLabel || "在浏览器打开 →" }}
            </button>
          </footer>
        </article>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.ad-modal-overlay {
  position: fixed; inset: 0; z-index: 9999;
  background: rgba(0, 0, 0, 0.55);
  backdrop-filter: blur(4px);
  display: flex; align-items: center; justify-content: center;
  padding: 24px;
}
.ad-modal {
  width: min(720px, 100%);
  max-height: min(82vh, 720px);
  background: var(--c-bg-card, #1a1d24);
  border: 1px solid var(--c-line, rgba(255,255,255,0.1));
  border-radius: 14px;
  box-shadow: 0 30px 60px -15px rgba(0,0,0,0.55);
  display: flex; flex-direction: column;
  overflow: hidden;
  color: var(--c-fg, #f8fafc);
}
.ad-modal[data-kind="embed_url"] {
  width: min(960px, 100%);
  max-height: min(85vh, 800px);
}
.ad-modal[data-kind="qr"] {
  width: min(420px, 100%);
}

.am-head {
  display: flex; align-items: flex-start; justify-content: space-between;
  padding: 18px 20px 14px;
  border-bottom: 1px solid var(--c-line, rgba(255,255,255,0.08));
  flex: 0 0 auto;
}
.am-titles { min-width: 0; flex: 1; }
.am-title { margin: 0; font-size: 18px; font-weight: 700; line-height: 1.3;
  color: var(--c-fg, #f8fafc); }
.am-sub { margin: 4px 0 0; font-size: 13px; color: var(--c-fg-muted, #94a3b8);
  line-height: 1.5; }
.am-close {
  flex-shrink: 0; margin-left: 12px;
  width: 32px; height: 32px; border-radius: 8px;
  background: transparent; border: none;
  color: var(--c-fg-muted, #94a3b8);
  font-size: 22px; line-height: 1; cursor: pointer;
  transition: background 0.15s, color 0.15s;
}
.am-close:hover { background: rgba(255,255,255,0.08); color: var(--c-fg, #f8fafc); }

.am-body { flex: 1 1 auto; min-height: 0; overflow: auto; }

/* rich_html · v-html 渲染 */
.am-body-rich_html { padding: 20px; }
.am-rich :deep(h1),
.am-rich :deep(h2),
.am-rich :deep(h3) { margin: 16px 0 8px; color: var(--c-fg, #f8fafc); }
.am-rich :deep(p) { margin: 8px 0; line-height: 1.65; color: var(--c-fg-soft, #cbd5e1); }
.am-rich :deep(a) { color: var(--c-brand, #06b6d4); }
.am-rich :deep(img) { max-width: 100%; height: auto; border-radius: 6px; }
.am-rich :deep(ul), .am-rich :deep(ol) { padding-left: 24px; margin: 8px 0; }
.am-rich :deep(li) { margin: 4px 0; color: var(--c-fg-soft, #cbd5e1); line-height: 1.55; }
.am-rich :deep(code) { background: rgba(255,255,255,0.08); padding: 2px 6px; border-radius: 4px;
  font-family: ui-monospace, monospace; font-size: 13px; }
.am-rich :deep(strong), .am-rich :deep(b) { color: var(--c-fg, #f8fafc); }

/* embed_url · iframe */
.am-body-embed_url { padding: 0; }
.am-iframe {
  width: 100%; height: 60vh;
  min-height: 420px;
  border: none; display: block;
  background: #fff;
}

/* qr · 二维码 */
.am-body-qr { padding: 24px 20px; text-align: center; }
.am-qr {
  display: flex; flex-direction: column; align-items: center; gap: 14px;
}
.am-qr-img {
  width: 220px; height: 220px;
  border-radius: 12px;
  background: #fff;
  padding: 8px;
  box-shadow: 0 4px 12px rgba(0,0,0,0.3);
}
.am-qr-tip { margin: 0; font-size: 13px; color: var(--c-fg-muted, #94a3b8); }
.am-qr-url {
  background: rgba(255,255,255,0.08);
  padding: 8px 12px; border-radius: 8px;
  font-family: ui-monospace, monospace; font-size: 12px;
  color: var(--c-fg-soft, #cbd5e1);
  word-break: break-all; text-align: left;
  max-width: 100%;
}
.am-qr-copy {
  background: rgba(255,255,255,0.08); border: 1px solid rgba(255,255,255,0.12);
  color: var(--c-fg-soft, #cbd5e1);
  padding: 6px 14px; border-radius: 6px; font-size: 12px; cursor: pointer;
  transition: background 0.15s;
}
.am-qr-copy:hover { background: rgba(255,255,255,0.15); }

/* footer */
.am-foot {
  display: flex; justify-content: flex-end; gap: 10px;
  padding: 12px 20px;
  border-top: 1px solid var(--c-line, rgba(255,255,255,0.08));
  flex: 0 0 auto;
}
.am-btn {
  padding: 8px 18px; border-radius: 8px; font-size: 13px; font-weight: 600;
  border: 1px solid transparent; cursor: pointer;
  transition: opacity 0.15s, transform 0.1s;
}
.am-btn:active { transform: translateY(1px); }
.am-btn-ghost {
  background: transparent;
  border-color: var(--c-line, rgba(255,255,255,0.15));
  color: var(--c-fg-soft, #cbd5e1);
}
.am-btn-ghost:hover { background: rgba(255,255,255,0.06); }
.am-btn-primary {
  background: linear-gradient(135deg, #2563eb, #1e40af);
  color: #fff; border-color: #1e40af;
}
.am-btn-primary:hover { opacity: 0.92; }

/* ── transition ── */
.ad-modal-enter-active, .ad-modal-leave-active { transition: opacity 0.2s; }
.ad-modal-enter-from, .ad-modal-leave-to { opacity: 0; }
.ad-modal-enter-active .ad-modal,
.ad-modal-leave-active .ad-modal { transition: transform 0.2s; }
.ad-modal-enter-from .ad-modal { transform: translateY(20px) scale(0.98); }
.ad-modal-leave-to .ad-modal { transform: translateY(8px) scale(0.99); }
</style>
