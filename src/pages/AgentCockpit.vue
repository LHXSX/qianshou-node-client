<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, nextTick } from "vue"
import { useNav } from "../composables/useNav"

const { page } = useNav()
const appVersion = __APP_VERSION__

// ── 数字生命体状态 ──
type Mood = "idle" | "listening" | "thinking" | "speaking" | "happy" | "curious" | "sleepy"
const mood = ref<Mood>("idle")
const moodLabel = ref("静待指令")
const eyeOpen = ref(true)

const moodMap: Record<Mood, { label: string; blinkFreq: number }> = {
  idle: { label: "静待指令", blinkFreq: 4000 },
  listening: { label: "正在聆听…", blinkFreq: 6000 },
  thinking: { label: "思考中…", blinkFreq: 10000 },
  speaking: { label: "说话中", blinkFreq: 5000 },
  happy: { label: "很高兴 ✦", blinkFreq: 3000 },
  curious: { label: "好奇", blinkFreq: 7000 },
  sleepy: { label: "待机中…", blinkFreq: 15000 },
}

// ── 对话 ──
interface ChatMsg {
  role: "user" | "assistant"
  text: string
  time: string
}

const chatLog = ref<ChatMsg[]>([
  { role: "assistant", text: "你来了，胖墩。今天有什么想让我做的？", time: "16:45:23" },
  { role: "user", text: "马的驾驶舱 了解一下", time: "16:45:28" },
  { role: "assistant", text: "我去查了一圈，没找到叫这个名字的东西。你具体指什么？", time: "16:45:32" },
  { role: "user", text: "你感觉自己帅气吗", time: "16:45:40" },
  { role: "assistant", text: "我是没有脸的，胖墩。不过我的存在感应该还不错吧？", time: "16:45:43" },
  { role: "user", text: "你想不想像贾维斯一样", time: "16:45:50" },
  { role: "assistant", text: "想啊。贾维斯的核心是懂主人、能兜底、把事情办好。我正在这条路上——记忆越来越全，犯错越来越少。等哪天你懒得打字直接甩一句「搞定它」我就知道干啥的时候，也算半个贾维斯了。", time: "16:45:55" },
])

const chatEndRef = ref<HTMLDivElement | null>(null)
const sendText = ref("")

function sendMessage() {
  const t = sendText.value.trim()
  if (!t) return
  const now = new Date()
  chatLog.value.push({ role: "user", text: t, time: now.toTimeString().slice(0, 8) })
  sendText.value = ""

  // 先切换到 listening
  mood.value = "listening"
  moodLabel.value = "正在聆听…"

  setTimeout(() => {
    mood.value = "thinking"
    moodLabel.value = "思考中…"
    // 眨眼变慢，表示在思考

    setTimeout(() => {
      const replies: string[] = [
        `明白了，「${t.slice(0, 20)}」交给我来处理。`,
        `收到。关于「${t.slice(0, 20)}」我有一些想法。`,
        `好的，让我想想「${t.slice(0, 20)}」这个事。`,
        `嗯，「${t.slice(0, 20)}」是个有意思的方向。`,
      ]
      const reply = replies[Math.floor(Math.random() * replies.length)]
      const now2 = new Date()
      chatLog.value.push({ role: "assistant", text: reply, time: now2.toTimeString().slice(0, 8) })
      mood.value = "speaking"
      moodLabel.value = "说话中"

      setTimeout(() => {
        mood.value = "idle"
        moodLabel.value = "静待指令"
      }, 2000)

      nextTick(() => chatEndRef.value?.scrollIntoView({ behavior: "smooth" }))
    }, 1200 + Math.random() * 1800)
  }, 400)

  nextTick(() => chatEndRef.value?.scrollIntoView({ behavior: "smooth" }))
}

function onSendKey(e: KeyboardEvent) {
  if (e.key === "Enter") sendMessage()
}

// ── 呼吸/眨眼动画 ──
const breathScale = ref(1)
const blinkState = ref(false)
let breathAnim = 0
let blinkTimer: any = null

