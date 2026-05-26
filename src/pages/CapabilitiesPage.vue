<script setup lang="ts">
/**
 * 能力中心 · 5 设备贡献能力详情页
 *
 * 布局:
 *   - 顶部 Hero stat 4 卡 (已授权 / 运行中 / 算力今日 ¥ / 算力累计 ¥)
 *   - 贡献矩阵 (CapabilityGrid 重用 · 用于快速 toggle)
 *   - 5 个详情 section · 锚点跳转 · 每个含工作原理 + 贡献价值 + 协议
 *
 * 收益策略: 仅 compute 显示 ¥ 给用户 · 其他 4 项贡献给平台和渠道商
 *           用户看到的是 '贡献价值描述' 而非收益数字
 */
import { computed, ref, onMounted } from "vue"
import Icon from "../components/Icon.vue"
import StatCard from "../components/dashboard/StatCard.vue"
import CapabilityGrid from "../components/dashboard/CapabilityGrid.vue"
import ConsentMatrixModal from "../components/ConsentMatrixModal.vue"
import { useCapabilities, type CapabilityId } from "../composables/useCapabilities"
import { PRIMARY_DOMAIN } from "@shared"

const {
  capabilities,
  consentedCount,
  activeCount,
  toggleConsent,
  resetConsent,
} = useCapabilities()

const showConsentModal = ref(false)

// 算力收益 (只取 compute · 不聚合其他能力)
const computeCard = computed(() => capabilities.value.find((c) => c.id === "compute"))
const computeToday = computed(() => computeCard.value?.todayEarnings ?? 0)
const computeTotal = computed(() => computeCard.value?.totalEarnings ?? 0)
// 4 个非 compute 能力的总贡献次数 (mock · 后期接 metrics)
const contributionTotalCount = computed(() =>
  capabilities.value
    .filter((c) => !c.userEarns)
    .reduce((sum, c) => sum + (c.contributionCount ?? 0), 0),
)

// 每个能力的详情元数据 (不放 composable · 仅页面用)
interface CapabilityDetail {
  /** 工作原理 (3-5 句) */
  howItWorks: string[]
  /**
   * 价值/收益模式描述
   * - compute: 用户得益的收益方式说明
   * - 其他 4 项: 贡献给平台 / 合作伙伴的价值定位
   */
  valueModel: string
  /** 隐私 / 安全提示 */
  safety: string
  /** FAQ 链接 */
  faqAnchor: string
}

const DETAILS: Record<CapabilityId, CapabilityDetail> = {
  compute: {
    howItWorks: [
      "节点空闲时自动接收平台派发的 AI 推理 / 图像视频处理任务",
      "任务在沙盒环境运行 · 资源占用受您设置的算力比例 (0-100%) 限制",
      "完成任务后结果上传平台 · 平台验证通过后实时入账",
    ],
    valueModel: "按任务即时结算到您账户 · 报价取决于复杂度与平台单价 · 算力越强单位时间收益越高",
    safety: "所有任务镜像由平台签名 · 沙盒执行 · 严禁访问您本地文件",
    faqAnchor: "compute-faq",
  },
  crawl: {
    howItWorks: [
      "节点接收平台分发的「公开数据采集任务」(商品价格指数 / 学术索引)",
      "节点用您的网络请求目标公开页面 · 解析后返回结构化数据",
      "所有 URL 经平台白名单审核 · 禁止抓取登录后内容 / 个人隐私 / 版权页面",
    ],
    valueModel: "贡献给科研机构和市场研究机构 · 帮助他们获取公开数据样本 · 由平台统一运营",
    safety: "禁止抓取个人信息 / 版权内容 · 平台为采集行为的合规性负责",
    faqAnchor: "crawl-faq",
  },
  proxy: {
    howItWorks: [
      "节点出口 IP 加入平台公共 IP 池",
      "用于公开数据采集 · 合规 API 调用",
      "不做代理转发 · 不承载第三方流量 · 用户随时可关",
    ],
    valueModel: "为合规数据采集提供 IP 池 · 由平台统一运营 · 用户仅贡献出口 IP",
    safety: "严禁中转任何流量 · 仅用于声明用途 · 平台对调用合规性负责",
    faqAnchor: "proxy-faq",
  },
  script: {
    howItWorks: [
      "节点接收平台签名的通用脚本 (Python/Shell)",
      "脚本在沙盒环境运行 · 受您设置的资源比例限制",
      "脚本均经平台审核 + 签名校验 · 任意脚本可拒绝",
    ],
    valueModel: "按脚本运行时长 + 复杂度结算到您账户 · 适合 B 端自定义批量处理",
    safety: "所有脚本均带平台签名 · 沙盒隔离 · 严禁访问您本地敏感文件",
    faqAnchor: "script-faq",
  },
}

