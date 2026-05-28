<script setup lang="ts">
import { ref, onMounted, computed, watch } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import Welcome from "./views/Welcome.vue"
import StatusRail from "./components/shell/StatusRail.vue"
import NavRail from "./components/shell/NavRail.vue"
import DashboardHome from "./pages/DashboardHome.vue"
import EarningsPage from "./pages/EarningsPage.vue"
import ThrottlePage from "./pages/ThrottlePage.vue"
import DevicePage from "./pages/DevicePage.vue"
import MarketPage from "./pages/MarketPage.vue"
import HistoryPage from "./pages/HistoryPage.vue"
import ToolboxPage from "./pages/ToolboxPage.vue"
import AICapabilityPage from "./pages/AICapabilityPage.vue"
import CapabilitiesPage from "./pages/CapabilitiesPage.vue"
// 2026-05-28 v8.1.1 · 节点信誉自查面板
import MyNCEPage from "./pages/MyNCEPage.vue"
import SettingsPage from "./pages/SettingsPage.vue"
import HelpPage from "./pages/HelpPage.vue"
import BundleStatusBar from "./components/BundleStatusBar.vue"
import ConsentMatrixModal from "./components/ConsentMatrixModal.vue"
import SplashAdModal from "./components/ads/SplashAdModal.vue"
// 2026-05-25 8.0.9 · 广告/通知点击统一弹窗 (modal/embed_url/qr) · 全局单例
import AdActionModal from "./components/ads/AdActionModal.vue"
import NoticeMarquee from "./components/shell/NoticeMarquee.vue"
import { useConnection } from "./composables/useConnection"
import { useNav } from "./composables/useNav"
import { useTasks } from "./composables/useTasks"
import { useUpdater } from "./composables/useUpdater"
import { useCapabilities } from "./composables/useCapabilities"

const { snap, authRestore } = useConnection()
const { page, goto } = useNav()
useTasks()
const { updateInfo, installing, dismissed, install: installUpdate, dismiss: dismissUpdate } = useUpdater()
const { hasCompletedOnboarding } = useCapabilities()

const restoring = ref(true)

// ── 首启同意矩阵 (用户登录后 · 未完成 onboarding · 自动弹) ──
const showConsent = ref(false)
function closeConsent() { showConsent.value = false }

// ── 老版本检测 ──
interface OldVersionInfo {
  found: boolean
  old_processes: string[]
  old_data_dirs: string[]
  current_version: string
}
const oldVersion = ref<OldVersionInfo | null>(null)
const cleaningOld = ref(false)
const cleanResult = ref<string[]>([])

async function checkOldVersions() {
  try {
    const info = await invoke<OldVersionInfo>("check_old_versions")
    if (info.found) {
      oldVersion.value = info
    }
  } catch (e) {
    console.warn("老版本检测失败:", e)
  }
}

async function killAndClean() {
  cleaningOld.value = true
  try {
    const killed = await invoke<string[]>("kill_old_processes")
    const cleaned = await invoke<string[]>("clean_old_data_dirs")
    cleanResult.value = [...killed, ...cleaned]
    setTimeout(() => {
      oldVersion.value = null
      cleanResult.value = []
    }, 3000)
  } catch (e) {
    console.error("清理失败:", e)
  } finally {
    cleaningOld.value = false
  }
}

function dismissOldVersion() {
  oldVersion.value = null
}

const view = computed(() => {
  if (restoring.value) return "loading"
  return snap.value.is_authenticated ? "dashboard" : "welcome"
})

const currentPageComp = computed(() => {
  switch (page.value) {
    case "dashboard": return DashboardHome
    case "market": return MarketPage
    case "history": return HistoryPage
    case "earnings": return EarningsPage
    case "throttle": return ThrottlePage
    case "device": return DevicePage
    case "toolbox": return ToolboxPage
    case "ai-capability": return AICapabilityPage
    case "capabilities": return CapabilitiesPage
    case "nce": return MyNCEPage
    case "settings": return SettingsPage
    case "help": return HelpPage
  }
  return DashboardHome
})

function openSettings() {
  goto("settings")
}

