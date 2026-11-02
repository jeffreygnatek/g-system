//! SysEx framing for the G-System.
//!
//! Every message:
//! ```text
//! F0 00 20 1F <dev> 55 <type> <payload...> F7
//! ```
//! `00 20 1F` is TC Electronic's manufacturer ID, `0x55` is the G-System model
//! ID (the G-Major uses `0x48`). `<dev>` is the unit's SysEx ID (set in
//! Global › MIDI) or `0x7F` for "any device".
//!
//! Message meanings follow the hardware observations published by the
//! independent `davidavalanches/gsystem-editor` project (see PROTOCOL.md);
//! this file is our own implementation of those facts.

use std::fmt;

pub const SYSEX_START: u8 = 0xF0;
pub const SYSEX_END: u8 = 0xF7;
pub const TC_MANUFACTURER: [u8; 3] = [0x00, 0x20, 0x1F];
pub const MODEL_GSYSTEM: u8 = 0x55;
pub const DEVICE_ANY: u8 = 0x7F;

/// Message type byte (offset 6).
pub mod msg {
    /// Preset dump: `20 01 <num:2> <1032 nibbles> <chk>`.
    pub const PRESET_DUMP: u8 = 0x20;
    /// Set one edit-buffer parameter: `22 <block> <param:2> <value:6 nibbles>`.
    /// The unit also sends these when a footswitch or knob is used.
    pub const SET_PARAM: u8 = 0x22;
    /// The unit's answer for a damaged slot: `28 <num:2> 00 01`.
    pub const CORRUPT_PRESET: u8 = 0x28;
    /// Request a stored preset (read-only): `45 01 <num:2>`.
    pub const REQUEST_PRESET: u8 = 0x45;
    /// Request parameters: `47 <block> <first:2> <count:2>`, answered with `22` messages.
    pub const REQUEST_PARAMS: u8 = 0x47;
    /// Firmware updater: enter/leave bootloader.
    pub const FW_CONTROL: u8 = 0x51;
    /// Firmware data block (305 bytes).
    pub const FW_BLOCK: u8 = 0x52;
    /// Bracket around a preset store: `57 00` begin, `57 01` end.
    pub const TRANSFER: u8 = 0x57;
}

/// Preset numbers on the wire: user slot 0 is preset 100.
pub const USER_PRESET_BASE: u16 = 100;
/// Number of nibble bytes in a preset dump payload (516 data bytes × 2).
pub const PRESET_NIBBLES: usize = 1032;
/// Decoded preset payload size in bytes.
pub const PRESET_BYTES: usize = PRESET_NIBBLES / 2;
/// Total framed length of a preset dump message.
pub const PRESET_DUMP_LEN: usize = 10 + PRESET_NIBBLES + 2;
/// Universal MIDI Device Inquiry (answered with the firmware version).
pub const DEVICE_INQUIRY: [u8; 6] = [0xF0, 0x7E, 0x7F, 0x06, 0x01, 0xF7];

/// Wire preset number for user slot `slot` (0..200).
pub fn slot_to_number(slot: usize) -> u16 {
    USER_PRESET_BASE + slot as u16
}
/// User slot for a wire preset number, if it is a user preset.
pub fn number_to_slot(number: u16) -> Option<usize> {
    number
        .checked_sub(USER_PRESET_BASE)
        .map(|s| s as usize)
        .filter(|&s| s < 200)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SysexError {
    TooShort,
    BadFraming,
    NotTc,
    NotGSystem,
    BadChecksum { expected: u8, found: u8 },
    BadLength { expected: usize, found: usize },
    NonNibble(usize),
}

impl fmt::Display for SysexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SysexError::TooShort => write!(f, "message too short"),
            SysexError::BadFraming => write!(f, "missing F0/F7"),
            SysexError::NotTc => write!(f, "not a TC Electronic message"),
            SysexError::NotGSystem => write!(f, "not a G-System message"),
            SysexError::BadChecksum { expected, found } => {
                write!(
                    f,
                    "checksum mismatch: expected {expected:02X}, found {found:02X}"
                )
            }
            SysexError::BadLength { expected, found } => {
                write!(f, "bad length: expected {expected}, found {found}")
            }
            SysexError::NonNibble(i) => write!(f, "byte {i} is not a nibble (>= 0x10)"),
        }
    }
}
impl std::error::Error for SysexError {}

