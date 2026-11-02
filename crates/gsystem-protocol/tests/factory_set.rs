use gsystem_protocol::preset::{slot_label, Preset};
use gsystem_protocol::sysex::Message;
use gsystem_protocol::{syx, vyzor};

fn factory() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../testdata/Factory_Set.SQS"
    ))
    .unwrap()
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
        if let Message::PresetDump { number, .. } = msg {
            assert!(
                (100..110).contains(&number),
                "file slot 0..10 = wire preset 100..110"
            );
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
    let b = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../testdata/Twilight.Prs"
    ))
    .unwrap();
    let forms = vyzor::parse_forms(&b).unwrap();
    assert_eq!(forms[0].file_name().unwrap(), "Twilight.Prs");
    let msgs = vyzor::script_messages(&forms[0]);
    // Vyzor's scripts: editor mode on, begin/end transfer, and parameter reads.
    assert!(msgs.iter().any(|(_, m)| matches!(
        m,
        Ok(Message::SetParam {
            block: 1,
            param: 0x1D,
            ..
        })
    )));
    let transfers = msgs
        .iter()
        .filter(|(_, m)| matches!(m, Ok(Message::Transfer { .. })))
        .count();
    let reads = msgs
        .iter()
        .filter(|(_, m)| matches!(m, Ok(Message::RequestParams { .. })))
        .count();
    assert!(transfers >= 2);
    assert!(reads >= 10);
}

/// Every value Vyzor's quick-start shows for "Scotty's Blues" (page 8),
/// read through the ported firmware parameter tables by name. Our field
/// positions were derived from the factory data independently, so this checks
/// the two sources against each other.
#[test]
fn parameter_tables_match_vyzor_screenshot() {
    use gsystem_protocol::levels::preset_levels;
    use gsystem_protocol::params::{block, BLOCKS};
    let bank = vyzor::load_bank(&factory()).unwrap();
    let p = bank.get(0).unwrap();
    let val = |key: &str, name: &str| {
        let (f, fp) = block(key)
            .fields(p)
            .into_iter()
            .find(|(_, fp)| fp.name() == name)
            .unwrap_or_else(|| panic!("{key}.{name}"));
        (f, fp.format(p.param_i24(f)))
    };
    let ty = |key: &str| block(key).effect_type(p).unwrap().name;
    assert_eq!(ty("comp"), "Sustaining");
    assert_eq!(ty("filter"), "Auto Wah");
    assert_eq!(ty("mod"), "Classic Chorus");
    assert_eq!(ty("pitch"), "Detune");
    assert_eq!(ty("reverb"), "Plate");
    assert_eq!(ty("delay"), "Single Delay");
    let on: Vec<_> = BLOCKS
        .iter()
        .filter(|b| b.is_on(p))
        .map(|b| b.key)
        .collect();
    assert_eq!(on, ["reverb", "eq"]);
    for (key, name, field, shown) in [
        ("comp", "Gain", 10, "-9 dB"),
        ("comp", "Drive", 11, "12"),
        ("comp", "Resp", 12, "5"),
        ("filter", "Speed", 20, "2.02 Hz"), // Vyzor shows 2.00 Hz
        ("mod", "Speed", 34, "0.12 Hz"),    // Vyzor shows 118 mHz
        ("pitch", "Delay1", 51, "7 ms"),
        ("pitch", "Delay2", 52, "4 ms"),
        ("pitch", "Mix", 57, "25 %"),
        ("reverb", "Decay", 67, "2.2 s"),
        ("reverb", "PreDly", 68, "94 ms"),
        ("reverb", "Shape", 69, "Curved"),
        ("reverb", "Size", 70, "Large"),
        ("reverb", "Hi Col", 71, "Real"),
        ("reverb", "Lo Col", 73, "Tight"),
        ("reverb", "Early", 75, "-25 dB"),
        ("reverb", "RevLev", 76, "-3 dB"),
        ("reverb", "Mix", 78, "35 %"),
        ("gate", "Mode", 82, "Soft"),
        ("gate", "Thres", 83, "-37 dB"),
        ("gate", "Damp", 84, "19 dB"),
        ("gate", "Speed", 85, "16 dB/s"),
        ("delay", "Delay", 89, "500 ms"),
        ("delay", "Tempo", 92, "1/4"),
        ("delay", "Fb", 96, "40 %"),
        ("delay", "Fbhcut", 102, "9.12 kHz"),
        ("delay", "Fblcut", 103, "50 Hz"),
        ("delay", "Mix", 106, "15 %"),
        ("eq", "Freq1", 116, "254 Hz"),
        ("eq", "Width1", 117, "1.0 oct"), // Vyzor shows 1.0
    ] {
        let (f, s) = val(key, name);
        assert_eq!((f, s.as_str()), (field, shown), "{key}.{name}");
    }
    // Factory presets: loops off, boost level set.
    let l = preset_levels(p);
    assert_eq!(l.loops, [false; 4]);
    assert_eq!(l.boost_level, 6);
}
