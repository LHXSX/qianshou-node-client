<script setup lang="ts">
/**
 * 算力调节 · 次时代重设计 (2026-05-21)
 */
import { computed } from "vue"
import { useConnection } from "../composables/useConnection"
import Icon from "../components/Icon.vue"

const { snap, setThrottle } = useConnection()

const throttlePct = computed({
  get: () => snap.value.throttle_pct ?? 100,
  set: (v: number) => {
    setThrottle(v).catch((e) => console.error("set_throttle failed:", e))
  },
})

const throttleLabel = computed(() => {
  const p = throttlePct.value
  if (p === 0) return "已暂停"
  if (p === 100) return "全速贡献"
  return `${p}% 限速运行`
})

const accent = computed<"ok" | "warn" | "err">(() => {
  const p = throttlePct.value
  if (p === 0) return "err"
  if (p < 100) return "warn"
  return "ok"
})

const presets = [
  { val: 0,   label: "暂停" },
  { val: 25,  label: "25%" },
  { val: 50,  label: "50%" },
  { val: 75,  label: "75%" },
  { val: 100, label: "全速" },
]
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1 class="page-title">算力调节</h1>
        <p class="page-sub">控制节点参与计算的强度 · 0% 暂停 / 100% 全速</p>
      </div>
    </header>

    <!-- 主控滑块 -->
    <section :class="['throttle-panel', `ac-${accent}`]">
      <div class="tp-head">
        <span class="tp-label">{{ throttleLabel }}</span>
        <span class="tp-value mono">{{ throttlePct }}<span class="u">%</span></span>
      </div>
      <input
        type="range"
        min="0" max="100" step="5"
        class="tp-slider"
        :value="throttlePct"
        :style="{ '--pct': throttlePct + '%' }"
        @input="(e: any) => throttlePct = Number((e.target as HTMLInputElement).value)"
      />
      <div class="tp-presets">
        <button
          v-for="p in presets" :key="p.val"
          :class="['preset', { on: throttlePct === p.val }]"
          @click="throttlePct = p.val"
        >
          {{ p.label }}
        </button>
      </div>
    </section>

    <!-- 说明 -->
    <section class="panel">
      <header class="p-head"><span class="p-title">规则说明</span></header>
      <div class="p-body">
        <ul class="rules">
          <li>
            <strong>暂停 (0%)</strong>
            <span>节点对调度器显示为不可用 · 不派发新任务 · 已开始的任务跑完</span>
          </li>
          <li>
            <strong>限速 (1-99%)</strong>
            <span>正常接任务 · CPU 优先级调低 · 不影响前台应用</span>
          </li>
          <li>
            <strong>全速 (100%)</strong>
            <span>默认模式 · 无资源限制 · 按全功率参与</span>
          </li>
        </ul>
      </div>
    </section>
  </div>
</template>

<style scoped>
.page { display: flex; flex-direction: column; gap: var(--sp-6); }
.page-head { display: flex; align-items: flex-end; justify-content: space-between; }
.page-title { margin: 0; font-size: var(--fs-xl); font-weight: var(--fw-semibold); letter-spacing: -0.02em; color: var(--c-fg); }
.page-sub { margin: 2px 0 0; font-size: var(--fs-sm); color: var(--c-mute); }

/* throttle panel */
.throttle-panel {
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md);
  padding: var(--sp-7) var(--sp-7);
  display: flex; flex-direction: column; gap: var(--sp-6);
  position: relative;
  overflow: hidden;
}
.throttle-panel::before {
  content: ""; position: absolute; left: 0; top: 0; bottom: 0;
  width: 3px;
}
.throttle-panel.ac-ok::before   { background: var(--c-ok);   box-shadow: 0 0 12px rgba(63,185,80,0.5); }
.throttle-panel.ac-warn::before { background: var(--c-warn); box-shadow: 0 0 12px rgba(210,153,34,0.4); }
.throttle-panel.ac-err::before  { background: var(--c-err); }

