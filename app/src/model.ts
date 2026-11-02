// Parameter model for the G-System preset. Each param names a 3-byte int24
// field in the 516-byte dump (`index`, 0-based after the 21-byte name).
// Indices were derived from the 100 factory presets and checked against every
// value Vyzor shows for "Scotty's Blues" (PROTOCOL.md, crates/gsystem-protocol
// tests). `index: null` = not mapped yet; Filter and Modulation parameters
// move with the effect type and are still open.

export type Unit = "dB" | "ms" | "s" | "%" | "Hz" | "kHz" | "¢" | "BPM" | "";

export interface Param {
  id: string;
  label: string;
  index: number | null;
  min: number;
  max: number;
  unit: Unit;
  /** displayed = raw * scale (e.g. 0.1 for tenths of a dB) */
  scale?: number;
  /** enum choices; when set the control is a select, raw value = option index */
  options?: string[];
  /** special encodings: sixth-octave frequency index, EQ width in 0.1-octave steps */
  fmt?: "freq" | "width";
}

export interface Block {
  id: string;
  name: string; // strip label, uppercase
  title: string; // card title
  /** index of the int24 that holds on/off (0 = off), or null if unmapped */
  enabledIndex: number | null;
  /** index of the int24 that holds the type/model, or null */
  typeIndex: number | null;
  types: string[];
  params: Param[];
  /** tempo-division pills shown under the params */
  tempo?: boolean;
}

const DB = (id: string, label: string, min = -100, max = 0): Param => ({ id, label, index: null, min, max, unit: "dB" });
const PCT = (id: string, label: string): Param => ({ id, label, index: null, min: 0, max: 100, unit: "%" });

// Signal chain in audio order, from the Vyzor "All" view.
export const BLOCKS: Block[] = [
  {
    id: "filter", name: "Filter", title: "Filter", enabledIndex: 15, typeIndex: 14,
    types: ["Resonance Filter", "Touch Res Filter", "Auto Res Filter", "Wah Wah", "Touch Wah", "Auto Wah", "Formant", "Touch Formant", "Auto Formant"],
    params: [
      { id: "speed", label: "Speed", index: null, min: 50, max: 20000, unit: "Hz", scale: 0.001 },
      { id: "shape", label: "Shape", index: null, min: 0, max: 1, unit: "", options: ["Triangle", "Sinewave"] },
      { id: "freqmax", label: "Freq max", index: null, min: 1000, max: 10000, unit: "Hz" },
      PCT("reso", "Resonance"),
      DB("out", "Out level"),
    ],
    tempo: true,
  },
  {
    id: "comp", name: "Comp", title: "Compressor", enabledIndex: 5, typeIndex: 4,
    types: ["Advanced", "Sustaining", "Percussive"],
    params: [
      { id: "drive", label: "Drive", index: 11, min: 1, max: 20, unit: "" },
      { id: "response", label: "Response", index: 12, min: 0, max: 10, unit: "" },
      { ...DB("gain", "Gain", -100, 12), index: 10 },
    ],
  },
  { id: "loop1", name: "Loop 1", title: "Loop 1", enabledIndex: null, typeIndex: null, types: [], params: [] },
  { id: "loop2", name: "Loop 2", title: "Loop 2", enabledIndex: null, typeIndex: null, types: [], params: [] },
  { id: "loop3", name: "Loop 3", title: "Loop 3", enabledIndex: null, typeIndex: null, types: [], params: [] },
  { id: "loop4", name: "Loop 4", title: "Loop 4", enabledIndex: null, typeIndex: null, types: [], params: [] },
  { id: "insert", name: "Insert", title: "Insert", enabledIndex: null, typeIndex: null, types: [], params: [] },
  {
    id: "mod", name: "Mod", title: "Modulation", enabledIndex: 31, typeIndex: 30,
    types: ["Classic Chorus", "Advanced Chorus", "Classic Flanger", "Advanced Flanger", "Vibrato", "Vintage Phaser", "Smooth Phaser", "Tremolo", "Panner", "Vintage UniVibe", "Modern UniVibe"],
    params: [
      { id: "speed", label: "Speed", index: null, min: 50, max: 20000, unit: "Hz", scale: 0.001 },
      PCT("depth", "Depth"),
      { id: "hicut", label: "High cut", index: null, min: 20, max: 20000, unit: "Hz" },
      PCT("mix", "Mix"),
      DB("out", "Out level"),
    ],
    tempo: true,
  },
  {
    id: "pitch", name: "Pitch", title: "Pitch", enabledIndex: 46, typeIndex: 45,
    types: ["Pitch Shifter", "Octaver", "Whammy", "Detune", "Intelligent", "Doubler"],
    params: [
      { id: "v1", label: "Voice 1", index: null, min: -1200, max: 1200, unit: "¢" },
      { id: "v2", label: "Voice 2", index: null, min: -1200, max: 1200, unit: "¢" },
      { id: "d1", label: "Delay 1", index: 51, min: 0, max: 350, unit: "ms" },
      { id: "d2", label: "Delay 2", index: 52, min: 0, max: 350, unit: "ms" },
      { ...PCT("mix", "Mix"), index: 57 },
      DB("out", "Out level"),
    ],
  },
  {
    id: "delay", name: "Delay", title: "Delay", enabledIndex: 88, typeIndex: 87,
    types: ["Single Delay", "Tape Delay", "Reverse Delay", "Lo-Fi Delay", "Ping Pong", "Dynamic Delay", "Dual Delay"],
    params: [
      { id: "time", label: "Time", index: 89, min: 0, max: 1800, unit: "ms" },
      { id: "fb", label: "Feedback", index: 96, min: 0, max: 120, unit: "%" },
      { id: "lo", label: "FB low cut", index: 103, min: 0, max: 60, unit: "Hz", fmt: "freq" },
      { id: "hi", label: "FB high cut", index: 102, min: 0, max: 60, unit: "Hz", fmt: "freq" },
      { ...PCT("mix", "Mix"), index: 106 },
      DB("out", "Out level"),
    ],
    tempo: true,
  },
  {
    id: "reverb", name: "Reverb", title: "Reverb", enabledIndex: 66, typeIndex: 65,
    types: ["Spring", "Hall", "Room", "Plate"],
    params: [
      { id: "decay", label: "Decay", index: 67, min: 1, max: 200, unit: "s", scale: 0.1 },
      { id: "pre", label: "Pre-delay", index: 68, min: 0, max: 100, unit: "ms" },
      { id: "shape", label: "Shape", index: 69, min: 0, max: 2, unit: "", options: ["Round", "Curved", "Square"] },
      { id: "size", label: "Size", index: 70, min: 0, max: 7, unit: "", options: ["Box", "Tiny", "Small", "Medium", "Large", "Xtra Large", "Grand", "Huge"] },
      { id: "hicol", label: "High color", index: 71, min: 0, max: 6, unit: "", options: ["Wool", "Warm", "Real", "Clear", "Bright", "Crisp", "Glass"] },
      { id: "hilev", label: "High level", index: 72, min: -25, max: 25, unit: "" },
      { id: "locol", label: "Low color", index: 73, min: 0, max: 6, unit: "", options: ["Thick", "Round", "Real", "Light", "Tight", "Thin", "NoBass"] },
      { id: "lolev", label: "Low level", index: 74, min: -25, max: 25, unit: "" },
      { ...DB("early", "Early"), index: 75 },
      { ...DB("rev", "Reverb"), index: 76 },
      { id: "diff", label: "Diffusion", index: 77, min: -25, max: 25, unit: "" },
      { ...PCT("mix", "Mix"), index: 78 },
      { ...DB("out", "Out level"), index: 79 },
    ],
  },
  {
    id: "eq", name: "EQ", title: "Equalizer", enabledIndex: 124, typeIndex: null, types: [],
    params: [1, 2, 3].flatMap((n) => [
      { id: `g${n}`, label: `Gain ${n}`, index: 112 + 3 * n, min: -12, max: 12, unit: "dB" as Unit },
      { id: `f${n}`, label: `Freq ${n}`, index: 113 + 3 * n, min: 0, max: 60, unit: "Hz" as Unit, fmt: "freq" as const },
      { id: `w${n}`, label: `Width ${n}`, index: 114 + 3 * n, min: 0, max: 39, unit: "" as Unit, fmt: "width" as const },
    ]),
  },
  {
    id: "boost", name: "Boost", title: "Boost", enabledIndex: null, typeIndex: null, types: [],
    params: [{ id: "level", label: "Level", index: null, min: 0, max: 10, unit: "dB" }],
  },
];

