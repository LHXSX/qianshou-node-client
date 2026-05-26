/**
 * 千手节点 · 4 能力数据模型 (2026-05-24)
 *
 * 一台设备的闲置资源可被 4 条收益线复用：
 *   compute  算力贡献   CPU/GPU 跑 AI/图像/视频任务      ✅ 已上线
 *   crawl    数据采集   帮 B 端抓取公开网络数据          🔜 内测中
 *   proxy    IP 池      出口 IP 加入平台 IP 池            🔜 设计中
 *   script   通用脚本   接收平台分发的任意签名脚本       � 内测中
 *
 * 历史: 2026-05-24 砍掉 display/storage (产品聚焦)
 *
 * 命名采用"去敏感化"策略：避开"代理/爬虫/广告/botnet"等敏感词，
 * 降低用户警觉、规避法律风险用语联想。
 *
 * 本 composable 提供:
 *   - 4 能力元数据 (capabilities)
 *   - 每个能力的同意状态 (localStorage 持久化 · 后端 sync 在 future)
 *   - 已授权 + 已上线能力的实时收益聚合 (基于 useEarnings)
 *   - 切换同意 (toggleConsent)
 */
import { computed, ref } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { apiUrl } from "@shared"
import { useEarnings } from "./useEarnings"
import { useAccount } from "./useAccount"

export type CapabilityId = "compute" | "crawl" | "proxy" | "script"

export type CapabilityStatus =
  | "live"      // 已上线 · 用户可开关 · 真实收益
  | "beta"      // 内测中 · 用户可申请 · 部分收益
  | "designing" // 设计中 · 仅展示 · 无收益
  | "planning"  // 规划中 · 仅展示 · 无收益

export interface Capability {
  id: CapabilityId
  /** 对外名 (用户可见 · 去敏感化) */
  name: string
  /** 一行副标题 */
  subtitle: string
  /** 详细描述 (详情页用) */
  description: string
  /** Icon 组件 name */
  icon: string
  /** 主色 (tailwind hex) */
  color: string
  /** 状态 */
  status: CapabilityStatus
  /** 状态徽章文案 */
  statusLabel: string
  /** 该能力使用到的资源 (展示用) */
  resources: string[]
  /** 预计上线时间 (非 live 才有) */
  expectedLaunch?: string
  /** 详情页 anchor (路由 hash) */
  anchor: string
  /**
   * 是否给用户展示真实收益 (¥)
   * - true: 用户自己赚 (compute)
   * - false: 收益归平台 + 渠道商 · 用户仅贡献设备 · UI 不显示金额
   */
  userEarns: boolean
  /**
   * 贡献价值描述 (替代收益数字 · 展示给用户)
   * 仅 userEarns=false 才用 (前 4 个能力)
   * e.g. "助力科研数据采集 · 已贡献 N 次"
   */
  valueDesc: string
  /** 贡献单位 (用于 mock 统计 · 后期换真实数据) */
  contributionUnit: string
}

/**
 * 5 能力静态元数据
 *
 * userEarns 设计原则:
 *   - compute · 用户自己赚 (¥ 即时结算) · userEarns=true
 *   - crawl/proxy/display/storage · 用户仅贡献设备资源 · 收益归平台+渠道商
 *     UI 不显示金额 · 改为"贡献价值描述 + 贡献次数"
 *
 * 文案策略: 强调"贡献"+"参与"+"助力" · 弱化"赚钱" · 让用户清楚知道
 *           自己贡献了什么 + 没贡献什么 · 但不强调收益对比
 */
