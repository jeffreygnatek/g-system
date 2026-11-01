# G-System Editor

A modern, cross-platform (macOS / Linux / Windows) librarian and editor for the
TC Electronic G-System, replacing the 2008 Vyzor-based editor that no longer
runs on current operating systems.

Status: **offline librarian works; MIDI transport is written but untested on
hardware.** See `PROTOCOL.md` for what is known and what still needs a live
capture.

```
crates/gsystem-protocol   SysEx codec, preset/bank model, .SQS/.Prs/.syx parsers (no I/O deps)
cli/                      `gsys` command-line tool (librarian + MIDI via midir)
app/                      Tauri 2 desktop app (Vite + TypeScript UI, Rust backend)
testdata/                 Factory_Set.SQS (100 factory presets), Twilight.Prs
tools/                    gsys_preset_decode.py — the Python prototype of the codec
```

## Installing

Download from the [Releases](https://github.com/jeffreygnatek/g-system/releases) page:

- **macOS** — `.dmg` (Apple Silicon or Intel). The build is not signed: the
  first time, right-click the app and choose **Open**, or run
  `xattr -dr com.apple.quarantine "/Applications/G-System Editor.app"`.
- **Linux** — `.deb` or `.AppImage`.
- **Windows** — `.msi`.
- `gsys-*` archives are the command-line tool on its own.

Releases are built by GitHub Actions when a `v*` tag is pushed
(`git tag v0.1.0 && git push origin v0.1.0`) and appear as a draft to
publish by hand. Set the `APPLE_*` secrets named in
`.github/workflows/release.yml` to get signed and notarized macOS builds.

## Build

Requirements: Rust (stable), Node 20+. On Linux also ALSA headers
(`libasound2-dev`) for MIDI and the usual Tauri deps (`libwebkit2gtk-4.1-dev`,
`libgtk-3-dev`, `librsvg2-dev`, `libayatana-appindicator3-dev`).

```sh
cargo test                                   # protocol tests against the factory set
cargo run -p gsys -- list testdata/Factory_Set.SQS

cd app && npm install
npm run tauri dev                            # desktop app (dev)
npm run tauri build                          # .app / .deb / .msi
```

## CLI

```
gsys list <file>                 slots and names from .SQS / .syx / .Prs
gsys export <in.SQS> <out.syx>   plain SysEx dump of all occupied slots
gsys json <file>                 names + 165 int24 params per preset
gsys scripts <file.Prs>          SysEx the old editor sent/expected for that preset
gsys rename <in.syx> <slot> "<name>" <out.syx>

gsys ports
gsys monitor <in-port>
gsys request <in-port> <out-port> <slot>
gsys backup  <in-port> <out-port> <out.syx> [from] [to]
gsys send    <in-port> <out-port> <file.syx>
gsys raw     <out-port> "F0 00 20 1F 00 55 57 01 F7"
```

Port names match by substring (`gsys ports` shows them). Slots are 0-based:
0–99 factory, 100–199 user.

## First session with the unit

1. Connect a USB-MIDI interface to the G-System's MIDI IN and OUT.
2. `gsys ports`, then `gsys monitor "<in>"` in one terminal.
3. Change presets on the unit with the footswitches; note what, if anything,
   it sends.
4. `gsys request "<in>" "<out>" 100` — this sends the editor's `22 01` select
   message and waits for a `20` dump. If nothing comes back, try
   `gsys raw "<out>" "F0 00 20 1F 00 55 57 00 F7"` first (edit mode) and watch
   the monitor; the answers to the open questions in `PROTOCOL.md` fall out of
   this session.
5. Once `request` works, `gsys backup … user.syx 100 199` backs up the user bank.

## License

MIT. Not affiliated with TC Electronic / Music Tribe.
