//! A video's frames from the GPU (gpu_frames.rs) against ffmpeg's, converted by the core (video.rs, convert.rs), byte
//! for byte: each frame's RGB, 720p luma and Y plane (all its rows), from the start or from a time on. Reports the frames that differ and, for the
//! first, which of ffmpeg's frames near it the GPU's equals (a frame lost or doubled shows as a shift).
//! cargo run -p aimview-service --release --example frames_check -- <video> [frames] [from (seconds)]

use std::path::PathBuf;

use aimview::convert::{Converter, DST_H, DST_W};
use aimview_service::gpu_frames::GpuFrames;
use aimview_service::video::{Frames, probe};

const DEFAULT_FRAMES: usize = 300;
/// How far either way a differing frame is looked for among ffmpeg's.
const SHIFT_REACH: usize = 3;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let video = PathBuf::from(args.get(1).ok_or("give a video")?);
    let count: usize = args.get(2).and_then(|arg| arg.parse().ok()).unwrap_or(DEFAULT_FRAMES);
    let from: Option<f64> = args.get(3).and_then(|arg| arg.parse().ok());
    let info = probe(&video)?;
    let (luma_bytes, yuv_bytes) = (info.width * info.height, aimview_service::review::frame_bytes(&info));
    let mut convert = Converter::new(info.width, info.height, info.matrix, info.full);
    let mut cpu = Frames::open(&video, from, Some(count))?;
    let mut gpu = GpuFrames::open(&video, &info, from, Some(count), info.height)?;
    let (mut cpu_frames, mut gpu_frames) = (Vec::new(), Vec::new());
    let mut yuv = vec![0u8; yuv_bytes];
    loop {
        let mut rgb = vec![0u8; DST_W * DST_H * 3];
        if !cpu.next_into(&mut yuv)? {
            break;
        }
        convert.rgb24(&yuv, &mut rgb);
        let mut small = vec![0u8; DST_W * DST_H];
        convert.luma(&yuv[..luma_bytes], &mut small);
        cpu_frames.push((rgb, small, yuv[..luma_bytes].to_vec()));
    }
    loop {
        let (mut rgb, mut small, mut plane) =
            (vec![0u8; DST_W * DST_H * 3], vec![0u8; DST_W * DST_H], vec![0u8; luma_bytes]);
        if !gpu.next_into(&mut rgb, &mut small, &mut plane)? {
            break;
        }
        gpu_frames.push((rgb, small, plane));
    }
    println!("ffmpeg gave {} frames, the GPU {}", cpu_frames.len(), gpu_frames.len());
    let differing: Vec<usize> = (0..cpu_frames.len().min(gpu_frames.len()))
        .filter(|&i| cpu_frames[i] != gpu_frames[i])
        .collect();
    println!("frames differing: {} (first {:?})", differing.len(), differing.iter().take(5).collect::<Vec<_>>());
    if let Some(&first) = differing.first() {
        let near = first.saturating_sub(SHIFT_REACH)..(first + SHIFT_REACH + 1).min(cpu_frames.len());
        let matches: Vec<usize> = near.filter(|&i| cpu_frames[i] == gpu_frames[first]).collect();
        let (cpu, gpu) = (&cpu_frames[first], &gpu_frames[first]);
        let (rgb_same, small_same, plane_same) = (cpu.0 == gpu.0, cpu.1 == gpu.1, cpu.2 == gpu.2);
        println!(
            "GPU frame {first} equals ffmpeg's frames {matches:?}; its RGB same {rgb_same}, 720p luma same              {small_same}, Y plane same {plane_same}"
        );
    }
    Ok(())
}
