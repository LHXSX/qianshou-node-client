/**
 * useEventStream · 订阅 v8 实时事件
 *
 * 2026-05-18 全栈实时化 · 替代 polling
 *
 * 用法 (页面):
 *   const { on, off } = useEventStream()
 *   onMounted(() => {
 *     on("workload.done", (payload) => { loadTasks() })
 *     on("worker.online", (payload) => { loadNodes() })
 *   })
 *   onUnmounted(() => { off() })
 *
 * 全局单例 · 整个 app 共用 1 个 ws · 多页面 listen 同一份事件
 */
import { ref, onUnmounted } from "vue"
import { PRIMARY_DOMAIN } from "@shared"

// 2026-05-25 8.0.9 修 WS URL bug · PRIMARY_DOMAIN 已含 https:// scheme
// 旧代码: host = PRIMARY_DOMAIN  → wss://https://www.wujisuanli.com/... (畸形)
const WS_BASE = (() => {
  // wss://www.wujisuanli.com/api/v8/ws/events
  const u = new URL(window.location.href)
  let proto = u.protocol === "https:" ? "wss:" : "ws:"
  // 默认走当前 host (生产 / Tauri webview)
  // Tauri 中 window.location.host 是 tauri.localhost · 需手动指向真实 backend
  let host = u.host
  if (host.includes("tauri") || host.includes("localhost") || host === "") {
    // 从 PRIMARY_DOMAIN (含 scheme) 解析出纯 hostname · 同时保留 wss
    try {
      const parsed = new URL(PRIMARY_DOMAIN)
      host = parsed.host
      proto = parsed.protocol === "https:" ? "wss:" : "ws:"
    } catch {
      host = "www.wujisuanli.com"
      proto = "wss:"
    }
  }
  return `${proto}//${host}/api/v8/ws/events`
})()

type EventHandler = (payload: any) => void

class EventStream {
  ws: WebSocket | null = null
  handlers: Map<string, Set<EventHandler>> = new Map()
  connected = ref(false)
  reconnectTimer: any = null
  pingTimer: any = null

  connect(token: string) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) return

    const url = `${WS_BASE}?token=${encodeURIComponent(token)}`
    try {
      this.ws = new WebSocket(url)
      this.ws.onopen = () => {
        console.log("[ws.events] 连上", url)
        this.connected.value = true
        // 启动 ping (30s 保活)
        this.pingTimer = setInterval(() => {
          try { this.ws?.send(JSON.stringify({ type: "ping" })) } catch {}
        }, 30_000)
      }
      this.ws.onmessage = (e) => {
        try {
          const msg = JSON.parse(e.data)
          const handlers = this.handlers.get(msg.type)
          if (handlers) {
            handlers.forEach(h => {
              try { h(msg.payload) } catch (err) { console.warn("[ws.events] handler 错", err) }
            })
          }
          // 也 trigger * 通配符 (调试用)
          const wild = this.handlers.get("*")
          if (wild) {
            wild.forEach(h => h({ type: msg.type, ...msg.payload }))
          }
        } catch (err) {
          console.warn("[ws.events] parse 错", err)
        }
      }
      this.ws.onclose = () => {
        console.log("[ws.events] 断 · 3s 后重连")
        this.connected.value = false
        if (this.pingTimer) { clearInterval(this.pingTimer); this.pingTimer = null }
        this.reconnectTimer = setTimeout(() => this.connect(token), 3000)
      }
      this.ws.onerror = (e) => {
        console.warn("[ws.events] 错", e)
      }
    } catch (e) {
      console.warn("[ws.events] 连接失败:", e)
      this.reconnectTimer = setTimeout(() => this.connect(token), 5000)
    }
  }

  on(eventType: string, handler: EventHandler) {
    if (!this.handlers.has(eventType)) {
      this.handlers.set(eventType, new Set())
    }
    this.handlers.get(eventType)!.add(handler)
  }

  off(eventType?: string, handler?: EventHandler) {
    if (!eventType) {
      this.handlers.clear()
    } else if (handler) {
      this.handlers.get(eventType)?.delete(handler)
    } else {
      this.handlers.delete(eventType)
    }
  }

  disconnect() {
    if (this.reconnectTimer) { clearTimeout(this.reconnectTimer); this.reconnectTimer = null }
    if (this.pingTimer) { clearInterval(this.pingTimer); this.pingTimer = null }
    try { this.ws?.close() } catch {}
    this.ws = null
    this.connected.value = false
  }
}

const _instance = new EventStream()

export function useEventStream() {
  const localHandlers: Array<[string, EventHandler]> = []

  return {
    connected: _instance.connected,
    connect: (token: string) => _instance.connect(token),
    on(eventType: string, handler: EventHandler) {
      _instance.on(eventType, handler)
      localHandlers.push([eventType, handler])
    },
    off() {
      // 仅清这次 use 注册的 handlers
      for (const [t, h] of localHandlers) {
        _instance.off(t, h)
      }
      localHandlers.length = 0
    },
    disconnect: () => _instance.disconnect(),
    // 自动清理 (页面 unmount 时)
    autoCleanup() {
      onUnmounted(() => {
        for (const [t, h] of localHandlers) {
          _instance.off(t, h)
        }
        localHandlers.length = 0
      })
    },
  }
}
