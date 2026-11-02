# TC Electronic G-System — protocol notes

Sources, and how much to trust each line:

- **[ours]** derived here from files: the Vyzor `.SQS`/`.Prs` banks, the 3.00
  firmware updater, and the Vyzor quick-start manual's screenshots. Verified
  by tests in `crates/gsystem-protocol/tests`.
- **[DA]** the independent
  [davidavalanches/gsystem-editor](https://github.com/davidavalanches/gsystem-editor)
  project (GPL-3.0, © David Avalanches), tested against a real unit. Its
  parameter tables, levels, tempo, modifiers, NovaDrive, switches, globals
  and empty-preset code are ported to Rust here (`params.rs`, `levels.rs`,
  `tempo.rs`, `modifiers.rs`, `novadrive.rs`, `switches.rs`, `globals.rs`,
  `empty.rs`, from commit 9b50ef8), with credit in each file. Their project is
  further along than ours. If you just want a working editor, use theirs:
  <https://davidavalanches.github.io/gsystem-editor/>.

## Transport

- **USB**: TC Electronic vendor `0x1220`, product `0x0005`, full speed. The
  configuration has a USB-MIDI *streaming* interface (class 1, subclass 3)
  but **no Audio Control interface**, which the USB-MIDI spec requires next
  to it **[ours, from a unit on macOS]**. Linux and Android accept it and
  create a MIDI port **[DA]**; **macOS doesn't attach its MIDI driver, so no
  MIDI port appears.** Since nothing claims the interface, the editor and
  `gsys` open it directly with libusb and exchange USB-MIDI 1.0 event packets
  (crate `gsystem-usb`; the port is shown as "G-System (USB)").
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
- Bytes 0–19: name. Bytes after the NUL aren't cleared. The unit sets bit 7 on
  some name bytes as a flag **[DA]**: the character is the low 7 bits
  ("[ÒHY]" is "[RHY]" with flags). Preserve the bits; mask them for display.
  Some community presets also show values like 128, 158 or 194 in parameters
  whose range is 0–1 or 0–60, i.e. a valid value with bit 7 set. It may be
  the same flag; unconfirmed, so `validate` still reports them.
  Byte 20 is always `00`.
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
- Fields 125–162: 38 modifier slots (pedal/encoder → parameter, min/mid/max %),
  grouped by block and following each effect type's controllable parameters
  **[DA]** → `modifiers.rs`. Fields 163–164: NovaDrive NDR-1 preset behind an
  extension header **[DA]** → `novadrive.rs`.

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

## Edit buffer (Live mode) [DA]

`55 22 <block> <param:2> <value:6 nibbles>` sent to device `7F` changes the
unit's edit buffer, exactly like turning a knob. Block ids: EQ 2, Mod 4,
Filter 7, Pitch 8, Delay 9, Comp 10, Reverb 11, Gate 12; levels 1 (routing 0,
boost 10, boost level 11, relay 16, relays A/B 17/18, loops 19–22, insert 23,
outputs 26/27); modifiers 13 (param 20 + slot); NovaDrive 14. A block's param
n is preset field `first_field + n`, type first. See `live.rs`.

## Storing presets — read this first

DA captured Vyzor's Sync and found that a store is:

1. editor mode on (`22 01 00 1D … 01`),
2. `57 00`,
3. the `20` dump,
4. `57 01`,

all addressed to the unit's own SysEx ID, with about 100 ms between messages.

**That exact sequence has frozen a unit intermittently.** The freeze needs a power cycle,
leaves the slot unreadable, and has once altered a neighbouring preset **[DA]**. So:

- the desktop app **does not store presets directly**. It writes only to the
  edit buffer (Audition / Live, `live.rs`); the user stores with the unit's own
  STORE button;
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
             (slots ≥ 10 have the 10-byte header zeroed). "Preset buffer" (.Prs): one
             such slot, header zeroed. The TRAN script builds the dump from it and
             computes the checksum when sending, so the stored checksum is often
             stale: don't use it to reject (see vyzor::decode_preset_buffer).
             "Edit Buffer" (.Ebf): 1246 bytes, the same dump without its last
             7 bytes (end of modifier slot 38 and the NovaDrive fields), then a
             checksum and usually F7 (vyzor::decode_edit_buffer).
```

## Global settings [DA]

Not stored in presets. Read with `55 47` requests over four ranges
(`globals::GLOBAL_RANGES`): block 1 params 0–29 (levels, locks), block 5
params 3–25 (utility, pedals, MIDI, tuner), block 13 param 58 (Volume Ctrl.),
block 5 params 226–243 (footswitches #1–#18, see `switches.rs`). The unit
answers with `22` messages. `gsys globals` and the app's Global screen use
this; both are read-only.

## Still open

- Program map (block 5, params 26–153): not decoded here or by DA yet.
- Writing global settings and footswitch assignments (DA does this with `22`
  messages; here they're read-only for now).
- A store sequence that doesn't risk freezing the unit.
