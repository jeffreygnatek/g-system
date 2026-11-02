//! Authoring API: write presets by name instead of by field number.
//!
//! - [`catalog`] describes every block, effect type and parameter (range,
//!   unit, scale, value lists) compactly enough to hand to an AI agent.
//! - [`PresetSpec`] is a partial preset in readable terms: block and type
//!   names, parameter names or labels, and values given as raw numbers,
//!   value-list labels ("1/8D", "Hall") or display strings ("2.5 kHz",
//!   "350 ms", "-6 dB", "35 %"). [`apply_spec`] applies one and reports what
//!   was actually set; [`new_preset`] starts from an empty preset.
//! - [`describe`] summarizes a preset: the chain, and the parameters of the
//!   blocks that are on, as display values.
//!
//! Ours; built on the tables ported from davidavalanches/gsystem-editor in
//! `params.rs`, `levels.rs`, `tempo.rs`, `modifiers.rs` and `novadrive.rs`.
//! Used by `gsys mcp`. Requires the `serde` feature.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::levels::{self, BOOST_LEVEL_MAX, OUTPUT_MIN, ROUTINGS};
use crate::modifiers::{self, Modifier, SOURCES};
use crate::novadrive::{self, NOVA_PRESETS};
use crate::params::{self, Block, FieldParam, Fmt, BLOCKS};
use crate::preset::{Preset, NAME_LEN};
use crate::tempo::{self, TAP_MAX_MS, TAP_MIN_MS, TEMPO_BLOCKS};

// ---------------------------------------------------------------- catalog

/// Everything needed to write a preset without guessing.
#[derive(Debug, Clone, Serialize)]
pub struct Catalog {
    /// How to give values in a [`PresetSpec`].
    pub values: &'static str,
    pub blocks: Vec<BlockCatalog>,
    pub routings: Vec<&'static str>,
    /// Boost level range in dB.
    pub boost_db: [i32; 2],
    pub loops: &'static str,
    pub insert: &'static str,
    pub relays: &'static str,
    /// Left/right output level range in dB.
    pub outputs_db: [i32; 2],
    pub modifier_sources: Vec<&'static str>,
    pub modifiers: &'static str,
    /// NovaDrive NDR-1 preset recalled with the preset; 0 = none.
    pub novadrive: [i32; 2],
    pub tempo_bpm: [i32; 2],
    pub tempo: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockCatalog {
    pub key: &'static str,
    pub label: &'static str,
    /// The parameter that switches the block on (set with `on` in a spec).
    pub on_param: &'static str,
    /// Effect types, grouped by shared parameter list. Empty for blocks
    /// without a type (gate, EQ): see `params`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<TypeGroup>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<ParamCatalog>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TypeGroup {
    pub types: Vec<&'static str>,
    pub params: Vec<ParamCatalog>,
    /// Per type: the parameters a modifier (pedal / encoder) can control.
    /// Only in the catalog of a single block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modifiable: Option<BTreeMap<&'static str, Vec<String>>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParamCatalog {
    /// Firmware name, unique within the type ("Delay#2" for a repeated name).
    pub name: String,
    /// Readable label, when it differs from the name. Accepted as a name too.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub lo: i32,
    pub hi: i32,
    /// "dB", "ms", "%", "cents", "dB/s", "number" (unitless), "choice",
    /// "freq index", "lfo index", "octave index" or "0.1 s steps"-style.
    pub unit: String,
    /// Raw value = display value for plain units; for index and scaled
    /// units, sample points "raw=display".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<String>,
    /// Labels for lo..=hi.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<String>>,
}

const VALUES_HELP: &str = "A value is a JSON number (raw units; clamped to lo..hi), true/false for On/Off switches, a choice label (\"1/8D\", \"Hall\"), or a display string like \"2.5 kHz\", \"350 ms\", \"1.8 s\", \"-6 dB\", \"35 %\" (nearest available value; a string without a unit is read in the parameter's display unit). For units 'freq index', 'lfo index', 'octave index' and scaled steps the raw number is an index, so give display strings. Parameter names match the name or label, case-insensitive.";

fn unit_of(fp: &FieldParam) -> String {
    match fp {
        FieldParam::Type(_) => "choice".into(),
        FieldParam::Param(p) if !p.choices.is_empty() => "choice".into(),
        FieldParam::Param(p) => match p.fmt {
            Fmt::Plain => "number".into(),
            Fmt::Unit(s) => s.into(),
            Fmt::Scaled(f, s) => format!("{f} {s} steps"),
            Fmt::Freq => "freq index".into(),
            Fmt::Lfo => "lfo index".into(),
            Fmt::OctWidth => "octave index".into(),
        },
    }
}

fn scale_of(fp: &FieldParam) -> Option<String> {
    let FieldParam::Param(p) = fp else {
        return None;
    };
    if !p.choices.is_empty() || matches!(p.fmt, Fmt::Plain | Fmt::Unit(_)) {
        return None;
    }
    let (lo, hi) = (p.lo, p.hi);
    let mut points: Vec<i32> = (0..=4).map(|i| lo + (hi - lo) * i / 4).collect();
    points.dedup();
    Some(
        points
            .iter()
            .map(|&v| format!("{v}={}", p.format(v)))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// Names unique within a parameter list: a repeated name gets "#2", "#3"…
fn unique_names(names: &[&'static str]) -> Vec<String> {
    names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let before = names[..i].iter().filter(|m| *m == n).count();
            if before == 0 {
                n.to_string()
            } else {
                format!("{n}#{}", before + 1)
            }
        })
        .collect()
}

fn label_of(unique: &str) -> String {
    match unique.split_once('#') {
        Some((n, k)) => format!("{}#{k}", params::label(n)),
        None => params::label(unique),
    }
}

fn param_catalog(list: &[FieldParam]) -> Vec<ParamCatalog> {
    let names: Vec<&'static str> = list.iter().map(|fp| fp.name()).collect();
    unique_names(&names)
        .into_iter()
        .zip(list)
        .map(|(name, fp)| {
            let (lo, hi) = fp.range();
            let label = label_of(&name);
            ParamCatalog {
                label: (label != name).then_some(label),
                name,
                lo,
                hi,
                unit: unit_of(fp),
                scale: scale_of(fp),
                choices: fp.choices(),
            }
        })
        .collect()
}

fn modifiable(b: &'static Block, type_index: usize) -> Vec<String> {
    let mut p = Preset::blank("");
    p.set_param_i24(b.first_field(), type_index as i32);
    modifiers::modifier_targets(&p, false)
        .into_iter()
        .filter(|t| t.key == b.key)
        .map(|t| t.name)
        .collect()
}

fn block_catalog(b: &'static Block, detailed: bool) -> BlockCatalog {
    let mut groups: Vec<TypeGroup> = Vec::new();
    let mut seen: Vec<*const [params::Param]> = Vec::new();
    for t in b.types {
        let key = t.params as *const [params::Param];
        if let Some(i) = seen.iter().position(|&s| std::ptr::eq(s, key)) {
            groups[i].types.push(t.name);
            continue;
        }
        seen.push(key);
        let list: Vec<FieldParam> = t.params.iter().map(FieldParam::Param).collect();
        groups.push(TypeGroup {
            types: vec![t.name],
            params: param_catalog(&list),
            modifiable: None,
        });
    }
    if detailed {
        for g in groups.iter_mut() {
            let m: BTreeMap<&'static str, Vec<String>> = g
                .types
                .iter()
                .map(|name| {
                    let i = b.types.iter().position(|t| t.name == *name).unwrap();
                    (*name, modifiable(b, i))
                })
                .collect();
            if m.values().any(|v| !v.is_empty()) {
                g.modifiable = Some(m);
            }
        }
    }
    let params = if b.has_type() {
        Vec::new()
    } else {
        param_catalog(&b.params.iter().map(FieldParam::Param).collect::<Vec<_>>())
    };
    BlockCatalog {
        key: b.key,
        label: b.label,
        on_param: b.enable_name,
        types: groups,
        params,
    }
}

/// The whole catalog. Compact: types sharing a parameter list are grouped.
pub fn catalog() -> Catalog {
    Catalog {
        values: VALUES_HELP,
        blocks: BLOCKS.iter().map(|b| block_catalog(b, false)).collect(),
        routings: ROUTINGS.to_vec(),
        boost_db: [0, BOOST_LEVEL_MAX],
        loops: "loops: [bool; 4] for effect loops 1-4",
        insert: "insert: bool (insert loop)",
        relays: "relays: {a, b, active}: settings A and B as 4 letters, O (open) or C (closed), contact 1 first, e.g. \"OCCO\"; active \"A\" or \"B\" picks which drives the relay jacks",
        outputs_db: [OUTPUT_MIN, 0],
        modifier_sources: SOURCES.to_vec(),
        modifiers: "modifiers: [{block, param, source, min, mid, max}]: min/mid/max are 0-100 % of the parameter's range. Only some parameters of each type can be controlled; get_catalog with a block lists them. Block \"novadrive\" has OD Gain, OD Level, DS Gain, DS Level.",
        novadrive: [0, NOVA_PRESETS],
        tempo_bpm: [tempo::ms_to_bpm(TAP_MAX_MS), tempo::ms_to_bpm(TAP_MIN_MS)],
        tempo: "tempo_bpm sets every tap time; delay times and mod/filter speeds whose Tempo is a note value (\"1/4\", \"1/8D\"…) follow it. Tempo \"Ignore\" keeps the time you set.",
    }
}

/// One block's catalog, including which parameters modifiers can control per type.
pub fn catalog_block(key: &str) -> Result<BlockCatalog, String> {
    let b = find_block(key)?;
    Ok(block_catalog(b, true))
}

// ---------------------------------------------------------------- spec

/// A value in a spec: raw number, switch, choice label or display string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SpecValue {
    Bool(bool),
    Number(f64),
    Text(String),
}

impl From<&str> for SpecValue {
    fn from(s: &str) -> Self {
        SpecValue::Text(s.to_string())
    }
}
impl From<f64> for SpecValue {
    fn from(v: f64) -> Self {
        SpecValue::Number(v)
    }
}
impl From<bool> for SpecValue {
    fn from(v: bool) -> Self {
        SpecValue::Bool(v)
    }
}

/// A partial preset. Every field is optional; what is left out is unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// "Serial", "Parallel", "Semi Parallel" or "Serial2".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tempo_bpm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boost: Option<BoostSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loops: Option<[bool; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relays: Option<RelaysSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outputs: Option<OutputsSpec>,
    /// NovaDrive NDR-1 preset 1-18, 0 = none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub novadrive: Option<i32>,
    /// Block key ("delay") or label ("Noise gate") → settings.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub blocks: BTreeMap<String, BlockSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<ModifierSpec>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoostSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level_db: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelaysSpec {
    /// "OCCO" (contact 1 first) or 0-15.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<SpecValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<SpecValue>,
    /// "A" or "B".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputsSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_db: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_db: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockSpec {
    /// Effect type name ("Hall", "Tape Delay").
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    /// Parameter name or label → value.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, SpecValue>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModifierSpec {
    /// Block key, or "novadrive".
    pub block: String,
    pub param: String,
    /// "None", "Vol", "Exp", "Enc A", "Enc B", "Enc C".
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mid: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<i32>,
}

