//! Preset tempo: tap times and the delay times / modulation speeds set from
//! them by each block's Tempo note value.
//!
//! Rust port of `web/src/core/tempo.ts` from davidavalanches/gsystem-editor
//! (commit 9b50ef8), Copyright © David Avalanches, GPL-3.0.

use crate::params::{block, FieldParam, Fmt};
use crate::preset::Preset;

pub const TAP_MIN_MS: i32 = 100;
pub const TAP_MAX_MS: i32 = 3000;
/// Blocks with a tap time, Delay first.
pub const TEMPO_BLOCKS: [&str; 3] = ["delay", "filter", "mod"];

fn is_tap(name: &str) -> bool {
    name.starts_with("Tap")
}

/// Fields of the tap times under the current effect types, Delay's first.
pub fn tap_fields(p: &Preset) -> Vec<usize> {
    TEMPO_BLOCKS
        .iter()
        .flat_map(|k| {
            block(k)
                .fields(p)
                .into_iter()
                .filter(|(_, fp)| is_tap(fp.name()))
                .map(|(f, _)| f)
        })
        .collect()
}

pub fn preset_tempo_ms(p: &Preset) -> Option<i32> {
    tap_fields(p).first().map(|&f| p.param_i24(f))
}

pub fn set_tempo_ms(p: &mut Preset, ms: i32) {
    let v = ms.clamp(TAP_MIN_MS, TAP_MAX_MS);
    for f in tap_fields(p) {
        p.set_param_i24(f, v);
    }
    sync_all_to_tempo(p);
}

/// Length of a Tempo note value ("1/8D"…) in quarter notes; None for Ignore.
pub fn note_quarters(label: &str) -> Option<f64> {
    let (body, mult) = match label.chars().last()? {
        'D' => (&label[..label.len() - 1], 1.5),
        'T' => (&label[..label.len() - 1], 2.0 / 3.0),
        _ => (label, 1.0),
    };
    let (num, den) = match body.split_once('/') {
        Some((n, d)) => (n.parse::<f64>().ok()?, d.parse::<f64>().ok()?),
        None => (body.parse::<f64>().ok()?, 1.0),
    };
    Some(4.0 * num / den * mult)
}

/// Tempo parameter and the one it sets.
const SYNCED: [(&str, &str); 6] = [
    ("Tempo", "Delay"),
    ("Tempo1", "Delay1"),
    ("Tempo2", "Delay2"),
    ("Tempo", "Speed"),
    ("Tempo1", "Speed1"),
    ("Tempo2", "Speed2"),
];

/// Set a block's delay times and speeds from its tap time (else the preset
/// tempo) and its Tempo note values.
pub fn sync_to_tempo(p: &mut Preset, key: &str) {
    let fields = block(key).fields(p);
    let find = |name: &str| fields.iter().find(|(_, fp)| fp.name() == name).copied();
    let tap_ms = fields
        .iter()
        .find(|(_, fp)| is_tap(fp.name()))
        .map(|(f, _)| p.param_i24(*f))
        .or_else(|| preset_tempo_ms(p));
    let Some(tap_ms) = tap_ms.filter(|&ms| ms > 0) else {
        return;
    };
    let original = p.clone();
    for (tempo_name, target_name) in SYNCED {
        let (Some((tf, tp)), Some((field, target))) = (find(tempo_name), find(target_name)) else {
            continue;
        };
        let Some(quarters) = note_quarters(&tp.format(original.param_i24(tf))) else {
            continue;
        };
        let ms = tap_ms as f64 * quarters;
        let FieldParam::Param(param) = target else {
            continue;
        };
        let value = if target_name.starts_with("Delay") {
            ms.round() as i32
        } else if param.fmt == Fmt::Lfo {
            // Speeds on the 1/24-octave scale from 0.05 Hz.
            (24.0 * (1000.0 / ms / 0.05).log2()).round() as i32
        } else {
            continue;
        };
        p.set_param_i24(field, param.clamp(value));
    }
}

pub fn sync_all_to_tempo(p: &mut Preset) {
    for k in TEMPO_BLOCKS {
        sync_to_tempo(p, k);
    }
}

pub fn ms_to_bpm(ms: i32) -> i32 {
    (60000.0 / ms as f64).round() as i32
}
pub fn bpm_to_ms(bpm: i32) -> i32 {
    (60000.0 / bpm as f64).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::lfo;

    fn field(p: &Preset, key: &str, name: &str) -> usize {
        block(key)
            .fields(p)
            .into_iter()
            .find(|(_, fp)| fp.name() == name)
            .unwrap()
            .0
    }
    fn note(p: &Preset, key: &str, name: &str, label: &str) -> i32 {
        let (_, fp) = block(key)
            .fields(p)
            .into_iter()
            .find(|(_, fp)| fp.name() == name)
            .unwrap();
        let FieldParam::Param(param) = fp else {
            panic!()
        };
        param.choices.iter().position(|c| *c == label).unwrap() as i32 + param.choices_base
    }

    #[test]
    fn quarters() {
        assert_eq!(note_quarters("1/4"), Some(1.0));
        assert_eq!(note_quarters("1/8D"), Some(0.75));
        assert!((note_quarters("1/4T").unwrap() - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(note_quarters("1"), Some(4.0));
        assert_eq!(note_quarters("2"), Some(8.0));
        assert_eq!(note_quarters("Ignore"), None);
    }

    #[test]
    fn delay_time_from_tap_and_note() {
        let mut p = Preset::blank("");
        let tap = field(&p, "delay", "TapTime");
        p.set_param_i24(tap, 500);
        let tempo = field(&p, "delay", "Tempo");
        let v = note(&p, "delay", "Tempo", "1/8D");
        p.set_param_i24(tempo, v);
        sync_to_tempo(&mut p, "delay");
        assert_eq!(p.param_i24(field(&p, "delay", "Delay")), 375);
    }

    #[test]
    fn ignore_leaves_delay_alone() {
        let mut p = Preset::blank("");
        p.set_param_i24(field(&p, "delay", "TapTime"), 500);
        let d = field(&p, "delay", "Delay");
        p.set_param_i24(d, 123);
        sync_to_tempo(&mut p, "delay");
        assert_eq!(p.param_i24(d), 123);
    }

    #[test]
    fn mod_speed_from_tap() {
        let mut p = Preset::blank("");
        p.set_param_i24(field(&p, "mod", "Tap"), 500);
        let v = note(&p, "mod", "Tempo", "1/4");
        p.set_param_i24(field(&p, "mod", "Tempo"), v);
        sync_to_tempo(&mut p, "mod");
        assert_eq!(lfo(p.param_i24(field(&p, "mod", "Speed"))), "2.02 Hz");
    }

    #[test]
    fn follows_a_new_preset_tempo() {
        let mut p = Preset::blank("");
        let v = note(&p, "delay", "Tempo", "1/4");
        p.set_param_i24(field(&p, "delay", "Tempo"), v);
        set_tempo_ms(&mut p, 600);
        assert_eq!(p.param_i24(field(&p, "delay", "Delay")), 600);
    }
}
