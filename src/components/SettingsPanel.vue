<script setup lang="ts">
import { ref, onMounted, computed } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { enable as enableAutostart, disable as disableAutostart, isEnabled as isAutostartEnabled } from "@tauri-apps/plugin-autostart"

interface Diagnostics {
  client_version: string
  platform: string
  arch: string
  api_base: string
  ws_url: string
  session_kind: string
  session_username: string
  session_email: string
  has_session: boolean
  node_id: string | null
  connection_state: string
  last_error: string | null
  mode: string
  throttle_pct: number
  data_dir: string
}

const props = defineProps<{ inline?: boolean }>()
const emit = defineEmits<{ close: [] }>()
const tab = ref<"settings" | "diagnostics" | "about">("settings")
const diag = ref<Diagnostics | null>(null)
const autostart = ref(false)
const checking = ref(false)
const updateMsg = ref<string | null>(null)
const resetting = ref(false)
const resetConfirm = ref(false)
const logs = ref<string[]>([])
const logsLoading = ref(false)
const copyDone = ref(false)

async function loadDiag() {
  diag.value = await invoke<Diagnostics>("get_diagnostics")
}

async function loadAutostart() {
  try {
    autostart.value = await isAutostartEnabled()
  } catch (e) {
    console.error("isAutostartEnabled failed:", e)
  }
}

async function toggleAutostart() {
  try {
    if (autostart.value) {
      await disableAutostart()
      autostart.value = false
    } else {
      await enableAutostart()
      autostart.value = true
    }
  } catch (e: any) {
    alert(`切换开机自启失败: ${e}`)
  }
}

async function checkUpdate() {
  checking.value = true
  updateMsg.value = null
  try {
    const r: any = await invoke("check_for_updates")
    if (r.available) {
      updateMsg.value = `✓ 发现新版本 v${r.version}`
    } else {
      updateMsg.value = "✓ 已是最新版本"
    }
  } catch (e: any) {
    updateMsg.value = `⚠ 检查失败: ${e}`
  } finally {
    checking.value = false
  }
}

async function loadLogs() {
  logsLoading.value = true
  try {
    const lines = await invoke<string[]>("get_recent_logs", { limit: 200 })
    logs.value = lines
  } catch {
    logs.value = ["（日志读取失败，请查看系统控制台）"]
  } finally {
    logsLoading.value = false
  }
}

async function copyDiag() {
  if (!diag.value) return
  const text = JSON.stringify(diag.value, null, 2)
  try {
    await navigator.clipboard.writeText(text)
    copyDone.value = true
    setTimeout(() => (copyDone.value = false), 2000)
  } catch {
    alert("复制失败，请手动截图")
  }
}

async function openDataDir() {
  try {
    await invoke("open_data_dir")
  } catch (e: any) {
    alert(`打开目录失败: ${e}`)
  }
}

async function doReset() {
  if (!resetConfirm.value) {
    resetConfirm.value = true
    setTimeout(() => (resetConfirm.value = false), 4000)
    return
  }
  resetting.value = true
  try {
    await invoke("reset_local_data")
    emit("close")
  } catch (e: any) {
    alert(`重置失败: ${e}`)
  } finally {
    resetting.value = false
    resetConfirm.value = false
  }
}

// ───── 老客户端检查 + 清理 (解决新旧客户端抢登录) ─────
interface OldVersionInfo {
  current_version: string
  old_processes: string[]
  old_data_dirs: string[]
}
const oldVersionChecking = ref(false)
const oldVersionCleaning = ref(false)
const oldVersionInfo = ref<OldVersionInfo | null>(null)
const oldVersionResult = ref<string | null>(null)

const oldVersionHasIssues = computed(() => {
  const i = oldVersionInfo.value
  return !!i && (i.old_processes.length > 0 || i.old_data_dirs.length > 0)
})

async function checkOldVersions() {
  oldVersionChecking.value = true
  oldVersionResult.value = null
  try {
    oldVersionInfo.value = await invoke<OldVersionInfo>("check_old_versions")
  } catch (e: any) {
    oldVersionResult.value = `检查失败: ${e}`
  } finally {
    oldVersionChecking.value = false
  }
}

