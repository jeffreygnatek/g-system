//! Modifiers: per preset, a pedal or encoder controls an effect parameter
//! between Min, Mid and Max (Edit › Enter › Enter on the unit, Modifier page
//! in Vyzor).
//!
//! Rust port of `web/src/core/modifiers.ts` from davidavalanches/gsystem-editor
//! (commit 9b50ef8), Copyright © David Avalanches, GPL-3.0.
//!
//! Preset fields 125-162 hold 38 modifier slots, grouped by block: Filter 4,
//! Comp 3, Mod 5, Pitch 8, Delay 10, Reverb 4, NovaDrive 4. A block's slots
//! follow the list of parameters its current effect type lets a modifier
//! control, in parameter order. Changing the type keeps the slots, so they
//! then apply to the new type's list.
//!
//! Each slot is a 24-bit word: source in bits 0-2 (0 none, 1 Vol, 2 Exp, 3-5
//! encoders A-C), then Min, Mid and Max in 7 bits each, as a percentage of the
//! parameter's range. Unassigned is source 0, 0 / 50 / 100 %. The lists of
//! controllable parameters come from the bit masks of firmware 4.02.

use crate::params::{block, FieldParam};
use crate::preset::Preset;

pub const MODIFIER_FIELD: usize = 125;
pub const MODIFIER_SLOTS: usize = 38;
/// `55 22` block of the modifiers; slot n is parameter 20 + n.
pub const MODIFIER_BLOCK_ID: u8 = 0x0D;
pub const MODIFIER_PARAM_BASE: u16 = 20;

pub const SOURCES: [&str; 6] = ["None", "Vol", "Exp", "Enc A", "Enc B", "Enc C"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modifier {
    pub source: u8,
    pub min: u8,
    pub mid: u8,
    pub max: u8,
}

pub const UNASSIGNED: Modifier = Modifier {
    source: 0,
    min: 0,
    mid: 50,
    max: 100,
};

pub fn decode_modifier(value: i32) -> Modifier {
    let raw = value as u32 & 0xFF_FFFF;
    Modifier {
        source: (raw & 7) as u8,
        min: ((raw >> 3) & 0x7F) as u8,
        mid: ((raw >> 10) & 0x7F) as u8,
        max: ((raw >> 17) & 0x7F) as u8,
    }
}

/// The field value (signed, as `Preset::param_i24` returns it).
pub fn encode_modifier(m: Modifier) -> i32 {
    let pct = |v: u8| v.min(100) as u32;
    let raw = (m.source as u32 & 7) | (pct(m.min) << 3) | (pct(m.mid) << 10) | (pct(m.max) << 17);
    ((raw << 8) as i32) >> 8
}

/// Values the unit cannot have set (seen in presets made with older firmware).
pub fn is_valid_modifier(m: Modifier) -> bool {
    (m.source as usize) < SOURCES.len() && m.min <= 100 && m.mid <= 100 && m.max <= 100
}

struct SlotGroup {
    key: &'static str,
    first: usize,
    size: usize,
    /// Per effect type (in the block's type order): parameter indices a modifier can control.
    targets: &'static [&'static [usize]],
}

const CHORUS: &[usize] = &[4, 6, 7, 8];
const FLANGER: &[usize] = &[4, 6, 7, 8, 12];
const DELAY: &[usize] = &[2, 9, 15, 16, 19, 20];
const TAPE: &[usize] = &[2, 9, 12, 15, 16, 19, 20];
const PING_PONG: &[usize] = &[2, 9, 15, 16, 19, 20, 21];

const GROUPS: [SlotGroup; 6] = [
    SlotGroup {
        key: "filter",
        first: 0,
        size: 4,
        targets: &[
            &[8, 9, 12],
            &[12],
            &[10, 12],
            &[8, 10],
            &[10],
            &[6, 10],
            &[7, 9, 14],
            &[9, 14],
            &[9, 12, 14],
        ],
    },
    SlotGroup {
        key: "comp",
        first: 4,
        size: 3,
        targets: &[&[2, 3, 6], &[6, 7], &[6, 7]],
    },
    SlotGroup {
        key: "mod",
        first: 7,
        size: 5,
        targets: &[
            CHORUS,
            CHORUS,
            FLANGER,
            FLANGER,
            &[4, 5, 6, 8],
            &[4, 6, 9, 10],
            &[4, 6, 9, 10],
            &[4, 5, 7, 8, 10],
            &[4, 5, 10],
            &[5, 6, 7, 10, 12],
            &[5, 6, 7, 10, 12],
        ],
    },
    SlotGroup {
        key: "pitch",
        first: 12,
        size: 8,
        targets: &[
            &[2, 3, 4, 5, 8, 9, 12, 13],
            &[12, 13],
            &[13, 16],
            &[12, 13, 17, 18],
            &[10, 11, 12, 13],
            &[],
        ],
    },
    SlotGroup {
        key: "delay",
        first: 20,
        size: 10,
        targets: &[
            DELAY,
            TAPE,
            DELAY,
            TAPE,
            PING_PONG,
            DELAY,
            &[3, 4, 10, 11, 15, 16, 17, 18, 19, 20],
        ],
    },
    SlotGroup {
        key: "reverb",
        first: 30,
        size: 4,
        targets: &[&[2, 13, 14], &[2, 8, 13, 14], &[2, 13, 14], &[2, 9, 13, 14]],
    },
];

