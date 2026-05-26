<script setup lang="ts">
/**
 * 设备信息 · 次时代重设计 (2026-05-21)
 *
 * - 4 张 KPI (CPU/MEM/磁盘/网络) - 待 DeviceCard 提供
 * - 节点身份 panel
 * - 系统信息 panel
 */
import DeviceCard from "../components/DeviceCard.vue"
import { useConnection } from "../composables/useConnection"
import Icon from "../components/Icon.vue"

const { snap } = useConnection()
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1 class="page-title">设备信息</h1>
        <p class="page-sub">节点硬件 · 身份标识 · 实时资源</p>
      </div>
    </header>

    <DeviceCard />

    <section class="panel">
      <header class="p-head">
        <span class="p-title">节点身份</span>
        <span :class="['st-pill', snap.connection_state === 'registered' ? 'ok' : 'warn']">
          <span class="dot" />
          {{ snap.state_label }}
        </span>
      </header>
      <div class="p-body">
        <dl class="kv-grid">
          <div>
            <dt>节点 ID</dt>
            <dd class="mono">{{ snap.node_id || "—" }}</dd>
          </div>
          <div>
            <dt>归属用户</dt>
            <dd v-if="snap.user">
              <span class="ub">
                <span class="ub-av">{{ (snap.user.username[0] || "?").toUpperCase() }}</span>
                <span>{{ snap.user.username }}</span>
                <span class="ub-id mono">#{{ snap.owner_id }}</span>
              </span>
            </dd>
            <dd v-else class="mute">—</dd>
          </div>
          <div>
            <dt>客户端版本</dt>
            <dd class="mono">v{{ snap.client_version }}</dd>
          </div>
          <div>
            <dt>服务端版本</dt>
            <dd class="mono">{{ snap.server_version || "—" }}</dd>
          </div>
          <div>
            <dt>当前任务</dt>
            <dd class="mono">{{ snap.current_task_id ? `#${snap.current_task_id.slice(0,12)}` : "无" }}</dd>
          </div>
          <div>
            <dt>WSS 延迟</dt>
            <dd class="mono">{{ snap.latency_ms != null ? `${snap.latency_ms}ms` : "—" }}</dd>
          </div>
        </dl>
      </div>
    </section>
  </div>
</template>

<style scoped>
.page { display: flex; flex-direction: column; gap: var(--sp-6); }
.page-head { display: flex; align-items: flex-end; justify-content: space-between; }
.page-title { margin: 0; font-size: var(--fs-xl); font-weight: var(--fw-semibold); letter-spacing: -0.02em; color: var(--c-fg); }
.page-sub { margin: 2px 0 0; font-size: var(--fs-sm); color: var(--c-mute); }

.panel {
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md);
  overflow: hidden;
}
.p-head {
  display: flex; align-items: center; justify-content: space-between;
  padding: 10px var(--sp-5);
  border-bottom: 1px solid var(--c-line);
}
.p-title {
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--c-fg-soft);
}
.st-pill {
  display: inline-flex; align-items: center; gap: 5px;
  font-size: var(--fs-xs);
  font-weight: var(--fw-medium);
  padding: 2px 8px;
  border-radius: var(--r-pill);
}
.st-pill.ok { background: var(--c-ok-soft); color: var(--c-ok); }
.st-pill.warn { background: var(--c-warn-soft); color: var(--c-warn); }
.st-pill .dot { width: 5px; height: 5px; border-radius: 50%; background: currentColor; }
.st-pill.ok .dot { animation: ok-pulse 2.4s ease-in-out infinite; }

.p-body { padding: var(--sp-6); }

.kv-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--sp-6) var(--sp-8);
  margin: 0;
}
.kv-grid div { display: flex; flex-direction: column; gap: 4px; }
.kv-grid dt {
  font-size: var(--fs-2xs);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--c-mute);
}
.kv-grid dd {
  margin: 0;
  font-size: var(--fs-md);
  font-weight: var(--fw-medium);
  color: var(--c-fg);
  word-break: break-all;
}
.kv-grid dd.mute { color: var(--c-mute); }

.ub { display: inline-flex; align-items: center; gap: 8px; }
.ub-av {
  width: 22px; height: 22px;
  border-radius: 50%;
  background: linear-gradient(135deg, var(--c-brand), var(--c-brand-2));
  color: #fff;
  display: flex; align-items: center; justify-content: center;
  font-size: var(--fs-2xs);
  font-weight: var(--fw-semibold);
}
.ub-id { color: var(--c-mute); font-size: var(--fs-sm); }
</style>
