//! `gsys mcp`: a Model Context Protocol server on stdin/stdout, so an AI
//! agent (Claude Desktop, Claude Code…) can design and edit presets in a
//! working bank file and audition them on the unit.
//!
//! JSON-RPC 2.0, one message per line; logs go to stderr. Implemented by
//! hand with serde_json (no SDK). Presets are written with the authoring API
//! in `gsystem_protocol::author`.
//!
//! Safety: the only thing sent to the unit is `audition`, which writes the
//! edit buffer (`live::full_push`) like turning knobs. There is no tool that
//! stores to the unit: Vyzor's store sequence has frozen units (PROTOCOL.md),
//! so the user keeps a sound with the unit's own STORE button.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use gsystem_protocol::author::{self, PresetSpec};
use gsystem_protocol::preset::slot_label;
use gsystem_protocol::{bank::SLOTS, live, syx, validate, vyzor, Bank, Preset};
use serde_json::{json, Value};

const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const WRITERS: [&str; 5] = [
    "create_preset",
    "update_preset",
    "copy_preset",
    "rename_preset",
    "clear_slot",
];

pub struct Options {
    pub bank: PathBuf,
    pub port: Option<String>,
    pub read_only: bool,
}

pub fn usage() -> &'static str {
    "gsys mcp --bank <file.syx> [--port <midi port>] [--read-only]

Runs a Model Context Protocol server on stdin/stdout for AI agents.
  --bank       working bank (.syx; created empty if missing). A .SQS/.Prs/.Ebf
               is loaded and changes are saved to <same name>.syx.
  --port       MIDI port of the G-System (enables the audition tool, which
               loads a preset into the unit's edit buffer; nothing is stored).
  --read-only  hide the tools that change the bank."
}

pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut bank = None;
    let mut port = None;
    let mut read_only = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--bank" => bank = Some(PathBuf::from(it.next().ok_or("--bank needs a path")?)),
            "--port" => port = Some(it.next().ok_or("--port needs a port name")?.clone()),
            "--read-only" => read_only = true,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown option {other:?}")),
        }
    }
    Ok(Options {
        bank: bank.ok_or("--bank <path> is required")?,
        port,
        read_only,
    })
}

pub fn run(opts: Options) {
    let mut server = match Server::open(opts) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("gsys mcp: {e}");
            std::process::exit(1)
        }
    };
    eprintln!(
        "gsys mcp: serving {} ({} presets){}{}",
        server.save_to.display(),
        server.bank.occupied(),
        if server.read_only { ", read-only" } else { "" },
        match &server.port {
            Some(p) => format!(", audition on MIDI port {p:?}"),
            None => String::new(),
        }
    );
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Err(e) => Some(error_reply(
                Value::Null,
                -32700,
                &format!("Parse error: {e}"),
            )),
            Ok(Value::Array(batch)) => {
                let replies: Vec<Value> =
                    batch.into_iter().filter_map(|m| server.handle(m)).collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            Ok(msg) => server.handle(msg),
        };
        if let Some(reply) = reply {
            let mut out = stdout.lock();
            if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
                break;
            }
        }
    }
}

fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

struct Server {
    bank: Bank,
    /// The file given with --bank.
    source: PathBuf,
    /// Where changes are written (.syx).
    save_to: PathBuf,
    backed_up: bool,
    port: Option<String>,
    read_only: bool,
}

fn is_vyzor(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["sqs", "prs", "ebf"].contains(&e.to_lowercase().as_str()))
}

