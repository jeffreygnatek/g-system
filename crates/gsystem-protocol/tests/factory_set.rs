use gsystem_protocol::preset::{slot_label, Preset};
use gsystem_protocol::sysex::Message;
use gsystem_protocol::{syx, vyzor};

fn factory() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/Factory_Set.SQS")).unwrap()
}

#[test]
fn parses_two_documents() {
    let b = factory();
    let forms = vyzor::parse_forms(&b).unwrap();
    let kinds: Vec<_> = forms.iter().map(|f| f.document_kind().unwrap()).collect();
    assert_eq!(kinds, ["Preset", "User Bank"]);
}

#[test]
fn loads_100_factory_presets_with_valid_checksums() {
    let bank = vyzor::load_bank(&factory()).unwrap();
    assert_eq!(bank.occupied(), 100);
    assert_eq!(bank.get(0).unwrap().name(), "Scotty's Blues");
    assert_eq!(bank.get(3).unwrap().name(), "Gilmour's Delays");
    assert_eq!(bank.get(5).unwrap().name(), "Lofi Delay         *");
    assert_eq!(bank.get(99).unwrap().name(), "All Bypassed");
    assert_eq!(slot_label(99), "19-5");
}

#[test]
fn preset_dump_roundtrip_is_byte_exact() {
    let b = factory();
    let forms = vyzor::parse_forms(&b).unwrap();
    let body = forms[1].chunk(b"BODY").unwrap(); // forms[1] is the "User Bank" document
    // first ten slots keep their full header: compare encode() to the original bytes
    for slot in body.data.chunks(vyzor::SLOT_LEN).take(10) {
        let original = &slot[..gsystem_protocol::sysex::PRESET_DUMP_LEN];
        let msg = Message::parse(original).unwrap();
        assert_eq!(msg.encode(), original);
        if let Message::PresetDump { preset, .. } = msg {
            assert!(preset >= 0x64, "header preset number is 100+ in the file");
        }
    }
}

#[test]
fn syx_save_and_reload() {
    let bank = vyzor::load_bank(&factory()).unwrap();
    let bytes = syx::save_bank(&bank, 0);
    assert_eq!(bytes.len(), 100 * gsystem_protocol::sysex::PRESET_DUMP_LEN);
    let again = syx::load_bank(&bytes).unwrap();
    assert_eq!(again, bank);
}

#[test]
fn rename_and_params() {
    let bank = vyzor::load_bank(&factory()).unwrap();
    let mut p: Preset = bank.get(0).unwrap().clone();
    p.set_name("My Lead");
    assert_eq!(p.name(), "My Lead");
    // Known values from the first preset's payload (see PROTOCOL.md)
    let params = bank.get(0).unwrap().params_i24();
    assert!(params.contains(&-20));
    assert!(params.contains(&500));
}

#[test]
fn prs_file_has_editor_scripts() {
    let b = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/Twilight.Prs")).unwrap();
    let forms = vyzor::parse_forms(&b).unwrap();
    assert_eq!(forms[0].file_name().unwrap(), "Twilight.Prs");
    let msgs = vyzor::script_messages(&forms[0]);
    let edit_modes = msgs
        .iter()
        .filter(|(_, m)| matches!(m, Ok(Message::EditMode { .. })))
        .count();
    let params = msgs
        .iter()
        .filter(|(_, m)| matches!(m, Ok(Message::Param { .. })))
        .count();
    assert!(edit_modes >= 2);
    assert!(params >= 10);
}