// 2026-05-25 8.0.9 · 监听 hashchange + qs:navigate · 让广告/通知的 internal 跳转生效
// hash 形如 "#market" / "#earnings" → 调 goto("market") 切页
const PAGE_HASHES: Record<string, Parameters<typeof goto>[0]> = {
  "#dashboard": "dashboard",
  "#market": "market",
  "#history": "history",
  "#earnings": "earnings",
  "#throttle": "throttle",
  "#device": "device",
  "#toolbox": "toolbox",
  "#ai-capability": "ai-capability",
  "#capabilities": "capabilities",
  "#settings": "settings",
  "#help": "help",
}
function handleHashNav() {
  const h = window.location.hash.toLowerCase()
  if (!h) return
  const dest = PAGE_HASHES[h]
  if (dest && dest !== page.value) {
    goto(dest)
  }
}
function handleQsNavigate(e: Event) {
  const ce = e as CustomEvent<{ path?: string }>
  const p = ce.detail?.path?.toLowerCase()
  if (!p) return
  // 支持 "market" 或 "#market" 都走 goto
  const key = p.startsWith("#") ? p : `#${p}`
  const dest = PAGE_HASHES[key]
  if (dest && dest !== page.value) {
    goto(dest)
  }
}

onMounted(async () => {
  try {
    await authRestore()
  } catch (e) {
    console.warn("auth_restore 失败:", e)
  } finally {
    restoring.value = false
  }
  checkOldVersions()

  // 2026-05-25 8.0.9 · 注册广告/通知 internal 跳转 listener
  window.addEventListener("hashchange", handleHashNav)
  window.addEventListener("qs:navigate", handleQsNavigate)
  // 启动时若 URL 自带 hash · 立即跳一次 (深链场景)
  handleHashNav()

  // 2026-05-26 8.0.17 fix · listen auto_updater 的 update_available 事件
  // 之前后端 emit 了但前端无人 listen → 老版用户永远看不到更新提示
  try {
    await listen<{ version?: string; notes?: string; download_url?: string }>(
      "update_available",
      (e) => {
        const v = e.payload?.version || "新版本"
        const notes = e.payload?.notes || ""
        const ok = window.confirm(
          `🎉 千手节点 ${v} 已发布!\n\n${notes}\n\n现在立即升级? (会下载新版并打开安装包)`,
        )
        if (ok) {
          invoke("install_update").catch((err) => {
            console.error("[updater] install_update 失败:", err)
            window.alert(`更新失败: ${err}\n\n您可以手动到 wujisuanli.com/#/downloads-center 下载`)
          })
        }
      },
    )
    console.log("[updater] update_available listener 已注册")
  } catch (e) {
    console.warn("[updater] listen 失败:", e)
  }
})

// 视图进入 dashboard 后 · 若用户未完成首次同意流程 · 自动弹 Modal
watch(view, (v) => {
  if (v === "dashboard" && !hasCompletedOnboarding.value) {
    // 略微延迟 · 避免与老版本检测弹窗同屏冲突
    setTimeout(() => {
      if (!hasCompletedOnboarding.value && !oldVersion.value?.found) {
        showConsent.value = true
      }
    }, 600)
  }
}, { immediate: true })
</script>

