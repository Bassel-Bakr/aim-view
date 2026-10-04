//! JSON as python/server.py reads and writes it, so the files the app keeps are the review server's, byte for byte:
//! numbers read exactly (serde_json's default parser can be a bit off in the last place, and a box saved back must be
//! the box the page sent), and written as Python's `json.dumps` writes them (", " and ": " between items, or one space
//! of indent, non-ASCII as \u escapes, floats as Python's `repr`).

use std::io;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use serde_json::ser::{CharEscape, Formatter};

/// JSON text as a value, every number read exactly: integers stay integers, the rest are the nearest f64.
pub fn parse(text: &[u8]) -> Result<Value, String> {
    // numbers become marked strings, read again with Rust's exact parser once the structure is known
    let mut marked = Vec::with_capacity(text.len() + 64);
    let (mut i, mut in_string) = (0, false);
    while i < text.len() {
        let c = text[i];
        if in_string {
            marked.push(c);
            if c == b'\\' && i + 1 < text.len() {
                marked.push(text[i + 1]);
                i += 1;
            } else if c == b'"' {
                in_string = false;
            }
        } else if c == b'"' {
            in_string = true;
            marked.push(c);
        } else if c == b'-' || c.is_ascii_digit() {
            let start = i;
            while i < text.len() && matches!(text[i], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E') {
                i += 1;
            }
            marked.extend_from_slice(b"\"\\u0000");
            marked.extend_from_slice(&text[start..i]);
            marked.push(b'"');
            continue;
        } else {
            marked.push(c);
        }
        i += 1;
    }
    let v: Value = serde_json::from_slice(&marked).map_err(|e| e.to_string())?;
    numbers(v)
}

fn numbers(v: Value) -> Result<Value, String> {
    Ok(match v {
        Value::String(s) if s.starts_with('\0') => {
            let t = &s[1..];
            if let Ok(n) = t.parse::<i64>() {
                Value::from(n)
            } else {
                let f: f64 = t.parse().map_err(|_| format!("not a number: {t}"))?;
                serde_json::Number::from_f64(f).map(Value::Number).ok_or_else(|| format!("not a number: {t}"))?
            }
        }
        Value::Array(a) => Value::Array(a.into_iter().map(numbers).collect::<Result<_, _>>()?),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| Ok((k, numbers(v)?))).collect::<Result<_, String>>()?),
        v => v,
    })
}

/// `json.dumps(v)` (indent false) or `json.dumps(v, indent=1)`, as Python writes them.
pub fn to_vec<T: Serialize + ?Sized>(v: &T, indent: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, Python { indent, depth: 0, has_value: false });
    // only a map with keys that are not strings fails, and the app writes none
    let _ = v.serialize(&mut ser);
    out
}

/// Text as Python's `open(path, "w")` writes it: on Windows each "\n" as "\r\n". The folder is made when missing.
pub fn write_text(path: &Path, text: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        crate::disk::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    crate::disk::write(path, newlines(text)).map_err(|e| format!("{}: {e}", path.display()))
}

/// Text added at the end of a file, as Python's `open(path, "a")` adds it.
pub fn append_text(path: &Path, text: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        crate::disk::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    crate::disk::append(path, &newlines(text)).map_err(|e| format!("{}: {e}", path.display()))
}

/// `json.dump(v, open(path, "w"))`, or with `indent=1`.
pub fn dump<T: Serialize + ?Sized>(path: &Path, v: &T, indent: bool) -> Result<(), String> {
    write_text(path, &to_vec(v, indent))
}

/// A JSON file read exactly (`parse`); None when it is missing or not JSON.
pub fn load(path: &Path) -> Option<Value> {
    parse(&crate::disk::read(path).ok()?).ok()
}

fn newlines(text: &[u8]) -> Vec<u8> {
    if !cfg!(windows) {
        return text.to_vec();
    }
    let mut out = Vec::with_capacity(text.len() + text.len() / 16);
    for &b in text {
        if b == b'\n' {
            out.push(b'\r');
        }
        out.push(b);
    }
    out
}

/// A float as Python's `repr` writes it: the shortest digits that read back the same, in plain notation from 1e-4 up
/// to 1e16, else with an exponent of at least two digits.
pub fn float_repr(x: f64) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exp: i32 = exp.parse().unwrap_or(0);
    let decpt = exp + 1;
    let n = digits.len() as i32;
    let body = if -4 < decpt && decpt <= 16 {
        if decpt <= 0 {
            format!("0.{}{digits}", "0".repeat((-decpt) as usize))
        } else if decpt >= n {
            format!("{digits}{}.0", "0".repeat((decpt - n) as usize))
        } else {
            format!("{}.{}", &digits[..decpt as usize], &digits[decpt as usize..])
        }
    } else {
        let rest = if n > 1 { format!(".{}", &digits[1..]) } else { String::new() };
        format!("{}{rest}e{}{:02}", &digits[..1], if exp < 0 { '-' } else { '+' }, exp.abs())
    };
    if x < 0.0 { format!("-{body}") } else { body }
}

