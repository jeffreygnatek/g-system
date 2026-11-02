//! Plain `.syx` files: a concatenation of framed SysEx messages.

use crate::bank::Bank;
use crate::preset::Preset;
use crate::sysex::{number_to_slot, split_stream, Message};
use crate::Error;

/// Parse every G-System message in a `.syx` byte stream. Messages that fail
/// to parse are skipped (returned in the second element with their error).
pub fn parse_all(bytes: &[u8]) -> (Vec<Message>, Vec<(usize, crate::SysexError)>) {
    let mut ok = Vec::new();
    let mut bad = Vec::new();
    for (i, m) in split_stream(bytes).into_iter().enumerate() {
        match Message::parse(m) {
            Ok(msg) => ok.push(msg),
            Err(e) => bad.push((i, e)),
        }
    }
    (ok, bad)
}

/// Collect preset dumps into a bank. User presets (wire numbers 100..300) go to
/// their slot; factory-numbered dumps (0..100) are placed in order of
/// appearance in the first free slots, so a factory .syx still loads.
pub fn load_bank(bytes: &[u8]) -> Result<Bank, Error> {
    let (msgs, _) = parse_all(bytes);
    let mut bank = Bank::empty();
    let mut any = false;
    let mut factory = Vec::new();
    for m in msgs {
        if let Message::PresetDump { number, data, .. } = m {
            if let Some(p) = Preset::from_raw(data) {
                match number_to_slot(number) {
                    Some(slot) => bank.set(slot, p),
                    None => factory.push(p),
                }
                any = true;
            }
        }
    }
    for p in factory {
        if let Some(free) = bank.slots.iter().position(|s| s.is_none()) {
            bank.set(free, p);
        }
    }
    if any {
        Ok(bank)
    } else {
        Err(Error::Format("no preset dumps found"))
    }
}

/// Serialize occupied slots as a stream of preset dumps.
pub fn save_bank(bank: &Bank, device: u8) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, slot) in bank.slots.iter().enumerate() {
        if let Some(p) = slot {
            out.extend(p.to_message(device, i).encode());
        }
    }
    out
}
