<script setup lang="ts">
/**
 * 算力驾舱 · 次时代重设计 (2026-05-21)
 *
 * 布局:
 *   Row 1: 4 张 KPI Hero 卡 (累计收益 · 今日完成 · 在线时长 · 平均收益/任务)
 *   Row 2: 12 列网格
 *     左 8 列: 实时任务流 TaskRunFeed
 *     右 4 列:
 *       - 节点状态 (mode/throttle/延迟/算力)
 *       - 7日收益 sparkline
 *       - 运行时就绪情况
 *
 * 所有数据真实 · 不使用 mock
 */
import { computed, onMounted, ref } from "vue"
import { invoke } from "@tauri-apps/api/core"
import TaskRunFeed from "../components/TaskRunFeed.vue"
import StatCard from "../components/dashboard/StatCard.vue"
import MiniSpark from "../components/dashboard/MiniSpark.vue"
// CapabilityGrid 已从主面板移除 (用户从 NavRail · 能力中心 进入查看)
import AdSlot from "../components/ads/AdSlot.vue"
import type { CapabilityId } from "../composables/useCapabilities"
import { useAccount } from "../composables/useAccount"
import { useConnection } from "../composables/useConnection"
import { useEarnings } from "../composables/useEarnings"
import { useTasks } from "../composables/useTasks"
import { useRuntime } from "../composables/useRuntime"
import { useHistory } from "../composables/useHistory"
import { useNav } from "../composables/useNav"
import { useOpSlots } from "../composables/useOpSlots"
import { PRIMARY_DOMAIN } from "@shared"

const { account } = useAccount()
const { snap } = useConnection()
const { series } = useEarnings(7)
const { running, verifying, queued, done } = useTasks()
const { manifest, installed, statusOf } = useRuntime()
const { items: histItems, refreshQuiet: refreshHistoryQuiet } = useHistory()
const { goto } = useNav()

// 2026-05-25 · 广告位空状态控制 · 后端没推数据时显示占位卡 (引导后台运营)
const { banner: opBanner, activity: opActivity } = useOpSlots()
const bannerEmpty = computed(() => opBanner.value.length === 0)
const activityEmpty = computed(() => opActivity.value.length === 0)

// catalog (用于能力速览 · 与 AICapability 同源)
// 2026-05-26 · 改走 invoke api_get · 避免 WebKit 跨域 CORS block
interface CatalogItem { task_type: string; required_software: string[] }
const catalogItems = ref<CatalogItem[]>([])
async function loadCatalog() {
  try {
    const url = `${PRIMARY_DOMAIN}/api/v8/runtime/task-catalog`
    const body = await invoke<string>("api_get", { url })
    const d = JSON.parse(body)
    catalogItems.value = (d.items || []) as CatalogItem[]
  } catch {}
}

onMounted(() => {
  refreshHistoryQuiet().catch(() => {})
  loadCatalog()
})

// ── KPI 计算 ──
const totalEarnings = computed(() => account.value?.total_earnings ?? 0)
const balance = computed(() => account.value?.balance ?? 0)
const completedTasks = computed(() => account.value?.completed_tasks ?? 0)

// 今日完成: 从 useTasks.done 算 (本会话) + 从 history 算 (今天)
const todayDone = computed(() => {
  // 优先用 history (跨重启)
  const start = new Date(); start.setHours(0, 0, 0, 0)
  const startMs = start.getTime()
  let okN = 0, failN = 0
  for (const h of histItems.value) {
    const t = h.completed_at ? new Date(h.completed_at).getTime() : 0
    if (t >= startMs) {
      if (h.status === "ok") okN++
      else if (h.status === "failed") failN++
    }
  }
  return { ok: okN, fail: failN, total: okN + failN }
})

// 7 日趋势 (供 KPI hint 显示 mini spark)
const earningSeries = computed(() => series.value.map((p) => Number(p.earnings || 0)))
const todayEarning = computed(() => {
  if (series.value.length === 0) return 0
  return Number(series.value[series.value.length - 1].earnings || 0)
})
const yesterdayEarning = computed(() => {
  if (series.value.length < 2) return 0
  return Number(series.value[series.value.length - 2].earnings || 0)
})
const earningsTrend = computed(() => {
  if (yesterdayEarning.value === 0) return 0
  return ((todayEarning.value - yesterdayEarning.value) / yesterdayEarning.value) * 100
})