function legalUrl(id: CapabilityId | "tos" | "privacy"): string {
  return `${PRIMARY_DOMAIN}/legal/${id}`
}

function scrollToAnchor(anchor: string) {
  const el = document.getElementById(`cap-${anchor}`)
  if (el) el.scrollIntoView({ behavior: "smooth", block: "start" })
}

function onGridDetail(id: CapabilityId) { scrollToAnchor(id) }
function onGridLearnMore(id: CapabilityId) { scrollToAnchor(id) }

function openConsentModal() { showConsentModal.value = true }
function closeConsentModal() { showConsentModal.value = false }

// 处理 hash anchor (从外部跳进来 · 例: #compute)
onMounted(() => {
  if (location.hash) {
    const id = location.hash.replace("#", "")
    setTimeout(() => scrollToAnchor(id), 200)
  }
})
</script>

<template>
  <div class="cap-page">
    <!-- ─── Hero ─── -->
    <header class="page-hero">
      <div>
        <h1 class="page-title">
          <Icon name="task-render" :size="22" class="page-title-icon" />
          能力中心
        </h1>
        <p class="page-sub">
          一台设备 · 多项资源贡献 · 算力收益归您 · 其他贡献支撑平台公共服务 · 您可随时撤回
        </p>
      </div>
      <div class="page-actions">
        <button class="btn-ghost" @click="resetConsent" title="清空所有授权 (Debug)">
          重置授权
        </button>
        <button class="btn-primary" @click="openConsentModal">
          <Icon name="task-render" :size="14" />
          管理授权矩阵
        </button>
      </div>
    </header>

    <!-- ─── Hero Stats ─── -->
    <div class="kpi-row">
      <StatCard
        label="已授权能力"
        :value="`${consentedCount}/4`"
        :hint="consentedCount === 4 ? '满级授权' : `还可开放 ${4 - consentedCount} 个`"
        accent="brand"
        icon="task-render"
      />
      <StatCard
        label="运行中能力"
        :value="activeCount"
        :hint="activeCount > 0 ? '设备正在贡献中' : '暂未启用任何已上线能力'"
        accent="ok"
        icon="status-running"
      />
      <StatCard
        label="算力今日收益"
        :value="computeToday.toFixed(2)"
        unit="EDG"
        hint="算力收益归您 · 即时结算"
        accent="brand"
        icon="task-compute"
      />
      <StatCard
        label="算力累计收益"
        :value="computeTotal.toFixed(2)"
        unit="EDG"
        hint="全部历史 · 仅算力贡献"
        accent="ok"
        icon="status-done"
      />
    </div>

    <!-- 2026-05-24 · 我的贡献矩阵 5 卡片已隐藏 · 用户认为冗余 · 详情走下方 sections + 顶部 Hero KPI -->
    <!--
    <CapabilityGrid
      @detail="onGridDetail"
      @learn-more="onGridLearnMore"
    />
    -->

    <!-- ─── 5 能力详情 sections ─── -->
    <div class="cap-details">
      <article
        v-for="c in capabilities"
        :key="c.id"
        :id="`cap-${c.anchor}`"
        :class="['cap-detail', `status-${c.status}`]"
        :style="{ '--cap-color': c.color }"
      >
        <header class="cd-head">
          <div class="cd-head-left">
            <span class="cd-icon-wrap">
              <Icon :name="(c.icon as any)" :size="20" />
            </span>
            <div>
              <h3 class="cd-name">{{ c.name }}</h3>
              <p class="cd-sub">{{ c.subtitle }}</p>
            </div>
          </div>
          <span :class="['cd-badge', `badge-${c.status}`]">
            {{ c.statusLabel }}
          </span>
        </header>

        <p class="cd-desc">{{ c.description }}</p>

        <!-- 资源 chips -->
        <div class="cd-resources">
          <span v-for="r in c.resources" :key="r" class="cd-chip">
            {{ r }}
          </span>
        </div>

        <!-- 工作原理 -->
        <section class="cd-section">
          <h4 class="cd-section-h">工作原理</h4>
          <ol class="cd-list">
            <li v-for="(step, i) in DETAILS[c.id].howItWorks" :key="i">
              {{ step }}
            </li>
          </ol>
        </section>

        <!-- 价值/收益方式 + 安全 -->
        <div class="cd-two-col">
          <section class="cd-section">
            <h4 class="cd-section-h">{{ c.userEarns ? "收益方式" : "贡献价值" }}</h4>
            <p class="cd-text">{{ DETAILS[c.id].valueModel }}</p>
            <!-- 仅 compute 显示 ¥ 收益 -->
            <div v-if="c.status === 'live' && c.consent && c.userEarns" class="cd-money-line">
              <span class="cd-money-label">今日收益</span>
              <span class="cd-money-val mono">¥{{ (c.todayEarnings ?? 0).toFixed(2) }}</span>
            </div>
            <!-- 非 compute 显示贡献统计 -->
            <div v-else-if="c.status === 'live' && c.consent && !c.userEarns" class="cd-money-line">
              <span class="cd-money-label">已贡献</span>
              <span class="cd-money-val mono">{{ c.contributionCount ?? 0 }} {{ c.contributionUnit }}</span>
            </div>
          </section>
          <section class="cd-section">
            <h4 class="cd-section-h">隐私 & 安全</h4>
            <p class="cd-text">{{ DETAILS[c.id].safety }}</p>
          </section>
        </div>

        <!-- 底部操作 -->
        <footer class="cd-foot">
          <a
            class="cd-legal"
            :href="legalUrl(c.id)"
            target="_blank"
            rel="noopener"
          >
            查看《{{ c.name }}服务协议》→
          </a>
          <div class="cd-foot-right">
            <span v-if="c.expectedLaunch && c.status !== 'live'" class="cd-eta">
              预计 {{ c.expectedLaunch }} 上线
            </span>
            <!-- live: toggle -->
            <button
              v-if="c.status === 'live'"
              :class="['cd-toggle-btn', c.consent ? 'is-on' : '']"
              @click="toggleConsent(c.id)"
            >
              <span class="cd-toggle-dot" />
              {{ c.consent ? "已授权" : "授权使用" }}
            </button>
            <!-- beta -->
            <button
              v-else-if="c.status === 'beta'"
              class="cd-action-beta"
            >
              申请内测
            </button>
            <!-- designing / planning -->
            <button v-else class="cd-action-ghost">
              关注动态
            </button>
          </div>
        </footer>
      </article>
    </div>

    <!-- 同意矩阵 Modal -->
    <ConsentMatrixModal
      :open="showConsentModal"
      mode="settings"
      @close="closeConsentModal"
    />
  </div>
