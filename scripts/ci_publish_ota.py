#!/usr/bin/env python3
# ─────────────────────────────────────────────────────────────────────────────
# ci_publish_ota.py · 8.1.6 · 在【服务器】上跑:分发客户端产物 + 更新 OTA binary.json +
#                     更新下载页 release.json。由 build-tauri.yml 的 publish-ota job
#                     scp 到服务器后 ssh 执行(本机 SSH 被 VPN 阻断,故走 CI runner)。
#
# 逻辑对齐已验证过 8.1.0–8.1.5 的 scripts/publish-from-release.sh,额外:
#   - binary.json 增 notes 字段
#   - 同步更新下载页 release.json(version/notes + 各平台 name/url/size/sha256/available)
#   - 改前自动 .bak 备份(可回滚)
#
# 入参(env):
#   VERSION    必填,如 8.1.6
#   NOTES_B64  base64(更新说明)· 规避中文/空格在 ssh 链里的转义坑
#   SRC_DIR    必填,解压后的产物目录(含 tauri-action 命名的 8 个文件)
#   DL_DIR     下载页目录,默认 /var/www/web/downloads/latest
#   OTA_BASE   OTA 根,默认 /var/www/qianshou-app/client-v3(下含 binary.json + binary/<ver>/)
#
# 安全:签名直接取 .sig 文件内容;客户端 updater 校签,签名不符只会"不升级"而非"装坏"。
# ─────────────────────────────────────────────────────────────────────────────
import base64
import hashlib
import json
import os
import shutil
import urllib.parse
from datetime import datetime, timezone

VER = os.environ["VERSION"]
NOTES = base64.b64decode(os.environ.get("NOTES_B64", "")).decode("utf-8") or VER
SRC = os.environ["SRC_DIR"]
DL = os.environ.get("DL_DIR", "/var/www/web/downloads/latest")
OTA_BASE = os.environ.get("OTA_BASE", "/var/www/qianshou-app/client-v3")
OTA = os.path.join(OTA_BASE, "binary", VER)
BINJSON = os.path.join(OTA_BASE, "binary.json")
RELJSON = os.path.join(DL, "release.json")
WUJI_BASE = f"https://www.wujisuanli.com/app/client-v3/binary/{VER}"

# tauri-action 命名(与 publish-from-release.sh EXPECT_FILES 一致)
ARM_DMG = f"千手节点_{VER}_aarch64.dmg"
ARM_APPGZ = f"千手节点_{VER}_aarch64.app.tar.gz"
INTEL_DMG = f"千手节点_{VER}_x64.dmg"
INTEL_APPGZ = f"千手节点_{VER}_x64.app.tar.gz"
WIN_EXE = f"千手节点_{VER}_x64-setup.exe"


def chown_nginx(p):
    try:
        shutil.chown(p, "nginx", "nginx")
    except Exception:
        pass


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for blk in iter(lambda: f.read(1 << 20), b""):
            h.update(blk)
    return h.hexdigest()


def src(name):
    return os.path.join(SRC, name)


def have(name):
    return os.path.isfile(src(name))


def readsig(name):
    p = src(name + ".sig")
    return open(p).read().strip() if os.path.isfile(p) else ""


print(f"== ci_publish_ota v{VER} · SRC={SRC} ==")
print("staged:", sorted(os.listdir(SRC)))

os.makedirs(OTA, exist_ok=True)
chown_nginx(OTA)
os.makedirs(DL, exist_ok=True)

# ── 1. 分发:下载页(安装器) + OTA 目录(payload + sig) ───────────────────────
moves = []
for n in (ARM_DMG, INTEL_DMG, WIN_EXE):           # 下载页 = 安装器
    if have(n):
        moves.append((n, DL))
for n in (WIN_EXE, ARM_APPGZ, INTEL_APPGZ):       # OTA = payload + sig
    if have(n):
        moves.append((n, OTA))
    if os.path.isfile(src(n + ".sig")):
        moves.append((n + ".sig", OTA))
for name, dst in moves:
    d = os.path.join(dst, name)
    shutil.copy(src(name), d)
    chown_nginx(d)
    print(f"  dist {d} ({os.path.getsize(d)} B)")

# ── 2. OTA binary.json(Tauri updater 清单) ──────────────────────────────────
if os.path.isfile(BINJSON):
    shutil.copy(BINJSON, BINJSON + f".bak.before-{VER}")
    d = json.load(open(BINJSON))
else:
    d = {}
d.setdefault("platforms", {})


def plat(url_name, sig):
    return {"url": f"{WUJI_BASE}/{urllib.parse.quote(url_name)}", "signature": sig}


win_sig, arm_sig, intel_sig = readsig(WIN_EXE), readsig(ARM_APPGZ), readsig(INTEL_APPGZ)
if win_sig:
    d["platforms"]["windows-x86_64"] = plat(WIN_EXE, win_sig)
if arm_sig:
    d["platforms"]["darwin-aarch64"] = plat(ARM_APPGZ, arm_sig)
if intel_sig:
    d["platforms"]["darwin-x86_64"] = plat(INTEL_APPGZ, intel_sig)
d["version"] = VER
d["pub_date"] = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
d["notes"] = NOTES
if "windows-x86_64" in d["platforms"]:            # 老客户端兜底读顶级字段
    d["url"] = d["platforms"]["windows-x86_64"]["url"]
    d["signature"] = d["platforms"]["windows-x86_64"]["signature"]
json.dump(d, open(BINJSON, "w"), ensure_ascii=False, indent=2)
chown_nginx(BINJSON)
print(f"✓ binary.json → v{VER} · platforms={list(d['platforms'])}")

# ── 3. 下载页 release.json ────────────────────────────────────────────────────
NAME_BY_PLATFORM = {"macos-arm64": ARM_DMG, "macos-intel": INTEL_DMG, "windows-x64": WIN_EXE}
if os.path.isfile(RELJSON):
    shutil.copy(RELJSON, RELJSON + f".bak.before-{VER}")
    r = json.load(open(RELJSON))
    today = datetime.now(timezone.utc).strftime("%Y-%m-%d")
    r["version"] = VER
    r["release_date"] = today
    r["release_notes"] = NOTES
    for prod in r.get("products", []):
        if prod.get("id") != "qianshou-standard":   # 只动千手节点客户端产品
            continue
        prod["version"] = VER
        prod["release_date"] = today
        for dl in prod.get("downloads", []):
            nm = NAME_BY_PLATFORM.get(dl.get("platform"))
            if not nm:
                continue
            fp = os.path.join(DL, nm)
            if os.path.isfile(fp):
                dl["name"] = nm
                dl["url"] = f"/downloads/latest/{nm}"
                dl["size_mb"] = round(os.path.getsize(fp) / 1048576, 2)
                dl["sha256"] = sha256(fp)
                dl["available"] = True
                dl.pop("unavailable_reason", None)
                print(f"  release.json {dl['platform']} → {nm} ({dl['size_mb']}MB)")
    json.dump(r, open(RELJSON, "w"), ensure_ascii=False, indent=2)
    chown_nginx(RELJSON)
    print(f"✓ release.json → v{VER}")
else:
    print(f"WARN: release.json 不存在 {RELJSON} · 跳过下载页更新")

print("✓ ci_publish_ota 完成")
