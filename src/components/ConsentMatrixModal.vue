<script setup lang="ts">
/**
 * 同意矩阵 Modal · 首启 / 升级后弹出
 *
 * 设计:
 *   - 用户必须独立勾选每个能力的授权 (默认全关 · 算力可推荐勾选)
 *   - 必须同时勾选 服务总协议 + 隐私政策
 *   - 至少授权 1 个能力才能确认
 *   - 关闭按钮: 稍后再说 (但下次启动还会再弹)
 *
 * 触发逻辑 (由 App.vue 控制):
 *   if (!hasCompletedOnboarding) showModal()
 *
 * 协议链接: 暂时跳到 PRIMARY_DOMAIN/legal/<id> · 后续放本地
 */
import { computed, ref } from "vue"
import Icon from "./Icon.vue"
import {
  useCapabilities,
  type CapabilityId,
} from "../composables/useCapabilities"
import { PRIMARY_DOMAIN } from "@shared"

const { capabilities, consentState, setConsents } = useCapabilities()

const props = withDefaults(defineProps<{
  open: boolean
  /** 首启 (用户必须确认或关闭) · 还是设置页打开 (可随意关闭) */
  mode?: "onboarding" | "settings"
}>(), {
  mode: "onboarding",
})

const emit = defineEmits<{
  (e: "close"): void
  (e: "confirmed", payload: { consents: Record<CapabilityId, boolean> }): void
}>()

// 本地草稿状态 (确认前不影响真实 consent)
const draft = ref<Record<CapabilityId, boolean>>({
  compute: consentState.value.consents.compute,
  crawl: consentState.value.consents.crawl,
  proxy: consentState.value.consents.proxy,
  script: consentState.value.consents.script,
})
const draftToS = ref<boolean>(consentState.value.agreedToS)
const draftPrivacy = ref<boolean>(consentState.value.agreedPrivacy)

const draftCount = computed(() => Object.values(draft.value).filter(Boolean).length)
const canConfirm = computed(
  () => draftCount.value > 0 && draftToS.value && draftPrivacy.value,
)

function legalUrl(id: CapabilityId | "tos" | "privacy"): string {
  return `${PRIMARY_DOMAIN}/legal/${id}`
}

function onConfirm() {
  if (!canConfirm.value) return
  setConsents({
    consents: draft.value,
    agreedToS: draftToS.value,
    agreedPrivacy: draftPrivacy.value,
  })
  emit("confirmed", { consents: { ...draft.value } })
  emit("close")
}

function onLater() {
  emit("close")
}

function recommendDefault() {
  // 一键推荐: 仅勾算力贡献 (最稳的能力)
  draft.value = {
    compute: true,
    crawl: false,
    proxy: false,
    script: false,
  }
  draftToS.value = true
  draftPrivacy.value = true
}
</script>