/// One thing a spec set, with the value actually stored.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Applied {
    /// "name", "routing", "tempo", "delay.Delay", "modifier delay.Mix"…
    pub target: String,
    pub value: String,
    /// Clamping, rounding to the nearest step, tempo sync…
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Everything wrong with a spec; nothing was changed.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecError {
    pub errors: Vec<String>,
}

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.errors.join("\n"))
    }
}
impl std::error::Error for SpecError {}

/// Lowercase, without spaces, underscores, dashes or dots.
fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-' | '.'))
        .flat_map(char::to_lowercase)
        .collect()
}

fn find_block(name: &str) -> Result<&'static Block, String> {
    let n = norm(name);
    BLOCKS
        .iter()
        .find(|b| norm(b.key) == n || norm(b.label) == n)
        .ok_or_else(|| {
            format!(
                "unknown block {name:?}; blocks are {}",
                BLOCKS.iter().map(|b| b.key).collect::<Vec<_>>().join(", ")
            )
        })
}

/// A block's parameters under its current type, with their unique names.
fn named_fields(b: &'static Block, p: &Preset) -> Vec<(usize, FieldParam, String)> {
    let fields: Vec<(usize, FieldParam)> = b
        .fields(p)
        .into_iter()
        .filter(|(_, fp)| !matches!(fp, FieldParam::Type(_)))
        .collect();
    let names: Vec<&'static str> = fields.iter().map(|(_, fp)| fp.name()).collect();
    fields
        .into_iter()
        .zip(unique_names(&names))
        .map(|((f, fp), n)| (f, fp, n))
        .collect()
}

fn type_desc(b: &'static Block, p: &Preset) -> String {
    match b.effect_type(p) {
        Some(t) => format!("{} ({})", b.key, t.name),
        None => b.key.to_string(),
    }
}

fn find_param(
    b: &'static Block,
    p: &Preset,
    query: &str,
) -> Result<(usize, FieldParam, String), String> {
    let q = norm(query);
    let fields = named_fields(b, p);
    if let Some(hit) = fields
        .iter()
        .find(|(_, _, n)| norm(n) == q || norm(&label_of(n)) == q)
    {
        return Ok(hit.clone());
    }
    if q == "type" && b.has_type() {
        return Err(format!(
            "{}: set the effect type with \"type\", not as a parameter",
            b.key
        ));
    }
    let valid: Vec<String> = fields
        .iter()
        .map(|(_, _, n)| {
            let l = label_of(n);
            if l == *n {
                n.clone()
            } else {
                format!("{n} ({l})")
            }
        })
        .collect();
    Err(format!(
        "{} has no parameter {query:?}. Valid: {}",
        type_desc(b, p),
        valid.join(", ")
    ))
}

