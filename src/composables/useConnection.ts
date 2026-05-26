import { ref, onMounted, onBeforeUnmount } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

export interface UserInfo {
  id: number
  username: string
  email: string
}

export interface AppStateSnapshot {
  connection_state: "disconnected" | "connecting" | "authenticating" | "registered" | "reconnecting"
  state_label: string
  node_id: string | null
  owner_id: number | null
  server_version: string | null
  last_error: string | null
  client_version: string
  user: UserInfo | null
  is_authenticated: boolean
  current_task_id: string | null
  mode: string
  throttle_pct: number
  /** 心跳 RTT 毫秒，null 表示尚未测量 */
  latency_ms: number | null
}

const _snap = ref<AppStateSnapshot>({
  connection_state: "disconnected",
  state_label: "未连接",
  node_id: null,
  owner_id: null,
  server_version: null,
  last_error: null,
  client_version: "3.0.0",
  user: null,
  is_authenticated: false,
  current_task_id: null,
  mode: "active",
  throttle_pct: 100,
  latency_ms: null,
})

async function setMode(mode: "active" | "paused" | "throttled") {
  await invoke("set_mode", { mode })
}

async function setThrottle(pct: number) {
  await invoke("set_throttle", { pct: Math.max(0, Math.min(100, Math.round(pct))) })
}

let _unlisten: UnlistenFn | null = null
let _initialized = false

export function useConnection() {
  async function refresh() {
    _snap.value = await invoke<AppStateSnapshot>("get_state")
  }

  async function disconnect() {
    await invoke("ws_disconnect")
  }

  // 登录三件套（M2.3 magic-link，保留兼容）
  async function authSendCode(email: string): Promise<number> {
    return await invoke<number>("auth_send_code", { email })
  }
  async function authVerify(email: string, code: string): Promise<UserInfo> {
    return await invoke<UserInfo>("auth_verify", { email, code })
  }
  async function authRestore(): Promise<UserInfo | null> {
    return await invoke<UserInfo | null>("auth_restore")
  }
  async function authLogout(): Promise<void> {
    await invoke("auth_logout")
  }
  // M3.5.1 密码登录 + 注册
  async function authLogin(username: string, password: string): Promise<UserInfo> {
    return await invoke<UserInfo>("auth_login", { username, password })
  }
  async function authRegister(username: string, email: string, password: string): Promise<UserInfo> {
    return await invoke<UserInfo>("auth_register", { username, email, password })
  }

  onMounted(async () => {
    if (_initialized) return
    _initialized = true
    _unlisten = await listen<AppStateSnapshot>("connection_state_changed", (e) => {
      console.log("📡 state event:", JSON.stringify(e.payload))
      _snap.value = e.payload
    })
    await refresh()
    console.log("📥 initial state:", JSON.stringify(_snap.value))
  })

  onBeforeUnmount(() => {
    // 不释放（单例，整个 app 生命周期）
  })

  return {
    snap: _snap,
    refresh,
    disconnect,
    authSendCode,
    authVerify,
    authRestore,
    authLogout,
    authLogin,
    authRegister,
    setMode,
    setThrottle,
  }
}

export function cleanupConnection() {
  if (_unlisten) {
    _unlisten()
    _unlisten = null
  }
  _initialized = false
}
