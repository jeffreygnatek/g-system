//! A bank of preset slots. The unit has 200 slots: 0..100 factory, 100..200
//! user (20 banks × 5 presets each half).

use crate::preset::{slot_label, Preset};

pub const SLOTS: usize = 200;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Bank {
    pub slots: Vec<Option<Preset>>,
}

impl Bank {
    pub fn empty() -> Bank {
        Bank {
            slots: vec![None; SLOTS],
        }
    }

    pub fn set(&mut self, index: usize, p: Preset) {
        if index < SLOTS {
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