impl Server {
    fn open(opts: Options) -> Result<Server, String> {
        let source = opts.bank;
        let save_to = if is_vyzor(&source) {
            source.with_extension("syx")
        } else {
            source.clone()
        };
        let bank = if source.exists() {
            let bytes = std::fs::read(&source)
                .map_err(|e| format!("cannot read {}: {e}", source.display()))?;
            if is_vyzor(&source) {
                vyzor::load_bank(&bytes).map_err(|e| format!("{}: {e}", source.display()))?
            } else if bytes.is_empty() {
                Bank::empty()
            } else {
                syx::load_bank(&bytes)
                    .or_else(|_| vyzor::load_bank(&bytes))
                    .map_err(|e| {
                        format!(
                            "{}: {e}. Refusing to overwrite a file that isn't a G-System bank.",
                            source.display()
                        )
                    })?
            }
        } else if is_vyzor(&source) {
            return Err(format!("{} does not exist", source.display()));
        } else {
            if !opts.read_only {
                if let Some(dir) = source.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir)
                        .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
                }
                std::fs::write(&source, b"")
                    .map_err(|e| format!("cannot create {}: {e}", source.display()))?;
                eprintln!("gsys mcp: created empty bank {}", source.display());
            }
            Bank::empty()
        };
        Ok(Server {
            bank,
            source,
            save_to,
            backed_up: false,
            port: opts.port,
            read_only: opts.read_only,
        })
    }

    /// Where changes go, for tool output.
    fn file_note(&self) -> String {
        if self.source != self.save_to {
            format!(
                "Loaded from {}; changes are saved to {} (the original is not modified).",
                self.source.display(),
                self.save_to.display()
            )
        } else {
            format!("Changes are saved to {}.", self.save_to.display())
        }
    }

    /// Write `bank` atomically and make it current. The first write of a
    /// session keeps the previous file as `<bank>.bak`.
    fn commit(&mut self, bank: Bank) -> Result<(), String> {
        let path = self.save_to.clone();
        if !self.backed_up {
            if std::fs::metadata(&path).is_ok_and(|m| m.len() > 0) {
                let mut bak = path.clone().into_os_string();
                bak.push(".bak");
                std::fs::copy(&path, &bak)
                    .map_err(|e| format!("cannot back up {}: {e}", path.display()))?;
                eprintln!(
                    "gsys mcp: kept the previous bank as {}",
                    PathBuf::from(&bak).display()
                );
            }
            self.backed_up = true;
        }
        let dir = path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let file = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("bank.syx");
        let tmp = dir.join(format!(".{file}.{}.tmp", std::process::id()));
        std::fs::write(&tmp, syx::save_bank(&bank, 0))
            .and_then(|_| std::fs::rename(&tmp, &path))
            .map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                format!("cannot write {}: {e}", path.display())
            })?;
        self.bank = bank;
        Ok(())
    }

    fn handle(&mut self, msg: Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            // A response to us (we send no requests) or garbage.
            if msg.get("result").is_some() || msg.get("error").is_some() {
                return None;
            }
            return Some(error_reply(
                id.unwrap_or(Value::Null),
                -32600,
                "Invalid Request",
            ));
        };
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
        let Some(id) = id else {
            // Notifications (initialized, cancelled…) need no answer.
            return None;
        };
        let result = match method {
            "initialize" => Ok(self.initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": self.tool_list()})),
            "tools/call" => self.call(&params),
            "prompts/list" => Ok(json!({"prompts": [prompt_def()]})),
            "prompts/get" => prompt_get(&params),
            _ => Err((-32601, format!("Method not found: {method}"))),
        };
        Some(match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, m)) => error_reply(id, code, &m),
        })
    }

    fn initialize(&self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str);
        let version = asked
            .filter(|v| PROTOCOL_VERSIONS.contains(v))
            .unwrap_or(PROTOCOL_VERSIONS[0]);
        json!({
            "protocolVersion": version,
            "capabilities": {"tools": {}, "prompts": {}},
            "serverInfo": {"name": "gsys", "version": env!("CARGO_PKG_VERSION")},
            "instructions": "Designs and edits presets for a TC Electronic G-System guitar processor in a working bank file (.syx). Call get_catalog first: it lists every block, effect type, parameter, range and value format. Presets are never stored on the unit by this server; audition (when available) only loads the unit's edit buffer, and the user presses STORE on the G-System to keep a sound.",
        })
    }

    fn tool_list(&self) -> Vec<Value> {
        tool_defs()
            .into_iter()
            .filter(|t| self.has_tool(t["name"].as_str().unwrap_or("")))
            .collect()
    }

    fn has_tool(&self, name: &str) -> bool {
        if self.read_only && WRITERS.contains(&name) {
            return false;
        }
        if name == "audition" && self.port.is_none() {
            return false;
        }
        tool_defs().iter().any(|t| t["name"] == name)
    }

    fn call(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((-32602, "tools/call needs a tool name".to_string()))?;
        if !self.has_tool(name) {
            return Err((-32602, format!("Unknown tool: {name}")));
        }
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let result = match name {
            "get_catalog" => self.get_catalog(&args),
            "list_presets" => self.list_presets(),
            "get_preset" => self.get_preset(&args),
            "create_preset" => self.create_preset(&args),
            "update_preset" => self.update_preset(&args),
            "copy_preset" => self.copy_preset(&args),
            "rename_preset" => self.rename_preset(&args),
            "clear_slot" => self.clear_slot(&args),
            "validate_preset" => self.validate_preset(&args),
            "find_empty_slot" => self.find_empty_slot(&args),
            "audition" => self.audition(&args),
            _ => Err(format!("Unknown tool: {name}")),
        };
        Ok(match result {
            Ok(v) => {
                let text = match v {
                    Value::String(s) => s,
                    v => serde_json::to_string_pretty(&v).unwrap_or_default(),
                };
                json!({"content": [{"type": "text", "text": text}], "isError": false})
            }
            Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
        })
    }

    // ------------------------------------------------------------ tools

    fn get_catalog(&self, args: &Value) -> Result<Value, String> {
        match args.get("block").and_then(Value::as_str) {
            Some(b) => author::catalog_block(b).map(|c| json!(c)),
            None => {
                // Compact JSON: the catalog is meant for the agent's context.
                let text = serde_json::to_string(&author::catalog()).map_err(|e| e.to_string())?;
                Ok(Value::String(text))
            }
        }
    }

    fn list_presets(&self) -> Result<Value, String> {
        let presets: Vec<Value> = self
            .bank
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                s.as_ref()
                    .map(|p| json!({"slot": i, "label": slot_label(i), "name": p.name()}))
            })
            .collect();
        Ok(json!({
            "bank": self.save_to.display().to_string(),
            "note": self.file_note(),
            "presets": presets,
            "occupied": self.bank.occupied(),
            "empty_slots": SLOTS - self.bank.occupied(),
        }))
    }

    fn preset(&self, slot: usize) -> Result<&Preset, String> {
        self.bank
            .get(slot)
            .ok_or_else(|| format!("slot {slot} ({}) is empty", slot_label(slot)))
    }

    fn get_preset(&self, args: &Value) -> Result<Value, String> {
        let slot = slot_arg(args, "slot")?;
        let p = self.preset(slot)?;
        let full = args.get("full").and_then(Value::as_bool).unwrap_or(false);
        let summary = if full {
            author::describe_full(p)
        } else {
            author::describe(p)
        };
        Ok(json!({"slot": slot, "label": slot_label(slot), "preset": summary}))
    }

    fn spec_arg(args: &Value) -> Result<PresetSpec, String> {
        let v = args.get("spec").cloned().unwrap_or_else(|| json!({}));
        serde_json::from_value(v).map_err(|e| format!("invalid spec: {e}"))
    }

    fn report(slot: usize, p: &Preset, applied: Vec<author::Applied>, note: String) -> Value {
        json!({
            "slot": slot,
            "label": slot_label(slot),
            "applied": applied,
            "preset": author::describe(p),
            "saved": note,
        })
    }

    fn check(p: &Preset) -> Result<Vec<String>, String> {
        let r = validate::check(p);
        if r.is_ok() {
            Ok(r.warnings.into_iter().map(|i| i.message).collect())
        } else {
            Err(format!(
                "the result would not be a valid preset, nothing was saved: {}",
                r.summary()
            ))
        }
    }

    fn create_preset(&mut self, args: &Value) -> Result<Value, String> {
        let slot = match args.get("slot") {
            Some(Value::Null) | None => self.first_empty(0).ok_or("the bank is full")?,
            Some(_) => slot_arg(args, "slot")?,
        };
        let overwrite = args
            .get("overwrite")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if let Some(p) = self.bank.get(slot) {
            if !overwrite {
                return Err(format!(
                    "slot {slot} ({}) holds {:?}. Pick another slot (find_empty_slot) or pass overwrite: true.",
                    slot_label(slot),
                    p.name()
                ));
            }
        }
        let spec = Self::spec_arg(args)?;
        let (p, applied) =
            author::new_preset(&spec, &author::default_template()).map_err(|e| e.to_string())?;
        let warnings = Self::check(&p)?;
        let mut bank = self.bank.clone();
        bank.set(slot, p.clone());
        self.commit(bank)?;
        let mut r = Self::report(slot, &p, applied, self.file_note());
        if !warnings.is_empty() {
            r["warnings"] = json!(warnings);
        }
        Ok(r)
    }

    fn update_preset(&mut self, args: &Value) -> Result<Value, String> {
        let slot = slot_arg(args, "slot")?;
        let mut p = self
            .preset(slot)
            .map_err(|e| format!("{e}; use create_preset"))?
            .clone();
        let spec = Self::spec_arg(args)?;
        let applied = author::apply_spec(&mut p, &spec).map_err(|e| e.to_string())?;
        let warnings = Self::check(&p)?;
        let mut bank = self.bank.clone();
        bank.set(slot, p.clone());
        self.commit(bank)?;
        let mut r = Self::report(slot, &p, applied, self.file_note());
        if !warnings.is_empty() {
            r["warnings"] = json!(warnings);
        }
        Ok(r)
    }

    fn copy_preset(&mut self, args: &Value) -> Result<Value, String> {
        let from = slot_arg(args, "from")?;
        let to = slot_arg(args, "to")?;
        let overwrite = args
            .get("overwrite")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let p = self.preset(from)?.clone();
        if from == to {
            return Err("from and to are the same slot".into());
        }
        if let Some(q) = self.bank.get(to) {
            if !overwrite {
                return Err(format!(
                    "slot {to} ({}) holds {:?}; pass overwrite: true to replace it",
                    slot_label(to),
                    q.name()
                ));
            }
        }
        let mut bank = self.bank.clone();
        bank.set(to, p.clone());
        self.commit(bank)?;
        Ok(json!({
            "copied": p.name(),
            "from": slot_label(from),
            "to": {"slot": to, "label": slot_label(to)},
            "saved": self.file_note(),
        }))
    }

    fn rename_preset(&mut self, args: &Value) -> Result<Value, String> {
        let slot = slot_arg(args, "slot")?;
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .ok_or("name is required")?;
        let mut p = self.preset(slot)?.clone();
        let old = p.name();
        let spec = PresetSpec {
            name: Some(name.to_string()),
            ..Default::default()
        };
        let applied = author::apply_spec(&mut p, &spec).map_err(|e| e.to_string())?;
        let mut bank = self.bank.clone();
        bank.set(slot, p.clone());
        self.commit(bank)?;
        Ok(json!({
            "slot": slot,
            "label": slot_label(slot),
            "old_name": old,
            "applied": applied,
            "saved": self.file_note(),
        }))
    }

    fn clear_slot(&mut self, args: &Value) -> Result<Value, String> {
        let slot = slot_arg(args, "slot")?;
        let old = self.preset(slot)?.name();
        let mut bank = self.bank.clone();
        bank.slots[slot] = None;
        self.commit(bank)?;
        Ok(json!({
            "cleared": {"slot": slot, "label": slot_label(slot), "was": old},
            "saved": self.file_note(),
        }))
    }

    fn validate_preset(&self, args: &Value) -> Result<Value, String> {
        let slot = slot_arg(args, "slot")?;
        let r = validate::check(self.preset(slot)?);
        Ok(json!({
            "slot": slot,
            "label": slot_label(slot),
            "ok": r.is_ok(),
            "errors": r.errors.iter().map(|i| i.message.clone()).collect::<Vec<_>>(),
            "warnings": r.warnings.iter().map(|i| i.message.clone()).collect::<Vec<_>>(),
        }))
    }

    fn first_empty(&self, from: usize) -> Option<usize> {
        (from..SLOTS).find(|&i| self.bank.get(i).is_none())
    }

    fn find_empty_slot(&self, args: &Value) -> Result<Value, String> {
        let from = match args.get("from") {
            Some(Value::Null) | None => 0,
            Some(_) => slot_arg(args, "from")?,
        };
        let slot = self
            .first_empty(from)
            .ok_or_else(|| format!("no empty slot from {} on", slot_label(from)))?;
        Ok(json!({"slot": slot, "label": slot_label(slot)}))
    }

    fn audition(&self, args: &Value) -> Result<Value, String> {
        let port = self
            .port
            .as_deref()
            .ok_or("no MIDI port was given (--port)")?;
        let slot = slot_arg(args, "slot")?;
        let p = self.preset(slot)?;
        let msgs = live::full_push(p);
        let mut session = crate::midi::Session::try_open(port, port)?;
        session.try_send_all(&msgs)?;
        Ok(Value::String(format!(
            "Loaded {:?} ({}) into the G-System's edit buffer over {port:?} ({} parameter messages). Nothing is stored on the unit: to keep this sound, the user must press STORE on the G-System and pick a slot. Loops, insert and boost are not switched by audition.",
            p.name(),
            slot_label(slot),
            msgs.len()
        )))
    }
}

