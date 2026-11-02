import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";

type Slot = { index: number; label: string; name: string | null };
type BankView = { path: string | null; occupied: number; dirty: number[]; slots: Slot[] };
// Decoded by the Rust core (gsystem_protocol::params / levels / tempo).
type Row = { field: number; label: string; value: number; display: string; lo: number; hi: number; choices: string[] | null };
type BlockView = { key: string; label: string; on: boolean; types: string[]; type_index: number | null; rows: Row[] };
type ChainView = { key: string; name: string; on: boolean; summary: string };
type Detail = {
  index: number; label: string; name: string; chain: ChainView[]; blocks: BlockView[];
  routing: number; routings: string[]; loops: boolean[]; insert: boolean; boost: boolean; boost_level: number; bpm: number | null;
  modifiers: ModView[]; sources: string[]; nova_preset: number;
};
type ModView = {
  slot: number; key: string; label: string; source: number; min: number; mid: number; max: number; display: string;
  field: number | null; values: [string, string, string] | null; scale: [string, string] | null;
};
type GlobalsView = {
  groups: { title: string; params: { name: string; help: string; value: number | null; display: string }[] }[];
  switches: { number: number; display: string; fixed: boolean }[];
};
type Edit =
  | { kind: "field"; field: number; value: number }
  | { kind: "type"; block: string; value: number }
  | { kind: "toggle"; key: string }
  | { kind: "routing"; value: number }
  | { kind: "boost_level"; value: number }
  | { kind: "bpm"; value: number }
  | { kind: "modifier"; slot: number; source: number; min: number; mid: number; max: number }
  | { kind: "nova"; value: number };
type Ports = { inputs: string[]; outputs: string[] };
type View = "connect" | "banks" | "preset" | "global";

// ---------------------------------------------------------------- state
const app = document.getElementById("app")!;
let view: View = "connect";
let bank: BankView = { path: null, occupied: 0, dirty: [], slots: [] };
let selected: number | null = null;
let detail: Detail | null = null;
let half: 0 | 1 = 0; // banks 00–19 / 20–39
let dragFrom: number | null = null;
let ports: Ports = { inputs: [], outputs: [] };
let portIn = "";
let portOut = "";
let unit: { connected: boolean; progress?: string; version?: string } = { connected: false };
let focusBlock: string | null = null;
let globals: GlobalsView | null = null;
let globalsBusy = false;
let live = false;
let pedalSlot: number | null = null; // modifier shown in the pedal panel
// Light / dark: follows the OS until the user picks one (remembered on this machine).
type Mode = "auto" | "light" | "dark";
let mode: Mode = "auto";
try {
  const m = localStorage.getItem("mode") as Mode | null;
  if (m === "light" || m === "dark") mode = m;
} catch { /* storage unavailable: follow the OS */ }
function applyMode() {
  if (mode === "auto") delete document.documentElement.dataset.mode;
  else document.documentElement.dataset.mode = mode;
}
function toggleMode() {
  const dark = mode === "dark" || (mode === "auto" && matchMedia("(prefers-color-scheme: dark)").matches);
  mode = dark ? "light" : "dark";
  try { localStorage.setItem("mode", mode); } catch { /* ignore */ }
  applyMode();
}
applyMode();
const STORE_HINT = "Nothing is stored yet: press STORE on the G-System to keep it.";

async function audition(index: number) {
  try {
    const name = await invoke<string>("midi_audition", { index, input: portIn, output: portOut });
    setStatus(`Loaded “${name}” into the unit. ${STORE_HINT}`);
  } catch (e) {
    setStatus(String(e), true);
  }
}
async function setLive(on: boolean) {
  try {
    live = await invoke<boolean>("midi_live", { on, index: selected, input: portIn, output: portOut });
    setStatus(live ? `Live: edits go straight to the unit. ${STORE_HINT}` : "Live off: edits change the file only.");
  } catch (e) {
    live = false;
    setStatus(String(e), true);
  }
  render();
}
let status = "";
let statusErr = false;

const dirtySet = () => new Set(bank.dirty);
const fileName = () => bank.path?.split(/[\\/]/).pop() ?? null;

function setStatus(s: string, err = false) {
  status = s;
  statusErr = err;
  document.querySelectorAll<HTMLElement>(".status").forEach((el) => {
    el.textContent = s;
    el.classList.toggle("err", err);
  });
}
function esc(s: string) {
  return s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
}
function go(v: View) {
  view = v;
  render();
}

async function refresh(v?: BankView) {
  bank = v ?? (await invoke<BankView>("get_bank"));
  if (selected != null && bank.slots[selected]?.name != null) {
    detail = await invoke<Detail>("preset_view", { index: selected });
  } else {
    detail = null;
  }
  render();
}

// ---------------------------------------------------------------- file ops
async function doOpen() {
  const path = await open({
    multiple: false,
    filters: [
      { name: "G-System banks", extensions: ["SQS", "sqs", "syx", "Prs", "prs", "Ebf", "ebf"] },
      { name: "All files", extensions: ["*"] },
    ],
  });
  if (!path) return;
  try {
    selected = null;
    await refresh(await invoke<BankView>("open_bank", { path }));
    if (view === "connect") go("banks");
    setStatus(`Loaded ${bank.occupied} presets`);
  } catch (e) {
    setStatus(String(e), true);
  }
}
async function doSave() {
  const path = await save({ defaultPath: (fileName() ?? "gsystem-bank").replace(/\.\w+$/, "") + ".syx", filters: [{ name: "SysEx", extensions: ["syx"] }] });
  if (!path) return;
  try {
    await invoke("save_syx", { path });
    setStatus(`Saved ${bank.occupied} presets`);
  } catch (e) {
    setStatus(String(e), true);
  }
}

// ---------------------------------------------------------------- MIDI
async function loadPorts() {
  try {
    ports = await invoke<Ports>("midi_ports");
    portIn ||= ports.inputs.find((p) => /g-system/i.test(p)) ?? ports.inputs.find((p) => /um-one|midi/i.test(p)) ?? ports.inputs[0] ?? "";
    portOut ||= ports.outputs.find((p) => /g-system/i.test(p)) ?? ports.outputs.find((p) => /um-one|midi/i.test(p)) ?? ports.outputs[0] ?? "";
  } catch (e) {
    setStatus(String(e), true);
  }
  render();
}
async function identify(): Promise<boolean> {
  try {
    const v = await invoke<string>("midi_identify", { input: portIn, output: portOut });
    unit = { connected: true, version: v };
    return true;
  } catch (e) {
    unit = { connected: false };
    setStatus(String(e), true);
    return false;
  }
}
async function midiBackup(from: number, to: number) {
  const n = to - from + 1;
  unit.progress = `Reading ${n} slot${n === 1 ? "" : "s"} (about ${Math.ceil((n * 0.8) / 60)} min)…`;
  render();
  try {
    const got = await invoke<number[]>("midi_backup", { input: portIn, output: portOut, from, to });
    unit = { ...unit, connected: true, progress: undefined };
    await refresh();
    setStatus(got.length ? `Received ${got.length} presets` : "No response from unit", !got.length);
  } catch (e) {
    unit = { connected: false };
    setStatus(String(e), true);
  }
  render();
}
const WRITE_DISABLED = "Writing to the unit is disabled for now: the store sequence can freeze the G-System (see PROTOCOL.md). Save a .syx instead.";
async function midiSend(from: number, to: number) {
  try {
    const n = await invoke<number>("midi_send", { output: portOut, from, to });
    await refresh();
    setStatus(`Sent ${n} preset(s)`);
  } catch (e) {
    setStatus(String(e), true);
  }
}
async function syncDirty() {
  const d = bank.dirty;
  if (!d.length) return;
  try {
    let n = 0;
    for (const i of d) n += await invoke<number>("midi_send", { output: portOut, from: i, to: i });
    await refresh();
    setStatus(`Synced ${n} preset(s)`);
  } catch (e) {
    setStatus(String(e), true);
  }
}

