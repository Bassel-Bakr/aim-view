//! Prototype: a recording decoded on the GPU by Windows Media Foundation into D3D11 textures (no copy to the CPU), and
//! each frame converted there to the detector's 1280 x 720 RGB by a compute shader (convert.hlsl) that does
//! src/convert.rs's 2:1 integer arithmetic. It checks the decoded frames against ffmpeg's software decode (a per-frame
//! SHA-256 list from `ffmpeg -i <video> -map 0:v -pix_fmt yuv420p -f framehash -hash sha256 <file>`), the shader's RGB
//! against convert.rs's, byte for byte, and times decoding alone and decoding with the shader, with the CPU time.
//! Only 2560 x 1440 recordings (the 2:1 path).
//! Usage: gpu-decode <video> <reference framehash file> <frames to check> <matrix code (0 BT.709, 1 BT.601)> <full 0/1>

use std::ffi::c_void;
use std::path::Path;
use std::time::Instant;

use aimview::convert::{Converter, DST_H, DST_W, Matrix};
use sha2::{Digest, Sha256};
use windows::Win32::Foundation::{FILETIME, HMODULE};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_1, D3D11_SRV_DIMENSION_BUFFER, D3D11_SRV_DIMENSION_TEXTURE2D, ID3DBlob,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIFactory1};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
use windows::core::{Interface, PCSTR, PCWSTR, Result};

const SRC_W: u32 = 2560;
const SRC_H: u32 = 1440;
const RGB_BYTES: usize = DST_W * DST_H * 3;
/// Media Foundation's version (MF_SDK_VERSION << 16 | MF_API_VERSION).
const MF_VERSION: u32 = 0x0002_0070;
const MF_START_FULL: u32 = 0;
/// The shader's threads a group and pixels a thread (convert.hlsl).
const THREADS_PER_GROUP: u32 = 64;
const PIXELS_PER_THREAD: u32 = 4;
/// ffmpeg's yuv2rgb tables, as src/convert.rs builds them (RgbTables::new).
const Y_TABLE_LEN: usize = 2048;
const CHROMA_TABLE_LEN: usize = 256;
/// The checks read up to this many frames before ffmpeg's first (the pre-roll an edit list hides).
const MAX_PRE_ROLL: usize = 200;

/// The GPU, its context, and the video's reader.
struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
}

/// One frame from the decoder: its texture (a slice of the decoder's array) and its time (100 ns units).
struct Decoded {
    texture: ID3D11Texture2D,
    slice: u32,
    time: i64,
}

/// What the shader works on: an NV12 copy of the frame it can read, the tables, the RGB out, and its views.
struct Conversion {
    nv12: ID3D11Texture2D,
    shader: ID3D11ComputeShader,
    views: [Option<ID3D11ShaderResourceView>; 3],
    rgb: ID3D11Buffer,
    rgb_view: Option<ID3D11UnorderedAccessView>,
}

/// The GPU with the most memory of its own (the discrete one where there are two).
fn largest_adapter() -> Result<IDXGIAdapter> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
    let mut best: Option<(usize, IDXGIAdapter)> = None;
    for index in 0.. {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else { break };
        let desc = unsafe { adapter.GetDesc1()? };
        if best.as_ref().is_none_or(|(memory, _)| desc.DedicatedVideoMemory > *memory) {
            let name = String::from_utf16_lossy(&desc.Description).trim_end_matches('\0').to_string();
            eprintln!("GPU {index}: {name}, {} MB of its own", desc.DedicatedVideoMemory >> 20);
            best = Some((desc.DedicatedVideoMemory, adapter.cast()?));
        }
    }
    Ok(best.expect("no GPU").1)
}

fn gpu() -> Result<Gpu> {
    let adapter = largest_adapter()?;
    let (mut device, mut context) = (None, None);
    unsafe {
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_1]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    let device: ID3D11Device = device.expect("a device");
    // Media Foundation's decoder works on the device from its own threads
    let _ = unsafe { device.cast::<ID3D11Multithread>()?.SetMultithreadProtected(true) };
    Ok(Gpu { device, context: context.expect("a context") })
}

