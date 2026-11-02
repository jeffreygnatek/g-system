//! A decoded, display-ready view of a preset, and the edits a UI can make.
//! Shared by the desktop app, the CLI and the screenshot tool. Requires the
//! `serde` feature.

use crate::preset::{slot_label, Preset};
use crate::{globals, levels, modifiers, novadrive, params, switches, tempo};

#[derive(Debug, Clone, serde::Serialize)]
pub struct RowView {
    pub field: usize,
    pub label: String,
    pub value: i32,
    pub display: String,
    pub lo: i32,
    pub hi: i32,
    /// Labels for lo..=hi when the parameter is a value list.
    pub choices: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BlockView {
    pub key: &'static str,
    pub label: &'static str,
    pub on: bool,
    pub types: Vec<&'static str>,
    pub type_index: Option<i32>,
    pub rows: Vec<RowView>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChainView {
    pub key: &'static str,
    pub name: &'static str,
    pub on: bool,
    pub summary: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModifierView {
    pub slot: usize,
    /// Block key ("delay"…) or "novadrive".
    pub key: &'static str,
    pub label: String,
    /// Index into `sources`.
    pub source: u8,
    pub min: u8,
    pub mid: u8,
    pub max: u8,
    /// e.g. "Exp · 0 → 100 %", or "None".
    pub display: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PresetView {
    pub index: usize,
    pub label: String,
    pub name: String,
    pub chain: Vec<ChainView>,
    pub blocks: Vec<BlockView>,
    pub routing: usize,
    pub routings: Vec<&'static str>,
    pub loops: [bool; 4],
    pub insert: bool,
    pub boost: bool,
    pub boost_level: i32,
    pub bpm: Option<i32>,
    /// Modifier slots for the current effect types (NovaDrive's included).
    pub modifiers: Vec<ModifierView>,
    pub sources: Vec<&'static str>,
    /// NovaDrive NDR-1 preset recalled with this preset: 1-18, 0 = none.
    pub nova_preset: i32,
}

/// Front-panel order, plus EQ and Gate.
const CHAIN: [(&str, &str, bool); 14] = [
    ("filter", "Filter", true),
    ("comp", "Comp", true),
    ("loop1", "Loop 1", false),
    ("loop2", "Loop 2", false),
    ("loop3", "Loop 3", false),
    ("loop4", "Loop 4", false),
    ("insert", "Insert", false),
    ("mod", "Mod", true),
    ("pitch", "Pitch", true),
    ("delay", "Delay", true),
    ("reverb", "Reverb", true),
    ("eq", "EQ", true),
    ("gate", "Gate", true),
    ("boost", "Boost", false),
];
const CARDS: [&str; 8] = [
    "filter", "comp", "mod", "pitch", "delay", "reverb", "eq", "gate",
];

pub fn preset_view(index: usize, p: &Preset) -> PresetView {
    let lv = levels::preset_levels(p);
    let chain = CHAIN
        .iter()
        .map(|&(key, name, is_block)| {
            let (on, summary) = if is_block {
                let b = params::block(key);
                let on = b.is_on(p);
                let t = b.effect_type(p).map(|t| t.name).unwrap_or("On");
                (on, if on { t.to_string() } else { "Off".into() })
            } else if key == "boost" {
                (
                    lv.boost,
                    if lv.boost {
                        format!("+{} dB", lv.boost_level)
                    } else {
                        "Off".into()
                    },
                )
            } else {
                let on = levels::level_on(p, key);
                (on, if on { "On".into() } else { "Off".into() })
            };
            ChainView {
                key,
                name,
                on,
                summary,
            }
        })
        .collect();
    let blocks = CARDS
        .iter()
        .map(|&key| {
            let b = params::block(key);
            let en = b.enable_field(p);
            let rows = b
                .stored_fields(p)
                .into_iter()
                .filter(|(f, fp)| Some(*f) != en && !matches!(fp, params::FieldParam::Type(_)))
                .map(|(f, fp)| {
                    let (lo, hi) = fp.range();
                    let v = p.param_i24(f);
                    RowView {
                        field: f,
                        label: params::label(fp.name()),
                        value: v,
                        display: fp.format(v),
                        lo,
                        hi,
                        choices: fp.choices(),
                    }
                })
                .collect();
            BlockView {
                key,
                label: b.label,
                on: b.is_on(p),
                types: b.types.iter().map(|t| t.name).collect(),
                type_index: b.has_type().then(|| p.param_i24(b.first_field())),
                rows,
            }
        })
        .collect();
    PresetView {
        index,
        label: slot_label(index),
        name: p.name(),
        chain,
        blocks,
        routing: lv.routing,
        routings: levels::ROUTINGS.to_vec(),
        loops: lv.loops,
        insert: lv.insert,
        boost: lv.boost,
        boost_level: lv.boost_level,
        modifiers: modifiers::modifier_targets(p, true)
            .into_iter()
            .map(|t| {
                let m = t.modifier;
                ModifierView {
                    slot: t.slot,
                    key: t.key,
                    label: params::label(&t.name),
                    source: m.source,
                    min: m.min,
                    mid: m.mid,
                    max: m.max,
                    display: if m.source == 0 {
                        "None".into()
                    } else {
                        format!(
                            "{} · {} / {} / {} %",
                            modifiers::SOURCES.get(m.source as usize).unwrap_or(&"?"),
                            m.min,
                            m.mid,
                            m.max
                        )
                    },
                }
            })
            .collect(),
        sources: modifiers::SOURCES.to_vec(),
        nova_preset: novadrive::nova_preset(p),
        bpm: tempo::preset_tempo_ms(p)
            .filter(|&ms| ms > 0)
            .map(tempo::ms_to_bpm),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GlobalView {
    pub block: u8,
    pub param: u16,
    pub name: &'static str,
    pub help: &'static str,
    /// None when the unit didn't report this parameter.
    pub value: Option<i32>,
    pub display: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GlobalGroupView {
    pub title: &'static str,
    pub params: Vec<GlobalView>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SwitchView {
    /// 1-based footswitch number.
    pub number: usize,
    pub display: String,
    pub fixed: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GlobalsView {
    pub groups: Vec<GlobalGroupView>,
    pub switches: Vec<SwitchView>,
}

/// Build the Global screen from `(block, param) → raw value` read from the unit.
pub fn globals_view(read: &std::collections::HashMap<(u8, u16), i32>) -> GlobalsView {
    let groups = globals::GLOBAL_GROUPS
        .iter()
        .map(|g| GlobalGroupView {
            title: g.title,
            params: g
                .params
                .iter()
                .map(|gp| {
                    let value = read.get(&(gp.block, gp.param)).map(|&raw| gp.value(raw));
                    GlobalView {
                        block: gp.block,
                        param: gp.param,
                        name: gp.name,
                        help: gp.help,
                        value,
                        display: value.map(|v| gp.format(v)).unwrap_or_else(|| "—".into()),
                    }
                })
                .collect(),
        })
        .collect();
    let switches = (0..switches::SWITCH_COUNT)
        .map(|i| {
            let fixed = switches::fixed_switch(i);
            let display = match fixed {
                Some(f) => f.to_string(),
                None => read
                    .get(&(switches::SWITCH_BLOCK, switches::SWITCH_FIRST + i as u16))
                    .map(|&v| switches::describe_switch(switches::decode_switch(v)))
                    .unwrap_or_else(|| "—".into()),
            };
            SwitchView {
                number: i + 1,
                display,
                fixed: fixed.is_some(),
            }
        })
        .collect();
    GlobalsView { groups, switches }
}

/// One edit to a preset, as sent by the UI.
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Edit {
    Field {
        field: usize,
        value: i32,
    },
    Type {
        block: String,
        value: usize,
    },
    Toggle {
        key: String,
    },
    Routing {
        value: usize,
    },
    BoostLevel {
        value: i32,
    },
    Bpm {
        value: i32,
    },
    Modifier {
        slot: usize,
        source: u8,
        min: u8,
        mid: u8,
        max: u8,
    },
    Nova {
        value: i32,
    },
}

/// Apply one edit. Values are clamped to the parameter's range, and delay
/// times / speeds follow tempo changes.
pub fn apply(p: &mut Preset, edit: Edit) -> Result<(), String> {
    match edit {
        Edit::Field { field, value } => {
            let b = params::owner(field).ok_or("field is not part of an effect block")?;
            let fp = b
                .fields(p)
                .into_iter()
                .find(|(f, _)| *f == field)
                .ok_or("field not used by this effect type")?
                .1;
            p.set_param_i24(field, fp.clamp(value));
            if matches!(b.key, "delay" | "filter" | "mod") {
                tempo::sync_to_tempo(p, b.key);
            }
        }
        Edit::Type { block, value } => params::set_type(p, &block, value),
        Edit::Toggle { key } => {
            if levels::level_bit(&key).is_some() {
                let on = levels::level_on(p, &key);
                levels::set_level(p, &key, !on);
            } else {
                let b = params::BLOCKS
                    .iter()
                    .find(|b| b.key == key)
                    .ok_or("unknown block")?;
                let f = b.enable_field(p).ok_or("block has no on/off")?;
                let v = p.param_i24(f);
                p.set_param_i24(f, (v == 0) as i32);
            }
        }
        Edit::Routing { value } => levels::set_routing(p, value),
        Edit::BoostLevel { value } => levels::set_boost_level(p, value),
        Edit::Bpm { value } => tempo::set_tempo_ms(p, tempo::bpm_to_ms(value.clamp(20, 600))),
        Edit::Modifier {
            slot,
            source,
            min,
            mid,
            max,
        } => {
            if slot >= modifiers::MODIFIER_SLOTS || source as usize >= modifiers::SOURCES.len() {
                return Err("bad modifier".into());
            }
            modifiers::write_modifier(
                p,
                slot,
                modifiers::Modifier {
                    source,
                    min: min.min(100),
                    mid: mid.min(100),
                    max: max.min(100),
                },
            );
        }
        Edit::Nova { value } => novadrive::write_nova_preset(p, value),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vyzor;

    fn scotty() -> Preset {
        let b = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../testdata/Factory_Set.SQS"
        ))
        .unwrap();
        vyzor::load_bank(&b).unwrap().get(0).unwrap().clone()
    }

    #[test]
    fn view_summarizes_the_chain() {
        let v = preset_view(0, &scotty());
        let on: Vec<_> = v
            .chain
            .iter()
            .filter(|c| c.on)
            .map(|c| (c.key, c.summary.as_str()))
            .collect();
        assert_eq!(on, [("reverb", "Plate"), ("eq", "On")]);
        assert_eq!(v.bpm, Some(120));
        // Auto Wah stores 11 params: AutoOn is not shown.
        let filter = v.blocks.iter().find(|b| b.key == "filter").unwrap();
        assert!(filter.rows.iter().all(|r| r.label != "Auto on"));
    }

    #[test]
    fn edits_toggle_retype_and_follow_tempo() {
        let mut p = scotty();
        apply(&mut p, Edit::Toggle { key: "comp".into() }).unwrap();
        assert!(params::block("comp").is_on(&p));
        apply(
            &mut p,
            Edit::Toggle {
                key: "loop2".into(),
            },
        )
        .unwrap();
        assert!(levels::preset_levels(&p).loops[1]);
        apply(
            &mut p,
            Edit::Type {
                block: "filter".into(),
                value: 0,
            },
        )
        .unwrap();
        assert_eq!(
            params::block("filter").effect_type(&p).unwrap().name,
            "Resonance Filter"
        );
        // Delay is Single at 1/4: a new tempo of 100 BPM sets 600 ms.
        apply(&mut p, Edit::Bpm { value: 100 }).unwrap();
        let d = params::block("delay")
            .fields(&p)
            .into_iter()
            .find(|(_, f)| f.name() == "Delay")
            .unwrap()
            .0;
        assert_eq!(p.param_i24(d), 600);
        // A tempo-synced delay time snaps back to the note value.
        apply(
            &mut p,
            Edit::Field {
                field: 89,
                value: 1234,
            },
        )
        .unwrap();
        assert_eq!(p.param_i24(89), 600);
        // Out-of-range values are clamped (feedback: 0..=120 %).
        apply(
            &mut p,
            Edit::Field {
                field: 96,
                value: 999,
            },
        )
        .unwrap();
        assert_eq!(p.param_i24(96), 120);
        assert!(apply(&mut p, Edit::Field { field: 0, value: 1 }).is_err());
    }
}
