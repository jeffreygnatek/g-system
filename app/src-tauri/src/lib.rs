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
            .map(|(i, s)| SlotView {
                index: i,
                label: slot_label(i),
                name: s.as_ref().map(|p| p.name()),
            })
            .collect(),
    }
}

#[tauri::command]
fn get_bank(state: State<AppState>) -> BankView {
    view(
        &state.bank.lock().unwrap(),
        &state.path.lock().unwrap(),
        &state.dirty.lock().unwrap(),
    )
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
        let slot = bank
            .slots
            .get_mut(index)
            .and_then(|s| s.as_mut())
            .ok_or("empty slot")?;
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
    let p = bank
        .slots
        .get_mut(index)
        .and_then(|s| s.as_mut())
        .ok_or("empty slot")?;
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

// ---- Preset decoding / editing (gsystem_protocol::view) ----

use gsystem_protocol::view::{self, Edit, PresetView};

#[tauri::command]
fn preset_view(index: usize, state: State<AppState>) -> Result<PresetView, String> {
    let bank = state.bank.lock().unwrap();
    let p = bank.get(index).ok_or("empty slot")?;
    Ok(view::preset_view(index, p))
}

#[tauri::command]
fn edit_preset(index: usize, edit: Edit, state: State<AppState>) -> Result<PresetView, String> {
    let mut bank = state.bank.lock().unwrap();
    let p = bank
        .slots
        .get_mut(index)
        .and_then(|s| s.as_mut())
        .ok_or("empty slot")?;
    view::apply(p, edit)?;
    let v = view::preset_view(index, p);
    drop(bank);
    state.dirty.lock().unwrap().insert(index);
    Ok(v)
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
        inputs: inp
            .ports()
            .iter()
            .filter_map(|p| inp.port_name(p).ok())
            .collect(),
        outputs: out
            .ports()
            .iter()
            .filter_map(|p| out.port_name(p).ok())
            .collect(),
    })
}

/// One open in/out pair, with incoming bytes reassembled into whole SysEx messages.
struct Midi {
    _in: midir::MidiInputConnection<()>,
    rx: std::sync::mpsc::Receiver<Vec<u8>>,
    out: midir::MidiOutputConnection,
}

impl Midi {
    fn open(input: &str, output: &str) -> Result<Midi, String> {
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
        let mut buf = Vec::new();
        let conn = inp
            .connect(
                &in_port,
                "in",
                move |_, bytes, _| {
                    for &b in bytes {
                        if b == 0xF0 {
                            buf.clear();
                        }
                        if !buf.is_empty() || b == 0xF0 {
                            buf.push(b);
                            if b == 0xF7 {
                                let _ = tx.send(std::mem::take(&mut buf));
                            }
                        }
                    }
                },
                (),
            )
            .map_err(|e| e.to_string())?;
        let out = out.connect(&out_port, "out").map_err(|e| e.to_string())?;
        Ok(Midi { _in: conn, rx, out })
    }

    fn wait<T>(&self, timeout: Duration, mut pick: impl FnMut(&[u8]) -> Option<T>) -> Option<T> {
        let deadline = Instant::now() + timeout;
        while let Ok(b) = self
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            if let Some(v) = pick(&b) {
                return Some(v);
            }
        }
        None
    }
}

/// Read the unit's global settings and footswitch assignments (55 47
/// requests, read-only).
#[tauri::command]
fn midi_globals(input: String, output: String) -> Result<view::GlobalsView, String> {
    let mut m = Midi::open(&input, &output)?;
    let mut read = std::collections::HashMap::new();
    for (block, first, count) in gsystem_protocol::globals::GLOBAL_RANGES {
        while m.rx.try_recv().is_ok() {}
        m.out
            .send(
                &Message::RequestParams {
                    device: 0,
                    block,
                    first,
                    count,
                }
                .encode(),
            )
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_millis(1500);
        while (first..first + count).any(|p| !read.contains_key(&(block, p))) {
            let Ok(bytes) =
                m.rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            else {
                break;
            };
            if let Ok(Message::SetParam {
                block: b,
                param,
                values,
                ..
            }) = Message::parse(&bytes)
            {
                for (i, v) in values.into_iter().enumerate() {
                    read.insert((b, param + i as u16), v);
                }
            }
        }
    }
    if read.is_empty() {
        return Err("The unit didn't answer the globals request".into());
    }
    Ok(view::globals_view(&read))
}

/// Ask the unit to identify itself (Universal Device Inquiry).
#[tauri::command]
fn midi_identify(input: String, output: String) -> Result<String, String> {
    let mut m = Midi::open(&input, &output)?;
    m.out
        .send(&gsystem_protocol::sysex::DEVICE_INQUIRY)
        .map_err(|e| e.to_string())?;
    m.wait(
        Duration::from_millis(1500),
        gsystem_protocol::sysex::parse_identity,
    )
    .ok_or_else(|| "No G-System answered on these ports".to_string())
}

/// Read user slots `from..=to` from the unit into the open bank.
/// Returns the slots that answered. Empty slots never answer.
#[tauri::command]
fn midi_backup(
    input: String,
    output: String,
    from: usize,
    to: usize,
    state: State<AppState>,
) -> Result<Vec<usize>, String> {
    let mut m = Midi::open(&input, &output)?;
    let mut got = Vec::new();
    for slot in from..=to.min(199) {
        while m.rx.try_recv().is_ok() {}
        m.out
            .send(&Message::preset_request(0, slot).encode())
            .map_err(|e| e.to_string())?;
        let want = gsystem_protocol::sysex::slot_to_number(slot);
        // A stored preset answers in ~0.77 s.
        let p = m.wait(Duration::from_millis(1100), |b| match Message::parse(b) {
            Ok(Message::PresetDump { number, data, .. }) if number == want => {
                Preset::from_raw(data)
            }
            _ => None,
        });
        if let Some(p) = p {
            state.bank.lock().unwrap().set(slot, p);
            state.dirty.lock().unwrap().remove(&slot);
            got.push(slot);
        }
    }
    Ok(got)
}

/// Writing presets to the unit is disabled in the app: the store sequence the
/// original editor uses has been observed to freeze G-System units
/// intermittently. The CLI's `gsys store` can do it, with a read-back check,
/// for people who accept that risk. See PROTOCOL.md "Storing presets".
#[tauri::command]
fn midi_send(_output: String, _from: usize, _to: usize) -> Result<usize, String> {
    Err("Writing presets to the unit is disabled for now: the store sequence can freeze the G-System. Save a .syx instead; see PROTOCOL.md.".into())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_bank,
            open_bank,
            save_syx,
            rename_slot,
            swap_slots,
            copy_slot,
            clear_slot,
            insert_slot,
            set_param,
            slot_detail,
            preset_view,
            edit_preset,
            midi_ports,
            midi_identify,
            midi_globals,
            midi_backup,
            midi_send
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