// 平均每任务收益
const avgPerTask = computed(() => {
  if (completedTasks.value === 0) return 0
  return totalEarnings.value / completedTasks.value
})

// ── 节点状态 ──
const onlineLabel = computed(() => {
  const s = snap.value.connection_state
  if (s === "registered") return "在线"
  if (s === "connecting" || s === "authenticating" || s === "reconnecting") return snap.value.state_label
  return "离线"
})
const onlineAccent = computed<"ok" | "warn" | "err">(() => {
  const s = snap.value.connection_state
  if (s === "registered") return "ok"
  if (s === "disconnected") return "err"
  return "warn"
})
const latencyText = computed(() => {
  const l = snap.value.latency_ms
  return l != null ? `${l}ms` : "—"
})

// ── 运行时就绪 ──
const runtimeReady = computed(() => {
  const tiers = manifest.value?.tiers ?? {}
  const names = Object.keys(tiers)
  const ready = names.filter((t) => statusOf(t) === "ready")
  return { ready: ready.length, total: names.length, names: ready }
})

// ── 环境检测 (取代原 "最近完成" widget) ──
// 数据源: useRuntime 的 manifest.tiers + statusOf(key) → ready/installing/missing/failed
interface EnvCheckItem {
  key: string
  label: string
  status: string
  required: boolean
  missing: boolean
}
const envCheck = computed(() => {
  const tiers = manifest.value?.tiers || {}
  const list: EnvCheckItem[] = []
  let missing = 0
  for (const [key, spec] of Object.entries(tiers)) {
    const st = statusOf(key)
    const isMissing = st === "missing" || st === "failed"
    list.push({
      key,
      label: spec.description || key,
      status: st,
      required: !!spec.required,
      missing: isMissing,
    })
    if (isMissing) missing++
  }
  // 排序: 缺的在上 + required 在上
  list.sort((a, b) => {
    if (a.missing !== b.missing) return a.missing ? -1 : 1
    if (a.required !== b.required) return a.required ? -1 : 1
    return a.label.localeCompare(b.label)
  })
  return { list, missing, total: list.length }
})
function envStatusText(st: string): string {
  switch (st) {
    case "ready": return "✓ 已装"
    case "installing": return "装中"
    case "missing": return "未装"
    case "failed": return "装失败"
    default: return st
  }
}

// ── 任务类型分布 (top 5) ──
const typeDist = computed<{ type: string; n: number; pct: number }[]>(() => {
  const m: Record<string, number> = {}
  for (const h of histItems.value) {
    const t = h.task_type || "(unknown)"
    m[t] = (m[t] || 0) + 1
  }
  const arr = Object.entries(m).sort((a, b) => b[1] - a[1]).slice(0, 5)
  const max = arr[0]?.[1] || 1
  return arr.map(([type, n]) => ({ type, n, pct: (n / max) * 100 }))
})

// ── 能力速览 (catalog × installed software) ──
const installedSw = computed<Set<string>>(() => {
  const s = new Set<string>(["python3", "shell"])
  for (const t of Object.values(installed.value.tiers || {})) {
    if (t.ok) for (const sw of t.software || []) s.add(sw)
  }
  return s
})
const capStats = computed(() => {
  const items = catalogItems.value
  const total = items.length
  let ready = 0
  for (const sp of items) {
    const miss = (sp.required_software || []).filter((sw) => !installedSw.value.has(sw))
    if (miss.length === 0) ready++
  }
  return { total, ready, missing: total - ready }
})

// ── 能力矩阵交互 (跳转到能力中心) ──
function onCapabilityDetail(_id: CapabilityId) {
  goto("capabilities")
}
function onCapabilityLearnMore(_id: CapabilityId) {
  goto("capabilities")
}
</script>

