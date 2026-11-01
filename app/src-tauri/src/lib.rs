//! Tauri backend: holds the open bank in memory and exposes commands to the UI.
//! All protocol work is delegated to `gsystem-protocol`; MIDI goes through
//! `midir`. Nothing here has been run against hardware yet.

use gsystem_protocol::preset::{slot_label, Preset};
use gsystem_protocol::sysex::Message;
use gsystem_protocol::{syx, vyzor, Bank};
use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::State;

#[derive(Default)]
struct AppState {
    bank: Mutex<Bank>,
    path: Mutex<Option<String>>,
    /// Slots changed since the last open / sync, for the amber "unsynced" dot.
    dirty: Mutex<std::collections::BTreeSet<usize>>,
}

#[derive(Serialize)]
struct SlotView {
    index: usize,
    label: String,
    name: Option<String>,
}

#[derive(Serialize)]
struct BankView {
    path: Option<String>,
    occupied: usize,
    dirty: Vec<usize>,
    slots: Vec<SlotView>,
}

fn view(bank: &Bank, path: &Option<String>, dirty: &std::collections::BTreeSet<usize>) -> BankView {
    BankView {
        path: path.clone(),
        occupied: bank.occupied(),
        dirty: dirty.iter().copied().collect(),
        slots: bank
            .slots
            .iter()
            .enumerate()
            .map(|(i, s)| SlotView { index: i, label: slot_label(i), name: s.as_ref().map(|p| p.name()) })
            .collect(),
    }
}

#[tauri::command]
fn get_bank(state: State<AppState>) -> BankView {
    view(&state.bank.lock().unwrap(), &state.path.lock().unwrap(), &state.dirty.lock().unwrap())
}

#[tauri::command]
fn open_bank(path: String, state: State<AppState>) -> Result<BankView, String> {
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let bank = vyzor::load_bank(&bytes)
        .or_else(|_| syx::load_bank(&bytes))
        .map_err(|e| format!("Not a G-System bank: {e}"))?;
    *state.bank.lock().unwrap() = bank;
    *state.path.lock().unwrap() = Some(path);
    state.dirty.lock().unwrap().clear();
    Ok(get_bank(state))
}

