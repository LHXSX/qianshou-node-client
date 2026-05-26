<script setup lang="ts">
/**
 * 帮助中心 · 次时代重设计 (2026-05-21)
 */
import Icon from "../components/Icon.vue"
const appVersion = __APP_VERSION__
</script>

<template>
  <div class="page">
    <header class="page-head">
      <div>
        <h1 class="page-title">帮助中心</h1>
        <p class="page-sub">使用说明 · 任务类型 · 故障排查 · 客户端 v{{ appVersion }}</p>
      </div>
    </header>

    <!-- 快速上手 -->
    <section class="panel">
      <header class="p-head">
        <span class="p-title">快速上手</span>
        <span class="p-meta">4 步开始贡献</span>
      </header>
      <div class="p-body">
        <ol class="steps">
          <li><span class="num">1</span><div><strong>登录账号</strong> · 客户端自动接入 WSS · 顶栏出现 <code class="ok">● 在线</code> 即注册成功</div></li>
          <li><span class="num">2</span><div><strong>装运行时</strong> · 工具管理页一键安装需要的 tier (lite / ocr / speech / vision)</div></li>
          <li><span class="num">3</span><div><strong>调节算力</strong> · 算力调节页拖动滑块 · 默认 100% 全速</div></li>
          <li><span class="num">4</span><div><strong>等待派单</strong> · 算力驾舱实时显示任务流 · 完成后 EDG 入账</div></li>
        </ol>
      </div>
    </section>

    <!-- 任务类型 -->
    <section class="panel">
      <header class="p-head"><span class="p-title">任务类型</span></header>
      <div class="p-body">
        <p class="muted">详细矩阵看 <strong>智能能力</strong> 页 (27 种 task_type, 8 类目)</p>
        <div class="cat-list">
          <span class="cat-pill"><Icon name="task-text" :size="13" /> 文本/编码</span>
          <span class="cat-pill"><Icon name="task-image" :size="13" /> 图像处理</span>
          <span class="cat-pill"><Icon name="task-video" :size="13" /> 音视频</span>
          <span class="cat-pill"><Icon name="task-doc" :size="13" /> 文档/OCR</span>
          <span class="cat-pill"><Icon name="task-compute" :size="13" /> 计算</span>
          <span class="cat-pill"><Icon name="task-ai" :size="13" /> AI/语音</span>
          <span class="cat-pill"><Icon name="task-render" :size="13" /> 渲染</span>
        </div>
      </div>
    </section>

    <!-- 任务 4 阶段 -->
    <section class="panel">
      <header class="p-head"><span class="p-title">任务生命周期</span></header>
      <div class="p-body">
        <div class="phases">
          <div class="phase">
            <span class="ph-dot queued" />
            <div><strong>待执行</strong><span>已 ack · 准备 spawn</span></div>
          </div>
          <span class="arr">→</span>
          <div class="phase">
            <span class="ph-dot running" />
            <div><strong>运行中</strong><span>子进程在跑</span></div>
          </div>
          <span class="arr">→</span>
          <div class="phase">
            <span class="ph-dot verifying" />
            <div><strong>验证中</strong><span>结果上报中</span></div>
          </div>
          <span class="arr">→</span>
          <div class="phase">
            <span class="ph-dot done" />
            <div><strong>已完成</strong><span>EDG 入账</span></div>
          </div>
        </div>
      </div>
    </section>

    <!-- 排查 -->
    <section class="panel">
      <header class="p-head"><span class="p-title">遇到问题</span></header>
      <div class="p-body">
        <ul class="tips">
          <li><Icon name="status-failed" :size="14" /> <span><strong>连接异常</strong> · 进 <em>系统设置</em> 看 WS URL 和错误日志</span></li>
          <li><Icon name="status-failed" :size="14" /> <span><strong>登录失败</strong> · 重置本地 token 后重新登录 (不影响余额)</span></li>
          <li><Icon name="status-failed" :size="14" /> <span><strong>任务失败</strong> · <em>任务历史</em> 点失败行展开看真实错误堆栈</span></li>
          <li><Icon name="status-failed" :size="14" /> <span><strong>缺依赖</strong> · <em>智能能力</em> 页缺依赖卡片有"装"按钮</span></li>
        </ul>
        <p class="contact">
          反馈邮箱: <a href="mailto:support@wujisuanli.com">support@wujisuanli.com</a>
        </p>
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
  font-size: var(--fs-2xs); font-weight: var(--fw-semibold);
  text-transform: uppercase; letter-spacing: 0.08em; color: var(--c-fg-soft);
}
.p-meta { font-size: var(--fs-xs); color: var(--c-mute); }
.p-body { padding: var(--sp-6); }