.tp-head {
  display: flex; align-items: baseline; justify-content: space-between;
}
.tp-label {
  font-size: var(--fs-md);
  font-weight: var(--fw-semibold);
  letter-spacing: -0.01em;
}
.throttle-panel.ac-ok   .tp-label { color: var(--c-ok); }
.throttle-panel.ac-warn .tp-label { color: var(--c-warn); }
.throttle-panel.ac-err  .tp-label { color: var(--c-err); }

.tp-value {
  font-size: var(--fs-3xl);
  font-weight: var(--fw-bold);
  letter-spacing: -0.03em;
  color: var(--c-fg);
}
.tp-value .u { font-size: var(--fs-md); color: var(--c-mute); font-weight: var(--fw-medium); margin-left: 4px; }

.tp-slider {
  width: 100%;
  height: 6px;
  -webkit-appearance: none;
  background: linear-gradient(to right,
    var(--c-brand) 0%,
    var(--c-brand) var(--pct, 100%),
    var(--c-bg-soft) var(--pct, 100%),
    var(--c-bg-soft) 100%);
  border-radius: var(--r-pill);
  outline: none;
  cursor: pointer;
}
.throttle-panel.ac-warn .tp-slider {
  background: linear-gradient(to right,
    var(--c-warn) 0%,
    var(--c-warn) var(--pct, 100%),
    var(--c-bg-soft) var(--pct, 100%),
    var(--c-bg-soft) 100%);
}
.throttle-panel.ac-err .tp-slider {
  background: linear-gradient(to right,
    var(--c-err) 0%,
    var(--c-err) var(--pct, 100%),
    var(--c-bg-soft) var(--pct, 100%),
    var(--c-bg-soft) 100%);
}
.tp-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  width: 18px; height: 18px;
  background: var(--c-bg-card);
  border-radius: 50%;
  border: 2.5px solid var(--c-brand);
  cursor: grab;
  box-shadow: var(--sh-2);
}
.throttle-panel.ac-warn .tp-slider::-webkit-slider-thumb { border-color: var(--c-warn); }
.throttle-panel.ac-err  .tp-slider::-webkit-slider-thumb { border-color: var(--c-err); }

.tp-presets {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: 6px;
}
.preset {
  background: var(--c-bg-soft);
  border: 1px solid var(--c-line);
  border-radius: var(--r-sm);
  padding: 8px 0;
  font-size: var(--fs-sm);
  font-weight: var(--fw-medium);
  color: var(--c-mute);
  cursor: pointer;
  font-family: inherit;
  transition: all var(--dur-base);
}
.preset:hover { color: var(--c-fg); border-color: var(--c-line-strong); }
.preset.on {
  background: var(--c-brand);
  color: #fff;
  border-color: var(--c-brand);
}

/* panel */
.panel {
  background: var(--c-bg-card);
  border: 1px solid var(--c-line);
  border-radius: var(--r-md);
  overflow: hidden;
}
.p-head { padding: 10px var(--sp-5); border-bottom: 1px solid var(--c-line); }
.p-title {
  font-size: var(--fs-2xs); font-weight: var(--fw-semibold);
  text-transform: uppercase; letter-spacing: 0.08em; color: var(--c-fg-soft);
}
.p-body { padding: var(--sp-5); }

.rules { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--sp-4); }
.rules li {
  display: flex; flex-direction: column; gap: 2px;
  padding: var(--sp-4) var(--sp-5);
  background: var(--c-bg-soft);
  border-left: 2px solid var(--c-line-strong);
  border-radius: 0 var(--r-sm) var(--r-sm) 0;
}
.rules li:nth-child(1) { border-left-color: var(--c-err); }
.rules li:nth-child(2) { border-left-color: var(--c-warn); }
.rules li:nth-child(3) { border-left-color: var(--c-ok); }
.rules strong {
  font-size: var(--fs-sm);
  font-weight: var(--fw-semibold);
  color: var(--c-fg);
}
.rules span {
  font-size: var(--fs-xs);
  color: var(--c-mute);
  line-height: 1.5;
}
</style>