// ---------------------------------------------------------------- shared chrome
function header(): string {
  const tabs = (["banks", "preset", "global"] as View[])
    .map((v) => `<button class="${view === v ? "on" : ""}" data-view="${v}" ${v === "preset" && selected == null ? "disabled" : ""}>${v[0].toUpperCase() + v.slice(1)}</button>`)
    .join("");
  return `<header class="top">
    <div class="brand">${logo()}<span>G-System Editor</span></div>
    <nav class="seg" aria-label="Mode">${tabs}</nav>
    <div class="file">
      <span class="name">${fileName() ? esc(fileName()!) : "No bank loaded"}</span>
      ${bank.dirty.length ? `<span class="pill">${bank.dirty.length} edited</span>` : ""}
    </div>
    <button class="btn icon" id="mode" title="Light or dark" aria-label="Switch light or dark"><svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.5" fill="none" stroke="currentColor" stroke-width="1.5"/><path d="M8 1.5a6.5 6.5 0 0 1 0 13z" fill="currentColor"/></svg></button>
    <div style="display:flex;gap:8px">
      <button class="btn" id="open">Open…</button>
      <button class="btn" id="save" ${bank.occupied ? "" : "disabled"}>Save .syx</button>
    </div>
    <div class="unit"><span class="led ${unit.connected ? "on" : ""}"></span><span>${unit.connected ? `G-System · ${esc(portIn)}` : "No unit"}</span></div>
  </header>`;
}
function logo() {
  // The app icon's "Signal G": a ring with a lit LED.
  return `<svg width="22" height="22" viewBox="0 0 24 24" fill="none" aria-hidden="true"><path d="M19 8.5A8 8 0 1 0 20 12h-7" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"/><circle cx="19.5" cy="5" r="2" fill="#ff3b2a"/></svg>`;
}
const pad2 = (n: number) => String(n).padStart(2, "0");
const STATUS_LEDS: [string, string][] = [["comp", "Comp"], ["filter", "Filter"], ["mod", "Mod"], ["pitch", "Pitch"], ["delay", "Delay"], ["reverb", "Reverb"], ["eq", "EQ"], ["gate", "Gate"], ["boost", "Boost"]];
// The unit's 2-digit LED display, drawn as real 7-segment digits.
const SEGMENTS: Record<string, string> = { "0": "abcdef", "1": "bc", "2": "abged", "3": "abgcd", "4": "fgbc", "5": "afgcd", "6": "afgedc", "7": "abc", "8": "abcdefg", "9": "abcdfg", "-": "g" };
const SEG_SHAPES: [string, string][] = [
  ["a", "6,2 34,2 30,7 10,7"], ["b", "35,4 35,30 31,27 31,8"], ["c", "35,34 35,60 31,56 31,37"], ["d", "6,62 34,62 30,57 10,57"],
  ["e", "4,34 4,60 8,56 8,37"], ["f", "4,4 4,30 8,27 8,8"], ["g", "7,32 11,29 29,29 33,32 29,35 11,35"],
];
function sevenSeg(text: string): string {
  const digits = [...text].map((ch, i) => {
    const lit = SEGMENTS[ch] ?? "";
    return SEG_SHAPES.map(([k, pts]) => `<polygon class="${lit.includes(k) ? "lit" : ""}" points="${pts}" transform="translate(${4 + i * 44},0) skewX(-6)"/>`).join("");
  }).join("");
  return `<svg class="seg7" viewBox="0 0 92 64" role="img" aria-label="${esc(text)}">${digits}</svg>`;
}
/** The unit's front panel: LED bank display, LCD, the five preset footswitches
 * with their LEDs and scribble strips, and the effect status LEDs. */
function strip(): string {
  const s = selected != null ? bank.slots[selected] : null;
  const bankNo = selected != null ? Math.floor(selected / 5) : null;
  const chain = new Map((detail?.chain ?? []).map((c) => [c.key, c]));
  const fs = [0, 1, 2, 3, 4].map((p) => {
    const i = bankNo != null ? bankNo * 5 + p : null;
    const nm = i != null ? bank.slots[i]?.name : null;
    const lit = selected != null && selected % 5 === p;
    return `<button class="fs ${lit ? "on" : ""}" data-fs="${p}" ${bankNo == null ? "disabled" : ""} aria-label="Switch ${p + 1}${nm ? `: ${esc(nm)}` : ""}">
      <i class="led"></i><span class="dome"></span><span class="scr">${nm ? esc(nm) : "—"}</span></button>`;
  }).join("");
  const leds = STATUS_LEDS.map(([k, label]) => `<span class="sl ${chain.get(k)?.on ? "on" : ""}"><i></i>${label}</span>`).join("");
  const routing = detail ? detail.routings[detail.routing] : "";
  return `<section class="frontpanel" aria-label="Unit">
    <div class="display">
      <div class="ledbank" aria-label="Bank ${bankNo != null ? pad2(bankNo) : "none"}">
        ${sevenSeg(bankNo != null ? pad2(bankNo) : "--")}<span class="sub">${selected != null ? (selected % 5) + 1 : ""}</span>
        <span class="nudges"><button class="nudge" data-bank="-1" aria-label="Previous bank">▲</button><button class="nudge" data-bank="1" aria-label="Next bank">▼</button></span>
      </div>
      <div class="lcd">
        <div class="l1"><span class="nm">${s?.name ? esc(s.name) : "NO PRESET"}</span><span class="bpm">${detail?.bpm != null ? `${detail.bpm} BPM` : ""}</span></div>
        <div class="l2"><span>${s ? `${s.label}${fileName() ? ` · ${esc(fileName()!)}` : ""}` : "SELECT A SLOT"}</span><span>${esc(routing)}</span></div>
      </div>
    </div>
    <div class="switchbank">${fs}</div>
    <div class="statusleds">${leds}</div>
  </section>`;
}