<template>
  <!-- 老版本检测弹窗 · 2026-05-25 重构为 3 段式 (header 固定 / body 滚动 / footer 固定) -->
  <div v-if="oldVersion?.found" class="old-version-overlay">
    <div class="old-version-dialog">
      <!-- HEADER · 固定 · 不滚 -->
      <div class="ovd-header">
        <div class="ovd-icon">⚠</div>
        <h3>检测到旧版本残留</h3>
        <p class="ovd-desc">
          发现旧版本千手/EdgeCompute 的进程或数据目录，可能导致冲突或数据混乱。<br>
          建议清理后再使用当前版本 v{{ oldVersion.current_version }}。
        </p>
      </div>
      <!-- BODY · 唯一滚动区 -->
      <div class="ovd-body">
        <div v-if="oldVersion.old_processes.length" class="ovd-section">
          <div class="ovd-label">旧进程 ({{ oldVersion.old_processes.length }})</div>
          <div v-for="p in oldVersion.old_processes" :key="p" class="ovd-item" :title="p">{{ p }}</div>
        </div>
        <div v-if="oldVersion.old_data_dirs.length" class="ovd-section">
          <div class="ovd-label">旧数据目录 ({{ oldVersion.old_data_dirs.length }})</div>
          <div v-for="d in oldVersion.old_data_dirs" :key="d" class="ovd-item" :title="d">{{ d }}</div>
        </div>
        <div v-if="cleanResult.length" class="ovd-result">
          <div v-for="r in cleanResult" :key="r" class="ovd-result-item">✓ {{ r }}</div>
        </div>
      </div>
      <!-- FOOTER · 固定 · 不滚 · 100% 保证按钮可见 -->
      <div class="ovd-actions">
        <button class="btn ghost" @click="dismissOldVersion">跳过</button>
        <button class="btn danger" :disabled="cleaningOld" @click="killAndClean">
          {{ cleaningOld ? "清理中..." : "一键清理" }}
        </button>
      </div>
    </div>
  </div>

  <Welcome v-if="view === 'welcome'" />
  <div v-else-if="view === 'dashboard'" class="app-shell">
    <StatusRail @open-settings="openSettings" />
    <NoticeMarquee />
    <transition name="banner">
      <section v-if="updateInfo?.available && !dismissed" class="update-banner">
        <span class="ub-tag">更新</span>
        <span class="ub-title">v{{ updateInfo.version }}</span>
        <span class="ub-notes" v-if="updateInfo.notes">{{ updateInfo.notes }}</span>
        <span class="ub-spacer" />
        <button class="ub-link" @click="dismissUpdate">稍后</button>
        <button class="ub-btn" :disabled="installing" @click="installUpdate">
          {{ installing ? "安装中…" : "立即更新" }}
        </button>
      </section>
    </transition>
    <div class="body">
      <NavRail />
      <main class="outlet">
        <component :is="currentPageComp" />
      </main>
    </div>
  </div>
  <div v-else class="loading">
    <div class="spinner" />
    <div>初始化中...</div>
  </div>

  <!-- 首启同意矩阵 (5 能力授权) -->

  <!-- 2026-05-23 · 开屏广告 · 延迟 1500ms 避开 ConsentMatrixModal 同时弹 -->
  <SplashAdModal :delay-ms="1500" />
  <!-- 2026-05-25 8.0.9 · 广告/通知点击统一弹窗 · 全局单例 (Teleport to body 内部) -->
  <AdActionModal />
  <ConsentMatrixModal :open="showConsent" mode="onboarding" @close="closeConsent" />
</template>

<style scoped>
.app-shell {
  display: flex;
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
}
.body {
  display: flex;
  flex: 1;
  min-height: 0;
  background: var(--c-bg);
  position: relative;
}
.outlet {
  flex: 1;
  padding: var(--sp-6) var(--sp-7) var(--sp-8);
  overflow-y: auto;
  min-width: 0;
  animation: fade-in var(--dur-slow) var(--ease-out);
}

/* 更新 banner · 紧凑薄条 */
.update-banner {
  display: flex;
  align-items: center;
  gap: var(--sp-4);
  padding: 6px var(--sp-6);
  background: var(--c-brand-soft);
  border-bottom: 1px solid var(--c-brand);
  font-size: var(--fs-sm);
  color: var(--c-fg-soft);
}
.ub-tag {
  font-size: var(--fs-2xs);
  font-weight: var(--fw-bold);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  background: var(--c-brand);
  color: #fff;
  padding: 1px 6px;
  border-radius: var(--r-xs);
}
.ub-title { font-weight: var(--fw-semibold); color: var(--c-fg); font-family: ui-monospace, monospace; }
.ub-notes { color: var(--c-mute); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 480px; }
.ub-spacer { flex: 1; }
.ub-link {
  color: var(--c-mute); font-size: var(--fs-sm); padding: 0 var(--sp-3);
}
.ub-link:hover { color: var(--c-fg); }
.ub-btn {
  background: var(--c-brand); color: #fff;
  padding: 4px 12px;
  border-radius: var(--r-sm);
  font-size: var(--fs-sm);
  font-weight: var(--fw-medium);
  transition: background var(--dur-fast);
}
.ub-btn:hover { background: var(--c-brand-2); }
.ub-btn:disabled { opacity: 0.6; cursor: not-allowed; }

