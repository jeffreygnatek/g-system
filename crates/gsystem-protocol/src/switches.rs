//! Footswitch assignments (Edit › Menu › Switches): global, not per preset.
//! Block 5 parameters 226-243 hold switches #1 to #18.
//!
//! Rust port of `web/src/core/switches.ts` from davidavalanches/gsystem-editor
//! (commit 9b50ef8), Copyright © David Avalanches, GPL-3.0.
//!
//! Each value packs three bytes: the function, then its first and second
//! parameters (style Toggle / Hold, or for Preset the bank and the preset, or
//! for MIDI CC the controller number). The function order is the manual's
//! table, confirmed against values read from a unit.

pub const SWITCH_BLOCK: u8 = 5;
pub const SWITCH_FIRST: u16 = 226;
pub const SWITCH_COUNT: usize = 18;

/// Switches with a fixed function (not in the unit's Switches menu), by index.
pub fn fixed_switch(index: usize) -> Option<&'static str> {
    match index {
        5 => Some("EDIT"),
        6 => Some("UP"),
        12 => Some("DOWN"),
        _ => None,
    }
}

pub const FUNCTIONS: [&str; 20] = [
    "None",
    "Filter",
    "Comp",
    "Loop 1",
    "Loop 2",
    "Loop 3",
    "Loop 4",
    "Insert",
    "Relay",
    "Gate",
    "Boost",
    "Mod",
    "Pitch",
    "Delay",
    "Reverb",
    "EQ",
    "Preset",
    "Tap Tempo",
    "Tuner Mute",
    "MIDI CC",
];
pub const PRESET: u8 = 16;
pub const TAP_TEMPO: u8 = 17;
pub const MIDI_CC: u8 = 19;
pub const STYLES: [&str; 2] = ["Toggle", "Hold"];

/// Bank parameter of a Preset switch: none (UP/DOWN decide), factory A0-A9 and
/// B0-B9, then user banks 00-39.
pub fn switch_bank(i: u8) -> String {
    match i {
        0 => "Current bank".into(),
        1..=10 => format!("A{}", i - 1),
        11..=20 => format!("B{}", i - 11),
        21..=60 => format!("{:02}", i - 21),
        _ => i.to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchSetting {
    pub function: u8,
    pub p1: u8,
    pub p2: u8,
}

pub fn decode_switch(value: i32) -> SwitchSetting {
    let raw = value as u32 & 0xFF_FFFF;
    SwitchSetting {
        function: (raw & 0xFF) as u8,
        p1: ((raw >> 8) & 0xFF) as u8,
        p2: ((raw >> 16) & 0xFF) as u8,
    }
}

pub fn encode_switch(s: SwitchSetting) -> i32 {
    (s.function as i32) | ((s.p1 as i32) << 8) | ((s.p2 as i32) << 16)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchKind {
    None,
    Style,
    Preset,
    Cc,
}

pub fn switch_kind(function: u8) -> SwitchKind {
    match function {
        PRESET => SwitchKind::Preset,
        MIDI_CC => SwitchKind::Cc,
        0 | TAP_TEMPO => SwitchKind::None,
        f if f as usize >= FUNCTIONS.len() => SwitchKind::None,
        _ => SwitchKind::Style,
    }
}

/// A new function starts from neutral parameters (the unit's own defaults are not known).
pub fn with_function(s: SwitchSetting, function: u8) -> SwitchSetting {
    if function == s.function {
        s
    } else {
        SwitchSetting {
            function,
            p1: 0,
            p2: 0,
        }
    }
}

pub fn describe_switch(s: SwitchSetting) -> String {
    let name = FUNCTIONS
        .get(s.function as usize)
        .map(|n| n.to_string())
        .unwrap_or(format!("Function {}", s.function));
    match switch_kind(s.function) {
        SwitchKind::Style => format!(
            "{name} ({})",
            STYLES
                .get(s.p1 as usize)
                .map(|x| x.to_string())
                .unwrap_or(s.p1.to_string())
        ),
        SwitchKind::Preset => format!("Preset {}-{}", switch_bank(s.p1), s.p2 as u32 + 1),
        SwitchKind::Cc => format!("MIDI CC #{}", s.p1),
        SwitchKind::None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_units_assignments() {
        let d = |v: i32| describe_switch(decode_switch(v));
        assert_eq!(d(1), "Filter (Toggle)");
        assert_eq!(d(0x101), "Filter (Hold)");
        assert_eq!(d(11), "Mod (Toggle)");
        assert_eq!(d(277264), "Preset 38-5");
        assert_eq!(d(202000), "Preset 00-4");
        assert_eq!(d(65552), "Preset Current bank-2");
        assert_eq!(encode_switch(decode_switch(277264)), 277264);
    }
}
