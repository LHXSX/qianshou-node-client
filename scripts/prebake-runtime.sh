#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# prebake-runtime.sh · CI 每平台 build 前烘焙 cpython + uv 到 src-tauri/resources/
#
# 历史背景:
#   早期把 mac-arm64 的 cpython 直接 commit 进 git (326MB · 4700 文件)
#   → Intel mac / Windows build 出来后没法跑 (架构不对)
#   → 双端"安装失败" 90% 起源
#
# 新方案:
#   .gitignore 排掉 resources/{runtime,bin} (不再 commit)
#   CI 每平台 runner 跑此脚本下载平台对应的 cpython + uv → 解到 resources/
#   tauri-action 接着 bundle 进 .app/.exe · 用户首启 bootstrap_bundled 拷到
#   ~/.qianshou/runtime/ · 跟旧路径完全一致
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

case "$PLATFORM" in
  macos-arm64)
    PY_TRIPLE="aarch64-apple-darwin"
    UV_TRIPLE="aarch64-apple-darwin"
    UV_ARCHIVE_EXT="tar.gz"
    UV_BIN_NAME="uv-aarch64-apple-darwin"
    PLATFORM_LABEL="macos-aarch64"
    ;;
  macos-intel)
    PY_TRIPLE="x86_64-apple-darwin"
    UV_TRIPLE="x86_64-apple-darwin"
    UV_ARCHIVE_EXT="tar.gz"
    UV_BIN_NAME="uv-x86_64-apple-darwin"
    PLATFORM_LABEL="macos-x86_64"
    ;;
  windows-x64)
    PY_TRIPLE="x86_64-pc-windows-msvc-shared"
    UV_TRIPLE="x86_64-pc-windows-msvc"
    UV_ARCHIVE_EXT="zip"
    UV_BIN_NAME="uv.exe"
    PLATFORM_LABEL="windows-x86_64"
    ;;
  *)
    echo "unknown platform: $PLATFORM (expected: macos-arm64 | macos-intel | windows-x64)" >&2
    exit 2
    ;;
esac

DEST_RESOURCES="src-tauri/resources"
DEST_CPYTHON_DIR="$DEST_RESOURCES/runtime/cpython"
DEST_BIN_DIR="$DEST_RESOURCES/bin"
mkdir -p "$DEST_CPYTHON_DIR" "$DEST_BIN_DIR"

# 清旧文件 (CI 上一般干净 · 本机重跑可能有残留)
rm -rf "$DEST_CPYTHON_DIR"/cpython-* "$DEST_BIN_DIR"/uv-* "$DEST_BIN_DIR"/uv.exe 2>/dev/null || true

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

# ─── 2. 下 uv ───────────────────────────────────────────────────────────────
UV_URL="https://github.com/astral-sh/uv/releases/download/${UV_VERSION}/uv-${UV_TRIPLE}.${UV_ARCHIVE_EXT}"
echo "[2/3] 下 uv"
echo "      URL: $UV_URL"
TMP_UV=$(mktemp -d)

if [[ "$UV_ARCHIVE_EXT" == "tar.gz" ]]; then
    curl -fL --retry 3 --connect-timeout 30 --max-time 300 -o "$TMP_UV/uv.tar.gz" "$UV_URL"
    tar -xzf "$TMP_UV/uv.tar.gz" -C "$TMP_UV"
    # archive 内: uv-<triple>/uv (或直接 uv)
    if [[ -f "$TMP_UV/uv-${UV_TRIPLE}/uv" ]]; then
        cp "$TMP_UV/uv-${UV_TRIPLE}/uv" "$DEST_BIN_DIR/$UV_BIN_NAME"
    elif [[ -f "$TMP_UV/uv" ]]; then
        cp "$TMP_UV/uv" "$DEST_BIN_DIR/$UV_BIN_NAME"
    else
        echo "ERROR: uv binary not found in archive" >&2
        ls -la "$TMP_UV" >&2
        exit 3
    fi
    chmod +x "$DEST_BIN_DIR/$UV_BIN_NAME"
else
    # windows · zip
    curl -fL --retry 3 --connect-timeout 30 --max-time 300 -o "$TMP_UV/uv.zip" "$UV_URL"
    # GH Win runner 默认无 unzip · 用 powershell Expand-Archive 兜
    if command -v unzip >/dev/null 2>&1; then
        unzip -q -o "$TMP_UV/uv.zip" -d "$TMP_UV"
    else
        powershell -NoProfile -Command "Expand-Archive -Force '$TMP_UV/uv.zip' '$TMP_UV'"
    fi
    # uv windows zip · 通常含 uv.exe 直接 (~v0.5+)
    if [[ -f "$TMP_UV/uv.exe" ]]; then
        cp "$TMP_UV/uv.exe" "$DEST_BIN_DIR/$UV_BIN_NAME"
    elif [[ -f "$TMP_UV/uv-${UV_TRIPLE}/uv.exe" ]]; then
        cp "$TMP_UV/uv-${UV_TRIPLE}/uv.exe" "$DEST_BIN_DIR/$UV_BIN_NAME"
    else
        echo "ERROR: uv.exe not found in zip" >&2
        ls -la "$TMP_UV" >&2
        exit 3
    fi