<template>
  <div class="dash">
    <!-- ─── Row 1: 4 张 Hero KPI ─── -->
    <div class="kpi-row">
      <StatCard
        label="累计收益"
        :value="totalEarnings.toFixed(2)"
        unit="EDG"
        accent="brand"
        icon="coin"
      >
        <div class="kpi-with-spark">
          <span>今日 <b class="mono">{{ todayEarning.toFixed(2) }}</b></span>
          <MiniSpark v-if="earningSeries.length" :series="earningSeries" :width="80" :height="22" />
        </div>
      </StatCard>

      <StatCard
        label="今日任务"
        :value="todayDone.total"
        :hint="todayDone.fail
          ? `完成 ${todayDone.ok} · 失败 ${todayDone.fail}`
          : `全部成功 ${todayDone.ok}`"
        accent="ok"
        icon="status-done"
        :trend="earningsTrend"
      />

      <StatCard
        label="实时进行"
        :value="running.length + verifying.length"
        :hint="`排队 ${queued.length} · 已完成 ${done.length}`"
        accent="warn"
        icon="status-running"
      />

      <StatCard
        label="节点状态"
        :value="onlineLabel"
        :unit="latencyText !== '—' ? `延迟 ${latencyText}` : ''"
        :hint="snap.node_id ? `节点 ${snap.node_id.slice(0,14)}` : '未连接'"
        :accent="onlineAccent"
        icon="cpu"
      />
    </div>

    <!-- 5 能力贡献矩阵已从主面板移除 · 用户从左侧导航 '能力中心' 查看 -->
    <!-- 节点身份信息可在「设备信息」页查看 · NoticeMarquee 顶部公告条已在 App.vue -->

    <!-- ─── Row 2: 12 列网格 · 上 (Feed 8×2 大 + banner 4×2) / 下左 (任务类型 + 7日 叠加) / 中 (最近完成 4×2) / 右 (activity 4×2) / 底 (能力速览通栏) ─── -->
    <div class="widget-grid">
      <!-- 环境检测 · 中下 (4×2 = 280px · 替代原 最近完成 · 不依赖任务历史) -->
      <section class="widget w-recent">
        <header class="w-head">
          <span class="w-label">环境检测</span>
          <button class="w-link" @click="goto('toolbox')">工具管理 →</button>
        </header>
        <!-- 检测中 -->
        <div v-if="envCheck.total === 0" class="w-empty">检测中…</div>
        <!-- 全装好 · 亮绿提示 -->
        <div v-else-if="envCheck.missing === 0" class="env-ok">
          <div class="env-ok-emoji">✓</div>
          <div class="env-ok-title">工具都装好了</div>
          <div class="env-ok-sub">{{ envCheck.total }} 个运行时全就绪 · 可接所有任务</div>
        </div>
        <!-- 有缺 · 列出 -->
        <ul v-else class="env-list">
          <li v-for="t in envCheck.list" :key="t.key" :class="['env-li', `st-${t.status}`]">
            <span :class="['env-dot', `st-${t.status}`]" />
            <span class="env-name" :title="t.label">{{ t.label }}<span v-if="t.required" class="env-req">必装</span></span>
            <span class="env-status">{{ envStatusText(t.status) }}</span>
          </li>
        </ul>
        <!-- 底部 CTA · 缺时提示 -->
        <div v-if="envCheck.missing > 0" class="env-foot">
          还有 <b>{{ envCheck.missing }}</b> 个未装 · <a @click="goto('toolbox')">去工具管理 →</a>
        </div>
      </section>

      <!-- TaskRunFeed 实时任务流 (左上 · 大块 · 8 列 × 2 行) -->
      <div class="w-feed">
        <TaskRunFeed />
      </div>

      <!-- 广告位 1 banner (右上 · 4 列 × 2 行) -->
      <div class="widget widget-ad w-ad-banner">
        <AdSlot slot-key="banner" layout="card" :max-items="1" />
        <div v-if="bannerEmpty" class="ad-placeholder" @click="goto('device')">
          <div class="aph-tag">运营位</div>
          <div class="aph-title">📢 此处可投放图文广告</div>
          <div class="aph-sub">后台 op_slots key=banner · 支持图片 / 标题 / CTA 跳转</div>
          <div class="aph-foot">点击查看节点身份 →</div>
        </div>
      </div>

      <!-- 任务类型分布 · 左下上半 (4×1 小) -->
      <section class="widget w-typedist">
        <header class="w-head">
          <span class="w-label">任务类型分布</span>
          <span class="w-meta mono">{{ histItems.length }} 条</span>
        </header>
        <ul v-if="typeDist.length" class="type-dist">
          <li v-for="d in typeDist" :key="d.type">
            <span class="td-name mono">{{ d.type }}</span>
            <span class="td-bar"><span class="td-fill" :style="{ width: d.pct + '%' }" /></span>
            <span class="td-n mono">{{ d.n }}</span>
          </li>
        </ul>
        <div v-else class="w-empty">尚无数据</div>
      </section>

      <!-- 7 日收益 · 左下下半 (4×1 小 · 与任务类型叠加 = 广告位大小) -->
      <section class="widget w-earn">
        <header class="w-head">
          <span class="w-label">7 日收益</span>
          <span class="w-sum mono">+{{ earningSeries.reduce((a,b)=>a+b,0).toFixed(2) }} EDG</span>
        </header>
        <MiniSpark v-if="earningSeries.length" :series="earningSeries" :width="280" :height="44" />
        <div v-else class="w-empty">暂无数据</div>
        <div class="seven-bars">
          <span v-for="(p, i) in series" :key="i" class="bar-cell" :title="`${p.date} · ${p.earnings.toFixed(2)} EDG`">
            <span class="bar-fill" :style="{ height: Math.max(2, (Number(p.earnings) / Math.max(...earningSeries, 0.001)) * 100) + '%' }" />
          </span>
        </div>
      </section>

      <!-- 广告位 2 activity (右下 · 4 列 × 2 行 · 后台 op_slot key=activity 推送) -->
      <div class="widget widget-ad w-ad-activity">
        <AdSlot slot-key="activity" layout="card" :max-items="1" />
        <div v-if="activityEmpty" class="ad-placeholder" @click="goto('toolbox')">
          <div class="aph-tag aph-tag-act">活动位</div>
          <div class="aph-title">🎁 此处可投放活动入口</div>
          <div class="aph-sub">后台 op_slots key=activity · 适合限时活动 / 邀请奖励</div>
          <div class="aph-foot">点击查看运行时 →</div>
        </div>
      </div>

      <!-- 能力速览 (底通栏): 12 列 -->
      <section class="widget w-cap">
        <header class="w-head">
          <span class="w-label">能力速览</span>
          <button class="w-link" @click="goto('ai-capability')">完整矩阵 →</button>
        </header>
        <div class="cap-flex">
          <div class="cap-num-block">
            <div class="cap-row">
              <span class="cap-big mono">{{ capStats.ready }}</span>
              <span class="cap-slash">/</span>
              <span class="cap-big mono small">{{ capStats.total }}</span>
              <span class="cap-lbl">种任务可执行</span>
            </div>
            <div class="cap-pct-bar">
              <span class="cap-pct-fill" :style="{ width: (capStats.total ? (capStats.ready/capStats.total*100) : 0) + '%' }" />
            </div>
            <div v-if="capStats.missing" class="cap-hint">
              还有 <b>{{ capStats.missing }}</b> 种任务缺依赖 · <a @click="goto('toolbox')">去工具管理装</a>
            </div>
            <div v-else class="cap-hint ok">✓ 全部就绪 · 可接所有任务</div>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.dash {
  display: flex;
  flex-direction: column;
  gap: var(--sp-6);
}