</template>

<style scoped>
.cap-page {
  display: flex;
  flex-direction: column;
  gap: var(--sp-6, 16px);
  max-width: 1200px;
  margin: 0 auto;
}

/* Hero */
.page-hero {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--sp-5, 12px);
}
.page-title {
  margin: 0 0 4px;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: var(--fs-xl, 19px);
  font-weight: var(--fw-bold, 700);
  color: var(--c-fg);
  letter-spacing: -0.02em;
}
.page-title-icon { color: var(--c-brand); }
.page-sub {
  margin: 0;
  font-size: var(--fs-sm, 14px);
  color: var(--c-mute);
}
.page-actions { display: flex; gap: 8px; }
.btn-ghost, .btn-primary {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: var(--fs-xs, 13px);
  font-weight: var(--fw-medium, 500);
  padding: 7px 12px;
  border-radius: var(--r-sm, 6px);
  border: 1px solid transparent;
  cursor: pointer;
  transition: all var(--dur-base, 0.15s);
}
.btn-ghost {
  color: var(--c-mute);
  border-color: var(--c-line);
  background: transparent;
}
.btn-ghost:hover {
  color: var(--c-fg);
  border-color: var(--c-line-strong);
}
.btn-primary {
  color: #fff;
  background: var(--c-brand);
  border-color: var(--c-brand);
}
.btn-primary:hover { filter: brightness(1.1); }