/// A slot as 0-199 or a "BB-P" label.
fn parse_slot(v: &Value) -> Result<usize, String> {
    let bad =
        || format!("slot {v}: give 0-199 or a label \"00-1\" … \"39-5\" (bank 00-39, preset 1-5)");
    let n = match v {
        Value::Number(n) => n.as_u64().ok_or_else(bad)? as usize,
        Value::String(s) => {
            let s = s.trim();
            if let Some((b, p)) = s.split_once('-') {
                let b: usize = b.trim().parse().map_err(|_| bad())?;
                let p: usize = p.trim().parse().map_err(|_| bad())?;
                if !(1..=5).contains(&p) || b >= SLOTS / 5 {
                    return Err(bad());
                }
                b * 5 + p - 1
            } else {
                s.parse().map_err(|_| bad())?
            }
        }
        _ => return Err(bad()),
    };
    if n < SLOTS {
        Ok(n)
    } else {
        Err(bad())
    }
}

fn slot_arg(args: &Value, key: &str) -> Result<usize, String> {
    parse_slot(args.get(key).ok_or_else(|| format!("{key} is required"))?)
}

// ------------------------------------------------------------ definitions

const SLOT_HELP: &str = "Preset slot: 0-199, where 0 = \"00-1\", 4 = \"00-5\", 5 = \"01-1\" … 199 = \"39-5\" (bank 00-39, preset 1-5 as on the unit), or the label string itself, e.g. \"12-3\".";

