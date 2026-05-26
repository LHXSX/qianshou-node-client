<script setup lang="ts">
import { computed, ref } from "vue"
import { useConnection } from "../composables/useConnection"
import { useAccount } from "../composables/useAccount"
import { useDevice } from "../composables/useDevice"
import { useTheme } from "../composables/useTheme"
import { useNceProfile, tierEmoji } from "../composables/useNceProfile"

const { snap, authLogout } = useConnection()
const { account, refresh: refreshAccount, loading: accountLoading } = useAccount()
const { device } = useDevice()
const { theme, toggle: toggleTheme } = useTheme()
const { hwTier, repMain, tier, income } = useNceProfile()

const dotColor = computed(() => {
  switch (snap.value.connection_state) {
    case "registered":
      return "var(--c-ok)"
    case "connecting":
    case "authenticating":
    case "reconnecting":
      return "var(--c-warn)"
    default:
      return "var(--c-mute)"
  }
})

const modeBadge = computed(() => {
  const m = snap.value.mode
  const pct = snap.value.throttle_pct
  if (m === "paused") return { label: "已暂停", cls: "warn" }
  if (m === "throttled") return { label: `${pct}%`, cls: "warn" }
  return { label: "全速", cls: "ok" }
})

const latencyText = computed(() => {
  const l = snap.value.latency_ms
  if (l == null) return "—"
  return `${l}ms`
})
const latencyCls = computed(() => {
  const l = snap.value.latency_ms
  if (l == null) return "muted"
  if (l < 200) return "ok"
  if (l < 600) return "warn"
  return "err"
})

const loggingOut = ref(false)
async function onLogout() {
  if (loggingOut.value) return
  loggingOut.value = true
  try {
    await authLogout()
  } catch (e) {
    console.error("auth_logout failed:", e)
  } finally {
    loggingOut.value = false
  }
}

const emit = defineEmits<{ (e: "open-settings"): void }>()
</script>

<template>
  <header class="topbar">
    <!-- Line 1: 品牌 + 用户 -->
    <div class="row-brand">
      <div class="brand">
        <span class="logo">◆</span>
        <span class="name">千手</span>
        <span class="ver">v{{ snap.client_version }}</span>
      </div>
      <div class="right">
        <span class="conn-pill" :class="snap.connection_state">
          <span class="dot" :style="{ background: dotColor }" />
          {{ snap.state_label }}
          <span v-if="snap.node_id" class="node">· {{ snap.node_id.slice(0, 14) }}</span>
        </span>
        <button
          class="icon-btn"
          :title="theme === 'light' ? '切换到深色模式' : '切换到浅色模式'"
          @click="toggleTheme"
        >
          {{ theme === "light" ? "🌙" : "☀️" }}
        </button>
        <button class="icon-btn" title="设置" @click="emit('open-settings')">⚙</button>
        <div class="user" v-if="snap.user">
          <div class="avatar">{{ snap.user.username.slice(0, 1).toUpperCase() }}</div>
          <div class="meta">
            <div class="u-name">{{ snap.user.username }}</div>
            <div class="u-mail">{{ snap.user.email || `#${snap.owner_id}` }}</div>
          </div>
        </div>
        <button class="link-btn" @click="onLogout">退出</button>
      </div>
    </div>

    <!-- Line 2: 5 KPI (2026-05-26 加 NCE 能力档) -->
    <div class="row-kpis">
      <div class="kpi kpi-balance" title="账户全局余额 · 跨本账户所有节点合计 · 来自服务器 /api/v8/users/me">
        <div class="kpi-label">
          <span>账户余额</span>
          <button class="kpi-refresh" :class="{ spinning: accountLoading }"
                  title="点击刷新最新余额" @click="refreshAccount">⟳</button>
        </div>
        <div class="kpi-hero">
          <span class="hero-num">{{ account?.balance?.toFixed(2) ?? "—" }}</span>
          <span class="hero-unit">EDG</span>
        </div>
        <div class="kpi-sub">
          <span title="该账户所有节点累计收到的 REWARD 之和">累计收益</span>
          <span class="accent">+{{ account?.total_earnings?.toFixed(2) ?? "0.00" }}</span>
          <span v-if="account?.username" class="kpi-owner" :title="`登录用户: ${account.username}`">
            · {{ account.username }}
          </span>
        </div>
      </div>

      <div class="kpi kpi-perf">
        <div class="kpi-label">贡献模式</div>
        <div class="kpi-hero">
          <span class="hero-num">{{ snap.throttle_pct }}</span>
          <span class="hero-unit">%</span>
          <span class="badge" :class="modeBadge.cls">{{ modeBadge.label }}</span>
        </div>
        <div class="kpi-sub">{{ snap.mode === "paused" ? "已暂停 · 不接收任务" : "正在贡献算力" }}</div>
      </div>

      <div class="kpi kpi-device">
        <div class="kpi-label">设备</div>
        <div class="kpi-hero">
          <span class="hero-num small">{{ device?.device_name ?? "本机" }}</span>
        </div>
        <div class="kpi-sub" v-if="device">
          {{ device.system.cpu_brand || device.system.arch }} · {{ Math.round(device.system.total_memory_mb / 1024) }} GB
        </div>
        <div class="kpi-sub muted" v-else>检测中…</div>
      </div>

      <div class="kpi kpi-tasks">
        <div class="kpi-label">已完成</div>
        <div class="kpi-hero">
          <span class="hero-num">{{ account?.completed_tasks ?? 0 }}</span>
          <span class="hero-unit">个任务</span>
        </div>
        <div class="kpi-sub">
          延迟 <span :class="latencyCls">{{ latencyText }}</span>
        </div>
      </div>

      <!-- NCE 档位 + 预估月收入 -->
      <div class="kpi kpi-nce" title="节点能力评估 (NCE) · 硬件档 + 信誉档">
        <div class="kpi-label">NCE 能力</div>
        <div class="kpi-hero">
          <span class="hero-num small">{{ tierEmoji(hwTier) }} {{ hwTier }} · {{ tier.emoji }}{{ tier.name }}</span>
        </div>
        <div class="kpi-sub">
          <span v-if="income > 0" class="accent">¥{{ income.toLocaleString() }}/月 预估</span>
          <span v-else class="muted">评分中 · rep={{ repMain }}</span>
        </div>
      </div>
    </div>
  </header>