/* KPI row · 4 等分 */
.kpi-row {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--sp-4, 10px);
}
@media (max-width: 900px) { .kpi-row { grid-template-columns: repeat(2, 1fr); } }
@media (max-width: 480px) { .kpi-row { grid-template-columns: 1fr; } }

/* 详情列表 */
.cap-details {
  display: flex;
  flex-direction: column;
  gap: var(--sp-5, 12px);
}

.cap-detail {
  position: relative;
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md, 8px);
  padding: var(--sp-6, 16px);
  scroll-margin-top: 20px;
  border-left: 3px solid var(--cap-color, var(--c-line));
}
.cap-detail.status-designing,
.cap-detail.status-planning {
  opacity: 0.92;
}

/* head */
.cd-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: var(--sp-4, 10px);
}
.cd-head-left { display: flex; align-items: center; gap: 12px; }
.cd-icon-wrap {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  border-radius: var(--r-md, 8px);
  background: color-mix(in srgb, var(--cap-color) 14%, transparent);
  color: var(--cap-color, var(--c-fg));
}
.cd-name {
  margin: 0;
  font-size: var(--fs-lg, 17px);
  font-weight: var(--fw-bold, 700);
  color: var(--c-fg);
  letter-spacing: -0.01em;
}
.cd-sub {
  margin: 2px 0 0;
  font-size: var(--fs-xs, 13px);
  color: var(--c-mute);
}
.cd-badge {
  font-size: var(--fs-2xs, 12px);
  font-weight: var(--fw-semibold, 600);
  padding: 3px 9px;
  border-radius: var(--r-pill, 999px);
}
.cd-badge.badge-live      { color: var(--c-ok);    background: var(--c-ok-soft); }
.cd-badge.badge-beta      { color: var(--c-warn);  background: var(--c-warn-soft); }
.cd-badge.badge-designing { color: var(--c-mute);  background: var(--c-bg-soft); }
.cd-badge.badge-planning  { color: var(--c-faint); background: var(--c-bg-soft); }

.cd-desc {
  margin: 0 0 var(--sp-4, 10px);
  font-size: var(--fs-sm, 14px);
  color: var(--c-fg-soft);
  line-height: 1.6;
}

/* resources chips */
.cd-resources {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: var(--sp-5, 12px);
}
.cd-chip {
  font-size: var(--fs-2xs, 12px);
  color: var(--c-mute);
  padding: 3px 9px;
  background: var(--c-bg-soft);
  border: 1px solid var(--c-line);
  border-radius: var(--r-pill, 999px);
  font-family: ui-monospace, monospace;
}

