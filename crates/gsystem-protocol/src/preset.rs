//! A decoded G-System preset: the 516-byte payload of a `0x20` dump.
//!
//! Layout (confirmed from 100 factory presets):
//! - bytes 0..20: name (20 chars max, NUL-terminated; leftover bytes after
//!   the NUL are not cleared by the editor), byte 20 is always 0
//! - bytes 21..516: 165 fields, each a little-endian signed 24-bit integer.
//!   See [`crate::params`] for the block layout and [`crate::levels`] for
//!   the packed loops/boost/routing field.

use crate::sysex::{slot_to_number, Message, PRESET_BYTES};

pub const NAME_LEN: usize = 20;
pub const PARAMS_OFFSET: usize = 21;
/// Number of 3-byte parameter slots after the name.
pub const PARAM_COUNT: usize = (PRESET_BYTES - PARAMS_OFFSET) / 3;

#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Preset {
    pub raw: Vec<u8>,
}

impl std::fmt::Debug for Preset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Preset")
            .field("name", &self.name())
            .finish()
    }
}

impl Preset {
    pub fn from_raw(raw: Vec<u8>) -> Option<Preset> {
        (raw.len() == PRESET_BYTES).then_some(Preset { raw })
    }

    /// An all-zero preset with the given name (useful as an "empty" slot).
    pub fn blank(name: &str) -> Preset {
        let mut p = Preset {
            raw: vec![0; PRESET_BYTES],
        };
        p.set_name(name);
        p
    }

    pub fn name(&self) -> String {
        let field = &self.raw[..NAME_LEN];
        let end = field.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
        // The unit sets bit 7 on some name bytes as a flag (kept in `raw`);
        // the character is the low 7 bits.
        field[..end]
            .iter()
            .map(|&b| (b & 0x7F) as char)
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    /// Set the name (Latin-1, truncated to 20 chars, NUL-padded).
    pub fn set_name(&mut self, name: &str) {
        let field = &mut self.raw[..NAME_LEN];
        field.fill(0);
        for (dst, ch) in field.iter_mut().zip(name.chars()) {
            *dst = if (ch as u32) < 256 { ch as u8 } else { b'?' };
        }
    }

    /// Parameter `i` as a little-endian signed 24-bit integer.
    pub fn param_i24(&self, i: usize) -> i32 {
        let o = PARAMS_OFFSET + i * 3;
        let b = &self.raw[o..o + 3];
        let v = (b[0] as u32) | ((b[1] as u32) << 8) | ((b[2] as u32) << 16);
        ((v << 8) as i32) >> 8
    }

    pub fn set_param_i24(&mut self, i: usize, v: i32) {
        let o = PARAMS_OFFSET + i * 3;
        let u = v as u32;
        self.raw[o] = u as u8;
        self.raw[o + 1] = (u >> 8) as u8;
        self.raw[o + 2] = (u >> 16) as u8;
    }

    pub fn params_i24(&self) -> Vec<i32> {
        (0..PARAM_COUNT).map(|i| self.param_i24(i)).collect()
    }

    /// Build the dump message for user slot `slot`.
    pub fn to_message(&self, device: u8, slot: usize) -> Message {
        Message::PresetDump {
            device,
            number: slot_to_number(slot),
            data: self.raw.clone(),
        }
    }
}

/// "BB-P" label the editor and unit use for slot `index` (0-based).
pub fn slot_label(index: usize) -> String {
    format!("{:02}-{}", index / 5, index % 5 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i24_sign() {
        let mut p = Preset::blank("x");
        p.set_param_i24(0, -20);
        assert_eq!(&p.raw[21..24], &[0xEC, 0xFF, 0xFF]);
        assert_eq!(p.param_i24(0), -20);
        p.set_param_i24(1, 500);
        assert_eq!(&p.raw[24..27], &[0xF4, 0x01, 0x00]);
        assert_eq!(p.param_i24(1), 500);
    }

    #[test]
    fn labels() {
        assert_eq!(slot_label(0), "00-1");
        assert_eq!(slot_label(4), "00-5");
        assert_eq!(slot_label(99), "19-5");
    }
}