const SPEC_HELP: &str = "Spec fields (all optional): name (max 20 chars); routing (Serial | Parallel | Semi Parallel | Serial2); tempo_bpm (20-600); boost {on, level_db 0-10}; loops [4 bools]; insert (bool); relays {a, b: \"OCCO\"-style, active: \"A\"|\"B\"}; outputs {left_db, right_db: -100..0}; novadrive (0-18, 0 = none); blocks {<key>: {type, on, params: {<name or label>: value}}} with keys comp, filter, mod, pitch, reverb, gate, delay, eq; modifiers [{block, param, source: None|Vol|Exp|Enc A|Enc B|Enc C, min, mid, max: 0-100 % of the parameter's range}]. \
Values: a string in display form is safest: \"2.5 kHz\", \"350 ms\", \"1.8 s\", \"-6 dB\", \"35 %\", or a choice label (\"1/8D\", \"Hall\", \"Large\"); it is set to the nearest available step. A JSON number is the raw value (clamped to the range), which for frequency, LFO speed, EQ width and reverb decay is an index, not Hz or seconds. true/false work for On/Off switches. Parameter names match the firmware name or the readable label, case-insensitive (\"Fbhcut\" = \"FB high cut\"). \
Delay times and mod/filter speeds follow tempo_bpm when their Tempo parameter is a note value (\"1/4\", \"1/8D\"); setting a Delay or Speed explicitly switches its Tempo to \"Ignore\" unless the same spec sets that Tempo. Changing a type keeps old values clamped into the new type's ranges. The result lists every value actually set (with notes on clamping or rounding) and the preset summary.";

