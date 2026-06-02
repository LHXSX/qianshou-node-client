#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# prebake-ocr-windows.sh · 8.1.6 · 把便携版 tesseract + tessdata 烘焙进 Windows 安装包
#
# 背景 (Win OCR 依赖缺陷):
#   skills/ocr-tools-v1 的 Python 依赖只有 pytesseract + Pillow(各平台 pip 可装),
#   真正缺的是原生 `tesseract.exe` —— Windows 节点无内置,manifest 的 Win 安装提示
#   还是让用户去 UB-Mannheim 手动下载。本脚本把便携 tesseract + 中英 tessdata 落到
#   src-tauri/resources/ocr/,由 tauri.conf.json 的 bundle.resources `resources/ocr/**/*`
#   打进安装包;客户端首启 bootstrap_bundled 拷到 ~/.qianshou/runtime/tiers/ocr/ 并注册
#   ocr tier(software=tesseract+pytesseract),executor 注入 PATH + TESSDATA_PREFIX。
#
# 仅 Windows 构建调用(macOS/Linux 走镜像 tier 或系统 tesseract)。
# 门控: 只有 resources/ocr/ 存在客户端才激活 OCR 内置,缺失则静默走老路径 → 零回归。
#
# 用法 (CI Windows job · tauri build 之前):
#   bash scripts/prebake-ocr-windows.sh
#
# 可配置环境变量(三种来源,优先级:本地已装 > 自家镜像 > 公共源):
#   TESS_LOCAL_DIR  已装 tesseract 目录(含 tesseract.exe + DLL,如 choco 装的
#                   "C:\Program Files\Tesseract-OCR")· CI 首选,最稳
#   TESS_PKG_URL    便携 tesseract zip/tar.gz URL(无 TESS_LOCAL_DIR 时用)
#   TESS_PKG_SHA256 上述包 sha256(强校验,留空跳过)
#   TESSDATA_BASE   tessdata 语言文件目录 URL 前缀(取 eng / chi_sim .traineddata)
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OCR_DEST="$ROOT/src-tauri/resources/ocr"
TESS_DIR="$OCR_DEST/tesseract"
TESSDATA_DIR="$OCR_DEST/tessdata"

# 自家镜像默认路径(团队在 by.wujisuanli.com 放置) · 可被环境变量覆盖
MIRROR_BASE="https://by.wujisuanli.com/v1/windows-x86_64/ocr"
TESS_PKG_URL="${TESS_PKG_URL:-$MIRROR_BASE/tesseract-portable-win64.tar.gz}"
TESS_PKG_SHA256="${TESS_PKG_SHA256:-}"
TESSDATA_BASE="${TESSDATA_BASE:-https://github.com/tesseract-ocr/tessdata_fast/raw/main}"
LANGS=("eng" "chi_sim")

echo "=========================================="
echo " prebake-ocr-windows.sh · 8.1.6"
echo " dest: $OCR_DEST"
echo "=========================================="

rm -rf "$OCR_DEST"
mkdir -p "$TESS_DIR" "$TESSDATA_DIR"

# ── 1. tesseract(tesseract.exe + DLL) · 本地已装优先,否则下便携包 ────────────
if [[ -n "${TESS_LOCAL_DIR:-}" && -f "$TESS_LOCAL_DIR/tesseract.exe" ]]; then
  echo "[1/3] 复制已装 tesseract: $TESS_LOCAL_DIR"
  cp -a "$TESS_LOCAL_DIR/." "$TESS_DIR/"