fi
rm -rf "$TMP_UV"
echo "      → $DEST_BIN_DIR/$UV_BIN_NAME"

# ─── 3. 烘焙 lite + crawl venv (双端内置 · 0 装机网络依赖) ──────────────
# 用户痛点: 客户首启装 lite/crawl tier 时 pip install 失败 (国内 mirror 超时 ·
# Windows 缺 VC++ build tools 编译 lxml 失败 · 30% 客户卡死弃用)
# 解法: CI build 时把 venv 烘焙进 bundle · 节点 bootstrap_bundled 直接拷 · 0 装
DEST_VENVS_DIR="$DEST_RESOURCES/runtime/venvs"
mkdir -p "$DEST_VENVS_DIR"

if [[ "$PLATFORM" == "windows-x64" ]]; then
    PY_BIN_REL="python.exe"
    VENV_PY_REL="Scripts/python.exe"
else
    PY_BIN_REL="bin/python3.11"
    VENV_PY_REL="bin/python"
fi
HOST_PY="$CPYTHON_TARGET/$PY_BIN_REL"

# uv 路径 (跨平台)
UV_BIN_PATH="$DEST_BIN_DIR/$UV_BIN_NAME"
[[ -f "$HOST_PY" ]] || { echo "ERROR: host python 不存在: $HOST_PY" >&2; exit 5; }
[[ -f "$UV_BIN_PATH" ]] || { echo "ERROR: uv binary 不存在: $UV_BIN_PATH" >&2; exit 5; }
chmod +x "$UV_BIN_PATH" || true

# tier 列表 + 包 + 自检 (跟 backend bundles.py auto_install tier 对齐)
prebake_tier() {
    local tier="$1"
    local packages="$2"
    local verify="$3"
    local venv_path="$DEST_VENVS_DIR/$tier"
    
    echo "[3/3] 烘焙 $tier venv (packages: $packages)"
    rm -rf "$venv_path"
    
    # 用 host cpython 创 venv
    "$HOST_PY" -m venv "$venv_path"
    
    local venv_py="$venv_path/$VENV_PY_REL"
    [[ -f "$venv_py" ]] || { echo "ERROR: venv python 不存在: $venv_py" >&2; return 1; }
    
    # uv pip install · 用阿里云镜像 (CI 任意地点都能拉到)
    "$UV_BIN_PATH" pip install \
        --python "$venv_py" \
        --index-url https://mirrors.aliyun.com/pypi/simple \
        --allow-insecure-host mirrors.aliyun.com \
        $packages || {
        echo "ERROR: uv pip install 失败 (tier=$tier)" >&2
        return 2
    }
    
    # 自检
    "$venv_py" -c "$verify" || {
        echo "ERROR: $tier 自检失败" >&2
        return 3
    }
    
    local size=$(du -sh "$venv_path" | cut -f1)
    echo "      ✓ $tier venv 烘焙 OK · size=$size"
}

# lite tier (auto_install=true · 算力业务必装)
prebake_tier "lite" \
    "pillow numpy onnxruntime PyMuPDF pdfplumber" \
    "import PIL, numpy, onnxruntime, fitz, pdfplumber; print('lite ok')"

# crawl tier (auto_install=true · 爬虫 + GEO 业务必装)
prebake_tier "crawl" \
    "requests selectolax tldextract readability-lxml lxml" \
    "import requests, selectolax, tldextract; from readability import Document; print('crawl ok')"

# ─── 4. 写 manifest.json (bootstrap_bundled 用这个判平台是否匹配 + 含 venvs 标记) ──
cat > "$DEST_RESOURCES/runtime/manifest.json" << EOF
{
  "schema_version": 2,
  "bundled_at": "$(date -u +%Y-%m-%d)",
  "python": "${PY_VERSION}",
  "uv": "${UV_VERSION}",
  "platform": "${PLATFORM_LABEL}",
  "bundled_venvs": ["lite", "crawl"],
  "note": "lite+crawl venv 已内置 · 节点首启 0 装 0 网络"
}
EOF
echo "[4/4] 写 $DEST_RESOURCES/runtime/manifest.json (platform=$PLATFORM_LABEL · bundled_venvs=[lite,crawl])"

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

TOTAL=$(du -sh "$DEST_RESOURCES" 2>/dev/null | cut -f1)
echo ""
echo "✅ prebake $PLATFORM 完成 · resources/ 总大小 = $TOTAL"
echo "   python:   $EXPECTED_PY_BIN"
echo "   uv:       $DEST_BIN_DIR/$UV_BIN_NAME"
echo "   manifest: $DEST_RESOURCES/runtime/manifest.json"
