# TC Electronic G-System — protocol notes

Sources, and how much to trust each line:

- **[ours]** derived here from files: the Vyzor `.SQS`/`.Prs` banks, the 3.00
  firmware updater, and the Vyzor quick-start manual's screenshots. Verified
  by tests in `crates/gsystem-protocol/tests`.
- **[DA]** hardware observations published by the independent
  [davidavalanches/gsystem-editor](https://github.com/davidavalanches/gsystem-editor)
  project (GPL-3.0), which has been tested against a real unit. We
  reimplemented these facts; no code or tables were copied. Their project is
  further along than ours. If you just want a working editor, use theirs:
  <https://davidavalanches.github.io/gsystem-editor/>.

## Transport

- **USB**: the unit's own USB port appears as a **MIDI device** on modern
  macOS, Linux and Android **[DA]**. Our earlier guess that it was a HID-only
  device (from class names in the 2008 updater binary) was wrong.
- **5-pin MIDI** through any USB-MIDI interface also works.

## SysEx framing [ours, DA]

```
F0 00 20 1F <dev> 55 <type> <payload…> F7
```

`00 20 1F` = TC Electronic, `55` = G-System (G-Major is `48`). `<dev>` is the
unit's SysEx ID (Global › MIDI) or `7F` for any device.

| type | payload | meaning | source |
|---|---|---|---|
| `20` | `01 <num:2> <1032 nibbles> <chk>` | preset dump (1044 bytes total) | ours, DA |
| `45` | `01 <num:2>` | request a stored preset; answered by a `20` dump in ~0.77 s. Empty slots never answer; the unit ignores requests while busy | DA |
| `28` | `<num:2> 00 01` | the unit's answer for a damaged slot | DA |
| `22` | `<block> <param:2> <value:6 nibbles>…` | set an edit-buffer parameter (signed 24-bit, MSB nibble first). The unit sends these too when a switch or knob is used. Writes need device `7F` | DA; framing ours |
| `47` | `<block> <first:2> <count:2>` | request parameters; answered with `22` messages (how Vyzor reads globals) | DA; seen in Vyzor's `.Prs` scripts |
| `57` | `00` / `01` | begin / end a preset store | DA; seen in Vyzor's scripts |
| `51`, `52` | | firmware updater: bootloader control, 305-byte image blocks | ours |
| — | `F0 7E 7F 06 01 F7` | Universal Device Inquiry, answered with the firmware version | DA |

`<num:2>` and `<param:2>` are 14-bit values, high 7 bits first. **User slot
`n` (0..200) is wire preset number `n + 100`.** The 200 user slots are banks
00–39; the factory bank (0..99) is read-only on the unit. The `22 01 00 1D …
01` message at the start of every Vyzor script is "editor mode on" (block 1,
param `0x1D`). We first misread it as a preset request.

## Preset dump payload [ours; confirmed by DA]

- 516 bytes, sent as 1032 nibble bytes (high nibble first). Checksum =
  `sum(nibbles) & 0x7F`. Verified on all 100 factory presets.
- Bytes 0–19: name. Bytes after the NUL aren't cleared. DA reports that the unit
  sets bit 7 on some name bytes; preserve it. Byte 20 is always `00`.
- Bytes 21–515: **165 fields, signed 24-bit little-endian.**

### Field map [ours]

Fields 0–2 hold a header (not decoded). Then eight effect blocks sit in fixed
slots in storage order. Each is stored as `[count][type][on/off][params…]`,
padded with `-1`:

| block | count field | slot length | type | on/off | notes |
|---|---|---|---|---|---|
| Comp | 3 | 10 | 4 | 5 | 6 threshold, 7 ratio, 8 attack, 9 release, **10 gain, 11 drive, 12 response** |
| Filter | 13 | 16 | 14 | 15 | params depend on type (count 11–15) |
| Mod | 29 | 15 | 30 | 31 | params depend on type (count 11–14) |
| Pitch | 44 | 20 | 45 | 46 | **51/52 delay L/R ms, 57 mix**, 62/63 detune voices |
| Reverb | 64 | 16 | 65 | 66 | 67 decay (×0.1 s), 68 pre-delay, 69 shape, 70 size, 71 hi color, 72 hi level, 73 lo color, 74 lo level, 75 early, 76 level, 77 diffusion, 78 mix, 79 out |
| Gate | 80 | 6 | — | 81 | 82 mode (Hard/Soft), 83 threshold, 84 damping, 85 release |
| Delay | 86 | 28 | 87 | 88 | 89 time ms, 96 feedback, 102 FB high cut, 103 FB low cut, 106 mix |
| EQ | 114 | 11 | — | 124 | 115/118/121 gain, 116/119/122 freq, 117/120/123 width |

Frequencies are a sixth-octave index: `Hz = 20 · 2^(i/6)`. EQ width is
`(i + 1) / 10` octaves. Every bold or listed field was checked against the
values Vyzor's manual shows for "Scotty's Blues" (51/51 match; see
`block_map_matches_vyzor_screenshot`). Type lists follow the order in Vyzor's
menus. Fields 125–164 hold packed levels, routing, loop/insert/boost switches
and assignments; these aren't decoded yet.

## Storing presets — read this first

DA captured Vyzor's Sync and found that a store is:

1. editor mode on (`22 01 00 1D … 01`),
2. `57 00`,
3. the `20` dump,
4. `57 01`,

all addressed to the unit's own SysEx ID, with about 100 ms between messages.

**That exact sequence has frozen a unit intermittently.** The freeze needs a power cycle,
leaves the slot unreadable, and has once altered a neighbouring preset **[DA]**. So:

- the desktop app **does not write to the unit**; edits stay in the file (save a `.syx`);
- `gsys store` implements the sequence for people who accept the risk.
  It requires `--sysex-id` and `--i-accept-the-risk`, and it reads the slot back to verify.

## Vyzor file formats [ours]

IFF-style, big-endian lengths. `.SQS` is a `LIST` of `FORM`s; `.Prs`/`.Ebf` is
one `FORM`. FORM lengths are a few dozen bytes short of the real content.

```
FORM <len> "BANK"
  SHDR 370   "TCE G-System" at +0, document kind at +16 ("Preset", "Edit Buffer", "User Bank")
  COMM FIND AUTH CPRT SONG MSCN BAND LNAM   text fields
  FILE n     original file name
  TRAN n     transmit script (bytecode: 08 <u32 len> <sysex> = send, 06 <u32 ms> = wait)
  RECV n     receive script
  BODY n     "User Bank": 200 × 1310-byte slots, each a 1044-byte dump zero-padded
             (slots ≥ 10 have the 10-byte header zeroed). "Preset": routing data.
```

## Still open

- Filter and Modulation per-type parameter positions; pitch voices for types other than Detune.
- Fields 0–2 and 125–164 (levels, routing, loops/insert/boost, switch and modifier assignments).
- Global (system) settings.
- A store sequence that doesn't risk freezing the unit.
