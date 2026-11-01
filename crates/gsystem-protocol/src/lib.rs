//! Protocol support for the TC Electronic G-System.
//!
//! Everything here was derived from captured editor files (Vyzor `.SQS` /
//! `.Prs` / `.Ebf` banks) and the firmware updater; see `PROTOCOL.md` at the
//! repository root for the evidence behind each constant.
//!
//! The crate has no I/O dependencies: it turns bytes into structs and back.
//! Transport (MIDI, HID) lives in other crates.

pub mod bank;
pub mod preset;
pub mod sysex;
pub mod syx;
pub mod vyzor;

pub use bank::Bank;
pub use preset::Preset;
pub use sysex::{Message, SysexError};

/// Errors produced by the file parsers.
#[derive(Debug)]
pub enum Error {
    Sysex(SysexError),
    Io(std::io::Error),
    Format(&'static str),
}

impl From<SysexError> for Error {
    fn from(e: SysexError) -> Self {
        Error::Sysex(e)
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Sysex(e) => write!(f, "sysex: {e}"),
            Error::Io(e) => write!(f, "io: {e}"),
            Error::Format(s) => write!(f, "format: {s}"),
        }
    }
}
impl std::error::Error for Error {}
