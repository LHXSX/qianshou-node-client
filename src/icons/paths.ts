/**
 * SVG icon paths · 一处定义 · 全 app 共用
 *
 * 设计:
 *   - viewBox 24×24 · stroke="currentColor" stroke-width=1.6 · linecap=round
 *   - 通过 currentColor 自动跟随上下文颜色 (text/icon color)
 *   - 用 lucide / heroicons 风格 · 细线条专业感
 *
 * 命名:
 *   nav-*       侧边栏导航
 *   task-*      任务类目 (跟 backend task_registry.category 对齐)
 *   status-*    任务/连接状态
 *   action-*    按钮里的小图标
 *   tier-*      runtime tier 图标
 */

export type IconName =
  // ── 导航 (跟 sidebar items 对齐) ──
  | "nav-dashboard"    // 算力驾舱: 仪表盘
  | "nav-market"       // 任务市场: 商店
  | "nav-history"      // 任务历史: 时钟
  | "nav-earnings"     // 收益统计: 趋势图
  | "nav-throttle"     // 算力调节: 滑块
  | "nav-device"       // 设备信息: 电脑芯片
  | "nav-toolbox"      // 工具管理: 工具箱
  | "nav-ai"           // 智能能力: 火花
  | "nav-settings"     // 系统设置: 齿轮
  | "nav-help"         // 帮助中心: 问号圆圈
  // ── 任务类目 (跟 backend category 对齐) ──
  | "task-text"        // 文本: 字
  | "task-encoding"    // 编码: 钥匙
  | "task-image"       // 图像: 山+太阳
  | "task-video"       // 视频: 播放
  | "task-doc"         // 文档: 文件
  | "task-compute"     // 计算: ∑
  | "task-ai"          // AI: 神经元
  | "task-render"      // 渲染: 立方体
  // ── 状态 ──
  | "status-queued"    // 待处理: 时钟
  | "status-running"   // 运行: 闪电
  | "status-verifying" // 验证: 放大镜
  | "status-done"      // 完成: 对勾
  | "status-failed"    // 失败: ×
  // ── 行动 ──
  | "action-install"   // 下载
  | "action-trash"     // 删除
  | "action-refresh"   // 刷新
  | "action-chevron"   // 折叠
  | "action-external"  // 外链
  | "action-copy"      // 复制
  // ── 杂 ──
  | "user"             // 人头
  | "coin"             // 金币
  | "shield"           // 验证
  | "clock"            // 时钟
  | "spark"            // 火花
  | "cpu"              // CPU

/**
 * 每个 path 是一段 SVG 内部 (<g> 内容)
 * 直接 v-html · 顶层 svg 由 Icon.vue 包裹
 */
