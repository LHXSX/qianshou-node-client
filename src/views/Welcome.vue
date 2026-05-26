<script setup lang="ts">
import { ref, onMounted } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { useConnection } from "../composables/useConnection"

const { authLogin, authRegister } = useConnection()

// 2026-05-25 · 版本号从 package.json 注入 (vite.config.ts define) · 不再硬编码
const appVersion = __APP_VERSION__

const tab = ref<"login" | "register">("login")
const username = ref("")
const password = ref("")
const email = ref("")
const confirmPassword = ref("")
const rememberMe = ref(false)
const rememberedLoaded = ref(false)
const busy = ref(false)
const errMsg = ref("")

onMounted(async () => {
  try {
    const saved = await invoke<string | null>("load_remembered_account")
    if (saved) {
      username.value = saved
      rememberMe.value = true
    }
  } catch {
    // 忽略
  } finally {
    rememberedLoaded.value = true
  }
})

/** 2026-05-26 · 兜底过滤 · 即使老 Rust 还在跑 · 也尽量去掉 "HTTP 4xx" + JSON 残留 */
function sanitizeErr(e: any): string {
  let raw = typeof e === "string" ? e : (e?.message || String(e))
  raw = raw.trim()
  // 形如 "HTTP 401 {...json...}" · 取 message 字段
  const m = raw.match(/^HTTP\s+\d{3}\s+(\{.*\})$/)
  if (m) {
    try {
      const j = JSON.parse(m[1])
      if (j.message) return String(j.message)
      if (j.detail) return typeof j.detail === "string" ? j.detail : JSON.stringify(j.detail)
    } catch {}
  }
  // 去掉开头的 "HTTP 4xx" 噪音
  raw = raw.replace(/^HTTP\s+\d{3}\s+/i, "").trim()
  return raw || "请求失败 · 请重试"
}

async function onLogin() {
  errMsg.value = ""
  if (!username.value.trim() || !password.value.trim()) {
    errMsg.value = "请输入账号和密码"
    return
  }
  busy.value = true
  try {
    await authLogin(username.value.trim(), password.value.trim())
    if (rememberMe.value) {
      await invoke("save_remembered_account", { username: username.value.trim() })
    } else {
      await invoke("clear_remembered_account")
    }
  } catch (e: any) {
    errMsg.value = sanitizeErr(e)
  } finally {
    busy.value = false
  }
}

