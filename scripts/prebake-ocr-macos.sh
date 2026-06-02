#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# prebake-ocr-macos.sh · 8.1.6 · 把可重定位 tesseract + dylib + tessdata 烘焙进 mac .app
#
# 背景(与 Win OCR 同源缺陷):
#   ocr-tools-v1 的原生依赖是 tesseract 二进制。Win 走 prebake-ocr-windows.sh,
#   mac 此前依赖系统/镜像 tesseract → 同属 OCR 依赖缺陷。本脚本把 Homebrew 的
#   tesseract 连同其全部非系统 dylib 拷进 src-tauri/resources/ocr/,把 dylib 引用
#   全部重写成 @executable_path/libs/(可重定位),再做 ad-hoc 签名,落地后客户端
#   bootstrap ensure_bundled_ocr(unix 分支已就绪)注册 ocr tier,executor 注入
#   PATH + TESSDATA_PREFIX → pytesseract 调 tesseract 开箱即用,0 装机 0 网络。
#
# 仅 arm64 mac 构建调用(GH runner 为 arm64,装的是 arm64 tesseract;Intel 包
# 若塞 arm64 二进制会跑不动 → Intel 不内置,静默回退系统/镜像 tesseract → 零回归)。
#
# 安全门控(确保零回归):
#   1. 重定位后 ./tesseract --version 自检;失败则删 manifest.json → 客户端静默回退
#   2. 整脚本任何环节失败都不让 CI 挂(调用方用 `|| true`),只是不产出内置 OCR
#
# 用法(CI mac arm64 job · tauri build 之前):
#   bash scripts/prebake-ocr-macos.sh
#
# 可配置:
#   TESSDATA_BASE  tessdata 语言文件 URL 前缀(补 chi_sim · 默认 tessdata_fast)
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OCR_DEST="$ROOT/src-tauri/resources/ocr"
TESS_DIR="$OCR_DEST/tesseract"
LIBS_DIR="$TESS_DIR/libs"
TESSDATA_DIR="$OCR_DEST/tessdata"

TESSDATA_BASE="${TESSDATA_BASE:-https://github.com/tesseract-ocr/tessdata_fast/raw/main}"
LANGS=("eng" "chi_sim")

ARCH="$(uname -m)"   # arm64 / x86_64
echo "=========================================="
echo " prebake-ocr-macos.sh · 8.1.6 · arch=$ARCH"
echo " dest: $OCR_DEST"
echo "=========================================="

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "ERROR: 仅 macOS 可跑本脚本" >&2; exit 1
fi

# ── 0. 准备 brew tesseract ────────────────────────────────────────────────────
if ! command -v brew &>/dev/null; then
  echo "ERROR: 无 Homebrew" >&2; exit 2
fi
if ! brew list tesseract &>/dev/null; then
  echo "[0/5] brew install tesseract"
  brew install tesseract
fi
BREW_TESS_PREFIX="$(brew --prefix tesseract)"
SRC_TESS="$BREW_TESS_PREFIX/bin/tesseract"
[[ -f "$SRC_TESS" ]] || { echo "ERROR: 未找到 $SRC_TESS" >&2; exit 3; }

rm -rf "$OCR_DEST"
mkdir -p "$TESS_DIR" "$LIBS_DIR" "$TESSDATA_DIR"

# ── 1. 拷 tesseract 主二进制 ──────────────────────────────────────────────────
echo "[1/5] 拷 tesseract: $SRC_TESS"
cp "$SRC_TESS" "$TESS_DIR/tesseract"
chmod 0755 "$TESS_DIR/tesseract"

