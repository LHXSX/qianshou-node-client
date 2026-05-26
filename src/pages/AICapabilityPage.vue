<script setup lang="ts">
/**
 * 智能能力 v2 · 任务能力矩阵 (2026-05-21)
 *
 * 数据流:
 *   GET /api/v8/runtime/task-catalog   全部 task_type 列表 (backend task_registry)
 *   GET /api/v8/runtime/manifest       tier → software 映射 (用户已装啥)
 *   client runtime/installed.json      当前节点 installed.software (本机有啥)
 *
 * 计算:
 *   - 任务 X 所需 software = spec.required_software
 *   - 当前节点 installed.software (并集所有 tier.software)
 *   - 若 required_software ⊂ installed.software → 可执行 (绿)
 *   - 若有缺 → 提示缺什么 · 找哪个 tier 包含 · 一键跳工具管理
 *   - GPU 任务 + 本机无 GPU → 灰色禁用
 *
 * 视觉:
 *   - 顶部统计条: 可执行 N/M · 缺依赖 K · 不支持 J
 *   - 类目筛选: 全部 / text / image / video / doc / compute / ai / render
 *   - 卡片网格: 每个 task 一张 · 大 SVG icon + 状态徽章 + 所需软件 chip
 *   - 缺依赖卡: 灰色叠加 + "去工具管理 →" 按钮
 */
import { computed, onMounted, ref } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { PRIMARY_DOMAIN } from "@shared"
import { useRuntime } from "../composables/useRuntime"
import { useNav } from "../composables/useNav"
import Icon from "../components/Icon.vue"
import { iconForCategory, type IconName } from "../icons/paths"

interface TaskSpec {
  task_type: string
  category: string
  description: string
  accepted_input_kinds: string[]
  default_input_kind: string
  slicer: string
  aggregator: string
  runtimes: string[]
  required_software: string[]
  min_memory_mb: number
  requires_gpu: boolean
  max_shards_limit: number
}

interface Catalog {
  total: number
  categories: string[]
  items: TaskSpec[]
}

const { installed, manifest, statusOf } = useRuntime()
const { goto } = useNav()

const catalog = ref<Catalog | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const filter = ref<string>("all")
const search = ref("")

// 2026-05-25 8.0.9 修 URL bug · PRIMARY_DOMAIN 已含 https://
// 2026-05-26 改用 invoke("api_get") · WebKit fetch 跨域被 CORS block (后端无 ACAO 头) · 走 Rust reqwest 绕开
const apiBase = PRIMARY_DOMAIN

async function loadCatalog() {
  loading.value = true
  error.value = null
  try {
    const url = `${apiBase}/api/v8/runtime/task-catalog`
    const body = await invoke<string>("api_get", { url })
    catalog.value = JSON.parse(body)
  } catch (e: any) {
    error.value = String(e?.message ?? e)
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  loadCatalog()
})

// 本机已装 software 并集 (从所有 ready tier 收集)
const installedSoftware = computed<Set<string>>(() => {
  const s = new Set<string>()
  // 内置一定有的: shell / python3 自身
  s.add("python3")
  s.add("shell")
  for (const t of Object.values(installed.value.tiers || {})) {
    if (t.ok) {
      for (const sw of t.software || []) s.add(sw)
    }
  }
  return s
})

// tier → software 映射 (用于"缺哪个软件 → 装哪个 tier")
const tierForSoftware = computed<Record<string, string>>(() => {
  const m: Record<string, string> = {}
  const tiers = manifest.value?.tiers || {}
  for (const [name, spec] of Object.entries(tiers)) {
    for (const sw of spec.software || []) {
      if (!m[sw]) m[sw] = name
    }
  }
  return m
})

interface TaskCardState {
  spec: TaskSpec
  canRun: boolean
  missing: string[]
  suggestTiers: string[]  // 装哪几个 tier 可解锁
  gpuBlocked: boolean
}

const cards = computed<TaskCardState[]>(() => {
  const items = catalog.value?.items || []
  return items.map((spec) => {
    const installedSw = installedSoftware.value
    const missing: string[] = []
    for (const sw of spec.required_software || []) {
      if (!installedSw.has(sw)) missing.push(sw)
    }
    // GPU 检测: 暂只标识 · 不强制 block (本机有 GPU 与否不在 manifest 里 · TODO)
    const gpuBlocked = false
    const suggestSet = new Set<string>()
    for (const sw of missing) {
      const t = tierForSoftware.value[sw]
      if (t) suggestSet.add(t)
    }
    return {
      spec,
      canRun: missing.length === 0 && !gpuBlocked,
      missing,
      suggestTiers: Array.from(suggestSet),
      gpuBlocked,
    }
  })
})

