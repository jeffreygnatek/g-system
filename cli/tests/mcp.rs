//! Drives `gsys mcp` over stdin/stdout like an MCP client would.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use gsystem_protocol::{params, syx};
use serde_json::{json, Value};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    fn start(args: &[&str]) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_gsys"))
            .arg("mcp")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn gsys mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut c = Client {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        let init = c.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
        );
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(init["result"]["serverInfo"]["name"], "gsys");
        assert!(init["result"]["capabilities"]["tools"].is_object());
        assert!(init["result"]["capabilities"]["prompts"].is_object());
        c.notify("notifications/initialized");
        c
    }

    fn send(&mut self, msg: &Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn read(&mut self) -> Value {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        assert!(!line.is_empty(), "server closed stdout");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("bad JSON {line:?}: {e}"))
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let reply = self.read();
        assert_eq!(reply["id"], id, "{reply}");
        assert_eq!(reply["jsonrpc"], "2.0");
        reply
    }

    fn notify(&mut self, method: &str) {
        self.send(&json!({"jsonrpc": "2.0", "method": method}));
    }

    fn tool_names(&mut self) -> Vec<String> {
        let r = self.request("tools/list", json!({}));
        r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                assert_eq!(t["inputSchema"]["type"], "object", "{t}");
                assert!(t["description"].as_str().unwrap().len() > 40, "{t}");
                t["name"].as_str().unwrap().to_string()
            })
            .collect()
    }

    /// Call a tool; returns (isError, text).
    fn call_raw(&mut self, name: &str, args: Value) -> (bool, String) {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        let result = &r["result"];
        assert!(result.is_object(), "{r}");
        let text = result["content"][0]["text"].as_str().unwrap().to_string();
        assert_eq!(result["content"][0]["type"], "text");
        (result["isError"] == true, text)
    }

    /// Call a tool that must succeed; its text parsed as JSON when it is JSON.
    fn call(&mut self, name: &str, args: Value) -> Value {
        let (err, text) = self.call_raw(name, args);
        assert!(!err, "{name} failed: {text}");
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "gsys-mcp-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn factory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/Factory_Set.SQS")
}

const WRITERS: [&str; 5] = [
    "create_preset",
    "update_preset",
    "copy_preset",
    "rename_preset",
    "clear_slot",
];

