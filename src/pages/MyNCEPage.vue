<script setup lang="ts">
/**
 * 节点信誉 · NCE 自查面板 (2026-05-28 · v8.1.1 新增)
 *
 * 数据源: composables/useNceProfile.ts → /api/v8/my/workers/{node_id}/nce
 *
 * 展示:
 *   - 4 张 KPI: hw_tier · 信誉段位 · 预估月入 · 当前负载
 *   - 硬件评分 panel: hw_tier 大字 + 5 子分 (CPU/RAM/GPU/Storage/Network)
 *   - 信誉评分 panel: rep_main + 4 子分 (稳定性/正确率/速度/资源)
 *   - 系统给的改进建议 list (warn/info/praise)
 *   - 顶部刷新按钮 + onboarding_status 徽章
 *
 * 设计跟 EarningsPage 对齐 · 用同一套 .panel / .kpi-row / StatCard 风格
 */
import { computed } from "vue"
import Icon from "../components/Icon.vue"
import StatCard from "../components/dashboard/StatCard.vue"
import { useNceProfile, tierEmoji } from "../composables/useNceProfile"

const {
  profile,
  loading,
  error,
  hwTier,
  hwScore,
  repMain,
  tier,
  income,
  suggestions,
  refresh,
} = useNceProfile()

// 派生计算
const loadPct = computed(() => Math.round((profile.value?.runtime.load ?? 0) * 100))
const activeShards = computed(() => profile.value?.runtime.active_shards ?? 0)
const onboardingStatus = computed(() => profile.value?.runtime.onboarding_status ?? "unknown")
const onboardingLabel = computed(() => ({
  probation: "实习期",
  active: "正常",
  banned: "封禁",
  small_pool: "小池",
  unknown: "—",
}[onboardingStatus.value] || onboardingStatus.value))
const onboardingTone = computed(() => ({
  probation: "warn",
  active: "ok",
  banned: "err",
  small_pool: "info",
  unknown: "info",
}[onboardingStatus.value] || "info"))

// 硬件 5 子分 (默认 0 + 展示英文 → 中文)
const HW_LABELS: Record<string, string> = {
  cpu: "CPU 处理器",
  ram: "内存",
  gpu: "GPU 加速器",
  storage: "存储",
  network: "网络",
}
const hwSubs = computed(() => {
  const sub = profile.value?.hardware.sub_scores ?? {}
  // 按权重顺序排 (CPU 30 · RAM 25 · GPU 35 · Storage 5 · Network 5)
  const order = ["cpu", "ram", "gpu", "storage", "network"]
  return order.map((k) => ({
    key: k,
    label: HW_LABELS[k] || k,
    score: Math.round(Number(sub[k] ?? 0)),
  }))
})

// 信誉 4 子分 (权重 stability25 · correctness40 · speed20 · resource15)
const REP_LABELS: Record<string, string> = {
  stability: "稳定性",
  correctness: "正确率",
  speed: "处理速度",
  resource: "资源效率",
}
const repSubs = computed(() => {
  const rep = profile.value?.reputation
  if (!rep) return []
  return [
    { key: "stability", label: "稳定性", score: rep.rep_stability, weight: 25 },
    { key: "correctness", label: "正确率", score: rep.rep_correctness, weight: 40 },
    { key: "speed", label: "处理速度", score: rep.rep_speed, weight: 20 },
    { key: "resource", label: "资源效率", score: rep.rep_resource, weight: 15 },
  ]
})

function scoreColor(s: number): string {
  if (s >= 80) return "var(--c-ok)"
  if (s >= 60) return "var(--c-brand)"
  if (s >= 40) return "var(--c-warn)"
  return "var(--c-err)"
}

