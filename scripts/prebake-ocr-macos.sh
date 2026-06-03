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
BREW_PREFIX="$(brew --prefix)"           # /opt/homebrew(arm) · 用于解析 @rpath 依赖
SRC_TESS="$BREW_TESS_PREFIX/bin/tesseract"
[[ -f "$SRC_TESS" ]] || { echo "ERROR: 未找到 $SRC_TESS" >&2; exit 3; }

rm -rf "$OCR_DEST"
mkdir -p "$TESS_DIR" "$LIBS_DIR" "$TESSDATA_DIR"

# ── 1. 拷 tesseract 主二进制 ──────────────────────────────────────────────────
echo "[1/5] 拷 tesseract: $SRC_TESS"
cp "$SRC_TESS" "$TESS_DIR/tesseract"
chmod 0755 "$TESS_DIR/tesseract"

# ── 2. 重定位全部非系统 dylib(主二进制→@executable_path/libs · 库间→@loader_path) ──
# 关键: Homebrew 部分库用 @rpath/libX.dylib 引用同伴(如 libwebp→@rpath/libsharpyuv),
# 绝不能跳过 @-前缀依赖,否则该库不被拷 → dyld "Library not loaded" → Abort trap:6。
# 故对 @rpath/@loader_path/@executable_path 依赖按 basename 到 Homebrew lib 目录解析后照拷。
echo "[2/5] 重定位 dylib(主→@executable_path/libs · 库间→@loader_path)"
# macOS 自带 /bin/bash 仍是 3.2(无 declare -A)· 用"dest 文件已存在"去重 · 索引数组做 BFS。

# 把依赖串解析成绝对源文件路径(系统库/解析不到 → 返回非 0)
resolve_dep_src() {
  local dep="$1" base d; base="$(basename "$dep")"
  case "$dep" in
    /usr/lib/*|/System/*) return 1 ;;                 # 系统库,保持原样不拷
    @*)                                                # @rpath/@loader_path/@executable_path
      for d in "$BREW_PREFIX"/lib "$BREW_PREFIX"/opt/*/lib; do
        [ -f "$d/$base" ] && { printf '%s\n' "$d/$base"; return 0; }
      done
      return 1 ;;
    *) [ -f "$dep" ] && { printf '%s\n' "$dep"; return 0; } || return 1 ;;
  esac
}

QUEUE=()

# 改写 target 的非系统依赖为可重定位路径并拷依赖本体入队。
# is_main=1 → 主二进制(@executable_path/libs/);=0 → 库(同目录 @loader_path/)。
process_refs() {
  local target="$1" is_main="$2"
  local deps dep base ref src dest_lib
  deps="$(otool -L "$target" 2>/dev/null | tail -n +2 | awk '{print $1}')"
  while IFS= read -r dep; do
    [ -z "$dep" ] && continue
    case "$dep" in /usr/lib/*|/System/*) continue ;; esac   # 系统库不动
    base="$(basename "$dep")"
    if [ "$is_main" = "1" ]; then ref="@executable_path/libs/$base"; else ref="@loader_path/$base"; fi
    install_name_tool -change "$dep" "$ref" "$target" 2>/dev/null || true
    dest_lib="$LIBS_DIR/$base"
    if [ ! -e "$dest_lib" ]; then                            # 去重 + 防环
      src="$(resolve_dep_src "$dep" || true)"
      if [ -n "$src" ]; then
        cp -f "$src" "$dest_lib" 2>/dev/null || true
        chmod 0644 "$dest_lib" 2>/dev/null || true
        install_name_tool -id "@loader_path/$base" "$dest_lib" 2>/dev/null || true
        QUEUE+=("$dest_lib")
      else
        echo "      WARN: 依赖解析不到(忽略): $dep" >&2
      fi
    fi
  done <<< "$deps"
}

process_refs "$TESS_DIR/tesseract" 1          # 主二进制 · 入队其依赖
idx=0
while (( idx < ${#QUEUE[@]} )); do            # BFS 处理所有库(含 @rpath 同伴)
  process_refs "${QUEUE[$idx]}" 0
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