async function onRegister() {
  errMsg.value = ""
  if (!username.value.trim() || !password.value.trim()) {
    errMsg.value = "请输入账号和密码"
    return
  }
  if (password.value !== confirmPassword.value) {
    errMsg.value = "两次密码不一致"
    return
  }
  busy.value = true
  try {
    await authRegister(username.value.trim(), email.value.trim(), password.value.trim())
    if (rememberMe.value) {
      await invoke("save_remembered_account", { username: username.value.trim() })
    } else {
      await invoke("clear_remembered_account")
    }
  } catch (e: any) {
    errMsg.value = sanitizeErr(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="welcome">
    <div class="brand">
      <h1>千手</h1>
      <p class="tagline">边缘算力贡献网络 · v{{ appVersion }}</p>
    </div>

    <div class="card">
      <div class="tabs">
        <button
          class="tab"
          :class="{ active: tab === 'login' }"
          @click="tab = 'login'"
        >登录</button>
        <button
          class="tab"
          :class="{ active: tab === 'register' }"
          @click="tab = 'register'"
        >注册</button>
      </div>

      <template v-if="tab === 'login'">
        <h2>欢迎回来</h2>
        <p class="hint">用账号和密码登录已有账户</p>
        <input
          v-model="username"
          type="text"
          placeholder="账号 / 用户名"
          class="input"
          autofocus
          @keyup.enter="onLogin"
        />
        <input
          v-model="password"
          type="password"
          placeholder="密码"
          class="input"
          @keyup.enter="onLogin"
        />
        <div v-if="errMsg" class="alert-err" role="alert">
          <span class="alert-icon" aria-hidden="true">⚠</span>
          <div class="alert-content">
            <div class="alert-title">登录失败</div>
            <div class="alert-msg">{{ errMsg }}</div>
          </div>
          <button class="alert-close" aria-label="关闭" @click="errMsg = ''">×</button>
        </div>
        <label class="remember-row">
          <input type="checkbox" v-model="rememberMe" />
          <span>记住账号</span>
        </label>
        <button class="btn primary" :disabled="busy" @click="onLogin">
          {{ busy ? "登录中..." : "登录" }}
        </button>
        <div class="divider"><span>其他登录方式</span></div>
        <button class="btn wechat" disabled>
          <span class="wechat-icon">💬</span> 微信登录（即将上线）
        </button>
      </template>

      <template v-else>
        <h2>创建账户</h2>
        <p class="hint">设置账号、邮箱和密码</p>
        <input
          v-model="username"
          type="text"
          placeholder="账号 / 用户名（≥ 3 位）"
          class="input"
          autofocus
          @keyup.enter="onRegister"
        />
        <input
          v-model="email"
          type="email"
          placeholder="邮箱"
          class="input"
          @keyup.enter="onRegister"
        />
        <input
          v-model="password"
          type="password"
          placeholder="密码（≥ 6 位）"
          class="input"
          @keyup.enter="onRegister"
        />
        <input
          v-model="confirmPassword"
          type="password"
          placeholder="确认密码"
          class="input"
          @keyup.enter="onRegister"
        />
        <div v-if="errMsg" class="alert-err" role="alert">
          <span class="alert-icon" aria-hidden="true">⚠</span>
          <div class="alert-content">
            <div class="alert-title">注册失败</div>
            <div class="alert-msg">{{ errMsg }}</div>
          </div>
          <button class="alert-close" aria-label="关闭" @click="errMsg = ''">×</button>
        </div>
        <label class="remember-row">
          <input type="checkbox" v-model="rememberMe" />
          <span>记住账号</span>
        </label>
        <button class="btn primary" :disabled="busy" @click="onRegister">
          {{ busy ? "注册中..." : "注册并登录" }}
        </button>
      </template>
    </div>

    <p class="footer-hint">登录后自动接入千手边缘网络，开始贡献闲置算力。</p>
  </div>
</template>

<style scoped>
.welcome {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-height: 100vh;
  padding: 40px 24px;
  gap: 32px;
}
.brand {
  text-align: center;
}
.brand h1 {
  margin: 0;
  font-size: 36px;
  font-weight: 600;
  letter-spacing: -0.02em;
  background: linear-gradient(135deg, var(--c-accent) 0%, var(--c-accent-2) 100%);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  background-clip: text;
}
.tagline {
  margin: 8px 0 0;
  color: var(--c-mute);
  font-size: 14.5px;
  font-family: ui-monospace, SFMono-Regular, monospace;
}
.card {
  width: 100%;
  max-width: 420px;
  background: var(--c-bg-card);
  border: 1px solid #222;
  border-radius: 16px;
  padding: 28px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.card h2 {
  margin: 0;
  font-size: 20px;
  font-weight: 600;
}

/* M3.5.1 登录/注册 tabs */
.tabs {
  display: flex;
  background: var(--c-bg-soft);
  border-radius: 10px;
  padding: 4px;
  gap: 2px;
}
.tab {
  flex: 1;
  background: transparent;
  border: none;
  color: var(--c-mute);
  padding: 8px 12px;
  border-radius: 7px;
  font-size: 14.5px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
}
.tab:hover {
  color: var(--c-fg);
}
.tab.active {
  background: var(--c-accent);
  color: #fff;
}
.hint {
  color: var(--c-mute);
  font-size: 14.5px;
  margin: 0;
  line-height: 1.6;
}
.muted {
  color: var(--c-warn);
  font-size: 13.5px;
}
.input {
  background: var(--c-bg-soft);
  border: 1px solid var(--c-border-strong);
  border-radius: 10px;
  padding: 12px 14px;
  color: var(--c-fg);
  font-size: 14px;
  outline: none;
  transition: border-color 0.15s;
}
.input:focus {
  border-color: var(--c-accent);
}
.code-input {
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 24px;
  letter-spacing: 0.4em;
  text-align: center;
  padding-left: 14px;
}
.btn {
  padding: 12px 20px;
  border-radius: 10px;
  font-size: 14px;
  font-weight: 500;
  border: none;
  transition: opacity 0.15s, background 0.15s;
}
.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.btn.primary {
  background: var(--c-accent);
  color: #fff;
}
.btn.primary:hover:not(:disabled) {
  background: #0070dd;
}
.btn.ghost {
  background: transparent;
  color: var(--c-mute);
  font-size: 14.5px;
}
.btn.ghost:hover:not(:disabled) {
  color: var(--c-fg);
}
.err {
  color: var(--c-err);
  font-size: 14.5px;
}
/* 2026-05-26 · 友好错误 alert (替换原 .err 朴素红字) */
.alert-err {
  display: flex;
  gap: 10px;
  align-items: flex-start;
  background: rgba(239, 68, 68, 0.08);
  border: 1px solid rgba(239, 68, 68, 0.35);
  border-left: 3px solid rgba(239, 68, 68, 0.9);
  border-radius: 10px;
  padding: 10px 12px;
  margin: 4px 0 2px;
  animation: alertSlideIn 0.18s ease-out;
}
@keyframes alertSlideIn {
  from { opacity: 0; transform: translateY(-4px); }
  to   { opacity: 1; transform: translateY(0); }
}
.alert-icon {
  flex: 0 0 auto;
  font-size: 18px;
  line-height: 1.2;
  color: #ef4444;
  margin-top: 1px;
}
.alert-content { flex: 1 1 auto; min-width: 0; }
.alert-title {
  font-size: 13.5px;
  font-weight: 600;
  color: #ef4444;
  margin-bottom: 2px;
}
.alert-msg {
  font-size: 13px;
  color: var(--c-fg);
  word-break: break-word;
  line-height: 1.45;
}
.alert-close {
  flex: 0 0 auto;
  background: transparent;
  border: none;
  color: rgba(239, 68, 68, 0.6);
  cursor: pointer;
  font-size: 18px;
  line-height: 1;
  padding: 0 4px;
  margin: -2px -4px 0 0;
  border-radius: 4px;
  transition: all 0.15s;
}
.alert-close:hover {
  color: #ef4444;
  background: rgba(239, 68, 68, 0.12);
}
.remember-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13.5px;
  color: var(--c-mute);
  cursor: pointer;
  user-select: none;
}
.remember-row input[type="checkbox"] {
  width: 16px;
  height: 16px;
  accent-color: var(--c-accent);
  cursor: pointer;
}
.divider {
  display: flex;
  align-items: center;
  gap: 12px;
  color: var(--c-mute);
  font-size: 12px;
}
.divider::before,
.divider::after {
  content: "";
  flex: 1;
  height: 1px;
  background: var(--c-border);
}
.btn.wechat {
  background: #07c160;
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
}
.btn.wechat:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.wechat-icon {
  font-size: 18px;
}
.footer-hint {
  color: var(--c-mute);
  font-size: 13.5px;
  text-align: center;
  max-width: 420px;
  margin: 0;
  line-height: 1.6;
}
</style>