/* ─── KPI ─── */
.kpi-row {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--sp-5);
}
@media (max-width: 1100px) {
  .kpi-row { grid-template-columns: repeat(2, 1fr); }
}

.kpi-with-spark {
  display: flex; align-items: center; justify-content: space-between; gap: var(--sp-4);
  font-size: var(--fs-xs);
  color: var(--c-mute);
}
.kpi-with-spark b { color: var(--c-fg-soft); font-weight: var(--fw-semibold); margin-left: 4px; }

/* ─── 12 列 widget 网格 (整齐) ─── */
.widget-grid {
  display: grid;
  grid-template-columns: repeat(12, 1fr);
  grid-auto-rows: 140px;
  gap: var(--sp-5);
}
@media (max-width: 1100px) {
  .widget-grid { grid-template-columns: repeat(6, 1fr); }
}
@media (max-width: 700px) {
  .widget-grid { grid-template-columns: 1fr; grid-auto-rows: auto; }
}

/* 2026-05-25 8.0.11 · grid-template-areas 精确定位 (不依赖 auto-flow) */
/* Feed 大 (8×2) | banner 右上 (4×2) / typ+earn 左下叠加 (4×1+4×1=4×2) | rec 中下 (4×2) | act 右下 (4×2) / cap 底通栏 */
.widget-grid {
  grid-template-areas:
    "feed feed feed feed feed feed feed feed banner banner banner banner"
    "feed feed feed feed feed feed feed feed banner banner banner banner"
    "typ  typ  typ  typ  rec  rec  rec  rec  act    act    act    act"
    "earn earn earn earn rec  rec  rec  rec  act    act    act    act"
    "cap  cap  cap  cap  cap  cap  cap  cap  cap    cap    cap    cap";
}
.w-feed       { grid-area: feed;     min-width: 0; min-height: 0; }
.w-ad-banner  { grid-area: banner; }
.w-typedist   { grid-area: typ; }
.w-earn       { grid-area: earn; }
.w-recent     { grid-area: rec; }
.w-ad-activity{ grid-area: act; }
.w-cap        { grid-area: cap; }
.w-feed > * { height: 100%; }

