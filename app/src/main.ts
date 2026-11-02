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
    <div style="display:flex;gap:8px">
      <button class="btn" id="open">Open…</button>
      <button class="btn" id="save" ${bank.occupied ? "" : "disabled"}>Save .syx</button>
    </div>
    <div class="unit"><span class="led ${unit.connected ? "on" : ""}"></span><span>${unit.connected ? `G-System · ${esc(portIn)}` : "No unit"}</span></div>
  </header>`;
}
function logo() {
  return `<svg width="22" height="22" viewBox="0 0 24 24" fill="none" aria-hidden="true"><rect x="3" y="6" width="18" height="12" rx="3" stroke="currentColor" stroke-width="1.6"/><circle cx="8" cy="12" r="1.6" fill="#f0b243"/><circle cx="12" cy="12" r="1.6" fill="#f0b243"/><circle cx="16" cy="12" r="1.6" fill="#f0b243"/></svg>`;
}
function strip(): string {
  const s = selected != null ? bank.slots[selected] : null;
  const bankNo = selected != null ? Math.floor(selected / 5) : null;
  const sw = [1, 2, 3, 4, 5].map((n) => `<span class="fsw ${selected != null && selected % 5 === n - 1 ? "on" : ""}">${n}</span>`).join("");
  const tags = (detail?.chain ?? []).filter((c) => !c.key.startsWith("loop") && c.key !== "insert")
    .map((c) => `<span class="tag ${c.on ? "on" : ""}">${esc(c.name.toUpperCase())}</span>`)
    .join("");
  return `<section class="strip" aria-label="Unit status">
    <div class="lcd"><span class="slot">${s ? s.label : "--·-"}</span><span class="sep">│</span><span>${s?.name ? esc(s.name) : "<span class='dim'>no preset</span>"}</span></div>
    <div class="switches"><span class="eyebrow">Bank ${bankNo != null ? String(bankNo).padStart(2, "0") : "--"}</span>${sw}</div>
    <div class="blocks">${tags}</div>
    <div class="tempo"><span class="muted">TEMPO</span><span>${detail?.bpm != null ? `${detail.bpm} BPM` : "— BPM"}</span></div>
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
      <div style="display:flex;justify-content:space-between;font-size:11px;margin-top:6px" class="dim"><span>${s.name.length} / 20 characters</span>${dirty ? `<span style="color:var(--amber)">Edited</span>` : ""}</div>
    </div>
    <div style="display:flex;flex-direction:column;gap:6px">
      <div class="eyebrow">Signal chain</div>
      <div class="chain-mini">${mini}</div>
    </div>
    <div style="display:flex;flex-direction:column;gap:8px">
      <button class="btn primary" id="edit" style="height:38px;border-radius:8px">Edit preset</button>
      <div class="actions2">
        <button class="btn" id="readone" ${unit.connected ? "" : "disabled"}>Re-read from unit</button>
        <button class="btn" id="syncone" disabled title="${WRITE_DISABLED}">Sync this slot</button>
        <button class="btn" id="copyto">Copy to…</button>
        <button class="btn" id="clear" style="color:var(--text-2)">Clear slot</button>
      </div>
    </div>
    ${unitPanel()}
  </aside>`;
}
function unitPanel(): string {
  if (!unit.connected) {
    return `<div class="note" style="margin-top:auto"><b>Unit vs. file</b>No G-System connected. Edits stay in this file until you <a href="#" id="toconnect" style="color:#9bb8e8">connect</a> and sync.</div>`;
  }
  const d = bank.dirty;
  const list = d.slice(0, 6).map((i) => bank.slots[i].label).join(", ") + (d.length > 6 ? ", …" : "");
  return `<div class="note" style="margin-top:auto"><b>Unit vs. file</b>${d.length ? `${d.length} preset(s) changed here (${list}). Writing to the unit is disabled for now, so save a .syx to keep them.` : "Matches what was read from the unit."}</div>`;
}

// ---------------------------------------------------------------- preset view
function paramRow(r: Row): string {
  if (r.choices) {
    const opts = r.choices.map((c, i) => `<option value="${r.lo + i}" ${r.lo + i === r.value ? "selected" : ""}>${esc(c)}</option>`).join("");
    return `<div class="prow"><label>${esc(r.label)}</label><select class="sel" data-field="${r.field}" aria-label="${esc(r.label)}">${opts}</select><span class="val"></span></div>`;
  }
  const out = r.value < r.lo || r.value > r.hi;
  return `<div class="prow ${out ? "unmapped" : ""}">
    <label>${esc(r.label)}</label>
    <input type="range" min="${r.lo}" max="${r.hi}" step="1" value="${Math.min(r.hi, Math.max(r.lo, r.value))}" data-field="${r.field}" aria-label="${esc(r.label)}">
    <span class="val">${esc(r.display)}</span>
  </div>`;
}
function blockCard(b: BlockView): string {
  const typeSel = b.types.length
    ? `<select class="sel" data-type="${b.key}" aria-label="${esc(b.label)} type">${b.types.map((t, i) => `<option value="${i}" ${i === b.type_index ? "selected" : ""}>${esc(t)}</option>`).join("")}</select>`
    : `<span style="margin-left:auto"></span>`;
  return `<section class="card ${b.on ? "" : "off"} ${focusBlock === b.key ? "focus" : ""}" id="card-${b.key}" aria-label="${esc(b.label)}">
    <div class="hd"><span class="ttl">${esc(b.label)}</span>${typeSel}<button class="toggle ${b.on ? "on" : ""}" data-toggle="${b.key}" aria-label="${b.on ? "On" : "Off"}"></button></div>
    <div class="bd">${b.rows.map(paramRow).join("")}${modifierRows(b.key)}</div>
  </section>`;
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
  return `<details class="mods" ${mods.some((m) => m.source) ? "open" : ""}><summary>Modifiers <span class="dim">${mods.filter((m) => m.source).length}/${mods.length} assigned · min / mid / max %</span></summary>${mods.map(row).join("")}</details>`;
}
function routingCard(d: Detail): string {
  const loops = d.chain.filter((c) => c.key.startsWith("loop") || c.key === "insert").map((c) =>
    `<button class="${c.on ? "on" : ""}" data-toggle="${c.key}"><div class="k">${esc(c.name)}</div><div class="v">${c.on ? "On" : "Off"}</div></button>`).join("");
  return `<section class="card" aria-label="Routing, loops and boost">
    <div class="hd"><span class="ttl">Routing · Loops · Boost</span>
      <select class="sel" data-routing aria-label="Routing">${d.routings.map((o, i) => `<option value="${i}" ${d.routing === i ? "selected" : ""}>${esc(o)}</option>`).join("")}</select></div>
    <div class="bd">
      <div class="loops4 five">${loops}</div>
      <div class="prow"><label>Boost</label><button class="toggle ${d.boost ? "on" : ""}" data-toggle="boost" aria-label="Boost ${d.boost ? "on" : "off"}"></button><span class="val"></span></div>
      <div class="prow"><label>Boost level</label><input type="range" min="0" max="10" step="1" value="${d.boost_level}" data-boost aria-label="Boost level"><span class="val">${d.boost_level} dB</span></div>
      <div class="prow"><label>Tempo</label><input type="number" min="20" max="600" value="${d.bpm ?? ""}" data-bpm class="sel" aria-label="Tempo in BPM" style="width:90px"><span class="val">BPM</span></div>
      <div class="note">Tempo sets every tap time; delay times and speeds with a note value follow it.</div>
      <div class="prow"><label>NovaDrive</label><select class="sel" data-nova aria-label="NovaDrive NDR-1 preset">${Array.from({ length: 19 }, (_, i) => `<option value="${i}" ${i === d.nova_preset ? "selected" : ""}>${i ? `Preset ${i}` : "None"}</option>`).join("")}</select><span class="val"></span></div>
      ${modifierRows("novadrive")}
    </div>
  </section>`;
}
function presetView(): string {
  const d = detail;
  if (!d) return `<main class="body"><div class="centered"><div class="note">Pick a preset in Banks first.</div></div></main>`;
  const tiles = d.chain.map((c) =>
    `<button class="tile ${c.on ? "on" : ""} ${focusBlock === c.key ? "focus" : ""}" data-block="${c.key}"><span class="t">${esc(c.name)}<i></i></span><span class="s">${esc(c.summary)}</span></button>`).join("");
  return `
    <section class="chain" aria-label="Signal chain">${tiles}</section>
    <div class="banner">Edits change this file only; sending them to the unit is disabled for now. Save a .syx to keep them.</div>
    <main class="cards">${d.blocks.map(blockCard).join("")}${routingCard(d)}</main>`;
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
  app.innerHTML = header() + (view === "connect" ? connectView() : strip() + (view === "banks" ? banksView() : view === "preset" ? presetView() : globalView()));
  wire();
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
  on("syncone", () => selected != null && midiSend(selected, selected));
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
    el.onclick = async () => { selected = i; await refresh(); };
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

  // preset view: tiles focus a card; sliders/selects/toggles write params
  document.querySelectorAll<HTMLButtonElement>("[data-block]").forEach((b) => (b.onclick = () => {
    focusBlock = b.dataset.block!;
    render();
    document.getElementById(`card-${focusBlock}`)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }));
  const edit = async (e: Edit) => {
    if (selected == null) return;
    try {
      detail = await invoke<Detail>("edit_preset", { index: selected, edit: e });
      bank.dirty = [...new Set([...bank.dirty, selected])];
      render();
    } catch (err) {
      setStatus(String(err), true);
    }
  };
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