function suggestionIcon(level: string): string {
  if (level === "praise") return "✨"
  if (level === "warn" || level === "warning") return "⚠️"
  if (level === "err") return "🚫"
  return "💡"
}
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1 class="page-title">
          节点信誉
          <span class="badge" :class="`tone-${onboardingTone}`">{{ onboardingLabel }}</span>
        </h1>
        <p class="page-sub">
          基于 NCE 多维评估 · 调度器按此分配任务 · 越高接到越好的任务
        </p>
      </div>
      <button class="refresh-btn" :disabled="loading" @click="refresh(true)">
        <Icon name="action-refresh" :size="14" v-if="!loading" />
        <span v-else class="spinner-mini" />
        {{ loading ? "刷新中" : "立即刷新" }}
      </button>
    </header>

    <!-- 错误 / 兼容模式提示 -->
    <div v-if="error" class="alert alert-err">
      ⚠️ 加载失败: {{ error }}
    </div>
    <div v-else-if="profile?.compatibility_mode" class="alert alert-info">
      ℹ️ 后端 NCE 完整评分尚未对你开放 · 当前显示简化值 · 联系 admin 启用 flag `nce_api_expose_multi_dim`
    </div>

    <!-- ════════ KPI 行 ════════ -->
    <div class="kpi-row">
      <StatCard
        label="硬件档位"
        :value="`${tierEmoji(hwTier)} ${hwTier}`"
        unit=""
        accent="brand"
        icon="cpu"
        :hint="`硬件分 ${Math.round(hwScore)}/100`"
      />
      <StatCard
        label="信誉段位"
        :value="`${tier.emoji} ${tier.name}`"
        unit=""
        accent="ok"
        icon="status-done"
        :hint="repMain !== null ? `信誉分 ${repMain}/100` : '评估中'"
      />
      <StatCard
        label="预估月入"
        :value="income > 0 ? income.toLocaleString() : '—'"
        unit="EDG"
        accent="warn"
        icon="coin"
        hint="基于档位 × 段位的粗估"
      />
      <StatCard
        label="当前负载"
        :value="`${loadPct}`"
        unit="%"
        :accent="loadPct >= 80 ? 'warn' : 'brand'"
        icon="spark"
        :hint="`运行中 ${activeShards} 个分片`"
      />
    </div>

    <!-- ════════ 硬件 5 子分 ════════ -->
    <section class="panel">
      <header class="p-head">
        <span class="p-title">硬件评分 · 5 维度</span>
        <span class="p-meta">CPU 30 · RAM 25 · GPU 35 · Storage 5 · Network 5</span>
      </header>
      <div class="p-body">
        <div v-if="hwSubs.every(s => s.score === 0)" class="empty-mini">
          硬件评分还未生成 · 启动后 24h 内首次评估
        </div>
        <div v-else class="subscore-grid">
          <div v-for="s in hwSubs" :key="s.key" class="subscore-item">
            <div class="ss-head">
              <span class="ss-label">{{ s.label }}</span>
              <span class="ss-val mono" :style="{ color: scoreColor(s.score) }">{{ s.score }}</span>
            </div>
            <div class="ss-bar-track">
              <div
                class="ss-bar-fill"
                :style="{ width: `${s.score}%`, background: scoreColor(s.score) }"
              />
            </div>
          </div>
        </div>
      </div>
    </section>

    <!-- ════════ 信誉 4 子分 ════════ -->
    <section class="panel">
      <header class="p-head">
        <span class="p-title">信誉评分 · 4 维度 (加权几何平均)</span>
        <span class="p-meta">完成任务越多 · 评分越精准</span>
      </header>
      <div class="p-body">
        <div v-if="repSubs.length === 0" class="empty-mini">
          信誉数据加载中
        </div>
        <div v-else class="subscore-grid">
          <div v-for="s in repSubs" :key="s.key" class="subscore-item">
            <div class="ss-head">
              <span class="ss-label">
                {{ s.label }}
                <span class="ss-weight">权重 {{ s.weight }}%</span>
              </span>
              <span class="ss-val mono" :style="{ color: scoreColor(s.score) }">{{ s.score }}</span>
            </div>
            <div class="ss-bar-track">
              <div
                class="ss-bar-fill"
                :style="{ width: `${s.score}%`, background: scoreColor(s.score) }"
              />
            </div>
          </div>
        </div>
      </div>
    </section>

    <!-- ════════ 改进建议 ════════ -->
    <section class="panel">
      <header class="p-head">
        <span class="p-title">系统建议</span>
        <span class="p-meta">{{ suggestions.length }} 条</span>
      </header>
      <div v-if="suggestions.length === 0" class="empty">
        <Icon name="status-done" :size="32" />
        <div>暂无建议 · 你的节点表现良好 ✨</div>
      </div>
      <ul v-else class="sg-list">
        <li v-for="(s, i) in suggestions" :key="i" :class="`sg-item level-${s.level}`">
          <span class="sg-icon">{{ suggestionIcon(s.level) }}</span>
          <div class="sg-body">
            <div class="sg-msg">{{ s.message }}</div>
            <div class="sg-advice">{{ s.advice }}</div>
          </div>
          <span class="sg-cat">{{ s.category }}</span>
        </li>
      </ul>
    </section>
  </div>
</template>

<style scoped>
.page { display: flex; flex-direction: column; gap: var(--sp-6); }

.page-head { display: flex; align-items: flex-end; justify-content: space-between; }
.page-title {
  margin: 0;
  font-size: var(--fs-xl);
  font-weight: var(--fw-semibold);
  letter-spacing: -0.02em;
  color: var(--c-fg);
  display: flex;
  align-items: center;
  gap: 10px;
}
.page-sub { margin: 2px 0 0; font-size: var(--fs-sm); color: var(--c-mute); }

