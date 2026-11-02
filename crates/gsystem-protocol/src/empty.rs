//! An "empty" preset: the G-System has no command to empty a user slot and
//! only takes complete presets, so clearing writes a neutral one instead.
//!
//! Rust port of `web/src/core/empty.ts` from davidavalanches/gsystem-editor
//! (commit 9b50ef8), Copyright © David Avalanches, GPL-3.0.
//!
//! Built from a valid preset (same layout, accepted by the unit) with every
//! effect, loop, insert and boost off, no modifiers, no NovaDrive preset, and
//! the name "Empty". Relay settings are kept.

use crate::levels::set_level;
use crate::modifiers::{write_modifier, MODIFIER_SLOTS, UNASSIGNED};
use crate::novadrive::write_nova_preset;
use crate::params::BLOCKS;
use crate::preset::Preset;

pub const EMPTY_NAME: &str = "Empty";

pub fn empty_preset(template: &Preset) -> Preset {
    let mut p = template.clone();
    for b in BLOCKS.iter() {
        if let Some(f) = b.enable_field(&p) {
            p.set_param_i24(f, 0);
        }
    }
    for key in ["loop1", "loop2", "loop3", "loop4", "insert", "boost"] {
        set_level(&mut p, key, false);
    }
    for m in 0..MODIFIER_SLOTS {
        write_modifier(&mut p, m, UNASSIGNED);
    }
    write_nova_preset(&mut p, 0);
    p.set_name(EMPTY_NAME);
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levels::{level_on, preset_relays, LEVELS_FIELD};
    use crate::modifiers::{read_modifier, Modifier};
    use crate::novadrive::nova_preset;

    #[test]
    fn turns_everything_off_and_keeps_the_relays() {
        let mut p = Preset::blank("Solo");
        p.set_param_i24(LEVELS_FIELD, ((0xC63F0Cu32 << 8) as i32) >> 8); // loops 1-4, insert, boost on
        for b in BLOCKS.iter() {
            if let Some(f) = b.enable_field(&p) {
                p.set_param_i24(f, 1);
            }
        }
        write_modifier(
            &mut p,
            0,
            Modifier {
                source: 2,
                min: 0,
                mid: 50,
                max: 100,
            },
        );
        write_nova_preset(&mut p, 7);
        let e = empty_preset(&p);
        assert_eq!(e.name(), "Empty");
        for key in ["loop1", "loop2", "loop3", "loop4", "insert", "boost"] {
            assert!(!level_on(&e, key), "{key}");
        }
        assert_eq!(preset_relays(&e), preset_relays(&p));
        assert!(BLOCKS.iter().all(|b| !b.is_on(&e)));
        assert_eq!(read_modifier(&e, 0).source, 0);
        assert_eq!(nova_preset(&e), 0);
    }
}
