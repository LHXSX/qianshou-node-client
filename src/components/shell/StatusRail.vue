<script setup lang="ts">
/**
 * 顶部状态条 · 40px 高 · 玻璃质感
 *
 * 设计:
 *   - 左: brand 字标 + node id mono
 *   - 中: WS 连接脉冲 (绿=在线 / 黄=连接中 / 红=断)
 *   - 右: 3 个紧凑 KPI (余额 / 任务数 / 延迟) + 用户头像菜单
 *
 * 数据全 reactive · 不要 fake
 */
import { computed, ref, onMounted, onUnmounted } from "vue"
import { useConnection } from "../../composables/useConnection"
import { useAccount } from "../../composables/useAccount"
import { useTheme } from "../../composables/useTheme"
import { useTasks } from "../../composables/useTasks"
import { useCapabilities } from "../../composables/useCapabilities"
import { useNav } from "../../composables/useNav"
import { useNceProfile, tierEmoji } from "../../composables/useNceProfile"
import Icon from "../Icon.vue"

const { snap, authLogout } = useConnection()
const { hwTier, repMain, tier, income } = useNceProfile()
const { account } = useAccount()
const { theme, toggle: toggleTheme } = useTheme()
const { running, verifying, done } = useTasks()
const { capabilities, activeCount } = useCapabilities()
const { goto } = useNav()

const emit = defineEmits<{ (e: "open-settings"): void }>()

function gotoCapabilities() { goto("capabilities") }

// ── 连接状态 ──
const connState = computed(() => {
  const s = snap.value.connection_state
  if (s === "registered") return { kind: "ok", label: "在线" }
  if (s === "connecting" || s === "authenticating" || s === "reconnecting")
    return { kind: "warn", label: snap.value.state_label || "连接中" }
  return { kind: "err", label: "离线" }
})

const latencyText = computed(() => {
  const l = snap.value.latency_ms
  if (l == null) return "—"
  return `${l}ms`
})
const latencyKind = computed(() => {
  const l = snap.value.latency_ms
  if (l == null) return "mute"
  if (l < 200) return "ok"
  if (l < 600) return "warn"
  return "err"
})

const nodeId = computed(() => snap.value.node_id?.slice(0, 12) || "—")
const taskCount = computed(() => running.value.length + verifying.value.length)
const doneCount = computed(() => done.value.filter((t) => t.ok).length)

// 用户菜单
const menuOpen = ref(false)
const menuEl = ref<HTMLElement | null>(null)
function toggleMenu() { menuOpen.value = !menuOpen.value }
function closeMenu() { menuOpen.value = false }

const loggingOut = ref(false)
async function onLogout() {
  if (loggingOut.value) return
  loggingOut.value = true
  try { await authLogout() } finally { loggingOut.value = false; closeMenu() }
}

function handleClickOutside(e: MouseEvent) {
  if (menuEl.value && !menuEl.value.contains(e.target as Node)) closeMenu()
}
onMounted(() => document.addEventListener("click", handleClickOutside))
onUnmounted(() => document.removeEventListener("click", handleClickOutside))

const userInitial = computed(() => (snap.value.user?.username?.[0] || "?").toUpperCase())
</script>