// Per-preset settings that aren't an effect block.
export const PRESET_MISC = {
  routing: { id: "routing", label: "Routing", index: null as number | null, options: ["Serial", "Semi-parallel", "Parallel"] },
  gate: [
    { id: "gon", label: "Gate", index: 81, min: 0, max: 1, unit: "" as Unit, options: ["Off", "On"] },
    { id: "gmode", label: "Mode", index: 82, min: 0, max: 1, unit: "" as Unit, options: ["Hard", "Soft"] },
    { id: "gthr", label: "Threshold", index: 83, min: -60, max: 0, unit: "dB" as Unit },
    { id: "gdamp", label: "Max damping", index: 84, min: 0, max: 90, unit: "dB" as Unit },
    { id: "grel", label: "Release", index: 85, min: 3, max: 200, unit: "" as Unit },
  ] as Param[],
  tempo: [
    { id: "bpm", label: "BPM", index: null as number | null, min: 20, max: 300, unit: "BPM" as Unit },
  ] as Param[],
};

export const TEMPO_DIVISIONS = ["Ignore", "2", "1", "1/2", "1/4", "1/8", "1/16", "1/32", "·", "3"];

export function formatValue(p: Param, raw: number): string {
  if (p.options) return p.options[raw] ?? String(raw);
  if (p.fmt === "freq") {
    const hz = 20 * 2 ** (raw / 6);
    return hz >= 1000 ? `${(hz / 1000).toFixed(2)} kHz` : `${Math.round(hz)} Hz`;
  }
  if (p.fmt === "width") return `${((raw + 1) / 10).toFixed(1)} oct`;
  const v = raw * (p.scale ?? 1);
  if (p.unit === "Hz" && v >= 1000) return `${(v / 1000).toFixed(2)} kHz`;
  if (p.unit === "Hz" && v < 1) return `${Math.round(v * 1000)} mHz`;
  const s = Number.isInteger(v) ? String(v) : v.toFixed(p.scale && p.scale < 1 ? 1 : 0);
  if (p.unit === "dB" && v > 0) return `+${s} dB`;
  if (p.unit === "¢" && v > 0) return `+${s} ¢`;
  return p.unit ? `${s} ${p.unit}` : s;
}