.banner-enter-active, .banner-leave-active { transition: all var(--dur-base) var(--ease-std); }
.banner-enter-from, .banner-leave-to { opacity: 0; transform: translateY(-4px); }

/* loading */
.loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-height: 100vh;
  gap: 16px;
  color: var(--c-mute);
  font-size: 14.5px;
}
.spinner {
  width: 28px;
  height: 28px;
  border: 2px solid #222;
  border-top-color: var(--c-accent);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}
@keyframes spin {
  to { transform: rotate(360deg); }
}

/* 老版本检测弹窗 */
.old-version-overlay {
  position: fixed;
  inset: 0;
  z-index: 9999;
  background: rgba(0, 0, 0, 0.7);
  display: flex;
  align-items: center;
  justify-content: center;
  backdrop-filter: blur(4px);
}
/* ════════════════════════════════════════════════════════════
   老版本弹窗 · 3 段式布局
   dialog = flex column · max-height 限制
   ├ header (flex: 0 0 auto · 不滚)
   ├ body   (flex: 1 1 auto · overflow-y: auto · 唯一滚动)
   └ footer (flex: 0 0 auto · 不滚 · 按钮永远可见)
   ════════════════════════════════════════════════════════════ */
.old-version-dialog {
  background: var(--c-bg-card);
  border: 1px solid var(--c-border-strong);
  border-radius: 16px;
  max-width: 580px;
  width: 90%;
  max-height: 80vh;
  display: flex;
  flex-direction: column;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
  /* dialog 自身不滚 · 让 body 滚 */
  overflow: hidden;
}
.ovd-header {
  flex: 0 0 auto;
  padding: 24px 28px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  border-bottom: 1px solid var(--c-line);
}
.ovd-body {
  flex: 1 1 auto;
  min-height: 0;          /* flex child overflow 兼容 */
  overflow-y: auto;
  padding: 16px 28px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.ovd-body::-webkit-scrollbar { width: 6px; }
.ovd-body::-webkit-scrollbar-thumb {
  background: rgba(255,255,255,0.15);
  border-radius: 3px;
}
.ovd-actions {
  flex: 0 0 auto;
  display: flex;
  gap: 10px;
  justify-content: flex-end;
  padding: 14px 28px;
  border-top: 1px solid var(--c-line);
  background: var(--c-bg-soft);
}

.ovd-icon {
  font-size: 36px;
  text-align: center;
}
.old-version-dialog h3 {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
  text-align: center;
  color: var(--c-warn);
}
.ovd-desc {
  margin: 0;
  font-size: 13.5px;
  color: var(--c-mute);
  text-align: center;
  line-height: 1.6;
}
.ovd-section {
  background: var(--c-bg-soft);
  border-radius: 8px;
  padding: 10px 12px;
}
.ovd-label {
  font-size: 11px;
  font-weight: 600;
  color: var(--c-mute);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  margin-bottom: 8px;
}
.ovd-item {
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 11.5px;
  color: var(--c-fg-soft);
  padding: 4px 0;
  word-break: break-all;
  /* 单条最多 2 行 · 鼠标悬停看 tooltip */
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  line-height: 1.45;
  border-bottom: 1px solid rgba(255,255,255,0.05);
  cursor: help;
}
.ovd-item:last-child { border-bottom: none; }
.ovd-result {
  background: rgba(48, 209, 88, 0.1);
  border-radius: 8px;
  padding: 10px 12px;
}
.ovd-result-item {
  font-size: 13px;
  color: var(--c-ok);
  padding: 3px 0;
}
.btn.danger {
  background: var(--c-err);
  color: #fff;
  border: none;
  padding: 8px 16px;
  border-radius: 8px;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
}
.btn.danger:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.btn.danger:hover:not(:disabled) {
  background: #ff3b30;
}
</style>
