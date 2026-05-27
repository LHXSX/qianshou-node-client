#!/usr/bin/env bash
# publish-from-release.sh · v2 简洁版
#
# CI build 完后 · 一键从 GitHub Release 拉 Win + macOS Intel 产物
#   → 上传到生产 OTA 目录
#   → 更新 binary.json 让 OTA endpoint 把 Win/Intel 用户也升到这个版本
#
# Mac arm64 不动 (已手工发了 · binary.json 里 darwin-aarch64 已经 8.1.0)
#
# 用法 (auto mode · 从 GH Release 拉):
#   GH_TOKEN=ghp_xxx ./scripts/publish-from-release.sh
#
# 用法 (manual mode · 文件手工下好了):
#   mkdir -p /tmp/qianshou-8.1.0-release
#   浏览器打 https://github.com/LHXSX/qianshou-node-client/releases/tag/v8.1.0
#   下 5 个 asset 到 /tmp/qianshou-8.1.0-release/
#   MANUAL=1 ./scripts/publish-from-release.sh
#
# 设计要点 (避坑):
#   - 中文文件名在 scp/ssh 链里被 zsh / bash / ssh 多次解析容易乱
#     → 全部打 tar 一次性传 · 远端 untar · 路径都用 tar 内部相对路径 (utf-8 安全)
#   - GH API 拉 private release asset · 用 PAT + Accept: application/octet-stream

set -euo pipefail

VERSION="${VERSION:-8.1.0}"
REPO="${REPO:-LHXSX/qianshou-node-client}"
SSH_HOST="${SSH_HOST:-edge}"
MANUAL="${MANUAL:-0}"   # 1 = 跳过下载 · 直接用 WORKDIR 里现有文件

if [ "${MANUAL}" != "1" ]; then
  GH_TOKEN="${GH_TOKEN:?需 GH_TOKEN env (或 MANUAL=1 则跳过下载)}"
fi

TAG="v${VERSION}"
WORKDIR="/tmp/qianshou-${VERSION}-release"
PROD_DL_DIR="/var/www/web/downloads/latest"
PROD_OTA_DIR="/var/www/qianshou-app/client-v3/binary/${VERSION}"

# 期望的文件 (tauri-action 命名规则)
EXPECT_FILES=(
  "千手节点_${VERSION}_x64-setup.exe"               # Win NSIS · 下载页 + OTA
  "千手节点_${VERSION}_x64-setup.exe.sig"           # Win OTA 签名
  "千手节点_${VERSION}_x64.dmg"                     # Mac Intel · 下载页
  "千手节点_${VERSION}_x64.app.tar.gz"              # Mac Intel OTA payload
  "千手节点_${VERSION}_x64.app.tar.gz.sig"          # Mac Intel OTA 签名
)

YEL='\033[33m'; GRN='\033[32m'; RED='\033[31m'; CYN='\033[36m'; NC='\033[0m'
log()  { printf "${CYN}▶${NC} %s\n" "$*"; }
ok()   { printf "${GRN}✓${NC} %s\n" "$*"; }
warn() { printf "${YEL}⚠${NC} %s\n" "$*" >&2; }
err()  { printf "${RED}✗${NC} %s\n" "$*" >&2; }

