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
/// checked against our field map.
#[test]
fn block_map_matches_vyzor_screenshot() {
    use gsystem_protocol::preset::BLOCKS;
    let bank = vyzor::load_bank(&factory()).unwrap();
    let p = bank.get(0).unwrap();
    let state = |id: &str| {
        p.block_state(BLOCKS.iter().find(|b| b.id == id).unwrap())
            .unwrap()
    };
    assert_eq!(state("comp"), (1, false)); // Sustaining, off
    assert_eq!(state("filter"), (5, false)); // Auto Wah, off
    assert_eq!(state("mod"), (0, false)); // Classic Chorus, off
    assert_eq!(state("pitch"), (3, false)); // Detune, off
    assert_eq!(state("reverb"), (3, true)); // Plate, on
    assert!(!state("gate").1);
    assert_eq!(state("delay"), (0, false)); // Single, off
    assert!(state("eq").1);
    for (field, want) in [
        (10, -9),
        (11, 12),
        (12, 5), // comp gain, drive, response
        (51, 7),
        (52, 4),
        (57, 25),
        (62, 12),
        (63, -14), // pitch delays, mix, detune voices
        (67, 22),
        (68, 94),
        (75, -25),
        (76, -3),
        (78, 35), // reverb decay, pre, early, level, mix
        (82, 1),
        (83, -37),
        (84, 19),
        (85, 16), // gate mode, threshold, damping, release
        (89, 500),
        (96, 40),
        (102, 53),
        (103, 8),
        (106, 15), // delay
        (116, 22),
        (119, 32),
        (122, 45), // EQ frequencies (sixth-octave index)
    ] {
        assert_eq!(p.param_i24(field), want, "field {field}");
    }
    // Every factory preset has the expected per-block counts.
    for slot in 0..100 {
        let p = bank.get(slot).unwrap();
        for b in BLOCKS {
            let n = p.param_i24(b.start) as usize;
            assert!(n >= 5 && n < b.len, "slot {slot} block {} count {n}", b.id);
        }
    }
}