async function cleanupOldVersions() {
  if (!oldVersionInfo.value) return
  oldVersionCleaning.value = true
  try {
    const killed = await invoke<string[]>("kill_old_processes")
    const cleaned = await invoke<string[]>("clean_old_data_dirs")
    oldVersionResult.value =
      `✓ 已终止 ${killed.length} 个旧进程，清理 ${cleaned.length} 个旧数据目录`
    // 重新检查
    await checkOldVersions()
  } catch (e: any) {
    oldVersionResult.value = `清理失败: ${e}`
  } finally {
    oldVersionCleaning.value = false
  }
}

const connStateLabel = computed(() => {
  const m: Record<string, string> = {
    disconnected: "未连接",
    connecting: "连接中",
    authenticating: "认证中",
    registered: "已连接",
    reconnecting: "重连中",
  }
  return m[diag.value?.connection_state || ""] || "—"
})

onMounted(async () => {
  await loadDiag()
  await loadAutostart()
})
</script>

<template>
  <div class="modal-overlay" :class="{ inline: props.inline }" @click.self="!props.inline && emit('close')">
    <div class="modal" :class="{ inline: props.inline }">
      <div class="modal-head">
        <div class="tabs">
          <button class="tab" :class="{ active: tab === 'settings' }" @click="tab = 'settings'">设置</button>
          <button class="tab" :class="{ active: tab === 'diagnostics' }" @click="tab = 'diagnostics'">诊断</button>
          <button class="tab" :class="{ active: tab === 'about' }" @click="tab = 'about'">关于</button>
        </div>
        <button v-if="!props.inline" class="close-btn" @click="emit('close')" title="关闭">✕</button>
      </div>

      <div class="modal-body">
        <!-- ── 设置 ── -->
        <div v-if="tab === 'settings'" class="content">
          <section class="row">
            <div class="row-info">
              <div class="row-title">开机自启</div>
              <div class="row-desc">登录系统后自动启动千手客户端，开始算力贡献</div>
            </div>
            <label class="switch">
              <input type="checkbox" :checked="autostart" @change="toggleAutostart" />
              <span class="slider"></span>
            </label>
          </section>

          <section class="row">
            <div class="row-info">
              <div class="row-title">检查更新</div>
              <div class="row-desc" v-if="!updateMsg">手动检查客户端是否有新版本</div>
              <div class="row-desc" v-else>{{ updateMsg }}</div>
            </div>
            <button class="btn ghost" :disabled="checking" @click="checkUpdate">
              {{ checking ? "检查中..." : "立即检查" }}
            </button>
          </section>

          <section class="row">
            <div class="row-info">
              <div class="row-title">数据目录</div>
              <div class="row-desc">查看 session.json / node_id.txt / device_name.txt 所在位置</div>
            </div>
            <button class="btn ghost" @click="openDataDir">打开</button>
          </section>

          <section class="row danger">
            <div class="row-info">
              <div class="row-title">重置本地数据</div>
              <div class="row-desc">清空 session、节点 ID、机器名等本地存储，回到登录页</div>
            </div>
            <button
              class="btn danger"
              :disabled="resetting"
              @click="doReset"
            >
              {{ resetting ? "重置中..." : (resetConfirm ? "确认清空" : "重置") }}
            </button>
          </section>
        </div>

        <!-- ── 诊断 ── -->
        <div v-else-if="tab === 'diagnostics'" class="content">
          <div class="diag-grid" v-if="diag">
            <div class="diag-section">
              <div class="diag-h">客户端</div>
              <div class="diag-kv"><span>版本</span><code>v{{ diag.client_version }}</code></div>
              <div class="diag-kv"><span>平台 / 架构</span><code>{{ diag.platform }} / {{ diag.arch }}</code></div>
              <div class="diag-kv"><span>数据目录</span><code class="path">{{ diag.data_dir }}</code></div>
            </div>

            <div class="diag-section">
              <div class="diag-h">连接</div>
              <div class="diag-kv"><span>状态</span><code>{{ connStateLabel }}</code></div>
              <div class="diag-kv"><span>节点 ID</span><code>{{ diag.node_id || "—" }}</code></div>
              <div class="diag-kv"><span>API</span><code class="path">{{ diag.api_base }}</code></div>
              <div class="diag-kv"><span>WebSocket</span><code class="path">{{ diag.ws_url }}</code></div>
              <div class="diag-kv" v-if="diag.last_error"><span>最近错误</span><code class="err">{{ diag.last_error }}</code></div>
            </div>

            <div class="diag-section">
              <div class="diag-h">会话</div>
              <div class="diag-kv"><span>状态</span><code>{{ diag.has_session ? "已登录" : "未登录" }}</code></div>
              <div class="diag-kv"><span>账号</span><code>{{ diag.session_username || "—" }}</code></div>
              <div class="diag-kv"><span>邮箱</span><code>{{ diag.session_email || "—" }}</code></div>
              <div class="diag-kv"><span>类型</span><code>{{ diag.session_kind || "—" }}</code></div>
            </div>

            <div class="diag-section">
              <div class="diag-h">运行模式</div>
              <div class="diag-kv"><span>当前模式</span><code>{{ diag.mode }}</code></div>
              <div class="diag-kv"><span>算力百分比</span><code>{{ diag.throttle_pct }}%</code></div>
            </div>
          </div>
          <div class="diag-actions">
            <button class="btn ghost" @click="loadDiag">↻ 刷新</button>
            <button class="btn ghost" @click="copyDiag">
              {{ copyDone ? "✓ 已复制" : "复制诊断信息" }}
            </button>
          </div>

          <div class="log-panel">
            <div class="log-panel-head">
              <span class="log-panel-title">最近日志</span>
              <button class="btn ghost small" @click="loadLogs" :disabled="logsLoading">
                {{ logsLoading ? "加载中..." : "↻ 加载" }}
              </button>
            </div>
            <div class="log-body" v-if="logs.length">
              <div v-for="(l, i) in logs" :key="i" class="log-line"
                :class="{ 'log-err': l.includes('ERROR') || l.includes('error'), 'log-warn': l.includes('WARN') }">
                {{ l }}
              </div>
            </div>
            <div class="log-empty" v-else>点击「↻ 加载」查看运行日志</div>
          </div>

          <!-- ── 老客户端清理 (解决新旧抢登录) ── -->
          <div class="old-version-panel">
            <div class="old-version-head">
              <div>
                <div class="old-version-title">老客户端检查</div>
                <div class="old-version-sub">解决新旧客户端抢登录、卸载不干净等问题</div>
              </div>
              <button class="btn ghost small" @click="checkOldVersions" :disabled="oldVersionChecking">
                {{ oldVersionChecking ? "检查中..." : "🔍 检查" }}
              </button>
            </div>

            <!-- 检查结果 -->
            <div v-if="oldVersionInfo" class="old-version-body">
              <div v-if="oldVersionHasIssues" class="ov-issues">
                <div v-if="oldVersionInfo.old_processes.length" class="ov-section">
                  <div class="ov-label">⚠ 发现 {{ oldVersionInfo.old_processes.length }} 个旧版进程</div>
                  <ul class="ov-list">
                    <li v-for="(p, i) in oldVersionInfo.old_processes" :key="i">
                      <code>{{ p }}</code>
                    </li>
                  </ul>
                </div>
                <div v-if="oldVersionInfo.old_data_dirs.length" class="ov-section">
                  <div class="ov-label">⚠ 发现 {{ oldVersionInfo.old_data_dirs.length }} 个旧数据目录</div>
                  <ul class="ov-list">
                    <li v-for="(d, i) in oldVersionInfo.old_data_dirs" :key="i">
                      <code class="path">{{ d }}</code>
                    </li>
                  </ul>
                </div>
                <button class="btn danger" @click="cleanupOldVersions" :disabled="oldVersionCleaning">
                  {{ oldVersionCleaning ? "清理中..." : "🧹 一键清理 (杀进程 + 删数据)" }}
                </button>
              </div>
              <div v-else class="ov-ok">
                ✓ 无老版本残留 · 当前版本 v{{ oldVersionInfo.current_version }}
              </div>
            </div>

            <!-- 清理结果 -->
            <div v-if="oldVersionResult" class="ov-result" :class="{ 'ov-result-err': oldVersionResult.startsWith('清理失败') || oldVersionResult.startsWith('检查失败') }">
              {{ oldVersionResult }}
            </div>
          </div>
        </div>

        <!-- ── 关于 ── -->
        <div v-else class="content about">
          <h2>千手</h2>
          <p class="version" v-if="diag">v{{ diag.client_version }} · {{ diag.platform }}/{{ diag.arch }}</p>
          <p class="desc">
            把闲置算力贡献给千手边缘网络，获得 EDG 收益。<br>
            支持 shell / Python / Node.js / FFmpeg 等多种任务类型。
          </p>
          <div class="links">
            <a href="https://www.wujisuanli.com" target="_blank" rel="noopener">官网</a>
            <span>·</span>
            <a href="https://github.com/LHXSX/suanli" target="_blank" rel="noopener">GitHub</a>
            <span>·</span>
            <a href="mailto:support@wujisuanli.com">问题反馈</a>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  backdrop-filter: blur(4px);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 200;
}
.modal-overlay.inline {
  position: static;
  inset: auto;
  background: transparent;
  backdrop-filter: none;
  display: block;
  z-index: auto;
}
.modal {
  width: min(640px, calc(100vw - 40px));
  max-height: calc(100vh - 80px);
  background: var(--c-bg-card);
  border: 1px solid #222;
  border-radius: 14px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.6);
}
.modal.inline {
  width: 100%;
  max-width: none;
  max-height: none;
  background: transparent;
  border: none;
  border-radius: 0;
  box-shadow: none;
}
.modal.inline .modal-head {
  border-bottom: none;
  padding: 0 0 12px;
}
.modal.inline .modal-body {
  padding: 0;
}
.modal-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  border-bottom: 1px solid var(--c-border);
}
.tabs {
  display: flex;
  background: var(--c-bg-soft);
  border-radius: 8px;
  padding: 3px;
  gap: 2px;
}
.tab {
  background: transparent;
  border: none;
  color: var(--c-mute);
  padding: 6px 14px;
  border-radius: 6px;
  font-size: 13.5px;
  font-weight: 500;
  cursor: pointer;
}
.tab:hover { color: var(--c-fg); }
.tab.active {
  background: var(--c-accent);
  color: #fff;
}
.close-btn {
  background: transparent;
  border: none;
  color: var(--c-mute);
  font-size: 18px;
  width: 28px;
  height: 28px;
  border-radius: 6px;
  cursor: pointer;
}
.close-btn:hover {
  color: var(--c-fg);
  background: var(--c-border);
}
.modal-body {
  flex: 1;
  overflow-y: auto;
  padding: 18px 20px;
}
.content {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

/* Setting row */
.row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  padding: 12px 14px;
  background: var(--c-bg-soft);
  border: 1px solid var(--c-border);
  border-radius: 10px;
}
.row.danger { border-color: rgba(255, 69, 58, 0.25); }
.row-info { flex: 1; min-width: 0; }
.row-title {
  font-size: 14.5px;
  font-weight: 500;
  margin-bottom: 2px;
}
.row-desc {
  font-size: 14.5px;
  color: var(--c-mute);
  line-height: 1.4;
}