// ---------------------------------------------------------------- banks view
function banksView(): string {
  const rows: string[] = [];
  const dirty = dirtySet();
  const start = half * 20;
  for (let b = start; b < start + 20; b++) {
    const cells = [0, 1, 2, 3, 4].map((p) => {
      const i = b * 5 + p;
      const s = bank.slots[i];
      const name = s?.name;
      return `<div class="cell ${selected === i ? "sel" : ""}" data-i="${i}" draggable="${name != null}">
        <span class="lbl">${s?.label ?? ""}</span>
        <span class="nm ${name == null ? "empty" : ""}">${name == null ? "—" : esc(name)}</span>
        ${dirty.has(i) ? `<span class="dot" title="Changed since last sync"></span>` : ""}
      </div>`;
    }).join("");
    rows.push(`<div class="grow"><div class="bankno">${String(b).padStart(2, "0")}</div>${cells}</div>`);
  }
  const defined = bank.occupied;
  return `<main class="body">
    <section class="center">
      <div class="toolbar">
        <div class="seg"><button class="${half === 0 ? "on" : ""}" data-half="0">Banks 00–19</button><button class="${half === 1 ? "on" : ""}" data-half="1">Banks 20–39</button></div>
        <span class="dim">${defined} of 200 defined</span>
        <div class="right">
          <button class="btn sm" id="getall" ${unit.connected ? "" : "disabled"}>Read all from unit</button>
          <button class="btn sm amber" id="syncdirty" disabled title="${WRITE_DISABLED}">Sync ${bank.dirty.length || ""} changed →</button>
        </div>
      </div>
      <div class="gridwrap">
        <div class="grow head"><div>Bank</div><div>Switch 1</div><div>Switch 2</div><div>Switch 3</div><div>Switch 4</div><div>Switch 5</div></div>
        ${rows.join("")}
      </div>
      <div class="legend"><span>Drag to swap · ⌥ drag to copy · ⇧ drag to insert · double-click to rename</span><span><span class="dot"></span>changed since last sync</span><span class="status ${statusErr ? "err" : ""}">${esc(status)}</span></div>
    </section>
    ${inspector()}
  </main>`;
}
function inspector(): string {
  const s = selected != null ? bank.slots[selected] : null;
  if (!s || s.name == null || !detail) {
    return `<aside class="inspector"><div class="eyebrow">Preset</div><div class="note">Select a slot to see what's in it. Double-click a name to rename, drag to move.</div>${unitPanel()}</aside>`;
  }
  const mini = detail.chain.filter((c) => ["filter", "comp", "mod", "pitch", "delay", "reverb", "eq", "gate", "boost"].includes(c.key))
    .map((c) => `<div class="${c.on ? "on" : ""}"><div class="k">${esc(c.name)}</div><div class="v">${esc(c.summary)}</div></div>`)
    .join("");
  const dirty = dirtySet().has(selected!);
  return `<aside class="inspector">
    <div>
      <div class="eyebrow mono" style="margin-bottom:6px">${s.label} · USER BANK</div>
      <label for="pname" style="position:absolute;left:-9999px">Preset name</label>
      <input id="pname" class="pname" value="${esc(s.name)}" maxlength="20">
      <div style="display:flex;justify-content:space-between;font-size:11px;margin-top:6px" class="dim"><span>${s.name.length} / 20 characters</span>${dirty ? `<span style="color:var(--edited)">Edited</span>` : ""}</div>
    </div>
    <div style="display:flex;flex-direction:column;gap:6px">
      <div class="eyebrow">Signal chain</div>
      <div class="chain-mini">${mini}</div>
    </div>
    <div style="display:flex;flex-direction:column;gap:8px">
      <button class="btn primary" id="edit" style="height:38px;border-radius:8px">Edit preset</button>
      <div class="actions2">
        <button class="btn" id="readone" ${unit.connected ? "" : "disabled"}>Re-read from unit</button>
        <button class="btn" id="audition" ${unit.connected ? "" : "disabled"} title="Load into the unit's edit buffer to hear it. Nothing is stored.">Audition on unit</button>
        <button class="btn" id="copyto">Copy to…</button>
        <button class="btn" id="clear" style="color:var(--text-2)">Clear slot</button>
      </div>
    </div>
    ${unitPanel()}
  </aside>`;
}
function unitPanel(): string {
  if (!unit.connected) {
    return `<div class="note" style="margin-top:auto"><b>Unit vs. file</b>No G-System connected. Edits stay in this file until you <a href="#" id="toconnect" class="link">connect</a> and sync.</div>`;
  }
  const d = bank.dirty;
  const list = d.slice(0, 6).map((i) => bank.slots[i].label).join(", ") + (d.length > 6 ? ", …" : "");
  return `<div class="note" style="margin-top:auto"><b>Unit vs. file</b>${d.length ? `${d.length} preset(s) changed here (${list}). Writing to the unit is disabled for now, so save a .syx to keep them.` : "Matches what was read from the unit."}</div>`;
}

