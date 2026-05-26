<script setup lang="ts">
import { computed, onMounted, onUnmounted } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { ref } from "vue"
import { useNav } from "../composables/useNav"

const { goto } = useNav()

interface SkillInfo {
  id: string
  name: string
  version: string
  tools_count: number
  verified: boolean
}
interface SkillSnapshot {
  skills_count: number
  skills: SkillInfo[]
}

const snapshot = ref<SkillSnapshot | null>(null)
const loaded = ref(false)
let unlistenSkills: UnlistenFn | null = null

async function refresh() {
  try {
    snapshot.value = await invoke<SkillSnapshot>("list_installed_skills")
  } catch {
    snapshot.value = { skills_count: 0, skills: [] }
  }
  loaded.value = true
}

onMounted(async () => {
  await refresh()
  unlistenSkills = await listen("skills_updated", () => refresh())
})

onUnmounted(() => {
  if (unlistenSkills) unlistenSkills()
})

const skillCount = computed(() => snapshot.value?.skills_count ?? 0)
const skills = computed(() => snapshot.value?.skills ?? [])

const statusLabel = computed(() => {
  if (!loaded.value) return "探测中..."
  return skillCount.value > 0 ? `${skillCount.value} 个技能集就绪` : "暂无技能集"
})

const statusColor = computed(() => {
  if (!loaded.value) return "var(--c-mute)"
  return skillCount.value > 0 ? "var(--c-ok)" : "var(--c-mute)"
})

const pulseClass = computed(() => {
  if (!loaded.value) return "pulse-off"
  return skillCount.value > 0 ? "pulse-ok" : "pulse-off"
})
</script>

<template>
  <div class="ai-card">
    <div class="card-head">
      <div class="head-left">
        <span class="head-icon">⚡</span>
        <div>
          <h3>算力技能集</h3>
          <p class="head-sub">零模型纯执行器 · V4 架构</p>
        </div>
      </div>
      <div class="head-status">
        <span class="status-dot" :class="pulseClass" />
        <span class="status-text" :style="{ color: statusColor }">{{ statusLabel }}</span>
      </div>
    </div>

    <div class="dep-rows" v-if="skills.length > 0">
      <div
        v-for="skill in skills"
        :key="skill.id"
        class="dep-row ok"
      >
        <span class="dep-dot">✓</span>
        <span class="dep-label">{{ skill.name || skill.id }}</span>
        <span class="dep-ver">v{{ skill.version }} · {{ skill.tools_count }} 工具</span>
      </div>
    </div>
    <div class="dep-rows" v-else-if="loaded">
      <div class="dep-row missing">
        <span class="dep-dot">○</span>
        <span class="dep-label">暂无技能集</span>
        <span class="dep-hint">前往工具箱安装</span>
      </div>
    </div>
    <div class="dep-rows" v-else>
      <div class="dep-row">
        <span class="dep-label" style="color:var(--c-mute)">探测中...</span>
      </div>
    </div>

    <div class="card-foot">
      <div class="foot-info">
        <span class="foot-chip">skill_exec</span>
        <span class="foot-chip">tool_call</span>
        <span class="foot-chip">分布式调度</span>
      </div>
      <button class="btn-ok" @click="goto('toolbox')">
        管理技能集 →
      </button>
    </div>
  </div>
</template>

<style scoped>
.ai-card {
  background: var(--c-bg-card);
  border: 1px solid var(--c-border);
  border-radius: var(--r-md);
  padding: 16px 18px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.ai-card.loading {
  opacity: 0.6;
}

.card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.head-left {
  display: flex;
  align-items: center;
  gap: 10px;
}

.head-icon {
  font-size: 24px;
}

.head-left h3 {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
  color: var(--c-fg);
}

.head-sub {
  margin: 2px 0 0;
  font-size: 12px;
  color: var(--c-mute);
}

.head-status {
  display: flex;
  align-items: center;
  gap: 6px;
}

.status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--c-mute);
}

.pulse-ok {
  background: var(--c-ok);
  box-shadow: 0 0 6px var(--c-ok);
  animation: pulse-glow 2s ease-in-out infinite;
}

.pulse-warn {
  background: var(--c-warn);
  box-shadow: 0 0 6px var(--c-warn);
  animation: pulse-glow 2s ease-in-out infinite;
}

.pulse-off {
  background: var(--c-mute);
}

@keyframes pulse-glow {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}

.status-text {
  font-size: 13px;
  font-weight: 600;
}

.dep-rows {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.dep-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  border-radius: 6px;
  font-size: 13px;
  background: var(--c-bg-soft);
}

.dep-row.ok {
  background: rgba(52, 211, 153, 0.08);
}

.dep-row.missing {
  background: rgba(251, 191, 36, 0.06);
}

.dep-dot {
  font-size: 12px;
  font-weight: 700;
  width: 16px;
  text-align: center;
}

.dep-row.ok .dep-dot { color: var(--c-ok); }
.dep-row.missing .dep-dot { color: var(--c-warn); }

.dep-label {
  flex: 1;
  font-weight: 500;
  color: var(--c-fg);
}

.dep-ver {
  font-size: 11px;
  color: var(--c-mute);
  font-family: ui-monospace, SFMono-Regular, monospace;
}

.dep-hint {
  font-size: 11px;
  color: var(--c-warn);
}

.card-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.foot-info {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}

.foot-chip {
  font-size: 10.5px;
  padding: 2px 7px;
  border-radius: 4px;
  background: rgba(99, 102, 241, 0.1);
  color: #818cf8;
  font-family: ui-monospace, SFMono-Regular, monospace;
}

.btn-install {
  background: var(--c-warn);
  color: #000;
  border: none;
  padding: 6px 14px;
  border-radius: 6px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  white-space: nowrap;
  transition: opacity 0.15s;
}

.btn-install:hover { opacity: 0.85; }

.btn-ok {
  background: transparent;
  color: var(--c-ok);
  border: 1px solid var(--c-ok);
  padding: 5px 12px;
  border-radius: 6px;
  font-size: 13px;
  cursor: pointer;
  white-space: nowrap;
  transition: all 0.15s;
}

.btn-ok:hover {
  background: rgba(52, 211, 153, 0.1);
}

.loading-text {
  font-size: 12px;
  color: var(--c-mute);
  margin-left: auto;
}
</style>