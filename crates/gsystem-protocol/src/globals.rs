//! Global settings (Edit › Menu on the unit): levels, utility, pedals, MIDI
//! and tuner. Not part of presets; the unit applies a change at once.
//!
//! Rust port of `web/src/core/globals.ts` from davidavalanches/gsystem-editor
//! (commit 9b50ef8), Copyright © David Avalanches, GPL-3.0.
//!
//! They live in `55 22` parameter blocks, read with `55 47`:
//! - block 1: levels and locks (mixed with the current preset's loops/routing);
//! - block 5: system settings, then the program map (26-153) and the switches
//!   (226-243, see [`crate::switches`]);
//! - Volume Ctrl. is the source bits (as in a modifier) of block 13 param 58.
//!
//! Identified by changing each setting on the unit and watching its messages;
//! the rest of block 5 follows firmware 4.02's global table. Value lists are
//! the firmware's.

use crate::switches::{SWITCH_BLOCK, SWITCH_COUNT, SWITCH_FIRST};

pub const LEVELS_BLOCK: u8 = 1;
pub const SYSTEM_BLOCK: u8 = 5;
/// Block 5 parameter of the unit's MIDI channel (0 Off, 1-16, 17 Omni).
pub const MIDI_CHANNEL_PARAM: u16 = 15;

/// Ranges read in one `55 47` request each: (block, first parameter, count).
pub const GLOBAL_RANGES: [(u8, u16, u16); 4] = [
    (LEVELS_BLOCK, 0, 30),
    (SYSTEM_BLOCK, 3, 23),
    (0x0D, 58, 1),
    (SWITCH_BLOCK, SWITCH_FIRST, SWITCH_COUNT as u16),
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GFmt {
    Plain,
    Db,
    Hz,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobalParam {
    pub block: u8,
    pub param: u16,
    pub name: &'static str,
    pub lo: i32,
    pub hi: i32,
    pub choices: &'static [&'static str],
    pub fmt: GFmt,
    pub help: &'static str,
    /// Only these bits are the setting; the others are kept when writing.
    pub mask: Option<i32>,
}

#[derive(Debug, Clone, Copy)]
pub struct GlobalGroup {
    pub title: &'static str,
    pub params: &'static [GlobalParam],
}

const ONOFF: &[&str] = &["Off", "On"];
const PRESET_GLOBAL: &[&str] = &["Preset", "Global"];
const PEDAL_TYPES: &[&str] = &["Off", "Momentary", "Alternate", "Exp.", "Vol."];
const CHANNELS: &[&str] = &[
    "Off", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
    "Omni",
];

const fn p(
    block: u8,
    param: u16,
    name: &'static str,
    lo: i32,
    hi: i32,
    fmt: GFmt,
    help: &'static str,
) -> GlobalParam {
    GlobalParam {
        block,
        param,
        name,
        lo,
        hi,
        choices: &[],
        fmt,
        help,
        mask: None,
    }
}
const fn e(
    block: u8,
    param: u16,
    name: &'static str,
    choices: &'static [&'static str],
    help: &'static str,
) -> GlobalParam {
    GlobalParam {
        block,
        param,
        name,
        lo: 0,
        hi: choices.len() as i32 - 1,
        choices,
        fmt: GFmt::Plain,
        help,
        mask: None,
    }
}

