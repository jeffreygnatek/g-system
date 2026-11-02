//! `gsys` — command-line librarian and MIDI tool for the G-System.
//!
//! File commands work offline. MIDI commands work over the G-System's own
//! USB port (it appears as a MIDI device) or a USB-MIDI interface on the
//! 5-pin jacks.

mod midi;

use gsystem_protocol::preset::slot_label;
use gsystem_protocol::{syx, vyzor, Bank};
use std::path::Path;

fn usage() -> ! {
    eprintln!(
        "gsys — TC Electronic G-System tool

File commands (offline):
  gsys list <file.SQS|file.syx>            list preset slots and names
  gsys export <in.SQS> <out.syx>            write occupied slots as a .syx dump
  gsys show <file> <slot>                   decode one preset: every block, type and parameter
  gsys json <file.SQS|file.syx>             dump presets as JSON (raw fields + decoded view)
  gsys scripts <file.Prs|.Ebf|.SQS>         show SysEx the editor's TRAN/RECV scripts send/expect
  gsys rename <in.syx> <slot> \"<name>\" <out.syx>
  gsys merge <out.syx> <file>…              combine banks into one, in order, skipping
                                            duplicate sounds (same parameters, any name)

MIDI commands (<port> matches by name, e.g. \"G-System\"; in and out may differ):
  gsys ports                                list MIDI in/out ports
  gsys identify <port> [<out-port>]         ask the unit for its firmware version
  gsys globals <port>                       read global settings and footswitch assignments
  gsys monitor <port>                       print every G-System message received
  gsys request <port> <slot>                read one user preset and print it
  gsys backup <port> <out.syx> [from] [to]  read user slots from..to (default all) into a .syx
  gsys audition <port> <file> <slot>        load a preset into the unit's edit buffer to hear it.
                                            Nothing is stored; press STORE on the unit to keep it.
  gsys raw <port> <hex bytes>               send an arbitrary SysEx

Writing to the unit (read PROTOCOL.md \"Storing presets\" first):
  gsys store <port> <file.syx> <file-slot> <unit-slot> --sysex-id <n> --i-accept-the-risk
                                            store one preset, then read it back to verify.
                                            The store sequence has frozen units; back up first.

Slots are 0-based user slots: 0 = 00-1 … 199 = 39-5."
    );
    std::process::exit(2);
}

fn load_any(path: &Path) -> Bank {
    let bytes = std::fs::read(path).unwrap_or_else(|e| {
        eprintln!("cannot read {}: {e}", path.display());
        std::process::exit(1)
    });
    vyzor::load_bank(&bytes)
        .or_else(|_| syx::load_bank(&bytes))
        .unwrap_or_else(|e| {
            eprintln!("{}: {e}", path.display());
            std::process::exit(1)
        })
}

fn print_bank(bank: &Bank) {
    for (i, slot) in bank.slots.iter().enumerate() {
        if let Some(p) = slot {
            println!("{:>3}  {}  {}", i, slot_label(i), p.name());
        }
    }
    println!("{} presets", bank.occupied());
}

fn main() {
    // `gsys list … | head` should not panic on a closed pipe.
    #[cfg(unix)]
    unsafe {
        extern "C" {
            fn signal(sig: i32, handler: usize) -> usize;
        }
        signal(13 /* SIGPIPE */, 0 /* SIG_DFL */);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a = |i: usize| args.get(i).map(String::as_str).unwrap_or_else(|| usage());
    match a(0) {
        "list" => print_bank(&load_any(Path::new(a(1)))),
        "export" => {
            let bank = load_any(Path::new(a(1)));
            std::fs::write(a(2), syx::save_bank(&bank, 0)).expect("write");
            println!("wrote {} presets to {}", bank.occupied(), a(2));
        }
        "merge" => {
            if args.len() < 3 {
                usage();
            }
            let mut out = Bank::empty();
            let mut seen = std::collections::HashSet::new();
            let (mut next, mut dupes, mut full) = (0usize, 0usize, 0usize);
            for path in &args[2..] {
                let bank = load_any(Path::new(path));
                let mut added = 0;
                for p in bank.slots.iter().flatten() {
                    let key = gsystem_protocol::validate::canonical_bytes(p);
                    if !seen.insert(key) {
                        dupes += 1;
                        continue;
                    }
                    if next >= 200 {
                        full += 1;
                        continue;
                    }
                    out.set(next, gsystem_protocol::validate::normalize(p));
                    next += 1;
                    added += 1;
                }
                println!("{added:>3} from {path}");
            }
            std::fs::write(a(1), syx::save_bank(&out, 0)).expect("write");
            println!(
                "wrote {} presets to {} ({dupes} duplicates skipped{})",
                out.occupied(),
                a(1),
                if full > 0 {
                    format!(", {full} left out: bank full")
                } else {
                    String::new()
                }
            );
        }
        "show" => {
            let bank = load_any(Path::new(a(1)));
            let slot: usize = a(2).parse().unwrap_or_else(|_| usage());
            let p = bank.get(slot).unwrap_or_else(|| {
                eprintln!("slot {slot} is empty");
                std::process::exit(1)
            });
            print_preset(slot, p);
        }
        "json" => {
            let bank = load_any(Path::new(a(1)));
            let rows: Vec<_> = bank
                .slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.as_ref().map(|p| (i, p)))
                .map(|(i, p)| {
                    serde_json::json!({
                        "slot": i, "label": slot_label(i), "name": p.name(),
                        "params_i24": p.params_i24(), "raw_hex": hex(&p.raw),
                        "view": gsystem_protocol::view::preset_view(i, p),
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&rows).unwrap());
        }
        "scripts" => {
            let bytes = std::fs::read(a(1)).expect("read");
            for f in vyzor::parse_forms(&bytes).expect("vyzor") {
                println!(
                    "== {} ({})",
                    f.file_name().unwrap_or_default(),
                    f.document_kind().unwrap_or_default()
                );
                for (chunk, m) in vyzor::script_messages(&f) {
                    match m {
                        Ok(m) => println!("  {chunk}: {m:?}"),
                        Err(e) => println!("  {chunk}: <unparsed: {e}>"),
                    }
                }
            }
        }
        "rename" => {
            let mut bank = load_any(Path::new(a(1)));
            let slot: usize = a(2).parse().unwrap_or_else(|_| usage());
            match bank.slots.get_mut(slot).and_then(|s| s.as_mut()) {
                Some(p) => p.set_name(a(3)),
                None => {
                    eprintln!("slot {slot} is empty");
                    std::process::exit(1)
                }
            }
            std::fs::write(a(4), syx::save_bank(&bank, 0)).expect("write");
            println!("renamed slot {slot} → {:?}", a(3));
        }
        "ports" => midi::list_ports(),
        "monitor" => midi::monitor(a(1)),
        "identify" => {
            let mut sess =
                midi::Session::open(a(1), args.get(2).map(String::as_str).unwrap_or(a(1)));
            match sess.identify() {
                Some(v) => println!("G-System answered (version bytes {v})"),
                None => {
                    eprintln!("no G-System answered the device inquiry");
                    std::process::exit(1)
                }
            }
        }
        "audition" => {
            let bank = load_any(Path::new(a(2)));
            let slot: usize = a(3).parse().unwrap_or_else(|_| usage());
            let p = bank.get(slot).cloned().unwrap_or_else(|| {
                eprintln!("slot {slot} is empty in {}", a(2));
                std::process::exit(1)
            });
            let msgs = gsystem_protocol::live::full_push(&p);
            let mut sess = midi::Session::open(a(1), a(1));
            sess.send_all(&msgs);
            println!(
                "loaded {:?} into the G-System's edit buffer ({} messages). Nothing was stored: press STORE on the unit to keep it.",
                p.name(),
                msgs.len()
            );
        }
        "globals" => {
            let mut sess = midi::Session::open(a(1), a(1));
            let read = sess.read_globals();
            if read.is_empty() {
                eprintln!("no answer from the unit");
                std::process::exit(1)
            }
            let v = gsystem_protocol::view::globals_view(&read);
            for g in v.groups {
                println!("{}", g.title);
                for p in g.params {
                    println!("  {:<18} {}", p.name, p.display);
                }
            }
            println!("Footswitches");
            for sw in v.switches {
                println!("  #{:<3} {}", sw.number, sw.display);
            }
        }
        "request" => {
            let slot: usize = a(2).parse().unwrap_or_else(|_| usage());
            let mut sess = midi::Session::open(a(1), a(1));
            match sess.request(slot) {
                Ok(Some(p)) => println!(
                    "{}  {}  {}\n{}",
                    slot,
                    slot_label(slot),
                    p.name(),
                    hex(&p.raw)
                ),
                Ok(None) => {
                    eprintln!("no answer: the slot may be empty, or the unit busy/not connected");
                    std::process::exit(1)
                }
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1)
                }
            }
        }
        "backup" => {
            let from: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            let to: usize = args
                .get(4)
                .and_then(|s| s.parse().ok())
                .unwrap_or(199)
                .min(199);
            let mut sess = midi::Session::open(a(1), a(1));
            let mut bank = Bank::empty();
            for slot in from..=to {
                match sess.request(slot) {
                    Ok(Some(p)) => {
                        println!("{:>3} {} {}", slot, slot_label(slot), p.name());
                        bank.set(slot, p);
                    }
                    Ok(None) => println!("{:>3} {} (empty / no answer)", slot, slot_label(slot)),
                    Err(e) => println!("{:>3} {} {e}", slot, slot_label(slot)),
                }
            }
            std::fs::write(a(2), syx::save_bank(&bank, 0)).expect("write");
            println!("saved {} presets to {}", bank.occupied(), a(2));
        }
        "store" => {
            if !args.iter().any(|x| x == "--i-accept-the-risk") {
                eprintln!("refusing: storing can freeze the unit (see PROTOCOL.md). Add --i-accept-the-risk after backing up.");
                std::process::exit(2)
            }
            let id: u8 = args
                .iter()
                .position(|x| x == "--sysex-id")
                .and_then(|i| args.get(i + 1))
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| {
                    eprintln!("--sysex-id <n> is required (Global › MIDI › SysEx ID on the unit)");
                    std::process::exit(2)
                });
            let bank = load_any(Path::new(a(2)));
            let from: usize = a(3).parse().unwrap_or_else(|_| usage());
            let to: usize = a(4).parse().unwrap_or_else(|_| usage());
            let p = bank.get(from).cloned().unwrap_or_else(|| {
                eprintln!("slot {from} is empty in {}", a(2));
                std::process::exit(1)
            });
            let mut sess = midi::Session::open(a(1), a(1));
            match sess.store(id, to, &p) {
                Ok(()) => println!("stored {:?} in {} and verified", p.name(), slot_label(to)),
                Err(e) => {
                    eprintln!("store failed: {e}");
                    std::process::exit(1)
                }
            }
        }
        "raw" => {
            let bytes: Vec<u8> = args[2..]
                .iter()
                .flat_map(|s| s.split_whitespace().map(String::from).collect::<Vec<_>>())
                .map(|h| u8::from_str_radix(&h, 16).unwrap_or_else(|_| usage()))
                .collect();
            midi::send_raw(a(1), &bytes);
        }
        _ => usage(),
    }
}

fn print_preset(slot: usize, p: &gsystem_protocol::Preset) {
    use gsystem_protocol::{levels, params, tempo};
    println!("{}  {}", slot_label(slot), p.name());
    let l = levels::preset_levels(p);
    let loops: Vec<String> = (0..4)
        .filter(|&i| l.loops[i])
        .map(|i| format!("{}", i + 1))
        .collect();
    println!(
        "  routing {} · loops {} · insert {} · boost {}{}",
        levels::ROUTINGS[l.routing],
        if loops.is_empty() {
            "off".into()
        } else {
            loops.join(",")
        },
        if l.insert { "on" } else { "off" },
        if l.boost { "on" } else { "off" },
        format!(" ({} dB)", l.boost_level),
    );
    if let Some(ms) = tempo::preset_tempo_ms(p) {
        println!("  tempo {} BPM ({ms} ms)", tempo::ms_to_bpm(ms));
    }
    let nova = gsystem_protocol::novadrive::nova_preset(p);
    if nova > 0 {
        println!("  NovaDrive preset {nova}");
    }
    let mods: Vec<_> = gsystem_protocol::modifiers::modifier_targets(p, nova > 0)
        .into_iter()
        .filter(|t| t.modifier.source != 0)
        .collect();
    for t in &mods {
        let m = t.modifier;
        println!(
            "  modifier {} → {} {}: {} / {} / {} %",
            gsystem_protocol::modifiers::SOURCES
                .get(m.source as usize)
                .unwrap_or(&"?"),
            t.key,
            params::label(&t.name),
            m.min,
            m.mid,
            m.max
        );
    }
    for b in params::BLOCKS.iter() {
        let ty = b
            .effect_type(p)
            .map(|t| format!(" — {}", t.name))
            .unwrap_or_default();
        println!(
            "\n  {}{} [{}]",
            b.label.to_uppercase(),
            ty,
            if b.is_on(p) { "ON" } else { "off" }
        );
        let en = b.enable_field(p);
        for (f, fp) in b.stored_fields(p) {
            if Some(f) == en || matches!(fp, params::FieldParam::Type(_)) {
                continue;
            }
            println!(
                "    {:<14} {:>12}   (field {f})",
                params::label(fp.name()),
                fp.format(p.param_i24(f))
            );
        }
    }
}

fn hex(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{x:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
