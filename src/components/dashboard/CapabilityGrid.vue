<script setup lang="ts">
/**
 * 能力矩阵网格 · Dashboard 收益矩阵板块
 *
 * 5 个能力卡片 (compute / crawl / proxy / display / storage)
 *   - live   : 真实今日收益 + 可开关
 *   - beta   : 申请内测按钮
 *   - designing / planning : 关注动态按钮
 *
 * 每张卡片:
 *   - 左侧 3px 彩色立条 (能力主色)
 *   - 顶部 icon + 名称 + 状态徽章
 *   - 中部今日收益数字 (live 才有) · 副标题
 *   - 底部操作按钮 / 切换开关
 *
 * 设计语言: 跟 StatCard 一脉相承 · GitHub dark + 8px 圆角 + accent bar
 */
import Icon from "../Icon.vue"
import { useCapabilities } from "../../composables/useCapabilities"
import type { CapabilityId, CapabilityStatus } from "../../composables/useCapabilities"

const { capabilities, toggleConsent } = useCapabilities()

const emit = defineEmits<{
  /** 卡片被点击 · 跳详情 */
  (e: "detail", id: CapabilityId): void
  /** 想申请内测 / 想了解更多 */
  (e: "learn-more", id: CapabilityId): void
}>()

function fmtMoney(n: number): string {
  if (n === 0) return "0.00"
  return n.toLocaleString("zh-CN", { minimumFractionDigits: 2, maximumFractionDigits: 2 })
}

function statusBadgeClass(s: CapabilityStatus): string {
  return `badge-${s}`
}

function onToggle(id: CapabilityId, event: Event) {
  event.stopPropagation()
  toggleConsent(id)
}

function onDetail(id: CapabilityId) {
  emit("detail", id)
}

function onLearnMore(id: CapabilityId, event: Event) {
  event.stopPropagation()
  emit("learn-more", id)
}
</script>

<template>
  <section class="cap-grid-section">
    <header class="cap-grid-header">
      <h3 class="cap-grid-title">
        <Icon name="task-render" :size="16" class="cap-grid-title-icon" />
        我的贡献矩阵 <span class="cap-grid-sub">· 一台设备多项资源价值化</span>
      </h3>
      <span class="cap-grid-hint">每项独立授权 · 可随时关闭</span>
    </header>

    <div class="cap-grid">
      <article
        v-for="c in capabilities"
        :key="c.id"
        :class="['cap-card', `status-${c.status}`, c.consent && c.status === 'live' ? 'is-active' : '']"
        :style="{ '--cap-color': c.color }"
        @click="onDetail(c.id)"
      >
        <div class="cap-bar" />

        <header class="cap-head">
          <div class="cap-head-left">
            <span class="cap-icon-wrap">
              <Icon :name="c.icon as any" :size="16" />
            </span>
            <span class="cap-name">{{ c.name }}</span>
          </div>
          <span :class="['cap-badge', statusBadgeClass(c.status)]">
            {{ c.statusLabel }}
          </span>
        </header>

        <div class="cap-body">
          <!-- live + 用户得益 (仅 compute): 真实收益 ¥ -->
          <template v-if="c.status === 'live' && c.userEarns">
            <div class="cap-money-row">
              <span class="cap-money mono">¥{{ fmtMoney(c.todayEarnings ?? 0) }}</span>
              <span class="cap-money-unit">今日</span>
            </div>
            <div class="cap-sub">累计 ¥{{ fmtMoney(c.totalEarnings ?? 0) }} · {{ c.subtitle }}</div>
          </template>

          <!-- live + 用户不得益 (crawl 上线后): 中性贡献描述 -->
          <template v-else-if="c.status === 'live' && !c.userEarns">
            <div class="cap-money-row">
              <span class="cap-contrib-dot" :style="{ background: c.color }" />
              <span class="cap-contrib-text">{{ c.consent ? "运行中" : "未启用" }}</span>
            </div>
            <div class="cap-sub">{{ c.valueDesc }}</div>
            <div v-if="c.consent" class="cap-eta cap-contrib-stat">
              已贡献 {{ c.contributionCount ?? 0 }} {{ c.contributionUnit }}
            </div>
          </template>

          <!-- beta: 即将开放 -->
          <template v-else-if="c.status === 'beta'">
            <div class="cap-money-row">
              <span class="cap-soon-emoji">🚀</span>
              <span class="cap-soon-text">即将开放</span>
            </div>
            <div class="cap-sub">{{ c.valueDesc }}</div>
            <div v-if="c.expectedLaunch" class="cap-eta">预计 {{ c.expectedLaunch }} 上线</div>
          </template>

          <!-- designing / planning: 关注动态 -->
          <template v-else>
            <div class="cap-money-row">
              <span class="cap-locked-icon">○</span>
              <span class="cap-locked-text">{{ c.statusLabel }}</span>
            </div>
            <div class="cap-sub">{{ c.valueDesc }}</div>
            <div v-if="c.expectedLaunch" class="cap-eta">预计 {{ c.expectedLaunch }} 上线</div>
          </template>
        </div>

        <footer class="cap-foot">
          <!-- live: toggle -->
          <label v-if="c.status === 'live'" class="cap-toggle" @click.stop>
            <input
              type="checkbox"
              :checked="c.consent"
              @change="onToggle(c.id, $event)"
            />
            <span class="cap-toggle-track">
              <span class="cap-toggle-thumb" />
            </span>
            <span class="cap-toggle-label">{{ c.consent ? "已启用" : "已关闭" }}</span>
          </label>

          <!-- beta: 申请内测 -->
          <button
            v-else-if="c.status === 'beta'"
            class="cap-action cap-action-primary"
            @click="onLearnMore(c.id, $event)"
          >
            申请内测
          </button>

          <!-- designing / planning: 关注动态 -->
          <button
            v-else
            class="cap-action cap-action-ghost"
            @click="onLearnMore(c.id, $event)"
          >
            关注动态
          </button>
        </footer>
      </article>
    </div>

    <p class="cap-grid-footer-hint">
      💡 算力收益归你 · 其他贡献支撑平台公共服务 · 你可随时开关任意一项
    </p>
  </section>
