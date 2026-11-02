//! MIDI transport via `midir` (CoreMIDI on macOS, ALSA on Linux, WinMM on Windows).
//!
//! The G-System's own USB port shows up as a MIDI device, so no interface is
//! needed; a USB-MIDI interface on the 5-pin jacks works too.
//!
//! Timing and message sequences follow the hardware observations documented
//! by the independent davidavalanches/gsystem-editor project (see PROTOCOL.md).
//! Reading is safe. Writing a preset can freeze the unit (see `store`).

use gsystem_protocol::sysex::{parse_identity, Message, DEVICE_ANY, DEVICE_INQUIRY};
use gsystem_protocol::Preset;
use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const CLIENT: &str = "gsys";
/// A stored preset answers in about 0.77 s; the unit ignores requests while busy.
const REQUEST_TIMEOUT: Duration = Duration::from_millis(1100);
const IDENTIFY_TIMEOUT: Duration = Duration::from_millis(1500);
/// Pause between the messages of a store, and before reading the slot back.
const STORE_STEP: Duration = Duration::from_millis(100);
const STORE_SETTLE: Duration = Duration::from_millis(1500);

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

/// An open in/out pair. Port names match by case-insensitive substring.
pub struct Session {
    _in: MidiInputConnection<()>,
    rx: mpsc::Receiver<Vec<u8>>,
    out: MidiOutputConnection,
}

fn matches(port: &str, want: &str) -> bool {
    port.to_lowercase().contains(&want.to_lowercase())
}

