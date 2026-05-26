<script setup lang="ts">
/**
 * 运行环境 · 卡片网格 (2026-05-21 v2)
 *
 * 设计:
 *   - 每个 tier 一张大卡片 · 未装灰色 / 装中脉冲 / 装完绿光点亮
 *   - 卡片上半部 SVG 大图标 (脉冲) + 状态徽章
 *   - 中间任务能力 chips · 下面依赖包 chips
 *   - 底部一键安装按钮
 *   - 安装日志 hover 在卡内 (可折叠)
 */
import { computed } from "vue"
import { useRuntime, type RuntimeTierSpec } from "../composables/useRuntime"
import Icon from "./Icon.vue"
import type { IconName } from "../icons/paths"

const {
  manifest, installed, hostPython, loading, error, stats,
  refreshManifest, refreshInstalled, refreshHostPython,
  installTier, uninstallTier, recheckTier, statusOf, logsForTier, tierJob,
} = useRuntime()

import { ref as vueRef } from "vue"
// 打开 "手工安装" 面板的 tier (空 = 不显示)
const manualOpen = vueRef<string | null>(null)
const rechecking = vueRef<Record<string, boolean>>({})
const recheckMsg = vueRef<Record<string, string>>({})

/**
 * 错误分类 · 从日志粗判原因
 */
function errorCategory(tier: string): { kind: string; advice: string } {
  const log = logsForTier(tier)
  if (!log || log.ok !== false) return { kind: "", advice: "" }
  const lines = log.lines.map((l) => l.line.toLowerCase()).join("\n")
  if (/timeout|timed out|connection reset|connection refused/.test(lines)) {
    return { kind: "网络超时", advice: "试试切个镜像 · 或检查代理 / 防火墙" }
  }
  if (/ssl|certificate|tls/.test(lines)) {
    return { kind: "SSL/证书错", advice: "提示使用 trusted-host 跳主机验证" }
  }
  if (/no matching distribution|could not find/.test(lines)) {
    return { kind: "包未找到", advice: "该包可能未发布到镜像 · 试官方 PyPI" }
  }
  if (/python.* not found|no python|missing python/i.test(lines)) {
    return { kind: "本机缺 Python", advice: "uv 会自动下载 · 请检查网络后重试" }
  }
  if (/building wheel|failed to build|cl\.exe|gcc/.test(lines)) {
    return { kind: "需本地编译", advice: "该包无 wheel · 需装 Xcode / build-tools / cmake 后重试" }
  }
  return { kind: "未知错误", advice: "按镜像复制命令手动安装 · 装完点“重检”" }
}

/**
 * 生成该 tier × 镜像的手工 uv pip install 命令 · 可复制贴终端
 */
function manualCmd(tier: string, mirror: { label: string; index_url: string; trusted_host?: string }): string {
  const spec = manifest.value?.tiers?.[tier]
  if (!spec) return ""
  const pkgs = spec.packages.join(" ")
  const hostFlag = mirror.trusted_host ? `--trusted-host ${mirror.trusted_host} ` : ""
  return `uv pip install --python ~/.qianshou/runtime/venvs/${tier}/bin/python \\
  --index-url ${mirror.index_url} \\
  ${hostFlag}${pkgs}`
}

async function runRecheck(tier: string) {
  rechecking.value[tier] = true
  try {
    const r = await recheckTier(tier)
    recheckMsg.value[tier] = r.ok
      ? "✓ 检测通过 · tier 已激活"
      : `✗ 未检测到可用物件: ${r.last_message || "空"}`
    if (r.ok) await refreshInstalled()
  } catch (e: any) {
    recheckMsg.value[tier] = `重检出错: ${e?.message || e}`
  } finally {
    rechecking.value[tier] = false
  }
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text)
  } catch {
    // 在 Tauri 中 clipboard 可能需要 permission · 静默失败
  }
}

interface TierRow {
  key: string
  spec: RuntimeTierSpec
}

const tierRows = computed<TierRow[]>(() => {
  const t = manifest.value?.tiers ?? {}
  return Object.keys(t).map((k) => ({ key: k, spec: t[k] as RuntimeTierSpec }))
})

const tierMeta: Record<string, { label: string; icon: IconName; gradient: string }> = {
  lite:        { label: "Lite · 轻量包",     icon: "task-text",  gradient: "linear-gradient(135deg, #4f8cff, #6e5cff)" },
  ocr:         { label: "OCR · 文字识别",    icon: "task-doc",   gradient: "linear-gradient(135deg, #ff8a4c, #ff5e7a)" },
  speech:      { label: "Speech · 语音转写", icon: "task-video", gradient: "linear-gradient(135deg, #a78bfa, #ec4899)" },
  "vision-ai": { label: "Vision · 图片理解", icon: "task-image", gradient: "linear-gradient(135deg, #14b8a6, #06b6d4)" },
}

