//! MIDI transport via `midir` (CoreMIDI on macOS, ALSA on Linux, WinMM on Windows).
//!
//! Timing follows the editor's recorded TRAN scripts: it waits 2000 ms after a
//! preset select/request before doing anything else. Hardware-untested as of
//! this commit; see PROTOCOL.md "Open questions".

use gsystem_protocol::sysex::Message;
use gsystem_protocol::Preset;
use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const CLIENT: &str = "gsys";
/// From the TRAN scripts: delay after a preset select before the next message.
const SELECT_SETTLE: Duration = Duration::from_millis(2000);
/// Gap between consecutive preset dumps when sending a bank (FreeMajor uses 100 ms).
const DUMP_GAP: Duration = Duration::from_millis(150);
const REPLY_TIMEOUT: Duration = Duration::from_millis(3500);

fn midi_unavailable(e: impl std::fmt::Display) -> ! {
    eprintln!("MIDI is not available on this system ({e}). On Linux make sure ALSA's sequencer is loaded (`modprobe snd-seq`).");
    std::process::exit(1)
}

pub fn list_ports() {
    let inp = MidiInput::new(CLIENT).unwrap_or_else(|e| midi_unavailable(e));
    println!("Inputs:");
    for p in inp.ports() {
        println!("  {}", inp.port_name(&p).unwrap_or_default());
    }
    let out = MidiOutput::new(CLIENT).unwrap_or_else(|e| midi_unavailable(e));
    println!("Outputs:");
    for p in out.ports() {
        println!("  {}", out.port_name(&p).unwrap_or_default());
    }
}

fn open_out(name: &str) -> MidiOutputConnection {
    let out = MidiOutput::new(CLIENT).unwrap_or_else(|e| midi_unavailable(e));
    let port = out
        .ports()
        .into_iter()
        .find(|p| out.port_name(p).map(|n| n.contains(name)).unwrap_or(false))
        .unwrap_or_else(|| {
            eprintln!("no MIDI output port matching {name:?} (try `gsys ports`)");
            std::process::exit(1)
        });
    out.connect(&port, CLIENT).expect("connect output")
}

/// Open an input and forward complete SysEx messages to a channel.
fn open_in(name: &str) -> (MidiInputConnection<()>, mpsc::Receiver<Vec<u8>>) {
    let mut inp = MidiInput::new(CLIENT).unwrap_or_else(|e| midi_unavailable(e));
    inp.ignore(midir::Ignore::None);
    let port = inp
        .ports()
        .into_iter()
        .find(|p| inp.port_name(p).map(|n| n.contains(name)).unwrap_or(false))
        .unwrap_or_else(|| {
            eprintln!("no MIDI input port matching {name:?} (try `gsys ports`)");
            std::process::exit(1)
        });
    let (tx, rx) = mpsc::channel();
    // midir delivers SysEx as one callback per complete message on all backends
    // we care about, but be defensive and reassemble F0..F7 anyway.
    let mut buf: Vec<u8> = Vec::new();
    let conn = inp
        .connect(
            &port,
            CLIENT,
            move |_, bytes, _| {
                if bytes.first() == Some(&0xF0) {
                    buf.clear();
                }
                if !buf.is_empty() || bytes.first() == Some(&0xF0) {
                    buf.extend_from_slice(bytes);
                    if buf.last() == Some(&0xF7) {
                        let _ = tx.send(std::mem::take(&mut buf));
                    }
                }
            },
            (),
        )
        .expect("connect input");
    (conn, rx)
}

pub fn monitor(in_name: &str) {
    let (_conn, rx) = open_in(in_name);
    eprintln!("monitoring {in_name:?}; Ctrl-C to stop");
    for bytes in rx {
        match Message::parse(&bytes) {
            Ok(m) => println!("{m:?}"),
            Err(e) => println!("<{e}> {}", hex(&bytes)),
        }
    }
}

pub fn send_raw(out_name: &str, bytes: &[u8]) {
    let mut out = open_out(out_name);
    out.send(bytes).expect("send");
    println!("sent {} bytes", bytes.len());
}

/// Ask the unit for one preset and wait for the `0x20` dump.
pub fn request_preset(in_name: &str, out_name: &str, slot: u8) -> Option<Preset> {
    let (_conn, rx) = open_in(in_name);
    let mut out = open_out(out_name);
    out.send(&Message::preset_request(0, slot).encode())
        .expect("send");
    let deadline = Instant::now() + REPLY_TIMEOUT;
    while let Ok(bytes) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        if let Ok(Message::PresetDump { data, .. }) = Message::parse(&bytes) {
            std::thread::sleep(SELECT_SETTLE);
            return Preset::from_raw(data);
        }
    }
    None
}

pub fn send_dumps(in_name: &str, out_name: &str, dumps: &[Message]) {
    let (_conn, rx) = open_in(in_name);
    let mut out = open_out(out_name);
    for (i, m) in dumps.iter().enumerate() {
        out.send(&m.encode()).expect("send");
        if let Message::PresetDump { preset, .. } = m {
            println!("sent {}/{} (slot {preset})", i + 1, dumps.len());
        }
        std::thread::sleep(DUMP_GAP);
        // surface anything the unit says back (acks / errors are not yet decoded)
        while let Ok(bytes) = rx.try_recv() {
            println!("  unit: {}", hex(&bytes));
        }
    }
}

fn hex(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{x:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