<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="open" class="cm-backdrop" @click.self="mode === 'settings' && onLater()">
        <div class="cm-modal" role="dialog" aria-modal="true">
          <!-- 顶部 -->
          <header class="cm-head">
            <div class="cm-head-line">
              <Icon name="task-render" :size="18" class="cm-head-icon" />
              <h2 class="cm-title">隐私授权 · 您完全掌控</h2>
            </div>
            <p class="cm-sub">
              千手节点支持 <b>4 项设备资源授权</b> · 算力 / 数据采集 / IP 池 / 通用脚本 · 每项独立授权 · 可随时撤回
            </p>
          </header>

          <!-- 能力授权列表 -->
          <div class="cm-list">
            <label
              v-for="c in capabilities"
              :key="c.id"
              :class="['cm-item', `status-${c.status}`, draft[c.id] ? 'is-on' : '']"
              :style="{ '--cap-color': c.color }"
            >
              <input
                v-model="draft[c.id]"
                type="checkbox"
                class="cm-check"
              />
              <span class="cm-check-box">
                <Icon v-if="draft[c.id]" name="status-done" :size="11" />
              </span>

              <div class="cm-item-body">
                <div class="cm-item-line1">
                  <span class="cm-item-icon">
                    <Icon :name="(c.icon as any)" :size="14" />
                  </span>
                  <span class="cm-item-name">{{ c.name }}</span>
                  <span :class="['cm-item-badge', `badge-${c.status}`]">
                    {{ c.statusLabel }}
                  </span>
                </div>
                <div class="cm-item-sub">{{ c.subtitle }}</div>
              </div>

              <a
                class="cm-legal-link"
                :href="legalUrl(c.id)"
                target="_blank"
                rel="noopener"
                @click.stop
              >
                查看协议 →
              </a>
            </label>
          </div>

          <!-- 总协议 + 隐私 -->
          <div class="cm-tos">
            <label class="cm-tos-item">
              <input v-model="draftToS" type="checkbox" class="cm-check" />
              <span class="cm-check-box">
                <Icon v-if="draftToS" name="status-done" :size="11" />
              </span>
              <span class="cm-tos-text">
                我已阅读并同意
                <a :href="legalUrl('tos')" target="_blank" rel="noopener" @click.stop>《千手节点服务总协议》</a>
              </span>
            </label>
            <label class="cm-tos-item">
              <input v-model="draftPrivacy" type="checkbox" class="cm-check" />
              <span class="cm-check-box">
                <Icon v-if="draftPrivacy" name="status-done" :size="11" />
              </span>
              <span class="cm-tos-text">
                我已阅读并同意
                <a :href="legalUrl('privacy')" target="_blank" rel="noopener" @click.stop>《隐私政策》</a>
              </span>
            </label>
          </div>

          <!-- 状态提示 -->
          <div class="cm-status">
            <span v-if="!canConfirm" class="cm-warn">
              <Icon name="status-failed" :size="12" />
              {{
                draftCount === 0
                  ? "请至少授权 1 个能力以使用千手节点"
                  : "请勾选服务总协议 + 隐私政策"
              }}
            </span>
            <span v-else class="cm-ok">
              <Icon name="status-done" :size="12" />
              已授权 {{ draftCount }} 个能力 · 可点击确认
            </span>
          </div>

          <!-- 底部按钮 -->
          <footer class="cm-foot">
            <button class="cm-btn cm-btn-ghost" @click="recommendDefault">
              推荐设置
            </button>
            <div class="cm-foot-right">
              <button class="cm-btn cm-btn-secondary" @click="onLater">
                {{ mode === "onboarding" ? "稍后再说" : "取消" }}
              </button>
              <button
                class="cm-btn cm-btn-primary"
                :disabled="!canConfirm"
                @click="onConfirm"
              >
                确认授权
              </button>
            </div>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.cm-backdrop {
  position: fixed;
  inset: 0;
  background: var(--c-bg-overlay, rgba(13, 17, 23, 0.78));
  backdrop-filter: blur(6px);
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 20px;
}

.cm-modal {
  width: 100%;
  max-width: 560px;
  max-height: calc(100vh - 40px);
  overflow-y: auto;
  background: var(--c-bg-card);
  border: 1px solid var(--c-line-strong);
  border-radius: var(--r-lg, 12px);
  box-shadow: var(--sh-3, 0 10px 28px -4px rgba(0, 0, 0, 0.55));
  padding: var(--sp-6, 16px) var(--sp-6, 16px) var(--sp-5, 12px);
  display: flex;
  flex-direction: column;
  gap: var(--sp-5, 12px);
}

.cm-head { display: flex; flex-direction: column; gap: 6px; }
.cm-head-line { display: flex; align-items: center; gap: 8px; }
.cm-head-icon { color: var(--c-brand); }
.cm-title {
  margin: 0;
  font-size: var(--fs-lg, 17px);
  font-weight: var(--fw-bold, 700);
  color: var(--c-fg);
  letter-spacing: -0.01em;
}
.cm-sub {
  margin: 0;
  font-size: var(--fs-xs, 13px);
  color: var(--c-mute);
  line-height: 1.5;
}

/* ── 能力列表 ── */
.cm-list { display: flex; flex-direction: column; gap: 6px; }
.cm-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  background: var(--c-bg-soft);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md, 8px);
  cursor: pointer;
  transition: border-color var(--dur-base, 0.15s), background var(--dur-base, 0.15s);
}
.cm-item:hover { border-color: var(--c-line-strong); }
.cm-item.is-on {
  border-color: var(--cap-color, var(--c-brand));
  background: color-mix(in srgb, var(--cap-color) 6%, var(--c-bg-soft));
}
.cm-item.status-designing,
.cm-item.status-planning { opacity: 0.78; }