#[tauri::command]
fn save_syx(path: String, state: State<AppState>) -> Result<(), String> {
    let bank = state.bank.lock().unwrap();
    std::fs::write(&path, syx::save_bank(&bank, 0)).map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_slot(index: usize, name: String, state: State<AppState>) -> Result<BankView, String> {
    {
        let mut bank = state.bank.lock().unwrap();
        let slot = bank.slots.get_mut(index).and_then(|s| s.as_mut()).ok_or("empty slot")?;
        slot.set_name(&name);
    }
    state.dirty.lock().unwrap().insert(index);
    Ok(get_bank(state))
}

#[tauri::command]
fn swap_slots(a: usize, b: usize, state: State<AppState>) -> BankView {
    state.bank.lock().unwrap().swap(a, b);
    state.dirty.lock().unwrap().extend([a, b]);
    get_bank(state)
}

#[tauri::command]
fn copy_slot(from: usize, to: usize, state: State<AppState>) -> Result<BankView, String> {
    {
        let mut bank = state.bank.lock().unwrap();
        let p = bank.get(from).cloned().ok_or("empty slot")?;
        bank.set(to, p);
    }
    state.dirty.lock().unwrap().insert(to);
    Ok(get_bank(state))
}

#[tauri::command]
fn clear_slot(index: usize, state: State<AppState>) -> BankView {
    if let Some(s) = state.bank.lock().unwrap().slots.get_mut(index) {
        *s = None;
    }
    state.dirty.lock().unwrap().insert(index);
    get_bank(state)
}

/// Raw parameter view for the inspector pane: name + int24 params + hex.
#[tauri::command]
fn slot_detail(index: usize, state: State<AppState>) -> Result<serde_json::Value, String> {
    let bank = state.bank.lock().unwrap();
    let p: &Preset = bank.get(index).ok_or("empty slot")?;
    Ok(serde_json::json!({
        "index": index, "label": slot_label(index), "name": p.name(),
        "params_i24": p.params_i24(),
        "raw_hex": p.raw.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "),
    }))
}

/// Write one int24 parameter by slot index. The UI's parameter table decides
/// which index means what; this stays dumb on purpose.
#[tauri::command]
fn set_param(index: usize, param: usize, value: i32, state: State<AppState>) -> Result<(), String> {
    let mut bank = state.bank.lock().unwrap();
    let p = bank.slots.get_mut(index).and_then(|s| s.as_mut()).ok_or("empty slot")?;
    if param >= gsystem_protocol::preset::PARAM_COUNT {
        return Err("param index out of range".into());
    }
    p.set_param_i24(param, value);
    state.dirty.lock().unwrap().insert(index);
    Ok(())
}

/// Move `from` to `to`, shifting the slots in between (Vyzor's shift-drag "insert").
#[tauri::command]
fn insert_slot(from: usize, to: usize, state: State<AppState>) -> BankView {
    {
        let mut bank = state.bank.lock().unwrap();
        if from < bank.slots.len() && to < bank.slots.len() && from != to {
            let p = bank.slots.remove(from);
            bank.slots.insert(to, p);
            let (lo, hi) = if from < to { (from, to) } else { (to, from) };
            state.dirty.lock().unwrap().extend(lo..=hi);
        }
    }
    get_bank(state)
}

// ---- MIDI -----------------------------------------------------------------

#[derive(Serialize)]
struct Ports {
    inputs: Vec<String>,
    outputs: Vec<String>,
}

#[tauri::command]
fn midi_ports() -> Result<Ports, String> {
    let inp = midir::MidiInput::new("gsystem-editor").map_err(|e| e.to_string())?;
    let out = midir::MidiOutput::new("gsystem-editor").map_err(|e| e.to_string())?;
    Ok(Ports {
        inputs: inp.ports().iter().filter_map(|p| inp.port_name(p).ok()).collect(),
        outputs: out.ports().iter().filter_map(|p| out.port_name(p).ok()).collect(),
    })
}

/// Request slots `from..=to` from the unit and merge them into the open bank.
/// Returns the slot indices that answered.
#[tauri::command]
fn midi_backup(input: String, output: String, from: usize, to: usize, state: State<AppState>) -> Result<Vec<usize>, String> {
    let mut inp = midir::MidiInput::new("gsystem-editor").map_err(|e| e.to_string())?;
    inp.ignore(midir::Ignore::None);
    let in_port = inp
        .ports()
        .into_iter()
        .find(|p| inp.port_name(p).map(|n| n == input).unwrap_or(false))
        .ok_or("input port not found")?;
    let out = midir::MidiOutput::new("gsystem-editor").map_err(|e| e.to_string())?;
    let out_port = out
        .ports()
        .into_iter()
        .find(|p| out.port_name(p).map(|n| n == output).unwrap_or(false))
        .ok_or("output port not found")?;
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let _conn_in = inp
        .connect(&in_port, "in", move |_, bytes, _| { let _ = tx.send(bytes.to_vec()); }, ())
        .map_err(|e| e.to_string())?;
    let mut conn_out = out.connect(&out_port, "out").map_err(|e| e.to_string())?;

    let mut got = Vec::new();
    for slot in from..=to.min(199) {
        conn_out.send(&Message::preset_request(0, slot as u8).encode()).map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_millis(3500);
        while let Ok(bytes) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            if let Ok(Message::PresetDump { data, .. }) = Message::parse(&bytes) {
                if let Some(p) = Preset::from_raw(data) {
                    state.bank.lock().unwrap().set(slot, p);
                    state.dirty.lock().unwrap().remove(&slot);
                    got.push(slot);
                }
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(2000)); // editor's recorded settle time
    }
    Ok(got)
}

/// Send the occupied slots in `from..=to` to the unit.
#[tauri::command]
fn midi_send(output: String, from: usize, to: usize, state: State<AppState>) -> Result<usize, String> {
    let out = midir::MidiOutput::new("gsystem-editor").map_err(|e| e.to_string())?;
    let out_port = out
        .ports()
        .into_iter()
        .find(|p| out.port_name(p).map(|n| n == output).unwrap_or(false))
        .ok_or("output port not found")?;
    let mut conn = out.connect(&out_port, "out").map_err(|e| e.to_string())?;
    let bank = state.bank.lock().unwrap();
    let mut n = 0;
    for slot in from..=to.min(199) {
        if let Some(p) = bank.get(slot) {
            conn.send(&p.to_message(0, slot as u8).encode()).map_err(|e| e.to_string())?;
            state.dirty.lock().unwrap().remove(&slot);
            n += 1;
            std::thread::sleep(Duration::from_millis(150));
        }
    }
    Ok(n)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_bank, open_bank, save_syx, rename_slot, swap_slots, copy_slot, clear_slot, insert_slot, set_param,
            slot_detail, midi_ports, midi_backup, midi_send
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
