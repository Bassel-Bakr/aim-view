//! NumPy's .npz files as python/ writes them with `np.savez_compressed` (a zip of .npy arrays, deflated): the cut-off's
//! detector labels (faint.rs) and the area finder's maps (finder.rs), so Python's tools read what the app writes and
//! the app reads what Python wrote. In: arrays to save, or a file's bytes to read one from. Out: the file's bytes, or
//! the array.

use std::io::{Cursor, Read, Write};

use zip::write::SimpleFileOptions;

/// What every .npy file starts with.
const MAGIC: &[u8] = b"\x93NUMPY";
/// The format the app writes, 1.0 (major, minor): its header's length is a u16.
const VERSION_1: [u8; 2] = [1, 0];
/// Where the format's major version is, after the magic.
const MAJOR_AT: usize = MAGIC.len();
/// Where the header's length starts, after the version's two bytes.
const LENGTH_AT: usize = MAJOR_AT + 2;
/// The bytes before the header in format 1.0, where its length is a u16.
const PREAMBLE_BYTES_V1: usize = LENGTH_AT + 2;
/// The bytes before the header in formats 2.0 and 3.0, where its length is a u32.
const PREAMBLE_BYTES_V2: usize = LENGTH_AT + 4;
/// NumPy pads the header with spaces so the data starts at a multiple of this many bytes.
const HEADER_ALIGN: usize = 64;

/// An array's element type: bytes, or 32-bit floats (little-endian).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dtype {
    /// Unsigned bytes (NumPy's uint8).
    U8,
    /// Little-endian 32-bit floats (NumPy's float32).
    F32,
}

impl Dtype {
    /// The type as a .npy header names it ("|u1", "<f4").
    fn descr(self) -> &'static str {
        match self {
            Dtype::U8 => "|u1",
            Dtype::F32 => "<f4",
        }
    }

    /// The bytes of one element.
    fn size(self) -> usize {
        match self {
            Dtype::U8 => 1,
            Dtype::F32 => 4,
        }
    }
}

/// An array: its type, its shape (empty for a single value) and its bytes in C order.
pub struct Array {
    /// Its element type.
    pub dtype: Dtype,
    /// Its shape; empty for a single value.
    pub shape: Vec<usize>,
    /// Its elements' bytes in C order (the last index fastest).
    pub data: Vec<u8>,
}

impl Array {
    /// A byte array of `shape` holding `data` (as many bytes as the shape has elements).
    pub fn u8(shape: &[usize], data: Vec<u8>) -> Array {
        Array { dtype: Dtype::U8, shape: shape.to_vec(), data }
    }

    /// A float array of `shape` holding `values`, stored little-endian.
    pub fn f32(shape: &[usize], values: &[f32]) -> Array {
        let data = values.iter().flat_map(|value| value.to_le_bytes()).collect();
        Array { dtype: Dtype::F32, shape: shape.to_vec(), data }
    }

