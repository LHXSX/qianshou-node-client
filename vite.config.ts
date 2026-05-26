import { defineConfig } from "vite"
import vue from "@vitejs/plugin-vue"
import { resolve } from "path"
import pkg from "./package.json"

// Tauri 2 推荐配置：
// - 固定 1420 端口（Tauri 默认）
// - 禁用 host check（Tauri WebView 同源）
// - 防止 Vite 清屏（让 cargo build 日志可读）
export default defineConfig(() => ({
  // 2026-05-21 · base 改成相对路径
  // 原因: 客户端前端热更新部署在 https://...com/app/client-v3/<version>/
  //       绝对路径 /assets/... 会被主站 nginx SPA 兜底吞成 index.html, 白屏
  //       相对路径 ./assets/... 会落 /app/client-v3/<version>/assets/, 正确
  // Tauri 打包内 (tauri://localhost) 用相对路径同样工作
  base: "./",
  plugins: [vue()],
  clearScreen: false,
  define: {
    // 从 package.json 自动注入 · 杜绝硬编码漂移
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  resolve: {
    alias: {
      "@shared": resolve(__dirname, "./shared"),
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    host: false,
    hmr: false,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target: process.env.TAURI_ENV_PLATFORM == "windows" ? "chrome105" : "safari13",
    minify: (!process.env.TAURI_ENV_DEBUG ? "esbuild" : false) as "esbuild" | false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
}))