/* ─── 广告位 widget · AdSlot 内嵌占满 ─── */
/* 2026-05-25 8.0.11 · 位置/大小由 grid-template-areas 控制 (banner / act) */
.widget-ad {
  position: relative;
  padding: 0;
  background: transparent;
  border: none;
  overflow: hidden;
}
.widget-ad :deep(.ad-card-list) {
  margin: 0 !important;
  display: block !important;
  height: 100% !important;
}
.widget-ad :deep(.ad-card) {
  height: 100% !important;
  display: flex !important;
  flex-direction: column !important;
  min-height: 0;
}
.widget-ad :deep(.ad-card-img) {
  flex: 0 0 auto !important;
  aspect-ratio: 16 / 9 !important;
  max-height: 160px !important;
}
.widget-ad :deep(.ad-card-img img) {
  width: 100% !important;
  height: 100% !important;
  object-fit: cover !important;
}
.widget-ad :deep(.ad-card-body) {
  flex: 1 1 auto !important;
  overflow: hidden !important;
  min-height: 0 !important;
  padding: 10px 12px !important;
}
.widget-ad :deep(.ad-card-title) {
  font-size: 14px !important;
  line-height: 1.3 !important;
  margin: 0 0 4px !important;
  display: -webkit-box !important;
  -webkit-line-clamp: 1 !important;
  -webkit-box-orient: vertical !important;
  overflow: hidden !important;
}
.widget-ad :deep(.ad-card-sub) {
  font-size: 12px !important;
  line-height: 1.4 !important;
  margin: 0 !important;
  display: -webkit-box !important;
  -webkit-line-clamp: 2 !important;
  -webkit-box-orient: vertical !important;
  overflow: hidden !important;
}
.widget-ad :deep(.ad-card-foot) {
  flex: 0 0 auto !important;
}

