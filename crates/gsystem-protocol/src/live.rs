//! Live editing: write a preset (or changes to one) into the unit's edit
//! buffer with `55 22` parameter messages, the same messages the unit sends
//! when a knob or switch is used. Nothing is stored: the user keeps a sound
//! by pressing Store on the unit, which runs the firmware's own store.
//!
//! Rust port of the Live mode in `web/src/ui/preset.ts` and the ids in
//! `web/src/core/protocol.ts` from davidavalanches/gsystem-editor (commit
//! 9b50ef8), Copyright © David Avalanches, GPL-3.0. Block ids and level
//! parameters were captured from the unit's own switches and knobs there.

use crate::levels::{self, LEVELS_FIELD, OUTPUTS_FIELD};
use crate::modifiers::{MODIFIER_BLOCK_ID, MODIFIER_FIELD, MODIFIER_PARAM_BASE, MODIFIER_SLOTS};
use crate::novadrive::{nova_word, NOVA_BLOCK_ID, NOVA_FIELD, NOVA_HEADER_FIELD};
use crate::params::{diff_fields, owner, BLOCKS};
use crate::preset::Preset;
use crate::sysex::{Message, DEVICE_ANY};

/// Pause between parameter messages (the unit keeps up at this rate).
pub const SEND_INTERVAL_MS: u64 = 2;

/// `55 22` block id of each effect block.
pub fn block_id(key: &str) -> Option<u8> {
    Some(match key {
        "eq" => 2,
        "mod" => 4,
        "filter" => 7,
        "pitch" => 8,
        "delay" => 9,
        "comp" => 10,
        "reverb" => 11,
        "gate" => 12,
        _ => return None,
    })
}

/// Block 1: routing, levels and switches.
pub const LEVELS_BLOCK_ID: u8 = 1;
pub const ROUTING_PARAM: u16 = 0;
pub const BOOST_LEVEL_PARAM: u16 = 11;
pub const OUTPUT_PARAMS: [u16; 2] = [26, 27];
pub const RELAY_PARAMS: [u16; 2] = [17, 18];

/// Block 1 parameter of each loop/insert/boost/relay switch.
pub fn level_switch_param(key: &str) -> Option<u16> {
    Some(match key {
        "loop1" => 19,
        "loop2" => 20,
        "loop3" => 21,
        "loop4" => 22,
        "insert" => 23,
        "boost" => 10,
        "relay" => 16,
        _ => return None,
    })
}

const LEVEL_KEYS: [&str; 7] = [
    "boost", "relay", "loop1", "loop2", "loop3", "loop4", "insert",
];

/// One edit-buffer write. Writes go to device 7F (the unit ignores them otherwise).
pub fn set_param(block: u8, param: u16, value: i32) -> Message {
    Message::SetParam {
        device: DEVICE_ANY,
        block,
        param,
        values: vec![value],
    }
}

fn routing_messages(p: &Preset, only: Option<usize>) -> Vec<Message> {
    let mut out = Vec::new();
    if only != Some(OUTPUTS_FIELD) {
        out.push(set_param(
            LEVELS_BLOCK_ID,
            ROUTING_PARAM,
            levels::preset_levels(p).routing as i32,
        ));
    }
    if only != Some(LEVELS_FIELD) {
        let (l, r) = levels::preset_outputs(p);
        out.push(set_param(LEVELS_BLOCK_ID, OUTPUT_PARAMS[0], l));
        out.push(set_param(LEVELS_BLOCK_ID, OUTPUT_PARAMS[1], r));
    }
    out
}

/// Audition a whole preset: every block (type first, since it decides what
/// the other parameters are), then the modifier slots, routing, output
/// levels and the NovaDrive word. Loops, insert and boost are not switched
/// (as in the original), so external gear isn't toggled by loading a sound.
pub fn full_push(p: &Preset) -> Vec<Message> {
    let mut out = Vec::new();
    for b in BLOCKS.iter() {
        let Some(id) = block_id(b.key) else { continue };
        for (f, _) in b.fields(p) {
            out.push(set_param(id, (f - b.first_field()) as u16, p.param_i24(f)));
        }
    }
    for slot in 0..MODIFIER_SLOTS {
        out.push(set_param(
            MODIFIER_BLOCK_ID,
            MODIFIER_PARAM_BASE + slot as u16,
            p.param_i24(MODIFIER_FIELD + slot),
        ));
    }
    out.extend(routing_messages(p, None));
    out.push(set_param(NOVA_BLOCK_ID, 0, nova_word(p) as i32));
    out
}