const filteredCards = computed<TaskCardState[]>(() => {
  let list = cards.value
  if (filter.value !== "all") {
    list = list.filter((c) => c.spec.category === filter.value)
  }
  if (search.value) {
    const q = search.value.toLowerCase()
    list = list.filter((c) =>
      c.spec.task_type.toLowerCase().includes(q) ||
      c.spec.description.toLowerCase().includes(q),
    )
  }
  // 可执行优先 · 同状态按字母
  return list.sort((a, b) => {
    if (a.canRun !== b.canRun) return a.canRun ? -1 : 1
    return a.spec.task_type.localeCompare(b.spec.task_type)
  })
})

const stats = computed(() => {
  const all = cards.value
  return {
    total: all.length,
    ready: all.filter((c) => c.canRun).length,
    missing: all.filter((c) => !c.canRun && c.missing.length > 0).length,
  }
})

function gradientForCategory(cat: string): string {
  switch (cat) {
    case "text":     return "linear-gradient(135deg, #4f8cff, #6e5cff)"
    case "encoding": return "linear-gradient(135deg, #a78bfa, #ec4899)"
    case "image":    return "linear-gradient(135deg, #14b8a6, #06b6d4)"
    case "video":    return "linear-gradient(135deg, #f59e0b, #ef4444)"
    case "doc":      return "linear-gradient(135deg, #ff8a4c, #ff5e7a)"
    case "compute":  return "linear-gradient(135deg, #64748b, #475569)"
    case "ai":       return "linear-gradient(135deg, #8b5cf6, #6366f1)"
    case "render":   return "linear-gradient(135deg, #ec4899, #f43f5e)"
    default:         return "linear-gradient(135deg, #64748b, #475569)"
  }
}

function categoryIcon(cat: string): IconName {
  return iconForCategory(cat)
}

function categoryLabel(cat: string): string {
  return ({
    text: "文本",
    encoding: "编码",
    image: "图像",
    video: "音视频",
    doc: "文档",
    compute: "计算",
    ai: "AI",
    render: "渲染",
  } as Record<string, string>)[cat] || cat
}

function jumpToToolbox() {
  goto("toolbox")
}
</script>

