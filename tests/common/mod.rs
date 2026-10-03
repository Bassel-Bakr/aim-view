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
        // only the fields Python's output has: a field the core adds (Python has no such thing) is not compared, so a
        // new field needs no change here; a field Python has and the core lacks is a difference
        (Value::Object(a), Value::Object(b)) => {
            for (k, y) in b {
                match a.get(k) {
                    Some(x) => compare(&format!("{path}.{k}"), x, y, diff),
                    None => diff.wrong.push(format!("{path}.{k}: missing against {y}")),
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

