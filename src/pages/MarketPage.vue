<script setup lang="ts">
import { ref, onMounted } from "vue"
import { invoke } from "@tauri-apps/api/core"

import { API as Paths, apiUrl } from "@shared"

interface ScriptItem {
  id: string
  description: string
  task_type: string
  code_url: string
  filename: string
  used_count: number
  tags: string
  preview: Record<string, any>
  created_at: string
  updated_at: string
}

interface DFTask {
  task_id: string
  name: string
  data_type: string
  operation: string
  security_level: string
  budget?: number
  estimate?: any
  status?: string
  created_at?: string
}

const tab = ref<"scripts" | "factory">("scripts")
const scripts = ref<ScriptItem[]>([])
const dfTasks = ref<DFTask[]>([])
const loading = ref(false)
const error = ref<string | null>(null)
const query = ref("")

async function loadScripts() {
  loading.value = true
  error.value = null
  try {
    // v8 收口 · 节点能力库 (54 个内置脚本)
    const body = await invoke<string>("api_get", { url: apiUrl(Paths.scripts.list()) })
    if (!body || !body.trim()) return
    let j: any
    try { j = JSON.parse(body) } catch { return }
    const items = j.items || []
    // 本地过滤
    const q = query.value.trim().toLowerCase()
    const mapped = items.map((s: any) => {
      // 净化 description: 去掉 "task_type —" 前缀
      let desc = String(s.description || "")
      const prefix = `${s.task_type} — `
      if (desc.startsWith(prefix)) desc = desc.slice(prefix.length)
      desc = desc.replace(/\.py — /, "")
      return {
        id: s.task_type,
        task_type: s.task_type,
        description: desc || "(无描述)",
        filename: s.name,
        code_url: apiUrl(s.url),
        used_count: 0,
        tags: s.category || "",
        preview: {},
        created_at: "",
        updated_at: "",
      }
    })
    scripts.value = q
      ? mapped.filter((s: any) =>
          String(s.task_type).toLowerCase().includes(q) ||
          String(s.description).toLowerCase().includes(q))
      : mapped
  } catch (e: any) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

async function loadFactory() {
  loading.value = true
  error.value = null
  try {
    // v8 收口 · 数据工厂 = 全平台排队/活跃的真实 workloads (高价悬赏)
    const body = await invoke<string>("api_get", { url: apiUrl(Paths.workloads.list({ limit: 200 })) })
    if (!body || !body.trim()) return
    let j: any
    try { j = JSON.parse(body) } catch { return }
    const items = Array.isArray(j) ? j : (j.items || [])
    // 只显示活跃: CREATED/RUNNING/DISPATCHED
    const active = items.filter((w: any) =>
      ["CREATED", "RUNNING", "DISPATCHED", "PENDING", "QUEUED"].includes(String(w.status || "").toUpperCase())
    )
    // 按 budget 高到低
    active.sort((a: any, b: any) => Number(b.budget || 0) - Number(a.budget || 0))
    dfTasks.value = active.map((w: any) => ({
      task_id: w.id,
      name: w.name,
      data_type: w.spec?.task_type || "",
      operation: w.spec?.task_type || "",
      security_level: "standard",
      budget: Number(w.budget || 0),
      status: String(w.status || "").toLowerCase(),
      created_at: w.created_at,
    }))
  } catch (e: any) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

function switchTab(t: "scripts" | "factory") {
  tab.value = t
  if (t === "scripts") loadScripts()
  else loadFactory()
}

function fmtTime(s?: string): string {
  if (!s) return ""
  try {
    return new Date(s).toLocaleString("zh-CN", { hour12: false, month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })
  } catch {
    return s
  }
}

onMounted(() => {
  loadScripts()
})
</script>

<template>
  <div class="page">
    <div class="page-head">
      <h2>任务市场</h2>
      <p class="sub">浏览 AI 脚本、数据工厂悬赏任务 · 接入贡献算力赚取 EDG</p>
    </div>

    <div class="tabs">
      <button
        class="tab"
        :class="{ active: tab === 'scripts' }"
        @click="switchTab('scripts')"
      >
        ⚡ 脚本市场
      </button>
      <button
        class="tab"
        :class="{ active: tab === 'factory' }"
        @click="switchTab('factory')"
      >
        🏭 数据工厂任务
      </button>
      <div class="spacer" />
      <div class="search" v-if="tab === 'scripts'">
        <input
          v-model="query"
          placeholder="搜索脚本描述..."
          class="search-input"
          @keyup.enter="loadScripts"
        />
        <button class="btn" @click="loadScripts">搜索</button>
      </div>
    </div>

    <div v-if="loading" class="state-msg">加载中…</div>
    <div v-else-if="error" class="state-msg err">加载失败：{{ error }}</div>

    <!-- 脚本市场 -->
    <div v-else-if="tab === 'scripts'" class="grid">
      <div v-if="scripts.length === 0" class="empty">
        <div class="empty-icon">📜</div>
        <div class="empty-title">暂无脚本</div>
        <div class="empty-hint">脚本通过 /api/v8/scripts/ai-generate 或 /templates 入库</div>
      </div>
      <article v-for="s in scripts" :key="s.id" class="script-card">
        <div class="sc-head">
          <span class="sc-type">{{ s.task_type || "generic" }}</span>
          <span class="sc-uses" v-if="s.used_count">★ {{ s.used_count }} 次使用</span>
        </div>
        <div class="sc-desc">{{ s.description || "(无描述)" }}</div>
        <div class="sc-file">
          <code>{{ s.filename }}</code>
        </div>
        <div class="sc-tags" v-if="s.tags">{{ s.tags }}</div>
        <div class="sc-foot">
          <span class="sc-time">{{ fmtTime(s.updated_at) }}</span>
          <a :href="s.code_url" target="_blank" rel="noopener" class="sc-view">查看源码 ↗</a>
        </div>
      </article>
    </div>

    <!-- 数据工厂任务 -->
    <div v-else-if="tab === 'factory'" class="factory-list">
      <div v-if="dfTasks.length === 0" class="empty">
        <div class="empty-icon">🏭</div>
        <div class="empty-title">暂无数据工厂任务</div>
        <div class="empty-hint">
          通过 <code>POST /api/v8/workloads</code> 提交数据任务<br>
          支持 text/pdf/image/audio/video，操作 ocr/dedup/transcribe 等
        </div>
      </div>
      <article v-for="t in dfTasks" :key="t.task_id" class="df-card">
        <div class="df-head">
          <span class="df-type">{{ t.data_type }}</span>
          <span class="df-op">{{ t.operation }}</span>
          <span class="df-status" v-if="t.status">{{ t.status }}</span>
          <span class="df-budget" v-if="t.budget">悬赏 {{ t.budget }} EDG</span>
        </div>
        <div class="df-name">{{ t.name }}</div>
        <div class="df-meta">
          <span>ID: <code>{{ t.task_id }}</code></span>
          <span v-if="t.security_level">安全: {{ t.security_level }}</span>
          <span>{{ fmtTime(t.created_at) }}</span>
        </div>
      </article>
    </div>
  </div>
</template>

<style scoped>
.page {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.page-head h2 {
  margin: 0 0 4px;
  font-size: 20px;
  font-weight: 600;
}
.sub {
  font-size: 14.5px;
  color: var(--c-mute);
  margin: 0;
}

/* Tabs */
.tabs {
  display: flex;
  align-items: center;
  gap: 6px;
  background: var(--c-bg-soft);
  border: 1px solid var(--c-border);
  border-radius: 9px;
  padding: 4px;
}
.tab {
  background: transparent;
  border: none;
  color: var(--c-mute);
  padding: 7px 14px;
  font-size: 14px;
  font-weight: 500;
  border-radius: 6px;
  cursor: pointer;
  font-family: inherit;
}
.tab:hover { color: var(--c-fg); }
.tab.active {
  background: var(--c-accent);
  color: #fff;
}
.spacer { flex: 1; }
.search { display: flex; gap: 6px; }
.search-input {
  background: var(--c-bg-card);
  border: 1px solid var(--c-border-strong);
  color: var(--c-fg);
  padding: 6px 10px;
  border-radius: 6px;
  font-size: 13.5px;
  font-family: inherit;
  width: 200px;
}
.search-input:focus {
  outline: none;
  border-color: var(--c-accent);
}
.btn {
  background: var(--c-accent);
  border: none;
  color: #fff;
  padding: 6px 14px;
  border-radius: 6px;
  font-size: 13.5px;
  font-weight: 500;
  cursor: pointer;
  font-family: inherit;
}

/* Grid */
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
  gap: 10px;
}

/* Script card */
.script-card {
  background: var(--c-bg-card);
  border: 1px solid var(--c-border);
  border-radius: 10px;
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  transition: border-color 0.12s;
}
.script-card:hover {
  border-color: var(--c-border-strong);
}
.sc-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.sc-type {
  font-size: 14px;
  font-weight: 600;
  color: var(--c-accent);
  background: rgba(10, 132, 255, 0.12);
  padding: 2px 7px;
  border-radius: 4px;
  text-transform: uppercase;
  letter-spacing: 0.03em;
}
.sc-uses {
  font-size: 14.5px;
  color: var(--c-warn);
  font-weight: 600;
}
.sc-desc {
  font-size: 14.5px;
  line-height: 1.45;
  color: var(--c-fg);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.sc-file code {
  background: var(--c-bg-soft);
  border: 1px solid var(--c-border);
  padding: 2px 6px;
  border-radius: 4px;
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 14.5px;
  color: var(--c-mute);
}
.sc-tags {
  font-size: 14px;
  color: var(--c-mute);
  font-style: italic;
}
.sc-foot {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-top: 4px;
  padding-top: 7px;
  border-top: 1px solid var(--c-border);
}
.sc-time {
  font-size: 14px;
  color: var(--c-mute);
  font-family: ui-monospace, SFMono-Regular, monospace;
}
.sc-view {
  font-size: 14.5px;
  color: var(--c-accent);
  text-decoration: none;
}
.sc-view:hover { text-decoration: underline; }

/* Factory card */
.factory-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.df-card {
  background: var(--c-bg-card);
  border: 1px solid var(--c-border);
  border-radius: 10px;
  padding: 12px 16px;
}
.df-head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
  flex-wrap: wrap;
}
.df-type {
  font-size: 14px;
  font-weight: 600;
  padding: 2px 7px;
  background: rgba(90, 60, 255, 0.15);
  color: var(--c-accent-2);
  border-radius: 4px;
  text-transform: uppercase;
}
.df-op {
  font-size: 14px;
  padding: 2px 7px;
  background: rgba(10, 132, 255, 0.12);
  color: var(--c-accent);
  border-radius: 4px;
}
.df-status {
  font-size: 14px;
  padding: 2px 7px;
  background: rgba(48, 209, 88, 0.12);
  color: var(--c-ok);
  border-radius: 4px;
}
.df-budget {
  margin-left: auto;
  font-size: 13.5px;
  color: var(--c-warn);
  font-weight: 600;
}
.df-name {
  font-size: 13.5px;
  font-weight: 500;
  margin-bottom: 4px;
}
.df-meta {
  display: flex;
  gap: 14px;
  font-size: 14.5px;
  color: var(--c-mute);
  flex-wrap: wrap;
}
.df-meta code {
  font-family: ui-monospace, SFMono-Regular, monospace;
  color: var(--c-fg);
}

/* State messages */
.state-msg {
  text-align: center;
  padding: 40px;
  color: var(--c-mute);
  font-size: 14px;
}
.state-msg.err {
  color: var(--c-err);
}
.empty {
  grid-column: 1 / -1;
  text-align: center;
  padding: 60px 20px;
  color: var(--c-mute);
}
.empty-icon {
  font-size: 40px;
  opacity: 0.4;
  margin-bottom: 10px;
}
.empty-title {
  font-size: 14px;
  color: var(--c-fg);
  margin-bottom: 6px;
}
.empty-hint {
  font-size: 13.5px;
  line-height: 1.5;
}
.empty-hint code {
  background: var(--c-bg-soft);
  padding: 1px 5px;
  border-radius: 4px;
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 14px;
}
</style>