/// Checksum used by preset dumps: sum of the nibble bytes, masked to 7 bits.
pub fn nibble_checksum(nibbles: &[u8]) -> u8 {
    (nibbles.iter().map(|&b| b as u32).sum::<u32>() & 0x7F) as u8
}

/// Expand bytes into (hi, lo) nibble pairs.
pub fn to_nibbles(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(b >> 4);
        out.push(b & 0x0F);
    }
    out
}

/// Collapse (hi, lo) nibble pairs into bytes. Errors on any byte >= 0x10.
pub fn from_nibbles(nibbles: &[u8]) -> Result<Vec<u8>, SysexError> {
    if !nibbles.len().is_multiple_of(2) {
        return Err(SysexError::BadLength {
            expected: nibbles.len() + 1,
            found: nibbles.len(),
        });
    }
    let mut out = Vec::with_capacity(nibbles.len() / 2);
    for (i, pair) in nibbles.chunks_exact(2).enumerate() {
        if pair[0] >= 0x10 {
            return Err(SysexError::NonNibble(i * 2));
        }
        if pair[1] >= 0x10 {
            return Err(SysexError::NonNibble(i * 2 + 1));
        }
        out.push((pair[0] << 4) | pair[1]);
    }
    Ok(out)
}

fn u14(hi: u8, lo: u8) -> u16 {
    ((hi as u16 & 0x7F) << 7) | (lo as u16 & 0x7F)
}
fn split14(v: u16) -> [u8; 2] {
    [((v >> 7) & 0x7F) as u8, (v & 0x7F) as u8]
}
/// Signed 24-bit value as six nibbles, most significant first.
fn i24_nibbles(v: i32) -> [u8; 6] {
    let raw = (v as u32) & 0xFF_FFFF;
    [20, 16, 12, 8, 4, 0].map(|s| ((raw >> s) & 0x0F) as u8)
}
fn nibbles_i24(n: &[u8]) -> i32 {
    let raw = n
        .iter()
        .fold(0u32, |acc, &x| (acc << 4) | (x as u32 & 0x0F));
    ((raw << 8) as i32) >> 8
}

/// A parsed G-System message. `Unknown` keeps the raw payload so nothing is lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// `20 01 <num:2> …` — a full preset. `number` is the wire number (user slot + 100).
    PresetDump {
        device: u8,
        number: u16,
        data: Vec<u8>,
    },
    /// `45 01 <num:2>` — ask the unit for a stored preset.
    PresetRequest { device: u8, number: u16 },
    /// `22 <block> <param:2> <value:6 nibbles>` — one edit-buffer parameter.
    /// A message from the unit may carry several consecutive values; see `values`.
    SetParam {
        device: u8,
        block: u8,
        param: u16,
        values: Vec<i32>,
    },
    /// `47 <block> <first:2> <count:2>` — ask for parameters `first..first+count`.
    RequestParams {
        device: u8,
        block: u8,
        first: u16,
        count: u16,
    },
    /// `57 00` / `57 01` — begin / end a preset store.
    Transfer { device: u8, begin: bool },
    /// `28 <num:2> 00 01` — the unit reports a damaged slot.
    CorruptPreset { device: u8, number: u16 },
    Unknown {
        device: u8,
        kind: u8,
        payload: Vec<u8>,
    },
}