<template>
  <div class="cap-page">
    <!-- 页头 -->
    <header class="page-head">
      <div>
        <h1 class="page-title">智能能力</h1>
        <p class="page-sub">本节点能接的任务种类 · 缺依赖的话点 "去装"</p>
      </div>
      <div class="head-stats">
        <span class="stat ready">{{ stats.ready }} / {{ stats.total }} 可执行</span>
        <span v-if="stats.missing" class="stat missing">{{ stats.missing }} 缺依赖</span>
      </div>
    </header>

    <!-- 筛选 + 搜索条 -->
    <section class="filter-bar">
      <div class="cat-tabs">
        <button :class="['cat-tab', { on: filter === 'all' }]" @click="filter = 'all'">
          全部 <span class="seg-n mono">{{ stats.total }}</span>
        </button>
        <button
          v-for="c in catalog?.categories ?? []" :key="c"
          :class="['cat-tab', { on: filter === c }]"
          @click="filter = c"
        >
          <Icon :name="categoryIcon(c)" :size="13" />
          {{ categoryLabel(c) }}
        </button>
      </div>
      <input v-model="search" class="search-input" placeholder="搜索任务类型 / 描述…" />
    </section>

    <!-- 状态 -->
    <div v-if="loading" class="state-msg">加载任务能力矩阵…</div>
    <div v-if="error" class="state-msg err">{{ error }}</div>

    <!-- 卡片网格 -->
    <div class="card-grid" v-if="!loading && !error">
      <article
        v-for="c in filteredCards" :key="c.spec.task_type"
        :class="['cap-card', { ready: c.canRun, blocked: !c.canRun }]"
      >
        <!-- header bar (icon + cat + status) -->
        <div class="cc-bar">
          <span class="cc-icon" :style="{ color: gradientForCategory(c.spec.category).match(/#[a-f0-9]+/i)?.[0] || 'var(--c-brand)' }">
            <Icon :name="categoryIcon(c.spec.category)" :size="16" />
          </span>
          <span class="cc-cat">{{ categoryLabel(c.spec.category) }}</span>
          <span class="cc-spacer" />
          <span v-if="c.canRun" class="cc-st-pill ok">就绪</span>
          <span v-else class="cc-st-pill miss">缺 {{ c.missing.length }}</span>
        </div>

        <!-- body -->
        <div class="cc-body">
          <div class="cc-type mono">{{ c.spec.task_type }}</div>
          <div class="cc-desc">{{ c.spec.description }}</div>

          <div class="cc-meta">
            <span class="meta-chip" :title="`并行切片 · 最多 ${c.spec.max_shards_limit} 片`">
              <Icon name="status-running" :size="10" /> 并行 ×{{ c.spec.max_shards_limit }}
            </span>
            <span v-if="c.spec.requires_gpu" class="meta-chip warn">
              <Icon name="cpu" :size="10" /> GPU
            </span>
          </div>

          <div v-if="c.spec.required_software.length" class="cc-deps">
            <span
              v-for="sw in c.spec.required_software" :key="sw"
              :class="['dep-pill', installedSoftware.has(sw) ? 'has' : 'miss']"
            >
              <span class="dot" />
              {{ sw }}
            </span>
          </div>
        </div>

        <!-- 底部按钮 (只缺时显示) -->
        <button
          v-if="!c.canRun && c.suggestTiers.length"
          class="cc-install"
          @click="jumpToToolbox"
          :title="`安装 ${c.suggestTiers.join('/')} 后可执行`"
        >
          <Icon name="action-install" :size="13" />
          安装 <code>{{ c.suggestTiers.join("/") }}</code>
        </button>
      </article>
    </div>

    <div v-if="!loading && !error && filteredCards.length === 0" class="empty">
      没有匹配的任务
    </div>
  </div>
</template>

<style scoped>
.cap-page { display: flex; flex-direction: column; gap: var(--sp-6); }

/* page head */
.page-head { display: flex; align-items: flex-end; justify-content: space-between; gap: var(--sp-5); }
.page-title {
  margin: 0;
  font-size: var(--fs-xl); font-weight: var(--fw-semibold);
  letter-spacing: -0.02em; color: var(--c-fg);
}
.page-sub { margin: 2px 0 0; font-size: var(--fs-sm); color: var(--c-mute); }

.head-stats { display: flex; gap: 6px; }
.head-stats .stat {
  font-size: var(--fs-sm); font-weight: var(--fw-medium);
  padding: 4px 10px; border-radius: var(--r-pill);
  background: var(--c-bg-soft); color: var(--c-mute);
}
.head-stats .stat.ready   { background: var(--c-ok-soft);   color: var(--c-ok); }
.head-stats .stat.missing { background: var(--c-warn-soft); color: var(--c-warn); }

/* filter bar */
.filter-bar {
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md);
  padding: var(--sp-4) var(--sp-5);
  display: flex; align-items: center; gap: var(--sp-5);
  flex-wrap: wrap;
}
.cat-tabs { display: flex; gap: 4px; flex-wrap: wrap; }
.cat-tab {
  display: inline-flex; align-items: center; gap: 5px;
  padding: 5px 12px;
  background: var(--c-bg-soft);
  border: 1px solid transparent;
  color: var(--c-mute);
  border-radius: var(--r-pill);
  font-size: var(--fs-sm);
  font-weight: var(--fw-medium);
  transition: all var(--dur-base);
}
.cat-tab:hover { color: var(--c-fg-soft); border-color: var(--c-line); }
.cat-tab.on {
  background: var(--c-brand-soft);
  color: var(--c-brand);
  border-color: var(--c-brand);
}
.seg-n {
  font-size: var(--fs-2xs);
  padding: 1px 5px;
  background: var(--c-bg);
  color: var(--c-faint);
  border-radius: var(--r-xs);
}
.cat-tab.on .seg-n { background: var(--c-brand); color: #fff; }

.search-input {
  flex: 1; min-width: 200px; max-width: 280px;
  background: var(--c-bg-soft);
  border: 1px solid var(--c-line);
  border-radius: var(--r-sm);
  padding: 6px 12px;
  font-size: var(--fs-sm);
  color: var(--c-fg);
  outline: none;
  margin-left: auto;
}
.search-input:focus { border-color: var(--c-brand); }
.search-input::placeholder { color: var(--c-faint); }

/* state */
.state-msg { padding: 40px; text-align: center; color: var(--c-mute); font-size: var(--fs-sm); }
.state-msg.err { color: var(--c-err); background: var(--c-err-soft); border-radius: var(--r-md); }
.empty { padding: 60px; text-align: center; color: var(--c-mute); font-size: var(--fs-sm); }

/* card grid */
.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
  gap: var(--sp-5);
}

