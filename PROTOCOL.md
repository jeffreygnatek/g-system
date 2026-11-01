# TC Electronic G-System — protocol notes

Everything below was derived from files, not from a live unit. Items marked
**confirmed** were verified against 100 factory presets (checksums, round-trip
encode). Items marked **inferred** come from the Vyzor editor's recorded
transmit/receive scripts or the firmware updater and still need a MIDI capture
from a real G-System to confirm.

## Transport

- **5-pin MIDI DIN** — standard. Any class-compliant USB-MIDI interface works
  (the original firmware updater offers this path too).
- **USB** — the G-System's USB port is a **USB HID device, not USB-MIDI**. The
  Mac updater (`SWUpdater`, JUCE, PPC/i386) uses `IOHIDDevice` with a vendor /
  product ID (`HIDUSBInterfaceClass`, `ComHidPort`). SysEx is tunneled inside
  HID reports. Report framing is unknown; it would need a USB capture from the
  old editor in a VM. Not needed if you use DIN. **inferred**

## SysEx framing (**confirmed**)

```
F0 00 20 1F <dev> 55 <type> <payload…> F7
```

| field | value |
|---|---|
| manufacturer | `00 20 1F` TC Electronic |
| `<dev>` | `00` = this unit, `7F` = any (editor uses both) |
| model | `55` G-System (G-Major is `48`) |

### Message types seen

| type | length | meaning | status |
|---|---|---|---|
| `20 01 00 <n>` + 1032 nibbles + chk | 1044 | **preset dump** for slot `n` | confirmed |
| `22 01 00 <n> 00 00 00 00 00 01` | 17 | select / request preset `n` (editor sends this, then waits 2000 ms) | inferred |
| `22 05 …` | 71 / 304 / 1355 | global / system data (MIDI map, routing, expression) | inferred |
| `22 0D …` / `22 0E …` | 239 / 108 | routing / loop tables (pattern `0C 08 0C 08 00 00` repeats) | inferred |
| `47 <block> <param> <v…>` | 13 | **real-time parameter edit**; blocks seen 01,02,04,07–0E | inferred |
| `57 00` / `57 01` | 9 | edit mode off / on (sent before and after a batch of `47`s) | inferred |
| `51 01` | 9 | firmware: enter bootloader | inferred |
| `52 …` | 305 | firmware data block (1330 of them = the 3.00 image) | inferred |

### Preset dump payload (**confirmed**)

- 516 bytes, transmitted as 1032 nibble bytes: for each data byte, `hi` then `lo`.
- Checksum = `sum(nibble bytes) & 0x7F`. Verified on all 100 factory presets.
- Byte 0–19: name, ≤20 chars, NUL terminated. Bytes after the NUL are **not**
  cleared by the editor (garbage), byte 20 is always `00`.
- Byte 21–515: 165 little-endian signed int24 parameters. Observed values look
  like dB (`-20`), ms (`500`), percent (`50`), enums (`0..15`).
- 130 of the 165 slots vary across the factory set; the rest are constant
  (probably reserved/unused or always-default).
- Slot number ↔ display: `BB-P` where `BB = n / 5`, `P = n % 5 + 1`.
  0–99 factory (banks 00–19), 100–199 user (banks 20–39).
- The header preset number in the file's first ten slots was `0x64..0x6D`
  (100–109) even though they sit in factory slots 0–9. Unknown whether the unit
  cares; the editor rewrites it on send. **open question**

### Parameter map (TODO)

Not yet done. Method: diff pairs of factory presets that differ in one obvious
way (e.g. "Whammy 2 Oct Up" vs "Whammy 1 Oct Down") and name the int24 slots
against the manual's effect list. `gsys json` emits every preset's params for
this.

## Vyzor file formats (**confirmed**)

IFF-style, big-endian lengths. `.SQS` = `LIST` of `FORM`s; `.Prs` / `.Ebf` =
one `FORM`. FORM declared lengths are a few dozen bytes **short** of the real
content — don't clamp chunk data to the parent length.

```
FORM <len> "BANK"
  SHDR 370   "TCE G-System" at +0, document kind at +16 ("Preset", "Edit Buffer", "User Bank")
  COMM FIND AUTH CPRT SONG MSCN BAND LNAM   text fields (256 or 51200 bytes)
  FILE n     original file name
  TRAN n     transmit script (bytecode)
  RECV n     receive script
  XTRA BDEF TDEF AUDI   misc
  BODY n     "User Bank": 200 × 1310-byte slots, each a 1044-byte preset dump
             zero-padded. Slots ≥ 10 have the 10-byte SysEx header zeroed.
             "Preset": one 1310-byte slot holding a 22 0D routing message.
```

### TRAN bytecode (partial)

`08 <u32 len> <bytes>` send SysEx · `06 <u32 ms>` delay · `0E`, `1D`, `21`,
`2E`, `0C`, `19` — variable / loop ops, not decoded. The scripts show the
editor's sequence for a preset: `22 01` select → 2000 ms → `57 00` → 2000 ms →
many `47` edits → `57 01`.

## Open questions (need a unit + MIDI monitor)

1. Does `22 01 00 <n> …` make the unit emit a `20` dump, or is a separate
   request message needed?
2. Does the unit ack a received `20` dump? With what?
3. Exact `47` encoding: value width, and the block→effect, param→name maps.
4. Whether the header preset number must match the target slot on send.
5. Global dump (`22 05`) layout.
