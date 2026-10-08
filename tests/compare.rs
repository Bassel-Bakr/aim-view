//! The parity tests' comparison (tests/common): it compares only the fields Python's output has, so a field the core
//! adds passes, while a field Python has that the core lacks, or a different value, is a difference.

mod common;
use common::{Diff, compare, rename_key};
use serde_json::json;

/// The differences `compare` finds between the core's JSON and Python's, from the root path "r".
fn differences(got: serde_json::Value, want: serde_json::Value) -> Vec<String> {
    let mut diff = Diff::default();
    compare("r", &got, &want, &mut diff);
    diff.wrong
}

/// A field in the core's output that Python's lacks passes.
#[test]
fn a_field_only_the_core_has_is_not_a_difference() {
    let got = json!({"kills": 3, "summary": {"median": 0.5, "what_if": [1, 2]}});
    let want = json!({"kills": 3, "summary": {"median": 0.5}});
    assert!(differences(got, want).is_empty());
}

/// A field in Python's output that the core's lacks is a difference, named by its path.
#[test]
fn a_field_python_has_and_the_core_lacks_is_a_difference() {
    let wrong = differences(json!({"summary": {}}), json!({"summary": {"median": 0.5}}));
    assert_eq!(wrong.len(), 1);
    assert!(wrong[0].contains("r.summary.median"));
}

/// Different values and arrays of different lengths are differences; a number off in its last bits is not.
#[test]
fn a_different_value_is_a_difference_and_float_noise_is_not() {
    assert_eq!(differences(json!({"a": 1.0, "b": "x"}), json!({"a": 1.5, "b": "y"})).len(), 2);
    assert!(differences(json!({"a": 1.0 + 1e-12}), json!({"a": 1.0})).is_empty());
    assert_eq!(differences(json!({"a": [1, 2]}), json!({"a": [1, 2, 3]})).len(), 1);
}

/// `rename_key` renames the key in every array element its path reaches, nested too, and a path to nothing changes
/// nothing.
#[test]
fn a_renamed_key_moves_in_every_element_the_path_reaches() {
    let mut value = json!({"flicks": [{"n": 1, "speed": {"v": [2]}}, {"n": 3}], "n": 4});
    rename_key(&mut value, "flicks[].n", "kill_number");
    rename_key(&mut value, "flicks[].speed.v", "speeds");
    rename_key(&mut value, "missing[].n", "kill_number");
    assert_eq!(value, json!({"flicks": [{"kill_number": 1, "speed": {"speeds": [2]}}, {"kill_number": 3}], "n": 4}));
    let mut list = json!([{"dir": 90}]);
    rename_key(&mut list, "[].dir", "direction_deg");
    assert_eq!(list, json!([{"direction_deg": 90}]));
}