function tierMetaOf(key: string) {
  return tierMeta[key] || { label: key, icon: "task-compute" as IconName, gradient: "linear-gradient(135deg, #64748b, #475569)" }
}

function statusLabel(tier: string): string {
  switch (statusOf(tier)) {
    case "ready": return "已就绪"
    case "installing": return "安装中…"
    case "failed": return "失败"
    default: return "未安装"
  }
}

async function refreshAll() {
  await Promise.all([refreshManifest(), refreshInstalled(), refreshHostPython()])
}
</script>

<template>
  <div class="runtime-panel">
    <!-- 顶部信息条 -->
    <header class="rp-head">
      <div class="rp-title-row">
        <h3>运行环境</h3>
        <span class="rp-progress">
          <span class="rp-progress-bar">
            <span class="rp-progress-fill"
              :style="{ width: stats.total ? (stats.ready / stats.total * 100) + '%' : '0%' }" />
          </span>
          <span class="rp-progress-text">{{ stats.ready }}/{{ stats.total }} 就绪</span>
        </span>
        <button class="btn-ghost" @click="refreshAll" :disabled="loading" title="刷新">
          <Icon name="action-refresh" :size="14" :class="{ spin: loading }" />
        </button>
      </div>
      <p class="rp-sub">
        公共镜像源 · venv 隔离 · 后端动态下发 · 装完自动点亮
        <span v-if="hostPython?.ok" class="rp-py">
          · host python <code>{{ hostPython.version }}</code>
        </span>
        <span v-else class="rp-py warn">· {{ hostPython?.message || "未探测到 Python" }}</span>
      </p>
    </header>

    <div v-if="error" class="err-block">⚠ {{ error }}</div>

    <!-- 卡片网格 -->
    <div class="tier-grid">
      <article
        v-for="row in tierRows" :key="row.key"
        :class="['tier-card', `st-${statusOf(row.key)}`]"
      >
        <!-- 卡片光晕 + 大图标 -->
        <div class="tc-hero" :style="{ background: tierMetaOf(row.key).gradient }">
          <Icon :name="tierMetaOf(row.key).icon" :size="42" :stroke-width="1.4" />
          <div class="tc-status">
            <span v-if="statusOf(row.key) === 'ready'" class="tc-check">
              <Icon name="status-done" :size="14" :stroke-width="2.5" />
            </span>
            <span v-else-if="statusOf(row.key) === 'installing'" class="tc-spinner">
              <Icon name="action-refresh" :size="14" :stroke-width="2.4" />
            </span>
            <span v-else-if="statusOf(row.key) === 'failed'" class="tc-fail">
              <Icon name="status-failed" :size="14" :stroke-width="2.4" />
            </span>
          </div>
        </div>

        <!-- 卡片正文 -->
        <div class="tc-body">
          <div class="tc-title">
            {{ tierMetaOf(row.key).label }}
            <span v-if="row.spec.required" class="tc-req">必装</span>
          </div>
          <div class="tc-desc">{{ row.spec.description }}</div>

          <div class="tc-tasks">
            <span class="tc-tasks-label">支持任务</span>
            <span v-for="tt in row.spec.task_types.slice(0, 4)" :key="tt" class="tc-tt">{{ tt }}</span>
            <span v-if="row.spec.task_types.length > 4" class="tc-tt more">+{{ row.spec.task_types.length - 4 }}</span>
          </div>

          <div class="tc-pkgs">
            <span v-for="p in row.spec.packages" :key="p" class="tc-pkg">{{ p }}</span>
          </div>

          <div class="tc-state">
            <span class="tc-pill" :class="`st-${statusOf(row.key)}`">{{ statusLabel(row.key) }}</span>
            <span v-if="installed.tiers[row.key]?.mirror_label" class="tc-mirror">
              · {{ installed.tiers[row.key].mirror_label }}
            </span>
          </div>
        </div>

        <!-- 卡片底部按钮 -->
        <div class="tc-actions">
          <button
            v-if="statusOf(row.key) !== 'ready'"
            class="tc-btn primary"
            :disabled="!!tierJob[row.key]"
            @click="installTier(row.key)"
          >
            <Icon name="action-install" :size="14" />
            {{ tierJob[row.key] ? "安装中…" : "一键安装" }}
          </button>
          <button v-else class="tc-btn ghost" @click="uninstallTier(row.key)">
            <Icon name="action-trash" :size="14" />
            移除
          </button>
        </div>

        <!-- 安装日志 -->
        <div v-if="logsForTier(row.key)" class="tc-log">
          <div class="tc-log-head">
            <span>实时日志</span>
            <span :class="{ ok: logsForTier(row.key)!.ok, fail: logsForTier(row.key)!.ok === false && !logsForTier(row.key)!.running, running: logsForTier(row.key)!.running }">
              {{ logsForTier(row.key)!.running ? "运行中…" : (logsForTier(row.key)!.ok ? "✓ 完成" : "✗ 失败") }}
            </span>
          </div>
          <div class="tc-log-body">
            <div v-for="(l, i) in logsForTier(row.key)!.lines.slice(-80)" :key="i" :class="{ err: l.err }">{{ l.line }}</div>
          </div>
        </div>

        <!-- 2026-05-26 · 安装失败 → 手工安装引导 · 仿 Ollama/Steam 错误产材 -->
        <div
          v-if="logsForTier(row.key) && logsForTier(row.key)!.ok === false && !logsForTier(row.key)!.running"
          class="tc-fallback"
        >
          <div class="fb-head">
            <Icon name="status-failed" :size="13" />
            <span class="fb-kind">{{ errorCategory(row.key).kind }}</span>
            <span class="fb-advice">{{ errorCategory(row.key).advice }}</span>
            <button class="fb-toggle" @click="manualOpen = manualOpen === row.key ? null : row.key">
              {{ manualOpen === row.key ? "收起" : "手工安装…" }}
            </button>
          </div>
          <div v-if="manualOpen === row.key" class="fb-body">
            <p class="fb-tip">
              自动装失败了·可以抷下面某个镜像的命令贴到终端·装完点
              <strong>"重新检测"</strong>让客户端接手。
            </p>
            <div
              v-for="(mir, mi) in manifest?.mirrors ?? []" :key="mir.index_url"
              class="fb-mirror"
            >
              <div class="fb-mirror-head">
                <span class="fb-mirror-label">{{ mi + 1 }}· {{ mir.label }}</span>
                <button class="fb-copy" @click="copyText(manualCmd(row.key, mir))">复制</button>
              </div>
              <pre class="fb-cmd">{{ manualCmd(row.key, mir) }}</pre>
            </div>
            <div class="fb-actions">
              <button
                class="tc-btn primary"
                :disabled="rechecking[row.key]"
                @click="runRecheck(row.key)"
              >
                <Icon name="action-refresh" :size="13" />
                {{ rechecking[row.key] ? "检测中…" : "我装好了 · 重新检测" }}
              </button>
              <span v-if="recheckMsg[row.key]" class="fb-recheck-msg">{{ recheckMsg[row.key] }}</span>
            </div>
          </div>
        </div>
      </article>
    </div>
  </div>
