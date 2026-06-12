#!/usr/bin/env bash
# ════════════════════════════════════════════════════════════════
# 千手节点客户端 8.2.0 · release build 一键脚本
# ════════════════════════════════════════════════════════════════
# 用户环境前置:
#   macOS:   Xcode CLT · Rust 1.83+ · Node 20+
#            Apple Developer ID 证书已 keychain · 或在 tauri.conf.json 配 signingIdentity
#   Windows: VS Build Tools 2022 · Rust msvc · Node 20+
#            (可选) 代码签名证书 cert.pfx + 设 TAURI_SIGNING_PRIVATE_KEY 等
#
# 用法:
#   bash scripts/build-release-8.2.0.sh           # 默认: onnx feature 开 · 打 dmg/msi
#   ONNX=0 bash scripts/build-release-8.2.0.sh    # 关 onnx · 老路径兼容版
#   bash scripts/build-release-8.2.0.sh --no-bundle  # 只编译不打包

set -euo pipefail
cd "$(dirname "$0")/.."

ONNX="${ONNX:-1}"
FEATURES=""
if [[ "$ONNX" == "1" ]]; then
    FEATURES="--features onnx"
    echo "▶ ONNX feature 已开(完整 V8.2)"
else
    FEATURES=""
    echo "▶ ONNX feature 关闭(老路径兼容版)"
fi

# ─── 0. 校验版本一致 ───
PKG_VER=$(node -p "require('./package.json').version")
CONF_VER=$(node -p "require('./src-tauri/tauri.conf.json').version")
CARGO_VER=$(grep '^version' src-tauri/Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
echo "  package.json: $PKG_VER"
echo "  tauri.conf.json: $CONF_VER"
echo "  Cargo.toml: $CARGO_VER"
if [[ "$PKG_VER" != "$CONF_VER" || "$PKG_VER" != "$CARGO_VER" ]]; then
    echo "✗ 版本号不一致 · 修了再来"
    exit 1
fi
echo "✓ 版本一致 $PKG_VER"

# ─── 1. 前端 ───
echo "── 前端 build ──"
npm ci --silent 2>&1 | tail -3 || npm install --silent 2>&1 | tail -3
npm run build

# ─── 2. Tauri build ───
echo "── Tauri build $* ──"
npm run tauri -- build $FEATURES "$@"

# ─── 3. 输出位置 ───
echo
echo "── 完成 ──"
echo "Binary:  src-tauri/target/release/qianshou-client"
echo "Bundle:  src-tauri/target/release/bundle/  (--no-bundle 跳过时无)"
echo
echo "签名提示(可选):"
echo "  macOS:   codesign --deep --force --verify --verbose --sign \"Developer ID Application: XXX\" \\"
echo "             \"src-tauri/target/release/bundle/macos/千手节点.app\""
echo "  Windows: signtool sign /f cert.pfx /p \$CERT_PASS /tr http://timestamp.digicert.com \\"
echo "             \"src-tauri/target/release/bundle/msi/千手节点_8.2.0_x64_en-US.msi\""
echo
echo "公证(macOS · 可选):"
echo "  xcrun notarytool submit \"千手节点.app.zip\" --keychain-profile AC_PROFILE --wait"