# ── 2. BFS 重定位全部非系统 dylib → @executable_path/libs/ ────────────────────
# 系统库(/usr/lib /System)保持原样不拷;其余(/opt/homebrew /usr/local/Cellar 等)
# 递归拷进 libs/ 并把所有引用 install_name_tool 改成 @executable_path/libs/<basename>。
echo "[2/5] 重定位 dylib(@executable_path/libs/)"
is_system_lib() { [[ "$1" == /usr/lib/* || "$1" == /System/* ]]; }

declare -A COPIED=()      # 绝对路径 → 已处理标记
QUEUE=("$TESS_DIR/tesseract")

# 先把主二进制对自身的引用清掉(可执行无需 -id)
process_refs() {
  local target="$1"
  # otool -L 第一行是文件名自身,跳过;dylib 第二行常是其 LC_ID
  local deps
  deps="$(otool -L "$target" 2>/dev/null | tail -n +2 | awk '{print $1}')"
  while IFS= read -r dep; do
    [[ -z "$dep" ]] && continue
    # 跳过已是 @executable_path / @loader_path / @rpath 的(理论上重写后会出现)
    [[ "$dep" == @* ]] && continue
    is_system_lib "$dep" && continue
    local base; base="$(basename "$dep")"
    local dest_lib="$LIBS_DIR/$base"
    # 把引用改成可重定位路径
    install_name_tool -change "$dep" "@executable_path/libs/$base" "$target" 2>/dev/null || true
    # 拷贝依赖本体(若尚未拷)并入队递归
    if [[ -z "${COPIED[$dep]:-}" ]]; then
      COPIED["$dep"]=1
      if [[ -f "$dep" ]]; then
        cp -f "$dep" "$dest_lib" 2>/dev/null || true
        chmod 0644 "$dest_lib" 2>/dev/null || true
        install_name_tool -id "@executable_path/libs/$base" "$dest_lib" 2>/dev/null || true
        QUEUE+=("$dest_lib")
      else
        echo "      WARN: 依赖缺失(忽略): $dep" >&2
      fi
    fi
  done <<< "$deps"
}

# BFS
idx=0
while (( idx < ${#QUEUE[@]} )); do
  process_refs "${QUEUE[$idx]}"
  idx=$((idx+1))
done
echo "      libs: $(ls -1 "$LIBS_DIR" 2>/dev/null | wc -l | tr -d ' ') 个 · $(du -sh "$LIBS_DIR" 2>/dev/null | cut -f1)"

# ── 3. tessdata(eng 用 brew 自带,chi_sim 补) ───────────────────────────────
echo "[3/5] 备 tessdata: ${LANGS[*]}"
BREW_TESSDATA="$BREW_TESS_PREFIX/share/tessdata"
if [[ -d "$BREW_TESSDATA" ]]; then
  cp -a "$BREW_TESSDATA/." "$TESSDATA_DIR/" 2>/dev/null || true
fi
for L in "${LANGS[@]}"; do
  if [[ -f "$TESSDATA_DIR/$L.traineddata" ]]; then
    echo "      ✓ $L(已自带)"
  else
    echo "      下 $TESSDATA_BASE/$L.traineddata"
    curl -fL --retry 3 --connect-timeout 30 --max-time 600 \
      -o "$TESSDATA_DIR/$L.traineddata" "$TESSDATA_BASE/$L.traineddata"
  fi
done
# 去掉无关大语种 · 仅留我们声明的(瘦身)
for f in "$TESSDATA_DIR"/*.traineddata; do
  [[ -e "$f" ]] || continue
  keep=0; b="$(basename "$f" .traineddata)"
  for L in "${LANGS[@]}" osd; do [[ "$b" == "$L" ]] && keep=1; done
  (( keep == 0 )) && rm -f "$f"
done
echo "      → $TESSDATA_DIR ($(du -sh "$TESSDATA_DIR" 2>/dev/null | cut -f1))"

# ── 4. ad-hoc 重新签名(install_name_tool 改过后必须重签,否则 arm64 被杀) ──────
echo "[4/5] ad-hoc codesign"
for f in "$LIBS_DIR"/*.dylib; do
  [[ -e "$f" ]] && codesign --force --timestamp=none -s - "$f" 2>/dev/null || true
done
codesign --force --timestamp=none -s - "$TESS_DIR/tesseract" 2>/dev/null || true

# ── 5. 自检 + 写 manifest ─────────────────────────────────────────────────────
echo "[5/5] 自检 ./tesseract --version"
if ! TESSDATA_PREFIX="$OCR_DEST" "$TESS_DIR/tesseract" --version >/tmp/_tess_ver 2>&1; then
  echo "ERROR: 重定位后的 tesseract 无法运行 · 自检失败 · 取消内置(客户端静默回退)" >&2
  sed 's/^/      /' /tmp/_tess_ver >&2 || true
  rm -f "$OCR_DEST/manifest.json"
  exit 0   # 不让 CI 挂 · 仅放弃内置
fi
sed 's/^/      /' /tmp/_tess_ver | head -3

cat > "$OCR_DEST/manifest.json" << EOF
{
  "schema_version": 1,
  "tier": "ocr",
  "platform": "macos-$ARCH",
  "bundled_at": "$(date -u +%Y-%m-%d)",
  "engine": "tesseract",
  "tesseract_exe": "tesseract/tesseract",
  "tessdata_dir": "tessdata",
  "languages": ["eng", "chi_sim"],
  "software": ["tesseract", "pytesseract", "pillow"],
  "note": "macOS 内置 OCR · 可重定位 tesseract(@executable_path/libs) · ad-hoc 签名 · executor 注入 PATH + TESSDATA_PREFIX"
}
EOF

echo ""
echo "✅ prebake-ocr-macos 完成 · resources/ocr 总大小 = $(du -sh "$OCR_DEST" | cut -f1)"
echo "   tesseract: $TESS_DIR/tesseract (+libs/)"
echo "   tessdata:  $TESSDATA_DIR (${LANGS[*]})"
echo "   manifest:  $OCR_DEST/manifest.json"