.cm-check { display: none; }
.cm-check-box {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  background: var(--c-bg);
  border: 1.5px solid var(--c-line-strong);
  border-radius: var(--r-xs, 4px);
  flex-shrink: 0;
  color: var(--c-bg);
  transition: all var(--dur-base, 0.15s);
}
.cm-item.is-on .cm-check-box {
  background: var(--cap-color, var(--c-brand));
  border-color: var(--cap-color, var(--c-brand));
  color: var(--c-bg);
}
.cm-tos-item .cm-check:checked + .cm-check-box {
  background: var(--c-brand);
  border-color: var(--c-brand);
  color: var(--c-bg);
}

.cm-item-body {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.cm-item-line1 { display: flex; align-items: center; gap: 6px; }
.cm-item-icon { color: var(--cap-color, var(--c-fg)); display: inline-flex; }
.cm-item-name {
  font-size: var(--fs-sm, 14px);
  font-weight: var(--fw-semibold, 600);
  color: var(--c-fg);
}
.cm-item-badge {
  font-size: var(--fs-2xs, 12px);
  padding: 1px 6px;
  border-radius: var(--r-pill, 999px);
  font-weight: var(--fw-medium, 500);
}
.cm-item-badge.badge-live      { color: var(--c-ok);    background: var(--c-ok-soft); }
.cm-item-badge.badge-beta      { color: var(--c-warn);  background: var(--c-warn-soft); }
.cm-item-badge.badge-designing { color: var(--c-mute);  background: transparent; border: 1px solid var(--c-line); }
.cm-item-badge.badge-planning  { color: var(--c-faint); background: transparent; border: 1px solid var(--c-line); }

.cm-item-sub {
  font-size: var(--fs-2xs, 12px);
  color: var(--c-mute);
}

.cm-legal-link {
  font-size: var(--fs-2xs, 12px);
  color: var(--c-faint);
  text-decoration: none;
  flex-shrink: 0;
  transition: color var(--dur-base, 0.15s);
}
.cm-legal-link:hover { color: var(--c-brand); }

/* ── ToS ── */
.cm-tos {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding-top: var(--sp-3, 8px);
  border-top: 1px dashed var(--c-line);
}
.cm-tos-item {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  user-select: none;
}
.cm-tos-text {
  font-size: var(--fs-xs, 13px);
  color: var(--c-fg-soft);
}
.cm-tos-text a {
  color: var(--c-brand);
  text-decoration: none;
}
.cm-tos-text a:hover { text-decoration: underline; }

/* ── 状态行 ── */
.cm-status {
  font-size: var(--fs-2xs, 12px);
  display: flex;
  align-items: center;
}
.cm-warn {
  color: var(--c-warn);
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.cm-ok {
  color: var(--c-ok);
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

/* ── 底部按钮 ── */
.cm-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding-top: var(--sp-3, 8px);
  border-top: 1px solid var(--c-line);
}
.cm-foot-right {
  display: flex;
  gap: 8px;
}
.cm-btn {
  font-size: var(--fs-sm, 14px);
  font-weight: var(--fw-medium, 500);
  padding: 7px 14px;
  border-radius: var(--r-sm, 6px);
  border: 1px solid transparent;
  cursor: pointer;
  transition: all var(--dur-base, 0.15s);
}
.cm-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.cm-btn-ghost {
  color: var(--c-mute);
  background: transparent;
  border-color: var(--c-line);
}
.cm-btn-ghost:hover {
  color: var(--c-fg);
  border-color: var(--c-line-strong);
}
.cm-btn-secondary {
  color: var(--c-fg);
  background: var(--c-bg-soft);
  border-color: var(--c-line);
}
.cm-btn-secondary:hover {
  border-color: var(--c-line-strong);
}
.cm-btn-primary {
  color: #fff;
  background: var(--c-brand);
  border-color: var(--c-brand);
}
.cm-btn-primary:hover:not(:disabled) {
  filter: brightness(1.1);
  box-shadow: var(--sh-glow-brand, 0 0 12px -2px rgba(88, 166, 255, 0.4));
}

/* ── Transition ── */
.fade-enter-active, .fade-leave-active {
  transition: opacity 0.22s ease;
}
.fade-enter-from, .fade-leave-to { opacity: 0; }
.fade-enter-active .cm-modal,
.fade-leave-active .cm-modal {
  transition: transform 0.22s ease;
}
.fade-enter-from .cm-modal { transform: translateY(12px) scale(0.96); }
.fade-leave-to .cm-modal   { transform: translateY(6px) scale(0.98); }
</style>