/// A reader of the video's first video stream, decoding on the GPU into NV12 textures.
fn reader(gpu: &Gpu, video: &Path) -> Result<IMFSourceReader> {
    unsafe {
        let mut token = 0;
        let mut manager = None;
        MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
        let manager = manager.expect("a device manager");
        manager.ResetDevice(&gpu.device, token)?;
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 3)?;
        let attributes = attributes.expect("attributes");
        attributes.SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, &manager)?;
        attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
        let path: Vec<u16> = video.to_string_lossy().encode_utf16().chain([0]).collect();
        let reader = MFCreateSourceReaderFromURL(PCWSTR(path.as_ptr()), &attributes)?;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, true)?;
        let wanted = MFCreateMediaType()?;
        wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        wanted.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        reader.SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &wanted)?;
        Ok(reader)
    }
}

/// The next decoded frame, or None at the end.
fn next_frame(reader: &IMFSourceReader) -> Result<Option<Decoded>> {
    loop {
        let (mut flags, mut time, mut sample) = (0u32, 0i64, None);
        unsafe {
            reader.ReadSample(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                0,
                None,
                Some(&mut flags),
                Some(&mut time),
                Some(&mut sample),
            )?;
        }
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            return Ok(None);
        }
        let Some(sample) = sample else { continue };
        unsafe {
            let buffer: IMFDXGIBuffer = sample.GetBufferByIndex(0)?.cast()?;
            let mut raw: *mut c_void = std::ptr::null_mut();
            buffer.GetResource(&ID3D11Texture2D::IID, &mut raw)?;
            let texture = ID3D11Texture2D::from_raw(raw);
            return Ok(Some(Decoded { texture, slice: buffer.GetSubresourceIndex()?, time }));
        }
    }
}

/// src/convert.rs's RgbTables for a matrix and range, flattened as convert.hlsl reads them.
fn tables(matrix: Matrix, full_range: bool) -> Vec<i32> {
    const ONE: i64 = 1 << 16;
    const ROUND: i64 = 1 << 15;
    let [crv, cbu, cgu, cgv]: [i64; 4] = match matrix {
        Matrix::Bt709 => [117489, 138438, 13975, 34925],
        Matrix::Bt601 => [104597, 132201, 25675, 53279],
        Matrix::Fcc => [104448, 132798, 24759, 53109],
        Matrix::Smpte240m => [117579, 136230, 16907, 35559],
        Matrix::Bt2020 => [110013, 140363, 12277, 42626],
    };
    let (cgu, cgv) = (-cgu, -cgv);
    let (crv, cbu, cgu, cgv, cy, oy) = if full_range {
        let to_full = |weight: i64| weight * 224 / 255;
        (to_full(crv), to_full(cbu), to_full(cgu), to_full(cgv), ONE, 0)
    } else {
        (crv, cbu, cgu, cgv, ONE * 255 / 219, 16 << 16)
    };
    let per_luma = |weight: i64| (weight * ONE + ROUND) / cy.max(1);
    let (crv, cbu, cgu, cgv) = (per_luma(crv), per_luma(cbu), per_luma(cgu), per_luma(cgv));
    let y_offset = (if full_range { 384 } else { 326 }) + 512;
    let y_base = -(384 << 16) - 512 * cy - oy + ROUND;
    let mut out: Vec<i32> = (0..Y_TABLE_LEN as i64).map(|i| ((y_base + i * cy) >> 16).clamp(0, 255) as i32).collect();
    for (weight, offset) in [(crv, y_offset), (cgu, y_offset), (cbu, y_offset), (cgv, 0)] {
        out.extend((0..CHROMA_TABLE_LEN as i64).map(|chroma| (offset - (weight >> 9) + ((chroma * weight) >> 16)) as i32));
    }
    out
}

