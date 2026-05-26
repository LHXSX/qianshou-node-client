/// <reference types="vite/client" />

declare module "*.vue" {
  import type { DefineComponent } from "vue"
  const component: DefineComponent<{}, {}, any>
  export default component
}

// 从 package.json 注入 · vite.config.ts define
declare const __APP_VERSION__: string
