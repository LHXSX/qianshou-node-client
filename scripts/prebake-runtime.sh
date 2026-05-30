#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# prebake-runtime.sh · CI 每平台 build 前烘焙 cpython + uv 到 src-tauri/resources/
#
# V8.1.5 (2026-05-30):
#   安装包内置 cpython + uv (工具链零依赖网络) · 不再内置任何 venv
#   之前 ~1.5G(含 venvs) → 现在 ~87MB(cpython 59M + uv 28M)
#   venv (lite/crawl/ocr/...) 首启从自家镜像 https://by.wujisuanli.com 下拉 ·
#   镜像无对应平台 tarball 时降级公共 PyPI 源本地 uv pip install
#   好处: 安装包小 · 工具链离线可用 · 更新不重装依赖 · 仅装依赖才需网络
#
#   首启流程:
#     1. bootstrap_bundled 拷贝 cpython 到 ~/.qianshou/runtime/
#     2. ensure_uv 优先用内置 uv (resources/bin/uv-<triple>) · 缺失才走镜像/GitHub
#     3. auto_install_tiers 用 uv 装 lite/crawl venv (优先自家镜像 tarball · 兜底 pip)
#
# 用法: bash scripts/prebake-runtime.sh <platform>
#   <platform> ∈ macos-arm64 | macos-intel | windows-x64
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

PLATFORM="${1:-}"
if [[ -z "$PLATFORM" ]]; then
    echo "usage: $0 <macos-arm64|macos-intel|windows-x64>" >&2
    exit 2
fi

# 固定一个稳定版本 · 不依赖 GH latest 防 CI flake
PY_VERSION="3.11.10"
PY_RELEASE_DATE="20241016"
UV_VERSION="0.5.4"

# uv 各平台:
#   MIRROR_PATH  自家镜像 raw 二进制 (by.wujisuanli.com/v1/<这里>)
#   UV_DEST_NAME resources/bin/<这里> · 必须与 uv.rs::bundled_uv_filename() 完全一致
#   UV_GH_PATH   GitHub fallback (压缩包 · 镜像挂了用) · 与 uv.rs::github_uv_url() 对齐
case "$PLATFORM" in
  macos-arm64)
    PY_TRIPLE="aarch64-apple-darwin"
    PLATFORM_LABEL="macos-aarch64"
    UV_MIRROR_PATH="macos-arm64/uv"
    UV_DEST_NAME="uv-aarch64-apple-darwin"
    UV_GH_ASSET="uv-aarch64-apple-darwin.tar.gz"
    UV_ARCHIVE="tar.gz"
    ;;
  macos-intel)
    PY_TRIPLE="x86_64-apple-darwin"
    PLATFORM_LABEL="macos-x86_64"
    UV_MIRROR_PATH="macos-x86_64/uv"
    UV_DEST_NAME="uv-x86_64-apple-darwin"
    UV_GH_ASSET="uv-x86_64-apple-darwin.tar.gz"
    UV_ARCHIVE="tar.gz"
    ;;
  windows-x64)
    PY_TRIPLE="x86_64-pc-windows-msvc-shared"
    PLATFORM_LABEL="windows-x86_64"
    UV_MIRROR_PATH="windows-x86_64/uv.exe"
    UV_DEST_NAME="uv-x86_64-pc-windows-msvc.exe"
    UV_GH_ASSET="uv-x86_64-pc-windows-msvc.zip"
    UV_ARCHIVE="zip"
    ;;
  *)
    echo "unknown platform: $PLATFORM (expected: macos-arm64 | macos-intel | windows-x64)" >&2
    exit 2
    ;;
esac

DEST_RESOURCES="src-tauri/resources"
DEST_CPYTHON_DIR="$DEST_RESOURCES/runtime/cpython"
mkdir -p "$DEST_CPYTHON_DIR"

# 清旧文件 (CI 上一般干净 · 本机重跑可能有残留)
rm -rf "$DEST_CPYTHON_DIR"/cpython-* 2>/dev/null || true

# ─── 1. 下 + 解 cpython ──────────────────────────────────────────────────────
PY_URL="https://github.com/indygreg/python-build-standalone/releases/download/${PY_RELEASE_DATE}/cpython-${PY_VERSION}+${PY_RELEASE_DATE}-${PY_TRIPLE}-install_only.tar.gz"
echo "[1/3] 下 cpython"
echo "      URL: $PY_URL"
TMP_PY=$(mktemp -d)
curl -fL --retry 3 --connect-timeout 30 --max-time 600 -o "$TMP_PY/cpython.tar.gz" "$PY_URL"
tar -xzf "$TMP_PY/cpython.tar.gz" -C "$TMP_PY"
# install_only archive 内是 `python/` 目录 · 改名为 cpython-<ver>-<platform>-none
# 让 paths.rs::bundled_python_bin 能扫到 (要求 dir 名以 "cpython-" 起头)
CPYTHON_TARGET="$DEST_CPYTHON_DIR/cpython-${PY_VERSION}-${PLATFORM_LABEL}-none"
mv "$TMP_PY/python" "$CPYTHON_TARGET"
rm -rf "$TMP_PY"
echo "      → $CPYTHON_TARGET ($(du -sh "$CPYTHON_TARGET" | cut -f1))"

