//! `convert::Converter` against ffmpeg 8.1 itself: frames of real recordings (AV1, H.264 and HEVC; 2560, 1920 and
//! 1280 wide; full and limited range) decoded as they are (`<key>_<n>_src.yuv`), and ffmpeg's
//! `scale=1280:720:flags=area` to rgb24 and yuv420p, with its x86 kernels (`.raw`) and its plain C code (`_c.raw`,
//! `-cpuflags 0`). Every byte must be equal, with the 2:1 shortcut and through the full pipeline.
//! Data in test_out/parity/convert/ (meta.json: each source's size and range).

use std::fs;
use std::path::PathBuf;

use aimview::convert::{Converter, DST_H, DST_W, Matrix};
use serde_json::Value;

#[test]
fn convert_matches_ffmpeg() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_out/parity/convert");
    let Ok(meta) = fs::read_to_string(dir.join("meta.json")) else {
        eprintln!("no {}: frames from ffmpeg are needed", dir.display());
        return;
    };
    let meta: Value = serde_json::from_str(&meta).unwrap();
    let mut checked = 0;
    let mut wrong = Vec::new();
    for (key, m) in meta.as_object().unwrap() {
        let (w, h) = (m["width"].as_u64().unwrap() as usize, m["height"].as_u64().unwrap() as usize);
        let full = m["color_range"] == "pc";
        for n in [0, 200] {
            let Ok(src) = fs::read(dir.join(format!("{key}_{n}_src.yuv"))) else { continue };
            for (suffix, x86) in [("", true), ("_c", false)] {
                for shortcut in [true, false] {
                    let mut conv = if shortcut {
                        Converter::with_kernels(w, h, Matrix::Bt709, full, x86)
                    } else {
                        Converter::without_shortcut(w, h, Matrix::Bt709, full, x86)
                    };
                    for format in ["rgb24", "yuv420p"] {
                        let Ok(want) = fs::read(dir.join(format!("{key}_{n}_{format}{suffix}.raw"))) else { continue };
                        let mut got = vec![0u8; if format == "rgb24" { DST_W * DST_H * 3 } else { DST_W * DST_H * 3 / 2 }];
                        if format == "rgb24" {
                            conv.rgb24(&src, &mut got);
                        } else {
                            conv.yuv420p(&src, &mut got);
                        }
                        let bad = got.iter().zip(&want).filter(|(a, b)| a != b).count();
                        checked += 1;
                        if bad > 0 || got.len() != want.len() {
                            wrong.push(format!("{key}_{n}_{format}{suffix} shortcut={shortcut}: {bad} bytes"));
                        }
                    }
                }
            }
        }
    }
    for w in &wrong {
        eprintln!("{w}");
    }
    assert!(checked > 0, "no frames checked");
    assert!(wrong.is_empty(), "{} of {checked} conversions differ", wrong.len());
    eprintln!("{checked} conversions equal to ffmpeg's, byte for byte");
}