fn spec_schema() -> Value {
    let value = json!({"type": ["string", "number", "boolean"]});
    let mut blocks = serde_json::Map::new();
    for b in gsystem_protocol::params::BLOCKS.iter() {
        let mut props = json!({
            "on": {"type": "boolean", "description": format!("Switch the {} block on or off", b.label)},
            "params": {
                "type": "object",
                "description": "Parameter name or label → value (see get_catalog)",
                "additionalProperties": value,
            },
        });
        if b.has_type() {
            props["type"] = json!({
                "type": "string",
                "enum": b.types.iter().map(|t| t.name).collect::<Vec<_>>(),
            });
        }
        blocks.insert(
            b.key.to_string(),
            json!({"type": "object", "description": b.label, "properties": props, "additionalProperties": false}),
        );
    }
    json!({
        "type": "object",
        "description": "Preset spec (see the tool description for fields and value formats)",
        "properties": {
            "name": {"type": "string", "maxLength": 20},
            "routing": {"type": "string", "enum": gsystem_protocol::levels::ROUTINGS},
            "tempo_bpm": {"type": "number", "minimum": 20, "maximum": 600},
            "boost": {"type": "object", "properties": {
                "on": {"type": "boolean"},
                "level_db": {"type": "integer", "minimum": 0, "maximum": 10}
            }, "additionalProperties": false},
            "loops": {"type": "array", "items": {"type": "boolean"}, "minItems": 4, "maxItems": 4,
                      "description": "Effect loops 1-4 on/off"},
            "insert": {"type": "boolean", "description": "Insert loop on/off"},
            "relays": {"type": "object", "properties": {
                "a": {"type": ["string", "integer"], "description": "Relay setting A: 4 letters O/C, contact 1 first, e.g. \"OCCO\""},
                "b": {"type": ["string", "integer"], "description": "Relay setting B"},
                "active": {"type": "string", "enum": ["A", "B"]}
            }, "additionalProperties": false},
            "outputs": {"type": "object", "properties": {
                "left_db": {"type": "integer", "minimum": -100, "maximum": 0},
                "right_db": {"type": "integer", "minimum": -100, "maximum": 0}
            }, "additionalProperties": false},
            "novadrive": {"type": "integer", "minimum": 0, "maximum": 18,
                          "description": "NovaDrive NDR-1 preset recalled with this preset, 0 = none"},
            "blocks": {"type": "object", "properties": blocks},
            "modifiers": {"type": "array", "items": {"type": "object", "properties": {
                "block": {"type": "string", "description": "Block key, or \"novadrive\""},
                "param": {"type": "string"},
                "source": {"type": "string", "enum": gsystem_protocol::modifiers::SOURCES},
                "min": {"type": "integer", "minimum": 0, "maximum": 100},
                "mid": {"type": "integer", "minimum": 0, "maximum": 100},
                "max": {"type": "integer", "minimum": 0, "maximum": 100}
            }, "required": ["block", "param", "source"], "additionalProperties": false}}
        },
        "additionalProperties": false,
    })
}

