//! Source-derived counterfactual used only by the corpus attribution driver.
//! The shipping tail path never compiles this module.
use super::Value;
use oxpinyin_core::cost::{NAN_COST, NEG_INF_COST, POS_INF_COST};
use std::cell::Cell;
thread_local! { static REFERENCE: Cell<bool> = const { Cell::new(false) }; }
/// Select the former final-tail comparator for the next diagnostic decode.
pub fn set_reference_order(enabled: bool) {
    REFERENCE.set(enabled);
}
pub(super) fn reference_order() -> bool {
    REFERENCE.get()
}
fn probability(value: &Value) -> f32 {
    match value.cost {
        NAN_COST => f32::NAN,
        POS_INF_COST => f32::NEG_INFINITY,
        NEG_INF_COST => f32::INFINITY,
        cost => (-(cost as f64) * std::f64::consts::LN_2 / 1000.0) as f32,
    }
}
fn left_first(left: &Value, right: &Value) -> bool {
    // 074a2219 phonetic_lookup.h:174-178, GLib gqsort.c split/merge.
    // This is the retained baseline's x86-64 gint conversion counterfactual,
    // not the product comparator. No oracle inference is involved.
    let difference = -(probability(left) - probability(right));
    !(-2147483648.0..2147483648.0).contains(&difference) || difference < 1.0
}
pub(super) fn sort(values: &mut [Value]) {
    if values.len() < 2 {
        return;
    }
    let mid = values.len() / 2;
    sort(&mut values[..mid]);
    sort(&mut values[mid..]);
    let left = values[..mid].to_vec();
    let (mut l, mut r, mut out) = (0, mid, 0);
    while l < left.len() && r < values.len() {
        if left_first(&left[l], &values[r]) {
            values[out] = left[l];
            l += 1;
        } else {
            values[out] = values[r];
            r += 1;
        }
        out += 1;
    }
    values[out..out + left.len() - l].copy_from_slice(&left[l..]);
}