.badge {
  display: inline-flex;
  align-items: center;
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  padding: 2px 8px;
  border-radius: var(--r-pill);
  background: var(--c-bg-soft);
  color: var(--c-fg-soft);
  border: 1px solid var(--c-line);
}
.badge.tone-ok { background: rgba(63,185,80,0.12); color: var(--c-ok); border-color: rgba(63,185,80,0.3); }
.badge.tone-warn { background: rgba(255,166,87,0.12); color: var(--c-warn); border-color: rgba(255,166,87,0.3); }
.badge.tone-err { background: rgba(248,81,73,0.12); color: var(--c-err); border-color: rgba(248,81,73,0.3); }
.badge.tone-info { background: rgba(56,139,253,0.12); color: var(--c-brand-2); border-color: rgba(56,139,253,0.3); }

.refresh-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 14px;
  font-size: var(--fs-sm);
  font-weight: var(--fw-medium);
  background: var(--c-bg-card);
  color: var(--c-fg-soft);
  border: 1px solid var(--c-line);
  border-radius: var(--r-sm);
  cursor: pointer;
  transition: all var(--dur-base);
}
.refresh-btn:hover:not(:disabled) { background: var(--c-bg-soft); color: var(--c-fg); }
.refresh-btn:disabled { opacity: 0.6; cursor: wait; }

.spinner-mini {
  width: 12px; height: 12px;
  border: 1.5px solid var(--c-line);
  border-top-color: var(--c-brand);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }

.alert {
  padding: 12px 16px;
  border-radius: var(--r-md);
  font-size: var(--fs-sm);
  display: flex;
  align-items: center;
  gap: 8px;
}
.alert-err { background: rgba(248,81,73,0.08); color: var(--c-err); border: 1px solid rgba(248,81,73,0.3); }
.alert-info { background: rgba(56,139,253,0.08); color: var(--c-brand-2); border: 1px solid rgba(56,139,253,0.3); }

.kpi-row {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--sp-5);
}
@media (max-width: 1100px) { .kpi-row { grid-template-columns: repeat(2, 1fr); } }

.panel {
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md);
  overflow: hidden;
}
.p-head {
  display: flex; align-items: center; justify-content: space-between;
  padding: 10px var(--sp-5);
  border-bottom: 1px solid var(--c-line);
}
.p-title {
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--c-fg-soft);
}
.p-meta { font-size: var(--fs-xs); color: var(--c-mute); }
.p-body { padding: var(--sp-5); }

.subscore-grid {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.subscore-item {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.ss-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
}
.ss-label {
  font-size: var(--fs-sm);
  color: var(--c-fg);
  font-weight: var(--fw-medium);
  display: flex;
  align-items: baseline;
  gap: 8px;
}
.ss-weight {
  font-size: var(--fs-2xs);
  color: var(--c-mute);
  font-weight: var(--fw-normal);
}
.ss-val {
  font-size: var(--fs-lg);
  font-weight: var(--fw-semibold);
  font-variant-numeric: tabular-nums;
}
.ss-bar-track {
  height: 6px;
  background: var(--c-bg-soft);
  border-radius: var(--r-pill);
  overflow: hidden;
}
.ss-bar-fill {
  height: 100%;
  border-radius: var(--r-pill);
  transition: width var(--dur-slow) ease;
}

.empty {
  padding: 56px;
  text-align: center;
  color: var(--c-mute);
  font-size: var(--fs-sm);
  display: flex; flex-direction: column; align-items: center; gap: 8px;
}
.empty :deep(.qs-icon) { color: var(--c-faint); }
.empty-mini { padding: 24px; text-align: center; color: var(--c-mute); font-size: var(--fs-sm); }

.sg-list {
  list-style: none;
  margin: 0;
  padding: 0;
}
.sg-item {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 14px var(--sp-5);
  border-bottom: 1px solid var(--c-line);
}
.sg-item:last-child { border-bottom: none; }
.sg-item.level-praise { background: rgba(63,185,80,0.04); }
.sg-item.level-warn,
.sg-item.level-warning { background: rgba(255,166,87,0.04); }
.sg-item.level-err { background: rgba(248,81,73,0.04); }
.sg-icon { font-size: 18px; flex-shrink: 0; line-height: 1.4; }
.sg-body { flex: 1; display: flex; flex-direction: column; gap: 4px; }
.sg-msg { font-size: var(--fs-sm); color: var(--c-fg); font-weight: var(--fw-medium); }
.sg-advice { font-size: var(--fs-xs); color: var(--c-mute); }
.sg-cat {
  align-self: flex-start;
  font-size: var(--fs-2xs);
  color: var(--c-faint);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  padding: 2px 6px;
  border: 1px solid var(--c-line);
  border-radius: var(--r-sm);
}

.mono { font-family: ui-monospace, monospace; }
</style>