/* sections */
.cd-section {
  margin-bottom: var(--sp-4, 10px);
}
.cd-section-h {
  font-size: var(--fs-xs, 13px);
  font-weight: var(--fw-semibold, 600);
  color: var(--c-mute);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  margin: 0 0 6px;
}
.cd-list {
  margin: 0;
  padding-left: 18px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.cd-list li {
  font-size: var(--fs-xs, 13px);
  color: var(--c-fg-soft);
  line-height: 1.55;
}
.cd-text {
  margin: 0;
  font-size: var(--fs-xs, 13px);
  color: var(--c-fg-soft);
  line-height: 1.55;
}

.cd-two-col {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--sp-5, 12px);
  padding: var(--sp-4, 10px);
  background: var(--c-bg);
  border-radius: var(--r-sm, 6px);
  border: 1px dashed var(--c-line);
  margin-bottom: var(--sp-4, 10px);
}
@media (max-width: 640px) { .cd-two-col { grid-template-columns: 1fr; } }

.cd-money-line {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin-top: 8px;
  padding-top: 8px;
  border-top: 1px dashed var(--c-line);
}
.cd-money-label {
  font-size: var(--fs-2xs, 12px);
  color: var(--c-mute);
}
.cd-money-val {
  font-size: var(--fs-md, 15px);
  font-weight: var(--fw-bold, 700);
  color: var(--cap-color, var(--c-fg));
}

/* foot */
.cd-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding-top: var(--sp-3, 8px);
  border-top: 1px solid var(--c-line);
}
.cd-legal {
  font-size: var(--fs-xs, 13px);
  color: var(--c-mute);
  text-decoration: none;
  transition: color var(--dur-base, 0.15s);
}
.cd-legal:hover { color: var(--c-brand); }

.cd-foot-right { display: flex; align-items: center; gap: 10px; }
.cd-eta {
  font-size: var(--fs-2xs, 12px);
  color: var(--c-faint);
  font-family: ui-monospace, monospace;
}

.cd-toggle-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: var(--fs-xs, 13px);
  font-weight: var(--fw-medium, 500);
  padding: 6px 14px;
  border-radius: var(--r-sm, 6px);
  border: 1px solid var(--c-line);
  background: var(--c-bg-soft);
  color: var(--c-fg);
  cursor: pointer;
  transition: all var(--dur-base, 0.15s);
}
.cd-toggle-btn:hover { border-color: var(--c-line-strong); }
.cd-toggle-dot {
  width: 8px; height: 8px;
  border-radius: 50%;
  background: var(--c-faint);
}
.cd-toggle-btn.is-on {
  color: var(--cap-color, var(--c-brand));
  border-color: var(--cap-color, var(--c-brand));
  background: color-mix(in srgb, var(--cap-color) 14%, transparent);
}
.cd-toggle-btn.is-on .cd-toggle-dot {
  background: var(--cap-color, var(--c-brand));
  box-shadow: 0 0 6px var(--cap-color, var(--c-brand));
}

.cd-action-beta {
  font-size: var(--fs-xs, 13px);
  font-weight: var(--fw-medium, 500);
  padding: 6px 14px;
  border-radius: var(--r-sm, 6px);
  border: 1px solid var(--c-warn);
  background: var(--c-warn-soft);
  color: var(--c-warn);
  cursor: pointer;
  transition: all var(--dur-base, 0.15s);
}
.cd-action-beta:hover { filter: brightness(1.1); }

.cd-action-ghost {
  font-size: var(--fs-xs, 13px);
  font-weight: var(--fw-medium, 500);
  padding: 6px 14px;
  border-radius: var(--r-sm, 6px);
  border: 1px solid var(--c-line);
  background: transparent;
  color: var(--c-mute);
  cursor: pointer;
  transition: all var(--dur-base, 0.15s);
}
.cd-action-ghost:hover {
  color: var(--c-fg);
  border-color: var(--c-line-strong);
  background: var(--c-bg-soft);
}

.mono { font-family: ui-monospace, "SF Mono", Menlo, monospace; }
</style>
