import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { BLOCKS, PRESET_MISC, TEMPO_DIVISIONS, formatValue, type Block, type Param } from "./model";

type Slot = { index: number; label: string; name: string | null };
type BankView = { path: string | null; occupied: number; dirty: number[]; slots: Slot[] };
type Detail = { index: number; label: string; name: string; params_i24: number[]; raw_hex: string };
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
let unit: { connected: boolean; progress?: string } = { connected: false };
let focusBlock: string | null = null;
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
    detail = await invoke<Detail>("slot_detail", { index: selected });
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
    portIn ||= ports.inputs.find((p) => /um-one|g-system/i.test(p)) ?? ports.inputs[0] ?? "";
    portOut ||= ports.outputs.find((p) => /um-one|g-system/i.test(p)) ?? ports.outputs[0] ?? "";
  } catch (e) {
    setStatus(String(e), true);
  }
  render();
}
async function midiBackup(from: number, to: number) {
  unit.progress = `Reading slots ${from}–${to}…`;
  render();
  try {
    const got = await invoke<number[]>("midi_backup", { input: portIn, output: portOut, from, to });
    unit = { connected: got.length > 0 };
    await refresh();
    setStatus(got.length ? `Received ${got.length} presets` : "No response from unit", !got.length);
  } catch (e) {
    unit = { connected: false };
    setStatus(String(e), true);
  }
  render();
}
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
function blockOn(b: Block): boolean | null {
  if (!detail || b.enabledIndex == null) return null;
  return detail.params_i24[b.enabledIndex] !== 0;
}
function blockType(b: Block): string {
  if (!detail || b.typeIndex == null) return b.types.length ? "—" : "";
  return b.types[detail.params_i24[b.typeIndex]] ?? "?";
}
function strip(): string {
  const s = selected != null ? bank.slots[selected] : null;
  const bankNo = selected != null ? Math.floor(selected / 5) : null;
  const sw = [1, 2, 3, 4, 5].map((n) => `<span class="fsw ${selected != null && selected % 5 === n - 1 ? "on" : ""}">${n}</span>`).join("");
  const tags = BLOCKS.filter((b) => b.types.length || b.id === "boost")
    .map((b) => { const on = blockOn(b); return `<span class="tag ${on ? "on" : ""}">${esc(b.name.toUpperCase())}</span>`; })
    .join("");
  return `<section class="strip" aria-label="Unit status">
    <div class="lcd"><span class="slot">${s ? s.label : "--·-"}</span><span class="sep">│</span><span>${s?.name ? esc(s.name) : "<span class='dim'>no preset</span>"}</span></div>
    <div class="switches"><span class="eyebrow">Bank ${bankNo != null ? String(bankNo).padStart(2, "0") : "--"}</span>${sw}</div>
    <div class="blocks">${tags}</div>
    <div class="tempo"><span class="muted">TEMPO</span><span>${tempoText()}</span><button class="btn sm">TAP</button></div>
  </section>`;
}
function tempoText() {
  const p = PRESET_MISC.tempo[0];
  return detail && p.index != null ? formatValue(p, detail.params_i24[p.index]) : "— BPM";
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
          <button class="btn sm" id="getall" ${unit.connected ? "" : "disabled"}>Get from unit</button>
          <button class="btn sm amber" id="syncdirty" ${unit.connected && bank.dirty.length ? "" : "disabled"}>Sync ${bank.dirty.length || ""} changed →</button>
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
  const mini = BLOCKS.filter((b) => b.types.length).map((b) => {
    const on = blockOn(b);
    return `<div class="${on ? "on" : ""}"><div class="k">${esc(b.name)}</div><div class="v">${on === null ? "—" : on ? esc(blockType(b)) : "Off"}</div></div>`;
  }).join("");
  const dirty = dirtySet().has(selected!);
  return `<aside class="inspector">
    <div>
      <div class="eyebrow mono" style="margin-bottom:6px">${s.label} · ${selected! < 100 ? "FACTORY" : "USER"} BANK</div>
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
        <button class="btn" id="audition" ${unit.connected ? "" : "disabled"}>Audition on unit</button>
        <button class="btn" id="syncone" ${unit.connected ? "" : "disabled"}>Sync this slot</button>
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
  return `<div class="note" style="margin-top:auto"><b>Unit vs. file</b>${d.length ? `${d.length} preset(s) differ from the unit (${list}). Nothing is written to the G-System until you press Sync.` : "In sync with the unit."}</div>`;
}

// ---------------------------------------------------------------- preset view
// Until any field is mapped the banner explains it once; per-row tags only
// appear when the map is partial, to point at what's still missing.
const ANY_MAPPED = BLOCKS.some((b) => b.enabledIndex != null || b.typeIndex != null || b.params.some((p) => p.index != null));
const unmapTag = (text = "unmapped") => (ANY_MAPPED ? `<span class="unmap">${text}</span>` : "");
function paramRow(p: Param, slot: number | null): string {
  const mapped = p.index != null && detail != null;
  const raw = mapped ? detail!.params_i24[p.index!] : null;
  const val = raw == null ? "—" : formatValue(p, raw);
  const tag = p.index == null ? unmapTag() : "";
  if (p.options) {
    const opts = p.options.map((o, i) => `<option value="${i}" ${raw === i ? "selected" : ""}>${esc(o)}</option>`).join("");
    return `<div class="prow ${mapped ? "" : "unmapped"}"><label>${esc(p.label)}${tag}</label><select class="sel" data-slot="${slot}" data-param="${p.index}" ${mapped ? "" : "disabled"}>${opts}</select><span class="val"></span></div>`;
  }
  const min = Math.round(p.min / (p.scale ?? 1) * (p.scale ? 1 : 1));
  return `<div class="prow ${mapped ? "" : "unmapped"}">
    <label>${esc(p.label)}${tag}</label>
    <input type="range" min="${p.min / (p.scale ?? 1)}" max="${p.max / (p.scale ?? 1)}" step="1" value="${raw ?? min}" data-slot="${slot}" data-param="${p.index}" ${mapped ? "" : "disabled"} aria-label="${esc(p.label)}">
    <span class="val">${esc(val)}</span>
  </div>`;
}
function tempoChips(): string {
  return `<div class="prow"><label>Tempo${unmapTag()}</label><div class="chips">${TEMPO_DIVISIONS.map((d, i) => `<button class="chip ${i === 0 ? "on" : ""}" disabled>${esc(d)}</button>`).join("")}</div><span class="val"></span></div>`;
}
function blockCard(b: Block): string {
  const on = blockOn(b);
  const type = blockType(b);
  const typeSel = b.types.length
    ? `<select class="sel" data-type="${b.typeIndex}" ${b.typeIndex == null ? "disabled" : ""} aria-label="${esc(b.title)} type">${b.types.map((t, i) => `<option value="${i}" ${t === type ? "selected" : ""}>${esc(t)}</option>`).join("")}</select>`
    : `<span style="margin-left:auto"></span>`;
  return `<section class="card ${on === false ? "off" : ""} ${focusBlock === b.id ? "focus" : ""}" id="card-${b.id}" aria-label="${esc(b.title)}">
    <div class="hd"><span class="ttl">${esc(b.title)}${b.enabledIndex == null ? unmapTag("on/off unmapped") : ""}</span>${typeSel}<button class="toggle ${on ? "on" : ""}" data-toggle="${b.enabledIndex}" ${b.enabledIndex == null ? "disabled" : ""} aria-label="${on ? "On" : "Off"}"></button></div>
    <div class="bd">${b.params.map((p) => paramRow(p, selected)).join("")}${b.tempo ? tempoChips() : ""}</div>
  </section>`;
}
function routingCard(): string {
  const loops = BLOCKS.filter((b) => b.id.startsWith("loop")).map((b) => {
    const on = blockOn(b);
    return `<div class="${on ? "on" : ""}"><div class="k">${esc(b.name)}</div><div class="v">${on === null ? "—" : on ? "On" : "Off"}</div></div>`;
  }).join("");
  const r = PRESET_MISC.routing;
  const rawR = detail && r.index != null ? detail.params_i24[r.index] : null;
  return `<section class="card" aria-label="Routing and loops">
    <div class="hd"><span class="ttl">Routing · Loops · Gate</span>
      <select class="sel" data-param="${r.index}" data-slot="${selected}" ${r.index == null ? "disabled" : ""} aria-label="Routing">${r.options.map((o, i) => `<option value="${i}" ${rawR === i ? "selected" : ""}>${esc(o)}</option>`).join("")}</select></div>
    <div class="bd">
      <div class="loops4">${loops}</div>
      ${PRESET_MISC.gate.map((p) => paramRow(p, selected)).join("")}
      ${PRESET_MISC.tempo.map((p) => paramRow(p, selected)).join("")}
      <div class="note">Changes are sent to the unit's edit buffer as you make them. Nothing is written to a bank slot until Store.</div>
    </div>
  </section>`;
}
function presetView(): string {
  const s = selected != null ? bank.slots[selected] : null;
  if (!s || !detail) return `<main class="body"><div class="centered"><div class="note">Pick a preset in Banks first.</div></div></main>`;
  const tiles = BLOCKS.map((b) => {
    const on = blockOn(b);
    return `<button class="tile ${on ? "on" : ""} ${focusBlock === b.id ? "focus" : ""}" data-block="${b.id}"><span class="t">${esc(b.name)}<i></i></span><span class="s">${esc(blockType(b) || (on === null ? "—" : on ? "On" : "Off"))}</span></button>`;
  }).join("");
  const cards = BLOCKS.filter((b) => b.types.length || b.id === "boost").map(blockCard).join("") + routingCard();
  const mappedCount = BLOCKS.flatMap((b) => b.params).filter((p) => p.index != null).length;
  return `
    <section class="chain" aria-label="Signal chain">${tiles}</section>
    ${mappedCount === 0 ? `<div class="banner">Parameter positions in the dump haven't been mapped yet, so controls are shown disabled. Values live in <span class="mono">app/src/model.ts</span>; fill in each param's <span class="mono">index</span> as PROTOCOL.md confirms it.</div>` : ""}
    <main class="cards">${cards}</main>`;
}

// ---------------------------------------------------------------- global view
function globalView(): string {
  const card = (title: string, rows: [string, string][], note?: string) =>
    `<section class="card"><div class="hd"><span class="ttl">${title}</span></div><div class="bd"><div class="kv">${rows.map(([k, v]) => `<span class="k">${k}</span><span class="v">${v}</span>`).join("")}</div>${note ? `<div class="note">${note}</div>` : ""}</div></section>`;
  const u = "—";
  return `
    <div class="banner">Global settings come from the unit's <span class="mono">22 05</span> system dump, which isn't decoded yet. This page shows the layout; values fill in once the dump format is confirmed from a live capture.</div>
    <main class="gcols">
      ${card("MIDI", [["Channel", u], ["Program change in", u], ["Program change out", u], ["Encoder A / B / C CC", u], ["Tap tempo CC", u], ["SysEx ID", u]])}
      ${card("Tuner", [["Output", u], ["Reference", u], ["Mode", u]])}
      ${card("Global levels", [["Routing", u], ["Guitar input", u], ["Input sensitivity", u], ["Loop level", u], ["Loop headroom", u], ["Volume", u], ["Output level", u], ["Boost max", u], ["Kill dry", u]])}
      ${card("Pedals", [["Master volume assignment", u], ["GFX1 volume pedal", u], ["GFX1 expression pedal", u], ["Controller volume pedal", u], ["Controller expression pedal", u], ["Volume master", u], ["Expression master", u]])}
      ${card("Locks", [["Boost", u], ["EQ", u], ["Insert", u], ["Relay", u], ["Routing", u], ["Factory bank select", u]])}
      ${card("Switches", [["1–5", u], ["6–12", u], ["13–18", u]], "Per-footswitch assignment: effect toggle, preset, or MIDI CC.")}
      ${card("Program map", [["Banks 00–19", u], ["Banks 20–39", u]], "Incoming MIDI program → preset slot. Shown as a 200-row table once decoded.")}
    </main>`;
}

// ---------------------------------------------------------------- connect view
function connectView(): string {
  const opts = (list: string[], cur: string) => list.length ? list.map((p) => `<option ${p === cur ? "selected" : ""}>${esc(p)}</option>`).join("") : `<option value="">(no ports found)</option>`;
  const busy = unit.progress;
  return `<main class="body"><div class="centered"><div class="connect">
    <div><h1>Connect your G-System</h1><p>Over 5-pin MIDI through any USB-MIDI interface. The unit's own USB port is not supported yet. You can also skip this and work with files only.</p></div>
    <section class="panel">
      <div class="two">
        <div class="field"><label class="eyebrow" for="pin">MIDI in (from unit)</label><select id="pin">${opts(ports.inputs, portIn)}</select></div>
        <div class="field"><label class="eyebrow" for="pout">MIDI out (to unit)</label><select id="pout">${opts(ports.outputs, portOut)}</select></div>
      </div>
      <div style="display:flex;flex-direction:column;gap:10px">
        <div class="step ${ports.inputs.length ? "ok" : ""}"><span class="ic">${ports.inputs.length ? "✓" : "1"}</span><span>MIDI ports found</span><span class="r">${ports.inputs.length ? esc(portIn) : "none"}</span></div>
        <div class="step ${unit.connected ? "ok" : busy ? "busy" : ""}"><span class="ic">${unit.connected ? "✓" : busy ? "" : "2"}</span><span>${unit.connected ? "G-System answered" : busy ? busy : "Read the user bank from the unit"}</span><span class="r">${unit.connected ? `${bank.occupied} presets` : ""}</span></div>
      </div>
      <div class="rowbtns">
        <button class="btn primary" id="connect" ${ports.inputs.length && !busy ? "" : "disabled"}>${busy ? "Reading…" : "Connect & read user bank"}</button>
        <button class="btn" id="rescan">Rescan ports</button>
        <a href="#" id="offline">Work offline with a file instead</a>
      </div>
      <div class="status ${statusErr ? "err" : ""}">${esc(status)}</div>
    </section>
    <div class="note"><b>If nothing answers</b>Check both MIDI cables are crossed (interface OUT → unit IN, unit OUT → interface IN), and that no other app has the interface open. The first read of a full bank takes a few minutes.</div>
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
  on("getall", () => midiBackup(100, 199));
  on("syncdirty", syncDirty);
  on("edit", () => go("preset"));
  on("audition", () => selected != null && midiSend(selected, selected));
  on("syncone", () => selected != null && midiSend(selected, selected));
  on("clear", async () => { if (selected != null) await refresh(await invoke<BankView>("clear_slot", { index: selected })); });
  on("copyto", () => setStatus("Drag the preset to a slot while holding ⌥ to copy it"));
  on("toconnect", () => go("connect"));
  on("offline", () => (bank.occupied ? go("banks") : doOpen()));
  on("rescan", loadPorts);
  on("connect", async () => { await midiBackup(100, 199); if (unit.connected) go("banks"); });
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
  document.querySelectorAll<HTMLInputElement | HTMLSelectElement>("[data-param]").forEach((el) => {
    const param = Number(el.dataset.param);
    if (!Number.isFinite(param)) return;
    const slot = Number(el.dataset.slot);
    const write = async () => {
      const value = Number(el.value);
      await invoke("set_param", { index: slot, param, value });
      if (detail) detail.params_i24[param] = value;
      const val = el.parentElement?.querySelector<HTMLElement>(".val");
      const p = findParam(param);
      if (val && p) val.textContent = formatValue(p, value);
      bank.dirty = [...new Set([...bank.dirty, slot])];
    };
    el.oninput = () => { const val = el.parentElement?.querySelector<HTMLElement>(".val"); const p = findParam(param); if (val && p) val.textContent = formatValue(p, Number(el.value)); };
    el.onchange = write;
  });
  document.querySelectorAll<HTMLButtonElement>("[data-toggle]").forEach((b) => (b.onclick = async () => {
    const param = Number(b.dataset.toggle);
    if (!Number.isFinite(param) || selected == null || !detail) return;
    const value = detail.params_i24[param] ? 0 : 1;
    await invoke("set_param", { index: selected, param, value });
    await refresh();
  }));
  document.querySelectorAll<HTMLSelectElement>("[data-type]").forEach((el) => (el.onchange = async () => {
    const param = Number(el.dataset.type);
    if (!Number.isFinite(param) || selected == null) return;
    await invoke("set_param", { index: selected, param, value: Number(el.value) });
    await refresh();
  }));
}
function findParam(index: number): Param | undefined {
  return [...BLOCKS.flatMap((b) => b.params), ...PRESET_MISC.gate, ...PRESET_MISC.tempo].find((p) => p.index === index);
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
