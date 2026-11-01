import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";

type Slot = { index: number; label: string; name: string | null };
type BankView = { path: string | null; occupied: number; slots: Slot[] };
type Detail = { index: number; label: string; name: string; params_i24: number[]; raw_hex: string };
type Ports = { inputs: string[]; outputs: string[] };

const app = document.getElementById("app")!;
let bank: BankView = { path: null, occupied: 0, slots: [] };
let selected: number | null = null;
let dragFrom: number | null = null;
let ports: Ports = { inputs: [], outputs: [] };
let status = "";
let statusErr = false;

function setStatus(s: string, err = false) {
  status = s;
  statusErr = err;
  const el = document.querySelector<HTMLElement>(".status");
  if (el) {
    el.textContent = s;
    el.classList.toggle("err", err);
  }
}

async function refresh(v?: BankView) {
  bank = v ?? (await invoke<BankView>("get_bank"));
  render();
}

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
    await refresh(await invoke<BankView>("open_bank", { path }));
    setStatus(`Loaded ${bank.occupied} presets`);
  } catch (e) {
    setStatus(String(e), true);
  }
}

async function doSave() {
  const path = await save({ defaultPath: "gsystem-bank.syx", filters: [{ name: "SysEx", extensions: ["syx"] }] });
  if (!path) return;
  try {
    await invoke("save_syx", { path });
    setStatus(`Saved ${bank.occupied} presets to ${path}`);
  } catch (e) {
    setStatus(String(e), true);
  }
}

async function select(i: number) {
  selected = i;
  render();
  if (bank.slots[i]?.name == null) return;
  const d = await invoke<Detail>("slot_detail", { index: i });
  renderDetail(d);
}

function startRename(i: number, cell: HTMLElement) {
  const slot = bank.slots[i];
  if (!slot || slot.name == null) return;
  const input = document.createElement("input");
  input.value = slot.name;
  input.maxLength = 20;
  cell.replaceChildren(input);
  input.focus();
  input.select();
  const commit = async () => {
    try {
      await refresh(await invoke<BankView>("rename_slot", { index: i, name: input.value }));
      await select(i);
    } catch (e) {
      setStatus(String(e), true);
      render();
    }
  };
  input.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter") commit();
    if (ev.key === "Escape") render();
  });
  input.addEventListener("blur", commit);
}

