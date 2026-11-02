//! The unit's user bank: 200 slots, 40 banks × 5 presets. On the wire slot
//! `n` is preset number `n + 100`; the factory bank (0..100) is read-only on
//! the unit and is not part of this model.

use crate::preset::{slot_label, Preset};

pub const SLOTS: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Bank {
    pub slots: Vec<Option<Preset>>,
}

/// The default bank is the full 200 empty slots (it used to be zero slots,
/// and the app panicked when the first preset read from the unit arrived).
impl Default for Bank {
    fn default() -> Bank {
        Bank::empty()
    }
}

impl Bank {
    pub fn empty() -> Bank {
        Bank {
            slots: vec![None; SLOTS],
        }
    }

    pub fn set(&mut self, index: usize, p: Preset) {
        if index < SLOTS {
            if self.slots.len() < SLOTS {
                self.slots.resize(SLOTS, None);
            }
            self.slots[index] = Some(p);
        }
    }

    pub fn get(&self, index: usize) -> Option<&Preset> {
        self.slots.get(index).and_then(|s| s.as_ref())
    }

    pub fn occupied(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    pub fn swap(&mut self, a: usize, b: usize) {
        if a < SLOTS && b < SLOTS {
            self.slots.swap(a, b);
        }
    }

    /// (label, name) pairs for display.
    pub fn listing(&self) -> Vec<(String, Option<String>)> {
        self.slots
            .iter()
            .enumerate()
            .map(|(i, s)| (slot_label(i), s.as_ref().map(|p| p.name())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bank_has_every_slot_and_accepts_presets() {
        let mut b = Bank::default();
        assert_eq!(b.slots.len(), SLOTS);
        b.set(0, Preset::blank("A"));
        b.set(199, Preset::blank("B"));
        assert_eq!(b.occupied(), 2);
        // A bank built by hand with too few slots grows instead of panicking.
        let mut short = Bank { slots: Vec::new() };
        short.set(5, Preset::blank("C"));
        assert_eq!(short.get(5).unwrap().name(), "C");
    }
}
