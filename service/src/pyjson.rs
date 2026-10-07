//! JSON as python/retired/server.py read and wrote it, so the files the app keeps are the review server's, byte for
//! byte.
//!
//! In: the kept JSON items (store.rs: areas and found areas, area kinds and examples, cut-offs, labelling lists) and
//! the page's bodies. Out: values for the library (areas.rs, faint.rs, finder.rs, labels.rs) and those items kept
//! again.
//! Numbers are read exactly (serde_json's default parser can be a bit off in the last place, and a box saved back must
//! be the box the page sent), and written as Python's `json.dumps` writes them (", " and ": " between items, or one
//! space of indent, non-ASCII as \u escapes, floats as Python's `repr`).

use std::io;
use std::ops::Range;

use serde::Serialize;
use serde_json::Value;
use serde_json::ser::{CharEscape, Formatter};

use crate::store::{Item, Store};

/// What `parse` puts before a number's text, making it a JSON string that starts with a NUL character, which
/// `numbers` reads back as a number.
const NUMBER_MARK: &[u8] = b"\"\\u0000";
/// Room for the marks `parse` adds, in bytes beyond the text's own: a guess, the buffer grows when it needs to.
const MARK_ROOM_BYTES: usize = 64;
/// Room for one "\r" in this many bytes of text: a guess, the buffer grows when it needs to.
const BYTES_PER_LINE_GUESS: usize = 16;
/// The exponents (of the float as d.ddd times ten to the exponent) that Python's `repr` writes in plain notation:
/// 1e-4 up to 1e16, not reaching it.
const PLAIN_EXPONENTS: Range<i32> = -4..16;