// ---------------------------------------------------------------- preset view
// ---------------------------------------------------------------- signal flow
// The unit's path (TC's block diagram and the configuration guide): a digital
// section before the amp, four analog true-bypass loops, the insert (usually the
// amp's preamp), then a digital section whose middle depends on the routing.
// Gate first and EQ last are our placement; TC's diagram doesn't show them.
const NW = 112, NH = 62, FW = 1316;
function flowNode(d: Detail, key: string, x: number, y: number): string {
  const c = d.chain.find((k) => k.key === key);
  const on = c?.on ?? false;
  const isSwitch = key.startsWith("loop") || key === "insert";
  const label = key === "insert" ? "Insert" : c?.name ?? key;
  const sub = isSwitch ? (on ? "Send → Return" : "Bypassed") : on ? c?.summary ?? "" : "Bypassed";
  const ped = d.modifiers.some((m) => m.key === key && m.source);
  const sel = !isSwitch && focusBlock === key;
  const act = isSwitch ? `data-toggle="${key}"` : `data-block="${key}"`;
  const title = isSwitch ? `${label}: click to switch ${on ? "off" : "on"}` : `Edit ${label}`;
  return `<g class="node ${on ? "on" : "off"} ${sel ? "sel" : ""} ${isSwitch ? "switch" : ""}" transform="translate(${x},${y})" ${act} tabindex="0" role="button" aria-label="${esc(title)}">
    <title>${esc(title)}</title>
    <rect class="box" width="${NW}" height="${NH}" rx="8"/>
    <circle class="led" cx="${NW - 13}" cy="13" r="4"/>
    <text class="k" x="12" y="25">${esc(label)}</text>
    <text class="t" x="12" y="46">${esc(sub.length > 15 ? sub.slice(0, 14) + "…" : sub)}</text>
    ${ped ? `<g class="ped" transform="translate(${NW - 54},6)"><rect width="30" height="15" rx="3"/><text x="15" y="11" text-anchor="middle">PDL</text></g>` : ""}
  </g>`;
}
function signalFlow(d: Detail): string {
  const on = (k: string) => d.chain.find((c) => c.key === k)?.on ?? false;
  const routing = d.routings[d.routing] ?? "Serial";
  const cable = (path: string, lit = true) => `<path class="cable ${lit ? "" : "off"}" d="${path}"/>`;
  const jack = (x: number, y: number, t: string) => `<g class="jack"><circle cx="${x}" cy="${y}" r="9"/><circle class="pin" cx="${x}" cy="${y}" r="3"/><text x="${x}" y="${y - 18}" text-anchor="middle">${t}</text></g>`;
  const zone = (x: number, y: number, w: number, h: number, label: string, cls: string, lx = 12) =>
    `<rect class="zone ${cls}" x="${x}" y="${y}" width="${w}" height="${h}" rx="8"/><text class="zl" x="${x + lx}" y="${y + 18}">${esc(label)}</text>`;
  let back = "", front = "";
  // Row 1
  const y1 = 44, cy1 = y1 + NH / 2;
  back += zone(32, 10, 448, 130, "Digital · before the amp", "pre");
  back += zone(494, 10, 560, 130, "Pedal loops · analog, true bypass", "loops");
  back += zone(1068, 10, 236, 130, "Insert · your amp's preamp", "amp");
  front += jack(24, cy1, "IN");
  const row1: [string, number][] = [["gate", 66], ["filter", 212], ["comp", 358], ["loop1", 514], ["loop2", 652], ["loop3", 790], ["loop4", 928], ["insert", 1110]];
  let px = 33;
  for (const [k, x] of row1) { back += cable(`M ${px} ${cy1} L ${x} ${cy1}`); px = x + NW; }
  for (const [k, x] of row1) front += flowNode(d, k, x, y1);
  // Row 2
  const y2 = 196, top = y2 + 34, endX = 1110 + NW;
  const post = ["mod", "pitch", "delay", "reverb"];
  const parallel = routing === "Parallel", semi = routing === "Semi Parallel";
  const stack = parallel ? 4 : semi ? 2 : 1, gap = 14;
  const bodyH = stack * NH + (stack - 1) * gap;
  const height = bodyH + 70, cy = top + bodyH / 2;
  back += zone(32, y2, 1272, height, `Digital · after the preamp — ${routing}`, "post", 40);
  back += cable(`M ${endX} ${cy1} L ${endX + 40} ${cy1} Q ${endX + 56} ${cy1} ${endX + 56} ${cy1 + 16} L ${endX + 56} ${y2 - 18} Q ${endX + 56} ${y2 - 4} ${endX + 40} ${y2 - 4} L 60 ${y2 - 4} Q 44 ${y2 - 4} 44 ${y2 + 12} L 44 ${cy - 16} Q 44 ${cy} 60 ${cy}`);
  const split = (x0: number, x1: number, keys: string[], note: string) => {
    keys.forEach((k, i) => {
      const ny = top + i * (NH + gap), ncy = ny + NH / 2;
      back += cable(`M ${x0} ${cy} C ${x0 + 70} ${cy} ${x1 - 90} ${ncy} ${x1} ${ncy}`, on(k));
      back += cable(`M ${x1 + NW} ${ncy} C ${x1 + NW + 90} ${ncy} ${x1 + NW + 50} ${cy} ${x1 + NW + 120} ${cy}`, on(k));
      front += flowNode(d, k, x1, ny);
    });
    front += `<text class="note" x="${x1 - 60}" y="${top + bodyH + 30}">${esc(note)}</text>`;
    return x1 + NW + 120;
  };
  let x = 60, nx = 100;
  if (parallel) {
    back += cable(`M 60 ${cy} L 330 ${cy}`);
    x = split(330, 470, post, "All four get the same signal: no effect colours another.");
    nx = x + 80;
  } else {
    const series = semi ? ["mod", "pitch"] : post;
    for (const k of series) { back += cable(`M ${x} ${cy} L ${nx} ${cy}`); front += flowNode(d, k, nx, cy - NH / 2); x = nx + NW; nx += NW + 46; }
    if (semi) {
      x = split(x, x + 150, ["delay", "reverb"], "Delay and reverb side by side: the repeats stay dry.");
      nx = x + 40;
    }
  }
  back += cable(`M ${x} ${cy} L ${nx} ${cy}`);
  front += flowNode(d, "eq", nx, cy - NH / 2);
  back += cable(`M ${nx + NW} ${cy} L 1250 ${cy}`);
  front += jack(1262, cy, "OUT");
  if (!parallel && !semi) front += `<text class="note" x="${nx + NW + 30}" y="${cy + 46}">Each block feeds the next, like pedals on a board.</text>`;
  return `<svg class="flow" viewBox="0 0 ${FW} ${y2 + height + 12}" role="group" aria-label="Signal flow">${back}${front}</svg>`;
}

