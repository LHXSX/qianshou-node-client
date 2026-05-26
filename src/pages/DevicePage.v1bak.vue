<script setup lang="ts">
import DeviceCard from "../components/DeviceCard.vue"
import { useConnection } from "../composables/useConnection"

const { snap } = useConnection()
</script>

<template>
  <div class="page">
    <div class="page-head">
      <h2>设备</h2>
      <p class="sub">本机硬件信息 · 节点 ID · 机器名</p>
    </div>
    <DeviceCard />
    <section class="card">
      <h3>节点身份</h3>
      <div class="kv">
        <span>节点 ID</span>
        <code>{{ snap.node_id || "—" }}</code>
      </div>
      <div class="kv">
        <span>归属用户</span>
        <code v-if="snap.user">{{ snap.user.username }} ({{ snap.owner_id }})</code>
        <code v-else>—</code>
      </div>
      <div class="kv">
        <span>连接状态</span>
        <code>{{ snap.state_label }}</code>
      </div>
    </section>
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
  font-size: 18px;
  font-weight: 600;
}
.sub {
  font-size: 13.5px;
  color: var(--c-mute);
  margin: 0;
}
.card {
  background: var(--c-bg-card);
  border: 1px solid var(--c-border);
  border-radius: 10px;
  padding: 14px 18px;
}
.card h3 {
  margin: 0 0 10px;
  font-size: 14px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}
.kv {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 6px 0;
  font-size: 13.5px;
  border-bottom: 1px solid var(--c-bg-card);
}
.kv:last-child { border-bottom: none; }
.kv span {
  color: var(--c-mute);
}
.kv code {
  font-family: ui-monospace, SFMono-Regular, monospace;
  font-size: 14.5px;
  color: var(--c-fg);
}
</style>