/* Switch */
.switch {
  position: relative;
  display: inline-block;
  width: 38px;
  height: 22px;
  cursor: pointer;
}
.switch input { display: none; }
.slider {
  position: absolute;
  inset: 0;
  background: var(--c-border-strong);
  border-radius: 999px;
  transition: background 0.2s;
}
.slider::before {
  content: "";
  position: absolute;
  height: 16px;
  width: 16px;
  left: 3px;
  top: 3px;
  background: #fff;
  border-radius: 50%;
  transition: transform 0.2s;
}
.switch input:checked + .slider {
  background: var(--c-accent);
}
.switch input:checked + .slider::before {
  transform: translateX(16px);
}

/* Buttons */
.btn {
  padding: 6px 14px;
  border-radius: 7px;
  font-size: 13.5px;
  font-weight: 500;
  border: none;
  cursor: pointer;
  white-space: nowrap;
}
.btn.ghost {
  background: transparent;
  border: 1px solid var(--c-border-strong);
  color: var(--c-mute);
}
.btn.ghost:hover { color: var(--c-fg); border-color: var(--c-mute); }
.btn.ghost:disabled { opacity: 0.5; cursor: not-allowed; }
.btn.danger {
  background: rgba(255, 69, 58, 0.1);
  border: 1px solid rgba(255, 69, 58, 0.4);
  color: var(--c-err);
}
.btn.danger:hover { background: rgba(255, 69, 58, 0.2); }

