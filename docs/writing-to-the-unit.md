# Writing to the unit: status and what to try next

## Where things stand

| Path | Status | Risk |
|---|---|---|
| **Audition / Live** — send a preset or each edit to the unit's *edit buffer* with `55 22` parameter messages, then press **STORE** on the unit | **Done** (`gsys audition`, app: *Audition on unit*, *Live* switch) | Low: the edit buffer is temporary, and storing uses the firmware's own Store |
| **Direct store** — send a full `20` dump into a user slot, as Vyzor's Sync does | `gsys store` only, behind `--i-accept-the-risk`, with read-back verification. **Disabled in the app** | Has frozen a unit (see below) |
| **Global settings / footswitches** — write with `55 22` to blocks 1, 5, 13 | Not built (read-only for now) | Low to medium: the unit applies changes at once and permanently |

The edit-buffer messages, block ids and level parameters come from
davidavalanches/gsystem-editor, which captured them from the unit's own knobs
and switches. They're tested here against those captures
(`live::tests::matches_messages_captured_on_the_unit`).

## The freeze, and why it may be the unit rather than the protocol

davidavalanches/gsystem-editor reports that Vyzor's exact store sequence

1. editor mode on: `22 01 00 1D … 01`
2. `57 00` (begin)
3. the `20` dump
4. `57 01` (end)

all sent to the unit's own SysEx ID with ~100 ms between messages,
worked four times and then froze the unit on the next store with identical
bytes and timing. The freeze needed a power cycle, left the slot unreadable,
and once altered a neighbouring preset.

On that unit, **all the freezes happened in banks 27–29**, and **slots 27-3 to
28-4 answer as damaged** (`28` messages). Stores elsewhere worked and verified
on read-back. That points at failing flash memory on that unit, not at the
protocol. It's a hypothesis, not established.

## Things to try, in order

### 1. Use Live + STORE on the unit (now)

This is the safe way to put presets on the unit today. Load a preset with
*Audition* or edit in *Live* mode, then press **STORE** on the G-System and
choose the slot there.

### 2. Characterize your own unit (first session with hardware)

Read-only checks first:

```sh
gsys identify G-System
gsys backup G-System backup-$(date +%F).syx   # also reports any damaged slots
gsys globals G-System                          # note the SysEx ID
```

If `backup` reports no damaged slots, run a store soak test on one spare slot
(e.g. 39-5, slot 199) with the backup safe:

```sh
for i in $(seq 1 20); do
  gsys store G-System backup.syx 0 199 --sysex-id <ID> --i-accept-the-risk || break
done
```

Record each result. If something fails, note the iteration count, whether the
unit froze, and what `gsys request G-System 199` returns after a power cycle.
Then vary one thing at a time:

- the gap between messages: 100 ms → 300 ms → 1 s;
- the settle time before read-back: 1.5 s → 3 s;
- leaving editor mode afterwards (`22 01 00 1D … 00`). Vyzor's captured Sync
  doesn't send it; the CLI doesn't yet either;
- sending to `7F` vs the unit's SysEx ID.

Twenty clean stores in a row on a healthy unit would support the
bad-flash explanation. Share the results with davidavalanches/gsystem-editor.

### 3. Capture the unit's own MIDI dump

The firmware updater contains the strings "MIDI BANK DUMP" and "MIDI SYSTEM
DUMP", which suggests the Utility menu can send banks over MIDI. With
`gsys monitor G-System` running, trigger a bank dump from the unit's front
panel and save the output. Questions to answer:

- Does it use the same `20` messages, and which device ID and preset numbers?
- Is it bracketed by `57 00`/`57 01`, or by something else?
- Does the unit accept the same stream sent back (classic hardware restore)?
  Try with a single preset in a spare slot first.

If the unit restores its own dump format reliably, that becomes the store
path, since it's the format the firmware itself produces.

### 4. Only then: re-enable store in the app

Required guards before turning it back on:

- automatic full-bank backup before the first write of a session;
- one slot at a time, read back and compare, stop at the first mismatch;
- never write a slot that answers as damaged, or its neighbours;
- the pacing found to be reliable in step 2;
- opt-in setting, off by default;
- "Clear slot" writes an `empty::empty_preset` (the unit has no delete), never an
  invalid preset.

### 5. Writing global settings and footswitches

Same `55 22` messages as Live mode, to blocks 1, 5 and 13 (see
`globals.rs`, `switches.rs`). The unit applies them at once and keeps them, so
the UI needs an explicit confirm per change and a "read back" after each.
Lower risk than a preset store, but changes are permanent. Still needed: the
program map (block 5, params 26–153) is not decoded.