export const CAPABILITIES: ReadonlyArray<Capability> = Object.freeze([
  {
    id: "compute",
    name: "算力贡献",
    subtitle: "CPU/GPU 跑 AI/图像/视频任务",
    description:
      "把电脑闲置时段的算力贡献给 B 端 AI 任务、批量图像视频处理、文档 OCR 等。已支持 30+ 任务类型 · 7×24 自动派单 · 按任务即时结算。",
    icon: "task-compute",
    color: "#7c3aed",
    status: "live",
    statusLabel: "已上线",
    resources: ["CPU", "GPU", "内存"],
    anchor: "compute",
    userEarns: true,
    valueDesc: "贡献闲置算力 · 按任务即时结算到你的账户",
    contributionUnit: "任务",
  },
  {
    id: "crawl",
    name: "数据采集",
    subtitle: "助力公开数据收集 · 服务科研与商业研究",
    description:
      "节点作为分布式采集网络的一员 · 协助完成公开数据收集任务 (商品价格指数 / 行业公开资讯 / 学术论文索引)。所有任务经平台合规审核 · 严禁采集个人隐私 / 版权内容。",
    icon: "task-text",
    color: "#0891b2",
    status: "beta",
    statusLabel: "内测中",
    resources: ["网络请求", "解析"],
    expectedLaunch: "2026 Q3",
    anchor: "crawl",
    userEarns: false,
    valueDesc: "助力科研机构和市场分析师获取公开数据 · 支撑研究决策",
    contributionUnit: "次采集",
  },
  {
    id: "proxy",
    name: "IP 池",
    subtitle: "贡献出口 IP 加入平台公共 IP 池",
    description:
      "节点出口 IP 加入平台 IP 池 · 用于公开数据采集与合规 API 调用 · 不做代理转发 · 不承载第三方流量 · 用户随时可关。",
    icon: "nav-throttle",
    color: "#16a34a",
    status: "designing",
    statusLabel: "设计中",
    resources: ["出口 IP"],
    expectedLaunch: "2026 Q4",
    anchor: "proxy",
    userEarns: false,
    valueDesc: "为合规数据采集提供 IP 池 · 不做代理转发",
    contributionUnit: "次复用",
  },
  {
    id: "script",
    name: "通用脚本",
    subtitle: "接收平台分发的签名脚本任务",
    description:
      "节点接收平台签名的任意通用脚本 (Python/Shell) · 在沙盒环境运行 · 适合 B 端自定义批量处理。所有脚本须经平台审核 + 签名 · 用户随时可关。",
    icon: "task-script",
    color: "#0ea5e9",
    status: "beta",
    statusLabel: "内测中",
    resources: ["CPU", "内存", "沙盒"],
    expectedLaunch: "2026 Q3",
    anchor: "script",
    userEarns: true,
    valueDesc: "按脚本运行时长 + 复杂度结算到您账户",
    contributionUnit: "次执行",
  },
])

/** 同意状态 localStorage key
 * v2: 2026-05-24 · 4 能力重构 · 老 v1 cache 包含 display/storage · 必须升 key 让旧数据失效
 */
const CONSENT_KEY = "qs.capability_consent.v2"
const LEGACY_CONSENT_KEYS = ["qs.capability_consent.v1"]

interface ConsentState {
  /** 每个能力的同意状态 · key=CapabilityId */
  consents: Record<CapabilityId, boolean>
  /** 是否同意了总协议 + 隐私 */
  agreedToS: boolean
  agreedPrivacy: boolean
  /** 用户最后一次确认时间 (ms) · 用于后续 ToS 变更弹再次同意 */
  confirmedAtMs: number
}

const DEFAULT_CONSENT: ConsentState = {
  consents: {
    compute: false,
    crawl: false,
    proxy: false,
    script: false,
  },
  agreedToS: false,
  agreedPrivacy: false,
  confirmedAtMs: 0,
}

function loadConsent(): ConsentState {
  // 顺手清掉老 v1 cache (含 display/storage · 已废弃)
  try {
    for (const k of LEGACY_CONSENT_KEYS) {
      localStorage.removeItem(k)
    }
  } catch {
    // ignore
  }
  try {
    const raw = localStorage.getItem(CONSENT_KEY)
    if (!raw) return { ...DEFAULT_CONSENT, consents: { ...DEFAULT_CONSENT.consents } }
    const parsed = JSON.parse(raw) as Partial<ConsentState>
    // 只保留当前 4 能力的 key · 其他 (display/storage) 自动剥离
    const validConsents: Record<CapabilityId, boolean> = { ...DEFAULT_CONSENT.consents }
    const rawConsents = (parsed.consents || {}) as Record<string, boolean>
    for (const key of Object.keys(validConsents) as CapabilityId[]) {
      if (typeof rawConsents[key] === "boolean") {
        validConsents[key] = rawConsents[key]
      }
    }
    return {
      consents: validConsents,
      agreedToS: !!parsed.agreedToS,
      agreedPrivacy: !!parsed.agreedPrivacy,
      confirmedAtMs: parsed.confirmedAtMs ?? 0,
    }
  } catch {
    return { ...DEFAULT_CONSENT, consents: { ...DEFAULT_CONSENT.consents } }
  }
}

function saveConsent(state: ConsentState) {
  try {
    localStorage.setItem(CONSENT_KEY, JSON.stringify(state))
  } catch {
    // localStorage 满或被禁 · 静默
  }
}

/**
 * Best-effort 把本地同意状态写到 Rust 持久化目录 (供 v8_ws hello 上报)
 * Rust 命令: save_capability_consent (commands.rs)
 */
async function persistConsentToRust(state: ConsentState) {
  try {
    await invoke<void>("save_capability_consent", { data: state })
  } catch (e) {
    // dev preview 在浏览器中无 Tauri runtime · 静默
    console.debug("[consent] persist to rust failed:", e)
  }
}