fn compile_shader(device: &ID3D11Device) -> Result<ID3D11ComputeShader> {
    let source = include_str!("convert.hlsl");
    let (mut code, mut errors): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
    let compiled = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR(c"convert.hlsl".as_ptr().cast()),
            None,
            None,
            PCSTR(c"main".as_ptr().cast()),
            PCSTR(c"cs_5_0".as_ptr().cast()),
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Some(errors) = &errors {
        let text = unsafe { std::slice::from_raw_parts(errors.GetBufferPointer() as *const u8, errors.GetBufferSize()) };
        eprintln!("{}", String::from_utf8_lossy(text));
    }
    compiled?;
    let code = code.expect("compiled code");
    let bytes = unsafe { std::slice::from_raw_parts(code.GetBufferPointer() as *const u8, code.GetBufferSize()) };
    let mut shader = None;
    unsafe { device.CreateComputeShader(bytes, None, Some(&mut shader))? };
    Ok(shader.expect("a compute shader"))
}

fn texture_view(device: &ID3D11Device, texture: &ID3D11Texture2D, format: DXGI_FORMAT) -> Result<ID3D11ShaderResourceView> {
    let desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: format,
        ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_SRV { MostDetailedMip: 0, MipLevels: 1 } },
    };
    let mut view = None;
    unsafe { device.CreateShaderResourceView(texture, Some(&desc), Some(&mut view))? };
    Ok(view.expect("a view"))
}

fn conversion(gpu: &Gpu, matrix: Matrix, full_range: bool) -> Result<Conversion> {
    let device = &gpu.device;
    let nv12_desc = D3D11_TEXTURE2D_DESC {
        Width: SRC_W,
        Height: SRC_H,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut nv12 = None;
    unsafe { device.CreateTexture2D(&nv12_desc, None, Some(&mut nv12))? };
    let nv12 = nv12.expect("an NV12 texture");
    let table_values = tables(matrix, full_range);
    let table_desc = D3D11_BUFFER_DESC {
        ByteWidth: (table_values.len() * 4) as u32,
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
        StructureByteStride: 4,
    };
    let initial = D3D11_SUBRESOURCE_DATA { pSysMem: table_values.as_ptr().cast(), SysMemPitch: 0, SysMemSlicePitch: 0 };
    let mut table_buffer = None;
    unsafe { device.CreateBuffer(&table_desc, Some(&initial), Some(&mut table_buffer))? };
    let table_view_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_UNKNOWN,
        ViewDimension: D3D11_SRV_DIMENSION_BUFFER,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D11_BUFFER_SRV {
                Anonymous1: D3D11_BUFFER_SRV_0 { FirstElement: 0 },
                Anonymous2: D3D11_BUFFER_SRV_1 { NumElements: table_values.len() as u32 },
            },
        },
    };
    let mut table_view = None;
    unsafe { device.CreateShaderResourceView(table_buffer.as_ref().expect("tables"), Some(&table_view_desc), Some(&mut table_view))? };
    let rgb_desc = D3D11_BUFFER_DESC {
        ByteWidth: RGB_BYTES as u32,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS.0 as u32,
        StructureByteStride: 0,
    };
    let mut rgb = None;
    unsafe { device.CreateBuffer(&rgb_desc, None, Some(&mut rgb))? };
    let rgb = rgb.expect("an RGB buffer");
    let rgb_view_desc = D3D11_UNORDERED_ACCESS_VIEW_DESC {
        Format: DXGI_FORMAT_R32_TYPELESS,
        ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
        Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
            Buffer: D3D11_BUFFER_UAV {
                FirstElement: 0,
                NumElements: (RGB_BYTES / 4) as u32,
                Flags: D3D11_BUFFER_UAV_FLAG_RAW.0 as u32,
            },
        },
    };
    let mut rgb_view = None;
    unsafe { device.CreateUnorderedAccessView(&rgb, Some(&rgb_view_desc), Some(&mut rgb_view))? };
    let views = [
        Some(texture_view(device, &nv12, DXGI_FORMAT_R8_UINT)?),
        Some(texture_view(device, &nv12, DXGI_FORMAT_R8G8_UINT)?),
        table_view,
    ];
    Ok(Conversion { nv12, shader: compile_shader(device)?, views, rgb, rgb_view })
}

