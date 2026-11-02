//! Loops, insert, boost, routing, relays and output levels of a preset,
//! packed into fields 0 and 2.
//!
//! Rust port of `web/src/core/levels.ts` from davidavalanches/gsystem-editor
//! (commit 9b50ef8), Copyright © David Avalanches, GPL-3.0.
//!
//! Field 0:
//! - bits 0-1   routing, see [`ROUTINGS`]
//! - bit  2     boost on
//! - bits 4-7   boost level in dB
//! - bit  8     relay switch (on = setting A drives the Relay jacks, off = B)
//! - bits 9-12  loops 1 to 4
//! - bit  13    insert loop
//! - bits 16-23 relay settings A and B, one nibble each
//!
//! Field 2: Left and Right output levels, two signed 12-bit values (left low).
//! Matched by that project against presets read from a unit and sent by
//! Vyzor; loops 1, 3 and 4 follow the bit order rather than an observation.

use crate::preset::Preset;

pub const LEVELS_FIELD: usize = 0;
pub const OUTPUTS_FIELD: usize = 2;
/// Routing names in the firmware's order.
pub const ROUTINGS: [&str; 4] = ["Serial", "Parallel", "Semi Parallel", "Serial2"];
pub const BOOST_LEVEL_MAX: i32 = 10;
pub const OUTPUT_MIN: i32 = -100;
pub const RELAY_CONTACTS: usize = 4;

/// Bit of each switch in field 0.
pub fn level_bit(key: &str) -> Option<u32> {
    Some(match key {
        "boost" => 2,
        "relay" => 8,
        "loop1" => 9,
        "loop2" => 10,
        "loop3" => 11,
        "loop4" => 12,
        "insert" => 13,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresetLevels {
    pub loops: [bool; 4],
    pub insert: bool,
    pub boost: bool,
    pub boost_level: i32,
    pub routing: usize,
}

fn raw(p: &Preset, field: usize) -> u32 {
    (p.param_i24(field) as u32) & 0xFF_FFFF
}
fn signed(raw: u32, bits: u32) -> i32 {
    ((raw << (32 - bits)) as i32) >> (32 - bits)
}

pub fn preset_levels(p: &Preset) -> PresetLevels {
    let r = raw(p, LEVELS_FIELD);
    let bit = |n: u32| (r >> n) & 1 == 1;
    PresetLevels {
        loops: [bit(9), bit(10), bit(11), bit(12)],
        insert: bit(13),
        boost: bit(2),
        boost_level: ((r >> 4) & 0x0F) as i32,
        routing: (r & 3) as usize,
    }
}

pub fn level_on(p: &Preset, key: &str) -> bool {
    level_bit(key)
        .map(|b| (raw(p, LEVELS_FIELD) >> b) & 1 == 1)
        .unwrap_or(false)
}

pub fn set_level(p: &mut Preset, key: &str, on: bool) {
    if let Some(b) = level_bit(key) {
        let r = raw(p, LEVELS_FIELD);
        let r = if on { r | (1 << b) } else { r & !(1 << b) };
        p.set_param_i24(LEVELS_FIELD, signed(r, 24));
    }
}

pub fn set_routing(p: &mut Preset, routing: usize) {
    let r = (raw(p, LEVELS_FIELD) & 0xFF_FFFC) | (routing as u32 & 3);
    p.set_param_i24(LEVELS_FIELD, signed(r, 24));
}

/// Boost level, 0 to 10 dB (the unit also caps it at the global Boost Max).
pub fn set_boost_level(p: &mut Preset, level: i32) {
    let v = level.clamp(0, BOOST_LEVEL_MAX) as u32;
    let r = (raw(p, LEVELS_FIELD) & 0xFF_FF0F) | (v << 4);
    p.set_param_i24(LEVELS_FIELD, signed(r, 24));
}

/// Left and right output levels in dB.
pub fn preset_outputs(p: &Preset) -> (i32, i32) {
    let r = raw(p, OUTPUTS_FIELD);
    (signed(r & 0xFFF, 12), signed(r >> 12, 12))
}

pub fn set_outputs(p: &mut Preset, left: i32, right: i32) {
    let r = (left as u32 & 0xFFF) | ((right as u32 & 0xFFF) << 12);
    p.set_param_i24(OUTPUTS_FIELD, signed(r, 24));
}

/// Relay settings A and B, 4 bits each: bit n closes contact n + 1.
pub fn preset_relays(p: &Preset) -> (u8, u8) {
    let r = raw(p, LEVELS_FIELD);
    (((r >> 16) & 0x0F) as u8, ((r >> 20) & 0x0F) as u8)
}

pub fn set_relay(p: &mut Preset, which: usize, value: u8) {
    let shift = if which == 0 { 16 } else { 20 };
    let r = (raw(p, LEVELS_FIELD) & !(0x0F << shift)) | ((value as u32 & 0x0F) << shift);
    p.set_param_i24(LEVELS_FIELD, signed(r & 0xFF_FFFF, 24));
}

/// "OCCO": open (O) or closed (C), contact 1 first, as the unit shows it.
pub fn relay_text(value: u8) -> String {
    (0..RELAY_CONTACTS)
        .map(|i| if (value >> i) & 1 == 1 { 'C' } else { 'O' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_levels(raw24: u32) -> Preset {
        let mut p = Preset::blank("");
        p.set_param_i24(LEVELS_FIELD, signed(raw24, 24));
        p
    }

    #[test]
    fn boost_level_keeps_other_bits() {
        let mut p = with_levels(0xAD202C);
        set_boost_level(&mut p, 7);
        assert_eq!(
            preset_levels(&p),
            PresetLevels {
                loops: [false; 4],
                insert: true,
                boost: true,
                boost_level: 7,
                routing: 0
            }
        );
    }

    #[test]
    fn factory_preset_loops_off_6db_boost() {
        assert_eq!(
            preset_levels(&with_levels(0x68)),
            PresetLevels {
                loops: [false; 4],
                insert: false,
                boost: false,
                boost_level: 6,
                routing: 0
            }
        );
    }

    #[test]
    fn relays() {
        let chute = with_levels(0xC62508);
        assert_eq!(preset_relays(&chute), (6, 12));
        assert_eq!(relay_text(6), "OCCO");
        assert_eq!(relay_text(12), "OOCC");
        assert!(level_on(&chute, "relay"));
        assert!(!level_on(&with_levels(0xAD202C), "relay"));

        let mut out = chute.clone();
        set_relay(&mut out, 1, 0b0001);
        assert_eq!(preset_relays(&out), (6, 1));
        assert_eq!(preset_levels(&out), preset_levels(&chute));
        assert_eq!(
            raw(&out, LEVELS_FIELD) & 0x0F_FFFF,
            raw(&chute, LEVELS_FIELD) & 0x0F_FFFF
        );

        let mut off = chute.clone();
        set_level(&mut off, "relay", false);
        assert!(!level_on(&off, "relay"));
        assert_eq!(preset_relays(&off), (6, 12));
        assert_eq!(preset_levels(&off), preset_levels(&chute));
    }

    #[test]
    fn outputs_roundtrip() {
        let mut p = Preset::blank("");
        set_outputs(&mut p, -3, -100);
        assert_eq!(preset_outputs(&p), (-3, -100));
    }
}