/// A display string as (number, normalized unit), with s → ms and kHz → Hz.
fn parse_display(s: &str) -> Option<(f64, String)> {
    let s = s.trim();
    let end = s
        .char_indices()
        .find(|(i, c)| !(c.is_ascii_digit() || *c == '.' || (*i == 0 && (*c == '-' || *c == '+'))))
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    let num: f64 = s[..end].parse().ok()?;
    let unit: String = s[end..]
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let (factor, unit) = match unit.as_str() {
        "khz" => (1000.0, "hz"),
        "s" | "sec" | "secs" | "seconds" | "second" => (1000.0, "ms"),
        "msec" | "milliseconds" => (1.0, "ms"),
        "cent" | "c" => (1.0, "cents"),
        "octave" | "octaves" | "octs" => (1.0, "oct"),
        "percent" | "pct" => (1.0, "%"),
        "db" | "decibel" | "decibels" => (1.0, "db"),
        other => (1.0, other),
    };
    Some((num * factor, unit.to_string()))
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v}")
    }
}

fn is_index_unit(fp: &FieldParam) -> bool {
    matches!(
        fp,
        FieldParam::Param(p) if p.choices.is_empty()
            && matches!(p.fmt, Fmt::Freq | Fmt::Lfo | Fmt::OctWidth | Fmt::Scaled(..))
    )
}

/// Resolve a spec value to a raw value for `fp`, with a note when it was
/// clamped, rounded or matched to the nearest step.
pub fn resolve_value(fp: &FieldParam, v: &SpecValue) -> Result<(i32, Option<String>), String> {
    let (lo, hi) = fp.range();
    let choices = fp.choices();
    let name = fp.name();
    match v {
        SpecValue::Bool(on) => {
            let ok = choices
                .as_ref()
                .is_some_and(|c| c.len() == 2 && c[0] == "Off" && c[1] == "On");
            if ok {
                Ok((*on as i32, None))
            } else {
                Err(format!("{name} is not an On/Off switch; give a value"))
            }
        }
        SpecValue::Number(n) => {
            if !n.is_finite() {
                return Err(format!("{name}: {n} is not a number"));
            }
            let r = n.round();
            let mut notes = Vec::new();
            if r != *n {
                notes.push(format!("rounded {} to {r}", fmt_num(*n)));
            }
            let raw = r.clamp(i32::MIN as f64, i32::MAX as f64) as i32;
            let c = fp.clamp(raw);
            if c != raw {
                notes.push(format!("{raw} is outside {lo}..{hi}, clamped"));
            }
            if is_index_unit(fp) && (r != *n || c != raw) {
                notes.push(format!(
                    "a number is the raw {} for {name}; give a display string like {:?}",
                    unit_of(fp),
                    fp.format((lo + hi) / 2)
                ));
            }
            Ok((c, (!notes.is_empty()).then(|| notes.join("; "))))
        }
        SpecValue::Text(s) => {
            // Labels hold digits and dots ("1.0 ms", "1/8D"): only case and
            // spaces are ignored here.
            let loose = |x: &str| -> String {
                x.chars()
                    .filter(|c| !c.is_whitespace())
                    .flat_map(char::to_lowercase)
                    .collect()
            };
            let q = loose(s);
            if let Some(c) = &choices {
                if let Some(i) = c.iter().position(|l| loose(l) == q) {
                    return Ok((lo + i as i32, None));
                }
                // On/Off switches also take true/false, yes/no.
                if c.len() == 2 && c[0] == "Off" && c[1] == "On" {
                    match q.as_str() {
                        "true" | "yes" | "1" => return Ok((1, None)),
                        "false" | "no" | "0" => return Ok((0, None)),
                        _ => {}
                    }
                }
            }
            let bad = || {
                let hint = match &choices {
                    Some(c) => format!("one of: {}", c.join(", ")),
                    None => format!(
                        "a display value between {:?} and {:?}",
                        fp.format(lo),
                        fp.format(hi)
                    ),
                };
                format!("{name}: cannot read {s:?}; expected {hint}")
            };
            let Some((want, unit)) = parse_display(s) else {
                return Err(bad());
            };
            let mut best: Option<(i32, f64)> = None;
            let mut units = Vec::new();
            for raw in lo..=hi {
                let Some((got, u)) = parse_display(&fp.format(raw)) else {
                    continue;
                };
                if !unit.is_empty() && u != unit {
                    if !units.contains(&u) {
                        units.push(u);
                    }
                    continue;
                }
                let d = (got - want).abs();
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((raw, d));
                }
            }
            let Some((raw, _)) = best else {
                return Err(if units.is_empty() {
                    bad()
                } else {
                    let hint = if s.contains('/') {
                        " Note values (\"1/4\", \"1/8D\") go in the block's Tempo parameter, \
                         which then sets this time from the preset tempo."
                    } else {
                        ""
                    };
                    format!(
                        "{name} is shown in {}, not {unit:?}; e.g. {:?}.{hint}",
                        units.join("/"),
                        fp.format((lo + hi) / 2)
                    )
                });
            };
            let shown = fp.format(raw);
            let exact = parse_display(&shown)
                .map(|(g, _)| (g - want).abs() <= 1e-9 * want.abs().max(1.0))
                .unwrap_or(false);
            Ok((
                raw,
                (!exact).then(|| format!("requested {s:?}; nearest available is {shown}")),
            ))
        }
    }
}