</template>

<style scoped>
.runtime-panel { display: flex; flex-direction: column; gap: var(--sp-6); }

/* ── head bar ── */
.rp-head {
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md);
  padding: var(--sp-5) var(--sp-6);
  display: flex; flex-direction: column; gap: 6px;
}
.rp-title-row {
  display: flex; align-items: center; gap: var(--sp-5);
}
.rp-title-row h3 {
  margin: 0;
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--c-mute);
}
.rp-progress {
  display: inline-flex; align-items: center; gap: var(--sp-4);
  flex: 1;
}
.rp-progress-bar {
  flex: 1;
  height: 6px;
  background: var(--c-bg-soft);
  border-radius: var(--r-pill);
  overflow: hidden;
  max-width: 320px;
}
.rp-progress-fill {
  display: block;
  height: 100%;
  background: linear-gradient(90deg, var(--c-brand), var(--c-info));
  border-radius: var(--r-pill);
  transition: width var(--dur-slow) var(--ease-out);
}
.rp-progress-text {
  font-size: var(--fs-sm);
  font-weight: var(--fw-semibold);
  color: var(--c-fg);
  font-family: ui-monospace, monospace;
}

.btn-ghost {
  width: 28px; height: 28px;
  display: flex; align-items: center; justify-content: center;
  background: var(--c-bg-soft);
  border: 1px solid var(--c-line);
  border-radius: var(--r-sm);
  color: var(--c-mute);
  transition: all var(--dur-base);
}
.btn-ghost:hover { color: var(--c-fg); border-color: var(--c-line-strong); }
.btn-ghost:disabled { opacity: 0.5; cursor: not-allowed; }