/* steps */
.steps { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--sp-5); }
.steps li {
  display: flex; gap: var(--sp-5); align-items: flex-start;
  font-size: var(--fs-sm); color: var(--c-fg-soft);
  line-height: 1.55;
}
.steps strong { color: var(--c-fg); font-weight: var(--fw-semibold); }
.steps code {
  font-family: ui-monospace, monospace;
  font-size: var(--fs-xs);
  padding: 1px 6px;
  background: var(--c-bg-soft);
  border-radius: var(--r-xs);
}
.steps code.ok { background: var(--c-ok-soft); color: var(--c-ok); }
.steps .num {
  flex-shrink: 0;
  width: 24px; height: 24px;
  border-radius: 50%;
  background: var(--c-brand-soft);
  color: var(--c-brand);
  display: flex; align-items: center; justify-content: center;
  font-family: ui-monospace, monospace;
  font-weight: var(--fw-bold);
  font-size: var(--fs-xs);
}

.muted { font-size: var(--fs-sm); color: var(--c-mute); margin: 0 0 var(--sp-4); }
.muted strong { color: var(--c-fg-soft); }

.cat-list { display: flex; flex-wrap: wrap; gap: 6px; }
.cat-pill {
  display: inline-flex; align-items: center; gap: 5px;
  font-size: var(--fs-sm);
  padding: 4px 10px;
  background: var(--c-bg-soft);
  border: 1px solid var(--c-line);
  border-radius: var(--r-pill);
  color: var(--c-fg-soft);
}
.cat-pill :deep(.qs-icon) { color: var(--c-brand); }

/* phases */
.phases {
  display: flex; align-items: center; gap: var(--sp-4); flex-wrap: wrap;
}
.phase {
  display: flex; align-items: center; gap: 8px;
  padding: 8px 14px;
  background: var(--c-bg-soft);
  border: 1px solid var(--c-line);
  border-radius: var(--r-sm);
  flex: 1;
  min-width: 140px;
}
.phase strong { display: block; font-size: var(--fs-sm); font-weight: var(--fw-semibold); color: var(--c-fg); }
.phase span { display: block; font-size: var(--fs-xs); color: var(--c-mute); }
.ph-dot {
  width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0;
}
.ph-dot.queued    { background: var(--c-mute); }
.ph-dot.running   { background: var(--c-info); animation: brand-pulse 1.6s ease-in-out infinite; }
.ph-dot.verifying { background: var(--c-warn); }
.ph-dot.done      { background: var(--c-ok); }
.arr { color: var(--c-faint); font-size: var(--fs-md); }

/* tips */
.tips { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
.tips li {
  display: flex; align-items: flex-start; gap: 8px;
  font-size: var(--fs-sm);
  color: var(--c-fg-soft);
  line-height: 1.55;
  padding: 8px var(--sp-5);
  background: var(--c-bg-soft);
  border-left: 2px solid var(--c-warn);
  border-radius: 0 var(--r-sm) var(--r-sm) 0;
}
.tips :deep(.qs-icon) { color: var(--c-warn); flex-shrink: 0; margin-top: 2px; }
.tips strong { color: var(--c-fg); font-weight: var(--fw-semibold); }
.tips em { color: var(--c-brand); font-style: normal; font-weight: var(--fw-medium); }

.contact { margin-top: var(--sp-5); font-size: var(--fs-sm); color: var(--c-mute); }
.contact a { color: var(--c-brand); text-decoration: none; }
.contact a:hover { text-decoration: underline; }
</style>
