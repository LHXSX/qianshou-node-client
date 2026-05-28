<script setup lang="ts">
/**
 * Tier 安装失败友好对话框 (2026-05-28 · v8.1.3)
 *
 * 触发: listen Tauri event "runtime_install_done" · payload.success === false
 * 行为:
 *   - 弹模态框 · 显示哪个 tier 装失败 · 报错折叠展示
 *   - 主按钮 "查看安装指引" → 用 Tauri shell::open 打开浏览器
 *     URL: https://www.wujisuanli.com/#/runtime-mirrors (我们官方镜像源指引页)
 *   - 次按钮 "稍后再说" 关闭
 *
 * 设计 (按用户要求):
 *   - 自动镜像源轮询失败 + (未来) OSS prebuilt_venv 也失败 → 弹此对话框
 *   - 不强行帮用户修 · 引导用户去官方页面看手动指引
 *   - 0 backend 改动 · 纯前端 listen 现有 install_done 事件
 */
import { ref, onMounted, onBeforeUnmount } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

const RUNTIME_MIRRORS_URL = 'https://www.wujisuanli.com/#/runtime-mirrors'

interface InstallDonePayload {
  job_id: string
  tier: string
  success: boolean
  used_mirror?: string
  venv_python?: string
  error?: string
}

const visible = ref(false)
const failedTier = ref('')
const errorMsg = ref('')
const showDetail = ref(false)
let _unlisten: UnlistenFn | null = null

// 中文 tier 名映射 (给用户看的友好名)
const TIER_LABELS: Record<string, string> = {
  lite: '基础包 (lite)',
  crawl: '爬虫包 (crawl)',
  ocr: 'OCR 文字识别',
  speech: '语音转写 (Whisper)',
  'vision-ai': 'AI 视觉 (Stable Diffusion / Caption)',
  ffmpeg: '音视频 FFmpeg',
  render: '3D 渲染 (Blender)',
}

const tierLabel = (t: string): string => TIER_LABELS[t] || t

onMounted(async () => {
  _unlisten = await listen<InstallDonePayload>('runtime_install_done', (e) => {
    const p = e.payload
    if (!p.success) {
      failedTier.value = p.tier || '未知'
      errorMsg.value = p.error || '(无详细报错)'
      showDetail.value = false
      visible.value = true
    }
  })
})

onBeforeUnmount(() => {
  _unlisten?.()
})

async function openMirrorsPage() {
  try {
    const { open } = await import('@tauri-apps/plugin-shell')
    await open(RUNTIME_MIRRORS_URL)
  } catch {
    // fallback: 直接 location (虽然 Tauri 默认禁) · 极少触发
    window.open(RUNTIME_MIRRORS_URL, '_blank')
  }
  visible.value = false
}

function close() {
  visible.value = false
}
</script>

<template>
  <Teleport to="body">
    <div v-if="visible" class="ihm-mask" @click.self="close">
      <div class="ihm-modal">
        <header class="ihm-head">
          <div class="ihm-icon">⚠️</div>
          <div class="ihm-title">
            <h3>{{ tierLabel(failedTier) }} · 安装失败</h3>
            <p class="ihm-sub">系统按 6 个镜像源都试过 · 仍然失败 · 请看官方指引</p>
          </div>
          <button class="ihm-close" @click="close" aria-label="关闭">×</button>
        </header>

        <div class="ihm-body">
          <div class="ihm-tip">
            <strong>💡 通常原因:</strong>
            <ul>
              <li>网络/防火墙拦截 PyPI 镜像</li>
              <li>系统缺编译工具 (Windows VC++ Build Tools)</li>
              <li>该 tier 需要特殊系统库 (CUDA / libcudnn / Apple Silicon wheel 缺)</li>
            </ul>
          </div>

          <button class="ihm-detail-toggle" @click="showDetail = !showDetail">
            {{ showDetail ? '▲ 收起报错' : '▼ 查看完整报错' }}
          </button>
          <pre v-if="showDetail" class="ihm-error">{{ errorMsg }}</pre>

          <div class="ihm-actions">
            <button class="ihm-btn-primary" @click="openMirrorsPage">
              📖 查看安装指引(浏览器打开)
            </button>
            <button class="ihm-btn-secondary" @click="close">稍后再说</button>
          </div>

          <p class="ihm-foot">
            指引页: <code>{{ RUNTIME_MIRRORS_URL }}</code>
          </p>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.ihm-mask {
  position: fixed; inset: 0;
  background: rgba(0, 0, 0, 0.55);
  z-index: 9999;
  display: flex; align-items: center; justify-content: center;
  backdrop-filter: blur(2px);
}
.ihm-modal {
  background: var(--c-bg-card, white);
  color: var(--c-fg, #111);
  border-radius: 12px;
  width: 520px; max-width: 92vw;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.4);
  overflow: hidden;
  animation: ihm-pop 0.2s ease-out;
}
@keyframes ihm-pop {
  from { opacity: 0; transform: scale(0.96); }
  to { opacity: 1; transform: scale(1); }
}

