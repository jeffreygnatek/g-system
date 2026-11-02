//! Direct USB-MIDI access to the G-System.
//!
//! The G-System's USB port (TC Electronic, vendor 0x1220, product 0x0005) has a
//! USB-MIDI *streaming* interface (class 1, subclass 3) but no Audio Control
//! interface, which the USB-MIDI spec requires alongside it. Linux and Android
//! accept that; macOS doesn't attach its MIDI driver, so no MIDI port appears.
//! Because no driver claims the interface, we can open it ourselves with
//! libusb and speak USB-MIDI's 4-byte event packets directly.
//!
//! [`open`] shares one connection per process (the interface can be
//! claimed only once); every opener gets its own receiver of complete SysEx
//! messages.

use rusb::{Context, DeviceHandle, Direction, TransferType, UsbContext};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

pub const TC_VENDOR: u16 = 0x1220;
/// G-System. Other TC units with the same USB layout could be added here.
pub const PRODUCTS: &[u16] = &[0x0005];
/// The name shown in port lists for the direct connection.
pub const PORT_NAME: &str = "G-System (USB)";

const AUDIO_CLASS: u8 = 1;
const MIDI_STREAMING: u8 = 3;
const IO_TIMEOUT: Duration = Duration::from_millis(1000);
const READ_POLL: Duration = Duration::from_millis(100);