impl Message {
    /// Parse one complete framed message (F0 ... F7).
    pub fn parse(bytes: &[u8]) -> Result<Message, SysexError> {
        if bytes.len() < 8 {
            return Err(SysexError::TooShort);
        }
        if bytes[0] != SYSEX_START || *bytes.last().unwrap() != SYSEX_END {
            return Err(SysexError::BadFraming);
        }
        if bytes[1..4] != TC_MANUFACTURER {
            return Err(SysexError::NotTc);
        }
        if bytes[5] != MODEL_GSYSTEM {
            return Err(SysexError::NotGSystem);
        }
        let device = bytes[4];
        let kind = bytes[6];
        let p = &bytes[7..bytes.len() - 1];
        Ok(match kind {
            msg::PRESET_DUMP if p.len() == PRESET_NIBBLES + 4 && p[0] == 0x01 => {
                let nibbles = &p[3..3 + PRESET_NIBBLES];
                let chk = p[3 + PRESET_NIBBLES];
                let calc = nibble_checksum(nibbles);
                if chk != calc {
                    return Err(SysexError::BadChecksum {
                        expected: calc,
                        found: chk,
                    });
                }
                Message::PresetDump {
                    device,
                    number: u14(p[1], p[2]),
                    data: from_nibbles(nibbles)?,
                }
            }
            msg::REQUEST_PRESET if p.len() == 3 && p[0] == 0x01 => Message::PresetRequest {
                device,
                number: u14(p[1], p[2]),
            },
            msg::SET_PARAM if p.len() >= 9 && (p.len() - 3).is_multiple_of(6) => {
                Message::SetParam {
                    device,
                    block: p[0],
                    param: u14(p[1], p[2]),
                    values: p[3..].chunks_exact(6).map(nibbles_i24).collect(),
                }
            }
            msg::REQUEST_PARAMS if p.len() == 5 => Message::RequestParams {
                device,
                block: p[0],
                first: u14(p[1], p[2]),
                count: u14(p[3], p[4]),
            },
            msg::TRANSFER if p.len() == 1 => Message::Transfer {
                device,
                begin: p[0] == 0,
            },
            msg::CORRUPT_PRESET if p.len() == 4 => Message::CorruptPreset {
                device,
                number: u14(p[0], p[1]),
            },
            _ => Message::Unknown {
                device,
                kind,
                payload: p.to_vec(),
            },
        })
    }

    /// Serialize to a framed message.
    pub fn encode(&self) -> Vec<u8> {
        let (device, kind, payload): (u8, u8, Vec<u8>) = match self {
            Message::PresetDump {
                device,
                number,
                data,
            } => {
                assert_eq!(data.len(), PRESET_BYTES, "preset payload must be 516 bytes");
                let nib = to_nibbles(data);
                let mut p = vec![0x01];
                p.extend(split14(*number));
                p.extend_from_slice(&nib);
                p.push(nibble_checksum(&nib));
                (*device, msg::PRESET_DUMP, p)
            }
            Message::PresetRequest { device, number } => {
                let mut p = vec![0x01];
                p.extend(split14(*number));
                (*device, msg::REQUEST_PRESET, p)
            }
            Message::SetParam {
                device,
                block,
                param,
                values,
            } => {
                let mut p = vec![*block];
                p.extend(split14(*param));
                for v in values {
                    p.extend(i24_nibbles(*v));
                }
                (*device, msg::SET_PARAM, p)
            }
            Message::RequestParams {
                device,
                block,
                first,
                count,
            } => {
                let mut p = vec![*block];
                p.extend(split14(*first));
                p.extend(split14(*count));
                (*device, msg::REQUEST_PARAMS, p)
            }
            Message::Transfer { device, begin } => {
                (*device, msg::TRANSFER, vec![if *begin { 0 } else { 1 }])
            }
            Message::CorruptPreset { device, number } => {
                let mut p = split14(*number).to_vec();
                p.extend([0x00, 0x01]);
                (*device, msg::CORRUPT_PRESET, p)
            }
            Message::Unknown {
                device,
                kind,
                payload,
            } => (*device, *kind, payload.clone()),
        };
        let mut out = Vec::with_capacity(8 + payload.len());
        out.push(SYSEX_START);
        out.extend_from_slice(&TC_MANUFACTURER);
        out.push(device);
        out.push(MODEL_GSYSTEM);
        out.push(kind);
        out.extend_from_slice(&payload);
        out.push(SYSEX_END);
        out
    }