// ---------------------------------------------------------------- block editor
// Parameters grouped by what they do. EQ is grouped by band.
const GROUPS: [string, RegExp][] = [
  ["Time", /^(Delay|Tempo|Tap|Offset|Speed|Rate|Pre.?delay|Decay|Chorus delay|Sweep)/i],
  ["Repeats", /^(Feedback|FB )/i],
  ["Tone & dynamics", /(cut|Hi |Lo |Col|Freq|Damp|Shape|Size|Diff|Range|Resonance|Res\b|Voice|Pitch|Interval|Key|Scale|Depth|Detune|Spread|Drive|Sens|Thresh|Ratio|Attack|Release|Hold|Resp|Gain|Early|(High|Low|Reverb) level)/i],
  ["Output", /^(Mix|Out|Pan|Width|Balance|Level)/i],
];
function groupRows(b: BlockView): [string, Row[]][] {
  if (b.key === "eq") {
    const bands: [string, Row[]][] = [["Band 1", []], ["Band 2", []], ["Band 3", []], ["", []]];
    for (const r of b.rows) {
      const n = Number(r.label.match(/(\d)$/)?.[1]);
      (n >= 1 && n <= 3 ? bands[n - 1][1] : bands[3][1]).push(r);
    }
    return bands.filter(([, rows]) => rows.length).map(([t, rows]) => [t || "Switch", rows]);
  }
  const out: [string, Row[]][] = [...GROUPS.map(([t]) => [t, []] as [string, Row[]]), ["Character", []]];
  for (const r of b.rows) {
    const i = GROUPS.findIndex(([t, re]) => (t !== "Tone & dynamics" ? re.test(r.label) : re.test(r.label) && !/^(Mix|Out)/i.test(r.label)));
    out[i < 0 ? out.length - 1 : i][1].push(r);
  }
  return out.filter(([, rows]) => rows.length);
}
function ribbon(r: Row, mod: ModView | undefined): string {
  if (r.choices) {
    const opts = r.choices.map((c, i) => `<option value="${r.lo + i}" ${r.lo + i === r.value ? "selected" : ""}>${esc(c)}</option>`).join("");
    return `<div class="prm choice"><label class="n" for="f${r.field}">${esc(r.label)}</label><select id="f${r.field}" class="pick" data-field="${r.field}">${opts}</select></div>`;
  }
  const out = r.value < r.lo || r.value > r.hi;
  const v = Math.min(r.hi, Math.max(r.lo, r.value));
  const range = mod?.values ? `<span class="pnote">${esc(d_src(mod))} pedal: ${esc(mod.values[0])} → ${esc(mod.values[2])}</span>` : "";
  return `<div class="prm ${out ? "unmapped" : ""}"><span class="n">${esc(r.label)}</span><span class="v" data-val="${r.field}">${esc(r.display)}</span>
    <div class="ribbon" data-field="${r.field}" data-lo="${r.lo}" data-hi="${r.hi}" data-v="${v}" ${mod ? `data-pmin="${mod.min}" data-pmax="${mod.max}"` : ""}
      tabindex="0" role="slider" aria-label="${esc(r.label)}" aria-valuemin="${r.lo}" aria-valuemax="${r.hi}" aria-valuenow="${v}" aria-valuetext="${esc(r.display)}">${ribbonInner(r.lo, r.hi, v, mod)}</div>${range}</div>`;
}
const d_src = (m: ModView) => detail?.sources[m.source] ?? "Pedal";
function ribbonInner(lo: number, hi: number, v: number, mod?: { min: number; max: number }): string {
  const t = (x: number) => ((x - lo) / (hi - lo || 1)) * 100;
  const fill = lo < 0 && hi > 0
    ? `<i class="fill bi" style="left:${Math.min(t(0), t(v))}%;width:${Math.abs(t(v) - t(0))}%"></i>`
    : `<i class="fill" style="width:${t(v)}%"></i>`;
  const range = mod ? `<i class="range" style="left:${mod.min}%;width:${Math.max(0, mod.max - mod.min)}%"></i>` : "";
  return `${range}${fill}<i class="thumb" style="left:${t(v)}%"></i>`;
}
function pedalPanel(d: Detail, b: BlockView): string {
  const mods = d.modifiers.filter((m) => m.key === b.key);
  if (!mods.length) return `<aside class="side"><h3>Pedal</h3><p class="hint">${esc(b.label)} has no parameters a pedal can control.</p></aside>`;
  let m = mods.find((x) => x.slot === pedalSlot) ?? mods.find((x) => x.source) ?? mods[0];
  pedalSlot = m.slot;
  const X0 = 50, X1 = 296, Y0 = 160, Y1 = 16;
  const px = (p: number) => X0 + (p / 100) * (X1 - X0), py = (p: number) => Y0 - (p / 100) * (Y0 - Y1);
  let c = "";
  for (const g of [25, 50, 75, 100]) c += `<line class="grid" x1="${X0}" x2="${X1}" y1="${py(g)}" y2="${py(g)}"/>`;
  c += `<line class="ax" x1="${X0}" x2="${X1}" y1="${Y0}" y2="${Y0}"/><line class="ax" x1="${X0}" x2="${X0}" y1="${Y0}" y2="${Y1}"/>`;
  const pts: ["min" | "mid" | "max", number, number][] = [["min", 0, m.min], ["mid", 50, m.mid], ["max", 100, m.max]];
  c += `<polyline class="ln ${m.source ? "" : "unassigned"}" points="${pts.map(([, p, v]) => `${px(p)},${py(v)}`).join(" ")}"/>`;
  pts.forEach(([k, p, v], i) => {
    const label = m.values?.[i] ?? `${v} %`;
    c += `<circle class="pt" data-pt="${k}" cx="${px(p)}" cy="${py(v)}" r="7" tabindex="0" role="slider" aria-label="${["Heel", "Middle", "Toe"][i]} value" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${v}" aria-valuetext="${esc(label)}"/>`;
    c += `<text class="vl" x="${px(p) + (p === 100 ? -10 : 10)}" y="${py(v) - 11}" text-anchor="${p === 100 ? "end" : "start"}">${esc(label)}</text>`;
  });
  c += `<text x="${X0}" y="${Y0 + 20}">Heel</text><text x="${px(50)}" y="${Y0 + 20}" text-anchor="middle">Middle</text><text x="${X1}" y="${Y0 + 20}" text-anchor="end">Toe</text>`;
  if (m.scale) c += `<text x="${X0 - 8}" y="${py(100) + 4}" text-anchor="end">${esc(m.scale[1])}</text><text x="${X0 - 8}" y="${Y0}" text-anchor="end">${esc(m.scale[0])}</text>`;
  const rows = mods.map((x) => `<div class="mrow ${x.slot === m.slot ? "act" : ""}" data-mod="${x.slot}">
      <button class="mname" data-pedal="${x.slot}">${esc(x.label)}</button>
      <select class="pick" data-mod-source aria-label="${esc(x.label)} pedal source">${d.sources.map((s, i) => `<option value="${i}" ${i === x.source ? "selected" : ""}>${esc(s)}</option>`).join("")}</select>
      <span class="r">${x.source && x.values ? `${esc(x.values[0])} → ${esc(x.values[2])}` : ""}</span></div>`).join("");
  return `<aside class="side">
    <h3>Pedal <span>${esc(m.label)}${m.source ? ` · ${esc(d.sources[m.source])}` : " · not assigned"}</span></h3>
    <div class="curve" data-curve="${m.slot}" data-source="${m.source}" data-min="${m.min}" data-mid="${m.mid}" data-max="${m.max}"><svg viewBox="0 0 308 190">${c}</svg></div>
    <div class="mods">${rows}</div>
    <p class="hint">Pick a source to assign a pedal, then drag the heel, middle and toe points.</p>
  </aside>`;
}
function blockEditor(d: Detail, b: BlockView): string {
  const typeSel = b.types.length
    ? `<select class="pick big" data-type="${b.key}" aria-label="${esc(b.label)} type">${b.types.map((t, i) => `<option value="${i}" ${i === b.type_index ? "selected" : ""}>${esc(t)}</option>`).join("")}</select>`
    : "";
  const mods = new Map(d.modifiers.filter((m) => m.key === b.key && m.source && m.field != null).map((m) => [m.field!, m]));
  const groups = groupRows(b).map(([title, rows]) => `<div class="group"><h3>${esc(title)}</h3>${rows.map((r) => ribbon(r, mods.get(r.field))).join("")}</div>`).join("");
  return `<section class="card editor ${b.on ? "" : "off"}" id="card-${b.key}" aria-label="${esc(b.label)}">
    <div class="main">
      <div class="cardhd"><h2>${esc(b.label)}</h2>${typeSel}
        <button class="onoff ${b.on ? "on" : ""}" data-toggle="${b.key}" aria-pressed="${b.on}"><i></i>${b.on ? "On" : "Off"}</button></div>
      <div class="groups">${groups}</div>
    </div>
    ${pedalPanel(d, b)}
  </section>`;
}
function presetView(): string {
  const d = detail;
  if (!d) return `<main class="body"><div class="centered"><div class="note">Pick a preset in Banks first.</div></div></main>`;
  if (!focusBlock || !d.blocks.some((b) => b.key === focusBlock)) {
    focusBlock = d.blocks.find((b) => b.on && b.key !== "eq")?.key ?? d.blocks[0]?.key ?? null;
  }
  const shown = ["Serial", "Semi Parallel", "Parallel"].map((n) => [n, d.routings.indexOf(n)] as [string, number]);
  if (d.routing === 3) shown.push([d.routings[3], 3]); // "Serial2": not in the manual, shown only when a preset uses it
  const seg = shown.map(([n, i]) => `<button class="${d.routing === i ? "on" : ""}" data-routing-set="${i}">${esc(n.replace("Semi Parallel", "Semi-parallel"))}</button>`).join("");
  const livebar = unit.connected
    ? `<div class="livebar ${live ? "on" : ""}">
        <button class="toggle ${live ? "on" : ""}" id="live" aria-label="Live mode ${live ? "on" : "off"}"></button>
        <div><b>Live</b> ${live ? "— every change goes to the unit's edit buffer as you make it." : "— off: edits change the file only."}
        <span class="dim">To keep a sound on the unit, press <b>STORE</b> on the G-System and pick a slot there.</span></div>
        <span class="status ${statusErr ? "err" : ""}">${esc(status)}</span>
      </div>`
    : `<div class="banner">Edits change this file. Connect a G-System to hear them live, then press STORE on the unit to keep them.</div>`;
  const b = d.blocks.find((x) => x.key === focusBlock);
  return `<main class="presetbody">
    ${livebar}
    <section class="card">
      <div class="cardhd">
        <h2>Signal flow</h2>
        <div class="seg" role="group" aria-label="Routing">${seg}</div>
        <div class="presetctl">
          <label>Tempo <input type="number" min="20" max="600" value="${d.bpm ?? ""}" data-bpm class="num" aria-label="Tempo in BPM"> <span class="dim">BPM</span></label>
          <button class="onoff sm ${d.boost ? "on" : ""}" data-toggle="boost" aria-pressed="${d.boost}"><i></i>Boost</button>
          <select class="pick" data-boost aria-label="Boost level">${Array.from({ length: 11 }, (_, i) => `<option value="${i}" ${i === d.boost_level ? "selected" : ""}>+${i} dB</option>`).join("")}</select>
          <label>NovaDrive <select class="pick" data-nova aria-label="NovaDrive NDR-1 preset">${Array.from({ length: 19 }, (_, i) => `<option value="${i}" ${i === d.nova_preset ? "selected" : ""}>${i ? `Preset ${i}` : "None"}</option>`).join("")}</select></label>
        </div>
      </div>
      <div class="flowwrap">${signalFlow(d)}</div>
      <div class="legend"><span><i class="sig"></i>Signal</span><span><i class="off"></i>Bypassed</span><span><i class="ped"></i>Pedal-controlled</span><span class="dim">Click a block to edit it; click a loop or the insert to switch it.</span></div>
    </section>
    ${b ? blockEditor(d, b) : ""}
  </main>`;
}

