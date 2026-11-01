#!/usr/bin/env python3
"""Decode TC Electronic G-System preset dumps from Vyzor .SQS/.Prs/.Ebf files
or raw .syx captures.

Preset dump message (model 0x55, type 0x20):
  F0 00 20 1F <dev> 55 20 01 00 <preset#> <1032 nibbles> <chk> F7
  payload = 516 bytes, each sent as (hi nibble, lo nibble)
  chk     = sum(nibbles) & 0x7F
  payload[0:20]  preset name, NUL-terminated
  rest           parameters, mostly little-endian signed int24 (3 bytes each)

Usage: gsys_preset_decode.py FILE [FILE...]  -> prints JSON per preset
"""
import json, struct, sys

HDR = b"\xf0\x00\x20\x1f"
PAYLOAD_NIBBLES = 1032

def iter_dumps(b: bytes):
    """Yield (preset_no, nibble_bytes, chk) for every 0x20 dump in b.
    Handles bank slots where the editor zeroed the 10-byte header."""
    seen = set()
    # 1) real framed messages
    i = 0
    while (i := b.find(HDR, i)) >= 0:
        if b[i + 5] == 0x55 and b[i + 6] == 0x20 and b[i + 7] == 0x01:
            start = i + 10
            nib = b[start:start + PAYLOAD_NIBBLES]
            if len(nib) == PAYLOAD_NIBBLES and b[start + PAYLOAD_NIBBLES + 1] == 0xF7:
                seen.add(start)
                yield b[i + 9], nib, b[start + PAYLOAD_NIBBLES]
        i += 4
    # 2) headerless slots: find "<1032 nibbles><chk>F7" by scanning for F7
    #    preceded by 1033 bytes all < 0x80 and a 20-char nibble-encoded name
    j = 0
    while (j := b.find(b"\xf7", j)) >= 0:
        start = j - 1 - PAYLOAD_NIBBLES
        if start >= 0 and start not in seen:
            nib = b[start:j - 1]
            if all(x < 0x10 for x in nib[:40]) and (sum(nib) & 0x7F) == b[j - 1]:
                seen.add(start)
                yield None, nib, b[j - 1]
        j += 1

def decode(nib: bytes) -> dict:
    raw = bytes((nib[k] << 4) | nib[k + 1] for k in range(0, PAYLOAD_NIBBLES, 2))
    name = raw[:20].split(b"\0")[0].decode("latin1")
    # bytes 20..515: 496 bytes; treat as int24 LE triples from offset 21
    # (offset guess — adjust once field map is known)
    body = raw[20:]
    n = len(body) // 3
    params = [struct.unpack("<i", body[k*3:k*3+3] + (b"\xff" if body[k*3+2] & 0x80 else b"\0"))[0]
              for k in range(n)]
    return {"name": name, "raw_hex": raw.hex(), "int24_le": params}

def encode(raw: bytes, preset_no: int, dev: int = 0) -> bytes:
    assert len(raw) == PAYLOAD_NIBBLES // 2
    nib = bytearray()
    for x in raw:
        nib += bytes((x >> 4, x & 0xF))
    chk = sum(nib) & 0x7F
    return HDR + bytes((dev, 0x55, 0x20, 0x01, 0x00, preset_no)) + bytes(nib) + bytes((chk, 0xF7))

if __name__ == "__main__":
    out = []
    for path in sys.argv[1:]:
        b = open(path, "rb").read()
        for no, nib, chk in iter_dumps(b):
            d = decode(nib)
            d["file"] = path
            d["preset_no"] = no
            d["checksum_ok"] = (sum(nib) & 0x7F) == chk
            # round-trip sanity
            raw = bytes.fromhex(d["raw_hex"])
            assert encode(raw, no or 0)[10:-2] == nib
            out.append(d)
    print(json.dumps(out, indent=1))
