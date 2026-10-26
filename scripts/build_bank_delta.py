"""Build a small, hash-checked patch manifest; never package the source SWF."""
import difflib
import hashlib
import struct
import sys
import zlib
from pathlib import Path

source, patched, output = map(Path, sys.argv[1:])
a, b = source.read_bytes(), patched.read_bytes()
x, y = zlib.decompress(a[8:]), zlib.decompress(b[8:])
start = 0
while x[start] == y[start]:
    start += 1
end = 0
while x[-end-1] == y[-end-1]:
    end += 1
ops = []
for tag, i, j, k, l in difflib.SequenceMatcher(None, x[start:len(x)-end], y[start:len(y)-end], autojunk=False).get_opcodes():
    if tag != 'equal':
        ops.append((start+i, j-i, y[start+k:start+l]))
manifest = b'MBP1' + hashlib.sha256(a).digest() + hashlib.sha256(y).digest()
manifest += struct.pack('<III', len(x), len(y), len(ops))
for pos, remove, data in ops:
    manifest += struct.pack('<III', pos, remove, len(data)) + data
output.write_bytes(manifest)
print(f'{len(ops)} edits; {len(manifest)} bytes; no full game asset packaged')
