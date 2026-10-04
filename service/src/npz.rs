//! NumPy's .npz files as python/ writes them with `np.savez_compressed` (a zip of .npy arrays, deflated): the cut-off's
//! detector labels (faint.rs) and the area finder's maps (areas.rs), so Python's tools read what the app writes and the
//! app reads what Python wrote.

use std::io::{Cursor, Read, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;

/// An array's element type: bytes, or 32-bit floats (little-endian).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dtype {
    U8,
    F32,
}

impl Dtype {
    fn descr(self) -> &'static str {
        match self {
            Dtype::U8 => "|u1",
            Dtype::F32 => "<f4",
        }
    }

    fn size(self) -> usize {
        match self {
            Dtype::U8 => 1,
            Dtype::F32 => 4,
        }
    }
}

/// An array: its type, its shape (empty for a single value) and its bytes in C order.
pub struct Array {
    pub dtype: Dtype,
    pub shape: Vec<usize>,
    pub data: Vec<u8>,
}

impl Array {
    pub fn u8(shape: &[usize], data: Vec<u8>) -> Array {
        Array { dtype: Dtype::U8, shape: shape.to_vec(), data }
    }

    pub fn f32(shape: &[usize], values: &[f32]) -> Array {
        Array { dtype: Dtype::F32, shape: shape.to_vec(), data: values.iter().flat_map(|v| v.to_le_bytes()).collect() }
    }

    /// The .npy file (format 1.0): the magic, the header (padded to 64 bytes, as NumPy pads it) and the data.
    fn npy(&self) -> Vec<u8> {
        let shape = match self.shape.len() {
            0 => "()".to_string(),
            1 => format!("({},)", self.shape[0]),
            _ => format!("({})", self.shape.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")),
        };
        let mut header = format!("{{'descr': '{}', 'fortran_order': False, 'shape': {shape}, }}", self.dtype.descr());
        while (10 + header.len() + 1) % 64 != 0 {
            header.push(' ');
        }
        header.push('\n');
        let mut out = Vec::with_capacity(10 + header.len() + self.data.len());
        out.extend_from_slice(b"\x93NUMPY\x01\x00");
        out.extend_from_slice(&(header.len() as u16).to_le_bytes());
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(&self.data);
        out
    }

    /// An array from a .npy file's bytes (format 1.0 to 3.0, C order, bytes or little-endian 32-bit floats).
    fn from_npy(bytes: &[u8]) -> Result<Array, String> {
        if bytes.len() < 10 || &bytes[..6] != b"\x93NUMPY" {
            return Err("not a .npy array".into());
        }
        let (len, start) = match bytes[6] {
            1 => (u16::from_le_bytes([bytes[8], bytes[9]]) as usize, 10),
            _ if bytes.len() >= 12 => (u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize, 12),
            _ => return Err("a .npy header is cut short".into()),
        };
        let header = String::from_utf8_lossy(bytes.get(start..start + len).ok_or("a .npy header is cut short")?);
        let value = |key: &str| header.split(&format!("'{key}':")).nth(1).map(str::trim).unwrap_or_default().to_string();
        let dtype = match value("descr") {
            d if d.starts_with("'|u1'") || d.starts_with("'u1'") => Dtype::U8,
            d if d.starts_with("'<f4'") => Dtype::F32,
            d => return Err(format!("a .npy array of {d} is not read here")),
        };
        if value("fortran_order").starts_with("True") {
            return Err("a .npy array in Fortran order is not read here".into());
        }
        let shape_text = value("shape");
        let inside = shape_text.trim_start_matches('(').split(')').next().unwrap_or_default();
        let shape: Vec<usize> = inside.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        let n = shape.iter().product::<usize>() * dtype.size();
        let data = bytes.get(start + len..start + len + n).ok_or("a .npy array is cut short")?.to_vec();
        Ok(Array { dtype, shape, data })
    }

    /// The values as floats.
    pub fn floats(&self) -> Vec<f32> {
        match self.dtype {
            Dtype::U8 => self.data.iter().map(|&v| v as f32).collect(),
            Dtype::F32 => self.data.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
        }
    }
}

/// Writes the arrays as `np.savez_compressed` does (each as "<name>.npy", deflated); a file there is replaced.
pub fn save(path: &Path, arrays: &[(&str, &Array)]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        crate::disk::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // made in memory, then written: the same bytes a zip written straight to the file has
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, array) in arrays {
        zip.start_file(format!("{name}.npy"), options).map_err(|e| e.to_string())?;
        zip.write_all(&array.npy()).map_err(|e| e.to_string())?;
    }
    let bytes = zip.finish().map_err(|e| e.to_string())?.into_inner();
    crate::disk::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// One array of a .npz file.
pub fn load(path: &Path, name: &str) -> Result<Array, String> {
    let file = crate::disk::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut entry = zip.by_name(&format!("{name}.npy")).map_err(|e| format!("{}: {name}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Array::from_npy(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrays_read_back() {
        let dir = std::env::temp_dir().join(format!("aimview-npz-{}", std::process::id()));
        let p = dir.join("a.npz");
        let (img, boxes, none) = (Array::u8(&[2, 3], vec![1, 2, 3, 4, 5, 6]), Array::f32(&[1, 4], &[1.5, 2.0, 3.25, 4.0]), Array::u8(&[], vec![0]));
        save(&p, &[("rgb", &img), ("boxes", &boxes), ("hidden", &none)]).unwrap();
        let a = load(&p, "rgb").unwrap();
        assert_eq!((a.dtype, a.shape.clone(), a.data.clone()), (Dtype::U8, vec![2, 3], vec![1, 2, 3, 4, 5, 6]));
        assert_eq!(load(&p, "boxes").unwrap().floats(), vec![1.5, 2.0, 3.25, 4.0]);
        assert_eq!(load(&p, "hidden").unwrap().shape, Vec::<usize>::new());
        assert_eq!((img.npy().len() - 6) % 64, 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
