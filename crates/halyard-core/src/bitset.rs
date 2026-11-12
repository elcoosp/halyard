//! `SmallBitset` — inline bitset over [`PkgId`]s (§5.7).
//!
//! Inline capacity is 128 packages (two `u64` words); higher package
//! indices spill into a heap word array that grows on demand. A fixed
//! `u64`-mask design would cap workspaces at 64 packages, which §5.7
//! explicitly forbids ("do not use a fixed `u64`").
//!
//! Equality and iteration are *logical*: a spilled-but-cleared bitset
//! compares equal to a never-spilled one, and iteration yields set bits in
//! ascending package order.

use crate::id::PkgId;

/// Inline word count: two `u64` words = 128 packages.
const INLINE_WORDS: usize = 2;
/// Inline bit capacity.
const INLINE_BITS: u32 = 128;

/// Bitset over package ids, inline up to 128 packages and heap-backed
/// beyond (§5.7). Used to mark which packages a commit touches and which
/// packages a walk flag paints.
#[derive(Clone, Default)]
pub struct SmallBitset {
    inline: [u64; INLINE_WORDS],
    spill: Option<Box<[u64]>>,
}

/// Single-bit mask for `idx` within its word.
fn bit(idx: u32) -> u64 {
    1u64 << (idx % 64)
}

impl SmallBitset {
    /// Inline capacity in packages.
    pub const INLINE_CAPACITY: u32 = INLINE_BITS;

    /// Empty bitset.
    pub fn new() -> Self {
        Self {
            inline: [0; INLINE_WORDS],
            spill: None,
        }
    }

    /// Sets the bit for `pkg`, growing the spill array when needed.
    pub fn set(&mut self, pkg: PkgId) {
        let idx = pkg.to_u32();
        if idx < INLINE_BITS {
            if let Some(word) = self.inline.get_mut((idx / 64) as usize) {
                *word |= bit(idx);
            }
            return;
        }
        let offset = ((idx - INLINE_BITS) / 64) as usize;
        self.ensure_spill_words(offset + 1);
        if let Some(word) = self.spill.as_deref_mut().and_then(|s| s.get_mut(offset)) {
            *word |= bit(idx);
        }
    }

    /// Clears the bit for `pkg` (the spill array is not shrunk; equality
    /// and `is_empty` remain logical).
    pub fn clear(&mut self, pkg: PkgId) {
        let idx = pkg.to_u32();
        if idx < INLINE_BITS {
            if let Some(word) = self.inline.get_mut((idx / 64) as usize) {
                *word &= !bit(idx);
            }
            return;
        }
        let offset = ((idx - INLINE_BITS) / 64) as usize;
        if let Some(word) = self.spill.as_deref_mut().and_then(|s| s.get_mut(offset)) {
            *word &= !bit(idx);
        }
    }

    /// True when the bit for `pkg` is set.
    pub fn get(&self, pkg: PkgId) -> bool {
        let idx = pkg.to_u32();
        if idx < INLINE_BITS {
            return self
                .inline
                .get((idx / 64) as usize)
                .is_some_and(|w| w & bit(idx) != 0);
        }
        let offset = ((idx - INLINE_BITS) / 64) as usize;
        self.spill
            .as_deref()
            .and_then(|s| s.get(offset))
            .is_some_and(|w| w & bit(idx) != 0)
    }

    /// Union in place: every bit set in `other` becomes set here.
    pub fn union_with(&mut self, other: &Self) {
        for (word, other_word) in self.inline.iter_mut().zip(other.inline.iter()) {
            *word |= *other_word;
        }
        if let Some(other_spill) = other.spill.as_deref() {
            self.ensure_spill_words(other_spill.len());
            if let Some(spill) = self.spill.as_deref_mut() {
                for (word, other_word) in spill.iter_mut().zip(other_spill.iter()) {
                    *word |= *other_word;
                }
            }
        }
    }

    /// True when the two bitsets share at least one set bit.
    pub fn intersects(&self, other: &Self) -> bool {
        if self
            .inline
            .iter()
            .zip(other.inline.iter())
            .any(|(a, b)| a & b != 0)
        {
            return true;
        }
        match (self.spill.as_deref(), other.spill.as_deref()) {
            (Some(a), Some(b)) => a.iter().zip(b.iter()).any(|(x, y)| x & y != 0),
            _ => false,
        }
    }

