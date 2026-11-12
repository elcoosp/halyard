//! Package identity and string interning (§5.1).
//!
//! All names and paths are interned once at discovery; hot loops carry
//! [`PkgId`] and [`Sym`] values, never `String`s. Symbols are assigned in
//! first-interned order, so identical discovery inputs produce identical
//! ids (invariant I3).

use std::collections::BTreeMap;

/// Dense package index (§5.1): every package in a [`crate::Workspace`]
/// owns one `PkgId`, assigned by registration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PkgId(u32);

impl PkgId {
    /// The first id handed out by a workspace.
    pub const FIRST: Self = Self(0);

    /// Wraps a raw index. Ids normally come from a workspace; the raw
    /// constructor exists for bitsets and tables keyed by package.
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Raw index value.
    pub const fn to_u32(self) -> u32 {
        self.0
    }

    /// Raw index as `usize` (widening; no truncation possible).
    pub const fn to_usize(self) -> usize {
        self.0 as usize
    }
}

/// Interned symbol: an index into the workspace [`Interner`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sym(u32);

impl Sym {
    /// Raw index value.
    pub const fn to_u32(self) -> u32 {
        self.0
    }
}

/// Arena-backed string interner (§5.1). Names and paths are interned once
/// at discovery and resolved through [`Interner::get`].
#[derive(Clone, Debug, Default)]
pub struct Interner {
    strings: Vec<Box<str>>,
    lookup: BTreeMap<Box<str>, Sym>,
}

impl Interner {
    /// New empty interner.
    pub fn new() -> Self {
        Self::default()
    }

    /// Interns `s`, returning its symbol (existing or freshly assigned in
    /// first-interned order).
    pub fn intern(&mut self, s: &str) -> Sym {
        if let Some(sym) = self.lookup.get(s).copied() {
            return sym;
        }
        // Saturation instead of a panic path: interning more than 2^32
        // distinct names would require more than 64 GiB of names alone,
        // which is unreachable for any real workspace. The determinism
        // tests pin normal behaviour.
        let sym = Sym(u32::try_from(self.strings.len()).unwrap_or(u32::MAX));
        self.strings.push(s.into());
        self.lookup.insert(s.into(), sym);
        sym
    }

    /// Resolves `sym` back to its string; `None` only for a symbol that
    /// was created by a different interner instance.
    pub fn get(&self, sym: Sym) -> Option<&str> {
        self.strings.get(sym.0 as usize).map(|s| &**s)
    }

    /// Number of interned strings.
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// True when nothing has been interned yet.
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_is_idempotent_and_ordered() {
        let mut interner = Interner::new();
        let a = interner.intern("alpha");
        let b = interner.intern("beta");
        let a_again = interner.intern("alpha");
        assert_eq!(a, a_again);
        assert_ne!(a, b);
        assert_eq!(interner.get(a), Some("alpha"));
        assert_eq!(interner.get(b), Some("beta"));
        assert_eq!(interner.len(), 2);
    }

    #[test]
    fn unknown_symbol_is_none() {
        let interner = Interner::new();
        assert_eq!(interner.get(Sym(u32::MAX)), None);
        assert!(interner.is_empty());
    }

    #[test]
    fn package_id_roundtrip() {
        let id = PkgId::new(7);
        assert_eq!(id.to_u32(), 7);
        assert_eq!(id.to_usize(), 7);
        assert_eq!(PkgId::FIRST.to_u32(), 0);
    }
}