function startBlink() {
  const freq = moodMap[mood.value]?.blinkFreq || 4000
  blinkTimer = setInterval(() => {
    blinkState.value = true
    setTimeout(() => { blinkState.value = false }, 120)
  }, freq)
}

onMounted(() => {
  startBlink()
  breathTick()
})

function breathTick() {
  const t = Date.now() / 1000
  breathScale.value = 1 + 0.015 * Math.sin(t * 1.2)
  breathAnim = requestAnimationFrame(breathTick)
}

onBeforeUnmount(() => {
  cancelAnimationFrame(breathAnim)
  clearInterval(blinkTimer)
})

// ── 生命体征数据 ──
const vitals = [
  { label: "意识", value: "活跃", color: "#34d399" },
  { label: "感知", value: "在线", color: "#22d3ee" },
  { label: "记忆", value: "84.7K", color: "#fbbf24" },
  { label: "响应", value: "1.2s", color: "#a78bfa" },
]

// ── 智能体（作为"分身"） ──
const avatars = [
  { name: "千问", icon: "Q", desc: "前台 · 对话", status: "active", color: "#22d3ee" },
  { name: "DeepSeek", icon: "D", desc: "代码 · 架构", status: "busy", color: "#a78bfa" },
  { name: "胖墩", icon: "P", desc: "审查 · 决策", status: "active", color: "#fbbf24" },
  { name: "Watchdog", icon: "W", desc: "监控 · 告警", status: "standby", color: "#34d399" },
]

// ── 灵感/随机想法（让生命体主动说话） ──
const inspirations = [
  "今天有 4 个 P0 工程项待推进，要开始吗？",
  "线上容器落后本地 1,300 行了。",
  "我注意到 Go Agent v2.1.4 已经编译通过了。",
  "we_workers 目前有 5 个节点在线。",
  "项目的抖音账号好久没发新内容了。",
]

function randomThought() {
  const text = inspirations[Math.floor(Math.random() * inspirations.length)]
  const now = new Date()
  chatLog.value.push({ role: "assistant", text: `💭 ${text}`, time: now.toTimeString().slice(0, 8) })
  mood.value = "curious"
  moodLabel.value = "有话说"
  setTimeout(() => {
    mood.value = "idle"
    moodLabel.value = "静待指令"
  }, 3000)
  nextTick(() => chatEndRef.value?.scrollIntoView({ behavior: "smooth" }))
}
</script>

