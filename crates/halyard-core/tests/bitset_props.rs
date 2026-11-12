//! Property tests for [`halyard_core::SmallBitset`] (M1.1 acceptance:
//! "100 % doc, proptest on bitset").
//!
//! The model is a `BTreeSet<u32>`; every operation the bitset exposes must
//! agree with the model, including the > 128-bit spill region (§5.7 forbids
//! a fixed `u64` bitset that caps at 64 packages).

use std::collections::BTreeSet;

use halyard_core::{PkgId, SmallBitset};
// `prelude` glob is the documented proptest usage (and carries the
// `prop_assert*` macros into scope); pedantic exempts prelude modules.
use proptest::prelude::*;

/// Model-backed op sequence for one property run.
#[derive(Clone, Debug)]
enum Op {
    Set(u32),
    Clear(u32),
    Union(Vec<u32>),
}

/// Indices biased towards the inline region (< 128) and the spill boundary,
/// plus fully random high indices.
fn idx_strategy() -> impl Strategy<Value = u32> {
    prop::num::u32::ANY.prop_map(|raw| match raw % 8 {
        0..=3 => raw % 128,
        4..=5 => 120 + (raw % 40),
        _ => 128 + (raw % 1500),
    })
}

fn op_strategy() -> impl Strategy<Value = Op> {
    let set: BoxedStrategy<Op> = idx_strategy().prop_map(Op::Set).boxed();
    let clear: BoxedStrategy<Op> = idx_strategy().prop_map(Op::Clear).boxed();
    let union: BoxedStrategy<Op> = proptest::collection::vec(idx_strategy(), 0..16)
        .prop_map(Op::Union)
        .boxed();
    prop::strategy::Union::new(vec![set, clear, union])
}

fn apply_ops(ops: &[Op]) -> (SmallBitset, BTreeSet<u32>) {
    let mut bitset = SmallBitset::new();
    let mut model = BTreeSet::new();
    for op in ops {
        match op {
            Op::Set(i) => {
                bitset.set(PkgId::new(*i));
                model.insert(*i);
            }
            Op::Clear(i) => {
                bitset.clear(PkgId::new(*i));
                model.remove(i);
            }
            Op::Union(items) => {
                let mut other = SmallBitset::new();
                for i in items {
                    other.set(PkgId::new(*i));
                    model.insert(*i);
                }
                bitset.union_with(&other);
            }
        }
    }
    (bitset, model)
}

proptest::proptest! {
    #[test]
    fn bitset_matches_btree_model(ops in proptest::collection::vec(op_strategy(), 0..64)) {
        let (bitset, model) = apply_ops(&ops);
        prop_assert_eq!(bitset.count() as usize, model.len());
        let collected: Vec<u32> = bitset.iter().map(PkgId::to_u32).collect();
        prop_assert_eq!(collected, model.iter().copied().collect::<Vec<u32>>());
        for i in model.iter().copied().chain(0u32..1700) {
            prop_assert_eq!(bitset.get(PkgId::new(i)), model.contains(&i), "mismatch at {}", i);
        }
        prop_assert_eq!(bitset.is_empty(), model.is_empty());
    }

    #[test]
    fn union_matches_model_union(
        left in proptest::collection::vec(idx_strategy(), 0..64),
        right in proptest::collection::vec(idx_strategy(), 0..64),
    ) {
        let mut a = SmallBitset::new();
        let mut a_model = BTreeSet::new();
        for i in &left {
            a.set(PkgId::new(*i));
            a_model.insert(*i);
        }
        let mut b = SmallBitset::new();
        let mut b_model = BTreeSet::new();
        for i in &right {
            b.set(PkgId::new(*i));
            b_model.insert(*i);
        }
        prop_assert_eq!(
            a.intersects(&b),
            a_model.iter().any(|i| b_model.contains(i))
        );
        prop_assert_eq!(
            a.is_superset_of(&b),
            b_model.iter().all(|i| a_model.contains(i))
        );
        a.union_with(&b);
        a_model.extend(b_model.iter().copied());
        let collected: Vec<u32> = a.iter().map(PkgId::to_u32).collect();
        prop_assert_eq!(collected, a_model.iter().copied().collect::<Vec<u32>>());
    }

    #[test]
    fn clone_and_clear_roundtrip(ops in proptest::collection::vec(op_strategy(), 0..64)) {
        let (bitset, model) = apply_ops(&ops);
        let clone = bitset.clone();
        prop_assert_eq!(clone, bitset.clone());
        let mut cleared = bitset.clone();
        for i in model.iter().copied() {
            cleared.clear(PkgId::new(i));
        }
        prop_assert!(cleared.is_empty());
        prop_assert_eq!(cleared.count(), 0);
    }
}
