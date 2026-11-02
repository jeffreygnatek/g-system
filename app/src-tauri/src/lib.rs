//! Tauri backend: holds the open bank in memory and exposes commands to the UI.
//! All protocol work is delegated to `gsystem-protocol`; MIDI goes through
//! `midir`. MIDI commands are async and do their work on a blocking thread,
//! so the window stays responsive while the unit answers (about a second per
//! preset when reading a bank).

use gsystem_protocol::preset::{slot_label, Preset};
use gsystem_protocol::sysex::Message;
use gsystem_protocol::{syx, vyzor, Bank};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

/// Run blocking MIDI work off the main thread.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Default)]
struct AppState {
    bank: Mutex<Bank>,
    path: Mutex<Option<String>>,
    /// Slots changed since the last open / sync, for the amber "unsynced" dot.
    dirty: Mutex<std::collections::BTreeSet<usize>>,
    /// Open while Live mode is on: edits go straight to the unit's edit buffer.
    live: Mutex<Option<Midi>>,
    /// Set by `midi_cancel` to stop a bank read between slots.
    cancel: AtomicBool,
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
    let before = p.clone();
    view::apply(p, edit)?;
    let v = view::preset_view(index, p);
    let msgs = gsystem_protocol::live::changes(&before, p);
    drop(bank);
    state.dirty.lock().unwrap().insert(index);
    if let Some(m) = state.live.lock().unwrap().as_mut() {
        send_all(m, &msgs)?;
    }
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
    // macOS creates no MIDI port for the G-System's own USB connection. When
    // it's plugged in and no MIDI port carries its name, offer the direct USB
    // connection first (Linux and Windows usually make a port; use that).
    let mut ports = Ports {
        inputs: Vec::new(),
        outputs: Vec::new(),
    };
    let inp = midir::MidiInput::new("gsystem-editor");
    let out = midir::MidiOutput::new("gsystem-editor");
    let midi_err = match (inp, out) {
        (Ok(inp), Ok(out)) => {
            ports
                .inputs
                .extend(inp.ports().iter().filter_map(|p| inp.port_name(p).ok()));
            ports
                .outputs
                .extend(out.ports().iter().filter_map(|p| out.port_name(p).ok()));
            None
        }
        (Err(e), _) => Some(e.to_string()),
        (_, Err(e)) => Some(e.to_string()),
    };
    let named = ports
        .inputs
        .iter()
        .any(|p| p.to_lowercase().contains("g-system"));
    if !named && gsystem_usb::present() {
        ports.inputs.insert(0, gsystem_usb::PORT_NAME.to_string());
        ports.outputs.insert(0, gsystem_usb::PORT_NAME.to_string());
    } else if let Some(e) = midi_err {
        return Err(e);
    }
    Ok(ports)
}

/// Where messages to the unit go: a MIDI port, or the direct USB connection.
enum MidiOut {
    Port(midir::MidiOutputConnection),
    Usb(gsystem_usb::UsbMidi),
}

impl MidiOut {
    fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        match self {
            MidiOut::Port(p) => p.send(bytes).map_err(|e| e.to_string()),
            MidiOut::Usb(u) => u.send(bytes),
        }
    }
}

/// One open in/out pair, with incoming bytes reassembled into whole SysEx messages.
struct Midi {
    _in: Option<midir::MidiInputConnection<()>>,
    rx: std::sync::mpsc::Receiver<Vec<u8>>,
    out: MidiOut,
}