    /// Request the preset in user slot `slot`.
    pub fn preset_request(device: u8, slot: usize) -> Message {
        Message::PresetRequest {
            device,
            number: slot_to_number(slot),
        }
    }

    /// "Editor mode" on/off, as Vyzor sends it before a store (block 1, param 0x1D).
    pub fn editor_mode(device: u8, on: bool) -> Message {
        Message::SetParam {
            device,
            block: 0x01,
            param: 0x1D,
            values: vec![on as i32],
        }
    }
}

/// Firmware string from a Universal Device Inquiry reply, if it is a G-System.
pub fn parse_identity(bytes: &[u8]) -> Option<String> {
    // F0 7E <ch> 06 02 00 20 1F 55 <family/model…> <version…> F7
    if bytes.len() < 12 || bytes[1] != 0x7E || bytes[3] != 0x06 || bytes[4] != 0x02 {
        return None;
    }
    if bytes[5..8] != TC_MANUFACTURER || bytes[8] != MODEL_GSYSTEM {
        return None;
    }
    let tail = &bytes[9..bytes.len() - 1];
    Some(
        tail.iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Split a byte stream into framed SysEx messages, skipping anything outside F0..F7.
pub fn split_stream(bytes: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == SYSEX_START {
            if let Some(end) = bytes[i..].iter().position(|&b| b == SYSEX_END) {
                out.push(&bytes[i..i + end + 1]);
                i += end + 1;
                continue;
            }
            break;
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nibble_roundtrip() {
        let data: Vec<u8> = (0..=255).collect();
        assert_eq!(from_nibbles(&to_nibbles(&data)).unwrap(), data);
    }

    #[test]
    fn transfer_roundtrip() {
        let m = Message::Transfer {
            device: 0,
            begin: false,
        };
        let b = m.encode();
        assert_eq!(b, [0xF0, 0x00, 0x20, 0x1F, 0x00, 0x55, 0x57, 0x01, 0xF7]);
        assert_eq!(Message::parse(&b).unwrap(), m);
    }

    #[test]
    fn editor_mode_matches_vyzor_capture() {
        // From Twilight.Prs TRAN chunk: what we first misread as a preset request.
        let cap = [
            0xF0, 0x00, 0x20, 0x1F, 0x00, 0x55, 0x22, 0x01, 0x00, 0x1D, 0, 0, 0, 0, 0, 1, 0xF7,
        ];
        assert_eq!(Message::editor_mode(0, true).encode(), cap);
        assert_eq!(
            Message::parse(&cap).unwrap(),
            Message::SetParam {
                device: 0,
                block: 1,
                param: 0x1D,
                values: vec![1]
            }
        );
    }

    #[test]
    fn preset_request_encoding() {
        // user slot 100 → wire number 200 = 0x01 0x48 (14-bit)
        assert_eq!(
            Message::preset_request(0, 100).encode(),
            [0xF0, 0x00, 0x20, 0x1F, 0x00, 0x55, 0x45, 0x01, 0x01, 0x48, 0xF7]
        );
    }

    #[test]
    fn set_param_negative() {
        let m = Message::SetParam {
            device: 0x7F,
            block: 11,
            param: 12,
            values: vec![-25],
        };
        let b = m.encode();
        assert_eq!(&b[10..16], &[0x0F, 0x0F, 0x0F, 0x0F, 0x0E, 0x07]);
        assert_eq!(Message::parse(&b).unwrap(), m);
    }

    #[test]
    fn slot_numbers() {
        assert_eq!(slot_to_number(0), 100);
        assert_eq!(number_to_slot(299), Some(199));
        assert_eq!(number_to_slot(99), None);
        assert_eq!(number_to_slot(300), None);
    }
}
