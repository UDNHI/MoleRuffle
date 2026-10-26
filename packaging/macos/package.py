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
notes = f'''{app_name} v{a.version}
適用：{platform}

本版更新
- 台版連線、銀行入口、游標與中文字型相容性調整。
- 改善書籍排版、關閉崩潰與餐廳卡頓。

已知問題
- 拜訪別人餐廳後返回自己餐廳，服務生可能在端菜時消失。
- 《伊蓮小理財》的排版尚未處理。

非官方客戶端，尚未經 Apple 公證。
'''
(out/'發布說明.txt').write_text(notes, encoding='utf-8')
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
