//! `gsys` — command-line librarian and MIDI tool for the G-System.
//!
//! File commands work offline. MIDI commands use any class-compliant
//! USB-MIDI interface wired to the unit's 5-pin DIN jacks.

mod midi;

use gsystem_protocol::preset::slot_label;
use gsystem_protocol::sysex::Message;
use gsystem_protocol::{syx, vyzor, Bank};
use std::path::Path;

fn usage() -> ! {
    eprintln!(
        "gsys — TC Electronic G-System tool

File commands (offline):
  gsys list <file.SQS|file.syx>            list preset slots and names
  gsys export <in.SQS> <out.syx>            write occupied slots as a .syx dump
  gsys json <file.SQS|file.syx>             dump presets as JSON (name + int24 params)
  gsys scripts <file.Prs|.Ebf|.SQS>         show SysEx the editor's TRAN/RECV scripts send/expect
  gsys rename <in.syx> <slot> \"<name>\" <out.syx>

MIDI commands:
  gsys ports                                list MIDI in/out ports
  gsys monitor <in-port>                    print every G-System message received
  gsys request <in-port> <out-port> <slot>  request one preset dump and print it
  gsys backup <in-port> <out-port> <out.syx> [from] [to]
                                            request slots from..to and save to .syx
  gsys send <in-port> <out-port> <file.syx> send every preset dump in the file
  gsys raw <out-port> <hex bytes>           send an arbitrary SysEx (e.g. \"F0 00 20 1F 00 55 57 01 F7\")

Slot numbers are 0-based (00-1 = 0 … 19-5 = 99, user bank starts at 100)."
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
        "request" => {
            let slot: u8 = a(3).parse().unwrap_or_else(|_| usage());
            match midi::request_preset(a(1), a(2), slot) {
                Some(p) => println!(
                    "{}  {}  {}\n{}",
                    slot,
                    slot_label(slot as usize),
                    p.name(),
                    hex(&p.raw)
                ),
                None => {
                    eprintln!("no dump received (check cables, MIDI channel, and that SysEx is enabled on the unit)");
                    std::process::exit(1)
                }
            }
        }
        "backup" => {
            let from: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
            let to: usize = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(199);
            let mut bank = Bank::empty();
            for slot in from..=to {
                match midi::request_preset(a(1), a(2), slot as u8) {
                    Some(p) => {
                        println!("{:>3} {} {}", slot, slot_label(slot), p.name());
                        bank.set(slot, p);
                    }
                    None => println!("{:>3} {} (no response)", slot, slot_label(slot)),
                }
            }
            std::fs::write(a(3), syx::save_bank(&bank, 0)).expect("write");
            println!("saved {} presets to {}", bank.occupied(), a(3));
        }
        "send" => {
            let bytes = std::fs::read(a(3)).expect("read");
            let (msgs, _) = syx::parse_all(&bytes);
            let dumps: Vec<Message> = msgs
                .into_iter()
                .filter(|m| matches!(m, Message::PresetDump { .. }))
                .collect();
            midi::send_dumps(a(1), a(2), &dumps);
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

fn hex(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{x:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