/* 广告位空状态占位卡 · 引导运营后台投放 */
.ad-placeholder {
  position: absolute; inset: 0;
  background: linear-gradient(135deg,
    color-mix(in srgb, var(--c-brand) 12%, var(--c-bg-card) 88%) 0%,
    var(--c-bg-card) 100%);
  border: 1px dashed color-mix(in srgb, var(--c-brand) 40%, transparent);
  border-radius: 12px;
  padding: 14px 16px;
  display: flex; flex-direction: column;
  cursor: pointer;
  transition: all 0.18s;
}
.ad-placeholder:hover {
  border-color: var(--c-brand);
  background: linear-gradient(135deg,
    color-mix(in srgb, var(--c-brand) 18%, var(--c-bg-card) 82%) 0%,
    var(--c-bg-card) 100%);
  transform: translateY(-1px);
}
.aph-tag {
  display: inline-block;
  align-self: flex-start;
  padding: 2px 8px;
  border-radius: 4px;
  background: color-mix(in srgb, var(--c-brand) 25%, transparent);
  color: var(--c-brand);
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.06em;
  margin-bottom: 8px;
  text-transform: uppercase;
}
.aph-tag-act {
  background: color-mix(in srgb, #ec4899 25%, transparent);
  color: #ec4899;
}
.aph-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--c-fg);
  margin-bottom: 4px;
}
.aph-sub {
  font-size: 11px;
  color: var(--c-fg-soft);
  line-height: 1.5;
  flex: 1;
}
.aph-foot {
  font-size: 11px;
  color: var(--c-brand);
  font-weight: 500;
  margin-top: 6px;
}

/* 中屏 (⁤1100px) · 2 列调整: Feed+banner / typ+earn+rec / act+cap */
@media (max-width: 1100px) {
  .widget-grid {
    grid-template-areas:
      "feed feed feed feed feed feed banner banner banner banner banner banner"
      "feed feed feed feed feed feed banner banner banner banner banner banner"
      "typ  typ  typ  typ  typ  typ  rec    rec    rec    rec    rec    rec"
      "earn earn earn earn earn earn rec    rec    rec    rec    rec    rec"
      "act  act  act  act  act  act  act    act    act    act    act    act"
      "cap  cap  cap  cap  cap  cap  cap    cap    cap    cap    cap    cap";
  }
}
/* 小屏 (≤700px) · 1 列瀑布 */
@media (max-width: 700px) {
  .widget-grid {
    grid-template-areas:
      "feed" "banner" "rec" "typ" "earn" "act" "cap";
    grid-template-columns: 1fr;
  }
}

/* ── 统一 widget 卡片 ── */
.widget {
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md);
  padding: var(--sp-5) var(--sp-5) var(--sp-5);
  display: flex; flex-direction: column;
  min-height: 0;
  overflow: hidden;
  transition: border-color var(--dur-base);
}
.widget:hover { border-color: var(--c-line-strong); }

.w-head {
  display: flex; align-items: center; justify-content: space-between;
  margin-bottom: var(--sp-4);
  flex-shrink: 0;
}
.w-label {
  font-size: var(--fs-xs);
  font-weight: var(--fw-semibold);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--c-mute);
}
.w-link {
  font-size: var(--fs-xs);
  color: var(--c-brand);
  font-weight: var(--fw-medium);
  transition: opacity var(--dur-fast);
}
.w-link:hover { opacity: 0.78; }
.w-meta { font-size: var(--fs-xs); color: var(--c-mute); }
.w-sum  { font-size: var(--fs-sm); color: var(--c-ok); font-weight: var(--fw-semibold); }
.w-empty {
  flex: 1;
  display: flex; align-items: center; justify-content: center;
  font-size: var(--fs-xs); color: var(--c-mute);
}

/* (原 最近完成 列表样式 · 仍保留 · 以防后续复用) */
.recent-list {
  list-style: none; margin: 0; padding: 0;
  display: flex; flex-direction: column; gap: 6px;
  flex: 1; overflow-y: auto; min-height: 0;
}
.recent-list::-webkit-scrollbar { width: 4px; }
.recent-list::-webkit-scrollbar-thumb { background: var(--c-line); border-radius: 2px; }

/* 环境检测 · 全装好 面板 */
.env-ok {
  flex: 1;
  display: flex; flex-direction: column;
  align-items: center; justify-content: center;
  gap: 6px;
  text-align: center;
}
.env-ok-emoji {
  font-size: 36px;
  color: var(--c-ok);
  line-height: 1;
  text-shadow: 0 0 12px color-mix(in srgb, var(--c-ok) 40%, transparent);
}
.env-ok-title {
  font-size: var(--fs-sm);
  font-weight: var(--fw-semibold);
  color: var(--c-ok);
}
.env-ok-sub {
  font-size: var(--fs-xs);
  color: var(--c-mute);
}