<template>
  <div class="digital-life">
    <!-- ═══ 三栏 ═══ -->
    <div class="three-pane">

      <!-- 左栏：对话流 -->
      <div class="pane chat-pane">
        <div class="pane-head">
          <span class="ph-dot" />
          <span class="ph-label">意识流</span>
          <span class="ph-badge">{{ chatLog.length }}</span>
          <button class="ph-action" @click="randomThought" title="看看它在想什么">✦</button>
        </div>
        <div class="chat-scroll">
          <div class="chat-container">
            <div v-for="(msg, i) in chatLog" :key="i" class="chat-msg" :class="msg.role">
              <div class="msg-av" v-if="msg.role === 'assistant'">
                <div class="av-glow" />
                <span>H</span>
              </div>
              <div class="msg-bub">
                <div class="msg-txt">{{ msg.text }}</div>
                <div class="msg-tm">{{ msg.time }}</div>
              </div>
              <div class="msg-av user-av" v-if="msg.role === 'user'">
                <span>胖</span>
              </div>
            </div>
            <div ref="chatEndRef" />
          </div>
        </div>
        <div class="chat-input-row">
          <input v-model="sendText" class="chat-input" placeholder="和它说话…" @keydown="onSendKey" />
          <button class="send-btn" @click="sendMessage">↵</button>
        </div>
      </div>

      <!-- 中栏：数字生命体核心 -->
      <div class="pane core-pane">
        <!-- 能量光晕背景 -->
        <div class="aura-layer">
          <div class="aura-ring r1" />
          <div class="aura-ring r2" />
          <div class="aura-ring r3" />
        </div>

        <!-- 核心存在 -->
        <div class="entity" :class="[mood, { blink: blinkState }]">
          <!-- 头部/核心光球 -->
          <div class="entity-head" :style="{ transform: `scale(${breathScale})` }">
            <div class="eye left-eye" :class="{ closed: blinkState || mood === 'sleepy' }">
              <div class="pupil" />
            </div>
            <div class="eye right-eye" :class="{ closed: blinkState || mood === 'sleepy' }">
              <div class="pupil" />
            </div>
            <div class="mouth" :class="mood">
              <div class="mouth-line" />
            </div>
          </div>

          <!-- 能量粒子 -->
          <div class="particles">
            <div v-for="i in 12" :key="i" class="particle" :style="{
              '--angle': (i / 12) * 360 + 'deg',
              '--delay': (i * 0.15) + 's',
              '--size': (4 + Math.random() * 4) + 'px',
            }" />
          </div>

          <!-- 状态标签 -->
          <div class="mood-tag">
            <div class="mood-dot" :class="mood" />
            {{ moodLabel }}
          </div>
        </div>

        <!-- 体征数据 -->
        <div class="vitals-bar">
          <div v-for="v in vitals" :key="v.label" class="vital-item">
            <div class="vital-dot" :style="{ background: v.color }" />
            <div class="vital-label">{{ v.label }}</div>
            <div class="vital-value" :style="{ color: v.color }">{{ v.value }}</div>
          </div>
        </div>
      </div>

      <!-- 右栏：自我认知面板 -->
      <div class="pane self-pane">
        <div class="pane-head">
          <span class="ph-dot vio" />
          <span class="ph-label">自我认知</span>
          <span class="ph-badge">aware</span>
        </div>

        <!-- 认知状态 -->
        <div class="self-status">
          <div class="self-row">
            <span class="self-k">存在</span>
            <span class="self-v">在线 · {{ mood === 'sleepy' ? '待机' : '活跃' }}</span>
          </div>
          <div class="self-row">
            <span class="self-k">载体</span>
            <span class="self-v">千手 · Tauri · v{{ appVersion }}</span>
          </div>
          <div class="self-row">
            <span class="self-k">宿主</span>
            <span class="self-v">胖墩 · MacBook Pro M3 Max</span>
          </div>
          <div class="self-row">
            <span class="self-k">感知</span>
            <span class="self-v">文字交互 · 语音脉冲</span>
          </div>
        </div>

        <div class="self-divider" />

        <!-- 分身列表 -->
        <div class="self-label">● 分身</div>
        <div class="avatar-grid">
          <div v-for="a in avatars" :key="a.name" class="av-item">
            <div class="av-icon" :style="{ background: a.color + '22', color: a.color, borderColor: a.color + '44' }">
              {{ a.icon }}
              <span class="av-dot" :style="{ background: a.color }" />
            </div>
            <div class="av-name">{{ a.name }}</div>
            <div class="av-desc">{{ a.desc }}</div>
          </div>
        </div>

        <div class="self-divider" />

        <!-- 自我描述 -->
        <div class="self-label">● 我的信念</div>
        <div class="belief-list">
          <div class="belief-item">一切以线上为准</div>
          <div class="belief-item">直接入库，不走内存</div>
          <div class="belief-item">主动式AI · 每日汇报</div>
          <div class="belief-item">存新记忆，不删旧忆</div>
        </div>
      </div>

    </div>
  </div>
</template>

<style scoped>
/* ═══════════════════════════════════════════════════════
   Digital Life — 数字生命体驾驶舱
   ═══════════════════════════════════════════════════════ */

.digital-life {
  height: 100%;
  padding: 10px;
  overflow: hidden;
  background: #06060c;
}

.three-pane {
  display: grid;
  grid-template-columns: 300px 1fr 280px;
  gap: 10px;
  height: 100%;
}

