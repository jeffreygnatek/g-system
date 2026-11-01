//! Vyzor editor files (`.SQS` set, `.Prs` preset, `.Ebf` edit buffer).
//!
//! IFF-style big-endian containers:
//! ```text
//! LIST <len> "    "          (an .SQS set: one FORM per document)
//!   FORM <len> "BANK"
//!     SHDR  370  header: "TCE G-System", document kind ("Preset", "Global", "Edit Buffer")
//!     COMM/FIND/AUTH/CPRT/SONG/MSCN/BAND/LNAM   text fields
//!     FILE  n    original file name
//!     TRAN  n    editor transmit script (bytecode; opcode 0x08 = send SysEx, 0x06 = delay ms)
//!     RECV  n    expected receive script
//!     XTRA/BDEF/TDEF/AUDI   misc
//!     BODY  n    data: for a preset document, N × 1310-byte slots
//! ```
//! A BODY slot is a framed 1044-byte preset dump padded with zeros. Slots past
//! the first ten in the factory set have their 10-byte header zeroed out,
//! leaving `<1032 nibbles><chk>F7` at offset 10 — the parser handles both.

use crate::bank::Bank;
use crate::preset::Preset;
use crate::sysex::{
    from_nibbles, nibble_checksum, split_stream, Message, PRESET_DUMP_LEN, PRESET_NIBBLES,
    SYSEX_END, SYSEX_START,
};
use crate::Error;

pub const SLOT_LEN: usize = 1310;

#[derive(Debug, Clone)]
pub struct Chunk<'a> {
    pub tag: [u8; 4],
    pub data: &'a [u8],
    pub offset: usize,
}

#[derive(Debug, Clone)]
pub struct Form<'a> {
    pub kind: [u8; 4],
    pub chunks: Vec<Chunk<'a>>,
}

impl<'a> Form<'a> {
    pub fn chunk(&self, tag: &[u8; 4]) -> Option<&Chunk<'a>> {
        self.chunks.iter().find(|c| &c.tag == tag)
    }
    /// Document kind from SHDR (e.g. "Preset", "Global", "Edit Buffer").
    pub fn document_kind(&self) -> Option<String> {
        let s = self.chunk(b"SHDR")?;
        // "TCE G-System" at +0, document kind string at +16 (NUL terminated)
        let field = s.data.get(16..56)?;
        let end = field.iter().position(|&b| b == 0)?;
        Some(String::from_utf8_lossy(&field[..end]).into_owned())
    }
    pub fn file_name(&self) -> Option<String> {
        let c = self.chunk(b"FILE")?;
        let end = c.data.iter().position(|&b| b == 0).unwrap_or(c.data.len());
        Some(String::from_utf8_lossy(&c.data[..end]).into_owned())
    }
}

fn be32(b: &[u8]) -> Option<usize> {
    Some(u32::from_be_bytes(b.get(..4)?.try_into().ok()?) as usize)
}

/// Walk chunks starting at `b[0]`. `declared` is the parent's declared length;
/// Vyzor writes FORM lengths that are a few dozen bytes short of the real
/// content, so chunk data is sliced from `b` (which extends to end of file)
/// and only the *start* of each chunk is bounded by `declared`.
fn walk_chunks<'a>(b: &'a [u8], base: usize, declared: usize) -> Vec<Chunk<'a>> {
    let mut out = Vec::new();
    let mut off = 0;
    while off + 8 <= b.len() && off < declared {
        let tag: [u8; 4] = b[off..off + 4].try_into().unwrap();
        let Some(len) = be32(&b[off + 4..]) else {
            break;
        };
        let start = off + 8;
        let end = (start + len).min(b.len());
        if tag == [0; 4] {
            break; // trailing padding in .SQS
        }
        out.push(Chunk {
            tag,
            data: &b[start..end],
            offset: base + start,
        });
        off = start + len + (len & 1);
    }
    out
}

/// Parse all FORMs in a Vyzor file (one for .Prs/.Ebf, several for .SQS).
pub fn parse_forms(b: &[u8]) -> Result<Vec<Form<'_>>, Error> {
    if b.len() < 12 {
        return Err(Error::Format("file too short"));
    }
    let top = &b[..4];
    let mut forms: Vec<Form<'_>> = Vec::new();
    // `start` is the offset of the FORM's data (the "BANK" kind tag) in `b`.
    fn make_form(b: &[u8], start: usize, declared: usize) -> Option<Form<'_>> {
        let body = b.get(start..)?;
        (body.len() >= 4).then(|| {
            let kind: [u8; 4] = body[..4].try_into().unwrap();
            Form {
                kind,
                chunks: walk_chunks(&body[4..], start + 4, declared.saturating_sub(4)),
            }
        })
    }
    if top == b"FORM" {
        let len = be32(&b[4..]).ok_or(Error::Format("bad FORM length"))?;
        forms.extend(make_form(b, 8, len));
    } else if top == b"LIST" {
        let len = be32(&b[4..]).ok_or(Error::Format("bad LIST length"))?;
        for c in walk_chunks(&b[12..], 12, len.saturating_sub(4)) {
            if &c.tag == b"FORM" {
                let declared = c.data.len();
                forms.extend(make_form(b, c.offset, declared));
            }
        }
    } else {
        return Err(Error::Format("not a Vyzor FORM/LIST file"));
    }
    Ok(forms)
}

/// Decode one 1310-byte BODY slot. Returns `(preset_no_from_header, preset)`.
pub fn decode_slot(slot: &[u8]) -> Option<(Option<u8>, Preset)> {
    if slot.len() < PRESET_DUMP_LEN || slot.iter().all(|&b| b == 0) {
        return None;
    }
    let nibbles = &slot[10..10 + PRESET_NIBBLES];
    let chk = slot[10 + PRESET_NIBBLES];
    if slot[11 + PRESET_NIBBLES] != SYSEX_END || nibble_checksum(nibbles) != chk {
        return None;
    }
    let data = from_nibbles(nibbles).ok()?;
    let no = (slot[0] == SYSEX_START).then_some(slot[9]);
    Some((no, Preset::from_raw(data)?))
}

/// Load every preset document's BODY into a bank, by slot position.
pub fn load_bank(b: &[u8]) -> Result<Bank, Error> {
    let forms = parse_forms(b)?;
    let mut bank = Bank::empty();
    let mut any = false;
    // Document kinds seen: "Preset" (single preset, body holds edit messages,
    // not a dump), "Edit Buffer", "User Bank" (200 × 1310-byte dump slots).
    // Try every BODY; only real dump slots decode.
    for f in &forms {
        let Some(body) = f.chunk(b"BODY") else {
            continue;
        };
        if body.data.len() % SLOT_LEN != 0 {
            continue;
        }
        for (i, slot) in body.data.chunks(SLOT_LEN).enumerate() {
            if let Some((_, p)) = decode_slot(slot) {
                bank.set(i, p);
                any = true;
            }
        }
    }
    if any {
        Ok(bank)
    } else {
        Err(Error::Format("no preset data in file"))
    }
}

/// Every framed SysEx message found in a form's TRAN and RECV scripts, in order.
/// Best-effort: the scripts are bytecode with length-prefixed SysEx inside.
pub fn script_messages(form: &Form<'_>) -> Vec<(&'static str, Result<Message, crate::SysexError>)> {
    let mut out = Vec::new();
    for (label, tag) in [("TRAN", b"TRAN"), ("RECV", b"RECV")] {
        if let Some(c) = form.chunk(tag) {
            for m in split_stream(c.data) {
                out.push((label, Message::parse(m)));
            }
        }
    }
    out
}