fn slot_schema(desc: &str) -> Value {
    json!({"type": ["integer", "string"], "description": format!("{desc} {SLOT_HELP}")})
}

fn tool(name: &str, title: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        },
    })
}

fn tool_defs() -> Vec<Value> {
    let block_keys: Vec<&str> = gsystem_protocol::params::BLOCKS
        .iter()
        .map(|b| b.key)
        .collect();
    vec![
        tool(
            "get_catalog",
            "Parameter catalog",
            "Everything needed to write presets: the 8 effect blocks (comp, filter, mod, pitch, reverb, gate, delay, eq), their effect types, and every parameter with name, readable label, raw range lo..hi, unit (dB, ms, %, cents, number, choice, freq index, lfo index, octave index, scaled steps), sample scale points (raw=display) for index units, and choice labels. Also routings, boost/output ranges, loops/insert/relays, modifier sources, NovaDrive and tempo ranges, and how values are written. Call this first. With `block`, returns that block only, plus which parameters a modifier (pedal/encoder) can control for each type.",
            json!({"block": {"type": "string", "enum": block_keys, "description": "Only this block, with modifier targets per type"}}),
            &[],
        ),
        tool(
            "list_presets",
            "List presets",
            "List the occupied slots of the working bank (slot number, label \"BB-P\", name), the number of empty slots, and the bank file changes are saved to.",
            json!({}),
            &[],
        ),
        tool(
            "get_preset",
            "Get preset",
            "Summary of one preset: name, routing, tempo, the blocks in audio order (filter, comp, mod, pitch, delay, reverb, eq, gate) with type and on/off, the parameters of every block that is on as display values (\"375 ms\", \"2.56 kHz\"), loops, insert, boost, outputs, relays, modifier assignments and NovaDrive. Parameter keys are the labels, which update_preset accepts. Set full: true to include the parameters of bypassed blocks too.",
            json!({
                "slot": slot_schema("Slot to read."),
                "full": {"type": "boolean", "default": false, "description": "Include bypassed blocks' parameters"}
            }),
            &["slot"],
        ),
        tool(
            "create_preset",
            "Create preset",
            &format!("Create a new preset from a spec and save it to the working bank. It starts empty (every block, loop, boost and modifier off, outputs 0 dB); each block you list in spec.blocks is switched on unless it says on: false, and its parameters keep sensible defaults unless set. Refuses an occupied slot unless overwrite is true; without a slot, the first empty slot is used. Nothing is sent to the unit. {SPEC_HELP}"),
            json!({
                "slot": slot_schema("Target slot; omit for the first empty slot."),
                "spec": spec_schema(),
                "overwrite": {"type": "boolean", "default": false, "description": "Replace a preset already in the slot"}
            }),
            &["spec"],
        ),
        tool(
            "update_preset",
            "Update preset",
            &format!("Change an existing preset with a partial spec: only what the spec names changes (blocks keep their on/off state unless `on` is given). All or nothing: if any name or value is wrong, nothing changes and every problem is listed with the valid alternatives. Saved to the working bank; nothing is sent to the unit. {SPEC_HELP}"),
            json!({
                "slot": slot_schema("Slot to change."),
                "spec": spec_schema()
            }),
            &["slot", "spec"],
        ),
        tool(
            "copy_preset",
            "Copy preset",
            "Copy a preset to another slot of the working bank (e.g. to make a variation). Refuses an occupied target unless overwrite is true.",
            json!({
                "from": slot_schema("Source slot."),
                "to": slot_schema("Target slot."),
                "overwrite": {"type": "boolean", "default": false}
            }),
            &["from", "to"],
        ),
        tool(
            "rename_preset",
            "Rename preset",
            "Rename a preset in the working bank. Names hold 20 characters (Latin-1).",
            json!({
                "slot": slot_schema("Slot to rename."),
                "name": {"type": "string", "maxLength": 20}
            }),
            &["slot", "name"],
        ),
        tool(
            "clear_slot",
            "Clear slot",
            "Remove a preset from the working bank file, leaving the slot empty. Only the file changes; the unit's own slots are never touched.",
            json!({"slot": slot_schema("Slot to clear.")}),
            &["slot"],
        ),
        tool(
            "validate_preset",
            "Validate preset",
            "Check a preset against the firmware 4.02 parameter tables: errors are values the unit can't hold (unknown type, value out of range); warnings are harmless oddities. Presets made with create_preset/update_preset are always valid.",
            json!({"slot": slot_schema("Slot to check.")}),
            &["slot"],
        ),
        tool(
            "find_empty_slot",
            "Find empty slot",
            "The first empty slot of the working bank, at or after `from` (default 0).",
            json!({"from": slot_schema("Start searching here.")}),
            &[],
        ),
        tool(
            "audition",
            "Audition on the unit",
            "Load a preset from the working bank into the G-System's edit buffer over MIDI so the user can hear it, like turning the knobs: effect blocks, types, parameters, modifiers, routing, output levels and NovaDrive (loops, insert and boost are not switched). Nothing is stored on the unit, and this server has no way to store: tell the user to press STORE on the G-System to keep the sound. Takes about a second.",
            json!({"slot": slot_schema("Slot to audition.")}),
            &["slot"],
        ),
    ]
}