.rp-sub {
  margin: 0;
  font-size: var(--fs-xs);
  color: var(--c-mute);
}
.rp-py { font-family: inherit; }
.rp-py code {
  font-family: ui-monospace, monospace;
  padding: 1px 6px;
  background: var(--c-bg-soft);
  border-radius: var(--r-xs);
  color: var(--c-fg-soft);
}
.rp-py.warn { color: var(--c-warn); }

.err-block {
  padding: var(--sp-4) var(--sp-5);
  background: var(--c-err-soft);
  border: 1px solid var(--c-err);
  border-radius: var(--r-sm);
  color: var(--c-err);
  font-size: var(--fs-sm);
}

/* ── tier grid (紧凑) ── */
.tier-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: var(--sp-5);
}

.tier-card {
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md);
  overflow: hidden;
  display: flex; flex-direction: column;
  transition: border-color var(--dur-base);
}
.tier-card:hover { border-color: var(--c-line-strong); }
.tier-card.st-ready { border-color: rgba(63,185,80,0.3); }
.tier-card.st-ready:hover { box-shadow: 0 0 0 1px var(--c-ok), 0 8px 22px -8px rgba(63,185,80,0.4); }
.tier-card.st-installing { border-color: rgba(88,166,255,0.4); animation: brand-pulse 1.6s ease-in-out infinite; }
.tier-card.st-failed { border-color: var(--c-err); }

/* hero · 紧凑 64px */
.tc-hero {
  position: relative;
  height: 64px;
  display: flex; align-items: center; justify-content: center;
  color: rgba(255,255,255,0.94);
}
.tier-card.st-missing .tc-hero { filter: saturate(0.5) brightness(0.7); }

.tc-status {
  position: absolute;
  top: 8px; right: 10px;
}
.tc-check, .tc-spinner, .tc-fail {
  width: 22px; height: 22px;
  border-radius: 50%;
  display: flex; align-items: center; justify-content: center;
  color: #fff;
  backdrop-filter: blur(6px);
}
.tc-check   { background: var(--c-ok);   box-shadow: 0 0 10px rgba(63,185,80,0.6); }
.tc-spinner { background: var(--c-info); animation: spin 1s linear infinite; }
.tc-fail    { background: var(--c-err); }

/* body */
.tc-body { padding: var(--sp-5); flex: 1; display: flex; flex-direction: column; gap: var(--sp-3); }
.tc-title {
  display: flex; align-items: center; gap: 6px;
  font-size: var(--fs-md);
  font-weight: var(--fw-semibold);
  color: var(--c-fg);
  letter-spacing: -0.01em;
}
.tc-req {
  font-size: var(--fs-2xs);
  padding: 1px 6px;
  background: var(--c-brand);
  color: #fff;
  border-radius: var(--r-xs);
  font-weight: var(--fw-bold);
}
.tc-desc {
  font-size: var(--fs-xs);
  color: var(--c-mute);
  line-height: 1.5;
  min-height: 36px;
}

.tc-tasks {
  display: flex; align-items: center; flex-wrap: wrap; gap: 4px;
  padding-top: var(--sp-2);
  border-top: 1px dashed var(--c-line);
}
.tc-tasks-label {
  font-size: var(--fs-2xs);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--c-mute);
  margin-right: 4px;
}
.tc-tt {
  font-size: var(--fs-2xs);
  font-family: ui-monospace, monospace;
  padding: 1px 6px;
  background: var(--c-bg-soft);
  border-radius: var(--r-xs);
  color: var(--c-fg-soft);
}
.tc-tt.more { color: var(--c-mute); }

.tc-pkgs { display: flex; flex-wrap: wrap; gap: 3px; }
.tc-pkg {
  font-size: var(--fs-2xs);
  font-family: ui-monospace, monospace;
  padding: 1px 6px;
  background: var(--c-brand-soft);
  color: var(--c-brand);
  border-radius: var(--r-xs);
}

.tc-state {
  display: flex; align-items: center; gap: 6px;
  font-size: var(--fs-xs);
  color: var(--c-mute);
  margin-top: 2px;
}
.tc-pill {
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  padding: 2px 8px;
  border-radius: var(--r-pill);
  background: var(--c-bg-soft);
  color: var(--c-mute);
}
.tc-pill.st-ready      { background: var(--c-ok-soft);   color: var(--c-ok); }
.tc-pill.st-installing { background: var(--c-info-soft); color: var(--c-info); }
.tc-pill.st-failed     { background: var(--c-err-soft);  color: var(--c-err); }
.tc-mirror { font-family: ui-monospace, monospace; }

