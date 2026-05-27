# 发版脚本 · `publish-from-release.sh`

## 干啥的

CI build 完后 · 一键从 GitHub Release 拉 Windows + macOS Intel 产物 → 上 OTA → 更 `binary.json`。

Mac arm64 已经手工先发了 (2026-05-27) · 这脚本只补齐另外 2 平台。

## 用法

```bash
# 1. 拿 GH PAT (Personal Access Token)
#    https://github.com/settings/tokens
#    权限: repo (private repo 必须)
export GH_TOKEN=ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

# 2. 等 GH Actions build 完 (浏览器看)
#    https://github.com/LHXSX/qianshou-node-client/actions
#
#    应看到 build-tauri-multi-platform 3 个 job 都打勾
#
# 3. 跑脚本 (脚本里会自己轮询 release 5 个 asset 齐了才下)
./scripts/publish-from-release.sh

# 4. 脚本会打 OTA endpoint 验 · 3 平台都该 200 + version=8.1.0
```

## 配置 (可选 env)

| env | 默认 | 说明 |
|-----|------|------|
| `GH_TOKEN` | **必填** | GitHub Personal Access Token (repo read) |
| `VERSION` | `8.1.0` | tag 名 (不含 v) |
| `REPO` | `LHXSX/qianshou-node-client` | GH repo |
| `SSH_HOST` | `edge` | `~/.ssh/config` 里的 alias |

例:
```bash
GH_TOKEN=ghp_xxx VERSION=8.1.1 ./scripts/publish-from-release.sh
```

## 流程

1. `GET https://api.github.com/repos/${REPO}/releases/tags/v${VERSION}`
2. 等 5 个文件齐 (最多 45 min · 每 60s 轮询):
   - `千手节点_${VERSION}_x64-setup.exe` (Win NSIS)
   - `千手节点_${VERSION}_x64-setup.exe.sig` (Win OTA 签名)
   - `千手节点_${VERSION}_x64.dmg` (Mac Intel 下载)
   - `千手节点_${VERSION}_x64.app.tar.gz` (Mac Intel OTA payload)
   - `千手节点_${VERSION}_x64.app.tar.gz.sig` (Mac Intel OTA 签名)
3. 用 `Authorization: Bearer $GH_TOKEN` + `Accept: application/octet-stream` 下到 `/tmp/qianshou-${VERSION}-release/`
4. 整目录打 `tar.gz` 一次性 scp 到服务器 (绕开中文文件名在 scp 链里被解析多次的坑)
5. 服务器 untar + 分发:
   - `.exe` → `/var/www/web/downloads/latest/` (新装下载页) + `/var/www/qianshou-app/client-v3/binary/${VERSION}/` (OTA)
   - `.exe.sig` → OTA 目录
   - `.dmg` → 下载页
   - `.app.tar.gz` + `.sig` → OTA 目录
6. 远端 Python 改 `binary.json`:
   - 新增 `platforms.darwin-x86_64`
   - 新增 `platforms.windows-x86_64`
   - 更 `pub_date`
   - 备份原文件到 `binary.json.bak.before-${VERSION}-multi`
7. curl 验证 3 个 OTA endpoint (darwin/aarch64 + darwin/x86_64 + windows/x86_64) 都返回 200 + version=${VERSION}

## 排错

| 报错 | 原因 | 处理 |
|------|------|------|
| `需 GH_TOKEN env` | 没设环境变量 | `export GH_TOKEN=ghp_...` |
| `Release v${VERSION} 还没出来` | CI 还在 build / 失败了 | 浏览器看 Actions log |
| `等了 45 min 没齐` | CI 部分 job 挂了 | 看哪个文件缺 · 重跑那个 job |
| `WIN_SIG 或 INTEL_SIG 空` | tauri-action 没生成 sig | 检查 GH secrets `TAURI_SIGNING_PRIVATE_KEY[_PASSWORD]` 是否配 |
| `permission denied` (scp) | ssh key 不对 | `ssh edge whoami` 测连接 |

## 回滚

```bash
ssh edge 'cp /var/www/qianshou-app/client-v3/binary.json.bak.before-8.1.0-multi /var/www/qianshou-app/client-v3/binary.json && chown nginx:nginx /var/www/qianshou-app/client-v3/binary.json'
```