</template>

<style scoped>
.topbar {
  display: flex;
  flex-direction: column;
  background: var(--c-bg-card);
  border-bottom: 1px solid var(--c-border);
}
.row-brand {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 20px;
  border-bottom: 1px solid var(--c-border);
  height: 44px;
  box-sizing: border-box;
}
.brand {
  display: flex;
  align-items: center;
  gap: 8px;
}
.logo {
  font-size: 14px;
  color: var(--c-accent);
}
.name {
  font-size: 15px;
  font-weight: 600;
  letter-spacing: -0.01em;
  background: linear-gradient(135deg, var(--c-accent), var(--c-accent-2));
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
}
.ver {
  font-size: 14.5px;
  color: var(--c-mute);
  font-family: ui-monospace, SFMono-Regular, monospace;
  padding: 1px 5px;
  border: 1px solid #222;
  border-radius: 4px;
}
.right {
  display: flex;
  align-items: center;
  gap: 10px;
}
.conn-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 11px;
  background: rgba(48, 209, 88, 0.08);
  border: 1px solid rgba(48, 209, 88, 0.3);
  border-radius: 999px;
  font-size: 13.5px;
  color: var(--c-ok);
}
.conn-pill.reconnecting,
.conn-pill.connecting,
.conn-pill.authenticating {
  background: rgba(255, 214, 10, 0.08);
  border-color: rgba(255, 214, 10, 0.3);
  color: var(--c-warn);
}
.conn-pill.disconnected {
  background: rgba(255, 69, 58, 0.08);
  border-color: rgba(255, 69, 58, 0.3);
  color: var(--c-err);
}
.dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
}
.node {
  color: var(--c-mute);
  font-family: ui-monospace, SFMono-Regular, monospace;
}
.icon-btn {
  background: transparent;
  border: 1px solid var(--c-border-strong);
  color: var(--c-mute);
  width: 26px;
  height: 26px;
  padding: 0;
  font-size: 14.5px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.12s;
}
.icon-btn:hover {
  color: var(--c-fg);
  border-color: var(--c-mute);
}
.user {
  display: flex;
  align-items: center;
  gap: 7px;
}
.avatar {
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: linear-gradient(135deg, var(--c-accent), var(--c-accent-2));
  color: #fff;
  font-size: 14.5px;
  font-weight: 600;
  display: flex;
  align-items: center;
  justify-content: center;
}
.meta {
  line-height: 1.15;
}
.u-name {
  font-size: 14.5px;
  font-weight: 500;
}
.u-mail {
  font-size: 14.5px;
  color: var(--c-mute);
}
.link-btn {
  background: transparent;
  border: none;
  color: var(--c-mute);
  font-size: 13.5px;
  cursor: pointer;
  padding: 4px 6px;
}
.link-btn:hover {
  color: var(--c-fg);
}