/* Diagnostics */
.diag-grid {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.diag-section {
  background: var(--c-bg-soft);
  border: 1px solid var(--c-border);
  border-radius: 10px;
  padding: 12px 14px;
}
.diag-h {
  font-size: 14.5px;
  color: var(--c-mute);
  text-transform: uppercase;
  letter-spacing: 0.05em;
  margin-bottom: 8px;
}
.diag-kv {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  padding: 4px 0;
  font-size: 13.5px;
  border-bottom: 1px solid var(--c-bg-card);
}
.diag-kv:last-child { border-bottom: none; }
.diag-kv span {
  color: var(--c-mute);
  flex-shrink: 0;
}
.diag-kv code {
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 14.5px;
  color: var(--c-fg);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 60%;
}
.diag-kv code.path {
  font-size: 14px;
}
.diag-kv code.err {
  color: var(--c-err);
  white-space: pre-wrap;
}
.diag-actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 8px;
}

/* About */
.about {
  text-align: center;
  padding: 24px 0;
  align-items: center;
}
.about h2 {
  font-size: 28px;
  font-weight: 700;
  background: linear-gradient(135deg, var(--c-accent), var(--c-accent-2));
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
  margin: 0 0 6px;
}
.about .version {
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 14.5px;
  color: var(--c-mute);
  margin: 0 0 16px;
}
.about .desc {
  font-size: 14.5px;
  color: var(--c-fg);
  line-height: 1.7;
  margin: 0 0 18px;
  max-width: 480px;
}
.about .links {
  display: flex;
  gap: 10px;
  align-items: center;
  justify-content: center;
  font-size: 13.5px;
}
.about .links a {
  color: var(--c-accent);
  text-decoration: none;
}
.about .links a:hover { text-decoration: underline; }
.about .links span { color: var(--c-mute); }

