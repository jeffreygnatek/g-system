//! NovaDrive (TC NDR-1 pedal on the G-System's NDR port): a preset only keeps
//! which of the pedal's 18 presets to recall.
//!
//! Rust port of `web/src/core/novadrive.ts` from davidavalanches/gsystem-editor
//! (commit 9b50ef8), Copyright © David Avalanches, GPL-3.0.
//!
//! Found in firmware 4.05: field 163 is an extension header, `0xEDAB00 |`
//! number of words that follow (1); field 164 is the NovaDrive word (block 14
//! parameter 0), with the pedal preset (1-18, 0 = none) in bits 16-23. Bits
//! 0-15 are not read by the firmware (Vyzor writes 0xFFFF, the unit 0x0000)
//! and are kept as found. Without the header the unit takes the word as 0.

use crate::preset::Preset;

pub const NOVA_HEADER_FIELD: usize = 163;
pub const NOVA_FIELD: usize = 164;
pub const NOVA_HEADER: u32 = 0xEDAB01;
/// Block id of the NovaDrive in `55 22` messages.
pub const NOVA_BLOCK_ID: u8 = 14;
pub const NOVA_PRESETS: i32 = 18;

fn signed(raw: u32) -> i32 {
    ((raw << 8) as i32) >> 8
}

fn has_header(p: &Preset) -> bool {
    let h = p.param_i24(NOVA_HEADER_FIELD) as u32 & 0xFF_FFFF;
    (h & 0xFF_FF00) == 0xED_AB00 && (h & 0xFF) >= 1
}

/// The NovaDrive word as the unit holds it.
pub fn nova_word(p: &Preset) -> u32 {
    if has_header(p) {
        p.param_i24(NOVA_FIELD) as u32 & 0xFF_FFFF
    } else {
        0
    }
}

/// The pedal preset recalled with this preset: 1-18, or 0 for none.
pub fn nova_preset(p: &Preset) -> i32 {
    let n = (nova_word(p) >> 16) as i32;
    if n <= NOVA_PRESETS {
        n
    } else {
        0
    }
}

pub fn write_nova_word(p: &mut Preset, word: u32) {
    if !has_header(p) {
        p.set_param_i24(NOVA_HEADER_FIELD, signed(NOVA_HEADER));
    }
    p.set_param_i24(NOVA_FIELD, signed(word & 0xFF_FFFF));
}

pub fn write_nova_preset(p: &mut Preset, preset: i32) {
    let n = preset.clamp(0, NOVA_PRESETS) as u32;
    let low = if has_header(p) {
        p.param_i24(NOVA_FIELD) as u32 & 0xFFFF
    } else {
        0
    };
    write_nova_word(p, (n << 16) | low);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(header: u32, word: u32) -> Preset {
        let mut p = Preset::blank("");
        p.set_param_i24(NOVA_HEADER_FIELD, signed(header));
        p.set_param_i24(NOVA_FIELD, signed(word));
        p
    }

    #[test]
    fn reads_the_preset_only_behind_the_header() {
        let vyzor = with(0xEDAB01, 0x12FFFF);
        assert_eq!(nova_preset(&vyzor), 18);
        assert_eq!(nova_word(&vyzor), 0x12FFFF);
        assert_eq!(nova_preset(&with(0, 0x12FFFF)), 0);
    }

    #[test]
    fn writes_the_header_and_keeps_the_low_bits() {
        let mut set = Preset::blank("");
        write_nova_preset(&mut set, 3);
        assert_eq!(
            set.param_i24(NOVA_HEADER_FIELD) as u32 & 0xFF_FFFF,
            0xEDAB01
        );
        assert_eq!(nova_word(&set), 0x030000);
        let mut vyzor = with(0xEDAB01, 0x01FFFF);
        write_nova_preset(&mut vyzor, 0);
        assert_eq!(nova_word(&vyzor), 0x00FFFF);
    }
}