    /// The .npy file (format 1.0): the magic, the header (padded as NumPy pads it: HEADER_ALIGN) and the data.
    fn npy(&self) -> Vec<u8> {
        let shape = match self.shape.len() {
            0 => "()".to_string(),
            1 => format!("({},)", self.shape[0]),
            _ => format!("({})", self.shape.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")),
        };
        let mut header = format!("{{'descr': '{}', 'fortran_order': False, 'shape': {shape}, }}", self.dtype.descr());
        // the padding comes before the header's last byte, a newline
        while !(PREAMBLE_BYTES_V1 + header.len() + 1).is_multiple_of(HEADER_ALIGN) {
            header.push(' ');
        }
        header.push('\n');
        let mut out = Vec::with_capacity(PREAMBLE_BYTES_V1 + header.len() + self.data.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION_1);
        out.extend_from_slice(&(header.len() as u16).to_le_bytes());
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(&self.data);
        out
    }

    /// An array from a .npy file's bytes (format 1.0 to 3.0, C order, bytes or little-endian 32-bit floats).
    fn from_npy(bytes: &[u8]) -> Result<Array, String> {
        if bytes.len() < PREAMBLE_BYTES_V1 || !bytes.starts_with(MAGIC) {
            return Err("not a .npy array".into());
        }
        let (len, start) = match bytes[MAJOR_AT] {
            1 => (u16::from_le_bytes([bytes[LENGTH_AT], bytes[LENGTH_AT + 1]]) as usize, PREAMBLE_BYTES_V1),
            _ if bytes.len() >= PREAMBLE_BYTES_V2 => {
                let length = [bytes[LENGTH_AT], bytes[LENGTH_AT + 1], bytes[LENGTH_AT + 2], bytes[LENGTH_AT + 3]];
                (u32::from_le_bytes(length) as usize, PREAMBLE_BYTES_V2)
            }
            _ => return Err("a .npy header is cut short".into()),
        };
        let header = String::from_utf8_lossy(bytes.get(start..start + len).ok_or("a .npy header is cut short")?);
        let value =
            |key: &str| header.split(&format!("'{key}':")).nth(1).map(str::trim).unwrap_or_default().to_string();
        let dtype = match value("descr") {
            descr if descr.starts_with("'|u1'") || descr.starts_with("'u1'") => Dtype::U8,
            descr if descr.starts_with("'<f4'") => Dtype::F32,
            descr => return Err(format!("a .npy array of {descr} is not read here")),
        };
        if value("fortran_order").starts_with("True") {
            return Err("a .npy array in Fortran order is not read here".into());
        }
        let shape_text = value("shape");
        let inside = shape_text.trim_start_matches('(').split(')').next().unwrap_or_default();
        let shape: Vec<usize> = inside.split(',').filter_map(|dimension| dimension.trim().parse().ok()).collect();
        let data_bytes = shape.iter().product::<usize>() * dtype.size();
        let data = bytes.get(start + len..start + len + data_bytes).ok_or("a .npy array is cut short")?.to_vec();
        Ok(Array { dtype, shape, data })
    }

    /// The values as floats.
    pub fn floats(&self) -> Vec<f32> {
        match self.dtype {
            Dtype::U8 => self.data.iter().map(|&value| f32::from(value)).collect(),
            Dtype::F32 => self
                .data
                .chunks_exact(Dtype::F32.size())
                .map(|value| f32::from_le_bytes([value[0], value[1], value[2], value[3]]))
                .collect(),
        }
    }
}

/// The arrays as `np.savez_compressed` writes them (each as "<name>.npy", deflated): the file's bytes.
pub fn to_bytes(arrays: &[(&str, &Array)]) -> Result<Vec<u8>, String> {
    // made in memory: the same bytes a zip written straight to a file has
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, array) in arrays {
        zip.start_file(format!("{name}.npy"), options).map_err(|error| error.to_string())?;
        zip.write_all(&array.npy()).map_err(|error| error.to_string())?;
    }
    Ok(zip.finish().map_err(|error| error.to_string())?.into_inner())
}

/// One array of a .npz file's bytes.
pub fn array(bytes: &[u8], name: &str) -> Result<Array, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|error| error.to_string())?;
    let mut entry = zip.by_name(&format!("{name}.npy")).map_err(|error| format!("{name}: {error}"))?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    Array::from_npy(&bytes)
}

/// Writing and reading .npz files.
#[cfg(test)]
mod tests {
    use super::*;

    /// Byte, float and single-value arrays written to a .npz read back the same, each .npy's data at a multiple of
    /// HEADER_ALIGN.
    #[test]
    fn arrays_read_back() {
        let img = Array::u8(&[2, 3], vec![1, 2, 3, 4, 5, 6]);
        let (boxes, none) = (Array::f32(&[1, 4], &[1.5, 2.0, 3.25, 4.0]), Array::u8(&[], vec![0]));
        let file = to_bytes(&[("rgb", &img), ("boxes", &boxes), ("hidden", &none)]).unwrap();
        let rgb = array(&file, "rgb").unwrap();
        assert_eq!((rgb.dtype, rgb.shape.clone(), rgb.data.clone()), (Dtype::U8, vec![2, 3], vec![1, 2, 3, 4, 5, 6]));
        assert_eq!(array(&file, "boxes").unwrap().floats(), vec![1.5, 2.0, 3.25, 4.0]);
        assert_eq!(array(&file, "hidden").unwrap().shape, Vec::<usize>::new());
        assert_eq!((img.npy().len() - 6) % HEADER_ALIGN, 0);
    }
}