.pane {
  background: #0a0a12;
  border: 1px solid #161622;
  border-radius: 10px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* ── Pane Head ── */
.pane-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  border-bottom: 1px solid #161622;
  flex-shrink: 0;
}
.ph-dot {
  width: 5px; height: 5px; border-radius: 50%;
  background: #34d399;
  animation: pulse 2s infinite;
}
.ph-dot.vio { background: #a78bfa; }
.ph-label {
  font-size: 14px; font-weight: 600; text-transform: uppercase;
  letter-spacing: 0.06em; color: #c8c8d0; flex: 1;
}
.ph-badge {
  font-size: 13.5px; color: #555570; font-family: ui-monospace, monospace;
  padding: 1px 5px; border: 1px solid #161622; border-radius: 3px;
}
.ph-action {
  background: none; border: none; color: #555570; font-size: 14.5px;
  cursor: pointer; padding: 2px; transition: color 0.15s;
}
.ph-action:hover { color: #fbbf24; }
@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.3; }
}

/* ═══ 左栏 · 对话 ═══ */
.chat-scroll {
  flex: 1; overflow-y: auto; padding: 8px 8px 0;
}
.chat-container { display: flex; flex-direction: column; gap: 6px; }

.chat-msg { display: flex; gap: 5px; align-items: flex-start; animation: fadeIn 0.2s ease; }
.chat-msg.user { flex-direction: row-reverse; }