const PROMPT_TEXT: &str = "You are designing a preset for a TC Electronic G-System guitar processor with the gsys tools.

1. Call get_catalog first, so you use real block keys, effect type names, parameter names, ranges and value formats rather than guessing. Use get_catalog with a block for modifier (pedal) targets.
2. Start from the musical goal: the song, artist, style or sound described. Decide which few effects make that sound before touching parameters; a good preset usually has two to four blocks on, not all eight.
3. Write values as display strings (\"350 ms\", \"2.5 kHz\", \"-3 dB\", \"25 %\", \"1/8D\"): raw numbers for frequency, speed, EQ width and decay are indices.
4. Keep levels sane: Out level 0 dB on each active block unless there's a reason; mixes moderate (delay mix about 15-35 %, reverb mix about 15-40 %, chorus mix about 40-60 %); feedback below 60 % unless self-oscillation is the point; boost off unless asked for a solo boost.
5. For rhythmic delays, set tempo_bpm and the delay's Tempo to a note value (\"1/4\", \"1/8D\") so the time follows the tempo; for a fixed time in ms (e.g. \"440 ms\"), set Delay directly (its Tempo then becomes Ignore).
6. Create the preset with create_preset (find_empty_slot if no slot was given; never overwrite without asking), read the result's applied values and notes, and fix anything that landed somewhere unexpected with update_preset. validate_preset if in doubt.
7. If audition is available, offer to audition the preset. Remind the user that nothing is stored on the unit until they press STORE on the G-System.
8. Finish with a short explanation of the choices: which blocks, why those types and values, and what to tweak by ear (e.g. delay mix, reverb decay).";

fn prompt_def() -> Value {
    json!({
        "name": "design_preset",
        "title": "Design a G-System preset",
        "description": "Guidance for designing a preset from a musical brief (song, artist or style) with the gsys tools.",
        "arguments": [
            {"name": "brief", "description": "The sound to make, e.g. \"Edge-style dotted-eighth delay with shimmer\"", "required": false},
            {"name": "slot", "description": "Slot to put it in (0-199 or \"BB-P\"); default: first empty slot", "required": false}
        ]
    })
}

fn prompt_get(params: &Value) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    if name != "design_preset" {
        return Err((-32602, format!("Unknown prompt: {name}")));
    }
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut text = PROMPT_TEXT.to_string();
    if let Some(b) = args
        .get("brief")
        .and_then(Value::as_str)
        .filter(|b| !b.trim().is_empty())
    {
        text.push_str(&format!("\n\nThe brief: {b}"));
    }
    if let Some(s) = args
        .get("slot")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
    {
        text.push_str(&format!("\nPut it in slot {s}."));
    }
    Ok(json!({
        "description": "Design a G-System preset",
        "messages": [{"role": "user", "content": {"type": "text", "text": text}}]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_parse_numbers_and_labels() {
        assert_eq!(parse_slot(&json!(0)), Ok(0));
        assert_eq!(parse_slot(&json!(199)), Ok(199));
        assert_eq!(parse_slot(&json!("12-3")), Ok(62));
        assert_eq!(parse_slot(&json!("00-1")), Ok(0));
        assert_eq!(parse_slot(&json!("39-5")), Ok(199));
        assert_eq!(parse_slot(&json!("57")), Ok(57));
        for bad in [
            json!(200),
            json!(-1),
            json!("40-1"),
            json!("12-6"),
            json!("x"),
            json!(1.5),
        ] {
            assert!(parse_slot(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn every_tool_has_a_schema_and_description() {
        for t in tool_defs() {
            assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
            assert!(
                t["description"].as_str().unwrap().len() > 40,
                "{}",
                t["name"]
            );
        }
    }
}
