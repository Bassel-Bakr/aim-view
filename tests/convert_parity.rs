//! `convert::Converter` against ffmpeg 8.1 itself: frames of real recordings (AV1, H.264 and HEVC; 2560, 1920 and
//! 1280 wide; full and limited range) decoded as they are (`<key>_<n>_src.yuv`), and ffmpeg's
//! `scale=1280:720:flags=area` to rgb24 and yuv420p, with its x86 kernels (`.raw`) and its plain C code (`_c.raw`,
//! `-cpuflags 0`). Every byte must be equal, with the 2:1 shortcut and through the full pipeline.
//! Data in test_out/parity/convert/ (meta.json: each source's size and range).

use std::fs;
use std::path::{Path, PathBuf};

use aimview::convert::{Converter, DST_H, DST_W, Matrix};
use serde_json::Value;

/// The source frames kept of each recording, by frame number.
const FRAME_NUMBERS: [u32; 2] = [0, 200];
/// ffmpeg's output file suffix and whether it used its x86 kernels.
const KERNELS: [(&str, bool); 2] = [("", true), ("_c", false)];
const OUTPUT_FORMATS: [&str; 2] = ["rgb24", "yuv420p"];
const RGB_BYTES: usize = DST_W * DST_H * 3;
/// The Y plane, then U and V at a quarter of its size each.
const YUV_BYTES: usize = DST_W * DST_H * 3 / 2;

/// One source frame: its recording's size and range, and where ffmpeg's outputs for it are.
struct Source<'a> {
    dir: &'a Path,
    /// `<key>_<n>`: the recording and the frame number.
    name: String,
    frame: Vec<u8>,
    width: usize,
    height: usize,
    full_range: bool,
}

/// The conversions checked, and each one that differs.
#[derive(Default)]
struct Outcome {
    checked: usize,
    wrong: Vec<String>,
}

/// Converts the source to every format with one converter (ffmpeg's x86 kernels or its C code, the 2:1 shortcut or the
/// full pipeline) and compares each with ffmpeg's output.
fn check_conversions(source: &Source, (suffix, x86): (&str, bool), shortcut: bool, outcome: &mut Outcome) {
    let (name, frame, width, height) = (&source.name, &source.frame, source.width, source.height);
    let mut converter = if shortcut {
        Converter::with_kernels(width, height, Matrix::Bt709, source.full_range, x86)
    } else {
        Converter::without_shortcut(width, height, Matrix::Bt709, source.full_range, x86)
    };
    for format in OUTPUT_FORMATS {
        let Ok(want) = fs::read(source.dir.join(format!("{name}_{format}{suffix}.raw"))) else { continue };
        let mut got = vec![0u8; if format == "rgb24" { RGB_BYTES } else { YUV_BYTES }];
        if format == "rgb24" {
            converter.rgb24(frame, &mut got);
        } else {
            converter.yuv420p(frame, &mut got);
            // the luma alone, from the Y plane alone, is the same bytes
            let mut luma = vec![0u8; DST_W * DST_H];
            converter.luma(&frame[..width * height], &mut luma);
            if luma[..] != got[..DST_W * DST_H] {
                outcome.wrong.push(format!("{name} luma{suffix} shortcut={shortcut}: not yuv420p's Y"));
            }
        }
        let bad = got.iter().zip(&want).filter(|(a, b)| a != b).count();
        outcome.checked += 1;
        if bad > 0 || got.len() != want.len() {
            outcome.wrong.push(format!("{name}_{format}{suffix} shortcut={shortcut}: {bad} bytes"));
        }
    }
}

#[test]
fn convert_matches_ffmpeg() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity/convert");
    let Ok(meta) = fs::read_to_string(dir.join("meta.json")) else {
        eprintln!("no {}: frames from ffmpeg are needed", dir.display());
        return;
    };
    let meta: Value = serde_json::from_str(&meta).unwrap();
    let mut outcome = Outcome::default();
    for (key, recording) in meta.as_object().unwrap() {
        let size = |field: &str| recording[field].as_u64().unwrap() as usize;
        let (width, height) = (size("width"), size("height"));
        let full_range = recording["color_range"] == "pc";
        for number in FRAME_NUMBERS {
            let name = format!("{key}_{number}");
            let Ok(frame) = fs::read(dir.join(format!("{name}_src.yuv"))) else { continue };
            let source = Source { dir: &dir, name, frame, width, height, full_range };
            for kernels in KERNELS {
                for shortcut in [true, false] {
                    check_conversions(&source, kernels, shortcut, &mut outcome);
                }
            }
        }
    }
    let Outcome { checked, wrong } = outcome;
    for difference in &wrong {
        eprintln!("{difference}");
    }
    assert!(checked > 0, "no frames checked");
    assert!(wrong.is_empty(), "{} of {checked} conversions differ", wrong.len());
    eprintln!("{checked} conversions equal to ffmpeg's, byte for byte");
}