pub static GLOBAL_GROUPS: [GlobalGroup; 5] = [
    GlobalGroup {
        title: "Gbl Levels",
        params: &[
            // Stored in dB, as the firmware table gives it (-8 to 18) and the unit sends it.
            p(
                1,
                3,
                "Input Gain",
                -8,
                18,
                GFmt::Db,
                "Instrument input gain, from −8 to +18 dB",
            ),
            p(
                1,
                4,
                "Loop Level",
                0,
                10,
                GFmt::Db,
                "Level sent into the loops",
            ),
            p(
                1,
                5,
                "Loop Headroom",
                2,
                6,
                GFmt::Db,
                "Headroom at the effects input, from 2 to 6 dB (firmware range)",
            ),
            p(1, 6, "Volume", -100, 0, GFmt::Db, ""),
            e(
                1,
                7,
                "Volume Pos.",
                &["Input", "Output"],
                "Volume before (Input) or after (Output) the effects",
            ),
            p(1, 8, "Output Level", -100, 0, GFmt::Db, ""),
            p(
                1,
                12,
                "Boost Max",
                0,
                10,
                GFmt::Db,
                "Maximum boost allowed per preset",
            ),
            e(
                1,
                2,
                "Input Select",
                &["Rear", "Front"],
                "Guitar input used on the GFX01",
            ),
        ],
    },
    GlobalGroup {
        title: "Utility",
        params: &[
            e(
                5,
                3,
                "Tap Master",
                PRESET_GLOBAL,
                "On preset change: the preset's tempo or the tapped tempo",
            ),
            e(
                1,
                24,
                "Insert Lock",
                ONOFF,
                "Keeps the Insert loop state for all presets",
            ),
            e(1, 13, "Boost Lock", ONOFF, ""),
            e(1, 14, "EQ Lock", ONOFF, ""),
            e(1, 25, "Relay Lock", ONOFF, ""),
            e(1, 1, "Routing Lock", ONOFF, ""),
            e(
                1,
                28,
                "Fact. Bank Lock",
                ONOFF,
                "Makes the factory banks inaccessible",
            ),
            p(5, 8, "View Angle", 0, 127, GFmt::Plain, "Display contrast"),
            e(5, 25, "KillDry", ONOFF, ""),
        ],
    },
    GlobalGroup {
        title: "Pedals",
        params: &[
            GlobalParam {
                mask: Some(7),
                ..e(
                    0x0D,
                    58,
                    "Volume Ctrl.",
                    &["None", "Vol", "Exp", "Enc A", "Enc B", "Enc C"],
                    "Pedal (or encoder) that controls the volume",
                )
            },
            e(5, 9, "GFX1 Vol. Type", PEDAL_TYPES, ""),
            e(5, 10, "GFX1 Exp. Type", PEDAL_TYPES, ""),
            e(5, 11, "Ctrl Vol. Type", PEDAL_TYPES, ""),
            e(5, 12, "Ctrl Exp. Type", PEDAL_TYPES, ""),
            e(
                5,
                13,
                "Vol. Master",
                PRESET_GLOBAL,
                "On preset change: the preset's value or the pedal position",
            ),
            e(5, 14, "Exp. Master", PRESET_GLOBAL, ""),
        ],
    },
    GlobalGroup {
        title: "MIDI",
        params: &[
            e(5, 15, "MIDI Channel", CHANNELS, ""),
            e(5, 16, "Prg. Change In", ONOFF, ""),
            e(5, 17, "Prg. Change Out", &["Off", "On", "On Map"], ""),
            p(
                5,
                19,
                "SysEx ID",
                0,
                126,
                GFmt::Plain,
                "Must match the SysEx ID used for Store",
            ),
            e(5, 20, "MIDI Clock In", ONOFF, ""),
            // CC 120-127 are MIDI channel mode messages: the firmware stops at 119.
            p(5, 21, "Enc. A CC", 0, 119, GFmt::Plain, ""),
            p(5, 22, "Enc. B CC", 0, 119, GFmt::Plain, ""),
            p(5, 23, "Enc. C CC", 0, 119, GFmt::Plain, ""),
            p(5, 24, "Tap CC", 0, 119, GFmt::Plain, ""),
        ],
    },
    GlobalGroup {
        title: "Tuner",
        params: &[
            e(
                5,
                5,
                "Output",
                &["Mute", "On"],
                "Whether the output is muted in tuner mode",
            ),
            e(5, 6, "Mode", &["Bar", "Strobe"], ""),
            p(5, 7, "Reference", 420, 460, GFmt::Hz, ""),
        ],
    },
];

impl GlobalParam {
    /// The setting's value within the parameter's raw value.
    pub fn value(&self, raw: i32) -> i32 {
        self.mask.map(|m| raw & m).unwrap_or(raw)
    }
    /// The raw value to send for a new setting.
    pub fn raw_for(&self, raw: i32, value: i32) -> i32 {
        self.mask.map(|m| (raw & !m) | value).unwrap_or(value)
    }
    pub fn format(&self, v: i32) -> String {
        if !self.choices.is_empty() {
            return usize::try_from(v - self.lo)
                .ok()
                .and_then(|i| self.choices.get(i))
                .map(|c| c.to_string())
                .unwrap_or(v.to_string());
        }
        match self.fmt {
            GFmt::Plain => v.to_string(),
            GFmt::Db => format!("{v} dB"),
            GFmt::Hz => format!("{v} Hz"),
        }
    }
}

pub fn global_param(block: u8, param: u16) -> Option<&'static GlobalParam> {
    GLOBAL_GROUPS
        .iter()
        .flat_map(|g| g.params.iter())
        .find(|g| g.block == block && g.param == param)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_gain_in_db_as_the_unit_sends_it() {
        let g = global_param(1, 3).unwrap();
        assert_eq!(g.format(g.value(8)), "8 dB");
        assert_eq!(g.format(g.value(-8)), "-8 dB");
        assert_eq!((g.lo, g.hi), (-8, 18));
    }

    #[test]
    fn volume_ctrl_keeps_other_bits() {
        let g = global_param(0x0D, 58).unwrap();
        let raw = 0xc8c800 | 1;
        assert_eq!(g.format(g.value(raw)), "Vol");
        assert_eq!(g.raw_for(raw, 2), 0xc8c800 | 2);
    }
}
