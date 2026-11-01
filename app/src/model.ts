// Parameter model for the G-System preset. Each param names a 3-byte int24 slot
// in the 516-byte dump (`index`, 0-based after the 21-byte name header).
// `index: null` means the slot has not been mapped yet — the UI shows it
// disabled with an "unmapped" tag. Fill these in as PROTOCOL.md's field map
// is confirmed; nothing else in the app needs to change.

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
    id: "filter", name: "Filter", title: "Filter", enabledIndex: null, typeIndex: null,
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
    id: "comp", name: "Comp", title: "Compressor", enabledIndex: null, typeIndex: null,
    types: ["Advanced", "Sustaining", "Percussive"],
    params: [
      { id: "drive", label: "Drive", index: null, min: 1, max: 20, unit: "" },
      { id: "response", label: "Response", index: null, min: 0, max: 10, unit: "" },
      DB("gain", "Gain", -100, 12),
    ],
  },
  { id: "loop1", name: "Loop 1", title: "Loop 1", enabledIndex: null, typeIndex: null, types: [], params: [] },
  { id: "loop2", name: "Loop 2", title: "Loop 2", enabledIndex: null, typeIndex: null, types: [], params: [] },
  { id: "loop3", name: "Loop 3", title: "Loop 3", enabledIndex: null, typeIndex: null, types: [], params: [] },
  { id: "loop4", name: "Loop 4", title: "Loop 4", enabledIndex: null, typeIndex: null, types: [], params: [] },
  { id: "insert", name: "Insert", title: "Insert", enabledIndex: null, typeIndex: null, types: [], params: [] },
  {
    id: "mod", name: "Mod", title: "Modulation", enabledIndex: null, typeIndex: null,
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
    id: "pitch", name: "Pitch", title: "Pitch", enabledIndex: null, typeIndex: null,
    types: ["Pitch Shifter", "Octaver", "Whammy", "Detune", "Intelligent", "Doubler"],
    params: [
      { id: "v1", label: "Voice 1", index: null, min: -1200, max: 1200, unit: "¢" },
      { id: "v2", label: "Voice 2", index: null, min: -1200, max: 1200, unit: "¢" },
      { id: "d1", label: "Delay 1", index: null, min: 0, max: 350, unit: "ms" },
      { id: "d2", label: "Delay 2", index: null, min: 0, max: 350, unit: "ms" },
      PCT("mix", "Mix"),
      DB("out", "Out level"),
    ],
  },
  {
    id: "delay", name: "Delay", title: "Delay", enabledIndex: null, typeIndex: null,
    types: ["Single Delay", "Tape Delay", "Reverse Delay", "Lo-Fi Delay", "Ping Pong", "Dynamic Delay", "Dual Delay"],
    params: [
      { id: "time", label: "Time", index: null, min: 0, max: 1800, unit: "ms" },
      { id: "fb", label: "Feedback", index: null, min: 0, max: 120, unit: "%" },
      { id: "lo", label: "FB low cut", index: null, min: 20, max: 20000, unit: "Hz" },
      { id: "hi", label: "FB high cut", index: null, min: 20, max: 20000, unit: "Hz" },
      PCT("mix", "Mix"),
      DB("out", "Out level"),
    ],
    tempo: true,
  },
  {
    id: "reverb", name: "Reverb", title: "Reverb", enabledIndex: null, typeIndex: null,
    types: ["Spring", "Hall", "Room", "Plate"],
    params: [
      { id: "decay", label: "Decay", index: null, min: 1, max: 200, unit: "s", scale: 0.1 },
      { id: "pre", label: "Pre-delay", index: null, min: 0, max: 100, unit: "ms" },
      DB("early", "Early"),
      DB("rev", "Reverb"),
      { id: "diff", label: "Diffusion", index: null, min: -25, max: 25, unit: "" },
      PCT("mix", "Mix"),
      DB("out", "Out level"),
    ],
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
    { id: "gmode", label: "Mode", index: null as number | null, min: 0, max: 1, unit: "" as Unit, options: ["Hard", "Soft"] },
    { id: "gthr", label: "Threshold", index: null as number | null, min: -60, max: 0, unit: "dB" as Unit },
    { id: "gdamp", label: "Max damping", index: null as number | null, min: 0, max: 90, unit: "dB" as Unit },
    { id: "grel", label: "Release", index: null as number | null, min: 3, max: 200, unit: "" as Unit },
  ] as Param[],
  tempo: [
    { id: "bpm", label: "BPM", index: null as number | null, min: 20, max: 300, unit: "BPM" as Unit },
  ] as Param[],
};

export const TEMPO_DIVISIONS = ["Ignore", "2", "1", "1/2", "1/4", "1/8", "1/16", "1/32", "·", "3"];

export function formatValue(p: Param, raw: number): string {
  if (p.options) return p.options[raw] ?? String(raw);
  const v = raw * (p.scale ?? 1);
  if (p.unit === "Hz" && v >= 1000) return `${(v / 1000).toFixed(2)} kHz`;
  if (p.unit === "Hz" && v < 1) return `${Math.round(v * 1000)} mHz`;
  const s = Number.isInteger(v) ? String(v) : v.toFixed(p.scale && p.scale < 1 ? 1 : 0);
  if (p.unit === "dB" && v > 0) return `+${s} dB`;
  if (p.unit === "¢" && v > 0) return `+${s} ¢`;
  return p.unit ? `${s} ${p.unit}` : s;
}