fn find_type(b: &'static Block, name: &str) -> Result<usize, String> {
    let n = norm(name);
    b.types
        .iter()
        .position(|t| norm(t.name) == n)
        .ok_or_else(|| {
            if b.has_type() {
                format!(
                    "{} has no type {name:?}; types: {}",
                    b.key,
                    b.types
                        .iter()
                        .map(|t| t.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                format!("{} has no effect types", b.key)
            }
        })
}

/// Store the full parameter count for the block's current type (older
/// presets store fewer) and pull every value into range, so nothing set
/// from a spec hides behind a short count.
fn complete_block(p: &mut Preset, b: &'static Block) {
    let fields = b.fields(p);
    let stored = p.param_i24(b.count_field).max(0) as usize;
    p.set_param_i24(b.count_field, fields.len() as i32);
    for (i, (f, fp)) in fields.into_iter().enumerate() {
        let mut v = p.param_i24(f);
        // Padding past the old count is -1; start such a field at 0 instead
        // ("-1 %" feedback is padding, not a choice).
        if i >= stored && v == crate::validate::PADDING {
            v = 0;
        }
        p.set_param_i24(f, fp.clamp(v));
    }
}

fn relay_value(v: &SpecValue) -> Result<u8, String> {
    match v {
        SpecValue::Number(n) if (0.0..=15.0).contains(n) && n.fract() == 0.0 => Ok(*n as u8),
        SpecValue::Text(s)
            if s.len() == levels::RELAY_CONTACTS
                && s.chars().all(|c| matches!(c, 'O' | 'o' | 'C' | 'c')) =>
        {
            Ok(s.chars()
                .enumerate()
                .filter(|(_, c)| c.eq_ignore_ascii_case(&'c'))
                .fold(0u8, |acc, (i, _)| acc | (1 << i)))
        }
        _ => Err(format!(
            "relay setting {v:?}: give 4 letters O/C, contact 1 first (\"OCCO\"), or 0-15"
        )),
    }
}

fn source_index(name: &str) -> Option<u8> {
    let n = norm(name);
    let i = match n.as_str() {
        "none" | "off" => 0,
        "vol" | "volume" | "volumepedal" => 1,
        "exp" | "expression" | "expressionpedal" | "pedal" => 2,
        "enca" | "encodera" => 3,
        "encb" | "encoderb" => 4,
        "encc" | "encoderc" => 5,
        _ => return None,
    };
    Some(i)
}

/// Tempo note parameter that drives each synced parameter (see tempo.rs).
const SYNCED: [(&str, &str); 6] = [
    ("Delay", "Tempo"),
    ("Delay1", "Tempo1"),
    ("Delay2", "Tempo2"),
    ("Speed", "Tempo"),
    ("Speed1", "Tempo1"),
    ("Speed2", "Tempo2"),
];

/// Apply a spec. All or nothing: on error the preset is unchanged and every
/// problem is listed.
///
/// Order: name and levels, then per block the type ([`params::set_type`],
/// which clamps the old values), `on` and parameters, then modifiers, then
/// tempo. Delay times and speeds then follow the tempo as on the unit
/// ([`tempo::sync_to_tempo`]). Setting a delay time or speed explicitly while
/// its Tempo is a note value switches that Tempo to Ignore, so the value
/// holds (unless the spec sets the Tempo too).
pub fn apply_spec(p: &mut Preset, spec: &PresetSpec) -> Result<Vec<Applied>, SpecError> {
    let mut q = p.clone();
    let mut errors = Vec::new();
    let mut out = Vec::new();
    let mut push = |target: &str, value: String, note: Option<String>| {
        out.push(Applied {
            target: target.to_string(),
            value,
            note,
        })
    };

    if let Some(name) = &spec.name {
        q.set_name(name);
        let mut notes = Vec::new();
        if name.chars().count() > NAME_LEN {
            notes.push(format!("names hold {NAME_LEN} characters; truncated"));
        }
        if name.chars().any(|c| (c as u32) > 255) {
            notes.push("characters outside Latin-1 became '?'".to_string());
        }
        push(
            "name",
            q.name(),
            (!notes.is_empty()).then(|| notes.join("; ")),
        );
    }
    if let Some(r) = &spec.routing {
        match ROUTINGS.iter().position(|x| norm(x) == norm(r)) {
            Some(i) => {
                levels::set_routing(&mut q, i);
                push("routing", ROUTINGS[i].into(), None);
            }
            None => errors.push(format!(
                "unknown routing {r:?}; routings: {}",
                ROUTINGS.join(", ")
            )),
        }
    }
    if let Some(b) = &spec.boost {
        if let Some(on) = b.on {
            levels::set_level(&mut q, "boost", on);
            push("boost", on_off(on).into(), None);
        }
        if let Some(db) = b.level_db {
            levels::set_boost_level(&mut q, db);
            let got = levels::preset_levels(&q).boost_level;
            push(
                "boost level",
                format!("{got} dB"),
                (got != db).then(|| format!("{db} dB is outside 0..{BOOST_LEVEL_MAX} dB, clamped")),
            );
        }
    }
    if let Some(loops) = spec.loops {
        for (i, on) in loops.iter().enumerate() {
            levels::set_level(&mut q, &format!("loop{}", i + 1), *on);
        }
        push("loops", loops_text(&levels::preset_levels(&q).loops), None);
    }
    if let Some(on) = spec.insert {
        levels::set_level(&mut q, "insert", on);
        push("insert", on_off(on).into(), None);
    }
    if let Some(r) = &spec.relays {
        for (which, v) in [&r.a, &r.b].into_iter().enumerate() {
            if let Some(v) = v {
                match relay_value(v) {
                    Ok(bits) => {
                        levels::set_relay(&mut q, which, bits);
                        push(
                            if which == 0 { "relay A" } else { "relay B" },
                            levels::relay_text(bits),
                            None,
                        );
                    }
                    Err(e) => errors.push(e),
                }
            }
        }
        if let Some(a) = &r.active {
            match norm(a).as_str() {
                "a" => {
                    levels::set_level(&mut q, "relay", true);
                    push("relay active", "A".into(), None);
                }
                "b" => {
                    levels::set_level(&mut q, "relay", false);
                    push("relay active", "B".into(), None);
                }
                _ => errors.push(format!("relays.active {a:?}: give \"A\" or \"B\"")),
            }
        }
    }
    if let Some(o) = &spec.outputs {
        let (l, r) = levels::preset_outputs(&q);
        let cl = |v: i32| v.clamp(OUTPUT_MIN, 0);
        let (nl, nr) = (o.left_db.map_or(l, cl), o.right_db.map_or(r, cl));
        levels::set_outputs(&mut q, nl, nr);
        let clamped = o.left_db.is_some_and(|v| v != nl) || o.right_db.is_some_and(|v| v != nr);
        push(
            "outputs",
            format!("L {nl} dB, R {nr} dB"),
            clamped.then(|| format!("outputs are {OUTPUT_MIN}..0 dB, clamped")),
        );
    }
    if let Some(n) = spec.novadrive {
        novadrive::write_nova_preset(&mut q, n);
        let got = novadrive::nova_preset(&q);
        push(
            "novadrive",
            if got == 0 {
                "none".into()
            } else {
                got.to_string()
            },
            (got != n).then(|| format!("{n} is outside 0..{NOVA_PRESETS}, clamped")),
        );
    }

    // Blocks, in storage order; remember what was set to report final values.
    let mut set_fields: Vec<(usize, i32, String, Option<String>)> = Vec::new();
    let mut touched: Vec<&'static Block> = Vec::new();
    let mut resolved: Vec<(&'static Block, &BlockSpec)> = Vec::new();
    for (key, bs) in &spec.blocks {
        match find_block(key) {
            Ok(b) if resolved.iter().any(|(x, _)| x.key == b.key) => {
                errors.push(format!("block {} is given twice", b.key))
            }
            Ok(b) => resolved.push((b, bs)),
            Err(e) => errors.push(e),
        }
    }
    resolved.sort_by_key(|(b, _)| b.count_field);
    for (b, bs) in resolved {
        touched.push(b);
        if let Some(t) = &bs.type_name {
            match find_type(b, t) {
                Ok(i) => {
                    params::set_type(&mut q, b.key, i);
                    push(&format!("{}.type", b.key), b.types[i].name.into(), None);
                }
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            }
        }
        complete_block(&mut q, b);
        if let Some(on) = bs.on {
            if let Some(f) = b.enable_field(&q) {
                q.set_param_i24(f, on as i32);
                push(&format!("{}.on", b.key), on_off(on).into(), None);
            }
        }
        let mut given: Vec<String> = Vec::new();
        let mut explicit: Vec<(usize, String)> = Vec::new();
        for (name, value) in &bs.params {
            let (field, fp, uname) = match find_param(b, &q, name) {
                Ok(x) => x,
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };
            match resolve_value(&fp, value) {
                Ok((raw, note)) => {
                    q.set_param_i24(field, raw);
                    given.push(uname.clone());
                    explicit.push((field, uname.clone()));
                    set_fields.retain(|(f, ..)| *f != field);
                    set_fields.push((field, raw, format!("{}.{}", b.key, uname), note));
                }
                Err(e) => errors.push(format!("{}.{e}", b.key)),
            }
        }
        // An explicit time or speed wins over a note value set earlier.
        for (field, uname) in explicit {
            let Some((_, tempo_name)) = SYNCED.iter().find(|(t, _)| *t == uname) else {
                continue;
            };
            if given.iter().any(|g| g == tempo_name) {
                continue;
            }
            let Some((tf, tfp, _)) = named_fields(b, &q)
                .into_iter()
                .find(|(_, _, n)| n == tempo_name)
            else {
                continue;
            };
            let current = tfp.format(q.param_i24(tf));
            if tempo::note_quarters(&current).is_some() {
                let (ignore, _) = resolve_value(&tfp, &"Ignore".into()).expect("Ignore");
                q.set_param_i24(tf, ignore);
                if let Some(entry) = set_fields.iter_mut().find(|(f, ..)| *f == field) {
                    let n = format!("{tempo_name} was {current}, set to Ignore so this time holds");
                    entry.3 = Some(match entry.3.take() {
                        Some(old) => format!("{old}; {n}"),
                        None => n,
                    });
                }
            }
        }
    }

    for m in &spec.modifiers {
        match apply_modifier(&mut q, m) {
            Ok(a) => out.push(a),
            Err(e) => errors.push(e),
        }
    }

    if let Some(bpm) = spec.tempo_bpm {
        if !(bpm.is_finite() && bpm > 0.0) {
            errors.push(format!("tempo_bpm {bpm} is not a tempo"));
        } else {
            let ms = (60000.0 / bpm).round() as i32;
            tempo::set_tempo_ms(&mut q, ms);
            let got = tempo::preset_tempo_ms(&q).unwrap_or(ms);
            out.push(Applied {
                target: "tempo".into(),
                value: tempo_text(got),
                note: (got != ms).then(|| {
                    format!(
                        "tempo is {}..{} BPM, clamped",
                        tempo::ms_to_bpm(TAP_MAX_MS),
                        tempo::ms_to_bpm(TAP_MIN_MS)
                    )
                }),
            });
        }
    } else {
        for b in &touched {
            if TEMPO_BLOCKS.contains(&b.key) {
                tempo::sync_to_tempo(&mut q, b.key);
            }
        }
    }

    if !errors.is_empty() {
        return Err(SpecError { errors });
    }
    set_fields.sort_by_key(|(f, ..)| *f);
    for (field, raw, target, note) in set_fields {
        let b = params::owner(field).expect("block field");
        let fp = b
            .fields(&q)
            .into_iter()
            .find(|(f, _)| *f == field)
            .map(|(_, fp)| fp)
            .expect("field of the current type");
        let now = q.param_i24(field);
        let mut note = note;
        if now != raw {
            let n = format!("follows the tempo: set to {}", fp.format(now));
            note = Some(match note {
                Some(old) => format!("{old}; {n}"),
                None => n,
            });
        }
        out.push(Applied {
            target,
            value: fp.format(now),
            note,
        });
    }
    *p = q;
    Ok(out)
}

fn apply_modifier(q: &mut Preset, m: &ModifierSpec) -> Result<Applied, String> {
    let key = if norm(&m.block) == "novadrive" {
        "novadrive"
    } else {
        find_block(&m.block)?.key
    };
    let targets: Vec<_> = modifiers::modifier_targets(q, true)
        .into_iter()
        .filter(|t| t.key == key)
        .collect();
    let want = norm(&m.param);
    let Some(t) = targets
        .iter()
        .find(|t| norm(&t.name) == want || norm(&params::label(&t.name)) == want)
    else {
        let what = match key {
            "novadrive" => "novadrive".to_string(),
            k => type_desc(params::block(k), q),
        };
        return Err(if targets.is_empty() {
            format!("modifiers can't control any parameter of {what}")
        } else {
            format!(
                "a modifier can't control {:?} on {what}; it can control: {}",
                m.param,
                targets
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        });
    };
    let source = source_index(&m.source).ok_or_else(|| {
        format!(
            "unknown modifier source {:?}; sources: {}",
            m.source,
            SOURCES.join(", ")
        )
    })?;
    let old = if t.modifier.source == 0 {
        modifiers::UNASSIGNED
    } else {
        t.modifier
    };
    let pct = |v: Option<i32>, default: u8, what: &str| -> Result<u8, String> {
        match v {
            None => Ok(default),
            Some(v) if (0..=100).contains(&v) => Ok(v as u8),
            Some(v) => Err(format!("modifier {what} {v} is outside 0..100 %")),
        }
    };
    let min = pct(m.min, old.min, "min")?;
    let max = pct(m.max, old.max, "max")?;
    // Without a mid, a new min or max puts it halfway (a linear pedal).
    let mid_default = if m.min.is_some() || m.max.is_some() {
        ((min as u16 + max as u16) / 2) as u8
    } else {
        old.mid
    };
    let new = Modifier {
        source,
        min,
        mid: pct(m.mid, mid_default, "mid")?,
        max,
    };
    let new = if source == 0 {
        modifiers::UNASSIGNED
    } else {
        new
    };
    modifiers::write_modifier(q, t.slot, new);
    Ok(Applied {
        target: format!("modifier {key}.{}", t.name),
        value: modifier_text(t.param.as_ref(), new),
        note: None,
    })
}

fn modifier_text(param: Option<&FieldParam>, m: Modifier) -> String {
    let src = SOURCES.get(m.source as usize).copied().unwrap_or("?");
    if m.source == 0 {
        return "None".into();
    }
    match param {
        Some(fp) => format!(
            "{src}: {} / {} / {} ({} / {} / {} %)",
            fp.format(modifiers::value_at(fp, m.min)),
            fp.format(modifiers::value_at(fp, m.mid)),
            fp.format(modifiers::value_at(fp, m.max)),
            m.min,
            m.mid,
            m.max
        ),
        None => format!("{src}: {} / {} / {} %", m.min, m.mid, m.max),
    }
}

fn on_off(on: bool) -> &'static str {
    if on {
        "On"
    } else {
        "Off"
    }
}

fn loops_text(loops: &[bool; 4]) -> String {
    let on: Vec<String> = (0..4)
        .filter(|&i| loops[i])
        .map(|i| (i + 1).to_string())
        .collect();
    if on.is_empty() {
        "off".into()
    } else {
        on.join(", ")
    }
}

fn tempo_text(ms: i32) -> String {
    format!("{} BPM ({ms} ms)", tempo::ms_to_bpm(ms))
}

/// A built-in starting point for new presets: the factory preset "All
/// Bypassed" (factory slot 19-5), a complete preset the unit accepts.
pub fn default_template() -> Preset {
    const HEX: [&str; 11] = [
        "416c6c20427970617373656400000059b133431a00680000000000000000090000010000000000ecffff080000030000",
        "0e0000f7ffff0c00000500000d00000200000000000f02000000000000000a00000100002e00003900002f0000020000",
        "000000000000ffffffffffff0b00000500000000000f02000000002200005a0000a6ffff010000000000640000000000",
        "ffffffffffffffffff130000030000000000000000000000000000000000070000040000000000000000000000000000",
        "1900000000000000000000000000000c0000f2ffff0f00000000000000000e00000b0000000000030000040000060000",
        "030000000000000000fdffff0a0000140000000000050000000000010000dbffff1300001000001b0000000000000000",
        "2b02002b02000000000600000600000000000f0200080000080000000000000000000000000000330000080000000000",
        "0000001600000000000000000000000000000000001400003200000a0000000000160000090000000000200000090000",
        "0000002d000009000001000000c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c8",
        "00c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c8",
        "00c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c800c8c8ffffffffffffffffff",
    ];
    let hex: String = HEX.concat();
    let raw: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
        .collect();
    Preset::from_raw(raw).expect("516 bytes")
}

/// A new preset: [`empty_preset`](crate::empty::empty_preset) of `template`
/// (every block, loop and modifier off), then the spec. A block listed in
/// the spec is switched on unless it says `on: false`.
pub fn new_preset(
    spec: &PresetSpec,
    template: &Preset,
) -> Result<(Preset, Vec<Applied>), SpecError> {
    let mut p = crate::empty::empty_preset(template);
    let mut spec = spec.clone();
    for bs in spec.blocks.values_mut() {
        bs.on.get_or_insert(true);
    }
    let applied = apply_spec(&mut p, &spec)?;
    Ok((p, applied))
}

// ---------------------------------------------------------------- describe

/// Name → display value, in parameter order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParamValues(pub Vec<(String, String)>);

impl Serialize for ParamValues {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

impl ParamValues {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BlockSummary {
    pub block: &'static str,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_name: Option<&'static str>,
    pub on: bool,
    /// Labels → display values; only for blocks that are on (all with `full`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<ParamValues>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModifierSummary {
    pub block: &'static str,
    pub param: String,
    /// "Exp: 0 % / 50 % / 100 % (0 / 50 / 100 %)": values at min / mid / max.
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PresetSummary {
    pub name: String,
    pub routing: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tempo: Option<String>,
    /// Audio order: filter, comp, (loops, insert), mod, pitch, delay, reverb, EQ, gate.
    pub blocks: Vec<BlockSummary>,
    pub loops: String,
    pub insert: bool,
    pub boost: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relays: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<ModifierSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub novadrive: Option<i32>,
}

const AUDIO_ORDER: [&str; 8] = [
    "filter", "comp", "mod", "pitch", "delay", "reverb", "eq", "gate",
];

fn block_values(b: &'static Block, p: &Preset) -> ParamValues {
    let en = b.enable_field(p);
    let stored: Vec<usize> = b.stored_fields(p).into_iter().map(|(f, _)| f).collect();
    ParamValues(
        named_fields(b, p)
            .into_iter()
            .filter(|(f, _, _)| Some(*f) != en && stored.contains(f))
            .map(|(f, fp, n)| (label_of(&n), fp.format(p.param_i24(f))))
            .collect(),
    )
}

fn summarize(p: &Preset, full: bool) -> PresetSummary {
    let lv = levels::preset_levels(p);
    let blocks = AUDIO_ORDER
        .iter()
        .map(|&k| {
            let b = params::block(k);
            let on = b.is_on(p);
            BlockSummary {
                block: b.key,
                type_name: b.effect_type(p).map(|t| t.name),
                on,
                params: (on || full).then(|| block_values(b, p)),
            }
        })
        .collect();
    let (l, r) = levels::preset_outputs(p);
    let (ra, rb) = levels::preset_relays(p);
    let nova = novadrive::nova_preset(p);
    PresetSummary {
        name: p.name(),
        routing: ROUTINGS.get(lv.routing).copied().unwrap_or("?"),
        tempo: tempo::preset_tempo_ms(p)
            .filter(|&ms| ms > 0)
            .map(tempo_text),
        blocks,
        loops: loops_text(&lv.loops),
        insert: lv.insert,
        boost: if lv.boost {
            format!("+{} dB", lv.boost_level)
        } else {
            "off".into()
        },
        outputs: (l != 0 || r != 0).then(|| format!("L {l} dB, R {r} dB")),
        relays: (ra != 0 || rb != 0).then(|| {
            format!(
                "A {}, B {} ({} active)",
                levels::relay_text(ra),
                levels::relay_text(rb),
                if levels::level_on(p, "relay") {
                    "A"
                } else {
                    "B"
                }
            )
        }),
        modifiers: modifiers::modifier_targets(p, nova > 0)
            .into_iter()
            .filter(|t| t.modifier.source != 0)
            .map(|t| ModifierSummary {
                block: t.key,
                param: params::label(&t.name),
                value: modifier_text(t.param.as_ref(), t.modifier),
            })
            .collect(),
        novadrive: (nova > 0).then_some(nova),
    }
}

/// Compact summary: the chain with types and on/off, and the parameters of
/// the blocks that are on.
pub fn describe(p: &Preset) -> PresetSummary {
    summarize(p, false)
}

/// Like [`describe`], with every block's parameters.
pub fn describe_full(p: &Preset) -> PresetSummary {
    summarize(p, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate;

    fn spec(json_like: impl FnOnce(&mut PresetSpec)) -> PresetSpec {
        let mut s = PresetSpec::default();
        json_like(&mut s);
        s
    }

    fn block_spec(ty: Option<&str>, params: &[(&str, SpecValue)]) -> BlockSpec {
        BlockSpec {
            type_name: ty.map(String::from),
            on: None,
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    fn param(b: &str, p: &Preset, name: &str) -> FieldParam {
        find_param(params::block(b), p, name).unwrap().1
    }

    fn factory() -> crate::Bank {
        crate::vyzor::load_bank(include_bytes!("../../../testdata/Factory_Set.SQS")).unwrap()
    }

    #[test]
    fn default_template_is_a_valid_factory_preset() {
        let t = default_template();
        assert_eq!(t.name(), "All Bypassed");
        assert_eq!(&t, factory().get(99).unwrap());
        assert!(validate::check(&t).is_ok());
    }

    #[test]
    fn spec_round_trips_through_describe() {
        let s = spec(|s| {
            s.name = Some("Edge Dotted".into());
            s.routing = Some("semi parallel".into());
            s.tempo_bpm = Some(120.0);
            s.blocks.insert(
                "delay".into(),
                block_spec(
                    Some("ping pong"),
                    &[
                        ("Tempo", "1/8D".into()),
                        ("feedback", "35 %".into()),
                        ("FB high cut", "4 kHz".into()),
                        ("Mix", 30.0.into()),
                    ],
                ),
            );
            s.blocks.insert(
                "Reverb".into(),
                block_spec(
                    Some("Hall"),
                    &[("Decay", "3.5 s".into()), ("predly", "20 ms".into())],
                ),
            );
            s.modifiers.push(ModifierSpec {
                block: "delay".into(),
                param: "Mix".into(),
                source: "Exp".into(),
                min: Some(0),
                mid: None,
                max: Some(60),
            });
        });
        let (p, applied) = new_preset(&s, &default_template()).unwrap();
        let d = describe(&p);
        assert_eq!(d.name, "Edge Dotted");
        assert_eq!(d.routing, "Semi Parallel");
        assert_eq!(d.tempo.as_deref(), Some("120 BPM (500 ms)"));
        let delay = d.blocks.iter().find(|b| b.block == "delay").unwrap();
        assert!(delay.on);
        assert_eq!(delay.type_name, Some("Ping Pong"));
        let v = delay.params.as_ref().unwrap();
        assert_eq!(v.get("Tempo"), Some("1/8D"));
        assert_eq!(v.get("Delay"), Some("375 ms")); // 500 ms × 3/4
        assert_eq!(v.get("Feedback"), Some("35 %"));
        assert_eq!(v.get("FB high cut"), Some("4.06 kHz"));
        assert_eq!(v.get("Mix"), Some("30 %"));
        let reverb = d.blocks.iter().find(|b| b.block == "reverb").unwrap();
        assert_eq!(reverb.type_name, Some("Hall"));
        let rv = reverb.params.as_ref().unwrap();
        assert_eq!(rv.get("Decay"), Some("3.5 s"));
        assert_eq!(rv.get("Pre-delay"), Some("20 ms"));
        // Bypassed blocks are listed without parameters.
        let comp = d.blocks.iter().find(|b| b.block == "comp").unwrap();
        assert!(!comp.on && comp.params.is_none());
        assert_eq!(d.modifiers.len(), 1);
        assert_eq!(d.modifiers[0].param, "Mix");
        // Mid defaults to halfway between the given min and max.
        assert_eq!(
            d.modifiers[0].value,
            "Exp: 0 % / 30 % / 60 % (0 / 30 / 60 %)"
        );
        // The report shows what was set, with the nearest-step note for 4 kHz.
        let fb = applied.iter().find(|a| a.target == "delay.Fbhcut").unwrap();
        assert_eq!(fb.value, "4.06 kHz");
        assert!(fb.note.as_ref().unwrap().contains("nearest"));
        // Re-applying the described values changes nothing.
        let mut again = p.clone();
        let respec = spec(|s| {
            s.blocks.insert(
                "delay".into(),
                block_spec(
                    None,
                    &v.0.iter()
                        .map(|(k, val)| (k.as_str(), SpecValue::from(val.as_str())))
                        .collect::<Vec<_>>(),
                ),
            );
        });
        apply_spec(&mut again, &respec).unwrap();
        assert_eq!(again, p);
    }

    #[test]
    fn resolves_display_strings() {
        let p = {
            let mut p = default_template();
            params::set_type(&mut p, "delay", 0);
            p
        };
        let r = |b: &str, n: &str, s: &str| resolve_value(&param(b, &p, n), &s.into()).unwrap();
        // Frequency index: 20 Hz · 2^(i/6).
        let fb = param("delay", &p, "Fbhcut");
        let (i, note) = r("delay", "Fbhcut", "2.5 kHz");
        assert_eq!(fb.format(i), "2.56 kHz");
        assert!(note.is_some());
        assert_eq!(r("delay", "Fbhcut", "2.56 kHz"), (i, None));
        assert_eq!(fb.format(r("delay", "Fbhcut", "2500 Hz").0), "2.56 kHz");
        assert_eq!(r("delay", "Fblcut", "80 Hz"), (12, None));
        // ms, s → ms.
        assert_eq!(r("delay", "Delay", "350 ms"), (350, None));
        assert_eq!(r("delay", "Delay", "0.44 s").0, 440);
        assert_eq!(r("delay", "Delay", "5 s").0, 1800);
        // dB and %.
        assert_eq!(r("delay", "OutLev", "-6 dB"), (-6, None));
        assert_eq!(r("delay", "Mix", "35 %"), (35, None));
        assert_eq!(r("delay", "Mix", "35%"), (35, None));
        assert_eq!(r("delay", "Mix", "35"), (35, None));
        // Scaled (0.1 s steps) and LFO speeds.
        assert_eq!(r("reverb", "Decay", "2.5 s"), (25, None));
        assert_eq!(r("reverb", "Decay", "2500 ms"), (25, None));
        let speed = param("mod", &p, "Speed");
        let (i, _) = resolve_value(&speed, &"0.8 Hz".into()).unwrap();
        assert_eq!(speed.format(i), "0.80 Hz");
        // Choice lists: labels, and numbers inside labels.
        assert_eq!(r("reverb", "Size", "large").0, 4);
        assert_eq!(
            param("comp", &p, "Ratio").format(r("comp", "Ratio", "4:1").0),
            "4.0:1"
        );
        assert_eq!(
            param("comp", &p, "Attack").format(r("comp", "Attack", "1 ms").0),
            "1.0 ms"
        );
        // "10 ms" is not the label "1.0 ms".
        let attack = param("comp", &p, "Attack");
        assert_eq!(attack.format(r("comp", "Attack", "10 ms").0), "10 ms");
        let release = param("comp", &p, "Release");
        assert_eq!(release.format(r("comp", "Release", "300 ms").0), "300 ms");
        // Switches.
        assert_eq!(
            resolve_value(&param("comp", &p, "Comp"), &true.into())
                .unwrap()
                .0,
            1
        );
        // Raw numbers clamp, with a note.
        let (v, note) = resolve_value(&param("delay", &p, "Fb"), &500.0.into()).unwrap();
        assert_eq!(v, 120);
        assert!(note.unwrap().contains("clamped"));
        // Wrong unit.
        let e = resolve_value(&param("delay", &p, "Delay"), &"3 dB".into()).unwrap_err();
        assert!(e.contains("ms"), "{e}");
    }

    #[test]
    fn unknown_param_lists_the_alternatives() {
        let mut p = default_template();
        let s = spec(|s| {
            s.blocks.insert(
                "reverb".into(),
                block_spec(Some("Hall"), &[("Feedbak", 10.0.into())]),
            );
        });
        let before = p.clone();
        let e = apply_spec(&mut p, &s).unwrap_err();
        assert_eq!(p, before, "nothing changes on error");
        let msg = e.to_string();
        assert!(
            msg.contains("reverb (Hall) has no parameter \"Feedbak\""),
            "{msg}"
        );
        assert!(
            msg.contains("Decay") && msg.contains("PreDly (Pre-delay)"),
            "{msg}"
        );

        let bad_type = spec(|s| {
            s.blocks
                .insert("mod".into(), block_spec(Some("Chorus"), &[]));
        });
        let msg = apply_spec(&mut p, &bad_type).unwrap_err().to_string();
        assert!(msg.contains("Classic Chorus"), "{msg}");
        let bad_block = spec(|s| {
            s.blocks.insert("wah".into(), BlockSpec::default());
        });
        let msg = apply_spec(&mut p, &bad_block).unwrap_err().to_string();
        assert!(msg.contains("filter"), "{msg}");
    }

    #[test]
    fn every_new_preset_validates() {
        let templates = [default_template(), factory().get(0).unwrap().clone()];
        for t in &templates {
            for b in BLOCKS.iter() {
                for ty in 0..b.types.len().max(1) {
                    let mut bs = BlockSpec::default();
                    if b.has_type() {
                        bs.type_name = Some(b.types[ty].name.into());
                    }
                    // Every parameter at its top value, then at its bottom.
                    for top in [true, false] {
                        let mut s = PresetSpec {
                            name: Some(format!("{} {ty}", b.key)),
                            tempo_bpm: Some(97.0),
                            ..Default::default()
                        };
                        let mut probe = crate::empty::empty_preset(t);
                        if b.has_type() {
                            params::set_type(&mut probe, b.key, ty);
                        }
                        for (_, fp, n) in named_fields(b, &probe) {
                            let (lo, hi) = fp.range();
                            bs.params
                                .insert(n, SpecValue::Number(if top { hi } else { lo } as f64));
                        }
                        s.blocks.insert(b.key.into(), bs.clone());
                        let (p, _) = new_preset(&s, t).unwrap();
                        let r = validate::check(&p);
                        assert!(r.is_ok(), "{} {ty}: {:?}", b.key, r.errors);
                        assert!(b.is_on(&p) || !top, "{} listed → on", b.key);
                    }
                }
            }
        }
    }

    #[test]
    fn type_change_clamps_old_params() {
        // Scotty's Blues: Auto Wah with 11 stored params. Make it a
        // Resonance filter: Fmax (lo 34) sits where Speed was.
        let mut p = factory().get(0).unwrap().clone();
        let s = spec(|s| {
            s.blocks
                .insert("filter".into(), block_spec(Some("Resonance Filter"), &[]));
        });
        apply_spec(&mut p, &s).unwrap();
        let f = params::block("filter");
        assert_eq!(f.stored_fields(&p).len(), f.fields(&p).len());
        assert!(validate::check(&p).is_ok());
        for (field, fp) in f.fields(&p) {
            let (lo, hi) = fp.range();
            assert!((lo..=hi).contains(&p.param_i24(field)), "{}", fp.name());
        }
    }

    #[test]
    fn explicit_delay_time_switches_tempo_to_ignore() {
        let s = spec(|s| {
            s.tempo_bpm = Some(120.0);
            s.blocks.insert(
                "delay".into(),
                block_spec(Some("Single Delay"), &[("Delay", "440 ms".into())]),
            );
        });
        // The template's delay Tempo is 1/4.
        let (p, applied) = new_preset(&s, &default_template()).unwrap();
        let v = describe(&p);
        let d = v.blocks.iter().find(|b| b.block == "delay").unwrap();
        let d = d.params.as_ref().unwrap();
        assert_eq!(d.get("Delay"), Some("440 ms"));
        assert_eq!(d.get("Tempo"), Some("Ignore"));
        let a = applied.iter().find(|a| a.target == "delay.Delay").unwrap();
        assert!(a.note.as_ref().unwrap().contains("Ignore"));
    }

    #[test]
    fn modifier_errors_list_targets() {
        let mut p = default_template();
        let s = spec(|s| {
            s.blocks
                .insert("reverb".into(), block_spec(Some("Hall"), &[]));
            s.modifiers.push(ModifierSpec {
                block: "reverb".into(),
                param: "Size".into(),
                source: "Exp".into(),
                ..Default::default()
            });
        });
        let msg = apply_spec(&mut p, &s).unwrap_err().to_string();
        assert!(msg.contains("Decay") && msg.contains("Mix"), "{msg}");
    }

    #[test]
    fn duplicate_names_get_a_suffix() {
        let p = default_template();
        let fields = named_fields(params::block("delay"), &p);
        assert!(fields.iter().any(|(_, _, n)| n == "Delay#2"));
        let names: Vec<_> = fields.iter().map(|(_, _, n)| norm(n)).collect();
        let mut dedup = names.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(names.len(), dedup.len());
    }

    #[test]
    fn spec_deserializes_and_rejects_unknown_fields() {
        let s: PresetSpec = serde_json::from_str(
            r#"{"name":"X","blocks":{"delay":{"type":"Tape Delay","params":{"Mix":25,"Tempo":"1/4","Enable":true}}}}"#,
        )
        .unwrap();
        assert_eq!(s.blocks["delay"].params["Mix"], SpecValue::Number(25.0));
        assert_eq!(s.blocks["delay"].params["Tempo"], SpecValue::from("1/4"));
        assert_eq!(s.blocks["delay"].params["Enable"], SpecValue::Bool(true));
        let e = serde_json::from_str::<PresetSpec>(r#"{"blocks":{"delay":{"parameters":{}}}}"#)
            .unwrap_err()
            .to_string();
        assert!(e.contains("parameters") && e.contains("params"), "{e}");
    }

    #[test]
    fn catalog_is_compact_and_complete() {
        let c = catalog();
        assert_eq!(c.blocks.len(), 8);
        let delay = c.blocks.iter().find(|b| b.key == "delay").unwrap();
        assert_eq!(delay.types.len(), 1);
        assert_eq!(delay.types[0].types.len(), 7);
        let fbh = delay.types[0]
            .params
            .iter()
            .find(|p| p.name == "Fbhcut")
            .unwrap();
        assert_eq!(fbh.unit, "freq index");
        assert!(fbh.scale.as_ref().unwrap().starts_with("0=20 Hz"));
        assert!(fbh.scale.as_ref().unwrap().ends_with("60=20.48 kHz"));
        let detailed = catalog_block("delay").unwrap();
        let m = detailed.types[0].modifiable.as_ref().unwrap();
        assert!(m["Ping Pong"].contains(&"Width".to_string()));
    }
}