/* 环境检测 · 缺项列表 */
.env-list {
  list-style: none; margin: 0; padding: 0;
  display: flex; flex-direction: column; gap: 4px;
  flex: 1; overflow-y: auto; min-height: 0;
}
.env-list::-webkit-scrollbar { width: 4px; }
.env-list::-webkit-scrollbar-thumb { background: var(--c-line); border-radius: 2px; }
.env-li {
  display: grid; grid-template-columns: 8px 1fr auto; gap: 8px;
  align-items: center;
  padding: 4px 0;
  font-size: var(--fs-sm);
  border-bottom: 1px solid var(--c-line);
}
.env-li:last-child { border-bottom: none; }
.env-dot { width: 8px; height: 8px; border-radius: 50%; }
.env-dot.st-ready     { background: var(--c-ok); box-shadow: 0 0 6px rgba(63,185,80,0.4); }
.env-dot.st-installing{ background: var(--c-warn); animation: pulse 1.4s infinite; }
.env-dot.st-missing   { background: var(--c-faint); }
.env-dot.st-failed    { background: var(--c-err); }
.env-name {
  color: var(--c-fg-soft);
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  display: flex; align-items: center; gap: 6px;
}
.env-req {
  display: inline-block;
  font-size: 9px;
  padding: 1px 5px;
  border-radius: 3px;
  background: color-mix(in srgb, var(--c-warn) 25%, transparent);
  color: var(--c-warn);
  font-weight: 600;
  letter-spacing: 0.04em;
  flex-shrink: 0;
}
.env-status {
  font-size: var(--fs-xs);
  color: var(--c-mute);
  font-weight: var(--fw-medium);
}
.env-li.st-ready .env-status   { color: var(--c-ok); }
.env-li.st-missing .env-status { color: var(--c-faint); }
.env-li.st-failed .env-status  { color: var(--c-err); }
.env-foot {
  flex-shrink: 0;
  margin-top: 6px;
  font-size: var(--fs-xs);
  color: var(--c-mute);
}
.env-foot b { color: var(--c-warn); font-weight: var(--fw-semibold); }
.env-foot a { color: var(--c-brand); cursor: pointer; text-decoration: underline; }
@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}
.recent-list li {
  display: grid; grid-template-columns: 8px 1fr auto; gap: 8px;
  align-items: center;
  padding: 5px 0;
  font-size: var(--fs-sm);
  border-bottom: 1px solid var(--c-line);
}
.recent-list li:last-child { border-bottom: none; }
.rd-dot { width: 8px; height: 8px; border-radius: 50%; }
.rd-dot.ok { background: var(--c-ok); box-shadow: 0 0 6px rgba(63,185,80,0.4); }
.rd-dot.fail { background: var(--c-err); }
.rd-name {
  color: var(--c-fg-soft);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.rd-rw {
  color: var(--c-warn);
  font-weight: var(--fw-semibold);
  font-size: var(--fs-xs);
}
.rd-rw:not(.has-num) { color: var(--c-faint); }

/* 类型分布柱 */
.type-dist { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
.type-dist li {
  display: grid; grid-template-columns: 90px 1fr 28px;
  align-items: center; gap: 8px;
  font-size: var(--fs-xs);
}
.td-name { color: var(--c-fg-soft); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.td-bar {
  display: block;
  height: 6px;
  background: var(--c-bg-soft);
  border-radius: var(--r-pill);
  overflow: hidden;
}
.td-fill {
  display: block; height: 100%;
  background: linear-gradient(90deg, var(--c-brand), var(--c-brand-2));
  border-radius: var(--r-pill);
  transition: width var(--dur-slow) var(--ease-out);
}
.td-n { color: var(--c-mute); text-align: right; }

/* 能力速览 (底通栏) */
.cap-flex { display: flex; align-items: center; gap: var(--sp-7); flex: 1; }
.cap-num-block { flex: 1; }
.cap-row { display: flex; align-items: baseline; gap: 4px; margin-bottom: var(--sp-3); }
.cap-big { font-size: var(--fs-2xl); font-weight: var(--fw-bold); color: var(--c-fg); letter-spacing: -0.02em; }
.cap-big.small { font-size: var(--fs-lg); color: var(--c-mute); font-weight: var(--fw-medium); }
.cap-slash { color: var(--c-mute); }
.cap-lbl { font-size: var(--fs-sm); color: var(--c-mute); margin-left: 8px; }
.cap-pct-bar {
  height: 6px;
  background: var(--c-bg-soft);
  border-radius: var(--r-pill);
  overflow: hidden;
  margin-bottom: var(--sp-3);
}
.cap-pct-fill {
  display: block; height: 100%;
  background: linear-gradient(90deg, var(--c-ok), var(--c-brand));
  border-radius: var(--r-pill);
  transition: width var(--dur-slow) var(--ease-out);
}
.cap-hint { font-size: var(--fs-sm); color: var(--c-mute); }
.cap-hint.ok { color: var(--c-ok); font-weight: var(--fw-medium); }
.cap-hint b { color: var(--c-warn); font-weight: var(--fw-semibold); }
.cap-hint a { color: var(--c-brand); cursor: pointer; text-decoration: underline; }

/* (旧 side-card 已被 .widget 替代) */

/* meta grid */
.meta-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--sp-4) var(--sp-5);
  margin: 0;
  flex: 1;
}
.meta-grid div { display: flex; flex-direction: column; gap: 2px; }
.meta-grid dt {
  font-size: var(--fs-2xs);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--c-mute);
  margin: 0;
}
.meta-grid dd {
  margin: 0;
  font-size: var(--fs-md);
  font-weight: var(--fw-semibold);
  color: var(--c-fg);
  letter-spacing: -0.01em;
  white-space: nowrap;
}
.meta-grid dd .u { font-size: var(--fs-xs); color: var(--c-mute); font-weight: var(--fw-medium); margin-left: 2px; }