/// serde_json's writer, with Python's separators, indent and escapes.
struct Python {
    indent: bool,
    depth: usize,
    has_value: bool,
}

impl Python {
    fn newline<W: ?Sized + io::Write>(&self, w: &mut W) -> io::Result<()> {
        w.write_all(b"\n")?;
        w.write_all(" ".repeat(self.depth).as_bytes())
    }

    fn open<W: ?Sized + io::Write>(&mut self, w: &mut W, bracket: &[u8]) -> io::Result<()> {
        self.depth += 1;
        self.has_value = false;
        w.write_all(bracket)
    }

    fn close<W: ?Sized + io::Write>(&mut self, w: &mut W, bracket: &[u8]) -> io::Result<()> {
        self.depth -= 1;
        if self.indent && self.has_value {
            self.newline(w)?;
        }
        self.has_value = true;
        w.write_all(bracket)
    }

    fn item<W: ?Sized + io::Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        match (first, self.indent) {
            (true, false) => Ok(()),
            (false, false) => w.write_all(b", "),
            (first, true) => {
                if !first {
                    w.write_all(b",")?;
                }
                self.newline(w)
            }
        }
    }
}

impl Formatter for Python {
    fn write_f64<W: ?Sized + io::Write>(&mut self, w: &mut W, value: f64) -> io::Result<()> {
        w.write_all(float_repr(value).as_bytes())
    }

    fn write_f32<W: ?Sized + io::Write>(&mut self, w: &mut W, value: f32) -> io::Result<()> {
        self.write_f64(w, value as f64)
    }

    fn write_string_fragment<W: ?Sized + io::Write>(&mut self, w: &mut W, fragment: &str) -> io::Result<()> {
        for c in fragment.chars() {
            if c.is_ascii() && c != '\x7f' {
                w.write_all(&[c as u8])?;
            } else {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    write!(w, "\\u{u:04x}")?;
                }
            }
        }
        Ok(())
    }

    fn write_char_escape<W: ?Sized + io::Write>(&mut self, w: &mut W, escape: CharEscape) -> io::Result<()> {
        let s: &[u8] = match escape {
            CharEscape::Quote => b"\\\"",
            CharEscape::ReverseSolidus => b"\\\\",
            CharEscape::Solidus => b"/",
            CharEscape::Backspace => b"\\b",
            CharEscape::FormFeed => b"\\f",
            CharEscape::LineFeed => b"\\n",
            CharEscape::CarriageReturn => b"\\r",
            CharEscape::Tab => b"\\t",
            CharEscape::AsciiControl(b) => return write!(w, "\\u{b:04x}"),
        };
        w.write_all(s)
    }

    fn begin_array<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.open(w, b"[")
    }

    fn end_array<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.close(w, b"]")
    }

    fn begin_array_value<W: ?Sized + io::Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        self.item(w, first)
    }

    fn end_array_value<W: ?Sized + io::Write>(&mut self, _w: &mut W) -> io::Result<()> {
        self.has_value = true;
        Ok(())
    }

    fn begin_object<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.open(w, b"{")
    }

    fn end_object<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.close(w, b"}")
    }

    fn begin_object_key<W: ?Sized + io::Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        self.item(w, first)
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, w: &mut W) -> io::Result<()> {
        w.write_all(b": ")
    }

    fn end_object_value<W: ?Sized + io::Write>(&mut self, _w: &mut W) -> io::Result<()> {
        self.has_value = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn floats_as_python_writes_them() {
        for (x, s) in [
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
            assert_eq!(float_repr(x), s);
        }
    }

    #[test]
    fn json_as_python_writes_it() {
        let v = json!([[0.5, 1, "Webcam"], {"a": null, "b": [true]}]);
        assert_eq!(String::from_utf8(to_vec(&v, false)).unwrap(), r#"[[0.5, 1, "Webcam"], {"a": null, "b": [true]}]"#);
        assert_eq!(String::from_utf8(to_vec(&json!(["a｜b", []]), true)).unwrap(), "[\n \"a\\uff5cb\",\n []\n]");
        assert_eq!(String::from_utf8(to_vec(&json!([{"id": "x"}]), true)).unwrap(), "[\n {\n  \"id\": \"x\"\n }\n]");
    }

    #[test]
    fn numbers_read_exactly() {
        let text = br#"[0.11423910861614354, 1, -2.5e-7, "a \" 1.5", {"k": 0.1}]"#;
        let v = parse(text).unwrap();
        assert_eq!(v[0].as_f64(), Some("0.11423910861614354".parse::<f64>().unwrap()));
        assert!(v[1].is_i64() && v[2].as_f64() == Some(-2.5e-7) && v[3] == "a \" 1.5" && v[4]["k"].as_f64() == Some(0.1));
    }
}