function render() {
  const rows: string[] = [];
  const header = `<tr><th class="bank-col">Bank</th>${[1, 2, 3, 4, 5].map((p) => `<th>Preset ${p}</th>`).join("")}</tr>`;
  for (let b = 0; b < 40; b++) {
    if (b === 0) rows.push(`<tr class="section-row"><td colspan="6">Factory 00–19</td></tr>`);
    if (b === 20) rows.push(`<tr class="section-row"><td colspan="6">User 20–39</td></tr>`);
    const cells = [0, 1, 2, 3, 4]
      .map((p) => {
        const i = b * 5 + p;
        const s = bank.slots[i];
        const name = s?.name;
        const cls = ["slot", b < 20 ? "factory" : "user", selected === i ? "selected" : ""].join(" ");
        return `<td><div class="${cls}" data-i="${i}" draggable="${name != null}">
          <span class="label">${s?.label ?? ""}</span>
          <span class="name ${name == null ? "empty" : ""}">${name == null ? "—" : esc(name)}</span></div></td>`;
      })
      .join("");
    rows.push(`<tr><td class="bank-col">${String(b).padStart(2, "0")}</td>${cells}</tr>`);
  }

  app.innerHTML = `
    <header>
      <span class="title">G-System Editor</span>
      <button id="open" class="primary">Open…</button>
      <button id="save" ${bank.occupied ? "" : "disabled"}>Save .syx…</button>
      <span class="path">${bank.path ? esc(bank.path) : "No bank loaded — open a .SQS, .syx or .Prs file"}</span>
    </header>
    <main>
      <div class="grid-wrap"><table class="bank">${header}${rows.join("")}</table></div>
      <aside>
        <div id="detail"><h2>Preset</h2><div class="hint">Select a slot. Double-click to rename, drag to move (swap).</div></div>
        <div class="midi">
          <h2>Unit (MIDI)</h2>
          <label>Input</label><select id="min">${opts(ports.inputs)}</select>
          <label>Output</label><select id="mout">${opts(ports.outputs)}</select>
          <div class="actions">
            <button id="ports">Refresh ports</button>
            <button id="backup" ${ports.inputs.length ? "" : "disabled"}>Backup user bank</button>
            <button id="sendsel" ${selected != null && ports.outputs.length ? "" : "disabled"}>Send selected</button>
          </div>
          <div class="status ${statusErr ? "err" : ""}">${esc(status)}</div>
        </div>
      </aside>
    </main>`;

  document.getElementById("open")!.onclick = doOpen;
  document.getElementById("save")!.onclick = doSave;
  document.getElementById("ports")!.onclick = loadPorts;
  document.getElementById("backup")!.onclick = () => midiBackup(100, 199);
  document.getElementById("sendsel")!.onclick = () => selected != null && midiSend(selected, selected);

  app.querySelectorAll<HTMLElement>(".slot").forEach((el) => {
    const i = Number(el.dataset.i);
    el.onclick = () => select(i);
    el.ondblclick = () => startRename(i, el.querySelector(".name")!);
    el.ondragstart = (ev) => {
      dragFrom = i;
      ev.dataTransfer?.setData("text/plain", String(i));
    };
    el.ondragover = (ev) => {
      ev.preventDefault();
      el.classList.add("drop-target");
    };
    el.ondragleave = () => el.classList.remove("drop-target");
    el.ondrop = async (ev) => {
      ev.preventDefault();
      el.classList.remove("drop-target");
      if (dragFrom == null || dragFrom === i) return;
      const v = ev.altKey
        ? await invoke<BankView>("copy_slot", { from: dragFrom, to: i })
        : await invoke<BankView>("swap_slots", { a: dragFrom, b: i });
      dragFrom = null;
      await refresh(v);
      await select(i);
    };
  });
  if (selected != null) app.querySelector(`.slot[data-i="${selected}"]`)?.scrollIntoView({ block: "nearest" });
}

function renderDetail(d: Detail) {
  const el = document.getElementById("detail");
  if (!el) return;
  const params = d.params_i24.map((v, k) => `${String(k).padStart(3)}: ${v}`).join("\n");
  el.innerHTML = `
    <h2>${esc(d.label)} — ${esc(d.name)}</h2>
    <div class="actions">
      <button id="rename">Rename</button>
      <button id="clear">Clear slot</button>
    </div>
    <div class="kv"><span class="k">Params</span><span>${d.params_i24.length} × int24 (unmapped)</span></div>
    <div class="params">${params}</div>`;
  document.getElementById("rename")!.onclick = () => {
    const cell = app.querySelector<HTMLElement>(`.slot[data-i="${d.index}"] .name`);
    if (cell) startRename(d.index, cell);
  };
  document.getElementById("clear")!.onclick = async () => {
    await refresh(await invoke<BankView>("clear_slot", { index: d.index }));
  };
}

async function loadPorts() {
  try {
    ports = await invoke<Ports>("midi_ports");
    setStatus(ports.inputs.length ? `${ports.inputs.length} MIDI input(s)` : "No MIDI ports found");
  } catch (e) {
    setStatus(String(e), true);
  }
  render();
}

function sel(id: string): string {
  return (document.getElementById(id) as HTMLSelectElement | null)?.value ?? "";
}

async function midiBackup(from: number, to: number) {
  setStatus(`Requesting slots ${from}–${to}… (about 2 s per preset)`);
  try {
    const got = await invoke<number[]>("midi_backup", { input: sel("min"), output: sel("mout"), from, to });
    await refresh();
    setStatus(got.length ? `Received ${got.length} presets` : "No response from unit", !got.length);
  } catch (e) {
    setStatus(String(e), true);
  }
}

async function midiSend(from: number, to: number) {
  try {
    const n = await invoke<number>("midi_send", { output: sel("mout"), from, to });
    setStatus(`Sent ${n} preset(s)`);
  } catch (e) {
    setStatus(String(e), true);
  }
}

function esc(s: string) {
  return s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
}
function opts(list: string[]) {
  return list.length ? list.map((p) => `<option>${esc(p)}</option>`).join("") : `<option value="">(none)</option>`;
}

render();
refresh().catch(() => {});