/* actions */
.tc-actions {
  padding: 0 var(--sp-5) var(--sp-5);
  display: flex; gap: var(--sp-3);
}
.tc-btn {
  display: inline-flex; align-items: center; gap: 5px;
  padding: 6px 14px;
  border-radius: var(--r-sm);
  font-size: var(--fs-sm);
  font-weight: var(--fw-medium);
  transition: all var(--dur-base);
  width: 100%;
  justify-content: center;
}
.tc-btn.primary {
  background: var(--c-brand);
  color: #fff;
}
.tc-btn.primary:hover:not(:disabled) { background: var(--c-brand-2); }
.tc-btn.primary:disabled { opacity: 0.6; cursor: not-allowed; }
.tc-btn.ghost {
  background: transparent;
  border: 1px solid var(--c-line-strong);
  color: var(--c-mute);
}
.tc-btn.ghost:hover { color: var(--c-err); border-color: var(--c-err); }

/* log */
.tc-log {
  margin: 0 var(--sp-5) var(--sp-5);
  background: var(--c-bg);
  border: 1px solid var(--c-line);
  border-radius: var(--r-sm);
  overflow: hidden;
}
.tc-log-head {
  display: flex; align-items: center; justify-content: space-between;
  padding: 4px 10px;
  background: var(--c-bg-soft);
  border-bottom: 1px solid var(--c-line);
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--c-mute);
}
.tc-log-head .ok { color: var(--c-ok); }
.tc-log-head .fail { color: var(--c-err); }
.tc-log-head .running { color: var(--c-info); }
.tc-log-body {
  padding: 6px 10px;
  max-height: 160px;
  overflow: auto;
  font-family: ui-monospace, monospace;
  font-size: var(--fs-2xs);
  line-height: 1.4;
  color: var(--c-fg-soft);
}
.tc-log-body div { white-space: pre-wrap; word-break: break-all; }
.tc-log-body .err { color: var(--c-warn); }

/* 2026-05-26 · 安装失败 → 手工安装引导面板 */
.tc-fallback {
  margin: 0 var(--sp-5) var(--sp-5);
  background: var(--c-warn-soft);
  border: 1px solid var(--c-warn);
  border-radius: var(--r-sm);
  overflow: hidden;
}
.fb-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px var(--sp-4);
  font-size: var(--fs-xs);
}
.fb-kind {
  font-weight: var(--fw-semibold);
  color: var(--c-warn);
}
.fb-advice {
  flex: 1;
  color: var(--c-fg-soft);
}
.fb-toggle {
  background: transparent;
  border: 1px solid var(--c-warn);
  color: var(--c-warn);
  padding: 3px 10px;
  border-radius: var(--r-pill);
  font-size: var(--fs-2xs);
  font-weight: var(--fw-medium);
  cursor: pointer;
}
.fb-toggle:hover { background: var(--c-warn); color: #fff; }

.fb-body {
  padding: 0 var(--sp-4) var(--sp-4);
  border-top: 1px dashed var(--c-warn);
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
}
.fb-tip {
  margin: var(--sp-3) 0 0;
  font-size: var(--fs-xs);
  color: var(--c-fg-soft);
  line-height: 1.5;
}
.fb-mirror {
  background: var(--c-bg);
  border: 1px solid var(--c-line);
  border-radius: var(--r-xs);
  overflow: hidden;
}
.fb-mirror-head {
  display: flex; align-items: center; justify-content: space-between;
  padding: 4px 8px;
  background: var(--c-bg-soft);
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  color: var(--c-mute);
}
.fb-mirror-label { letter-spacing: 0.04em; }
.fb-copy {
  background: var(--c-bg);
  border: 1px solid var(--c-line);
  color: var(--c-fg-soft);
  font-size: var(--fs-2xs);
  padding: 2px 8px;
  border-radius: var(--r-xs);
  cursor: pointer;
}
.fb-copy:hover { color: var(--c-brand); border-color: var(--c-brand); }
.fb-cmd {
  margin: 0;
  padding: 6px 10px;
  font-family: ui-monospace, monospace;
  font-size: var(--fs-2xs);
  line-height: 1.55;
  color: var(--c-fg);
  white-space: pre-wrap;
  word-break: break-all;
}
.fb-actions {
  display: flex; align-items: center; gap: var(--sp-3);
  margin-top: var(--sp-2);
}
.fb-actions .tc-btn {
  width: auto;
  flex: 0 0 auto;
}
.fb-recheck-msg {
  font-size: var(--fs-xs);
  color: var(--c-fg-soft);
}

.spin { animation: spin 1s linear infinite; }
</style>