</template>

<style scoped>
.cap-grid-section {
  margin: var(--sp-6, 16px) 0;
}

.cap-grid-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: var(--sp-5, 12px);
  padding: 0 2px;
}
.cap-grid-title {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  font-size: var(--fs-md, 15px);
  font-weight: var(--fw-semibold, 600);
  color: var(--c-fg);
  letter-spacing: -0.01em;
}
.cap-grid-title-icon { color: var(--c-brand); }
.cap-grid-sub {
  font-size: var(--fs-xs, 13px);
  color: var(--c-mute);
  font-weight: var(--fw-medium, 500);
  letter-spacing: 0;
}
.cap-grid-hint {
  font-size: var(--fs-2xs, 12px);
  color: var(--c-faint);
}

/* 网格: 桌面 5 列 · 平板 3 · 手机 2 · 极小 1 */
.cap-grid {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  gap: var(--sp-4, 10px);
}
@media (max-width: 1200px) { .cap-grid { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
@media (max-width: 768px)  { .cap-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
@media (max-width: 480px)  { .cap-grid { grid-template-columns: 1fr; } }

.cap-card {
  position: relative;
  display: flex;
  flex-direction: column;
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md, 8px);
  padding: var(--sp-5, 12px) var(--sp-5, 12px) var(--sp-4, 10px);
  cursor: pointer;
  overflow: hidden;
  transition: border-color var(--dur-base, 0.15s), transform var(--dur-base, 0.15s),
              box-shadow var(--dur-base, 0.15s);
  min-height: 168px;
}
.cap-card:hover {
  border-color: var(--c-line-strong);
  transform: translateY(-1px);
  box-shadow: 0 4px 12px -2px rgba(0, 0, 0, 0.3);
}
.cap-card.is-active {
  border-color: var(--cap-color, var(--c-brand));
  box-shadow: 0 0 0 1px var(--cap-color, var(--c-brand)),
              0 4px 14px -4px var(--cap-color, var(--c-brand-glow));
}

.cap-bar {
  position: absolute;
  left: 0; top: 0; bottom: 0;
  width: 3px;
  background: var(--cap-color, var(--c-line));
  opacity: 0.85;
}
.cap-card.status-designing .cap-bar,
.cap-card.status-planning .cap-bar { opacity: 0.35; }

.cap-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: var(--sp-4, 10px);
  gap: 6px;
}
.cap-head-left {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.cap-icon-wrap {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: var(--r-sm, 6px);
  background: color-mix(in srgb, var(--cap-color) 12%, transparent);
  color: var(--cap-color, var(--c-fg));
  flex-shrink: 0;
}
.cap-name {
  font-size: var(--fs-sm, 14px);
  font-weight: var(--fw-semibold, 600);
  color: var(--c-fg);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.cap-badge {
  font-size: var(--fs-2xs, 12px);
  font-weight: var(--fw-semibold, 600);
  padding: 2px 7px;
  border-radius: var(--r-pill, 999px);
  letter-spacing: 0.02em;
  flex-shrink: 0;
}
.cap-badge.badge-live      { color: var(--c-ok);    background: var(--c-ok-soft); }
.cap-badge.badge-beta      { color: var(--c-warn);  background: var(--c-warn-soft); }
.cap-badge.badge-designing { color: var(--c-mute);  background: var(--c-bg-soft); }
.cap-badge.badge-planning  { color: var(--c-faint); background: var(--c-bg-soft); }

.cap-body {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-height: 0;
}
.cap-money-row {
  display: flex;
  align-items: baseline;
  gap: 6px;
}
.cap-money {
  font-size: 22px;
  font-weight: var(--fw-bold, 700);
  color: var(--c-fg);
  letter-spacing: -0.02em;
  line-height: 1.1;
}
.cap-money-unit {
  font-size: var(--fs-2xs, 12px);
  color: var(--c-mute);
  letter-spacing: 0.04em;
}
.cap-soon-emoji { font-size: 18px; }
.cap-soon-text {
  font-size: var(--fs-sm, 14px);
  color: var(--c-warn);
  font-weight: var(--fw-semibold, 600);
}
.cap-locked-icon {
  font-size: 18px;
  color: var(--c-faint);
  line-height: 1;
}
.cap-locked-text {
  font-size: var(--fs-sm, 14px);
  color: var(--c-mute);
  font-weight: var(--fw-medium, 500);
}

.cap-sub {
  font-size: var(--fs-2xs, 12px);
  color: var(--c-mute);
  line-height: 1.45;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.cap-eta {
  font-size: var(--fs-2xs, 12px);
  color: var(--c-faint);
  margin-top: 2px;
  font-family: ui-monospace, monospace;
}

.cap-foot {
  display: flex;
  align-items: center;
  margin-top: var(--sp-4, 10px);
  padding-top: var(--sp-3, 8px);
  border-top: 1px dashed var(--c-line);
}

/* toggle */
.cap-toggle {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  user-select: none;
}
.cap-toggle input { display: none; }
.cap-toggle-track {
  position: relative;
  display: inline-block;
  width: 30px;
  height: 16px;
  background: var(--c-bg-soft);
  border: 1px solid var(--c-line);
  border-radius: 999px;
  transition: background var(--dur-base, 0.15s), border-color var(--dur-base, 0.15s);
}
.cap-toggle-thumb {
  position: absolute;
  top: 1px;
  left: 1px;
  width: 12px;
  height: 12px;
  border-radius: 50%;
  background: var(--c-mute);
  transition: transform var(--dur-base, 0.15s), background var(--dur-base, 0.15s);
}
.cap-toggle input:checked + .cap-toggle-track {
  background: color-mix(in srgb, var(--cap-color) 22%, transparent);
  border-color: var(--cap-color);
}
.cap-toggle input:checked + .cap-toggle-track .cap-toggle-thumb {
  transform: translateX(14px);
  background: var(--cap-color);
}
.cap-toggle-label {
  font-size: var(--fs-xs, 13px);
  color: var(--c-mute);
}
.cap-toggle input:checked ~ .cap-toggle-label { color: var(--c-fg); }

/* button */
.cap-action {
  flex: 1;
  font-size: var(--fs-xs, 13px);
  font-weight: var(--fw-medium, 500);
  padding: 5px 10px;
  border-radius: var(--r-sm, 6px);
  border: 1px solid var(--c-line);
  background: transparent;
  cursor: pointer;
  transition: all var(--dur-base, 0.15s);
}
.cap-action-primary {
  color: var(--c-warn);
  border-color: var(--c-warn);
  background: var(--c-warn-soft);
}
.cap-action-primary:hover {
  background: color-mix(in srgb, var(--c-warn) 22%, transparent);
}
.cap-action-ghost {
  color: var(--c-mute);
}
.cap-action-ghost:hover {
  color: var(--c-fg);
  border-color: var(--c-line-strong);
  background: var(--c-bg-soft);
}

.cap-grid-footer-hint {
  margin: var(--sp-5, 12px) 0 0;
  text-align: center;
  font-size: var(--fs-xs, 13px);
  color: var(--c-faint);
  line-height: 1.5;
}

.mono { font-family: ui-monospace, "SF Mono", Menlo, monospace; }
</style>
