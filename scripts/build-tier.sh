#!/usr/bin/env bash
# build-tier.sh · 一键构建 Runtime Tier venv tarball
#
# 用法:
#   ./build-tier.sh lite macos-arm64 "pillow numpy onnxruntime PyMuPDF pdfplumber"
#   ./build-tier.sh crawl linux-x86_64 "requests selectolax tldextract readability-lxml lxml"
#   ./build-tier.sh ocr macos-arm64 "paddleocr paddlepaddle"
#   ./build-tier.sh speech linux-x86_64 "faster-whisper"
#   ./build-tier.sh ffmpeg macos-arm64 "imageio-ffmpeg"
#
# 参数:
#   $1 = tier name (lite/crawl/ffmpeg/ocr/speech/vision-ai/render)
#   $2 = platform (macos-arm64/macos-x86_64/linux-x86_64/windows-x86_64)
#   $3 = 空格分隔的 pip 包名列表
#
# 可选环境变量:
#   CPYTHON_TARBALL = 本地 cpython tarball 路径 (没传就自动下)
#   UPLOAD = true     自动上传到 by.wujisuanli.com
#   DRY_RUN = true    只构建不上传

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TIER="$1"
PLATFORM="$2"
PACKAGES="$3"

if [ -z "$TIER" ] || [ -z "$PLATFORM" ]; then
    echo "用法: $0 <tier> <platform> <packages>"
    echo "示例: $0 lite macos-arm64 'pillow numpy onnxruntime'"
    exit 1
fi

echo "=========================================="
echo " build-tier.sh · $TIER · $PLATFORM"
echo " packages: $PACKAGES"
echo "=========================================="

WORK="$SCRIPT_DIR/_build/$PLATFORM-$TIER"
rm -rf "$WORK"
mkdir -p "$WORK"

# ── 1. 找到或下载 cpython ──────────────────────────────────
CPYTHON="$SCRIPT_DIR/_runtime/$PLATFORM"
if [ -n "${CPYTHON_TARBALL:-}" ] && [ -f "$CPYTHON_TARBALL" ]; then
    echo "▶ 使用本地 cpython tarball: $CPYTHON_TARBALL"
    mkdir -p "$CPYTHON"
    tar xzf "$CPYTHON_TARBALL" -C "$CPYTHON"
else
    if [ -x "$CPYTHON/bin/python3" ]; then
        echo "✓ cpython 已就绪: $CPYTHON/bin/python3"
    else
        echo "⚠ cpython 未就绪 · 请先运行 prebake-runtime.sh 或设置 CPYTHON_TARBALL"
        exit 1
    fi
fi

PYTHON_BIN="$CPYTHON/bin/python3"
if [ ! -x "$PYTHON_BIN" ]; then
    echo "✗ 找不到 python3: $PYTHON_BIN"
    exit 1
fi
echo "  python: $($PYTHON_BIN --version)"

# ── 2. 创建 venv ───────────────────────────────────────────
VENV="$WORK/venv"
echo "▶ 创建 venv: $VENV"
"$PYTHON_BIN" -m venv --clear "$VENV"
VENV_PY="$VENV/bin/python"

# ── 3. 升级 pip + 安装包 ──────────────────────────────────
echo "▶ pip install: $PACKAGES"
"$VENV_PY" -m pip install --upgrade pip setuptools wheel -q
# 国内加速 · 走阿里云镜像
"$VENV_PY" -m pip install \
    -i https://mirrors.aliyun.com/pypi/simple \
    --trusted-host mirrors.aliyun.com \
    $PACKAGES

echo "  installed:"
"$VENV_PY" -m pip list --format=columns 2>/dev/null | head -20

# ── 4. 清理 venv 无用文件 · 减小 tarball ──────────────────
echo "▶ 清理 venv 缓存…"
find "$VENV" -type d -name "__pycache__" -exec rm -rf {} + 2>/dev/null || true
find "$VENV" -type f -name "*.pyc" -delete 2>/dev/null || true
find "$VENV" -type f -name "*.pyo" -delete 2>/dev/null || true
rm -rf "$VENV"/share/doc 2>/dev/null || true
rm -rf "$VENV"/share/man 2>/dev/null || true

# ── 5. 打包 tarball ────────────────────────────────────────
OUTDIR="$SCRIPT_DIR/_output/$PLATFORM"
mkdir -p "$OUTDIR"
TARBALL="$OUTDIR/$TIER.tar.gz"
echo "▶ 打包: $TARBALL"
cd "$WORK"
tar czf "$TARBALL" "venv"
cd "$SCRIPT_DIR"

SIZE_MB=$(du -m "$TARBALL" | cut -f1)
echo "  大小: ${SIZE_MB} MB"

# ── 6. SHA256 ──────────────────────────────────────────────
if command -v shasum &>/dev/null; then
    SHA256=$(shasum -a 256 "$TARBALL" | awk '{print $1}')
elif command -v sha256sum &>/dev/null; then
    SHA256=$(sha256sum "$TARBALL" | awk '{print $1}')
else
    SHA256="TBD"
    echo "⚠ 没有 shasum/sha256sum · SHA256 未计算"
fi
echo "  sha256: $SHA256"

# ── 7. 输出摘要 ────────────────────────────────────────────
echo ""
echo "══════════════════════════════════════════════════"
echo " ✓ build-tier.sh · $TIER · $PLATFORM · 完成"
echo ""
echo "   文件: $TARBALL"
echo "   大小: ${SIZE_MB} MB"
echo "   SHA256: $SHA256"
echo ""
echo "   上传到 by.wujisuanli.com 后, 在 Admin 面板填入:"
echo "     prebuilt_url: https://by.wujisuanli.com/v1/$PLATFORM/$TIER.tar.gz"
echo "     prebuilt_sha256: $SHA256"
echo "     prebuilt_size_mb: $SIZE_MB"
echo "══════════════════════════════════════════════════"

# ── 8. 上传 (可选) ──────────────────────────────────────────
if [ "${UPLOAD:-}" = "true" ] && [ "${DRY_RUN:-}" != "true" ]; then
    REMOTE="qianshou@47.243.39.24"
    REMOTE_PATH="/var/www/qianshou-runtime/v1/$PLATFORM/$TIER.tar.gz"
    echo "▶ 上传到: $REMOTE:$REMOTE_PATH"
    scp "$TARBALL" "$REMOTE:$REMOTE_PATH"
    echo "✓ 上传完成 · https://by.wujisuanli.com/v1/$PLATFORM/$TIER.tar.gz"
elif [ "${DRY_RUN:-}" = "true" ]; then
    echo "⏭ DRY_RUN = true · 跳过上传"
fi