.cap-card {
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md);
  display: flex; flex-direction: column;
  overflow: hidden;
  transition: border-color var(--dur-base), transform var(--dur-base);
}
.cap-card:hover { border-color: var(--c-line-strong); transform: translateY(-1px); }
.cap-card.ready { border-color: rgba(63,185,80,0.25); }
.cap-card.ready:hover { box-shadow: 0 0 0 1px var(--c-ok), 0 8px 22px -8px rgba(63,185,80,0.4); }
.cap-card.blocked { opacity: 0.86; }
.cap-card.blocked:hover { box-shadow: 0 0 0 1px var(--c-warn), 0 6px 18px -8px rgba(210,153,34,0.4); opacity: 1; }

/* head bar */
.cc-bar {
  display: flex; align-items: center; gap: 6px;
  padding: 8px var(--sp-5);
  background: var(--c-bg-soft);
  border-bottom: 1px solid var(--c-line);
}
.cc-icon { display: inline-flex; }
.cc-cat {
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--c-fg-soft);
}
.cc-spacer { flex: 1; }
.cc-st-pill {
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  padding: 1px 7px;
  border-radius: var(--r-pill);
}
.cc-st-pill.ok   { background: var(--c-ok-soft);   color: var(--c-ok); }
.cc-st-pill.miss { background: var(--c-warn-soft); color: var(--c-warn); }

/* body */
.cc-body {
  flex: 1;
  padding: var(--sp-5);
  display: flex; flex-direction: column; gap: var(--sp-3);
}
.cc-type {
  font-size: var(--fs-md);
  font-weight: var(--fw-semibold);
  color: var(--c-fg);
  letter-spacing: -0.01em;
}
.cc-desc {
  font-size: var(--fs-xs);
  color: var(--c-mute);
  line-height: 1.5;
  min-height: 36px;
}

.cc-meta { display: flex; flex-wrap: wrap; gap: 4px; }
.meta-chip {
  display: inline-flex; align-items: center; gap: 3px;
  font-size: var(--fs-2xs);
  font-family: ui-monospace, monospace;
  background: var(--c-bg-soft);
  color: var(--c-mute);
  padding: 1px 7px;
  border-radius: var(--r-xs);
}
.meta-chip.warn { background: var(--c-warn-soft); color: var(--c-warn); }

.cc-deps { display: flex; flex-wrap: wrap; gap: 4px; padding-top: 4px; border-top: 1px dashed var(--c-line); }
.dep-pill {
  display: inline-flex; align-items: center; gap: 4px;
  font-size: var(--fs-2xs);
  font-family: ui-monospace, monospace;
  padding: 2px 8px;
  border-radius: var(--r-xs);
  border: 1px solid transparent;
}
.dep-pill.has  { background: var(--c-ok-soft);   color: var(--c-ok);   border-color: rgba(63,185,80,0.2); }
.dep-pill.miss { background: var(--c-warn-soft); color: var(--c-warn); border-color: rgba(210,153,34,0.2); }
.dep-pill .dot { width: 5px; height: 5px; border-radius: 50%; background: currentColor; }

/* install button */
.cc-install {
  display: flex; align-items: center; gap: 6px;
  width: 100%;
  padding: 8px var(--sp-5);
  background: transparent;
  border-top: 1px solid var(--c-line);
  color: var(--c-brand);
  font-size: var(--fs-sm);
  font-weight: var(--fw-medium);
  transition: all var(--dur-base);
}
.cc-install:hover { background: var(--c-brand-soft); }
.cc-install code {
  font-family: ui-monospace, monospace;
  font-size: var(--fs-xs);
  padding: 1px 6px;
  background: var(--c-bg-soft);
  border-radius: var(--r-xs);
  color: var(--c-fg-soft);
}
</style>
