//! Checks a preset before it is shared, and gives it a canonical form for
//! deduplication.
//!
//! Used by the preset library server and by the editor before uploading, so
//! both accept and reject exactly the same presets.
//!
//! - [`check`] reports **errors** (values the unit cannot hold: an unknown
//!   effect type, a stored parameter outside its range, a boost level above
//!   10) and **warnings** (oddities real presets do have, such as modifier
//!   slots with out-of-range bits left by older firmware). Only errors make a
//!   preset unshareable.
//! - [`normalize`] cleans what doesn't affect the sound: name bytes after the
//!   first NUL, and byte 20 (always 0 on the unit).
//! - [`canonical_bytes`] is what two copies of the same sound have in common:
//!   the parameter area with unused block slots reset to the padding value.
//!   The name is excluded, so a renamed copy is still the same preset.

use crate::levels::{preset_levels, BOOST_LEVEL_MAX, ROUTINGS};
use crate::modifiers::{decode_modifier, is_valid_modifier, MODIFIER_FIELD, MODIFIER_SLOTS};
use crate::novadrive::{nova_preset, NOVA_PRESETS};
use crate::params::{FieldParam, BLOCKS};
use crate::preset::{Preset, NAME_LEN, PARAMS_OFFSET, PARAM_COUNT};

/// Unused parameter slots hold -1.
pub const PADDING: i32 = -1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// Preset field (0..165), when the problem is in one field.
    pub field: Option<usize>,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub errors: Vec<Issue>,
    pub warnings: Vec<Issue>,
}