impl Session {
    pub fn open(in_name: &str, out_name: &str) -> Session {
        Session::try_open(in_name, out_name).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1)
        })
    }

    /// Like [`open`](Self::open), but returns errors instead of exiting
    /// (for long-running callers such as `gsys mcp`).
    pub fn try_open(in_name: &str, out_name: &str) -> Result<Session, String> {
        let unavailable = |e: &dyn std::fmt::Display| {
            format!("MIDI is not available on this system ({e}). On Linux make sure ALSA's sequencer is loaded (`modprobe snd-seq`).")
        };
        let mut inp = MidiInput::new(CLIENT).map_err(|e| unavailable(&e))?;
        inp.ignore(midir::Ignore::None);
        let in_port = inp
            .ports()
            .into_iter()
            .find(|p| {
                inp.port_name(p)
                    .map(|n| matches(&n, in_name))
                    .unwrap_or(false)
            })
            .ok_or_else(|| format!("no MIDI input port matching {in_name:?} (try `gsys ports`)"))?;
        let out = MidiOutput::new(CLIENT).map_err(|e| unavailable(&e))?;
        let out_port = out
            .ports()
            .into_iter()
            .find(|p| {
                out.port_name(p)
                    .map(|n| matches(&n, out_name))
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                format!("no MIDI output port matching {out_name:?} (try `gsys ports`)")
            })?;
        let (tx, rx) = mpsc::channel();
        // Reassemble F0..F7: some backends split long SysEx across callbacks.
        let mut buf: Vec<u8> = Vec::new();
        let conn = inp
            .connect(
                &in_port,
                CLIENT,
                move |_, bytes, _| {
                    for &b in bytes {
                        if b == 0xF0 {
                            buf.clear();
                        }
                        if !buf.is_empty() || b == 0xF0 {
                            buf.push(b);
                            if b == 0xF7 {
                                let _ = tx.send(std::mem::take(&mut buf));
                            }
                        }
                    }
                },
                (),
            )
            .map_err(|e| format!("cannot open MIDI input {in_name:?}: {e}"))?;
        let out = out
            .connect(&out_port, CLIENT)
            .map_err(|e| format!("cannot open MIDI output {out_name:?}: {e}"))?;
        Ok(Session { _in: conn, rx, out })
    }

    fn send(&mut self, bytes: &[u8]) {
        self.out.send(bytes).expect("send");
    }

    fn drain(&self) {
        while self.rx.try_recv().is_ok() {}
    }

    /// Wait until `pick` accepts a message, or the timeout passes.
    fn wait<T>(&self, timeout: Duration, mut pick: impl FnMut(&[u8]) -> Option<T>) -> Option<T> {
        let deadline = Instant::now() + timeout;
        while let Ok(bytes) = self
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            if let Some(v) = pick(&bytes) {
                return Some(v);
            }
        }
        None
    }

    /// Universal Device Inquiry. Returns the raw version bytes if a G-System answers.
    pub fn identify(&mut self) -> Option<String> {
        self.drain();
        self.send(&DEVICE_INQUIRY);
        self.wait(IDENTIFY_TIMEOUT, parse_identity)
    }

    /// Read user slot `slot` (0..200). `Err` if the unit reports the slot damaged;
    /// `Ok(None)` if it doesn't answer (empty slots never answer).
    pub fn request(&mut self, slot: usize) -> Result<Option<Preset>, String> {
        self.drain();
        self.send(&Message::preset_request(0, slot).encode());
        let want = gsystem_protocol::sysex::slot_to_number(slot);
        let got = self.wait(REQUEST_TIMEOUT, |b| match Message::parse(b) {
            Ok(Message::PresetDump { number, data, .. }) if number == want => {
                Some(Ok(Preset::from_raw(data)))
            }
            Ok(Message::CorruptPreset { number, .. }) if number == want => Some(Err(())),
            _ => None,
        });
        match got {
            Some(Ok(p)) => Ok(p),
            Some(Err(())) => Err(format!("slot {slot} is damaged on the unit")),
            None => Ok(None),
        }
    }

    /// Send messages with the unit's pacing (edit-buffer writes).
    pub fn send_all(&mut self, msgs: &[Message]) {
        self.try_send_all(msgs).expect("send");
    }

    /// [`send_all`](Self::send_all), returning send errors instead of panicking.
    pub fn try_send_all(&mut self, msgs: &[Message]) -> Result<(), String> {
        for m in msgs {
            self.out
                .send(&m.encode())
                .map_err(|e| format!("MIDI send failed: {e}"))?;
            std::thread::sleep(Duration::from_millis(
                gsystem_protocol::live::SEND_INTERVAL_MS,
            ));
        }
        Ok(())
    }

    /// Read the unit's global settings and footswitch assignments with
    /// `55 47` requests (read-only; this is how Vyzor reads them).
    pub fn read_globals(&mut self) -> std::collections::HashMap<(u8, u16), i32> {
        let mut out = std::collections::HashMap::new();
        for (block, first, count) in gsystem_protocol::globals::GLOBAL_RANGES {
            self.drain();
            self.send(
                &Message::RequestParams {
                    device: 0,
                    block,
                    first,
                    count,
                }
                .encode(),
            );
            let want = first..first + count;
            let deadline = Instant::now() + Duration::from_millis(1500);
            while (want.clone()).any(|p| !out.contains_key(&(block, p))) {
                let Ok(bytes) = self
                    .rx
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                else {
                    break;
                };
                if let Ok(Message::SetParam {
                    block: b,
                    param,
                    values,
                    ..
                }) = Message::parse(&bytes)
                {
                    for (i, v) in values.into_iter().enumerate() {
                        out.insert((b, param + i as u16), v);
                    }
                }
            }
        }
        out
    }

    /// Store `preset` into user slot `slot`, the way Vyzor's Sync does it, then
    /// read the slot back to verify.
    ///
    /// WARNING: the same sequence has been observed to freeze a G-System
    /// intermittently (power cycle needed; slot left unreadable; once a
    /// neighbouring preset altered). Back up first and expect to power-cycle.
    pub fn store(&mut self, sysex_id: u8, slot: usize, preset: &Preset) -> Result<(), String> {
        if sysex_id == DEVICE_ANY {
            return Err("store needs the unit's own SysEx ID (Global › MIDI), not 7F".into());
        }
        self.drain();
        for msg in [
            Message::editor_mode(sysex_id, true).encode(),
            Message::Transfer {
                device: sysex_id,
                begin: true,
            }
            .encode(),
            preset.to_message(sysex_id, slot).encode(),
            Message::Transfer {
                device: sysex_id,
                begin: false,
            }
            .encode(),
        ] {
            self.send(&msg);
            std::thread::sleep(STORE_STEP);
        }
        std::thread::sleep(STORE_SETTLE);
        match self.request(slot)? {
            Some(back) if back == *preset => Ok(()),
            Some(_) => Err(format!(
                "slot {slot} read back different from what was sent"
            )),
            None => Err(format!(
                "slot {slot} did not answer after the store (the unit may need a power cycle)"
            )),
        }
    }
}

pub fn monitor(in_name: &str) {
    let s = Session::open(in_name, in_name);
    eprintln!("monitoring {in_name:?}; Ctrl-C to stop");
    for bytes in s.rx.iter() {
        match Message::parse(&bytes) {
            Ok(m) => println!("{m:?}"),
            Err(e) => println!("<{e}> {}", hex(&bytes)),
        }
    }
}

pub fn send_raw(out_name: &str, bytes: &[u8]) {
    let mut s = Session::open(out_name, out_name);
    s.send(bytes);
    println!("sent {} bytes", bytes.len());
}

pub fn hex(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{x:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