/// JSON text as a value, every number read exactly: integers stay integers, the rest are the nearest f64.
pub fn parse(text: &[u8]) -> Result<Value, String> {
    // numbers become marked strings, read again with Rust's exact parser once the structure is known
    let mut marked = Vec::with_capacity(text.len() + MARK_ROOM_BYTES);
    let (mut i, mut in_string) = (0, false);
    while i < text.len() {
        let byte = text[i];
        if in_string {
            marked.push(byte);
            if byte == b'\\' && i + 1 < text.len() {
                marked.push(text[i + 1]);
                i += 1;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if byte == b'"' {
            in_string = true;
            marked.push(byte);
        } else if byte == b'-' || byte.is_ascii_digit() {
            let start = i;
            while i < text.len() && matches!(text[i], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E') {
                i += 1;
            }
            marked.extend_from_slice(NUMBER_MARK);
            marked.extend_from_slice(&text[start..i]);
            marked.push(b'"');
            continue;
        } else {
            marked.push(byte);
        }
        i += 1;
    }
    let value: Value = serde_json::from_slice(&marked).map_err(|error| error.to_string())?;
    numbers(value)
}

/// The value with the strings `parse` marked read back as numbers.
fn numbers(value: Value) -> Result<Value, String> {
    Ok(match value {
        Value::String(marked) if marked.starts_with('\0') => number(&marked[1..])?,
        Value::Array(items) => Value::Array(items.into_iter().map(numbers).collect::<Result<_, _>>()?),
        Value::Object(fields) => Value::Object(
            fields.into_iter().map(|(key, field)| Ok((key, numbers(field)?))).collect::<Result<_, String>>()?,
        ),
        other => other,
    })
}

/// A number's text as a value: an integer when it reads as one, else the nearest f64.
fn number(text: &str) -> Result<Value, String> {
    if let Ok(integer) = text.parse::<i64>() {
        return Ok(Value::from(integer));
    }
    let float: f64 = text.parse().map_err(|_| format!("not a number: {text}"))?;
    serde_json::Number::from_f64(float).map(Value::Number).ok_or_else(|| format!("not a number: {text}"))
}

/// `json.dumps(value)` (indent false) or `json.dumps(value, indent=1)`, as Python writes them.
pub fn to_vec<T: Serialize + ?Sized>(value: &T, indent: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let formatter = Python { indent, depth: 0, has_value: false };
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    // only a map with keys that are not strings fails, and the app writes none
    let _ = value.serialize(&mut serializer);
    out
}

/// Text kept as Python's `open(path, "w")` writes a file (store.rs: `Files`); a failure names where it is kept.
pub fn write_text(store: &dyn Store, item: Item<'_>, text: &[u8]) -> Result<(), String> {
    store.write(item, text).map_err(|error| format!("{}: {error}", store.name(item)))
}

/// Text added at the end of a kept item, as Python's `open(path, "a")` adds it to a file.
pub fn append_text(store: &dyn Store, item: Item<'_>, text: &[u8]) -> Result<(), String> {
    store.append(item, text).map_err(|error| format!("{}: {error}", store.name(item)))
}

/// `json.dump(value, open(path, "w"))`, or with `indent=1`, into a kept item.
pub fn dump<T: Serialize + ?Sized>(store: &dyn Store, item: Item<'_>, value: &T, indent: bool) -> Result<(), String> {
    write_text(store, item, &to_vec(value, indent))
}

/// A kept JSON item read exactly (`parse`); None when nothing is kept or it is not JSON.
pub fn load(store: &dyn Store, item: Item<'_>) -> Option<Value> {
    parse(&store.read(item).ok()??).ok()
}

/// Text with each "\n" as "\r\n" on Windows, as Python's text files write it; elsewhere unchanged.
pub(crate) fn newlines(text: &[u8]) -> Vec<u8> {
    if !cfg!(windows) {
        return text.to_vec();
    }
    let mut out = Vec::with_capacity(text.len() + text.len() / BYTES_PER_LINE_GUESS);
    for &byte in text {
        if byte == b'\n' {
            out.push(b'\r');
        }
        out.push(byte);
    }
    out
}

/// A float as Python's `repr` writes it: the shortest digits that read back the same, in plain notation from 1e-4 up
/// to 1e16, else with an exponent of at least two digits.
pub fn float_repr(x: f64) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let scientific = format!("{:e}", x.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let body = if PLAIN_EXPONENTS.contains(&exponent) {
        plain_notation(&digits, exponent)
    } else {
        exponent_notation(&digits, exponent)
    };
    if x < 0.0 { format!("-{body}") } else { body }
}

/// A float's digits (no point, no sign) with the point where its exponent puts it: "0.000ddd", "ddd000.0" or "dd.d".
fn plain_notation(digits: &str, exponent: i32) -> String {
    let (point, digit_count) = (exponent + 1, digits.len() as i32);
    if point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else if point >= digit_count {
        format!("{digits}{}.0", "0".repeat((point - digit_count) as usize))
    } else {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    }
}

/// A float's digits (no point, no sign) with its exponent: "d.ddde-05", "de+16".
fn exponent_notation(digits: &str, exponent: i32) -> String {
    let rest = if digits.len() > 1 { format!(".{}", &digits[1..]) } else { String::new() };
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{}{rest}e{sign}{:02}", &digits[..1], exponent.abs())
}

/// serde_json's writer, with Python's separators, indent and escapes.
struct Python {
    /// Whether to write with `indent=1`: each item on a line of its own, one space a level.
    indent: bool,
    /// How many arrays and objects the writer is inside.
    depth: usize,
    /// Whether the array or object being written has an item yet (an empty one closes on the same line).
    has_value: bool,
}

impl Python {
    /// A new line, indented one space a level.
    fn newline<W: ?Sized + io::Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b"\n")?;
        writer.write_all(" ".repeat(self.depth).as_bytes())
    }

    /// Opens an array or an object with `bracket`, one level deeper.
    fn open<W: ?Sized + io::Write>(&mut self, writer: &mut W, bracket: &[u8]) -> io::Result<()> {
        self.depth += 1;
        self.has_value = false;
        writer.write_all(bracket)
    }

    /// Closes an array or an object with `bracket`, on a new line when indented and it has items.
    fn close<W: ?Sized + io::Write>(&mut self, writer: &mut W, bracket: &[u8]) -> io::Result<()> {
        self.depth -= 1;
        if self.indent && self.has_value {
            self.newline(writer)?;
        }
        self.has_value = true;
        writer.write_all(bracket)
    }

    /// What comes before an item: ", " between items, or with indent "," and a new line.
    fn item<W: ?Sized + io::Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        match (first, self.indent) {
            (true, false) => Ok(()),
            (false, false) => writer.write_all(b", "),
            (first, true) => {
                if !first {
                    writer.write_all(b",")?;
                }
                self.newline(writer)
            }
        }
    }
}

impl Formatter for Python {
    /// A float as Python's `repr` writes it.
    fn write_f64<W: ?Sized + io::Write>(&mut self, writer: &mut W, value: f64) -> io::Result<()> {
        writer.write_all(float_repr(value).as_bytes())
    }