impl Midi {
    fn open(input: &str, output: &str) -> Result<Midi, String> {
        if input == gsystem_usb::PORT_NAME || output == gsystem_usb::PORT_NAME {
            let (usb, rx) = gsystem_usb::open()?;
            return Ok(Midi {
                _in: None,
                rx,
                out: MidiOut::Usb(usb),
            });
        }
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
        Ok(Midi {
            _in: Some(conn),
            rx,
            out: MidiOut::Port(out),
        })
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

fn send_all(m: &mut Midi, msgs: &[Message]) -> Result<(), String> {
    for msg in msgs {
        m.out.send(&msg.encode()).map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(
            gsystem_protocol::live::SEND_INTERVAL_MS,
        ));
    }
    Ok(())
}

/// Load a preset into the unit's edit buffer so it can be heard. Nothing is
/// stored; the user keeps it with the unit's own Store button.
#[tauri::command]
async fn midi_audition(
    index: usize,
    input: String,
    output: String,
    app: AppHandle,
) -> Result<String, String> {
    blocking(move || audition_blocking(index, &input, &output, &app.state::<AppState>())).await
}
fn audition_blocking(
    index: usize,
    input: &str,
    output: &str,
    state: &AppState,
) -> Result<String, String> {
    let p = state
        .bank
        .lock()
        .unwrap()
        .get(index)
        .cloned()
        .ok_or("empty slot")?;
    let msgs = gsystem_protocol::live::full_push(&p);
    let mut live = state.live.lock().unwrap();
    match live.as_mut() {
        Some(m) => send_all(m, &msgs)?,
        None => send_all(&mut Midi::open(input, output)?, &msgs)?,
    }
    Ok(p.name())
}

/// Turn Live mode on (loading `index` into the edit buffer) or off.
#[tauri::command]
async fn midi_live(
    on: bool,
    index: Option<usize>,
    input: String,
    output: String,
    app: AppHandle,
) -> Result<bool, String> {
    blocking(move || live_blocking(on, index, &input, &output, &app.state::<AppState>())).await
}
fn live_blocking(
    on: bool,
    index: Option<usize>,
    input: &str,
    output: &str,
    state: &AppState,
) -> Result<bool, String> {
    let mut live = state.live.lock().unwrap();
    if !on {
        *live = None;
        return Ok(false);
    }
    let mut m = Midi::open(input, output)?;
    if let Some(i) = index {
        if let Some(p) = state.bank.lock().unwrap().get(i) {
            send_all(&mut m, &gsystem_protocol::live::full_push(p))?;
        }
    }
    *live = Some(m);
    Ok(true)
}

/// Read the unit's global settings and footswitch assignments (55 47
/// requests, read-only).
#[tauri::command]
async fn midi_globals(input: String, output: String) -> Result<view::GlobalsView, String> {
    blocking(move || globals_blocking(&input, &output)).await
}
fn globals_blocking(input: &str, output: &str) -> Result<view::GlobalsView, String> {
    let mut m = Midi::open(input, output)?;
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

/// Ask the unit to identify itself (Universal Device Inquiry). If something
/// answers but not as a G-System, the error says what came back.
#[tauri::command]
async fn midi_identify(input: String, output: String) -> Result<String, String> {
    blocking(move || {
        let mut m = Midi::open(&input, &output)?;
        while m.rx.try_recv().is_ok() {}
        m.out
            .send(&gsystem_protocol::sysex::DEVICE_INQUIRY)
            .map_err(|e| e.to_string())?;
        let mut other: Option<Vec<u8>> = None;
        let found = m.wait(Duration::from_millis(2000), |b| {
            let id = gsystem_protocol::sysex::parse_identity(b);
            if id.is_none() && other.is_none() {
                other = Some(b.to_vec());
            }
            id
        });
        found.ok_or_else(|| match other {
            Some(b) => format!(
                "Something answered on these ports, but not as a G-System: {}",
                b.iter()
                    .take(24)
                    .map(|x| format!("{x:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            None => "No answer on these ports. Check the G-System is switched on and that both \
                     lists show the G-System's own ports, then try again."
                .to_string(),
        })
    })
    .await
}

#[derive(Clone, Serialize)]
struct BackupProgress {
    slot: usize,
    done: usize,
    total: usize,
    found: usize,
    /// The preset's name when this slot answered (empty slots don't).
    name: Option<String>,
    /// The unit answered that this slot is damaged (`28` message).
    damaged: bool,
}

#[derive(Serialize)]
struct BackupResult {
    received: Vec<usize>,
    damaged: Vec<usize>,
}

enum SlotAnswer {
    Preset(Preset),
    Damaged,
}

/// Read user slots `from..=to` from the unit into the open bank, emitting a
/// `backup-progress` event after each slot. Returns the slots that answered
/// (empty slots never answer). `midi_cancel` stops it between slots.
#[tauri::command]
async fn midi_backup(
    input: String,
    output: String,
    from: usize,
    to: usize,
    app: AppHandle,
) -> Result<BackupResult, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        state.cancel.store(false, Ordering::SeqCst);
        let mut m = Midi::open(&input, &output)?;
        let to = to.min(199);
        let total = to + 1 - from;
        let mut got = Vec::new();
        let mut damaged = Vec::new();
        // For a useful error when nothing usable comes back.
        let mut heard = 0usize;
        let mut first_heard: Option<Vec<u8>> = None;
        for (n, slot) in (from..=to).enumerate() {
            if state.cancel.load(Ordering::SeqCst) {
                break;
            }
            while m.rx.try_recv().is_ok() {}
            m.out
                .send(&Message::preset_request(0, slot).encode())
                .map_err(|e| e.to_string())?;
            let want = gsystem_protocol::sysex::slot_to_number(slot);
            // A stored preset answers in ~0.77 s.
            let answer = m.wait(Duration::from_millis(1100), |b| {
                heard += 1;
                if first_heard.is_none() {
                    first_heard = Some(b.to_vec());
                }
                match Message::parse_lenient(b) {
                    Ok(Message::PresetDump { number, data, .. }) if number == want => {
                        Preset::from_raw(data).map(SlotAnswer::Preset)
                    }
                    Ok(Message::CorruptPreset { number, .. }) if number == want => {
                        Some(SlotAnswer::Damaged)
                    }
                    _ => None,
                }
            });
            let mut name = None;
            let mut is_damaged = false;
            match answer {
                Some(SlotAnswer::Preset(p)) => {
                    name = Some(p.name());
                    state.bank.lock().unwrap().set(slot, p);
                    state.dirty.lock().unwrap().remove(&slot);
                    got.push(slot);
                }
                Some(SlotAnswer::Damaged) => {
                    is_damaged = true;
                    damaged.push(slot);
                }
                None => {}
            }
            let _ = app.emit(
                "backup-progress",
                BackupProgress {
                    slot,
                    done: n + 1,
                    total,
                    found: got.len(),
                    name,
                    damaged: is_damaged,
                },
            );
        }
        if got.is_empty() && damaged.is_empty() && heard > 0 {
            let first = first_heard.unwrap_or_default();
            return Err(format!(
                "The unit sent {heard} message(s) but no preset dumps were recognised. First one ({} bytes): {}",
                first.len(),
                first.iter().take(24).map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
            ));
        }
        Ok(BackupResult {
            received: got,
            damaged,
        })
    })
    .await
}

/// Stop a running bank read after the current slot.
#[tauri::command]
fn midi_cancel(state: State<AppState>) {
    state.cancel.store(true, Ordering::SeqCst);
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
            midi_audition,
            midi_live,
            midi_backup,
            midi_send,
            midi_cancel
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