else
  echo "[1/3] 下 tesseract 便携包"
  echo "      URL: $TESS_PKG_URL"
  TMP="$(mktemp -d)"
  PKG="$TMP/tess_pkg"
  if ! curl -fL --retry 3 --connect-timeout 30 --max-time 600 -o "$PKG" "$TESS_PKG_URL"; then
    echo "ERROR: tesseract 便携包下载失败 · 设 TESS_LOCAL_DIR(已装目录)或 TESS_PKG_URL" >&2
    exit 5
  fi
  if [[ -n "$TESS_PKG_SHA256" ]]; then
    echo "      校验 sha256…"
    if command -v sha256sum &>/dev/null; then GOT="$(sha256sum "$PKG" | awk '{print $1}')";
    else GOT="$(shasum -a 256 "$PKG" | awk '{print $1}')"; fi
    if [[ "$GOT" != "$TESS_PKG_SHA256" ]]; then
      echo "ERROR: sha256 不匹配 · 期望 $TESS_PKG_SHA256 · 实得 $GOT" >&2; exit 6
    fi
    echo "      ✓ sha256 OK"
  fi
  mkdir -p "$TMP/unp"
  if file "$PKG" 2>/dev/null | grep -qi zip || [[ "$TESS_PKG_URL" == *.zip ]]; then
    unzip -o -q "$PKG" -d "$TMP/unp"
  else
    tar -xzf "$PKG" -C "$TMP/unp"
  fi
  TESS_EXE_DIR="$(dirname "$(find "$TMP/unp" -type f -iname 'tesseract.exe' | head -1)")"
  if [[ -z "$TESS_EXE_DIR" || ! -f "$TESS_EXE_DIR/tesseract.exe" ]]; then
    echo "ERROR: 便携包内未找到 tesseract.exe" >&2
    find "$TMP/unp" -maxdepth 3 -type f | head -40 >&2 || true
    exit 7
  fi
  cp -a "$TESS_EXE_DIR/." "$TESS_DIR/"
  rm -rf "$TMP"
fi
echo "      → $TESS_DIR ($(du -sh "$TESS_DIR" 2>/dev/null | cut -f1))"

# ── 2. tessdata 语言文件(eng + chi_sim) · 先用已装自带,缺的从公共源补 ─────────
echo "[2/3] 备 tessdata: ${LANGS[*]}"
# 已装目录常自带 tessdata(至少 eng)· 先 seed 进来,再删 tesseract/ 下的副本(去重瘦身)
if [[ -d "$TESS_DIR/tessdata" ]]; then
  cp -a "$TESS_DIR/tessdata/." "$TESSDATA_DIR/" 2>/dev/null || true
  rm -rf "$TESS_DIR/tessdata"
fi
for L in "${LANGS[@]}"; do
  if [[ -f "$TESSDATA_DIR/$L.traineddata" ]]; then
    echo "      ✓ $L(已自带)"
  else
    URL="$TESSDATA_BASE/$L.traineddata"
    echo "      下 $URL"
    curl -fL --retry 3 --connect-timeout 30 --max-time 600 -o "$TESSDATA_DIR/$L.traineddata" "$URL"
  fi
done
echo "      → $TESSDATA_DIR ($(du -sh "$TESSDATA_DIR" 2>/dev/null | cut -f1))"

# ── 3. 写 ocr/manifest.json(bootstrap 据此注册 ocr tier) ─────────────────────
cat > "$OCR_DEST/manifest.json" << EOF
{
  "schema_version": 1,
  "tier": "ocr",
  "platform": "windows-x86_64",
  "bundled_at": "$(date -u +%Y-%m-%d)",
  "engine": "tesseract",
  "tesseract_exe": "tesseract/tesseract.exe",
  "tessdata_dir": "tessdata",
  "languages": ["eng", "chi_sim"],
  "software": ["tesseract", "pytesseract", "pillow"],
  "note": "Win 内置 OCR · 免装依赖 · executor 注入 PATH + TESSDATA_PREFIX"
}
EOF

# ── 校验 ──────────────────────────────────────────────────────────────────────
[[ -f "$TESS_DIR/tesseract.exe" ]] || { echo "WARN: tesseract.exe 缺失" >&2; exit 4; }
for L in "${LANGS[@]}"; do
  [[ -f "$TESSDATA_DIR/$L.traineddata" ]] || { echo "WARN: 缺 $L.traineddata" >&2; exit 4; }
done

echo ""
echo "✅ prebake-ocr-windows 完成 · resources/ocr 总大小 = $(du -sh "$OCR_DEST" | cut -f1)"
echo "   tesseract: $TESS_DIR/tesseract.exe"
echo "   tessdata:  $TESSDATA_DIR (${LANGS[*]})"
echo "   manifest:  $OCR_DEST/manifest.json"