# ─────────────────────────────────────────────────────────
# 0. Manual mode 分支 (MANUAL=1 时跳过下载 · 直接用 WORKDIR 里现有文件)
# ─────────────────────────────────────────────────────────
SKIP_DOWNLOAD=0
if [ "${MANUAL}" = "1" ]; then
  log "MANUAL=1 · 跳过下载 · 用 ${WORKDIR} 里现有文件"
  if [ ! -d "${WORKDIR}" ]; then
    err "${WORKDIR} 不存在 · 先 mkdir + 下 5 个 asset 到这"
    exit 1
  fi
  missing=()
  for f in "${EXPECT_FILES[@]}"; do
    if [ ! -f "${WORKDIR}/${f}" ]; then
      missing+=("${f}")
    fi
  done
  if [ ${#missing[@]} -gt 0 ]; then
    err "${WORKDIR} 缺 ${#missing[@]} 个文件:"
    printf '  - %s\n' "${missing[@]}" >&2
    exit 1
  fi
  ok "5 个文件齐"
  ls -la "${WORKDIR}"
  SKIP_DOWNLOAD=1
fi

if [ "${SKIP_DOWNLOAD}" = "0" ]; then
# ─────────────────────────────────────────────────────────
# 1. 等 release ready
# ─────────────────────────────────────────────────────────
log "等 GitHub Release ${TAG} 的 5 个 assets 齐 (最多 45 min)"

API_URL="https://api.github.com/repos/${REPO}/releases/tags/${TAG}"
WAIT_MAX=$((45 * 60))
WAIT_INTERVAL=60
elapsed=0
RESP=""

while true; do
  RESP=$(curl -sS -H "Authorization: Bearer ${GH_TOKEN}" \
              -H "Accept: application/vnd.github+json" \
              "${API_URL}")

  if python3 -c "import json,sys; sys.exit(0 if 'assets' in json.loads('''${RESP}''') else 1)" 2>/dev/null; then
    missing=()
    for f in "${EXPECT_FILES[@]}"; do
      if ! echo "${RESP}" | python3 -c "
import json, sys
d = json.load(sys.stdin)
names = [a['name'] for a in d.get('assets', [])]
sys.exit(0 if '${f}' in names else 1)
"; then
        missing+=("${f}")
      fi
    done
    if [ ${#missing[@]} -eq 0 ]; then
      ok "5 个文件齐 (elapsed=${elapsed}s)"
      break
    fi
    warn "缺 ${#missing[@]} 个 (elapsed=${elapsed}s): ${missing[*]}"
  else
    msg=$(echo "${RESP}" | python3 -c "import json,sys; print(json.load(sys.stdin).get('message','?'))" 2>/dev/null || echo "?")
    warn "Release ${TAG} 还没出来 (msg=${msg}) elapsed=${elapsed}s"
  fi

  if [ $elapsed -ge $WAIT_MAX ]; then
    err "等了 45 min 没齐 · 当前 assets:"
    echo "${RESP}" | python3 -c "import json,sys; [print(' ', a['name']) for a in json.load(sys.stdin).get('assets',[])]"
    exit 2
  fi
  sleep ${WAIT_INTERVAL}
  elapsed=$((elapsed + WAIT_INTERVAL))
done

# ─────────────────────────────────────────────────────────
# 2. 下载到本地
# ─────────────────────────────────────────────────────────
log "下到 ${WORKDIR}"
rm -rf "${WORKDIR}" && mkdir -p "${WORKDIR}"

EXPECT_BLOB=$(printf "%s\n" "${EXPECT_FILES[@]}")
echo "${RESP}" | EXPECT="${EXPECT_BLOB}" python3 > /tmp/qianshou_assets.tsv <<'PY'
import json, os, sys
d = json.load(sys.stdin)
expect = set(os.environ.get("EXPECT", "").strip().split("\n"))
for a in d.get("assets", []):
    if a["name"] in expect:
        print(f"{a['url']}\t{a['name']}")
PY

while IFS=$'\t' read -r url name; do
  [ -z "${url}" ] && continue
  log "  下 ${name}"
  curl -sSL \
    -H "Authorization: Bearer ${GH_TOKEN}" \
    -H "Accept: application/octet-stream" \
    -o "${WORKDIR}/${name}" \
    "${url}"
  sz=$(stat -f '%z' "${WORKDIR}/${name}" 2>/dev/null || stat -c '%s' "${WORKDIR}/${name}")
  ok "    ${sz} bytes"
done < /tmp/qianshou_assets.tsv

ls -la "${WORKDIR}"
fi  # SKIP_DOWNLOAD

# ─────────────────────────────────────────────────────────
# 3. 全部打 tar 传上服务器 (绕开中文 scp 坑)
# ─────────────────────────────────────────────────────────
log "tar 打包 + scp"
( cd "${WORKDIR}" && tar -czf /tmp/qs-release.tar.gz ./ )
scp -q /tmp/qs-release.tar.gz "${SSH_HOST}:/tmp/qs-release.tar.gz"

# ─────────────────────────────────────────────────────────
# 4. 远端 untar + 分发
# ─────────────────────────────────────────────────────────
log "远端分发到下载页 + OTA 目录"
ssh "${SSH_HOST}" "VERSION=${VERSION} PROD_DL_DIR='${PROD_DL_DIR}' PROD_OTA_DIR='${PROD_OTA_DIR}' python3 -" << 'PYEOF'
import os, shutil, subprocess

VER = os.environ["VERSION"]
DL = os.environ["PROD_DL_DIR"]
OTA = os.environ["PROD_OTA_DIR"]

os.makedirs(OTA, exist_ok=True)
try:
    shutil.chown(OTA, "root", "nginx")
except Exception:
    pass

# untar
extract = f"/tmp/qs-release-{VER}-extract"
shutil.rmtree(extract, ignore_errors=True)
os.makedirs(extract)
subprocess.run(["tar", "-xzf", "/tmp/qs-release.tar.gz", "-C", extract], check=True)

# 列文件
files = sorted(os.listdir(extract))
print("解压得:")
for f in files:
    sz = os.path.getsize(os.path.join(extract, f))
    print(f"  {f}  ({sz} bytes)")

# 分发规则
moves = []
exe = f"千手节点_{VER}_x64-setup.exe"
dmg = f"千手节点_{VER}_x64.dmg"
appgz = f"千手节点_{VER}_x64.app.tar.gz"

if exe in files:
    moves += [(exe, DL, exe), (exe, OTA, exe)]
if exe + ".sig" in files:
    moves += [(exe + ".sig", OTA, exe + ".sig")]
if dmg in files:
    moves += [(dmg, DL, dmg)]
if appgz in files:
    moves += [(appgz, OTA, appgz)]
if appgz + ".sig" in files:
    moves += [(appgz + ".sig", OTA, appgz + ".sig")]

for src_name, dst_dir, dst_name in moves:
    src = os.path.join(extract, src_name)
    dst = os.path.join(dst_dir, dst_name)
    shutil.copy(src, dst)
    try:
        shutil.chown(dst, "nginx", "nginx")
    except Exception:
        pass
    print(f"  ✓ {dst}  ({os.path.getsize(dst)} bytes)")

# 清临时
shutil.rmtree(extract, ignore_errors=True)
os.remove("/tmp/qs-release.tar.gz")
print("✓ 分发完成")
PYEOF

# ─────────────────────────────────────────────────────────
# 5. 更新 binary.json (远端 python 完成)
# ─────────────────────────────────────────────────────────
log "更新 binary.json"

WIN_SIG=$(cat "${WORKDIR}/千手节点_${VERSION}_x64-setup.exe.sig" 2>/dev/null || echo "")
INTEL_SIG=$(cat "${WORKDIR}/千手节点_${VERSION}_x64.app.tar.gz.sig" 2>/dev/null || echo "")

if [ -z "${WIN_SIG}" ] || [ -z "${INTEL_SIG}" ]; then
  warn "WIN_SIG 或 INTEL_SIG 空 · 跳过 binary.json 更新"
else
  ssh "${SSH_HOST}" "VERSION=${VERSION} WIN_SIG='${WIN_SIG}' INTEL_SIG='${INTEL_SIG}' python3 -" << 'PYEOF'
import json, os, shutil, urllib.parse
from datetime import datetime, timezone

VER = os.environ["VERSION"]
WIN_SIG = os.environ["WIN_SIG"]
INTEL_SIG = os.environ["INTEL_SIG"]

path = "/var/www/qianshou-app/client-v3/binary.json"
shutil.copy(path, path + f".bak.before-{VER}-multi")

with open(path) as f:
    d = json.load(f)

base = f"https://www.wujisuanli.com/app/client-v3/binary/{VER}"

win_name = urllib.parse.quote(f"千手节点_{VER}_x64-setup.exe")
d["platforms"]["windows-x86_64"] = {
    "url": f"{base}/{win_name}",
    "signature": WIN_SIG.strip(),
}

intel_name = urllib.parse.quote(f"千手节点_{VER}_x64.app.tar.gz")
d["platforms"]["darwin-x86_64"] = {
    "url": f"{base}/{intel_name}",
    "signature": INTEL_SIG.strip(),
}

d["pub_date"] = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

with open(path, "w") as f:
    json.dump(d, f, ensure_ascii=False, indent=2)
shutil.chown(path, "nginx", "nginx")

print(f"✓ binary.json 已更 · 3 平台齐:")
for k, v in d["platforms"].items():
    print(f"  {k}: {v['url']}")
PYEOF
fi

# ─────────────────────────────────────────────────────────
# 6. 验证 OTA endpoint
# ─────────────────────────────────────────────────────────
log "验 OTA endpoint (老 8.0.0 客户端模拟查更新)"

for tgt_arch in "darwin/aarch64" "darwin/x86_64" "windows/x86_64"; do
  status=$(curl -sS -o /dev/null -w '%{http_code}' \
    "https://www.wujisuanli.com/api/v8/client/updater/${tgt_arch}/8.0.0")
  ver="?"
  if [ "${status}" = "200" ]; then
    ver=$(curl -sS "https://www.wujisuanli.com/api/v8/client/updater/${tgt_arch}/8.0.0" \
      | python3 -c "import json,sys; print(json.load(sys.stdin).get('version','?'))" 2>/dev/null || echo "?")
  fi
  printf "  %-25s HTTP=%s  version=%s\n" "${tgt_arch}" "${status}" "${ver}"
done

ok "完成 · 8.1.0 已发 3 平台 OTA"
echo "下载页: https://www.wujisuanli.com/downloads/"