/// The decoded frame copied (on the GPU) into the texture the shader reads, and converted.
fn convert_on_gpu(gpu: &Gpu, conversion: &Conversion, frame: &Decoded) {
    let context = &gpu.context;
    let whole = D3D11_BOX { left: 0, top: 0, front: 0, right: SRC_W, bottom: SRC_H, back: 1 };
    unsafe {
        context.CopySubresourceRegion(&conversion.nv12, 0, 0, 0, 0, &frame.texture, frame.slice, Some(&whole));
        context.CSSetShader(&conversion.shader, None);
        context.CSSetShaderResources(0, Some(&conversion.views));
        context.CSSetUnorderedAccessViews(0, 1, Some(&conversion.rgb_view), None);
        let threads = (DST_W as u32 / PIXELS_PER_THREAD) * DST_H as u32;
        context.Dispatch(threads.div_ceil(THREADS_PER_GROUP), 1, 1);
    }
}

/// Waits until the GPU has done all it was given.
fn finish(gpu: &Gpu) -> Result<()> {
    let desc = D3D11_QUERY_DESC { Query: D3D11_QUERY_EVENT, MiscFlags: 0 };
    let mut query = None;
    unsafe {
        gpu.device.CreateQuery(&desc, Some(&mut query))?;
        let query = query.expect("a query");
        gpu.context.End(&query);
        let mut done = 0u32;
        while gpu.context.GetData(&query, Some((&mut done as *mut u32).cast()), 4, 0).is_err() || done == 0 {
            std::thread::yield_now();
        }
    }
    Ok(())
}

fn staging_texture(gpu: &Gpu) -> Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: SRC_W,
        Height: SRC_H,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe { gpu.device.CreateTexture2D(&desc, None, Some(&mut texture))? };
    Ok(texture.expect("a staging texture"))
}

fn staging_buffer(gpu: &Gpu) -> Result<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: RGB_BYTES as u32,
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut buffer = None;
    unsafe { gpu.device.CreateBuffer(&desc, None, Some(&mut buffer))? };
    Ok(buffer.expect("a staging buffer"))
}

/// The shader's NV12 texture as yuv420p bytes (Y, then U, then V), read back for the checks.
fn read_yuv420p(gpu: &Gpu, conversion: &Conversion, staging: &ID3D11Texture2D) -> Result<Vec<u8>> {
    let (width, height) = (SRC_W as usize, SRC_H as usize);
    let mut out = vec![0u8; width * height * 3 / 2];
    unsafe {
        gpu.context.CopyResource(staging, &conversion.nv12);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        gpu.context.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        let pitch = mapped.RowPitch as usize;
        let data = std::slice::from_raw_parts(mapped.pData as *const u8, pitch * height * 3 / 2);
        for row in 0..height {
            out[row * width..(row + 1) * width].copy_from_slice(&data[row * pitch..row * pitch + width]);
        }
        let (chroma_width, chroma_height, luma_bytes) = (width / 2, height / 2, width * height);
        for row in 0..chroma_height {
            let line = &data[(height + row) * pitch..(height + row) * pitch + width];
            for x in 0..chroma_width {
                out[luma_bytes + row * chroma_width + x] = line[2 * x];
                out[luma_bytes + chroma_width * chroma_height + row * chroma_width + x] = line[2 * x + 1];
            }
        }
        gpu.context.Unmap(staging, 0);
    }
    Ok(out)
}

fn read_rgb(gpu: &Gpu, conversion: &Conversion, staging: &ID3D11Buffer) -> Result<Vec<u8>> {
    unsafe {
        gpu.context.CopyResource(staging, &conversion.rgb);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        gpu.context.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        let out = std::slice::from_raw_parts(mapped.pData as *const u8, RGB_BYTES).to_vec();
        gpu.context.Unmap(staging, 0);
        Ok(out)
    }
}

