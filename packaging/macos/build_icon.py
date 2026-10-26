"""Rebuild the macOS icon from draw_icon.m using Apple tools only."""
import argparse
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--preview", type=Path, help="Also save the 1024px PNG")
args = parser.parse_args()
folder = Path(__file__).resolve().parent
with tempfile.TemporaryDirectory(prefix="icon-build-", dir=folder) as temp:
    temp = Path(temp)
    env = dict(os.environ, TMPDIR=str(temp))
    subprocess.run(["xcrun", "clang", "-fobjc-arc", "-framework", "Cocoa",
                    str(folder/"draw_icon.m"), "-o", str(temp/"draw-icon")], check=True, env=env)
    preview = temp/"icon.png"
    subprocess.run([str(temp/"draw-icon"), str(preview)], check=True, env=env)
    # ICNS supports PNG payloads. Write standard icon chunks directly.
    chunks = []
    for tag, size in [(b"icp4",16), (b"icp5",32), (b"icp6",64),
                      (b"ic07",128), (b"ic08",256), (b"ic09",512),
                      (b"ic10",1024), (b"ic11",32), (b"ic12",64),
                      (b"ic13",256), (b"ic14",512)]:
        image = temp/(tag.decode()+".png")
        subprocess.run(["sips","-z",str(size),str(size),str(preview),"--out",str(image)],
                       check=True, env=env, stdout=subprocess.DEVNULL)
        data = image.read_bytes()
        chunks.append(tag + struct.pack(">I", len(data)+8) + data)
    data = b"".join(chunks)
    (folder/"AppIcon.icns").write_bytes(b"icns"+struct.pack(">I",len(data)+8)+data)
    if args.preview:
        shutil.copyfile(preview, args.preview)