export const ICON_PATHS: Record<IconName, string> = {
  // ── nav ──
  "nav-dashboard": `<rect x="3" y="3" width="7" height="9" rx="1.2"/><rect x="14" y="3" width="7" height="5" rx="1.2"/><rect x="14" y="12" width="7" height="9" rx="1.2"/><rect x="3" y="16" width="7" height="5" rx="1.2"/>`,
  "nav-market": `<path d="M3 9l2-5h14l2 5"/><path d="M3 9v11a1 1 0 0 0 1 1h16a1 1 0 0 0 1-1V9"/><path d="M3 9h18"/><path d="M9 14h6"/>`,
  "nav-history": `<circle cx="12" cy="12" r="9"/><polyline points="12 7 12 12 15 14"/>`,
  "nav-earnings": `<polyline points="3 17 9 11 13 15 21 7"/><polyline points="14 7 21 7 21 14"/>`,
  "nav-throttle": `<line x1="4" y1="7" x2="20" y2="7"/><circle cx="9" cy="7" r="2.4" fill="currentColor"/><line x1="4" y1="12" x2="20" y2="12"/><circle cx="15" cy="12" r="2.4" fill="currentColor"/><line x1="4" y1="17" x2="20" y2="17"/><circle cx="11" cy="17" r="2.4" fill="currentColor"/>`,
  "nav-device": `<rect x="4" y="4" width="16" height="16" rx="2"/><rect x="8" y="8" width="8" height="8" rx="0.8"/><line x1="9" y1="4" x2="9" y2="2"/><line x1="15" y1="4" x2="15" y2="2"/><line x1="9" y1="22" x2="9" y2="20"/><line x1="15" y1="22" x2="15" y2="20"/><line x1="2" y1="9" x2="4" y2="9"/><line x1="2" y1="15" x2="4" y2="15"/><line x1="20" y1="9" x2="22" y2="9"/><line x1="20" y1="15" x2="22" y2="15"/>`,
  "nav-toolbox": `<rect x="3" y="7" width="18" height="13" rx="1.5"/><path d="M8 7V5a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/><line x1="3" y1="13" x2="21" y2="13"/><rect x="10" y="11" width="4" height="4" rx="0.5" fill="currentColor"/>`,
  "nav-ai": `<path d="M12 2v3"/><path d="M12 19v3"/><path d="M5 12H2"/><path d="M22 12h-3"/><path d="M4.93 4.93l2.12 2.12"/><path d="M16.95 16.95l2.12 2.12"/><path d="M4.93 19.07l2.12-2.12"/><path d="M16.95 7.05l2.12-2.12"/><circle cx="12" cy="12" r="4"/>`,
  "nav-settings": `<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>`,
  "nav-help": `<circle cx="12" cy="12" r="9"/><path d="M9.5 9a2.5 2.5 0 1 1 4.27 1.78c-.79.83-1.27 1.46-1.27 2.72v.5"/><line x1="12" y1="17.5" x2="12.01" y2="17.5" stroke-width="2.4"/>`,
  // ── task category ──
  "task-text": `<line x1="6" y1="6" x2="18" y2="6"/><line x1="6" y1="10" x2="14" y2="10"/><line x1="6" y1="14" x2="18" y2="14"/><line x1="6" y1="18" x2="12" y2="18"/>`,
  "task-encoding": `<circle cx="8" cy="15" r="4"/><path d="M10.85 12.15 19 4l-3 3 1 3-3 1-3 3"/>`,
  "task-image": `<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="8.5" cy="9.5" r="1.5" fill="currentColor"/><polyline points="21 16 16 11 5 20"/>`,
  "task-video": `<rect x="3" y="5" width="13" height="14" rx="1.5"/><polygon points="16 9 22 5 22 19 16 15" fill="currentColor"/>`,
  "task-doc": `<path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><polyline points="14 3 14 9 20 9"/><line x1="8" y1="13" x2="16" y2="13"/><line x1="8" y1="17" x2="13" y2="17"/>`,
  "task-compute": `<path d="M5 5h14L13 12l6 7H5l6-7z" fill="none"/>`,
  "task-ai": `<circle cx="12" cy="12" r="3"/><circle cx="5" cy="6" r="1.5" fill="currentColor"/><circle cx="19" cy="6" r="1.5" fill="currentColor"/><circle cx="5" cy="18" r="1.5" fill="currentColor"/><circle cx="19" cy="18" r="1.5" fill="currentColor"/><line x1="6.4" y1="6.9" x2="9.6" y2="10.5"/><line x1="17.6" y1="6.9" x2="14.4" y2="10.5"/><line x1="6.4" y1="17.1" x2="9.6" y2="13.5"/><line x1="17.6" y1="17.1" x2="14.4" y2="13.5"/>`,
  "task-render": `<path d="M12 3 21 8v8l-9 5-9-5V8z"/><path d="M3 8l9 5 9-5"/><line x1="12" y1="13" x2="12" y2="21"/>`,
  // ── status ──
  "status-queued": `<circle cx="12" cy="12" r="9"/><polyline points="12 7 12 12 15 14"/>`,
  "status-running": `<polygon points="13 2 4 14 12 14 11 22 20 10 12 10 13 2" fill="currentColor" stroke="none"/>`,
  "status-verifying": `<circle cx="11" cy="11" r="6"/><line x1="20" y1="20" x2="15.5" y2="15.5"/><polyline points="9 11 11 13 14 9"/>`,
  "status-done": `<circle cx="12" cy="12" r="9"/><polyline points="8.5 12 11 14.5 16 9.5"/>`,
  "status-failed": `<circle cx="12" cy="12" r="9"/><line x1="9" y1="9" x2="15" y2="15"/><line x1="15" y1="9" x2="9" y2="15"/>`,
  // ── action ──
  "action-install": `<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/>`,
  "action-trash": `<polyline points="3 6 5 6 21 6"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6"/><path d="M14 11v6"/>`,
  "action-refresh": `<polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10"/><path d="M20.49 15a9 9 0 0 1-14.85 3.36L1 14"/>`,
  "action-chevron": `<polyline points="6 9 12 15 18 9"/>`,
  "action-external": `<path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/>`,
  "action-copy": `<rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>`,
  // ── misc ──
  "user": `<circle cx="12" cy="8" r="4"/><path d="M4 21v-1a6 6 0 0 1 6-6h4a6 6 0 0 1 6 6v1"/>`,
  "coin": `<circle cx="12" cy="12" r="9"/><path d="M15 9.5c-.7-1-2-1.5-3-1.5-2 0-3 1-3 2.5s1 2 3 2.5 3 1 3 2.5-1 2.5-3 2.5c-1 0-2.3-.5-3-1.5"/><line x1="12" y1="6" x2="12" y2="8"/><line x1="12" y1="16" x2="12" y2="18"/>`,
  "shield": `<path d="M12 2 4 5v6c0 5 3.5 9.5 8 11 4.5-1.5 8-6 8-11V5z"/><polyline points="9 12 11 14 15 10"/>`,
  "clock": `<circle cx="12" cy="12" r="9"/><polyline points="12 7 12 12 15 14"/>`,
  "spark": `<path d="M12 2v6"/><path d="M12 16v6"/><path d="M4 12h6"/><path d="M14 12h6"/><path d="M6 6l4 4"/><path d="M14 14l4 4"/><path d="M6 18l4-4"/><path d="M14 10l4-4"/>`,
  "cpu": `<rect x="6" y="6" width="12" height="12" rx="1"/><rect x="9" y="9" width="6" height="6" rx="0.4" fill="currentColor"/><line x1="9" y1="2" x2="9" y2="5"/><line x1="15" y1="2" x2="15" y2="5"/><line x1="9" y1="19" x2="9" y2="22"/><line x1="15" y1="19" x2="15" y2="22"/><line x1="2" y1="9" x2="5" y2="9"/><line x1="2" y1="15" x2="5" y2="15"/><line x1="19" y1="9" x2="22" y2="9"/><line x1="19" y1="15" x2="22" y2="15"/>`,
}

