"""Make bank debug trace calls resolve only in the public/global namespace.

Usage: python3 scripts/patch_bank_trace.py original.swf patched.swf
No game code, scores or network data are changed. Game assets stay local.
"""
import struct
import sys
import zlib
from pathlib import Path


def enc(n):
    b = bytearray()
    while n > 127:
        b.append((n & 127) | 128)
        n >>= 7
    b.append(n)
    return bytes(b)


class Reader:
    def __init__(self, data):
        self.data, self.pos = data, 4  # ABC minor/major version

    def uint(self):
        n = 0
        for shift in range(0, 35, 7):
            b = self.data[self.pos]
            self.pos += 1
            n |= (b & 127) << shift
            if not b & 128:
                return n
        raise ValueError('Invalid variable integer')


def patch_abc(data):
    r = Reader(data)
    for _ in range(2):
        for _ in range(max(0, r.uint() - 1)):
            r.uint()
    double_count = r.uint()
    r.pos += max(0, double_count - 1) * 8
    strings = [b'']
    for _ in range(max(0, r.uint() - 1)):
        size = r.uint()
        strings.append(data[r.pos:r.pos + size])
        r.pos += size
    namespaces = [None]
    for _ in range(max(0, r.uint() - 1)):
        kind = data[r.pos]
        r.pos += 1
        namespaces.append((kind, r.uint()))
    public = next(i for i, ns in enumerate(namespaces) if ns and ns[0] == 0x16 and strings[ns[1]] == b'')
    sets_start = r.pos
    count = r.uint()
    sets_body = r.pos
    sets = [None]
    for _ in range(count - 1):
        sets.append([r.uint() for _ in range(r.uint())])
    sets_end = r.pos
    # Append a new namespace set, leaving every existing pool index unchanged.
    target = count
    edits = []
    for _ in range(max(0, r.uint() - 1)):
        kind = data[r.pos]
        r.pos += 1
        if kind in (0x07, 0x0D):
            r.uint(); r.uint()
        elif kind in (0x09, 0x0E):
            name = r.uint()
            start = r.pos
            nsset = r.uint()
            if strings[name] == b'trace' and any(
                strings[namespaces[n][1]] == b'flash.system' for n in sets[nsset]
            ):
                edits.append((start, r.pos, enc(target)))
        elif kind in (0x0F, 0x10, 0x1B, 0x1C):
            r.uint()
        elif kind in (0x11, 0x12):
            pass
        elif kind == 0x1D:
            r.uint()
            for _ in range(r.uint()):
                r.uint()
        else:
            raise ValueError(f'Unsupported multiname kind {kind:x}')
    if not edits:
        return data, 0
    edits.append((sets_start, sets_end, enc(count + 1) + data[sets_body:sets_end] + enc(1) + enc(public)))
    for start, end, replacement in sorted(edits, reverse=True):
        data = data[:start] + replacement + data[end:]
    return data, len(edits) - 1


def patch_swf(raw):
    if raw[:3] == b'CWS':
        body = zlib.decompress(raw[8:])
    elif raw[:3] == b'FWS':
        body = raw[8:]
    else:
        raise ValueError('Expected FWS or CWS')
    nbits = body[0] >> 3
    pos = (5 + nbits * 4 + 7) // 8 + 4
    prefix, tags, changed = body[:pos], [], 0
    while pos < len(body):
        tag_start = pos
        header = struct.unpack_from('<H', body, pos)[0]
        pos += 2
        code, size = header >> 6, header & 63
        if size == 63:
            size = struct.unpack_from('<I', body, pos)[0]
            pos += 4
        payload = body[pos:pos + size]
        pos += size
        if code == 82:  # DoABC: flags + null-terminated name + ABC
            abc_start = payload.index(b'\0', 4) + 1
            abc, n = patch_abc(payload[abc_start:])
            payload = payload[:abc_start] + abc
            changed += n
        if payload == body[pos - size:pos]:
            tags.append(body[tag_start:pos])
        else:
            tags.append(struct.pack('<HI', (code << 6) | 63, len(payload)) + payload)
        if code == 0:
            break
    if not changed:
        raise ValueError('No ambiguous bank trace references found; refusing patch')
    body = prefix + b''.join(tags)
    return b'CWS' + raw[3:4] + struct.pack('<I', len(body) + 8) + zlib.compress(body), changed


if __name__ == '__main__':
    source, target = map(Path, sys.argv[1:])
    result, count = patch_swf(source.read_bytes())
    target.write_bytes(result)
    print(f'Patched {count} trace multiname reference(s): {target}')
