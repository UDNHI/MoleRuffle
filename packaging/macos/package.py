# -*- coding: utf-8 -*-
"""Create a single-architecture macOS application bundle from a built client (developer tool).

python3 packaging/macos/package.py --binary PATH --bank-original PATH \
    --bank-patched PATH --metadata PATH --out NEW_DIRECTORY
"""
import argparse
import json
import plistlib
import shutil
import subprocess
import sys
from pathlib import Path

p = argparse.ArgumentParser()
for name in ['binary', 'bank-original', 'bank-patched', 'metadata', 'out']:
    p.add_argument('--' + name, required=True, type=Path)
p.add_argument('--arch', choices=['arm64', 'x86_64'], default='arm64')
p.add_argument('--version', default='0.1.9')
p.add_argument('--build-version', default='2026102401')
a = p.parse_args()
actual = subprocess.check_output(['lipo', '-archs', str(a.binary)], text=True).strip()
if actual != a.arch:
    p.error(f'Binary architecture {actual!r} does not match {a.arch!r}')

repo = Path(__file__).resolve().parents[2]
pack = Path(__file__).resolve().parent
out = a.out.resolve()
out.mkdir(parents=True, exist_ok=False)
app_name = 'MoleRuffle 台版'
app = out / (app_name + '.app')
mac = app / 'Contents/MacOS'
res = app / 'Contents/Resources'
mac.mkdir(parents=True)
res.mkdir()
shutil.copy2(a.binary, mac / 'moleruffle')
subprocess.run(['strip', '-S', str(mac / 'moleruffle')], check=True)
for source, target in [(pack/'launcher.m', mac/'launcher'), (repo/'scripts/bank_patch_helper.c', mac/'bank-patch')]:
    subprocess.run(['xcrun', 'clang', '-arch', a.arch, '-mmacosx-version-min=11.0', '-O2', '-Wno-deprecated-declarations', str(source), '-fobjc-arc', '-framework', 'Cocoa', '-lz', '-o', str(target)], check=True)
subprocess.run([sys.executable, str(repo/'scripts/build_bank_delta.py'), str(a.bank_original), str(a.bank_patched), str(res/'bank.patch')], check=True)
shutil.copy2(pack/'launch.sh', res/'launch.sh')
shutil.copy2(pack/'AppIcon.icns', res/'AppIcon.icns')
info = {'CFBundleName':app_name, 'CFBundleDisplayName':app_name,
        'CFBundleIdentifier':'io.github.udnhi.moleruffle.taiwan.' + a.arch, 'CFBundleExecutable':'launcher',
        'CFBundlePackageType':'APPL', 'CFBundleVersion':a.build_version, 'CFBundleShortVersionString':a.version, 'CFBundleIconFile':'AppIcon.icns',
        'LSArchitecturePriority':[a.arch], 'LSMinimumSystemVersion':'11.0', 'NSHighResolutionCapable':True}
(app/'Contents/Info.plist').write_bytes(plistlib.dumps(info))
platform = 'Apple Silicon Mac（M 系列）' if a.arch == 'arm64' else 'Intel Mac'
guide = f'''{app_name} v{a.version} — {platform}

1. 完整解壓縮 ZIP，將「{app_name}.app」拖至「應用程式」。
2. 雙擊 App；首次啟動會自動準備必要資源，完成後進入遊戲。
3. 使用自己的台版摩爾莊園帳號登入。

不需要安裝 Rust、Java、Python 或 Flash。遊玩需要網路。
本包只支援 {platform}，請選擇符合處理器的下載檔案。
最低系統版本標記為 macOS 11；其他機型與系統版本的相容性仍待驗證。

此為以 MoleRuffle／Ruffle 為基礎的非官方台版客戶端，非淘米官方產品。
原專案：https://github.com/moleworld-dev/MoleRuffle
本包不包含完整遊戲資源、開發者帳號、快取或遊玩日誌。
遊戲內容仍由官方伺服器載入，相關權利歸原權利人所有。

本版包含台版連線、銀行入口、游標、中文字型與書籍排版相容性修正，
以及關閉流程與餐廳卡頓改善。程式使用台灣時區，不修改系統時區。
已知問題：拜訪別人餐廳再回自己餐廳後，服務生可能在端菜途中消失。
伊蓮小理財的排版暫未處理。本版仍可能有其他相容性問題。

啟動失敗或遊戲意外結束時，可使用「重試」「開啟日誌」「關閉」。
回報問題請提供操作步驟、地圖／小遊戲名稱，以及截圖或日誌；不要提供密碼。
日誌可能含遊戲錯誤網址，分享前請檢查內容。

App 有本機簽章，尚未經 Apple 公證。若 macOS 顯示安全警告，
請確認下載來源，再依系統提示處理；不需要停用系統安全設定。

為沿用既有設定，資料與日誌繼續使用下列位置：
~/Library/Application Support/MoleRuffle-Taiwan-Test/
請勿分享 SharedObjects（可能含記住的帳號）。
'''
(out/'開始使用.txt').write_text(guide, encoding='utf-8')
licenses = res/'Licenses'
licenses.mkdir()
for name in ['LICENSE-MIT', 'LICENSE-APACHE']:
    shutil.copy2(repo/name, licenses/name)
shutil.copy2(repo/'crates/moleruffle-core/assets/NOTO-OFL.txt', licenses/'Noto-Sans-TC-OFL.txt')
notices = ['MoleRuffle: MIT OR Apache-2.0', 'Ruffle pinned revision: 304a3c9dcaf42ed6d4b1c8bbd05f10d21f407c2e', 'Noto Sans TC: SIL Open Font License 1.1', '', 'Dependency notices (includes build/target-specific packages):']
for item in json.loads(a.metadata.read_text())['packages']:
    name = f"{item['name']}-{item['version']}"
    notices.append(f"{name}: {item.get('license') or 'see upstream license'}")
    parent = Path(item['manifest_path']).parent
    files = [f for f in parent.iterdir() if f.is_file() and f.name.lower().startswith(('license', 'copying', 'notice', 'copyright', 'licence'))]
    if item.get('license_file'):
        f = parent/item['license_file']
        if f.is_file() and f not in files:
            files.append(f)
    if files:
        folder = licenses/name
        folder.mkdir(exist_ok=True)
        for f in files:
            shutil.copy2(f, folder/f.name)
(licenses/'DEPENDENCIES.txt').write_text('\n'.join(notices)+'\n')
for f in sorted(mac.iterdir(), key=lambda f: f.name == 'launcher'):
    subprocess.run(['codesign', '--force', '--sign', '-', str(f)], check=True)
subprocess.run(['codesign', '--force', '--sign', '-', str(app)], check=True)
subprocess.run(['codesign', '--verify', '--deep', '--strict', str(app)], check=True)
print(out)