<template>
  <header class="status-rail">
    <!-- 左 brand + node id -->
    <div class="sr-left">
      <span class="brand">
        <span class="b-dot" />
        <span class="b-name">千手</span>
        <span class="b-ver mono">v{{ snap.client_version }}</span>
      </span>
      <span class="sep">|</span>
      <span class="node-pill" :title="snap.node_id || ''">
        <span class="np-dot" :data-state="connState.kind" />
        <span class="np-label">{{ connState.label }}</span>
        <span class="np-id mono">{{ nodeId }}</span>
      </span>

      <!-- 5 能力状态指示器 (点击跳能力中心) -->
      <button
        class="cap-strip"
        :title="`${activeCount}/4 项授权运行中 · 点击查看详情`"
        @click="gotoCapabilities"
      >
        <span
          v-for="c in capabilities"
          :key="c.id"
          :class="['cs-dot', c.consent && c.status === 'live' ? 'is-on' : '', `status-${c.status}`]"
          :style="{ '--cap-color': c.color }"
          :title="`${c.name} · ${c.statusLabel}${c.consent ? ' · 已授权' : ''}`"
        />
        <span class="cs-count mono">{{ activeCount }}/4</span>
      </button>
    </div>

    <!-- 中 KPI -->
    <div class="sr-kpis">
      <div class="kpi" title="余额">
        <span class="k-icon"><Icon name="coin" :size="11" /></span>
        <span class="k-num mono">{{ account?.balance?.toFixed(2) ?? "—" }}</span>
        <span class="k-unit">EDG</span>
      </div>
      <div class="kpi-sep" />
      <div class="kpi" title="进行中 / 已完成">
        <span class="k-icon"><Icon name="status-running" :size="11" /></span>
        <span class="k-num mono">{{ taskCount }}</span>
        <span class="k-slash">/</span>
        <span class="k-num mono small">{{ doneCount }}</span>
      </div>
      <div class="kpi-sep" />
      <div class="kpi" title="延迟">
        <span class="k-icon"><Icon name="clock" :size="11" /></span>
        <span class="k-num mono" :data-kind="latencyKind">{{ latencyText }}</span>
      </div>
      <div class="kpi-sep" />
      <!-- 2026-05-26 · NCE 档位 + 预估月收入 -->
      <div class="kpi nce-kpi"
        :title="`NCE 能力评估 · 硬件档 ${hwTier} · 信誉 ${tier.name} (${repMain}分)` + (income > 0 ? ` · 预估 ¥${income.toLocaleString()}/月` : '')">
        <span class="k-icon">{{ tierEmoji(hwTier) }}</span>
        <span class="k-num mono">{{ hwTier }}</span>
        <span class="k-slash">·</span>
        <span class="k-num mono small">{{ tier.emoji }}{{ tier.name }}</span>
        <span v-if="income > 0" class="k-unit accent mono">¥{{ income.toLocaleString() }}</span>
      </div>
    </div>

    <!-- 右 actions -->
    <div class="sr-right">
      <button class="ic-btn" :title="theme === 'dark' ? '切浅色' : '切深色'" @click="toggleTheme">
        <Icon :name="theme === 'dark' ? 'spark' : 'nav-settings'" :size="14" />
      </button>
      <button class="ic-btn" title="设置" @click="emit('open-settings')">
        <Icon name="nav-settings" :size="14" />
      </button>
      <div class="user-wrap" ref="menuEl">
        <button class="user-btn" @click="toggleMenu" :class="{ on: menuOpen }">
          <span class="ub-avatar">{{ userInitial }}</span>
          <span class="ub-name">{{ snap.user?.username || "未登录" }}</span>
          <Icon name="action-chevron" :size="11" />
        </button>
        <transition name="menu">
          <div v-if="menuOpen" class="user-menu">
            <div class="um-head">
              <span class="ub-avatar lg">{{ userInitial }}</span>
              <div>
                <div class="um-name">{{ snap.user?.username || "—" }}</div>
                <div class="um-mail mono">{{ snap.user?.email || `#${snap.owner_id}` }}</div>
              </div>
            </div>
            <div class="um-divider" />
            <button class="um-item" @click="emit('open-settings'); closeMenu()">
              <Icon name="nav-settings" :size="13" /> 系统设置
            </button>
            <button class="um-item danger" :disabled="loggingOut" @click="onLogout">
              <Icon name="action-external" :size="13" />
              {{ loggingOut ? "退出中…" : "退出登录" }}
            </button>
          </div>
        </transition>
      </div>
    </div>
  </header>
</template>

<style scoped>
.status-rail {
  height: var(--statusbar-h);
  display: flex;
  align-items: center;
  padding: 0 var(--sp-6);
  gap: var(--sp-6);
  background: var(--c-bg-elev-1);
  border-bottom: 1px solid var(--c-line);
  position: relative;
  z-index: 10;
}

/* ── 左 ── */
.sr-left { display: flex; align-items: center; gap: var(--sp-5); min-width: 0; }
.brand {
  display: inline-flex; align-items: center; gap: 6px;
}
.b-dot {
  width: 7px; height: 7px;
  background: var(--c-brand);
  border-radius: 50%;
  box-shadow: 0 0 8px var(--c-brand-glow);
}
.b-name {
  font-size: var(--fs-lg);
  font-weight: var(--fw-semibold);
  letter-spacing: -0.01em;
  color: var(--c-fg);
}
.b-ver {
  font-size: var(--fs-2xs);
  color: var(--c-mute);
  padding: 1px 6px;
  background: var(--c-bg-soft);
  border-radius: var(--r-xs);
}
.sep { color: var(--c-line-strong); }

.node-pill {
  display: inline-flex; align-items: center; gap: 7px;
  padding: 4px 10px;
  background: var(--c-bg-soft);
  border: 1px solid var(--c-line);
  border-radius: var(--r-pill);
  font-size: var(--fs-xs);
}
.np-dot {
  width: 6px; height: 6px;
  border-radius: 50%;
}
.np-dot[data-state="ok"]   { background: var(--c-ok);   animation: brand-pulse 2.2s ease-in-out infinite; box-shadow: 0 0 6px rgba(16,185,129,0.7); }
.np-dot[data-state="warn"] { background: var(--c-warn); animation: brand-pulse 1.2s ease-in-out infinite; }
.np-dot[data-state="err"]  { background: var(--c-err); }
.np-label { color: var(--c-fg-soft); }
.np-id    { color: var(--c-mute); }