/// Shown by the unit when a NovaDrive NDR-1 pedal is connected to its MIDI ports.
pub const NOVADRIVE_FIRST: usize = 34;
pub const NOVADRIVE_NAMES: [&str; 4] = ["OD Gain", "OD Level", "DS Gain", "DS Level"];

#[derive(Debug, Clone, PartialEq)]
pub struct ModifierTarget {
    /// Block key ("delay"…), or "novadrive".
    pub key: &'static str,
    pub slot: usize,
    pub field: usize,
    /// Preset field of the controlled parameter, None for NovaDrive.
    pub param_field: Option<usize>,
    pub param: Option<FieldParam>,
    pub name: String,
    pub modifier: Modifier,
}

pub fn modifier_field(slot: usize) -> usize {
    MODIFIER_FIELD + slot
}

pub fn read_modifier(p: &Preset, slot: usize) -> Modifier {
    decode_modifier(p.param_i24(modifier_field(slot)))
}

pub fn write_modifier(p: &mut Preset, slot: usize, m: Modifier) {
    p.set_param_i24(modifier_field(slot), encode_modifier(m));
}

/// The parameters of this preset a modifier can control, as the unit lists them.
pub fn modifier_targets(p: &Preset, nova_drive: bool) -> Vec<ModifierTarget> {
    let mut out = Vec::new();
    for g in GROUPS.iter() {
        let blk = block(g.key);
        let ty = p.param_i24(blk.first_field());
        let params: Vec<FieldParam> = blk.fields(p).into_iter().map(|(_, fp)| fp).collect();
        let targets = usize::try_from(ty)
            .ok()
            .and_then(|t| g.targets.get(t))
            .copied()
            .unwrap_or(&[]);
        for (n, &index) in targets.iter().take(g.size).enumerate() {
            let slot = g.first + n;
            let param = params.get(index).copied();
            out.push(ModifierTarget {
                key: g.key,
                slot,
                field: modifier_field(slot),
                param_field: Some(blk.first_field() + index),
                param,
                name: param
                    .map(|fp| fp.name().to_string())
                    .unwrap_or_else(|| format!("Param {index}")),
                modifier: read_modifier(p, slot),
            });
        }
    }
    if nova_drive {
        for (n, name) in NOVADRIVE_NAMES.iter().enumerate() {
            let slot = NOVADRIVE_FIRST + n;
            out.push(ModifierTarget {
                key: "novadrive",
                slot,
                field: modifier_field(slot),
                param_field: None,
                param: None,
                name: name.to_string(),
                modifier: read_modifier(p, slot),
            });
        }
    }
    out
}

/// Parameter value at a percentage of its range, assuming the unit scales linearly.
pub fn value_at(param: &FieldParam, percent: u8) -> i32 {
    let (lo, hi) = param.range();
    (lo as f64 + (percent as f64 / 100.0) * (hi - lo) as f64).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT_0D: [u32; 10] = [
        0x040404, 0x989804, 0x040404, 0x040404, 0x030303, 0x030303, 0x030303, 0xc8c800, 0xc8c800,
        0x243002,
    ];

    #[test]
    fn decodes_the_units_slot_words() {
        assert_eq!(decode_modifier(0xc8c800), UNASSIGNED);
        assert_eq!(
            decode_modifier(0x243002),
            Modifier {
                source: 2,
                min: 0,
                mid: 12,
                max: 18
            }
        );
        assert_eq!(
            decode_modifier(0xc918f1),
            Modifier {
                source: 1,
                min: 30,
                mid: 70,
                max: 100
            }
        );
        for raw in UNIT_0D {
            assert_eq!(
                encode_modifier(decode_modifier(raw as i32)) as u32 & 0xFF_FFFF,
                raw
            );
        }
    }

    #[test]
    fn stores_slots_in_fields_125_162() {
        let mut p = Preset::blank("");
        write_modifier(
            &mut p,
            13,
            Modifier {
                source: 2,
                min: 0,
                mid: 50,
                max: 100,
            },
        );
        assert_eq!(
            p.param_i24(MODIFIER_FIELD + 13) as u32 & 0xFF_FFFF,
            0xc8c802
        );
        assert_eq!(MODIFIER_FIELD + MODIFIER_SLOTS, 163);
    }

    #[test]
    fn lists_the_controllable_parameters_of_the_current_types() {
        let mut p = Preset::blank("");
        for (key, ty) in [("pitch", 2), ("delay", 1), ("reverb", 3), ("mod", 0)] {
            p.set_param_i24(block(key).first_field(), ty);
        }
        let names = |key: &str| -> Vec<String> {
            modifier_targets(&p, false)
                .into_iter()
                .filter(|t| t.key == key)
                .map(|t| format!("{}:{}", t.slot, t.name))
                .collect()
        };
        assert_eq!(names("pitch"), ["12:OutLev", "13:Pitch"]);
        assert_eq!(
            names("delay"),
            [
                "20:Delay",
                "21:Fb",
                "22:Drive",
                "23:Fbhcut",
                "24:Fblcut",
                "25:Mix",
                "26:OutLev"
            ]
        );
        assert_eq!(
            names("reverb"),
            ["30:Decay", "31:Lo Lev", "32:Mix", "33:OutLev"]
        );
        assert_eq!(names("mod"), ["7:Speed", "8:HiCut", "9:Mix", "10:OutLev"]);
        let nova: Vec<_> = modifier_targets(&p, true)
            .into_iter()
            .filter(|t| t.key == "novadrive")
            .map(|t| t.slot)
            .collect();
        assert_eq!(nova, [34, 35, 36, 37]);
    }
}
