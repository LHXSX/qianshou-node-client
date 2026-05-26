# EdgeCompute Desktop Client v3.0.0

> 一个进程一个 PID 的全新桌面客户端，Tauri 2 + Vue 3 + Rust WebSocket。

## 技术栈

- **框架**：Tauri 2.x（Rust 主进程） + Vue 3 + Vite 6
- **通讯**：WebSocket Secure 长连 (`tokio-tungstenite`) + REST 兜底 (`reqwest`)
- **认证**：Magic link 无密码登录 + JWT (15min) + Refresh token (30天，OS Keyring)
- **任务**：Rust tokio task 内执行（v3.0.0），WASM sidecar 留 v3.1
- **更新**：Tauri updater plugin (ed25519 签名)
- **UI**：Vue 3 + Pinia + Element Plus（与官网一致）
- **图表**：ECharts 5

## 目录结构

```
client-v3/
├── package.json
├── vite.config.ts
├── tsconfig.json
├── index.html
├── src/                            # Vue 端
│   ├── main.ts
│   ├── App.vue
│   ├── env.d.ts
│   ├── styles/global.css
│   ├── views/                      # M2.2+ 添加
│   ├── stores/                     # M2.2+ 添加
│   └── composables/                # M2.2+ 添加
└── src-tauri/                      # Rust 端
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── build.rs
    ├── capabilities/
    │   └── default.json
    └── src/
        ├── main.rs
        ├── lib.rs
        ├── comm/                   # M2.2 WebSocket / REST 模块
        ├── auth/                   # M2.3 token + magic-link
        ├── task/                   # M3 任务执行
        ├── system/                 # M3 硬件探测
        ├── state/                  # M3 持久化
        └── updater.rs              # M3 自动更新封装
```

## 开发

```bash
cd /Users/pd/算力/client-v3
npm install                # 安装前端依赖
npm run tauri:dev          # 启动 dev（同时跑 vite + cargo run）
```

第一次构建会下载所有 Rust crate（约 200-500 个），需要几分钟。后续增量编译秒级。

## 构建

```bash
npm run tauri:build        # 生成 .dmg / .msi / .deb / .AppImage
```

产物在 `src-tauri/target/release/bundle/`。

## 协议契约

详见 `/Users/pd/算力/backend/simple_server.py` 中 `ws_agent_v3` 函数注释。
WSS endpoint：`wss://pidbai.com/api/v1/ws/agent`
Sec-WebSocket-Protocol：`edgecompute.v1`

## 里程碑

- [x] **M1** 后端契约就绪（WS / magic-link / refresh / updater 端点）
- [ ] **M2** 客户端骨架（**此里程碑进行中**）
  - [x] M2.1 最小可启动骨架
  - [ ] M2.2 Rust WS client + 状态机
  - [ ] M2.3 magic-link 登录 UI
  - [ ] M2.4 主面板 + 状态恢复
- [ ] **M3** 核心功能闭环（任务 / 收益 / 暂停 / 更新 / 持久化）
- [ ] **M4** 跨平台 + CI/CD + 发布