// ---------------------------------------------------------------- global view
function globalView(): string {
  if (!globals) {
    return `<main class="body"><div class="centered"><div class="connect">
      <div><h1>Global settings</h1><p>These live on the unit, not in presets: levels, locks, pedals, MIDI, tuner and the footswitch assignments. Reading them doesn't change anything on the unit.</p></div>
      <div class="rowbtns"><button class="btn primary" id="readglobals" ${unit.connected && !globalsBusy ? "" : "disabled"}>${globalsBusy ? "Reading…" : "Read from unit"}</button>
      ${unit.connected ? "" : `<a href="#" id="toconnect2">Connect a G-System first</a>`}</div>
      <div class="status ${statusErr ? "err" : ""}">${esc(status)}</div>
    </div></div></main>`;
  }
  const card = (title: string, rows: string) => `<section class="card"><div class="hd"><span class="ttl">${esc(title)}</span></div><div class="bd"><div class="kv">${rows}</div></div></section>`;
  const groups = globals.groups.map((g) => card(g.title, g.params.map((p) =>
    `<span class="k" title="${esc(p.help)}">${esc(p.name)}</span><span class="v">${esc(p.display)}</span>`).join(""))).join("");
  const sw = card("Footswitches", globals.switches.map((s) =>
    `<span class="k">#${s.number}</span><span class="v ${s.fixed ? "dim" : ""}">${esc(s.display)}</span>`).join(""));
  return `
    <div class="banner">Read from the unit. Editing global settings isn't supported yet; change them on the unit's front panel.</div>
    <main class="gcols">${groups}${sw}</main>`;
}

// ---------------------------------------------------------------- connect view
function connectView(): string {
  const opts = (list: string[], cur: string) => list.length ? list.map((p) => `<option ${p === cur ? "selected" : ""}>${esc(p)}</option>`).join("") : `<option value="">(no ports found)</option>`;
  const busy = unit.progress;
  return `<main class="body"><div class="centered"><div class="connect">
    <div><h1>Connect your G-System</h1><p>Plug the G-System's USB port into this computer; it shows up as a MIDI device. A USB-MIDI interface on the 5-pin jacks works too. Reading is safe; this app doesn't write to the unit yet.</p></div>
    <section class="panel">
      <div class="two">
        <div class="field"><label class="eyebrow" for="pin">MIDI in (from unit)</label><select id="pin">${opts(ports.inputs, portIn)}</select></div>
        <div class="field"><label class="eyebrow" for="pout">MIDI out (to unit)</label><select id="pout">${opts(ports.outputs, portOut)}</select></div>
      </div>
      <div style="display:flex;flex-direction:column;gap:10px">
        <div class="step ${ports.inputs.length ? "ok" : ""}"><span class="ic">${ports.inputs.length ? "✓" : "1"}</span><span>MIDI ports found</span><span class="r">${ports.inputs.length ? esc(portIn) : "none"}</span></div>
        <div class="step ${unit.version ? "ok" : ""}"><span class="ic">${unit.version ? "✓" : "2"}</span><span>${unit.version ? "G-System answered" : "Identify the unit"}</span><span class="r">${unit.version ? esc(unit.version) : ""}</span></div>
        <div class="step ${busy ? "busy" : unit.connected && bank.occupied ? "ok" : ""}"><span class="ic">${busy ? "" : unit.connected && bank.occupied ? "✓" : "3"}</span><span>${busy ? esc(busy) : "Read the 200 user presets"}</span><span class="r">${!busy && unit.connected && bank.occupied ? `${bank.occupied} presets` : ""}</span></div>
      </div>
      <div class="rowbtns">
        <button class="btn primary" id="connect" ${ports.inputs.length && !busy ? "" : "disabled"}>${busy ? "Reading…" : "Connect & read user bank"}</button>
        <button class="btn" id="rescan">Rescan ports</button>
        <a href="#" id="offline">Work offline with a file instead</a>
      </div>
      <div class="status ${statusErr ? "err" : ""}">${esc(status)}</div>
    </section>
    <div class="note"><b>If nothing answers</b>Pick the G-System's own ports in both lists, and close any other app that has them open. With a 5-pin interface, cross the cables (interface OUT → unit IN, unit OUT → interface IN). Empty slots don't answer, so a new unit reads as mostly empty.</div>
  </div></div></main>`;
}

