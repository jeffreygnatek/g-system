//! SysEx framing for the G-System.
//!
//! Every message:
//! ```text
//! F0 00 20 1F <dev> 55 <type> <payload...> F7
//! ```
//! `00 20 1F` is TC Electronic's manufacturer ID, `0x55` is the G-System model
//! ID (the G-Major uses `0x48`). `<dev>` is `0x00` for a specific unit or
//! `0x7F` for "any device" (the editor uses both).

use std::fmt;

pub const SYSEX_START: u8 = 0xF0;
pub const SYSEX_END: u8 = 0xF7;
pub const TC_MANUFACTURER: [u8; 3] = [0x00, 0x20, 0x1F];
pub const MODEL_GSYSTEM: u8 = 0x55;
pub const DEVICE_ANY: u8 = 0x7F;

/// Message type byte (offset 6).
pub mod msg {
    /// Preset dump: `20 01 00 <preset#> <1032 nibbles> <chk>`.
    pub const PRESET_DUMP: u8 = 0x20;
    /// Request / select family. Observed sub-forms:
    /// `22 01 00 <n> 00 00 00 00 00 01` (17 bytes, preset request/select),
    /// `22 05 ...` (global/system data), `22 0D`/`22 0E` (routing tables).
    pub const REQUEST: u8 = 0x22;
    /// Parameter edit: `47 <block> <param> <lo> <hi>`-ish, 13 bytes total.
    pub const PARAM: u8 = 0x47;
    /// Firmware updater: enter/leave bootloader (`51 01`).
    pub const FW_CONTROL: u8 = 0x51;
    /// Firmware data block (305 bytes).
    pub const FW_BLOCK: u8 = 0x52;
    /// Edit mode on/off: `57 00` / `57 01`.
    pub const EDIT_MODE: u8 = 0x57;
}

/// Number of nibble bytes in a preset dump payload (516 data bytes × 2).
pub const PRESET_NIBBLES: usize = 1032;
/// Decoded preset payload size in bytes.
pub const PRESET_BYTES: usize = PRESET_NIBBLES / 2;
/// Total framed length of a preset dump message.
pub const PRESET_DUMP_LEN: usize = 10 + PRESET_NIBBLES + 2;

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
    if nibbles.len() % 2 != 0 {
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

/// A parsed G-System message. `Unknown` keeps the raw payload so nothing is lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    PresetDump {
        device: u8,
        preset: u8,
        data: Vec<u8>,
    },
    EditMode {
        device: u8,
        on: bool,
    },
    /// `22 01 00 <n> ...` — select/request preset `n`.
    PresetRequest {
        device: u8,
        preset: u8,
        tail: Vec<u8>,
    },
    /// `47 <block> <param> <value bytes...>`
    Param {
        device: u8,
        block: u8,
        param: u8,
        raw: Vec<u8>,
    },
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
        let payload = &bytes[7..bytes.len() - 1];
        Ok(match kind {
            msg::PRESET_DUMP if payload.len() == PRESET_NIBBLES + 4 && payload[0] == 0x01 => {
                let preset = payload[2];
                let nibbles = &payload[3..3 + PRESET_NIBBLES];
                let chk = payload[3 + PRESET_NIBBLES];
                let calc = nibble_checksum(nibbles);
                if chk != calc {
                    return Err(SysexError::BadChecksum {
                        expected: calc,
                        found: chk,
                    });
                }
                Message::PresetDump {
                    device,
                    preset,
                    data: from_nibbles(nibbles)?,
                }
            }
            msg::EDIT_MODE if payload.len() == 1 => Message::EditMode {
                device,
                on: payload[0] != 0,
            },
            msg::REQUEST if payload.len() >= 3 && payload[0] == 0x01 => Message::PresetRequest {
                device,
                preset: payload[2],
                tail: payload[3..].to_vec(),
            },
            msg::PARAM if payload.len() >= 2 => Message::Param {
                device,
                block: payload[0],
                param: payload[1],
                raw: payload[2..].to_vec(),
            },
            _ => Message::Unknown {
                device,
                kind,
                payload: payload.to_vec(),
            },
        })
    }

    /// Serialize to a framed message.
    pub fn encode(&self) -> Vec<u8> {
        let (device, kind, payload): (u8, u8, Vec<u8>) = match self {
            Message::PresetDump {
                device,
                preset,
                data,
            } => {
                assert_eq!(data.len(), PRESET_BYTES, "preset payload must be 516 bytes");
                let nib = to_nibbles(data);
                let mut p = vec![0x01, 0x00, *preset];
                p.extend_from_slice(&nib);
                p.push(nibble_checksum(&nib));
                (*device, msg::PRESET_DUMP, p)
            }
            Message::EditMode { device, on } => (*device, msg::EDIT_MODE, vec![*on as u8]),
            Message::PresetRequest {
                device,
                preset,
                tail,
            } => {
                let mut p = vec![0x01, 0x00, *preset];
                p.extend_from_slice(tail);
                (*device, msg::REQUEST, p)
            }
            Message::Param {
                device,
                block,
                param,
                raw,
            } => {
                let mut p = vec![*block, *param];
                p.extend_from_slice(raw);
                (*device, msg::PARAM, p)
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

    /// The canonical "request preset n" message as the Vyzor editor sends it.
    pub fn preset_request(device: u8, preset: u8) -> Message {
        Message::PresetRequest {
            device,
            preset,
            tail: vec![0, 0, 0, 0, 0, 1],
        }
    }
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
    fn edit_mode_roundtrip() {
        let m = Message::EditMode {
            device: 0,
            on: true,
        };
        let b = m.encode();
        assert_eq!(b, [0xF0, 0x00, 0x20, 0x1F, 0x00, 0x55, 0x57, 0x01, 0xF7]);
        assert_eq!(Message::parse(&b).unwrap(), m);
    }

    #[test]
    fn preset_request_matches_capture() {
        // From Twilight.Prs TRAN chunk
        let cap = [
            0xF0, 0x00, 0x20, 0x1F, 0x00, 0x55, 0x22, 0x01, 0x00, 0x1D, 0, 0, 0, 0, 0, 1, 0xF7,
        ];
        assert_eq!(Message::preset_request(0, 0x1D).encode(), cap);
    }
}
