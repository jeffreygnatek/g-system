# TC Electronic G-System — protocol notes

Sources, and how much to trust each line:

- **[ours]** derived here from files: the Vyzor `.SQS`/`.Prs` banks, the 3.00
  firmware updater, and the Vyzor quick-start manual's screenshots. Verified
  by tests in `crates/gsystem-protocol/tests`.
- **[DA]** the independent
  [davidavalanches/gsystem-editor](https://github.com/davidavalanches/gsystem-editor)
  project (GPL-3.0, © David Avalanches), tested against a real unit. Its
  parameter tables, levels and tempo code are ported to Rust here
  (`params.rs`, `levels.rs`, `tempo.rs`, from commit 9b50ef8), with credit in
  each file. Their project is
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

### Field map

- Field 0: packed routing, boost, boost level, relay switch, loops 1–4, insert,
  relay settings A/B; field 2: left/right output levels **[DA]** →
  `crates/gsystem-protocol/src/levels.rs`.
- Fields 3–124: eight effect blocks in fixed slots, in storage order Comp (3),
  Filter (13), Mod (29), Pitch (44), Reverb (64), Gate (80), Delay (86),
  EQ (114). Each is `[count][type][params…]`, padded with -1 **[ours, DA]**.
  Which parameters follow the type depends on it; the full tables (names,
  ranges, value lists, from firmware 4.02) are in
  `crates/gsystem-protocol/src/params.rs`, ported from DA's `params.ts`.
- Fields 125–164: modifier/pedal assignments (not ported yet).

Older presets store fewer parameters than the 4.02 tables list (the count
field says how many; e.g. "Scotty's Blues" Auto Wah has 11, without AutoOn).
`Block::stored_fields` respects that **[ours]**.

Frequencies are a sixth-octave index (`Hz = 20 · 2^(i/6)`), modulation speeds a
1/24-octave index from 0.05 Hz, EQ width `(i + 1) / 10` octaves.

Verification: `parameter_tables_match_vyzor_screenshot` reads every value
Vyzor's manual shows for "Scotty's Blues" through the ported tables. That
cross-checks our independently derived field positions against DA's firmware
tables, and they agree. It also caught a one-step error in the original EQ
width display, which is fixed in the port. `tools/xcheck` diffs the port
against the original TypeScript: 0 differences.

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

- Not yet ported from DA: modifiers (fields 125–164), footswitch assignments,
  globals, NovaDrive. All exist in their code.
- A store sequence that doesn't risk freezing the unit.