.ihm-head {
  display: flex; align-items: flex-start; gap: 14px;
  padding: 20px 22px 14px;
  border-bottom: 1px solid var(--c-line, #e5e7eb);
}
.ihm-icon { font-size: 28px; flex-shrink: 0; line-height: 1; }
.ihm-title { flex: 1; }
.ihm-title h3 {
  margin: 0; font-size: 17px; font-weight: 600;
  color: var(--c-fg, #111);
}
.ihm-sub {
  margin: 4px 0 0; font-size: 13px;
  color: var(--c-mute, #6b7280);
}
.ihm-close {
  width: 28px; height: 28px;
  display: flex; align-items: center; justify-content: center;
  background: transparent; border: none;
  border-radius: 6px; cursor: pointer;
  font-size: 22px; color: var(--c-mute, #9ca3af);
  line-height: 1;
}
.ihm-close:hover { background: var(--c-bg-soft, #f3f4f6); color: var(--c-fg, #111); }

.ihm-body { padding: 18px 22px 22px; }

.ihm-tip {
  background: var(--c-bg-soft, #fef3c7);
  border: 1px solid var(--c-warn-soft, #fde68a);
  border-radius: 8px;
  padding: 12px 14px;
  margin-bottom: 14px;
}
.ihm-tip strong { font-size: 13px; color: var(--c-warn, #b45309); }
.ihm-tip ul {
  margin: 8px 0 0 18px; padding: 0;
  font-size: 12px; color: var(--c-fg-soft, #6b7280);
  line-height: 1.7;
}

.ihm-detail-toggle {
  background: transparent; border: none; cursor: pointer;
  font-size: 12px; color: var(--c-brand, #2563eb);
  padding: 4px 0; margin-bottom: 6px;
}
.ihm-error {
  background: #1e293b; color: #fca5a5;
  padding: 12px; border-radius: 6px;
  font-size: 11px; line-height: 1.5;
  font-family: ui-monospace, monospace;
  max-height: 200px; overflow-y: auto;
  white-space: pre-wrap; word-break: break-all;
  margin: 0 0 14px;
}

.ihm-actions {
  display: flex; gap: 10px; margin-top: 4px;
}
.ihm-btn-primary, .ihm-btn-secondary {
  flex: 1; padding: 10px 16px;
  font-size: 13px; font-weight: 500;
  border-radius: 7px; cursor: pointer;
  transition: all 0.15s;
  border: 1px solid transparent;
}
.ihm-btn-primary {
  background: var(--c-brand, #2563eb); color: white;
  border-color: var(--c-brand, #2563eb);
}
.ihm-btn-primary:hover { background: var(--c-brand-2, #1d4ed8); }
.ihm-btn-secondary {
  background: var(--c-bg-soft, #f3f4f6); color: var(--c-fg, #374151);
  border-color: var(--c-line, #d1d5db);
}
.ihm-btn-secondary:hover { background: var(--c-bg-elev-1, #e5e7eb); }

.ihm-foot {
  margin: 14px 0 0; text-align: center;
  font-size: 11px; color: var(--c-faint, #9ca3af);
}
.ihm-foot code {
  font-family: ui-monospace, monospace;
  font-size: 11px;
}
</style>
