//! What the parity tests share: comparing the core's JSON with Python's.

use std::fs;
use std::path::Path;

use serde_json::Value;

/// Where two JSON values differ (paths), and how many numbers were equal only within the tolerance.
#[derive(Default)]
pub struct Diff {
    pub wrong: Vec<String>,
    pub close: usize,
}

pub fn compare(path: &str, got: &Value, want: &Value, diff: &mut Diff) {
    match (got, want) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            if a != b {
                if (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1e-3) {
                    diff.close += 1;
                } else {
                    diff.wrong.push(format!("{path}: {a} against {b}"));
                }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                compare(&format!("{path}[{i}]"), x, y, diff);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for k in a.keys().chain(b.keys().filter(|k| !a.contains_key(*k))) {
                match (a.get(k), b.get(k)) {
                    (Some(x), Some(y)) => compare(&format!("{path}.{k}"), x, y, diff),
                    (x, y) => diff.wrong.push(format!("{path}.{k}: {x:?} against {y:?}")),
                }
            }
        }
        _ if got == want => {}
        _ => diff.wrong.push(format!("{path}: {got} against {want}")),
    }
}

pub fn read(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