// ---------------------------------------------------------------- render + wiring
function render() {
  // Keep keyboard focus on the same control across re-renders.
  const a = document.activeElement as HTMLElement | null;
  const key = a?.dataset.field != null ? `${a.tagName.toLowerCase()}[data-field="${a.dataset.field}"]` : a?.dataset.pt ? `[data-pt="${a.dataset.pt}"]` : null;
  app.innerHTML = header() + (view === "connect" ? connectView() : strip() + (view === "banks" ? banksView() : view === "preset" ? presetView() : globalView()));
  wire();
  if (key) document.querySelector<HTMLElement>(key)?.focus();
}
function on(id: string, fn: (el: HTMLElement) => void) {
  const el = document.getElementById(id);
  if (el) el.onclick = (ev) => { ev.preventDefault(); fn(el); };
}
function wire() {
  on("open", doOpen);
  on("save", doSave);
  document.querySelectorAll<HTMLButtonElement>("[data-view]").forEach((b) => (b.onclick = () => go(b.dataset.view as View)));
  document.querySelectorAll<HTMLButtonElement>("[data-half]").forEach((b) => (b.onclick = () => { half = Number(b.dataset.half) as 0 | 1; render(); }));
  on("getall", () => midiBackup(0, 199));
  on("syncdirty", syncDirty);
  on("edit", () => go("preset"));
  on("readone", () => selected != null && midiBackup(selected, selected));
  on("audition", () => selected != null && audition(selected));
  on("live", () => setLive(!live));
  on("clear", async () => { if (selected != null) await refresh(await invoke<BankView>("clear_slot", { index: selected })); });
  on("copyto", () => setStatus("Drag the preset to a slot while holding ⌥ to copy it"));
  on("toconnect", () => go("connect"));
  on("offline", () => (bank.occupied ? go("banks") : doOpen()));
  on("rescan", loadPorts);
  on("connect", async () => { if (await identify()) { await midiBackup(0, 199); go("banks"); } else render(); });
  const pin = document.getElementById("pin") as HTMLSelectElement | null;
  const pout = document.getElementById("pout") as HTMLSelectElement | null;
  if (pin) pin.onchange = () => (portIn = pin.value);
  if (pout) pout.onchange = () => (portOut = pout.value);

  // rename field
  const pname = document.getElementById("pname") as HTMLInputElement | null;
  if (pname) {
    const commit = async () => {
      if (selected == null || pname.value === bank.slots[selected].name) return;
      try { await refresh(await invoke<BankView>("rename_slot", { index: selected, name: pname.value })); } catch (e) { setStatus(String(e), true); }
    };
    pname.onchange = commit;
    pname.onkeydown = (ev) => { if (ev.key === "Enter") pname.blur(); };
  }

  // grid cells
  document.querySelectorAll<HTMLElement>(".cell").forEach((el) => {
    const i = Number(el.dataset.i);
    el.onclick = async () => {
      selected = i;
      await refresh();
      if (live && bank.slots[i]?.name != null) await audition(i);
    };
    el.ondblclick = () => startRename(i, el);
    el.ondragstart = (ev) => { dragFrom = i; ev.dataTransfer?.setData("text/plain", String(i)); };
    el.ondragover = (ev) => { ev.preventDefault(); el.classList.add("drop"); };
    el.ondragleave = () => el.classList.remove("drop");
    el.ondrop = async (ev) => {
      ev.preventDefault();
      el.classList.remove("drop");
      if (dragFrom == null || dragFrom === i) return;
      const v = ev.altKey
        ? await invoke<BankView>("copy_slot", { from: dragFrom, to: i })
        : ev.shiftKey
          ? await invoke<BankView>("insert_slot", { from: dragFrom, to: i })
          : await invoke<BankView>("swap_slots", { a: dragFrom, b: i });
      dragFrom = null;
      selected = i;
      await refresh(v);
    };
  });

  // finish, front panel
  on("mode", toggleMode);
  document.querySelectorAll<HTMLButtonElement>("[data-fs]").forEach((b) => (b.onclick = async () => {
    if (selected == null) return;
    await selectSlot(Math.floor(selected / 5) * 5 + Number(b.dataset.fs));
  }));
  document.querySelectorAll<HTMLButtonElement>("[data-bank]").forEach((b) => (b.onclick = async () => {
    const cur = selected ?? 0;
    const nb = Math.min(39, Math.max(0, Math.floor(cur / 5) + Number(b.dataset.bank)));
    half = nb < 20 ? 0 : 1;
    await selectSlot(nb * 5 + (cur % 5));
  }));

  // preset view: flow nodes pick the block to edit (loops/insert toggle); ribbons, selects and the pedal curve write params
  document.querySelectorAll<SVGGElement>("g.node").forEach((g) => (g.onkeydown = (ev) => {
    if (ev.key === "Enter" || ev.key === " ") { ev.preventDefault(); g.dispatchEvent(new MouseEvent("click", { bubbles: true })); }
  }));
  document.querySelectorAll<HTMLElement>("[data-block]").forEach((b) => (b.onclick = () => { focusBlock = b.dataset.block!; render(); }));
  document.querySelectorAll<HTMLElement>(".ribbon").forEach(wireRibbon);
  document.querySelectorAll<HTMLSelectElement>("select[data-field]").forEach((el) => (el.onchange = () => edit({ kind: "field", field: Number(el.dataset.field), value: Number(el.value) })));
  document.querySelectorAll<HTMLSelectElement>("[data-type]").forEach((el) => (el.onchange = () => edit({ kind: "type", block: el.dataset.type!, value: Number(el.value) })));
  document.querySelectorAll<HTMLElement>("[data-toggle]").forEach((b) => (b.onclick = () => edit({ kind: "toggle", key: b.dataset.toggle! })));
  document.querySelectorAll<HTMLButtonElement>("[data-routing-set]").forEach((b) => (b.onclick = () => edit({ kind: "routing", value: Number(b.dataset.routingSet) })));
  document.querySelectorAll<HTMLSelectElement>("[data-boost]").forEach((el) => (el.onchange = () => edit({ kind: "boost_level", value: Number(el.value) })));
  document.querySelectorAll<HTMLInputElement>("[data-bpm]").forEach((el) => (el.onchange = () => edit({ kind: "bpm", value: Number(el.value) })));
  document.querySelectorAll<HTMLSelectElement>("[data-nova]").forEach((el) => (el.onchange = () => edit({ kind: "nova", value: Number(el.value) })));
  document.querySelectorAll<HTMLButtonElement>("[data-pedal]").forEach((b) => (b.onclick = () => { pedalSlot = Number(b.dataset.pedal); render(); }));
  document.querySelectorAll<HTMLElement>("[data-mod]").forEach((row) => {
    const slot = Number(row.dataset.mod);
    const sel = row.querySelector<HTMLSelectElement>("[data-mod-source]")!;
    sel.onchange = () => {
      const m = detail?.modifiers.find((x) => x.slot === slot);
      if (!m) return;
      pedalSlot = slot;
      edit({ kind: "modifier", slot, source: Number(sel.value), min: m.min, mid: m.mid, max: m.max });
    };
  });
  document.querySelectorAll<HTMLElement>("[data-curve]").forEach(wireCurve);
  on("readglobals", async () => {
    globalsBusy = true;
    render();
    try {
      globals = await invoke<GlobalsView>("midi_globals", { input: portIn, output: portOut });
      setStatus("");
    } catch (e) {
      setStatus(String(e), true);
    }
    globalsBusy = false;
    render();
  });
  on("toconnect2", () => go("connect"));
}
async function selectSlot(i: number) {
  selected = i;
  await refresh();
  if (live && bank.slots[i]?.name != null) await audition(i);
}
/** Apply one edit through the Rust core. `rerender: false` keeps the DOM (used while a knob is being dragged). */
async function edit(e: Edit, rerender = true): Promise<Detail | null> {
  if (selected == null) return null;
  try {
    detail = await invoke<Detail>("edit_preset", { index: selected, edit: e });
    if (!bank.dirty.includes(selected)) bank.dirty = [...bank.dirty, selected];
    if (rerender) render();
    return detail;
  } catch (err) {
    setStatus(String(err), true);
    return null;
  }
}
function showFieldValue(field: string, d: Detail | null) {
  const text = d?.blocks.flatMap((b) => b.rows).find((r) => r.field === Number(field))?.display;
  if (text == null) return;
  document.querySelector(`[data-field="${field}"].ribbon`)?.setAttribute("aria-valuetext", text);
  const out = document.querySelector<HTMLElement>(`[data-val="${field}"]`);
  if (out) out.textContent = text;
}
/** Horizontal slider: click or drag anywhere on it, or use the arrow keys
 * (Shift for steps of 10). While dragging, edits go out at most every 60 ms
 * (Live mode hears them); the screen re-renders when you let go. */
