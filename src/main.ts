import { createApp } from "vue"
import { createPinia } from "pinia"
import App from "./App.vue"
import "./styles/global.css"
import "./composables/useTheme" // 启动立即应用 data-theme 属性

/**
 * 前端热更新 bootstrap (2026-05-21)
 *
 * 设计:
 *   - 本地内嵌版本 (Tauri 打包内) 启动时 fetch /api/v8/client/web-manifest
 *   - 远端 entry_url 可达 → location.replace(entry_url) 切到远端最新版
 *   - 远端 fail / 离线 → 用本地内嵌挂载 (兜底)
 *
 *   远端版本加载后, location.protocol === 'https:' → 跳过 bootstrap 直接挂载, 避免无限循环
 *
 * 工程师改前端代码 → ./scripts/deploy-client-web.sh → 用户重启客户端就用上新版
 * 不需要重新打 dmg / 不需要用户重装
 */
const HOT_RELOAD_FLAG_KEY = "qs.web.hotReload"
const HOT_RELOAD_TIMEOUT_MS = 2500

async function bootstrap() {
  const isRemote = location.protocol === "https:" || location.protocol === "http:"
  // 2026-05-21 · 暂禁远端跳转
  //   Tauri 2 严格 ACL 在 https 加载下会拒绝自定义 invoke command,
  //   导致登录/任务全失败. 等加齐 capability 后再用 VITE_ENABLE_REMOTE_HOT_RELOAD=1 开启.
  const remoteEnabled = (import.meta as any).env?.VITE_ENABLE_REMOTE_HOT_RELOAD === "1"
  const skipHotReload =
    isRemote ||
    !remoteEnabled ||
    new URLSearchParams(location.search).get("noHotReload") === "1"

  if (skipHotReload) {
    mountApp(isRemote ? "远端已加载" : "本地模式 (远端热更新待 ACL 修复后启用)")
    return
  }

  // API base · 允许 env 覆盖 (dev 时指本地 127.0.0.1:8000)
  const apiBase =
    (import.meta as any).env?.VITE_API_BASE ||
    "https://www.wujisuanli.com"

  try {
    const ctrl = new AbortController()
    const timer = setTimeout(() => ctrl.abort(), HOT_RELOAD_TIMEOUT_MS)
    const resp = await fetch(`${apiBase}/api/v8/client/web-manifest`, {
      signal: ctrl.signal,
      cache: "no-store",
    })
    clearTimeout(timer)
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`)
    const manifest = await resp.json()
    const entryUrl = String(manifest?.entry_url || "").trim()
    if (entryUrl && entryUrl.startsWith("https://")) {
      try {
        sessionStorage.setItem(HOT_RELOAD_FLAG_KEY, JSON.stringify({
          version: manifest?.version,
          entry_url: entryUrl,
          at: new Date().toISOString(),
        }))
      } catch (_) { /* ignore */ }
      console.info("[hot-reload] navigate →", entryUrl)
      location.replace(entryUrl)
      return // 不挂载, 等远端 HTML 加载
    }
    mountApp(`远端无新版本 (manifest.version=${manifest?.version || "<空>"})`)
  } catch (e) {
    console.warn("[hot-reload] 拉 manifest 失败 · 走本地内嵌:", e)
    mountApp(`远端不可达 · ${String((e as any)?.message || e)}`)
  }
}

function mountApp(reason: string) {
  console.info("[bootstrap] mount local Vue ·", reason)
  const app = createApp(App)
  app.use(createPinia())
  app.mount("#app")
}

bootstrap()