/* ── 5 能力指示器 ── */
.cap-strip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 4px 9px;
  background: var(--c-bg-soft);
  border: 1px solid var(--c-line);
  border-radius: var(--r-pill);
  cursor: pointer;
  transition: border-color var(--dur-base), background var(--dur-base);
}
.cap-strip:hover {
  border-color: var(--c-line-strong);
  background: var(--c-bg-card);
}
.cs-dot {
  width: 6px; height: 6px;
  border-radius: 50%;
  background: var(--c-line-strong);
  transition: background var(--dur-base), box-shadow var(--dur-base), transform var(--dur-base);
}
.cs-dot.is-on {
  background: var(--cap-color, var(--c-brand));
  box-shadow: 0 0 6px var(--cap-color, var(--c-brand));
}
.cs-dot.status-beta {
  background: color-mix(in srgb, var(--cap-color) 40%, var(--c-line-strong));
  outline: 1px solid color-mix(in srgb, var(--cap-color) 35%, transparent);
  outline-offset: 1px;
}
.cap-strip:hover .cs-dot {
  transform: scale(1.15);
}
.cs-count {
  font-size: var(--fs-2xs);
  color: var(--c-mute);
  font-weight: var(--fw-medium);
  margin-left: 4px;
}
.cap-strip:hover .cs-count { color: var(--c-fg); }

/* ── 中 ── */
.sr-kpis {
  flex: 1;
  display: flex; align-items: center; justify-content: center;
  gap: var(--sp-6);
}
.kpi {
  display: inline-flex; align-items: center; gap: 6px;
  font-size: var(--fs-xs);
}
.k-icon { color: var(--c-mute); display: inline-flex; align-items: center; }
.k-num { font-size: var(--fs-lg); font-weight: var(--fw-semibold); color: var(--c-fg); letter-spacing: -0.01em; }
.k-num.small { font-size: var(--fs-md); color: var(--c-mute); font-weight: var(--fw-medium); }
.k-num[data-kind="ok"]   { color: var(--c-ok); }
.k-num[data-kind="warn"] { color: var(--c-warn); }
.k-num[data-kind="err"]  { color: var(--c-err); }
.k-num[data-kind="mute"] { color: var(--c-mute); }
.k-unit, .k-slash { color: var(--c-mute); font-size: var(--fs-2xs); font-weight: var(--fw-medium); }
.k-unit.accent { color: var(--c-brand); }

/* NCE KPI · 略高亮 · 能力档位 · 点开可跳收益页 (未加) */
.nce-kpi { gap: 4px; padding: 2px 4px; border-radius: var(--r-xs); }
.nce-kpi:hover { background: var(--c-bg-soft); }

.kpi-sep {
  width: 1px; height: 14px;
  background: var(--c-line);
}

/* ── 右 ── */
.sr-right { display: flex; align-items: center; gap: 4px; }
.ic-btn {
  width: 32px; height: 32px;
  display: flex; align-items: center; justify-content: center;
  color: var(--c-mute);
  border-radius: var(--r-sm);
  transition: color var(--dur-base), background var(--dur-base);
}
.ic-btn:hover { color: var(--c-fg); background: var(--c-bg-soft); }

.user-wrap { position: relative; margin-left: 4px; }
.user-btn {
  display: inline-flex; align-items: center; gap: 8px;
  padding: 3px 10px 3px 3px;
  border-radius: var(--r-pill);
  border: 1px solid var(--c-line);
  color: var(--c-fg-soft);
  transition: all var(--dur-base);
}
.user-btn:hover, .user-btn.on { border-color: var(--c-line-strong); color: var(--c-fg); background: var(--c-bg-soft); }
.ub-avatar {
  width: 26px; height: 26px;
  border-radius: 50%;
  background: linear-gradient(135deg, var(--c-brand), var(--c-brand-2));
  color: #fff;
  display: flex; align-items: center; justify-content: center;
  font-size: var(--fs-xs);
  font-weight: var(--fw-semibold);
}
.ub-avatar.lg { width: 40px; height: 40px; font-size: var(--fs-md); }
.ub-name {
  font-size: var(--fs-sm);
  font-weight: var(--fw-medium);
  max-width: 120px;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}

/* user menu */
.user-menu {
  position: absolute;
  top: calc(100% + 6px);
  right: 0;
  width: 240px;
  background: var(--c-bg-card);
  border: 1px solid var(--c-line-strong);
  border-radius: var(--r-md);
  box-shadow: var(--sh-3);
  padding: var(--sp-3);
  z-index: 20;
}
.um-head {
  display: flex; align-items: center; gap: var(--sp-4);
  padding: var(--sp-4) var(--sp-4) var(--sp-5);
}
.um-name { font-size: var(--fs-md); font-weight: var(--fw-semibold); }
.um-mail { font-size: var(--fs-2xs); color: var(--c-mute); }
.um-divider { height: 1px; background: var(--c-line); margin: 0 var(--sp-2); }
.um-item {
  width: 100%;
  display: flex; align-items: center; gap: var(--sp-4);
  padding: 8px var(--sp-4);
  font-size: var(--fs-sm);
  color: var(--c-fg-soft);
  border-radius: var(--r-sm);
  transition: all var(--dur-fast);
  text-align: left;
}
.um-item:hover { background: var(--c-bg-soft); color: var(--c-fg); }
.um-item.danger { color: var(--c-err); }
.um-item.danger:hover { background: var(--c-err-soft); }
.um-item:disabled { opacity: 0.5; cursor: not-allowed; }

.menu-enter-active, .menu-leave-active { transition: opacity var(--dur-base), transform var(--dur-base); }
.menu-enter-from, .menu-leave-to { opacity: 0; transform: translateY(-4px); }
</style>