/**
 * 把 backend task_registry.category 映射到 icon
 */
export function iconForCategory(category: string): IconName {
  switch (category) {
    case "text": return "task-text"
    case "encoding": return "task-encoding"
    case "image": return "task-image"
    case "video": return "task-video"
    case "doc": return "task-doc"
    case "compute": return "task-compute"
    case "ai": return "task-ai"
    case "render": return "task-render"
    default: return "task-text"
  }
}

/**
 * task_type 字符串 → 推断 category → icon
 * (前端可能没有 backend category 字段时的兜底)
 */
export function iconForTaskType(taskType: string): IconName {
  if (!taskType) return "task-text"
  const t = taskType.toLowerCase()
  if (t.startsWith("image_") || t === "image") return "task-image"
  if (t.startsWith("video_") || t.startsWith("audio_")) return "task-video"
  if (t.startsWith("pdf_") || t.includes("doc") || t === "ocr_image") return "task-doc"
  if (t.includes("llm") || t.includes("whisper") || t.includes("embedding") || t.includes("vision")) return "task-ai"
  if (t.includes("hash") || t.includes("base64") || t.includes("encode")) return "task-encoding"
  if (t.includes("pi_") || t.includes("monte") || t.includes("fft") || t.includes("onnx")) return "task-compute"
  if (t.includes("render") || t.includes("blender")) return "task-render"
  if (t.includes("dedup") || t.includes("word") || t.includes("json")) return "task-text"
  return "task-text"
}

/**
 * status → icon
 */
export function iconForStatus(status: string, ok?: boolean): IconName {
  if (status === "done" && ok === false) return "status-failed"
  switch (status) {
    case "queued": return "status-queued"
    case "running": return "status-running"
    case "verifying": return "status-verifying"
    case "done": return "status-done"
    case "failed": return "status-failed"
    default: return "status-queued"
  }
}