    /// A 32-bit float, widened, as Python's `repr` writes it.
    fn write_f32<W: ?Sized + io::Write>(&mut self, writer: &mut W, value: f32) -> io::Result<()> {
        self.write_f64(writer, value as f64)
    }

    /// Text with every character beyond ASCII (and DEL) as \u escapes, as `ensure_ascii` writes it.
    fn write_string_fragment<W: ?Sized + io::Write>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()> {
        for character in fragment.chars() {
            if character.is_ascii() && character != '\x7f' {
                writer.write_all(&[character as u8])?;
            } else {
                let mut units = [0u16; 2];
                for unit in character.encode_utf16(&mut units) {
                    write!(writer, "\\u{unit:04x}")?;
                }
            }
        }
        Ok(())
    }

    /// An escaped character as Python writes it: its short escapes, other control characters as \u00XX.
    fn write_char_escape<W: ?Sized + io::Write>(&mut self, writer: &mut W, escape: CharEscape) -> io::Result<()> {
        let escaped: &[u8] = match escape {
            CharEscape::Quote => b"\\\"",
            CharEscape::ReverseSolidus => b"\\\\",
            CharEscape::Solidus => b"/",
            CharEscape::Backspace => b"\\b",
            CharEscape::FormFeed => b"\\f",
            CharEscape::LineFeed => b"\\n",
            CharEscape::CarriageReturn => b"\\r",
            CharEscape::Tab => b"\\t",
            CharEscape::AsciiControl(byte) => return write!(writer, "\\u{byte:04x}"),
        };
        writer.write_all(escaped)
    }

    /// "[", one level deeper.
    fn begin_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.open(writer, b"[")
    }

    /// "]", on a new line when indented and the array has items.
    fn end_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.close(writer, b"]")
    }

    /// What comes before an item (`item`).
    fn begin_array_value<W: ?Sized + io::Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        self.item(writer, first)
    }

    /// Notes that the array has an item.
    fn end_array_value<W: ?Sized + io::Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        self.has_value = true;
        Ok(())
    }

    /// "{", one level deeper.
    fn begin_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.open(writer, b"{")
    }

    /// "}", on a new line when indented and the object has items.
    fn end_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.close(writer, b"}")
    }

    /// What comes before a key (`item`).
    fn begin_object_key<W: ?Sized + io::Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        self.item(writer, first)
    }

    /// ": " between a key and its value, as Python writes it.
    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b": ")
    }

    /// Notes that the object has an item.
    fn end_object_value<W: ?Sized + io::Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        self.has_value = true;
        Ok(())
    }
}

/// Python's JSON, read and written.
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Floats are written as Python's `repr` writes them, in plain and exponent notation.
    #[test]
    fn floats_as_python_writes_them() {
        for (x, written) in [
            (0.16015625, "0.16015625"),
            (150.0 / 720.0, "0.20833333333333334"),
            (1.0, "1.0"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (-2.5e-7, "-2.5e-07"),
            (123.456, "123.456"),
            (1.5e300, "1.5e+300"),
        ] {
            assert_eq!(float_repr(x), written);
        }
    }

    /// Values are written with Python's separators, `indent=1` and \u escapes.
    #[test]
    fn json_as_python_writes_it() {
        let value = json!([[0.5, 1, "Webcam"], {"a": null, "b": [true]}]);
        let written = r#"[[0.5, 1, "Webcam"], {"a": null, "b": [true]}]"#;
        assert_eq!(String::from_utf8(to_vec(&value, false)).unwrap(), written);
        assert_eq!(String::from_utf8(to_vec(&json!(["a｜b", []]), true)).unwrap(), "[\n \"a\\uff5cb\",\n []\n]");
        assert_eq!(String::from_utf8(to_vec(&json!([{"id": "x"}]), true)).unwrap(), "[\n {\n  \"id\": \"x\"\n }\n]");
    }

    /// Numbers read to the nearest f64 (integers stay integers), and digits inside strings stay text.
    #[test]
    fn numbers_read_exactly() {
        let text = br#"[0.11423910861614354, 1, -2.5e-7, "a \" 1.5", {"k": 0.1}]"#;
        let value = parse(text).unwrap();
        assert_eq!(value[0].as_f64(), Some("0.11423910861614354".parse::<f64>().unwrap()));
        assert!(value[1].is_i64() && value[2].as_f64() == Some(-2.5e-7) && value[3] == "a \" 1.5");
        assert_eq!(value[4]["k"].as_f64(), Some(0.1));
    }
}
