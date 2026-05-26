# 🚀 GitHub Actions 一键多平台 build 指南

> 不用本机装 Rust / VS Build Tools / Xcode · push tag 自动出 mac+win+linux 三平台 build

## 🎯 一次性配置 (10 分钟)

### 步骤 1 · 创建 GitHub repo (2 min)

1. 打开 [github.com/new](https://github.com/new)
2. Repository name: `qianshou-node-client` (随便起)
3. **Private** (推荐 · 私有 repo 每月 2000 min 免费 build 时长 · 够 80+ 次)
4. 不勾 README / .gitignore / license (我们已有)
5. Create repository

### 步骤 2 · 把代码 push 上去 (3 min)

在本机 mac 上执行 (我已经帮你建好 .github + 改好 .gitignore):

```bash
cd /Users/pangdundun/算力/client-v3-latest

# 初始化 git
git init
git branch -M main

# 看一眼会被 push 的内容 (确认私钥 .key 不在列表里!)
git add . --dry-run | head -30

# 真 add + commit
git add .
git commit -m "init: tauri 2 client v8.0.17 + CI workflow"

# 关联你的 GitHub repo (把 YOUR-USERNAME 换成你的 GH 用户名)
git remote add origin https://github.com/YOUR-USERNAME/qianshou-node-client.git

# push
git push -u origin main
```

> ⚠️ **如果系统提示 password** · 用 GitHub Personal Access Token (PAT) 而非密码
> 生成: github.com → Settings → Developer settings → Personal access tokens → tokens (classic) → Generate new token
> 勾权限: `repo` (full) + `workflow`. 复制 token 当 password 用.

### 步骤 3 · 配置 3 个 Secrets (2 min)

进入你的 repo → Settings → Secrets and variables → Actions → **New repository secret**

| Secret 名 | Secret 值 |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | 把 `.tauri-keys-2026/qianshou-2026.key` 整个内容粘贴 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 留空 (无密码) |
| (GITHUB_TOKEN 不用配 · workflow 自动注入) | - |

**第一个 secret 值怎么拿:**
```bash
cat /Users/pangdundun/算力/client-v3-latest/.tauri-keys-2026/qianshou-2026.key
# 把输出复制 (含 BEGIN/END 行) · 粘贴到 GH Secrets 里
```

### 步骤 4 · 触发首次 build (1 min)

打开 repo → Actions tab → 左侧选 `build-tauri-multi-platform` → 右侧 **Run workflow** → 选 main 分支 → Run

或者 push 一个 tag (推荐 · 自动出 release):
```bash
git tag v8.0.17
git push origin v8.0.17
```

→ 等 5-10 分钟 · GH Actions 会自动 build mac+win+linux · 全部签名 · 上传到 Releases.

---

## 📦 build 完成后

打开 repo → **Releases** 看新版本:
```
qianshou-node-v8.0.17 (draft)
├─ 千手节点_8.0.17_aarch64.dmg               (macOS Apple Silicon)
├─ 千手节点_8.0.17_x64.dmg                    (macOS Intel)
├─ 千手节点_8.0.17_x64-setup.exe              (Windows 安装包)
├─ 千手节点_8.0.17_x64-setup.nsis.zip         (Windows OTA)
├─ 千手节点_8.0.17_x64-setup.nsis.zip.sig     (Windows OTA 签名)
├─ 千手节点.app.tar.gz                         (macOS OTA)
└─ 千手节点.app.tar.gz.sig                     (macOS OTA 签名)
```

把这些 URL 抓出来 · 直接配进 `binary.json` 和 `release.json`. 全自动!

---

## 🔄 之后每次发版

```bash
# 1 · 改代码 → 改版本号 (tauri.conf.json + Cargo.toml + package.json) → commit
git add .
git commit -m "feat: 8.0.18 - 某某修复"

# 2 · 打 tag → push → 自动 build → 自动出 Release
git tag v8.0.18
git push origin main
git push origin v8.0.18

# 3 · 等 5-10 分钟 → GH Releases 自动出新版 → 复制 URL 配后端 OTA
```

完全脱离本地编译 · 你只管写代码 + push.

---

## ⚙️ workflow 矩阵 (本仓库默认)

| 平台 | runner | 目标 | 产物 |
|---|---|---|---|
| macOS Apple Silicon | macos-latest | aarch64-apple-darwin | .dmg / .app.tar.gz |
| macOS Intel | macos-latest | x86_64-apple-darwin | .dmg / .app.tar.gz |
| Windows x64 | windows-latest | x86_64-pc-windows-msvc | .exe / .nsis.zip |

想加 Linux · 在 `.github/workflows/build-tauri.yml` 注释里取消掉 `ubuntu-22.04` 那段即可.

---

## 🚨 常见坑

| 坑 | 解决 |
|---|---|
| `Error: TAURI_SIGNING_PRIVATE_KEY is not set` | 步骤 3 没配 secret · 重配 |
| Windows build 报 `link.exe` not found | runner 自带 · 不会出 (除非你乱改 yml) |
| build 卡 8 分钟以上 | 第一次正常 · 后续有 rust-cache 加速到 3-5 分钟 |
| 私钥不小心 push 上去了 | 立刻 GitHub → Settings → Delete repo → 重做 + 重生新 key + 改 binary.json |
| GH Releases 没出现 | tag 没 push (`git push origin v8.0.17`) · 或 workflow 失败看 Actions tab 日志 |
| 私有 repo 每月 2000 min 用完 | 改为 public repo (公开无限) 或买 GitHub Pro ($4/月) |

---

## 💡 进阶 · 自动更新 binary.json

可以再加一个 workflow · build 完后自动:
1. 拿到 `.app.tar.gz.sig` 和 `.nsis.zip.sig` 内容
2. SSH 到 47.86.60.178
3. 更新 `/var/www/qianshou-app/client-v3/binary.json`
4. 完全无人值守发版

需要再配:
- secret `SSH_PRIVATE_KEY` (你的 id_ed25519_edge)
- secret `PROD_HOST` = `47.86.60.178`

需要的话告诉我 · 再加一个 `deploy-ota.yml`.

---

## ✅ 验证清单

- [ ] GitHub repo 已建
- [ ] 代码已 push (检查 repo Code tab 能看到 src-tauri/)
- [ ] 3 个 secrets 已配 (Settings → Secrets · 看到 TAURI_SIGNING_PRIVATE_KEY)
- [ ] Actions tab 看到 `build-tauri-multi-platform` workflow
- [ ] 首次 Run workflow 成功 (5-10 min)
- [ ] Releases 看到新版本
- [ ] 下载 .exe 装 win 测试 OK

到这一步 · Win build 永久解决.
