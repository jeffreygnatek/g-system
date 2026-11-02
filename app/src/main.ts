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
type ModView = { slot: number; key: string; label: string; source: number; min: number; mid: number; max: number; display: string };
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
let presetMode: "block" | "all" = "block";
type Skin = "grey" | "black" | "red";
const SKINS: [Skin, string][] = [["grey", "Grey brushed"], ["black", "Black brushed"], ["red", "Red brushed"]];
let skin: Skin = "grey";
try {
  const s = localStorage.getItem("skin") as Skin | null;
  if (s && SKINS.some(([k]) => k === s)) skin = s;
} catch { /* storage unavailable: keep the default */ }
function setSkin(s: Skin) {
  skin = s;
  document.documentElement.dataset.skin = s;
  try { localStorage.setItem("skin", s); } catch { /* ignore */ }
  render();
}
document.documentElement.dataset.skin = skin;
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
    <div class="finish" role="group" aria-label="Panel finish">${SKINS.map(([k, label]) =>
      `<button class="sw-${k} ${skin === k ? "on" : ""}" data-skin="${k}" title="${label}" aria-label="${label}" aria-pressed="${skin === k}"></button>`).join("")}</div>
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
        <span class="digits">${bankNo != null ? pad2(bankNo) : "--"}<sup>${selected != null ? (selected % 5) + 1 : ""}</sup></span>
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
// Knobs: 270° sweep. Bipolar ranges (e.g. -12..+12 dB) light the arc from the centre.
const KNOB_R = 24;
function polar(deg: number, r = KNOB_R): string {
  const a = (deg * Math.PI) / 180;
  return `${(30 + r * Math.sin(a)).toFixed(2)} ${(30 - r * Math.cos(a)).toFixed(2)}`;
}
function arc(a1: number, a2: number): string {
  if (a2 < a1) [a1, a2] = [a2, a1];
  if (a2 - a1 < 0.5) return "";
  return `M ${polar(a1)} A ${KNOB_R} ${KNOB_R} 0 ${a2 - a1 > 180 ? 1 : 0} 1 ${polar(a2)}`;
}
function knobSvg(lo: number, hi: number, v: number): string {
  const t = hi > lo ? (Math.min(hi, Math.max(lo, v)) - lo) / (hi - lo) : 0;
  const av = -135 + 270 * t;
  const a0 = lo < 0 && hi > 0 ? -135 + (270 * -lo) / (hi - lo) : -135;
  return `<svg viewBox="0 0 60 60" aria-hidden="true">
    <path class="trk" d="${arc(-135, 135)}"/>
    <path class="val" d="${arc(a0, av)}"/>
    <circle class="face" cx="30" cy="30" r="17"/>
    <path class="ptr" d="M ${polar(av, 6)} L ${polar(av, 14)}"/>
  </svg>`;
}
function knob(attrs: string, label: string, lo: number, hi: number, value: number, display: string): string {
  const v = Math.min(hi, Math.max(lo, value));
  return `<div class="knob" ${attrs} data-lo="${lo}" data-hi="${hi}" data-v="${v}" tabindex="0" role="slider"
      aria-label="${esc(label)}" aria-valuemin="${lo}" aria-valuemax="${hi}" aria-valuenow="${v}" aria-valuetext="${esc(display)}">${knobSvg(lo, hi, v)}</div>`;
}
function control(r: Row): string {
  const out = r.value < r.lo || r.value > r.hi;
  if (r.choices) {
    const opts = r.choices.map((c, i) => `<option value="${r.lo + i}" ${r.lo + i === r.value ? "selected" : ""}>${esc(c)}</option>`).join("");
    return `<div class="kc choice"><span class="kn">${esc(r.label)}</span><select class="sel" data-field="${r.field}" aria-label="${esc(r.label)}">${opts}</select></div>`;
  }
  return `<div class="kc ${out ? "unmapped" : ""}"><span class="kn">${esc(r.label)}</span>
    ${knob(`data-field="${r.field}"`, r.label, r.lo, r.hi, r.value, r.display)}
    <span class="kv" data-val="${r.field}">${esc(r.display)}</span></div>`;
}
function modifierRows(key: string): string {
  const d = detail;
  const mods = d ? d.modifiers.filter((m) => m.key === key) : [];
  if (!d || !mods.length) return "";
  const row = (m: ModView) => `<div class="mrow" data-mod="${m.slot}">
      <span class="ml">${esc(m.label)}</span>
      <select class="sel" data-mod-source aria-label="${esc(m.label)} modifier source">${d.sources.map((s, i) => `<option value="${i}" ${i === m.source ? "selected" : ""}>${esc(s)}</option>`).join("")}</select>
      ${(["min", "mid", "max"] as const).map((k) => `<input type="number" min="0" max="100" value="${m[k]}" data-mod-${k} ${m.source ? "" : "disabled"} aria-label="${esc(m.label)} ${k} %">`).join("")}
    </div>`;
  return `<details class="mods" ${mods.some((m) => m.source) ? "open" : ""}><summary>Modifiers <span class="dim">${mods.filter((m) => m.source).length}/${mods.length} assigned · source · min / mid / max %</span></summary>${mods.map(row).join("")}</details>`;
}
const modeSeg = () => `<div class="seg"><button class="${presetMode === "block" ? "on" : ""}" data-mode="block">Block</button><button class="${presetMode === "all" ? "on" : ""}" data-mode="all">All blocks</button></div>`;
function blockPanel(b: BlockView, withMode: boolean): string {
  const typeSel = b.types.length
    ? `<select class="sel" data-type="${b.key}" aria-label="${esc(b.label)} type">${b.types.map((t, i) => `<option value="${i}" ${i === b.type_index ? "selected" : ""}>${esc(t)}</option>`).join("")}</select>`
    : "";
  const mix = b.rows.find((r) => r.label === "Mix");
  const outl = b.rows.find((r) => r.label === "Out level");
  const sum = [mix && `<span>Mix <b>${esc(mix.display)}</b></span>`, outl && `<span>Out <b>${esc(outl.display)}</b></span>`].filter(Boolean).join("");
  return `<section class="focus ${b.on ? "" : "off"}" id="card-${b.key}" aria-label="${esc(b.label)}">
    <div class="fhd"><span class="ttl">${esc(b.label)}</span>${typeSel}
      <button class="onoff ${b.on ? "on" : ""}" data-toggle="${b.key}" aria-pressed="${b.on}"><i></i>${b.on ? "On" : "Off"}</button>
      <span class="sum">${sum}</span>${withMode ? modeSeg() : ""}</div>
    <div class="knobs">${b.rows.map(control).join("")}</div>
    ${modifierRows(b.key) ? `<div class="fmods">${modifierRows(b.key)}</div>` : ""}
  </section>`;
}
function routingPanel(d: Detail, withMode: boolean): string {
  return `<section class="focus" id="card-routing" aria-label="Routing and tempo">
    <div class="fhd"><span class="ttl">Routing</span>
      <select class="sel" data-routing aria-label="Routing">${d.routings.map((o, i) => `<option value="${i}" ${d.routing === i ? "selected" : ""}>${esc(o)}</option>`).join("")}</select>
      <span class="sum"></span>${withMode ? modeSeg() : ""}</div>
    <div class="knobs">
      <div class="kc"><span class="kn">Boost level</span>${knob("data-boost", "Boost level", 0, 10, d.boost_level, `${d.boost_level} dB`)}<span class="kv" data-val="boost">${d.boost_level} dB</span></div>
      <div class="kc choice"><span class="kn">Tempo</span><input type="number" min="20" max="600" value="${d.bpm ?? ""}" data-bpm class="sel" aria-label="Tempo in BPM"><span class="kv">BPM</span></div>
      <div class="kc choice"><span class="kn">NovaDrive</span><select class="sel" data-nova aria-label="NovaDrive NDR-1 preset">${Array.from({ length: 19 }, (_, i) => `<option value="${i}" ${i === d.nova_preset ? "selected" : ""}>${i ? `Preset ${i}` : "None"}</option>`).join("")}</select></div>
    </div>
    <div class="fnote">Tempo sets every tap time; delay times and speeds with a note value follow it.</div>
    ${modifierRows("novadrive") ? `<div class="fmods">${modifierRows("novadrive")}</div>` : ""}
  </section>`;
}
function presetView(): string {
  const d = detail;
  if (!d) return `<main class="body"><div class="centered"><div class="note">Pick a preset in Banks first.</div></div></main>`;
  const chain = new Map(d.chain.map((c) => [c.key, c]));
  if (!focusBlock || (focusBlock !== "routing" && !d.blocks.some((b) => b.key === focusBlock))) {
    focusBlock = d.blocks.find((b) => b.on)?.key ?? d.blocks[0]?.key ?? "routing";
  }
  const modules = d.blocks.map((b) => {
    const c = chain.get(b.key);
    return `<button class="module ${b.on ? "on" : ""} ${focusBlock === b.key ? "sel" : ""}" data-block="${b.key}">
      <span class="ml">${esc(c?.name ?? b.label)}</span><span class="mt">${esc(b.type_index != null ? b.types[b.type_index] : c?.summary ?? "")}</span><span class="cap"></span></button>`;
  }).join("");
  const assigned = d.modifiers.filter((m) => m.source).length;
  const rack = `${modules}
    <button class="module text ${focusBlock === "routing" ? "sel" : ""}" data-block="routing"><span class="ml">Routing</span><span class="mt">${esc(d.routings[d.routing])}</span><span class="big">${d.bpm != null ? `${d.bpm}` : "—"}<small>BPM</small></span></button>
    <div class="module text static"><span class="ml">Modifiers</span><span class="mt">${assigned ? `${assigned} assigned` : "none"}</span><span class="big">${assigned}<small>/${d.modifiers.length}</small></span></div>`;
  const loops = d.chain.filter((c) => c.key.startsWith("loop") || c.key === "insert").map((c) =>
    `<button class="lsw ${c.on ? "on" : ""}" data-toggle="${c.key}" aria-pressed="${c.on}"><i></i>${esc(c.key === "insert" ? "INS" : c.name.replace(/Loop\s*/i, "L"))}</button>`).join("");
  const switches = `<div class="switchrow">${loops}
    <button class="lsw boost ${d.boost ? "on" : ""}" data-toggle="boost" aria-pressed="${d.boost}"><i></i>Boost <b>${d.boost_level} dB</b></button></div>`;
  const livebar = unit.connected
    ? `<div class="livebar ${live ? "on" : ""}">
        <button class="toggle ${live ? "on" : ""}" id="live" aria-label="Live mode ${live ? "on" : "off"}"></button>
        <div><b>Live</b> ${live ? "— every change goes to the unit's edit buffer as you make it." : "— off: edits change the file only."}
        <span class="dim">To keep a sound on the unit, press <b>STORE</b> on the G-System and pick a slot there.</span></div>
        <span class="status ${statusErr ? "err" : ""}">${esc(status)}</span>
      </div>`
    : `<div class="banner">Edits change this file. Connect a G-System to hear them live, then press STORE on the unit to keep them.</div>`;
  const body = presetMode === "all"
    ? `<div class="allhd">${modeSeg()}</div>${d.blocks.map((b) => blockPanel(b, false)).join("")}${routingPanel(d, false)}`
    : focusBlock === "routing"
      ? routingPanel(d, true)
      : blockPanel(d.blocks.find((b) => b.key === focusBlock)!, true);
  return `<section class="rack" aria-label="Effect blocks">${rack}</section>
    ${switches}
    <main class="presetbody">${livebar}${body}</main>`;
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
  const key = a?.dataset.field != null ? `[data-field="${a.dataset.field}"]` : a?.hasAttribute("data-boost") ? "[data-boost]" : null;
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
  document.querySelectorAll<HTMLButtonElement>("[data-skin]").forEach((b) => (b.onclick = () => setSkin(b.dataset.skin as Skin)));
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

  // preset view: modules pick the block shown in the panel; knobs/selects/toggles write params
  document.querySelectorAll<HTMLButtonElement>("[data-block]").forEach((b) => (b.onclick = () => {
    focusBlock = b.dataset.block!;
    if (presetMode === "all") document.getElementById(`card-${focusBlock}`)?.scrollIntoView({ block: "start", behavior: "smooth" });
    else render();
  }));
  document.querySelectorAll<HTMLButtonElement>("[data-mode]").forEach((b) => (b.onclick = () => { presetMode = b.dataset.mode as "block" | "all"; render(); }));
  document.querySelectorAll<HTMLElement>(".knob").forEach(wireKnob);
  document.querySelectorAll<HTMLInputElement | HTMLSelectElement>("[data-field]").forEach((el) => {
    const field = Number(el.dataset.field);
    el.onchange = () => edit({ kind: "field", field, value: Number(el.value) });
  });
  document.querySelectorAll<HTMLSelectElement>("[data-type]").forEach((el) => (el.onchange = () => edit({ kind: "type", block: el.dataset.type!, value: Number(el.value) })));
  document.querySelectorAll<HTMLButtonElement>("[data-toggle]").forEach((b) => (b.onclick = () => edit({ kind: "toggle", key: b.dataset.toggle! })));
  document.querySelectorAll<HTMLSelectElement>("[data-routing]").forEach((el) => (el.onchange = () => edit({ kind: "routing", value: Number(el.value) })));
  document.querySelectorAll<HTMLInputElement>("[data-boost]").forEach((el) => (el.onchange = () => edit({ kind: "boost_level", value: Number(el.value) })));
  document.querySelectorAll<HTMLInputElement>("[data-bpm]").forEach((el) => (el.onchange = () => edit({ kind: "bpm", value: Number(el.value) })));
  document.querySelectorAll<HTMLSelectElement>("[data-nova]").forEach((el) => (el.onchange = () => edit({ kind: "nova", value: Number(el.value) })));
  document.querySelectorAll<HTMLElement>("[data-mod]").forEach((row) => {
    const slot = Number(row.dataset.mod);
    const q = (sel: string) => row.querySelector<HTMLInputElement | HTMLSelectElement>(sel)!;
    const send = () => edit({ kind: "modifier", slot, source: Number(q("[data-mod-source]").value),
      min: Number(q("[data-mod-min]").value), mid: Number(q("[data-mod-mid]").value), max: Number(q("[data-mod-max]").value) });
    row.querySelectorAll("select, input").forEach((el) => ((el as HTMLInputElement).onchange = send));
  });
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
function knobEdit(el: HTMLElement, v: number): Edit {
  return el.dataset.field != null ? { kind: "field", field: Number(el.dataset.field), value: v } : { kind: "boost_level", value: v };
}
function setKnob(el: HTMLElement, v: number) {
  const lo = Number(el.dataset.lo), hi = Number(el.dataset.hi);
  el.dataset.v = String(v);
  el.setAttribute("aria-valuenow", String(v));
  el.innerHTML = knobSvg(lo, hi, v);
}
function showKnobValue(el: HTMLElement, d: Detail | null) {
  if (!d) return;
  const field = el.dataset.field;
  let text: string | undefined;
  if (field != null) text = d.blocks.flatMap((b) => b.rows).find((r) => r.field === Number(field))?.display;
  else text = `${d.boost_level} dB`;
  if (text == null) return;
  el.setAttribute("aria-valuetext", text);
  const out = document.querySelector<HTMLElement>(`[data-val="${field ?? "boost"}"]`);
  if (out) out.textContent = text;
}
/** Drag up/down (or wheel, arrow keys) to turn. While dragging, edits are sent
 * at most every 60 ms (Live mode hears them); the screen re-renders on release. */
let settle: number | null = null;
function wireKnob(el: HTMLElement) {
  const lo = Number(el.dataset.lo), hi = Number(el.dataset.hi);
  const clamp = (v: number) => Math.min(hi, Math.max(lo, v));
  const pxPerStep = Math.min(12, Math.max(1.5, 180 / Math.max(1, hi - lo)));
  let pending: number | null = null;
  let timer: number | null = null;
  const push = () => {
    timer = null;
    if (pending == null) return;
    const v = pending;
    pending = null;
    edit(knobEdit(el, v), false).then((d) => {
      showKnobValue(el, d);
      // Wheel and keys: refresh the rest of the screen once turning stops.
      if (!el.classList.contains("turning")) {
        if (settle != null) clearTimeout(settle);
        settle = window.setTimeout(render, 500);
      }
    });
  };
  const turnTo = (v: number) => {
    v = clamp(v);
    if (v === Number(el.dataset.v)) return;
    setKnob(el, v);
    pending = v;
    if (timer == null) timer = window.setTimeout(push, 60);
  };
  el.onpointerdown = (ev) => {
    ev.preventDefault();
    el.setPointerCapture(ev.pointerId);
    el.classList.add("turning");
    const y0 = ev.clientY, v0 = Number(el.dataset.v);
    el.onpointermove = (m) => turnTo(v0 + Math.round((y0 - m.clientY) / (m.shiftKey ? pxPerStep * 4 : pxPerStep)));
    el.onpointerup = el.onpointercancel = async () => {
      el.onpointermove = null;
      el.classList.remove("turning");
      if (timer != null) { clearTimeout(timer); timer = null; }
      if (pending != null) { const v = pending; pending = null; await edit(knobEdit(el, v), false); }
      render();
    };
  };
  el.onwheel = (ev) => { ev.preventDefault(); turnTo(Number(el.dataset.v) + (ev.deltaY < 0 ? 1 : -1)); };
  el.onkeydown = (ev) => {
    const step = { ArrowUp: 1, ArrowRight: 1, ArrowDown: -1, ArrowLeft: -1, PageUp: 10, PageDown: -10 }[ev.key];
    if (step != null) { ev.preventDefault(); turnTo(Number(el.dataset.v) + step); }
    else if (ev.key === "Home") turnTo(lo);
    else if (ev.key === "End") turnTo(hi);
  };
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