    /// True when every bit set in `other` is also set here.
    pub fn is_superset_of(&self, other: &Self) -> bool {
        if !self
            .inline
            .iter()
            .zip(other.inline.iter())
            .all(|(a, b)| a & b == *b)
        {
            return false;
        }
        match (self.spill.as_deref(), other.spill.as_deref()) {
            (_, None) => true,
            (Some(a), Some(b)) => b
                .iter()
                .enumerate()
                .all(|(i, y)| a.get(i).copied().unwrap_or(0) & y == *y),
            (None, Some(_)) => false,
        }
    }

    /// Number of set bits.
    pub fn count(&self) -> u32 {
        let inline: u32 = self.inline.iter().map(|w| w.count_ones()).sum();
        let spill: u32 = self
            .spill
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|w| w.count_ones())
            .sum();
        inline.saturating_add(spill)
    }

    /// True when no bits are set (logical; a cleared spill counts as empty).
    pub fn is_empty(&self) -> bool {
        self.inline.iter().all(|w| *w == 0)
            && self.spill.as_deref().unwrap_or(&[]).iter().all(|w| *w == 0)
    }

    /// Set bits as package ids, in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = PkgId> + '_ {
        let inline_pairs = self
            .inline
            .iter()
            .enumerate()
            .map(|(i, w)| (u64::try_from(i).unwrap_or(u64::MAX), *w));
        let spill_pairs = self
            .spill
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .map(|(i, w)| (u64::try_from(INLINE_WORDS + i).unwrap_or(u64::MAX), *w));
        inline_pairs
            .chain(spill_pairs)
            .flat_map(move |(word_index, word)| {
                let base = word_index.saturating_mul(64);
                (0u32..64).filter_map(move |bit_index| {
                    if word & (1u64 << bit_index) != 0 {
                        let idx = base + u64::from(bit_index);
                        u32::try_from(idx).ok().map(PkgId::new)
                    } else {
                        None
                    }
                })
            })
    }

    /// Grows the spill array to at least `words` words, preserving bits.
    fn ensure_spill_words(&mut self, words: usize) {
        let spill = self
            .spill
            .get_or_insert_with(|| vec![0u64; words].into_boxed_slice());
        if spill.len() < words {
            let mut grown = vec![0u64; words];
            for (dst, src) in grown.iter_mut().zip(spill.iter()) {
                *dst = *src;
            }
            *spill = grown.into_boxed_slice();
        }
    }
}

impl PartialEq for SmallBitset {
    fn eq(&self, other: &Self) -> bool {
        if self.inline != other.inline {
            return false;
        }
        let a = self.spill.as_deref().unwrap_or(&[]);
        let b = other.spill.as_deref().unwrap_or(&[]);
        let longest = a.len().max(b.len());
        (0..longest).all(|i| a.get(i).copied().unwrap_or(0) == b.get(i).copied().unwrap_or(0))
    }
}

impl Eq for SmallBitset {}

impl std::fmt::Debug for SmallBitset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Deterministic rendering: the set of package ids, not the raw
        // words (snapshot output must not depend on spill bookkeeping).
        f.debug_set()
            .entries(self.iter().map(PkgId::to_u32))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_and_spill_bits_agree() {
        let mut set = SmallBitset::new();
        set.set(PkgId::new(0));
        set.set(PkgId::new(63));
        set.set(PkgId::new(64));
        set.set(PkgId::new(127));
        set.set(PkgId::new(128));
        set.set(PkgId::new(200));
        for idx in [0u32, 63, 64, 127, 128, 200] {
            assert!(set.get(PkgId::new(idx)), "bit {idx} must be set");
        }
        assert!(!set.get(PkgId::new(1)));
        assert!(!set.get(PkgId::new(126)));
        assert_eq!(set.count(), 6);
    }

    #[test]
    fn cleared_spill_is_logically_equal_to_never_spilled() {
        let mut spilled = SmallBitset::new();
        spilled.set(PkgId::new(200));
        spilled.clear(PkgId::new(200));
        assert_eq!(spilled, SmallBitset::new());
        assert!(spilled.is_empty());
    }

    #[test]
    fn debug_renders_sorted_ids() {
        let mut set = SmallBitset::new();
        set.set(PkgId::new(130));
        set.set(PkgId::new(2));
        assert_eq!(format!("{set:?}"), "{2, 130}");
    }
}