/* KPI 行 (2026-05-26 · 5 列 · 加 NCE) */
.row-kpis {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: 1px;
  background: var(--c-border);
}
.kpi {
  display: flex;
  flex-direction: column;
  justify-content: center;
  padding: 14px 20px;
  min-height: 80px;
  box-sizing: border-box;
  position: relative;
  overflow: hidden;
}
.kpi::before {
  content: "";
  position: absolute; inset: 0;
  opacity: 0.06;
  z-index: 0;
}
.kpi > * { position: relative; z-index: 1; }

.kpi-balance { background: linear-gradient(135deg, rgba(10,132,255,0.06), transparent); }
.kpi-balance::before { background: linear-gradient(135deg, #0a84ff, #5ac8fa); }
.kpi-perf { background: linear-gradient(135deg, rgba(48,209,88,0.06), transparent); }
.kpi-perf::before { background: linear-gradient(135deg, #30d158, #34c759); }
.kpi-device { background: linear-gradient(135deg, rgba(255,159,10,0.06), transparent); }
.kpi-device::before { background: linear-gradient(135deg, #ff9f0a, #ffcc00); }
.kpi-tasks { background: linear-gradient(135deg, rgba(94,92,230,0.06), transparent); }
.kpi-tasks::before { background: linear-gradient(135deg, #5e5ce6, #bf5af2); }

.kpi-label {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--c-mute);
  margin-bottom: 6px;
}
.kpi-refresh {
  border: none;
  background: transparent;
  color: var(--c-mute);
  cursor: pointer;
  font-size: 14px;
  line-height: 1;
  padding: 2px 4px;
  border-radius: 4px;
  opacity: 0.6;
  transition: all 0.15s;
}
.kpi-refresh:hover { opacity: 1; color: var(--c-fg); background: rgba(127, 127, 127, 0.1); }
.kpi-refresh.spinning { animation: kpiSpin 0.8s linear infinite; pointer-events: none; }
@keyframes kpiSpin { to { transform: rotate(360deg); } }
.kpi-owner {
  color: var(--c-mute);
  font-size: 11.5px;
  margin-left: 4px;
  opacity: 0.7;
}
.kpi-hero {
  display: flex;
  align-items: baseline;
  gap: 6px;
  font-variant-numeric: tabular-nums;
}
.hero-num {
  font-size: 26px;
  font-weight: 700;
  color: var(--c-fg);
  letter-spacing: -0.02em;
  line-height: 1;
}
.hero-num.small {
  font-size: 16px;
  font-weight: 600;
}
.hero-unit {
  font-size: 13px;
  color: var(--c-mute);
  font-weight: 500;
}
.badge {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 4px;
  font-weight: 600;
  margin-left: 4px;
}
.badge.ok {
  background: rgba(48, 209, 88, 0.15);
  color: var(--c-ok);
}
.badge.warn {
  background: rgba(255, 214, 10, 0.15);
  color: var(--c-warn);
}
.kpi-sub {
  font-size: 12px;
  color: var(--c-mute);
  margin-top: 6px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.kpi-sub .accent {
  color: var(--c-ok);
  font-weight: 600;
}
.kpi-sub .ok { color: var(--c-ok); }
.kpi-sub .warn { color: var(--c-warn); }
.kpi-sub .err { color: var(--c-err); }
.kpi-sub .muted { color: var(--c-mute); opacity: 0.6; }
</style>