/// Is a G-System connected over USB (whether or not a MIDI driver claimed it)?
pub fn present() -> bool {
    Context::new()
        .and_then(|c| c.devices())
        .map(|list| {
            list.iter().any(|d| {
                d.device_descriptor()
                    .map(|dd| dd.vendor_id() == TC_VENDOR && PRODUCTS.contains(&dd.product_id()))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

struct Shared {
    handle: DeviceHandle<Context>,
    iface: u8,
    ep_out: u8,
    out_bulk: bool,
    out_packet: usize,
    ep_in: u8,
    in_bulk: bool,
    in_packet: usize,
    subscribers: Mutex<Vec<Sender<Vec<u8>>>>,
    stop: AtomicBool,
    write_lock: Mutex<()>,
}

impl Drop for Shared {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.handle.release_interface(self.iface);
    }
}

/// The sending half of a user of the shared connection. Complete incoming
/// SysEx messages arrive on the `Receiver` returned with it by [`open`].
pub struct UsbMidi {
    shared: Arc<Shared>,
}

fn current() -> &'static Mutex<Weak<Shared>> {
    static CURRENT: OnceLock<Mutex<Weak<Shared>>> = OnceLock::new();
    CURRENT.get_or_init(|| Mutex::new(Weak::new()))
}

/// Open (or join) the direct USB connection to the G-System.
pub fn open() -> Result<(UsbMidi, Receiver<Vec<u8>>), String> {
    UsbMidi::open()
}

impl UsbMidi {
    /// Open (or join) the direct USB connection to the G-System.
    pub fn open() -> Result<(UsbMidi, Receiver<Vec<u8>>), String> {
        let mut cur = current().lock().unwrap();
        let shared = match cur.upgrade() {
            Some(s) => s,
            None => {
                let s = Arc::new(connect()?);
                spawn_reader(&s)?;
                *cur = Arc::downgrade(&s);
                s
            }
        };
        let (tx, rx) = mpsc::channel();
        shared.subscribers.lock().unwrap().push(tx);
        Ok((UsbMidi { shared }, rx))
    }

    /// Send one complete SysEx message (F0 … F7).
    pub fn send(&self, sysex: &[u8]) -> Result<(), String> {
        let packets = pack_sysex(sysex);
        let s = &self.shared;
        let _guard = s.write_lock.lock().unwrap();
        for chunk in packets.chunks(s.out_packet.max(4) / 4 * 4) {
            let n = if s.out_bulk {
                s.handle.write_bulk(s.ep_out, chunk, IO_TIMEOUT)
            } else {
                s.handle.write_interrupt(s.ep_out, chunk, IO_TIMEOUT)
            }
            .map_err(|e| format!("USB write failed: {e}"))?;
            if n != chunk.len() {
                return Err(format!(
                    "USB write was short ({n} of {} bytes)",
                    chunk.len()
                ));
            }
        }
        Ok(())
    }
}

fn connect() -> Result<Shared, String> {
    let ctx = Context::new().map_err(|e| format!("USB is not available: {e}"))?;
    let devices = ctx
        .devices()
        .map_err(|e| format!("cannot list USB devices: {e}"))?;
    let dev = devices
        .iter()
        .find(|d| {
            d.device_descriptor()
                .map(|dd| dd.vendor_id() == TC_VENDOR && PRODUCTS.contains(&dd.product_id()))
                .unwrap_or(false)
        })
        .ok_or("No G-System found on USB. Check the cable and that the unit is switched on.")?;
    let config = dev
        .active_config_descriptor()
        .map_err(|e| format!("cannot read the G-System's USB configuration: {e}"))?;
    let mut found = None;
    for iface in config.interfaces() {
        for alt in iface.descriptors() {
            if alt.class_code() != AUDIO_CLASS || alt.sub_class_code() != MIDI_STREAMING {
                continue;
            }
            let mut ep_in = None;
            let mut ep_out = None;
            for ep in alt.endpoint_descriptors() {
                let kind = ep.transfer_type();
                if kind != TransferType::Bulk && kind != TransferType::Interrupt {
                    continue;
                }
                match ep.direction() {
                    Direction::In => {
                        ep_in = ep_in.or(Some((ep.address(), kind, ep.max_packet_size())))
                    }
                    Direction::Out => {
                        ep_out = ep_out.or(Some((ep.address(), kind, ep.max_packet_size())))
                    }
                }
            }
            if let (Some(i), Some(o)) = (ep_in, ep_out) {
                found = Some((alt.interface_number(), alt.setting_number(), i, o));
            }
        }
    }
    let (iface, setting, ep_in, ep_out) =
        found.ok_or("The G-System's USB interface has no MIDI endpoints.")?;
    let handle = dev
        .open()
        .map_err(|e| format!("cannot open the G-System over USB: {e}"))?;
    // Linux: snd-usb-audio may have it; detach for the session.
    let _ = handle.set_auto_detach_kernel_driver(true);
    handle.claim_interface(iface).map_err(|e| {
        format!("cannot claim the G-System's USB interface ({e}). Is another app (or another copy of this one) using it?")
    })?;
    if setting != 0 {
        let _ = handle.set_alternate_setting(iface, setting);
    }
    let shared = Shared {
        handle,
        iface,
        ep_out: ep_out.0,
        out_bulk: ep_out.1 == TransferType::Bulk,
        out_packet: ep_out.2 as usize,
        ep_in: ep_in.0,
        in_bulk: ep_in.1 == TransferType::Bulk,
        in_packet: ep_in.2 as usize,
        subscribers: Mutex::new(Vec::new()),
        stop: AtomicBool::new(false),
        write_lock: Mutex::new(()),
    };
    Ok(shared)
}

fn spawn_reader(shared: &Arc<Shared>) -> Result<(), String> {
    let (ep, bulk, size) = (shared.ep_in, shared.in_bulk, shared.in_packet);
    let weak = Arc::downgrade(shared);
    std::thread::Builder::new()
        .name("gsystem-usb-in".into())
        .spawn(move || {
            let mut buf = vec![0u8; size.max(64)];
            let mut asm = Assembler::default();
            loop {
                let Some(s) = weak.upgrade() else { break };
                if s.stop.load(Ordering::SeqCst) {
                    break;
                }
                let r = if bulk {
                    s.handle.read_bulk(ep, &mut buf, READ_POLL)
                } else {
                    s.handle.read_interrupt(ep, &mut buf, READ_POLL)
                };
                match r {
                    Ok(n) => {
                        for pkt in buf[..n].chunks_exact(4) {
                            for msg in asm.push_packet(pkt) {
                                let mut subs = s.subscribers.lock().unwrap();
                                subs.retain(|tx| tx.send(msg.clone()).is_ok());
                            }
                        }
                    }
                    Err(rusb::Error::Timeout) => {}
                    Err(rusb::Error::NoDevice) => break,
                    Err(_) => std::thread::sleep(READ_POLL),
                }
            }
        })
        .map_err(|e| format!("cannot start the USB reader: {e}"))?;
    Ok(())
}

/// USB-MIDI 1.0 event packets for one SysEx message, on cable 0.
pub fn pack_sysex(sysex: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(sysex.len().div_ceil(3) * 4);
    let chunks: Vec<&[u8]> = sysex.chunks(3).collect();
    for (i, c) in chunks.iter().enumerate() {
        let last = i + 1 == chunks.len() && c.last() == Some(&0xF7);
        let cin = if last { 0x4 + c.len() as u8 } else { 0x4 };
        out.push(cin);
        for j in 0..3 {
            out.push(*c.get(j).unwrap_or(&0));
        }
    }
    out
}

/// Rebuilds complete SysEx messages from USB-MIDI event packets.
#[derive(Default)]
pub struct Assembler {
    buf: Vec<u8>,
}

impl Assembler {
    /// Feed one 4-byte packet; returns any SysEx messages it completed.
    pub fn push_packet(&mut self, pkt: &[u8]) -> Vec<Vec<u8>> {
        let n = match pkt[0] & 0x0F {
            0x4 | 0x7 => 3, // SysEx start/continue, or end with 3 bytes
            0x5 | 0xF => 1, // SysEx end with 1 byte, or a single byte
            0x6 => 2,       // SysEx end with 2 bytes
            _ => 0,         // channel and system messages: not SysEx
        };
        let mut done = Vec::new();
        for &b in &pkt[1..1 + n] {
            if b == 0xF0 {
                self.buf.clear();
            }
            if !self.buf.is_empty() || b == 0xF0 {
                self.buf.push(b);
                if b == 0xF7 {
                    done.push(std::mem::take(&mut self.buf));
                }
            }
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_sysex_per_usb_midi_spec() {
        // 6 bytes: start(3) + end-with-3
        assert_eq!(
            pack_sysex(&[0xF0, 0x7E, 0x7F, 0x06, 0x01, 0xF7]),
            vec![0x04, 0xF0, 0x7E, 0x7F, 0x07, 0x06, 0x01, 0xF7]
        );
        // 4 bytes: start(3) + end-with-1
        assert_eq!(
            pack_sysex(&[0xF0, 1, 2, 0xF7]),
            vec![0x04, 0xF0, 1, 2, 0x05, 0xF7, 0, 0]
        );
        // 5 bytes: start(3) + end-with-2
        assert_eq!(
            pack_sysex(&[0xF0, 1, 2, 3, 0xF7]),
            vec![0x04, 0xF0, 1, 2, 0x06, 3, 0xF7, 0]
        );
    }

    #[test]
    fn round_trips_through_the_assembler() {
        for len in 2..40 {
            let mut msg = vec![0xF0];
            msg.extend((0..len).map(|i| (i % 0x7F) as u8));
            msg.push(0xF7);
            let packets = pack_sysex(&msg);
            let mut a = Assembler::default();
            let got: Vec<_> = packets.chunks(4).flat_map(|p| a.push_packet(p)).collect();
            assert_eq!(got, vec![msg]);
        }
    }

    #[test]
    fn ignores_channel_messages_between_sysex() {
        let mut a = Assembler::default();
        assert!(a.push_packet(&[0x0C, 0xC0, 0x05, 0x00]).is_empty()); // program change
        let msg = [0xF0, 0x00, 0x20, 0x1F, 0xF7];
        let got: Vec<_> = pack_sysex(&msg)
            .chunks(4)
            .flat_map(|p| a.push_packet(p))
            .collect();
        assert_eq!(got, vec![msg.to_vec()]);
    }
}
