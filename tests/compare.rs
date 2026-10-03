//! The parity tests' comparison (tests/common): it compares only the fields Python's output has, so a field the core
//! adds passes, while a field Python has that the core lacks, or a different value, is a difference.

#[allow(dead_code)] // the other parity tests use the rest of it
mod common;
use common::{compare, Diff};
use serde_json::json;

fn differences(got: serde_json::Value, want: serde_json::Value) -> Vec<String> {
    let mut diff = Diff::default();
    compare("r", &got, &want, &mut diff);
    diff.wrong
}

#[test]
fn a_field_only_the_core_has_is_not_a_difference() {
    let got = json!({"kills": 3, "summary": {"median": 0.5, "what_if": [1, 2]}});
    let want = json!({"kills": 3, "summary": {"median": 0.5}});
    assert!(differences(got, want).is_empty());
}

#[test]
fn a_field_python_has_and_the_core_lacks_is_a_difference() {
    let wrong = differences(json!({"summary": {}}), json!({"summary": {"median": 0.5}}));
    assert_eq!(wrong.len(), 1);
    assert!(wrong[0].contains("r.summary.median"));
}

#[test]
fn a_different_value_is_a_difference_and_float_noise_is_not() {
    assert_eq!(differences(json!({"a": 1.0, "b": "x"}), json!({"a": 1.5, "b": "y"})).len(), 2);
    assert!(differences(json!({"a": 1.0 + 1e-12}), json!({"a": 1.0})).is_empty());
    assert_eq!(differences(json!({"a": [1, 2]}), json!({"a": [1, 2, 3]})).len(), 1);
}