let settle: number | null = null;
function wireRibbon(el: HTMLElement) {
  const lo = Number(el.dataset.lo), hi = Number(el.dataset.hi), field = el.dataset.field!;
  const mod = el.dataset.pmin != null ? { min: Number(el.dataset.pmin), max: Number(el.dataset.pmax) } : undefined;
  const clamp = (v: number) => Math.min(hi, Math.max(lo, Math.round(v)));
  let pending: number | null = null;
  let timer: number | null = null;
  let dragging = false;
  const push = () => {
    timer = null;
    if (pending == null) return;
    const v = pending;
    pending = null;
    edit({ kind: "field", field: Number(field), value: v }, false).then((d) => {
      showFieldValue(field, d);
      if (!dragging) {
        if (settle != null) clearTimeout(settle);
        settle = window.setTimeout(render, 500);
      }
    });
  };
  const moveTo = (v: number) => {
    v = clamp(v);
    if (v === Number(el.dataset.v)) return;
    el.dataset.v = String(v);
    el.setAttribute("aria-valuenow", String(v));
    el.innerHTML = ribbonInner(lo, hi, v, mod);
    pending = v;
    if (timer == null) timer = window.setTimeout(push, 60);
  };
  const at = (clientX: number) => {
    const r = el.getBoundingClientRect();
    return lo + ((clientX - r.left) / r.width) * (hi - lo);
  };
  el.onpointerdown = (ev) => {
    ev.preventDefault();
    el.focus();
    el.setPointerCapture(ev.pointerId);
    dragging = true;
    el.classList.add("dragging");
    moveTo(at(ev.clientX));
    el.onpointermove = (m) => moveTo(at(m.clientX));
    el.onpointerup = el.onpointercancel = async () => {
      el.onpointermove = null;
      dragging = false;
      el.classList.remove("dragging");
      if (timer != null) { clearTimeout(timer); timer = null; }
      if (pending != null) { const v = pending; pending = null; await edit({ kind: "field", field: Number(field), value: v }, false); }
      render();
    };
  };
  el.onkeydown = (ev) => {
    const big = ev.shiftKey ? 10 : 1;
    const step = { ArrowUp: big, ArrowRight: big, ArrowDown: -big, ArrowLeft: -big, PageUp: 10, PageDown: -10 }[ev.key];
    if (step != null) { ev.preventDefault(); moveTo(Number(el.dataset.v) + step); }
    else if (ev.key === "Home") { ev.preventDefault(); moveTo(lo); }
    else if (ev.key === "End") { ev.preventDefault(); moveTo(hi); }
  };
}
/** Pedal curve: drag the heel, middle and toe points up or down (0–100 % of
 * the parameter's range), or focus one and use the arrow keys. */
function wireCurve(box: HTMLElement) {
  const slot = Number(box.dataset.curve), source = Number(box.dataset.source);
  const svg = box.querySelector("svg")!;
  const vals = { min: Number(box.dataset.min), mid: Number(box.dataset.mid), max: Number(box.dataset.max) };
  const Y0 = 160, Y1 = 16;
  const send = () => edit({ kind: "modifier", slot, source, ...vals });
  const redraw = () => {
    const xs = [50, 173, 296];
    const pts = (["min", "mid", "max"] as const).map((k, i) => `${xs[i]},${Y0 - (vals[k] / 100) * (Y0 - Y1)}`);
    svg.querySelector("polyline")?.setAttribute("points", pts.join(" "));
    svg.querySelectorAll<SVGCircleElement>("[data-pt]").forEach((c) => {
      const v = vals[c.dataset.pt as "min" | "mid" | "max"];
      c.setAttribute("cy", String(Y0 - (v / 100) * (Y0 - Y1)));
      c.setAttribute("aria-valuenow", String(v));
    });
    svg.querySelectorAll(".vl").forEach((t) => t.remove());
  };
  svg.querySelectorAll<SVGCircleElement>("[data-pt]").forEach((c) => {
    const k = c.dataset.pt as "min" | "mid" | "max";
    if (!source) { c.classList.add("disabled"); return; }
    c.onpointerdown = (ev) => {
      ev.preventDefault();
      c.setPointerCapture(ev.pointerId);
      const move = (e: PointerEvent) => {
        const m = svg.getScreenCTM();
        if (!m) return;
        const y = (e.clientY - m.f) / m.d;
        vals[k] = Math.max(0, Math.min(100, Math.round(((Y0 - y) / (Y0 - Y1)) * 100)));
        redraw();
      };
      c.onpointermove = move;
      c.onpointerup = c.onpointercancel = () => { c.onpointermove = null; send(); };
    };
    c.onkeydown = (ev) => {
      const step = { ArrowUp: 1, ArrowRight: 1, ArrowDown: -1, ArrowLeft: -1 }[ev.key];
      if (step == null) return;
      ev.preventDefault();
      vals[k] = Math.max(0, Math.min(100, vals[k] + (ev.shiftKey ? step * 10 : step)));
      redraw();
      if (settle != null) clearTimeout(settle);
      settle = window.setTimeout(send, 400);
    };
  });
}
function startRename(i: number, cell: HTMLElement) {
  const slot = bank.slots[i];
  if (!slot || slot.name == null) return;
  const nm = cell.querySelector<HTMLElement>(".nm")!;
  const input = document.createElement("input");
  input.value = slot.name;
  input.maxLength = 20;
  nm.replaceChildren(input);
  input.focus();
  input.select();
  let done = false;
  const commit = async () => {
    if (done) return;
    done = true;
    try {
      selected = i;
      await refresh(await invoke<BankView>("rename_slot", { index: i, name: input.value }));
    } catch (e) { setStatus(String(e), true); render(); }
  };
  input.onkeydown = (ev) => { if (ev.key === "Enter") commit(); if (ev.key === "Escape") { done = true; render(); } };
  input.onblur = commit;
}

render();
loadPorts();
refresh().catch(() => {});
