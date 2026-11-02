//! Preset parameter layout: which field holds what, with ranges, value lists
//! and display formats for every effect block and type.
//!
//! Rust port of `web/src/core/params.ts` from davidavalanches/gsystem-editor
//! (commit 9b50ef8), Copyright © David Avalanches, GPL-3.0. Their names,
//! ranges and value lists come from the parameter tables of G-System firmware
//! 4.02, cross-checked against factory and user presets on real hardware.
//!
//! A preset's 516 data bytes are a 21-byte name followed by 165 fields of 3
//! bytes (signed 24-bit LE). Fields 0-2 hold packed levels/outputs (see
//! [`crate::levels`]); 3-124 the effect blocks; 125-164 packed assignments.
//! Each block is `[parameter count][parameters…]` in a fixed slot, in the
//! order Comp, Filter, Mod, Pitch, Reverb, Gate, Delay, EQ. Parameter 0 is the
//! effect type when the block has one; which parameters follow depends on it.

use crate::preset::Preset;

/// How a raw value is shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fmt {
    Plain,
    /// `"{v} {suffix}"`
    Unit(&'static str),
    /// `"{v*factor:.1} {suffix}"`
    Scaled(f64, &'static str),
    /// Sixth-octave index from 20 Hz.
    Freq,
    /// 1/24-octave index from 0.05 Hz.
    Lfo,
    /// EQ bandwidth: 0.1 to 4.0 octaves, `(v + 1) / 10`.
    OctWidth,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Param {
    pub name: &'static str,
    pub lo: i32,
    pub hi: i32,
    pub choices: &'static [&'static str],
    /// Value of the first entry of `choices` (e.g. -12 for intervals).
    pub choices_base: i32,
    pub fmt: Fmt,
}

impl Param {
    pub fn format(&self, v: i32) -> String {
        if !self.choices.is_empty() {
            let i = v - self.choices_base;
            return if i >= 0 && (i as usize) < self.choices.len() {
                self.choices[i as usize].to_string()
            } else {
                v.to_string()
            };
        }
        match self.fmt {
            Fmt::Plain => v.to_string(),
            Fmt::Unit(s) => format!("{v} {s}"),
            Fmt::Scaled(f, s) => format!("{:.1} {s}", v as f64 * f),
            Fmt::Freq => freq(v),
            Fmt::Lfo => lfo(v),
            Fmt::OctWidth => format!("{:.1} oct", (v + 1) as f64 / 10.0),
        }
    }
    pub fn is_switch(&self) -> bool {
        self.choices == ONOFF
    }
    pub fn clamp(&self, v: i32) -> i32 {
        v.clamp(self.lo, self.hi)
    }
}

fn hz(v: f64) -> String {
    if v >= 1000.0 {
        format!("{:.2} kHz", v / 1000.0)
    } else {
        format!("{} Hz", v.round() as i64)
    }
}
/// Filter/EQ frequencies: 20 Hz to 20 kHz in sixth-octave steps.
pub fn freq(i: i32) -> String {
    hz(20.0 * 2f64.powf(i as f64 / 6.0))
}
/// Modulation speed: 0.05 Hz to 20 Hz in 1/24-octave steps.
pub fn lfo(i: i32) -> String {
    let f = 0.05 * 2f64.powf(i as f64 / 24.0);
    if f < 10.0 {
        format!("{f:.2} Hz")
    } else {
        format!("{f:.1} Hz")
    }
}

const fn p(name: &'static str, lo: i32, hi: i32, fmt: Fmt) -> Param {
    Param {
        name,
        lo,
        hi,
        choices: &[],
        choices_base: 0,
        fmt,
    }
}
const fn e(name: &'static str, choices: &'static [&'static str]) -> Param {
    Param {
        name,
        lo: 0,
        hi: choices.len() as i32 - 1,
        choices,
        choices_base: 0,
        fmt: Fmt::Plain,
    }
}
/// Value list whose values index into a longer list (e.g. release starts at index 13).
const fn l(name: &'static str, lo: i32, hi: i32, choices: &'static [&'static str]) -> Param {
    Param {
        name,
        lo,
        hi,
        choices,
        choices_base: 0,
        fmt: Fmt::Plain,
    }
}

const DB: Fmt = Fmt::Unit("dB");
const MS: Fmt = Fmt::Unit("ms");
const PCT: Fmt = Fmt::Unit("%");
const CENTS: Fmt = Fmt::Unit("cents");
const PLAIN: Fmt = Fmt::Plain;

pub const ONOFF: &[&str] = &["Off", "On"];
pub const TEMPO: &[&str] = &[
    "Ignore", "2", "1", "1/2D", "1/2", "1/2T", "1/4D", "1/4", "1/4T", "1/8D", "1/8", "1/8T",
    "1/16D", "1/16", "1/16T", "1/32D", "1/32", "1/32T",
];
pub const DELAY_TEMPO: &[&str] = &[
    "Ignore", "1", "1/2D", "1/2", "1/2T", "1/4D", "1/4", "1/4T", "1/8D", "1/8", "1/8T", "1/16D",
    "1/16", "1/16T", "1/32D", "1/32", "1/32T",
];
const TIMES: &[&str] = &[
    "0.3 ms", "0.5 ms", "0.7 ms", "1.0 ms", "1.4 ms", "2.0 ms", "3.0 ms", "5.0 ms", "7.0 ms",
    "10 ms", "14 ms", "20 ms", "30 ms", "50 ms", "70 ms", "100 ms", "140 ms", "200 ms", "300 ms",
    "500 ms", "700 ms", "1.0 s", "1.4 s", "2.0 s",
];
const VOWELS: &[&str] = &[
    "HEAT", "HIT", "HEAD", "HAD", "BUT", "FATHER", "ALL", "BOOT", "FOOT", "BIRD",
];
const SLOW_FAST: &[&str] = &["Slow", "Fast"];
const TRI_SINE: &[&str] = &["Tri", "Sine"];
const LOW_HIGH: &[&str] = &["Low", "High"];

const TAP: Param = p("Tap", 100, 3000, MS);
const OUTLEV: Param = p("OutLev", -100, 0, DB);
const MIX: Param = p("Mix", 0, 100, PCT);
const ENABLE: Param = e("Enable", ONOFF);

const FILTER_WAH: &[Param] = &[
    ENABLE,
    TAP,
    e("Tempo", TEMPO),
    p("Sense", 0, 10, PLAIN),
    e("Resp", SLOW_FAST),
    p("Speed", 0, 208, Fmt::Lfo),
    e("Shape", TRI_SINE),
    p("Sweep", 0, 100, PCT),
    e("Range", LOW_HIGH),
    OUTLEV,
    e(
        "AutoOn",
        &["Off", "0.3s", "0.5s", "1s", "3s", "5s", "10s", "Inf"],
    ),
];
const FILTER_RES: &[Param] = &[
    ENABLE,
    TAP,
    e("Tempo", TEMPO),
    e("Style", &["2nd", "4th"]),
    p("Sense", 0, 10, PLAIN),
    e("Resp", SLOW_FAST),
    p("Fmax", 34, 54, Fmt::Freq),
    p("Hicut", 18, 57, Fmt::Freq),
    p("Resnce", 0, 100, PCT),
    p("Speed", 0, 208, Fmt::Lfo),
    e("Shape", TRI_SINE),
    OUTLEV,
];
const FILTER_FORMANT: &[Param] = &[
    ENABLE,
    TAP,
    e("Tempo", TEMPO),
    e("Low", VOWELS),
    e("Mid", VOWELS),
    e("High", VOWELS),
    p("Sweep", 0, 100, PCT),
    p("Bright", 0, 100, PCT),
    p("Resnce", 0, 100, PCT),
    p("Sense", 0, 10, PLAIN),
    e("Resp", SLOW_FAST),
    p("Speed", 0, 208, Fmt::Lfo),
    e("Shape", TRI_SINE),
    OUTLEV,
];

const MOD_CHORUS: &[Param] = &[
    ENABLE,
    TAP,
    e("Tempo", TEMPO),
    p("Speed", 0, 208, Fmt::Lfo),
    p("Depth", 0, 100, PCT),
    p("HiCut", 0, 60, Fmt::Freq),
    MIX,
    OUTLEV,
    p("Cho Dly", 0, 500, Fmt::Scaled(0.1, "ms")),
    e("Ratio", ONOFF),
    e("PhaRev", ONOFF),
    p("FeedB", -100, 100, PCT),
    p("FB Cut", 0, 60, Fmt::Freq),
];
const MOD_PHASER: &[Param] = &[
    ENABLE,
    TAP,
    e("Tempo", TEMPO),
    p("Speed", 0, 208, Fmt::Lfo),
    p("Depth", 0, 100, PCT),
    p("Fb", -100, 100, PCT),
    e("Range", LOW_HIGH),
    e("Rev", ONOFF),
    MIX,
    OUTLEV,
];
const MOD_UNIVIBE: &[Param] = &[
    ENABLE,
    e("BpMode", &["Delay", "Direct"]),
    e("Tempo1", TEMPO),
    e("Tempo2", TEMPO),
    p("Depth", 0, 100, PCT),
    p("Speed1", 38, 184, Fmt::Lfo),
    p("Speed2", 38, 184, Fmt::Lfo),
    p("Attack", 50, 5000, MS),
    p("Releas", 50, 5000, MS),
    e("Select", &["Speed1", "Speed2"]),
    e("PhaRev", ONOFF),
    MIX,
    OUTLEV,
];
const MOD_TREMOLO: &[Param] = &[
    ENABLE,
    TAP,
    e("Tempo", TEMPO),
    p("Speed", 0, 368, PLAIN),
    p("Depth", 0, 100, PCT),
    e("Type", &["Soft", "Hard"]),
    p("Width", 0, 100, PCT),
    p("Hi Cut", 0, 60, Fmt::Freq),
    MIX,
    OUTLEV,
];

const INTERVALS: &[&str] = &[
    "-13", "-12", "-11", "-10", "-9", "-Oct1", "-7", "-6", "-5", "-4", "-3", "-2", "Unison", "2",
    "3", "4", "5", "6", "7", "Oct1", "9", "10", "11", "12", "13",
];
const fn interval(name: &'static str) -> Param {
    Param {
        name,
        lo: -12,
        hi: 12,
        choices: INTERVALS,
        choices_base: -12,
        fmt: Fmt::Plain,
    }
}
const DIR: Param = e("Dir", &["Down", "Up"]);
const PITCH_SHIFTER: &[Param] = &[
    ENABLE,
    p("Voice1", -1200, 1200, CENTS),
    p("Voice2", -1200, 1200, CENTS),
    p("Pan1", -50, 50, PLAIN),
    p("Pan2", -50, 50, PLAIN),
    p("Delay1", 0, 350, MS),
    p("Delay2", 0, 350, MS),
    p("Fb1", 0, 100, PCT),
    p("Fb2", 0, 100, PCT),
    p("Level1", -100, 0, DB),
    p("Level2", -100, 0, DB),
    MIX,
    OUTLEV,
    DIR,
    e("Range", &["1 Oct", "2 Oct"]),
    p("Pitch", 0, 100, PCT),
    p("VoiceL", -100, 100, PLAIN),
    p("VoiceR", -100, 100, PLAIN),
];
const PITCH_INTELLIGENT: &[Param] = &[
    ENABLE,
    interval("Voice1"),
    interval("Voice2"),
    p("Pan1", -50, 50, PLAIN),
    p("Pan2", -50, 50, PLAIN),
    p("Delay1", 0, 50, MS),
    p("Delay2", 0, 50, MS),
    e(
        "Key",
        &[
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ],
    ),
    e(
        "Scale",
        &[
            "Ionian", "Dorian", "Phrygi", "Lydian", "MixoLy", "Aeolia", "Locria", "PntMin",
            "PntMaj", "Blues", "DimHlf", "DimWhl", "Whole", "MelMin", "HrmMin", "SupLoc", "Lyd b7",
            "PhryMa",
        ],
    ),
    p("Level1", -100, 0, DB),
    p("Level2", -100, 0, DB),
    MIX,
    OUTLEV,
    DIR,
    e("Mode", &["Step", "Smooth"]),
    p("Pitch", 0, 100, PCT),
    p("VoiceL", -100, 100, PLAIN),
    p("VoiceR", -100, 100, PLAIN),
];
/// The doubler's parameter names are not in the firmware tables.
const PITCH_DOUBLER: &[Param] = &[
    ENABLE,
    p("Pitch", -20, 20, PLAIN),
    p("Param 3", -20, 20, PLAIN),
    p("Param 4", -50, 50, PLAIN),
    p("Param 5", -50, 50, PLAIN),
    p("Delay", 3, 60, MS),
    p("Param 7", 1, 20, PLAIN),
    p("Param 8", 0, 100, PLAIN),
    p("Param 9", 0, 100, PLAIN),
    p("Param 10", -100, 0, DB),
    p("Param 11", -100, 0, DB),
    p("Param 12", 0, 100, PLAIN),
    OUTLEV,
    e("Side", &["Left", "Right"]),
];

const DELAY: &[Param] = &[
    ENABLE,
    p("Delay", 0, 1800, MS),
    p("Delay1", 0, 1800, MS),
    p("Delay2", 0, 1800, MS),
    e("Tempo", DELAY_TEMPO),
    e("Tempo1", DELAY_TEMPO),
    e("Tempo2", DELAY_TEMPO),
    p("TapTime", 100, 3000, MS),
    p("Fb", 0, 120, PCT),
    p("Fb1", 0, 120, PCT),
    p("Fb2", 0, 120, PCT),
    p("Drive", 0, 24, DB),
    p("Drive1", 0, 24, DB),
    p("Drive2", 0, 24, DB),
    p("Fbhcut", 0, 60, Fmt::Freq),
    p("Fblcut", 0, 60, Fmt::Freq),
    p("Pan1", -50, 50, PLAIN),
    p("Pan2", -50, 50, PLAIN),
    MIX,
    OUTLEV,
    p("Width", 0, 100, PCT),
    p("Offset", 0, 200, MS),
    p("Sense", -50, 0, DB),
    p("Damp", 0, 100, DB),
    p("Rel", 20, 1000, MS),
    p("Delay", 50, 800, MS),
];

const REVERB: &[Param] = &[
    ENABLE,
    p("Decay", 1, 200, Fmt::Scaled(0.1, "s")),
    p("PreDly", 0, 100, MS),
    e("Shape", &["Round", "Curved", "Square"]),
    e(
        "Size",
        &[
            "Box", "Tiny", "Small", "Medium", "Large", "XL", "Grand", "Huge",
        ],
    ),
    e(
        "Hi Col",
        &["Wool", "Warm", "Real", "Clear", "Bright", "Crisp", "Glass"],
    ),
    p("Hi Lev", -25, 25, PLAIN),
    e(
        "Lo Col",
        &["Thick", "Round", "Real", "Light", "Tight", "Thin", "NoBass"],
    ),
    p("Lo Lev", -25, 25, PLAIN),
    p("Early", -100, 0, DB),
    p("RevLev", -100, 0, DB),
    p("Diff", -25, 25, PLAIN),
    MIX,
    OUTLEV,
];

const COMP: &[Param] = &[
    e("Comp", ONOFF),
    p("Thres", -40, 0, DB),
    e(
        "Ratio",
        &[
            "Off", "1.12:1", "1.25:1", "1.40:1", "1.60:1", "1.80:1", "2.0:1", "2.5:1", "3.2:1",
            "4.0:1", "5.6:1", "8.0:1", "16:1", "32:1", "64:1", "inf:1",
        ],
    ),
    l("Attack", 0, 16, TIMES),
    l("Releas", 13, 23, TIMES),
    p("Gain", -100, 12, DB),
    p("Drive", 1, 20, PLAIN),
    p("Resp", 1, 10, PLAIN),
];

const GATE: &[Param] = &[
    e("NG", ONOFF),
    e("Mode", &["Hard", "Soft"]),
    p("Thres", -60, 0, DB),
    p("Damp", 0, 90, DB),
    p("Speed", 3, 200, Fmt::Unit("dB/s")),
];

// The original table shows width as v × 0.1 oct, one step low: Vyzor shows a
// stored 9 as 1.0 oct ("Scotty's Blues") and its scale runs 0.1–4.0 oct.
const OCT: Fmt = Fmt::OctWidth;
const EQ: &[Param] = &[
    p("Gain1", -12, 12, DB),
    p("Freq1", 0, 60, Fmt::Freq),
    p("Width1", 0, 39, OCT),
    p("Gain2", -12, 12, DB),
    p("Freq2", 0, 60, Fmt::Freq),
    p("Width2", 0, 39, OCT),
    p("Gain3", -12, 12, DB),
    p("Freq3", 0, 60, Fmt::Freq),
    p("Width3", 0, 39, OCT),
    e("EQ", ONOFF),
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectType {
    pub name: &'static str,
    /// Parameters 1.. (parameter 0 is the type itself).
    pub params: &'static [Param],
}

const fn t(name: &'static str, params: &'static [Param]) -> EffectType {
    EffectType { name, params }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    pub key: &'static str,
    pub label: &'static str,
    pub count_field: usize,
    /// Parameter slots reserved in the layout (after the count).
    pub size: usize,
    pub types: &'static [EffectType],
    /// For blocks without a type parameter.
    pub params: &'static [Param],
    pub enable_name: &'static str,
}

/// A parameter in a preset: either the block's type selector or a table entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldParam {
    Type(&'static Block),
    Param(&'static Param),
}

impl FieldParam {
    pub fn name(&self) -> &'static str {
        match self {
            FieldParam::Type(_) => "Type",
            FieldParam::Param(p) => p.name,
        }
    }
    pub fn range(&self) -> (i32, i32) {
        match self {
            FieldParam::Type(b) => (0, b.types.len() as i32 - 1),
            FieldParam::Param(p) => (p.lo, p.hi),
        }
    }
    /// The value list, if this is a selector (labels for lo..=hi).
    pub fn choices(&self) -> Option<Vec<String>> {
        match self {
            FieldParam::Type(b) => Some(b.types.iter().map(|t| t.name.to_string()).collect()),
            FieldParam::Param(p) if !p.choices.is_empty() => {
                Some((p.lo..=p.hi).map(|v| p.format(v)).collect())
            }
            _ => None,
        }
    }
    pub fn format(&self, v: i32) -> String {
        match self {
            FieldParam::Type(b) => b
                .types
                .get(v as usize)
                .map(|t| t.name.to_string())
                .unwrap_or_else(|| v.to_string()),
            FieldParam::Param(p) => p.format(v),
        }
    }
    pub fn clamp(&self, v: i32) -> i32 {
        let (lo, hi) = self.range();
        v.clamp(lo, hi)
    }
}

impl Block {
    pub fn first_field(&self) -> usize {
        self.count_field + 1
    }
    pub fn has_type(&self) -> bool {
        !self.types.is_empty()
    }
    pub fn effect_type(&self, p: &Preset) -> Option<&'static EffectType> {
        if !self.has_type() {
            return None;
        }
        let i = p.param_i24(self.first_field());
        usize::try_from(i).ok().and_then(|i| self.types.get(i))
    }
    /// Parameters, in slot order, for the effect type currently stored.
    pub fn fields(&'static self, p: &Preset) -> Vec<(usize, FieldParam)> {
        let list: Vec<FieldParam> = if self.has_type() {
            std::iter::once(FieldParam::Type(self))
                .chain(
                    self.effect_type(p)
                        .map(|t| t.params)
                        .unwrap_or(&[])
                        .iter()
                        .map(FieldParam::Param),
                )
                .collect()
        } else {
            self.params.iter().map(FieldParam::Param).collect()
        };
        list.into_iter()
            .enumerate()
            .map(|(i, fp)| (self.first_field() + i, fp))
            .collect()
    }
    /// Like [`fields`](Self::fields), but only the parameters this preset
    /// actually stores: older presets carry fewer than the firmware 4.02 table
    /// lists (e.g. Auto Wah with 11, without AutoOn), padding the rest with -1.
    /// (Ours; the original always shows the full list.)
    pub fn stored_fields(&'static self, p: &Preset) -> Vec<(usize, FieldParam)> {
        let all = self.fields(p);
        let count = p.param_i24(self.count_field);
        if count > 0 && (count as usize) < all.len() {
            all.into_iter().take(count as usize).collect()
        } else {
            all
        }
    }

    pub fn enable_field(&'static self, p: &Preset) -> Option<usize> {
        self.fields(p)
            .into_iter()
            .find(|(_, fp)| fp.name() == self.enable_name)
            .map(|(f, _)| f)
    }
    pub fn is_on(&'static self, p: &Preset) -> bool {
        self.enable_field(p)
            .map(|f| p.param_i24(f) != 0)
            .unwrap_or(false)
    }
    pub fn all_fields(&self) -> std::ops::Range<usize> {
        self.count_field..self.first_field() + self.size
    }
}

pub static BLOCKS: [Block; 8] = [
    Block {
        key: "comp",
        label: "Compressor",
        count_field: 3,
        size: 9,
        types: &[
            t("Advanced", COMP),
            t("Sustaining", COMP),
            t("Percussive", COMP),
        ],
        params: &[],
        enable_name: "Comp",
    },
    Block {
        key: "filter",
        label: "Filter",
        count_field: 13,
        size: 15,
        types: &[
            t("Resonance Filter", FILTER_RES),
            t("Touch Res Filter", FILTER_RES),
            t("Auto Res Filter", FILTER_RES),
            t("Wah wah", FILTER_WAH),
            t("Touch Wah", FILTER_WAH),
            t("Auto Wah", FILTER_WAH),
            t("Formant", FILTER_FORMANT),
            t("Touch Formant", FILTER_FORMANT),
            t("Auto Formant", FILTER_FORMANT),
        ],
        params: &[],
        enable_name: "Enable",
    },
    Block {
        key: "mod",
        label: "Mod",
        count_field: 29,
        size: 14,
        types: &[
            t("Classic Chorus", MOD_CHORUS),
            t("Advanced Chorus", MOD_CHORUS),
            t("Classic Flanger", MOD_CHORUS),
            t("Advanced Flanger", MOD_CHORUS),
            t("Vibrato", MOD_CHORUS),
            t("Vintage Phaser", MOD_PHASER),
            t("Smooth Phaser", MOD_PHASER),
            t("Tremolo", MOD_TREMOLO),
            t("Panner", MOD_TREMOLO),
            t("Vintage Univibe", MOD_UNIVIBE),
            t("Modern Univibe", MOD_UNIVIBE),
        ],
        params: &[],
        enable_name: "Enable",
    },
    Block {
        key: "pitch",
        label: "Pitch",
        count_field: 44,
        size: 19,
        types: &[
            t("Pitch Shifter", PITCH_SHIFTER),
            t("Octaver", PITCH_SHIFTER),
            t("Whammy", PITCH_SHIFTER),
            t("Detune", PITCH_SHIFTER),
            t("Intelligent", PITCH_INTELLIGENT),
            t("Doubler", PITCH_DOUBLER),
        ],
        params: &[],
        enable_name: "Enable",
    },
    Block {
        key: "reverb",
        label: "Reverb",
        count_field: 64,
        size: 15,
        types: &[
            t("Spring", REVERB),
            t("Hall", REVERB),
            t("Room", REVERB),
            t("Plate", REVERB),
        ],
        params: &[],
        enable_name: "Enable",
    },
    Block {
        key: "gate",
        label: "Noise gate",
        count_field: 80,
        size: 5,
        types: &[],
        params: GATE,
        enable_name: "NG",
    },
    Block {
        key: "delay",
        label: "Delay",
        count_field: 86,
        size: 27,
        types: &[
            t("Single Delay", DELAY),
            t("Tape Delay", DELAY),
            t("Reverse", DELAY),
            t("Lo-Fi", DELAY),
            t("Ping Pong", DELAY),
            t("Dynamic Delay", DELAY),
            t("Dual Delay", DELAY),
        ],
        params: &[],
        enable_name: "Enable",
    },
    Block {
        key: "eq",
        label: "EQ",
        count_field: 114,
        size: 10,
        types: &[],
        params: EQ,
        enable_name: "EQ",
    },
];

pub fn block(key: &str) -> &'static Block {
    BLOCKS
        .iter()
        .find(|b| b.key == key)
        .unwrap_or_else(|| panic!("unknown block {key}"))
}

/// The block owning field `index`, if any.
pub fn owner(index: usize) -> Option<&'static Block> {
    BLOCKS.iter().find(|b| b.all_fields().contains(&index))
}

/// Change a block's effect type. Other fields keep their slots, as on the
/// unit; values are clamped into the new parameters' ranges so the preset
/// stays valid. (Clamping is ours, not part of the original code.)
pub fn set_type(p: &mut Preset, key: &str, type_index: usize) {
    let b = block(key);
    if type_index >= b.types.len() {
        return;
    }
    p.set_param_i24(b.first_field(), type_index as i32);
    for (f, fp) in b.fields(p).into_iter().skip(1) {
        let v = p.param_i24(f);
        p.set_param_i24(f, fp.clamp(v));
    }
}

/// Readable label for a firmware parameter name ("Fbhcut" → "FB high cut").
/// Ours: the firmware's abbreviations stay the canonical names.
pub fn label(name: &str) -> String {
    let fixed = match name {
        "OutLev" => "Out level",
        "Resnce" => "Resonance",
        "Fbhcut" => "FB high cut",
        "Fblcut" => "FB low cut",
        "PreDly" => "Pre-delay",
        "RevLev" => "Reverb level",
        "Hi Col" => "High color",
        "Hi Lev" => "High level",
        "Lo Col" => "Low color",
        "Lo Lev" => "Low level",
        "Thres" => "Threshold",
        "Releas" => "Release",
        "Resp" => "Response",
        "Fb" => "Feedback",
        "FeedB" => "Feedback",
        "Diff" => "Diffusion",
        "Damp" => "Damping",
        "Sense" => "Sensitivity",
        "Fmax" => "Freq max",
        "HiCut" | "Hi Cut" | "Hicut" => "High cut",
        "PhaRev" => "Phase reverse",
        "Rev" => "Reverse",
        "FB Cut" => "FB cut",
        "Cho Dly" => "Chorus delay",
        "Dir" => "Direction",
        "AutoOn" => "Auto on",
        "BpMode" => "Bypass mode",
        "TapTime" | "Tap" => "Tap time",
        "Rel" => "Release",
        "NG" => "Gate",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_string();
    }
    // "Delay1" → "Delay 1", "Fb2" → "Feedback 2"
    let digits = name.trim_start_matches(|c: char| !c.is_ascii_digit());
    if !digits.is_empty() && digits.len() < name.len() && digits.chars().all(|c| c.is_ascii_digit())
    {
        let stem = &name[..name.len() - digits.len()];
        return format!("{} {digits}", label(stem));
    }
    name.to_string()
}

/// Fields that differ between two presets: (index, value in a, value in b).
pub fn diff_fields(a: &Preset, b: &Preset) -> Vec<(usize, i32, i32)> {
    (0..crate::preset::PARAM_COUNT)
        .filter_map(|i| {
            let (x, y) = (a.param_i24(i), b.param_i24(i));
            (x != y).then_some((i, x, y))
        })
        .collect()
}

/// JSON-ish dump of the tables, for cross-checking against the original
/// TypeScript (see tools/xcheck). Not part of the public API.
#[doc(hidden)]
pub fn dump_tables() -> String {
    fn esc(s: &str) -> String {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
    fn param(p: &Param) -> String {
        let ch: Vec<String> = p.choices.iter().map(|c| esc(c)).collect();
        format!(
            "[{},{},{},[{}],{}]",
            esc(p.name),
            p.lo,
            p.hi,
            ch.join(","),
            p.choices_base
        )
    }
    let blocks: Vec<String> = BLOCKS
        .iter()
        .map(|b| {
            let types: Vec<String> = b
                .types
                .iter()
                .map(|t| format!("[{},[{}]]", esc(t.name), t.params.iter().map(param).collect::<Vec<_>>().join(",")))
                .collect();
            format!(
                "{{\"key\":{},\"count\":{},\"size\":{},\"enable\":{},\"types\":[{}],\"params\":[{}]}}",
                esc(b.key),
                b.count_field,
                b.size,
                esc(b.enable_name),
                types.join(","),
                b.params.iter().map(param).collect::<Vec<_>>().join(",")
            )
        })
        .collect();
    format!("[{}]", blocks.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> Preset {
        Preset::blank("")
    }

    #[test]
    fn covers_the_payload() {
        assert_eq!(crate::preset::PARAM_COUNT, 165);
    }

    #[test]
    fn contiguous_blocks_fit_their_slots() {
        let mut expected = 3;
        for b in BLOCKS.iter() {
            assert_eq!(b.count_field, expected, "{}", b.key);
            expected = b.first_field() + b.size;
            for t in 0..b.types.len().max(1) {
                let mut d = blank();
                if b.has_type() {
                    d.set_param_i24(b.first_field(), t as i32);
                }
                assert!(b.fields(&d).len() <= b.size, "{} type {t}", b.key);
            }
        }
        assert_eq!(expected, 125);
    }

    #[test]
    fn decodes_the_eq_snippet_saved_by_vyzor() {
        let snippet = [
            10u8, 0, 0, 253, 255, 255, 12, 0, 0, 10, 0, 0, 2, 0, 0, 31, 0, 0, 18, 0, 0, 251, 255,
            255, 48, 0, 0, 26, 0, 0, 1, 0, 0,
        ];
        let mut d = blank();
        let off = crate::preset::PARAMS_OFFSET + 3 * 114;
        d.raw[off..off + snippet.len()].copy_from_slice(&snippet);
        let eq = block("eq");
        let get = |name: &str| {
            eq.fields(&d)
                .into_iter()
                .find(|(_, p)| p.name() == name)
                .map(|(f, _)| d.param_i24(f))
        };
        assert_eq!(get("Gain1"), Some(-3));
        assert_eq!(get("EQ"), Some(1));
        assert_eq!(eq.enable_field(&d), Some(124));
    }

    #[test]
    fn follows_the_effect_type() {
        let filt = block("filter");
        let mut wah = blank();
        wah.set_param_i24(filt.first_field(), 5);
        assert_eq!(filt.effect_type(&wah).unwrap().name, "Auto Wah");
        assert_eq!(filt.fields(&wah).last().unwrap().1.name(), "AutoOn");
        assert_eq!(filt.enable_field(&wah), Some(filt.first_field() + 1));
        assert_eq!(freq(34), "1.02 kHz");
        assert_eq!(freq(18), "160 Hz");
    }

    #[test]
    fn diffs_fields() {
        let a = blank();
        let mut b = blank();
        b.set_param_i24(3, 9);
        b.set_param_i24(100, -2);
        assert_eq!(diff_fields(&a, &b), vec![(3, 0, 9), (100, 0, -2)]);
    }

    #[test]
    fn intervals_use_their_base() {
        let v = interval("Voice1");
        assert_eq!(v.format(-12), "-13");
        assert_eq!(v.format(0), "Unison");
        assert_eq!(v.format(7), "Oct1");
    }

    #[test]
    fn set_type_clamps_into_new_ranges() {
        let mut d = blank();
        let f = block("filter");
        // Resonance filter: Fmax (field first+7) has lo 34; a 0 left over must be pulled up.
        set_type(&mut d, "filter", 0);
        let fmax = f
            .fields(&d)
            .into_iter()
            .find(|(_, p)| p.name() == "Fmax")
            .unwrap()
            .0;
        assert_eq!(d.param_i24(fmax), 34);
    }
}
