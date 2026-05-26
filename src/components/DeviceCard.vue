<script setup lang="ts">
import { ref, computed } from "vue"
import { useDevice } from "../composables/useDevice"

const { device, rename } = useDevice()

const editing = ref(false)
const draft = ref("")
const saving = ref(false)
const err = ref<string | null>(null)

function startEdit() {
  if (!device.value) return
  draft.value = device.value.device_name
  err.value = null
  editing.value = true
}

async function save() {
  if (!draft.value.trim()) {
    err.value = "机器名不能为空"
    return
  }
  saving.value = true
  const e = await rename(draft.value.trim())
  saving.value = false
  if (e) {
    err.value = e
    return
  }
  editing.value = false
}

function cancel() {
  editing.value = false
  err.value = null
}

const memGB = computed(() => {
  if (!device.value) return 0
  return Math.round((device.value.system.total_memory_mb / 1024) * 10) / 10
})
</script>

<template>
  <section class="device-card" v-if="device">
    <div class="dc-head">
      <div class="dc-name-row">
        <span class="dc-label">本机</span>
        <template v-if="!editing">
          <span class="dc-name">{{ device.device_name }}</span>
          <button class="dc-edit" @click="startEdit" title="重命名">✎</button>
        </template>
        <template v-else>
          <input
            v-model="draft"
            class="dc-input"
            maxlength="64"
            autofocus
            @keyup.enter="save"
            @keyup.escape="cancel"
          />
          <button class="dc-btn primary" :disabled="saving" @click="save">{{ saving ? "..." : "保存" }}</button>
          <button class="dc-btn ghost" :disabled="saving" @click="cancel">取消</button>
        </template>
      </div>
    </div>
    <div class="dc-grid">
      <div class="dc-item">
        <div class="dc-k">系统</div>
        <div class="dc-v">{{ device.system.os_name }} {{ device.system.os_version }}</div>
      </div>
      <div class="dc-item">
        <div class="dc-k">CPU</div>
        <div class="dc-v">{{ device.system.cpu_brand || "未知" }}</div>
        <div class="dc-sub">{{ device.system.cpu_cores }} 核 / {{ device.system.cpu_threads }} 线程 · {{ device.system.arch }}</div>
      </div>
      <div class="dc-item">
        <div class="dc-k">内存</div>
        <div class="dc-v">{{ memGB }} GB</div>
      </div>
    </div>
    <div class="dc-err" v-if="err">⚠ {{ err }}</div>
  </section>
</template>

<style scoped>
.device-card {
  background: var(--c-bg-card);
  border: 1px solid #222;
  border-radius: 12px;
  padding: 16px 20px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.dc-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.dc-name-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
}
.dc-label {
  font-size: 14.5px;
  color: var(--c-mute);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}
.dc-name {
  font-size: 16px;
  font-weight: 600;
}
.dc-edit {
  background: transparent;
  border: 1px solid var(--c-border-strong);
  border-radius: 6px;
  padding: 2px 8px;
  color: var(--c-mute);
  font-size: 13.5px;
  cursor: pointer;
}
.dc-edit:hover {
  color: var(--c-fg);
  border-color: var(--c-mute);
}
.dc-input {
  background: var(--c-bg-soft);
  border: 1px solid var(--c-accent);
  border-radius: 6px;
  padding: 4px 8px;
  color: var(--c-fg);
  font-size: 14px;
  outline: none;
  min-width: 200px;
}
.dc-btn {
  border-radius: 6px;
  padding: 4px 10px;
  font-size: 13.5px;
  border: 1px solid var(--c-border-strong);
  cursor: pointer;
}
.dc-btn.primary {
  background: var(--c-accent);
  color: #fff;
  border-color: var(--c-accent);
}
.dc-btn.ghost {
  background: transparent;
  color: var(--c-mute);
}
.dc-btn.ghost:hover { color: var(--c-fg); }
.dc-grid {
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  gap: 14px;
}
.dc-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.dc-k {
  font-size: 14.5px;
  color: var(--c-mute);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}
.dc-v {
  font-size: 14.5px;
  color: var(--c-fg);
}
.dc-sub {
  font-size: 14.5px;
  color: var(--c-mute);
}
.dc-err {
  color: var(--c-err);
  font-size: 13.5px;
}
</style>