/**
 * Best-effort 把本地同意状态 POST 到后端 · 失败仅日志 · 不阻塞 UI
 * 服务端 endpoint: POST /api/v8/my/consent (user_consent.py)
 */
async function syncConsentToServer(state: ConsentState) {
  try {
    // 2026-05-25 · apiUrl 已自带 /api/v8 · path 去掉重复前缀
    const url = apiUrl("/my/consent")
    const body = {
      consents: state.consents,
      agreed_tos: state.agreedToS,
      agreed_privacy: state.agreedPrivacy,
      tos_version: "v1.0",
      privacy_version: "v1.0",
    }
    await invoke<string>("api_post", { url, body })
  } catch (e) {
    // 用户未登录 / 网络断 · 静默 · 下次启动 hello 还会带
    console.debug("[consent] sync to server failed:", e)
  }
}

/** 同时把 consent 写到 Rust 磁盘 + POST 到后端 · 两路并行不阻塞 */
function dispatchConsentSync(state: ConsentState) {
  void persistConsentToRust(state)
  void syncConsentToServer(state)
}

const consentState = ref<ConsentState>(loadConsent())

export function useCapabilities() {
  const { series } = useEarnings(7)
  const { account } = useAccount()

  /** 算力今日真实收益 (取 useEarnings 最新一天) */
  const computeToday = computed(() => {
    if (series.value.length === 0) return 0
    return Number(series.value[series.value.length - 1].earnings || 0)
  })

  /** 算力累计真实收益 (取 useAccount) */
  const computeTotal = computed(() => account.value?.total_earnings ?? 0)

  /**
   * 增强版能力数据 (元数据 + 同意状态 + 实时数据)
   *
   * 数据展示策略:
   *   - userEarns=true (compute): 展示 todayEarnings / totalEarnings (¥)
   *   - userEarns=false (其他 4): 展示 contributionCount (贡献次数 · 中性数字)
   *     不展示收益金额 · 收益归平台和渠道商
   *
   * contributionCount 当前为 mock 0 · 后期接入 metrics 后端真实数据
   * (TODO: 后端 we_contribution_stats 表 · 按 worker_id + capability_id 聚合)
   */
  const enriched = computed(() =>
    CAPABILITIES.map((c) => ({
      ...c,
      consent: consentState.value.consents[c.id],
      // 仅 compute 有真实收益数字
      todayEarnings: c.userEarns ? computeToday.value : null,
      totalEarnings: c.userEarns ? computeTotal.value : null,
      hasRealEarnings: c.userEarns,
      // 4 个非 compute 用贡献次数代替收益数字 (mock · 待后端接入)
      contributionCount: c.userEarns ? null : 0,
    })),
  )

  /** 已授权数量 */
  const consentedCount = computed(
    () => Object.values(consentState.value.consents).filter(Boolean).length,
  )

  /** 已上线 + 已授权数量 (实际生效的能力数) */
  const activeCount = computed(
    () =>
      enriched.value.filter((c) => c.status === "live" && c.consent).length,
  )

  /** 是否完成首次授权流程 (ToS + Privacy + 至少 1 个能力) */
  const hasCompletedOnboarding = computed(
    () =>
      consentState.value.agreedToS &&
      consentState.value.agreedPrivacy &&
      consentedCount.value > 0,
  )

  /** 切换某能力的同意 */
  function toggleConsent(id: CapabilityId, value?: boolean) {
    const next = value ?? !consentState.value.consents[id]
    consentState.value.consents[id] = next
    saveConsent(consentState.value)
    dispatchConsentSync(consentState.value)
  }

  /** 批量设置 (Modal 用) */
  function setConsents(payload: {
    consents?: Partial<Record<CapabilityId, boolean>>
    agreedToS?: boolean
    agreedPrivacy?: boolean
  }) {
    if (payload.consents) {
      consentState.value.consents = {
        ...consentState.value.consents,
        ...payload.consents,
      }
    }
    if (payload.agreedToS !== undefined) consentState.value.agreedToS = payload.agreedToS
    if (payload.agreedPrivacy !== undefined)
      consentState.value.agreedPrivacy = payload.agreedPrivacy
    consentState.value.confirmedAtMs = Date.now()
    saveConsent(consentState.value)
    dispatchConsentSync(consentState.value)
  }

  /** 重置 (debug 用) */
  function resetConsent() {
    consentState.value = { ...DEFAULT_CONSENT, consents: { ...DEFAULT_CONSENT.consents } }
    saveConsent(consentState.value)
  }

  return {
    capabilities: enriched,
    consentState: computed(() => consentState.value),
    consentedCount,
    activeCount,
    hasCompletedOnboarding,
    toggleConsent,
    setConsents,
    resetConsent,
  }
}