#[test]
fn designs_presets_end_to_end() {
    let dir = temp_dir("e2e");
    let bank = dir.join("agent.syx");
    let mut c = Client::start(&["--bank", bank.to_str().unwrap()]);
    assert!(bank.exists(), "an empty bank is created");

    let names = c.tool_names();
    for t in [
        "get_catalog",
        "list_presets",
        "get_preset",
        "create_preset",
        "update_preset",
        "copy_preset",
        "rename_preset",
        "clear_slot",
        "validate_preset",
        "find_empty_slot",
    ] {
        assert!(names.contains(&t.to_string()), "{t} missing from {names:?}");
    }
    assert!(
        !names.contains(&"audition".to_string()),
        "audition needs --port"
    );

    // Unknown methods are JSON-RPC errors; ping works.
    let r = c.request("resources/list", json!({}));
    assert_eq!(r["error"]["code"], -32601);
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));

    // Catalog.
    let cat = c.call("get_catalog", json!({}));
    assert_eq!(cat["blocks"].as_array().unwrap().len(), 8);
    assert!(cat["routings"]
        .as_array()
        .unwrap()
        .contains(&json!("Semi Parallel")));
    let delay = c.call("get_catalog", json!({"block": "delay"}));
    assert_eq!(delay["key"], "delay");
    assert!(delay["types"][0]["modifiable"]["Ping Pong"].is_array());

    // Create.
    let created = c.call(
        "create_preset",
        json!({"slot": "02-3", "spec": {
            "name": "Edge Dotted",
            "tempo_bpm": 120,
            "blocks": {
                "delay": {"type": "Ping Pong", "params": {"Tempo": "1/8D", "Feedback": "35 %", "Mix": "30 %"}},
                "reverb": {"type": "Hall", "params": {"Decay": "3.5 s", "Mix": "25 %"}}
            }
        }}),
    );
    assert_eq!(created["slot"], 12);
    assert_eq!(created["label"], "02-3");
    assert!(created["applied"].as_array().unwrap().len() >= 6);
    let saved = created["saved"].as_str().unwrap();
    assert!(saved.contains("agent.syx"), "{saved}");

    // Occupied slots are refused without overwrite.
    let (err, text) = c.call_raw(
        "create_preset",
        json!({"slot": 12, "spec": {"name": "Other"}}),
    );
    assert!(err && text.contains("overwrite"), "{text}");

    // Get it back.
    let got = c.call("get_preset", json!({"slot": 12}));
    let preset = &got["preset"];
    assert_eq!(preset["name"], "Edge Dotted");
    assert_eq!(preset["tempo"], "120 BPM (500 ms)");
    let block = |p: &Value, k: &str| -> Value {
        p["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["block"] == k)
            .unwrap()
            .clone()
    };
    let d = block(preset, "delay");
    assert_eq!(d["on"], true);
    assert_eq!(d["type"], "Ping Pong");
    assert_eq!(d["params"]["Delay"], "375 ms");
    assert_eq!(d["params"]["Feedback"], "35 %");
    assert_eq!(block(preset, "reverb")["params"]["Decay"], "3.5 s");
    assert!(
        block(preset, "comp")["params"].is_null(),
        "bypassed: no params"
    );
    let full = c.call("get_preset", json!({"slot": "02-3", "full": true}));
    assert!(block(&full["preset"], "comp")["params"].is_object());

    // Update: tempo change moves the synced delay; add a chorus.
    let updated = c.call(
        "update_preset",
        json!({"slot": 12, "spec": {"tempo_bpm": 100, "blocks": {"mod": {"type": "Classic Chorus", "on": true, "params": {"Mix": "50 %"}}}}}),
    );
    let d = block(&updated["preset"], "delay");
    assert_eq!(d["params"]["Delay"], "450 ms");
    assert_eq!(block(&updated["preset"], "mod")["on"], true);

    // Unknown parameter: isError with the alternatives.
    let (err, text) = c.call_raw(
        "update_preset",
        json!({"slot": 12, "spec": {"blocks": {"reverb": {"params": {"Feedbak": 10}}}}}),
    );
    assert!(err);
    assert!(
        text.contains("Feedbak") && text.contains("Decay") && text.contains("Pre-delay"),
        "{text}"
    );
    // Unknown spec fields are reported too.
    let (err, text) = c.call_raw(
        "update_preset",
        json!({"slot": 12, "spec": {"colour": "red"}}),
    );
    assert!(err && text.contains("colour"), "{text}");

    // Validate, list, copy, rename, clear.
    let v = c.call("validate_preset", json!({"slot": 12}));
    assert_eq!(v["ok"], true, "{v}");
    let list = c.call("list_presets", json!({}));
    assert_eq!(list["occupied"], 1);
    assert_eq!(list["empty_slots"], 199);
    assert_eq!(list["presets"][0]["name"], "Edge Dotted");
    c.call("copy_preset", json!({"from": 12, "to": "02-4"}));
    c.call(
        "rename_preset",
        json!({"slot": 13, "name": "Edge Variation"}),
    );
    let empty = c.call("find_empty_slot", json!({"from": 12}));
    assert_eq!(empty["slot"], 14);
    let (err, _) = c.call_raw("copy_preset", json!({"from": 12, "to": 13}));
    assert!(err, "copy refuses an occupied target");

    // The bank on disk parses and holds both presets.
    let disk = syx::load_bank(&std::fs::read(&bank).unwrap()).unwrap();
    assert_eq!(disk.get(12).unwrap().name(), "Edge Dotted");
    assert_eq!(disk.get(13).unwrap().name(), "Edge Variation");
    assert!(params::block("delay").is_on(disk.get(12).unwrap()));
    assert!(gsystem_protocol::validate::check(disk.get(12).unwrap()).is_ok());
    let mut bak = bank.clone().into_os_string();
    bak.push(".bak");
    // The first write of the session found an empty file: nothing to back up.
    assert!(!Path::new(&bak).exists());

    c.call("clear_slot", json!({"slot": 13}));
    let disk = syx::load_bank(&std::fs::read(&bank).unwrap()).unwrap();
    assert!(disk.get(13).is_none());
    assert_eq!(disk.occupied(), 1);

    // Prompt.
    let prompts = c.request("prompts/list", json!({}));
    assert_eq!(prompts["result"]["prompts"][0]["name"], "design_preset");
    let p = c.request(
        "prompts/get",
        json!({"name": "design_preset", "arguments": {"brief": "80s chorus clean"}}),
    );
    let text = p["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(text.contains("get_catalog") && text.contains("80s chorus clean"));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn vyzor_bank_saves_next_to_it_and_backs_up() {
    let dir = temp_dir("sqs");
    let sqs = dir.join("Factory.SQS");
    std::fs::copy(factory(), &sqs).unwrap();
    // A .syx from an earlier session is kept as .bak on the first write.
    std::fs::write(dir.join("Factory.syx"), b"old").unwrap();
    let mut c = Client::start(&["--bank", sqs.to_str().unwrap()]);
    let list = c.call("list_presets", json!({}));
    assert_eq!(list["occupied"], 100);
    assert!(list["note"].as_str().unwrap().contains("Factory.syx"));
    let r = c.call(
        "update_preset",
        json!({"slot": 0, "spec": {"name": "Scotty Edit"}}),
    );
    assert!(r["saved"].as_str().unwrap().contains("Factory.syx"));
    let disk = syx::load_bank(&std::fs::read(dir.join("Factory.syx")).unwrap()).unwrap();
    assert_eq!(disk.occupied(), 100);
    assert_eq!(disk.get(0).unwrap().name(), "Scotty Edit");
    assert_eq!(
        std::fs::read(dir.join("Factory.syx.bak")).unwrap(),
        b"old".to_vec()
    );
    // The .SQS itself is untouched.
    assert_eq!(
        std::fs::read(&sqs).unwrap(),
        std::fs::read(factory()).unwrap()
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn read_only_hides_writers() {
    let dir = temp_dir("ro");
    let sqs = dir.join("Factory.SQS");
    std::fs::copy(factory(), &sqs).unwrap();
    let mut c = Client::start(&["--bank", sqs.to_str().unwrap(), "--read-only"]);
    let names = c.tool_names();
    for w in WRITERS {
        assert!(
            !names.contains(&w.to_string()),
            "{w} listed in read-only mode"
        );
    }
    assert!(names.contains(&"get_preset".to_string()));
    let r = c.request(
        "tools/call",
        json!({"name": "create_preset", "arguments": {"spec": {}}}),
    );
    assert!(r["error"].is_object(), "{r}");
    let got = c.call("get_preset", json!({"slot": 3}));
    assert_eq!(got["preset"]["name"], "Gilmour's Delays");
    assert!(!dir.join("Factory.syx").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn audition_is_listed_only_with_a_port() {
    let dir = temp_dir("port");
    let bank = dir.join("a.syx");
    let mut c = Client::start(&["--bank", bank.to_str().unwrap(), "--port", "No Such Port"]);
    let names = c.tool_names();
    assert!(names.contains(&"audition".to_string()));
    c.call("create_preset", json!({"spec": {"name": "X"}}));
    // No unit here: the call fails as a tool error, and the server keeps running.
    let (err, text) = c.call_raw("audition", json!({"slot": 0}));
    assert!(err, "{text}");
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn negotiates_protocol_versions() {
    let dir = temp_dir("ver");
    let bank = dir.join("a.syx");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gsys"))
        .args(["mcp", "--bank", bank.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for (id, v) in [(1, "2024-11-05"), (2, "2025-03-26"), (3, "1999-01-01")] {
        writeln!(
            stdin,
            "{}",
            json!({"jsonrpc": "2.0", "id": id, "method": "initialize", "params": {"protocolVersion": v}})
        )
        .unwrap();
    }
    writeln!(stdin, "not json").unwrap();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let replies: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let versions: Vec<_> = replies[..3]
        .iter()
        .map(|r| r["result"]["protocolVersion"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(versions, ["2024-11-05", "2025-03-26", "2025-06-18"]);
    assert_eq!(replies[3]["error"]["code"], -32700);
    let _ = std::fs::remove_dir_all(dir);
}