/// The process's CPU time so far, in seconds (user and kernel).
fn cpu_seconds() -> f64 {
    let (mut created, mut exited, mut kernel, mut user) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe {
        let _ = GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user);
    }
    let seconds = |time: FILETIME| (((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64) as f64 / 1e7;
    seconds(kernel) + seconds(user)
}

/// ffmpeg's framehash lines' hashes, in order (the video stream's, stream 0).
fn reference_hashes(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .expect("the reference framehash file")
        .lines()
        .filter(|line| !line.starts_with('#') && line.starts_with('0'))
        .filter_map(|line| line.rsplit(',').next().map(|hash| hash.trim().to_string()))
        .collect()
}

/// Decodes the whole video and reports the time and CPU time, with or without the shader per frame.
fn timed_pass(gpu: &Gpu, video: &Path, conversion: Option<&Conversion>) -> Result<()> {
    let reader = reader(gpu, video)?;
    let (started, cpu_started) = (Instant::now(), cpu_seconds());
    let (mut frames, mut first_time) = (0usize, None);
    while let Some(frame) = next_frame(&reader)? {
        first_time.get_or_insert(frame.time);
        if let Some(conversion) = conversion {
            convert_on_gpu(gpu, conversion, &frame);
        }
        frames += 1;
    }
    finish(gpu)?;
    let what = if conversion.is_some() { "decoded and converted on the GPU" } else { "decoded on the GPU" };
    println!(
        "{what}: {frames} frames (first at {:.4} s) in {:.2} s, CPU {:.2} s",
        first_time.unwrap_or(0) as f64 / 1e7,
        started.elapsed().as_secs_f64(),
        cpu_seconds() - cpu_started
    );
    Ok(())
}

/// The first `count` frames: each decoded frame against ffmpeg's, and the shader's RGB against convert.rs's.
fn checked_pass(gpu: &Gpu, video: &Path, conversion: &Conversion, reference: &[String], count: usize, matrix: Matrix, full_range: bool) -> Result<()> {
    let reader = reader(gpu, video)?;
    let (staging, rgb_staging) = (staging_texture(gpu)?, staging_buffer(gpu)?);
    let mut converter = Converter::new(SRC_W as usize, SRC_H as usize, matrix, full_range);
    let mut cpu_rgb = vec![0u8; RGB_BYTES];
    let (mut hashes, mut times, mut same_rgb) = (Vec::new(), Vec::new(), 0);
    // the pre-roll an edit list hides comes first here: read that many more frames, then line ffmpeg's up with them
    for _ in 0..count + MAX_PRE_ROLL {
        let Some(frame) = next_frame(&reader)? else { break };
        convert_on_gpu(gpu, conversion, &frame);
        let yuv = read_yuv420p(gpu, conversion, &staging)?;
        hashes.push(format!("{:x}", Sha256::digest(&yuv)));
        times.push(frame.time);
        converter.rgb24(&yuv, &mut cpu_rgb);
        if read_rgb(gpu, conversion, &rgb_staging)? == cpu_rgb {
            same_rgb += 1;
        }
    }
    println!("shader RGB equal to convert.rs's on {same_rgb} of {} frames", hashes.len());
    let wanted = &reference[..count.min(reference.len())];
    match (0..=hashes.len().saturating_sub(wanted.len())).find(|&skip| hashes[skip] == wanted[0]) {
        Some(skip) => {
            let same = wanted.iter().zip(&hashes[skip..]).filter(|(want, got)| want == got).count();
            println!(
                "ffmpeg's first frame is decoded frame {skip} (at {:.4} s); from there {same} of {} frames equal",
                times[skip] as f64 / 1e7,
                wanted.len()
            );
        }
        None => println!("ffmpeg's first frame is not among the first {} decoded frames", hashes.len()),
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let video = Path::new(&args[1]);
    let reference = reference_hashes(Path::new(&args[2]));
    let count: usize = args[3].parse().expect("a frame count");
    let matrix = Matrix::from_code(args[4].parse().expect("a matrix code"));
    let full_range = args[5] == "1";
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        MFStartup(MF_VERSION, MF_START_FULL)?;
    }
    let gpu = gpu()?;
    let conversion = conversion(&gpu, matrix, full_range)?;
    checked_pass(&gpu, video, &conversion, &reference, count, matrix, full_range)?;
    timed_pass(&gpu, video, None)?;
    timed_pass(&gpu, video, Some(&conversion))?;
    Ok(())
}