/// The messages that bring the unit's edit buffer from `before` to `after`.
pub fn changes(before: &Preset, after: &Preset) -> Vec<Message> {
    let mut out = Vec::new();
    for (index, _, value) in diff_fields(before, after) {
        if let Some(b) = owner(index) {
            if index != b.count_field {
                if let Some(id) = block_id(b.key) {
                    out.push(set_param(id, (index - b.first_field()) as u16, value));
                }
            }
        } else if (MODIFIER_FIELD..MODIFIER_FIELD + MODIFIER_SLOTS).contains(&index) {
            out.push(set_param(
                MODIFIER_BLOCK_ID,
                MODIFIER_PARAM_BASE + (index - MODIFIER_FIELD) as u16,
                value,
            ));
        } else if index == LEVELS_FIELD || index == OUTPUTS_FIELD {
            out.extend(routing_messages(after, Some(index)));
        } else if index == NOVA_HEADER_FIELD || index == NOVA_FIELD {
            out.push(set_param(NOVA_BLOCK_ID, 0, nova_word(after) as i32));
        }
        if index == LEVELS_FIELD {
            let (lb, la) = (levels::preset_levels(before), levels::preset_levels(after));
            if la.boost_level != lb.boost_level {
                out.push(set_param(
                    LEVELS_BLOCK_ID,
                    BOOST_LEVEL_PARAM,
                    la.boost_level,
                ));
            }
            // Relay settings first: the unit applies them to its jacks when the relay switch is set.
            let (rb, ra) = (levels::preset_relays(before), levels::preset_relays(after));
            let mut relays_changed = false;
            for (which, (b, a)) in [(rb.0, ra.0), (rb.1, ra.1)].into_iter().enumerate() {
                if a != b {
                    out.push(set_param(LEVELS_BLOCK_ID, RELAY_PARAMS[which], a as i32));
                    relays_changed = true;
                }
            }
            for key in LEVEL_KEYS {
                let on = levels::level_on(after, key);
                if on != levels::level_on(before, key) || (key == "relay" && relays_changed) {
                    out.push(set_param(
                        LEVELS_BLOCK_ID,
                        level_switch_param(key).unwrap(),
                        on as i32,
                    ));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::block;

    #[test]
    fn encodes_parameter_messages_like_the_unit() {
        // Reverb (block 11) parameter 12 = -25: F0 00 20 1F 7F 55 22 0B 00 0C F F F F E 7 F7
        assert_eq!(
            set_param(11, 12, -25).encode(),
            [
                0xF0, 0x00, 0x20, 0x1F, 0x7F, 0x55, 0x22, 0x0B, 0x00, 0x0C, 0x0F, 0x0F, 0x0F, 0x0F,
                0x0E, 0x07, 0xF7
            ]
        );
    }

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.split_whitespace().collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Messages captured from the unit by davidavalanches/gsystem-editor.
    #[test]
    fn matches_messages_captured_on_the_unit() {
        let delay_off = hex("f000201f7f5522090001000000000000f7");
        assert_eq!(set_param(9, 1, 0).encode(), delay_off);
        assert_eq!(
            set_param(11, 2, 39).encode(),
            hex("f000201f7f55220b0002000000000207f7")
        );
        let gate = hex("f000201f7f55220c00020f0f0f0f0e03f7");
        assert_eq!(set_param(12, 2, -29).encode(), gate);
        assert_eq!(Message::parse(&gate).unwrap(), set_param(12, 2, -29));
        // Vyzor's Sync, as captured: editor mode on, begin, dump, end — to SysEx ID 1.
        assert_eq!(
            Message::editor_mode(1, true).encode(),
            hex("f000201f015522 01001d000000000001 f7")
        );
        assert_eq!(
            Message::editor_mode(1, false).encode(),
            hex("f000201f015522 01001d000000000000 f7")
        );
        assert_eq!(
            Message::Transfer {
                device: 1,
                begin: true
            }
            .encode(),
            hex("f000201f01555700f7")
        );
        assert_eq!(
            Message::Transfer {
                device: 1,
                begin: false
            }
            .encode(),
            hex("f000201f01555701f7")
        );
        let dump = Preset::blank("Fragile").to_message(1, 136).encode();
        assert_eq!(
            &dump[..10],
            &[0xf0, 0, 0x20, 0x1f, 0x01, 0x55, 0x20, 0x01, 0x01, 0x6c]
        ); // n° 236
    }

    #[test]
    fn full_push_sends_types_first_and_everything_else() {
        let mut p = Preset::blank("");
        p.set_param_i24(block("reverb").first_field(), 3);
        let msgs = full_push(&p);
        // First reverb message is its type.
        let first_reverb = msgs
            .iter()
            .find(|m| matches!(m, Message::SetParam { block: 11, .. }))
            .unwrap();
        assert_eq!(*first_reverb, set_param(11, 0, 3));
        assert!(msgs.contains(&set_param(MODIFIER_BLOCK_ID, MODIFIER_PARAM_BASE + 37, 0)));
        assert!(msgs.contains(&set_param(NOVA_BLOCK_ID, 0, 0)));
        // No loop/insert/boost switching on a full push.
        assert!(!msgs.contains(&set_param(LEVELS_BLOCK_ID, 19, 0)));
    }

    #[test]
    fn changes_send_only_what_moved() {
        let before = Preset::blank("");
        let mut after = before.clone();
        let d = block("delay");
        after.set_param_i24(d.first_field() + 1, 1); // delay on
        levels::set_level(&mut after, "loop2", true);
        let msgs = changes(&before, &after);
        assert_eq!(msgs.len(), 3);
        assert!(msgs.contains(&set_param(9, 1, 1)));
        assert!(msgs.contains(&set_param(LEVELS_BLOCK_ID, ROUTING_PARAM, 0)));
        assert!(msgs.contains(&set_param(LEVELS_BLOCK_ID, 20, 1)));
    }
}