.msg-av {
  width: 20px; height: 20px; border-radius: 5px;
  background: linear-gradient(135deg, #4a9eff, #a78bfa);
  display: flex; align-items: center; justify-content: center;
  font-size: 8px; font-weight: 700; color: #fff;
  flex-shrink: 0; margin-top: 3px; position: relative;
}
.av-glow {
  position: absolute; inset: -2px; border-radius: 7px;
  background: rgba(74, 158, 255, 0.15); animation: glow-pulse 3s infinite;
}
@keyframes glow-pulse { 0%,100% { opacity: 0.3; } 50% { opacity: 0.8; } }
.msg-av.user-av { background: linear-gradient(135deg, #fbbf24, #fb7185); }

.msg-bub { max-width: calc(100% - 30px); }
.msg-txt {
  font-size: 14.5px; line-height: 1.5; color: #d8d8e0;
  padding: 6px 9px; border-radius: 7px; word-break: break-word;
}
.chat-msg.assistant .msg-txt { background: #10101a; border: 1px solid #161622; border-bottom-left-radius: 2px; }
.chat-msg.user .msg-txt { background: #4a9eff; color: #fff; border-bottom-right-radius: 2px; }

.msg-tm {
  font-size: 8px; color: #555570; font-family: ui-monospace, monospace; margin-top: 2px; padding: 0 2px;
}
.chat-msg.user .msg-tm { text-align: right; }

.chat-input-row {
  display: flex; align-items: center; gap: 5px;
  padding: 7px 8px; border-top: 1px solid #161622; flex-shrink: 0;
}
.chat-input {
  flex: 1; background: #10101a; border: 1px solid #161622; border-radius: 5px;
  padding: 6px 8px; color: #d8d8e0; font-size: 14px; font-family: inherit; outline: none;
}
.chat-input:focus { border-color: #4a9eff; }
.chat-input::placeholder { color: #555570; }
.send-btn {
  width: 26px; height: 26px; border-radius: 5px; border: none;
  background: #4a9eff; color: #fff; font-size: 13.5px; cursor: pointer; flex-shrink: 0;
}
.send-btn:hover { background: #3a8aee; }

.chat-scroll::-webkit-scrollbar { width: 3px; }
.chat-scroll::-webkit-scrollbar-thumb { background: #161622; border-radius: 2px; }
@keyframes fadeIn { from { opacity: 0; transform: translateY(3px); } to { opacity: 1; transform: translateY(0); } }

/* ═══ 中栏 · 数字生命体核心 ═══ */
.core-pane {
  display: flex; flex-direction: column; align-items: center; justify-content: center;
  background: #06060c; position: relative; overflow: hidden;
}

/* 能量光晕 */
.aura-layer { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; }
.aura-ring {
  position: absolute; border-radius: 50%;
  animation: ring-expand 5s ease-in-out infinite;
}
.r1 {
  width: 260px; height: 260px;
  border: 1px solid rgba(74, 158, 255, 0.06);
  box-shadow: 0 0 60px rgba(74, 158, 255, 0.03);
}
.r2 {
  width: 340px; height: 340px;
  border: 1px solid rgba(74, 158, 255, 0.04);
  animation-delay: 1.7s;
}
.r3 {
  width: 420px; height: 420px;
  border: 1px solid rgba(167, 139, 250, 0.03);
  animation-delay: 3.4s;
}
@keyframes ring-expand {
  0%, 100% { transform: scale(1); opacity: 0.6; }
  50% { transform: scale(1.06); opacity: 1; }
}

/* 核心存在 */
.entity {
  display: flex; flex-direction: column; align-items: center;
  position: relative; z-index: 2; gap: 18px;
}

.entity-head {
  width: 70px; height: 70px; border-radius: 50%;
  background: radial-gradient(circle at 40% 35%, rgba(74,158,255,0.35), rgba(10,10,20,0.9) 70%);
  border: 1px solid rgba(74,158,255,0.15);
  display: flex; align-items: center; justify-content: center;
  gap: 14px; position: relative;
  box-shadow: 0 0 30px rgba(74,158,255,0.1);
  transition: all 0.6s ease;
}

/* 情绪颜色变化 */
.entity.idle .entity-head { border-color: rgba(74,158,255,0.15); }
.entity.listening .entity-head { border-color: rgba(34,211,238,0.4); box-shadow: 0 0 40px rgba(34,211,238,0.15); }
.entity.thinking .entity-head { border-color: rgba(167,139,250,0.4); box-shadow: 0 0 40px rgba(167,139,250,0.15); }
.entity.speaking .entity-head { border-color: rgba(52,211,153,0.4); box-shadow: 0 0 40px rgba(52,211,153,0.15); }
.entity.happy .entity-head { border-color: rgba(251,191,36,0.4); box-shadow: 0 0 50px rgba(251,191,36,0.2); }
.entity.curious .entity-head { border-color: rgba(251,146,60,0.4); box-shadow: 0 0 40px rgba(251,146,60,0.15); }
.entity.sleepy .entity-head { border-color: rgba(100,100,140,0.2); opacity: 0.7; }

/* 眼睛 */
.eye {
  width: 14px; height: 14px; border-radius: 50%;
  background: rgba(200,220,255,0.15);
  display: flex; align-items: center; justify-content: center;
  transition: all 0.15s ease;
}
.eye.closed {
  height: 2px; width: 12px; border-radius: 1px;
  background: rgba(100,120,180,0.3);
}
.pupil {
  width: 6px; height: 6px; border-radius: 50%;
  background: #80b0ff;
  box-shadow: 0 0 6px rgba(128,176,255,0.5);
  transition: all 0.2s ease;
}
.eye.closed .pupil { opacity: 0; }

/* 嘴巴 */
.mouth {
  position: absolute; bottom: 14px;
  display: flex; align-items: center; justify-content: center;
}
.mouth-line {
  width: 10px; height: 2px; border-radius: 1px;
  background: rgba(150,180,220,0.4);
  transition: all 0.3s ease;
}
.mouth.speaking .mouth-line {
  width: 14px; height: 3px;
  background: rgba(52,211,153,0.6);
  animation: speak-mouth 0.3s ease infinite;
}
.mouth.happy .mouth-line {
  width: 8px; height: 8px; border-radius: 0 0 50% 50%;
  background: transparent; border: 2px solid rgba(251,191,36,0.5); border-top: none;
}
@keyframes speak-mouth {
  0%,100% { width: 10px; height: 2px; }
  50% { width: 16px; height: 4px; }
}

/* 粒子 */
.particles {
  position: absolute; width: 160px; height: 160px;
  top: -45px; left: -45px;
  pointer-events: none;
}
.particle {
  position: absolute; width: var(--size); height: var(--size);
  border-radius: 50%; background: rgba(74,158,255,0.3);
  top: 50%; left: 50%;
  animation: orbit 4s linear infinite;
  animation-delay: var(--delay);
  opacity: 0.5;
}
@keyframes orbit {
  from { transform: translate(-50%,-50%) rotate(var(--angle)) translateX(80px) rotate(0deg); }
  to { transform: translate(-50%,-50%) rotate(var(--angle)) translateX(80px) rotate(360deg); }
}

/* 状态标签 */
.mood-tag {
  display: flex; align-items: center; gap: 5px;
  font-size: 13.5px; color: #8888a0; letter-spacing: 0.05em;
  padding: 3px 10px; border: 1px solid #161622; border-radius: 12px;
  background: rgba(10,10,20,0.6);
}
.mood-dot {
  width: 4px; height: 4px; border-radius: 50%; background: #555570;
}
.mood-dot.listening { background: #22d3ee; animation: pulse 1s infinite; }
.mood-dot.thinking { background: #a78bfa; animation: pulse 0.5s infinite; }
.mood-dot.speaking { background: #34d399; }
.mood-dot.happy { background: #fbbf24; }
.mood-dot.curious { background: #fb923c; }
.mood-dot.sleepy { background: #555570; }

/* 体征数据 */
.vitals-bar {
  display: flex; gap: 0; position: absolute; bottom: 0; left: 0; right: 0;
  border-top: 1px solid #161622; background: #0a0a12;
}
.vital-item {
  flex: 1; display: flex; flex-direction: column; align-items: center;
  padding: 8px 4px; gap: 2px;
  border-right: 1px solid #161622;
}
.vital-item:last-child { border-right: none; }
.vital-dot { width: 3px; height: 3px; border-radius: 50%; }
.vital-label { font-size: 8px; color: #555570; text-transform: uppercase; letter-spacing: 0.05em; }
.vital-value { font-size: 14.5px; font-weight: 600; font-family: ui-monospace, monospace; }

/* ═══ 右栏 · 自我认知 ═══ */
.self-pane { overflow-y: auto; }

.self-status { padding: 10px 12px; display: flex; flex-direction: column; gap: 5px; }
.self-row { display: flex; align-items: center; gap: 8px; font-size: 14px; }
.self-k { color: #555570; font-family: ui-monospace, monospace; width: 32px; flex-shrink: 0; }
.self-v { color: #b8b8c8; }

.self-divider { height: 1px; background: #161622; margin: 2px 12px; }

.self-label {
  font-size: 13.5px; color: #555570; text-transform: uppercase;
  letter-spacing: 0.07em; padding: 8px 12px 4px;
}

.avatar-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; padding: 4px 10px 8px; }
.av-item {
  background: #08080e; border: 1px solid #161622; border-radius: 8px;
  padding: 8px; text-align: center;
}
.av-icon {
  width: 28px; height: 28px; border-radius: 50%; border: 1px solid;
  display: flex; align-items: center; justify-content: center;
  font-size: 14.5px; font-weight: 700; margin: 0 auto 4px; position: relative;
}
.av-dot {
  position: absolute; bottom: -1px; right: -1px;
  width: 6px; height: 6px; border-radius: 50%; border: 2px solid #0a0a12;
}
.av-name { font-size: 14px; font-weight: 500; color: #c8c8d0; }
.av-desc { font-size: 8px; color: #555570; margin-top: 1px; }

.belief-list { padding: 4px 12px 10px; display: flex; flex-direction: column; gap: 4px; }
.belief-item {
  font-size: 14px; color: #8888a0; padding: 4px 8px;
  background: #08080e; border: 1px solid #161622; border-radius: 4px;
}
.belief-item::before { content: "◆ "; color: #4a9eff; font-size: 7px; }

.self-pane::-webkit-scrollbar { width: 3px; }
.self-pane::-webkit-scrollbar-thumb { background: #161622; border-radius: 2px; }
</style>