/* 7日柱 */
.seven-bars {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: 3px;
  height: 22px;
  margin-top: 6px;
}
.bar-cell {
  display: flex; flex-direction: column; justify-content: flex-end;
  background: var(--c-bg-soft);
  border-radius: 2px;
  overflow: hidden;
}
.bar-fill {
  display: block;
  background: linear-gradient(180deg, var(--c-brand-2), var(--c-brand));
  border-radius: 1px;
  transition: height var(--dur-slow) var(--ease-out);
}

/* 运行时 row */
.rt-row {
  display: flex; align-items: baseline; gap: 4px;
  margin-bottom: var(--sp-4);
}
.rt-num { font-size: var(--fs-xl); font-weight: var(--fw-bold); color: var(--c-fg); letter-spacing: -0.02em; }
.rt-num.small { font-size: var(--fs-md); color: var(--c-mute); font-weight: var(--fw-medium); }
.rt-slash { color: var(--c-mute); }
.rt-label { font-size: var(--fs-xs); color: var(--c-mute); margin-left: 4px; }

.rt-list { display: flex; flex-wrap: wrap; gap: 4px; }
.rt-pill {
  display: inline-flex; align-items: center; gap: 4px;
  font-size: var(--fs-2xs);
  font-family: ui-monospace, monospace;
  padding: 2px 8px;
  border-radius: var(--r-pill);
  background: var(--c-bg-soft);
  color: var(--c-mute);
  border: 1px solid var(--c-line);
}
.rt-pill.s-ready { background: var(--c-ok-soft); color: var(--c-ok); border-color: rgba(63,185,80,0.3); }
.rt-pill.s-installing { background: var(--c-warn-soft); color: var(--c-warn); }
.rt-pill.s-failed { background: var(--c-err-soft); color: var(--c-err); }
.rt-dot {
  width: 5px; height: 5px;
  border-radius: 50%;
  background: currentColor;
}
.rt-pill.s-ready .rt-dot { animation: ok-pulse 2.4s ease-in-out infinite; }
</style>