/* Diag actions */
.diag-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 8px;
}
.btn.small { padding: 4px 10px; font-size: 12px; }

/* Log panel */
.log-panel {
  margin-top: 12px;
  background: #0d1117;
  border: 1px solid #1a1f2e;
  border-radius: 10px;
  overflow: hidden;
}
.log-panel-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 12px;
  border-bottom: 1px solid #1a1f2e;
  background: rgba(255,255,255,0.03);
}
.log-panel-title {
  font-size: 12px;
  font-weight: 600;
  color: #8a93a6;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}
.log-body {
  max-height: 220px;
  overflow-y: auto;
  padding: 8px 12px;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.log-line {
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 11.5px;
  color: #c9d1d9;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
}
.log-err { color: #f87171; }
.log-warn { color: #fbbf24; }
.log-empty {
  padding: 20px;
  text-align: center;
  font-size: 13px;
  color: #4a5568;
}

/* ── 老客户端清理面板 ── */
.old-version-panel {
  margin-top: 18px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 12px;
  padding: 16px 18px;
}
.old-version-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
}
.old-version-title {
  font-size: 14px;
  font-weight: 600;
  color: #fff;
}
.old-version-sub {
  font-size: 12px;
  color: #a0aec0;
  margin-top: 2px;
}
.old-version-body {
  margin-top: 14px;
}
.ov-issues {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.ov-section {
  background: rgba(255, 69, 58, 0.05);
  border-left: 3px solid rgba(255, 159, 10, 0.6);
  padding: 10px 14px;
  border-radius: 8px;
}
.ov-label {
  font-size: 13px;
  font-weight: 600;
  color: #fbbf24;
  margin-bottom: 8px;
}
.ov-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.ov-list li code {
  font-family: ui-monospace, "JetBrains Mono", monospace;
  font-size: 12px;
  color: #cbd5e0;
  word-break: break-all;
}
.ov-list code.path {
  color: #a5b4fc;
}
.ov-ok {
  padding: 12px 14px;
  background: rgba(34, 197, 94, 0.08);
  border-left: 3px solid #22c55e;
  border-radius: 8px;
  font-size: 13px;
  color: #86efac;
}
.ov-result {
  margin-top: 12px;
  padding: 10px 14px;
  background: rgba(34, 197, 94, 0.08);
  border-radius: 8px;
  font-size: 13px;
  color: #86efac;
}
.ov-result-err {
  background: rgba(255, 69, 58, 0.1);
  color: #fca5a5;
}
</style>