impl Report {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
    /// The errors as one line, for an upload rejection message.
    pub fn summary(&self) -> String {
        self.errors
            .iter()
            .map(|i| i.message.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

fn issue(field: Option<usize>, message: String) -> Issue {
    Issue { field, message }
}

/// Validate a preset against the firmware 4.02 parameter tables.
pub fn check(p: &Preset) -> Report {
    let mut r = Report::default();

    if p.name().trim().is_empty() {
        r.warnings
            .push(issue(None, "the preset has no name".into()));
    }

    let lv = preset_levels(p);
    if lv.routing >= ROUTINGS.len() {
        r.errors.push(issue(
            Some(0),
            format!("routing {} is not a G-System routing", lv.routing),
        ));
    }
    if lv.boost_level > BOOST_LEVEL_MAX {
        r.errors.push(issue(
            Some(0),
            format!(
                "boost level {} dB is above the maximum of {BOOST_LEVEL_MAX} dB",
                lv.boost_level
            ),
        ));
    }

    for b in BLOCKS.iter() {
        let count = p.param_i24(b.count_field);
        if count < 0 || count as usize > b.size + 1 {
            r.errors.push(issue(
                Some(b.count_field),
                format!("{}: parameter count {count} doesn't fit the block", b.label),
            ));
            continue;
        }
        if b.has_type() && b.effect_type(p).is_none() {
            r.errors.push(issue(
                Some(b.first_field()),
                format!(
                    "{}: unknown effect type {}",
                    b.label,
                    p.param_i24(b.first_field())
                ),
            ));
            continue;
        }
        for (field, fp) in b.stored_fields(p) {
            let v = p.param_i24(field);
            let (lo, hi) = fp.range();
            if v < lo || v > hi {
                let what = match fp {
                    FieldParam::Type(_) => "type".to_string(),
                    FieldParam::Param(param) => param.name.to_string(),
                };
                r.errors.push(issue(
                    Some(field),
                    format!("{} {what} = {v} is outside {lo}..{hi}", b.label),
                ));
            }
        }
    }

    for slot in 0..MODIFIER_SLOTS {
        let field = MODIFIER_FIELD + slot;
        let m = decode_modifier(p.param_i24(field));
        if !is_valid_modifier(m) {
            r.warnings.push(issue(
                Some(field),
                format!("modifier slot {} holds values the unit ignores", slot + 1),
            ));
        }
    }

    let n = nova_preset(p);
    if !(0..=NOVA_PRESETS).contains(&n) {
        r.warnings
            .push(issue(None, format!("NovaDrive preset {n} is out of range")));
    }
    r
}

/// Clear what doesn't change the sound: bytes after the name's NUL, and byte
/// 20. The unit's bit-7 flags inside the name are kept.
pub fn normalize(p: &Preset) -> Preset {
    let mut out = p.clone();
    if let Some(nul) = out.raw[..NAME_LEN].iter().position(|&b| b == 0) {
        out.raw[nul..NAME_LEN].fill(0);
    }
    out.raw[NAME_LEN] = 0;
    out
}

/// The parameter area with every block's unused slots set to padding, so that
/// leftovers from a previous effect type don't make two identical sounds look
/// different. The name is not included.
pub fn canonical_bytes(p: &Preset) -> Vec<u8> {
    let mut c = p.clone();
    for b in BLOCKS.iter() {
        let used: Vec<usize> = b.stored_fields(p).into_iter().map(|(f, _)| f).collect();
        for f in b.first_field()..b.first_field() + b.size {
            if f < PARAM_COUNT && !used.contains(&f) {
                c.set_param_i24(f, PADDING);
            }
        }
    }
    c.raw[PARAMS_OFFSET..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Preset {
        // "Scotty's Blues", the first factory preset.
        let bank =
            crate::vyzor::load_bank(include_bytes!("../../../testdata/Factory_Set.SQS")).unwrap();
        bank.get(0).unwrap().clone()
    }

    #[test]
    fn empty_preset_is_valid() {
        let r = check(&crate::empty::empty_preset(&sample()));
        assert!(r.is_ok(), "{:?}", r.errors);
    }

    #[test]
    fn out_of_range_type_is_an_error() {
        let mut p = sample();
        let reverb = crate::params::block("reverb");
        p.set_param_i24(reverb.first_field(), 99);
        let r = check(&p);
        assert!(!r.is_ok());
        assert!(
            r.summary().contains("unknown effect type 99"),
            "{}",
            r.summary()
        );
    }

    #[test]
    fn out_of_range_param_is_an_error() {
        let mut p = sample();
        let delay = crate::params::block("delay");
        let (field, fp) = delay.stored_fields(&p)[1];
        let (_, hi) = fp.range();
        p.set_param_i24(field, hi + 1);
        let r = check(&p);
        assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
        assert_eq!(r.errors[0].field, Some(field));
    }

    #[test]
    fn normalize_clears_after_nul_and_keeps_the_name() {
        let mut p = sample();
        p.set_name("Clean");
        p.raw[10] = b'X'; // junk after the NUL at index 5
        p.raw[NAME_LEN] = 7;
        let n = normalize(&p);
        assert_eq!(n.name(), "Clean");
        assert!(n.raw[5..=NAME_LEN].iter().all(|&b| b == 0));
        assert_eq!(n.raw[NAME_LEN + 1..], p.raw[NAME_LEN + 1..]);
    }

    #[test]
    fn canonical_ignores_name_and_unused_slots() {
        let a = sample();
        let mut b = a.clone();
        b.set_name("Another name");
        assert_eq!(canonical_bytes(&a), canonical_bytes(&b));

        // A leftover value past the stored parameters doesn't matter…
        let delay = crate::params::block("delay");
        let last = delay.first_field() + delay.size - 1;
        if !delay.stored_fields(&a).iter().any(|(f, _)| *f == last) {
            b.set_param_i24(last, 42);
            assert_eq!(canonical_bytes(&a), canonical_bytes(&b));
        }
        // …but a real parameter change does.
        let (f, _) = delay.stored_fields(&a)[1];
        b.set_param_i24(f, a.param_i24(f) + 1);
        assert_ne!(canonical_bytes(&a), canonical_bytes(&b));
    }
}