# ─── 2. 下 uv → resources/bin/<UV_DEST_NAME> ──────────────────────────────────
# 优先自家镜像 raw 二进制 · 挂了走 GitHub release 压缩包 · 文件名必须与
# uv.rs::bundled_uv_filename() 一致 · 否则客户端拷不到内置 uv 会退化到运行时下载
DEST_BIN_DIR="$DEST_RESOURCES/bin"
mkdir -p "$DEST_BIN_DIR"
# 清旧 uv 二进制 (防止跨架构残留 · 保留 README.md)
rm -f "$DEST_BIN_DIR"/uv-* "$DEST_BIN_DIR"/uv.exe 2>/dev/null || true

UV_DEST="$DEST_BIN_DIR/$UV_DEST_NAME"
UV_MIRROR_URL="https://by.wujisuanli.com/v1/${UV_MIRROR_PATH}"
UV_GH_VERSION="0.11.15"
UV_GH_URL="https://github.com/astral-sh/uv/releases/download/${UV_GH_VERSION}/${UV_GH_ASSET}"

echo "[2/3] 下 uv"
echo "      镜像: $UV_MIRROR_URL"
if curl -fL --retry 3 --connect-timeout 30 --max-time 300 -o "$UV_DEST" "$UV_MIRROR_URL"; then
    echo "      ← 自家镜像 raw 二进制"
else
    echo "      镜像失败 · 走 GitHub: $UV_GH_URL" >&2
    TMP_UV=$(mktemp -d)
    curl -fL --retry 3 --connect-timeout 30 --max-time 300 -o "$TMP_UV/uv_pkg" "$UV_GH_URL"
    if [[ "$UV_ARCHIVE" == "zip" ]]; then
        unzip -o -q "$TMP_UV/uv_pkg" -d "$TMP_UV/unp"
    else
        mkdir -p "$TMP_UV/unp" && tar -xzf "$TMP_UV/uv_pkg" -C "$TMP_UV/unp"
    fi
    # GitHub 包内可执行名为 uv / uv.exe (可能在子目录) · 找出来挪到目标名
    UV_FOUND=$(find "$TMP_UV/unp" -type f \( -name uv -o -name uv.exe \) | head -1)
    if [[ -z "$UV_FOUND" ]]; then
        echo "ERROR: GitHub 包内未找到 uv 可执行" >&2
        ls -laR "$TMP_UV/unp" >&2 || true
        exit 5
    fi
    mv "$UV_FOUND" "$UV_DEST"
    rm -rf "$TMP_UV"
fi
# Windows 不需要 +x · *nix 必须可执行
if [[ "$PLATFORM" != "windows-x64" ]]; then
    chmod +x "$UV_DEST"
fi
echo "      → $UV_DEST ($(du -sh "$UV_DEST" | cut -f1))"

# ─── 3. 写 manifest.json ────────────────────────────────────────────────
# V8.1.5 内置 cpython + uv · 不内置 venv · venv 首启从镜像/公共源装
cat > "$DEST_RESOURCES/runtime/manifest.json" << EOF
{
  "schema_version": 2,
  "bundled_at": "$(date -u +%Y-%m-%d)",
  "python": "${PY_VERSION}",
  "uv": "${UV_VERSION}",
  "platform": "${PLATFORM_LABEL}",
  "bundled_venvs": [],
  "mirror_base": "https://by.wujisuanli.com",
  "note": "内置 cpython + uv · venv 首启从 by.wujisuanli.com 下拉(缺则公共源) · 更新不删已装"
}
EOF
echo "[3/3] 写 $DEST_RESOURCES/runtime/manifest.json (platform=$PLATFORM_LABEL · cpython+uv · venvs 走镜像)"

# ─── 校验: bundled_python_bin 能不能找到 ──────────────────────────────────
EXPECTED_PY_BIN="$CPYTHON_TARGET/bin/python3.11"
if [[ "$PLATFORM" == "windows-x64" ]]; then
    EXPECTED_PY_BIN="$CPYTHON_TARGET/python.exe"
fi
if [[ ! -f "$EXPECTED_PY_BIN" ]]; then
    echo "WARN: 预期 python 二进制不存在: $EXPECTED_PY_BIN" >&2
    echo "      bundled_python_bin() 可能找不到 · 检查 cpython archive 结构" >&2
    ls -la "$CPYTHON_TARGET" >&2 || true
    exit 4
fi

# uv 必须落地 · 否则客户端内置 uv 失效 (退化运行时下载)
if [[ ! -f "$UV_DEST" ]]; then
    echo "WARN: 预期 uv 二进制不存在: $UV_DEST" >&2
    echo "      uv.rs::ensure_uv 会退化到镜像/GitHub 运行时下载" >&2
    exit 6
fi

TOTAL=$(du -sh "$DEST_RESOURCES" 2>/dev/null | cut -f1)
echo ""
echo "✅ prebake $PLATFORM 完成 · resources/ 总大小 = $TOTAL"
echo "   python:   $EXPECTED_PY_BIN"
echo "   uv:       $UV_DEST"
echo "   manifest: $DEST_RESOURCES/runtime/manifest.json"
