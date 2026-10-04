//! A run's frames decoded on the GPU (Windows): Media Foundation decodes the recording into D3D11 textures, and a
//! compute shader (gpu_frames.hlsl) makes the detector's 1280 x 720 RGB with src/convert.rs's 2:1 integer arithmetic
//! and lays out the full-size Y plane the camera and the HUD read. Both are read back a few frames behind, through a
//! ring of staging buffers, so the GPU never waits for the CPU. The CPU does no decoding and no conversion: ffmpeg's
//! software decode took about 7 ms of CPU a 1440p frame. prototypes/gpu_decode checked the decoded frames against
//! ffmpeg's and the RGB against convert.rs's, byte for byte. Media Foundation counts its times from the file's
//! earliest frame, the pre-roll an MP4 edit list hides included (OBS's AV1 files have about 100 such frames; its H.264
//! files none), where ffmpeg's and the browser's start at the first frame shown: a frame's time here is its time there
//! less `VideoInfo::earliest`. Only 2560 x 1440 MP4s (`usable`); other videos keep ffmpeg (video.rs). In: the video,
//! its `VideoInfo`, where a run starts. Out: each frame's RGB and Y plane.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::path::Path;
use std::sync::Once;

use aimview::convert::{DST_H, DST_W, Matrix};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_1, D3D11_SRV_DIMENSION_BUFFER, D3D11_SRV_DIMENSION_TEXTURE2D, ID3DBlob,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIFactory1};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::core::{GUID, Interface, PCSTR, PCWSTR};

use crate::video::VideoInfo;

/// The size this path converts (exactly 2:1 to the detector's frame).
const SRC_W: u32 = 2 * DST_W as u32;
const SRC_H: u32 = 2 * DST_H as u32;
const RGB_BYTES: usize = DST_W * DST_H * 3;
const LUMA_BYTES: usize = (SRC_W * SRC_H) as usize;
/// Frames read back this many behind the newest the GPU was given.
const RING: usize = 4;
/// Media Foundation's version (MF_SDK_VERSION << 16 | MF_API_VERSION) and its full start.
const MF_VERSION: u32 = 0x0002_0070;
const MF_START_FULL: u32 = 0;
/// The shader's threads a group and pixels a thread (gpu_frames.hlsl).
const THREADS_PER_GROUP: u32 = 64;
const PIXELS_PER_THREAD: u32 = 4;
/// ffmpeg's yuv2rgb tables, as src/convert.rs builds them (RgbTables::new).
const Y_TABLE_LEN: i64 = 2048;
const CHROMA_TABLE_LEN: i64 = 256;
/// Media Foundation's times are in units of 100 ns.
const UNITS_PER_SECOND: f64 = 1e7;

static MF_STARTED: Once = Once::new();

fn failed(what: &str) -> impl Fn(windows::core::Error) -> String + '_ {
    move |error| format!("{what}: {error}")
}

/// The codecs whose frames were checked against ffmpeg's, byte for byte (frames_check, and whole reviews).
const CHECKED_CODECS: [&str; 2] = ["av1", "h264"];

/// Whether a run of this video can take its frames from the GPU: an MP4 of exactly 2560 x 1440 (the shader's 2:1),
/// in a codec checked against ffmpeg's frames.
pub fn usable(video: &Path, info: &VideoInfo) -> bool {
    let mp4 = video.extension().is_some_and(|extension| {
        let extension = extension.to_string_lossy().to_lowercase();
        extension == "mp4" || extension == "mov"
    });
    mp4 && info.width == SRC_W as usize && info.height == SRC_H as usize && CHECKED_CODECS.contains(&info.codec.as_str())
}

/// The GPU with the most memory of its own (the discrete one where there are two), and its device.
fn device() -> Result<(ID3D11Device, ID3D11DeviceContext), String> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.map_err(failed("DXGI"))?;
    let mut best: Option<(usize, IDXGIAdapter)> = None;
    for index in 0.. {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else { break };
        let memory = unsafe { adapter.GetDesc1() }.map_err(failed("the GPU's description"))?.DedicatedVideoMemory;
        if best.as_ref().is_none_or(|(most, _)| memory > *most) {
            best = Some((memory, adapter.cast().map_err(failed("the GPU"))?));
        }
    }
    let adapter = best.ok_or("no GPU")?.1;
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
        )
    }
    .map_err(failed("Direct3D 11"))?;
    let device: ID3D11Device = device.ok_or("no Direct3D device")?;
    // Media Foundation's decoder works on the device from its own threads
    let _ = unsafe { device.cast::<ID3D11Multithread>().map_err(failed("Direct3D 11"))?.SetMultithreadProtected(true) };
    Ok((device, context.ok_or("no Direct3D context")?))
}

/// A reader of the video's first video stream, decoding on the GPU into NV12 textures.
fn reader(device: &ID3D11Device, video: &Path) -> windows::core::Result<IMFSourceReader> {
    unsafe {
        let mut token = 0;
        let mut manager = None;
        MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
        let manager = manager.ok_or_else(windows::core::Error::empty)?;
        manager.ResetDevice(device, token)?;
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 2)?;
        let attributes = attributes.ok_or_else(windows::core::Error::empty)?;
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

/// src/convert.rs's RgbTables for a matrix and range, flattened as gpu_frames.hlsl reads them (convert.rs keeps its
/// tables private; the shader's output is checked against convert.rs's, so the two cannot drift apart unseen).
fn tables(matrix: Matrix, full_range: bool) -> Vec<i32> {
    const ONE: i64 = 1 << 16;
    const ROUND: i64 = 1 << 15;
    let [crv, cbu, cgu, cgv]: [i64; 4] = match matrix {
        Matrix::Bt709 => [117_489, 138_438, 13_975, 34_925],
        Matrix::Bt601 => [104_597, 132_201, 25_675, 53_279],
        Matrix::Fcc => [104_448, 132_798, 24_759, 53_109],
        Matrix::Smpte240m => [117_579, 136_230, 16_907, 35_559],
        Matrix::Bt2020 => [110_013, 140_363, 12_277, 42_626],
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
    let mut out: Vec<i32> = (0..Y_TABLE_LEN).map(|i| ((y_base + i * cy) >> 16).clamp(0, 255) as i32).collect();
    for (weight, offset) in [(crv, y_offset), (cgu, y_offset), (cbu, y_offset), (cgv, 0)] {
        out.extend((0..CHROMA_TABLE_LEN).map(|chroma| (offset - (weight >> 9) + ((chroma * weight) >> 16)) as i32));
    }
    out
}

fn compile_shader(device: &ID3D11Device) -> Result<ID3D11ComputeShader, String> {
    let source = include_str!("gpu_frames.hlsl");
    let (mut code, mut errors): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
    let compiled = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR(c"gpu_frames.hlsl".as_ptr().cast()),
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
    let blob_bytes = |blob: &ID3DBlob| unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()).to_vec()
    };
    if let Err(error) = compiled {
        let message = errors.as_ref().map(|errors| String::from_utf8_lossy(&blob_bytes(errors)).into_owned());
        return Err(format!("the frame shader: {error} {}", message.unwrap_or_default()));
    }
    let bytes = blob_bytes(&code.ok_or("the frame shader gave no code")?);
    let mut shader = None;
    unsafe { device.CreateComputeShader(&bytes, None, Some(&mut shader)) }.map_err(failed("the frame shader"))?;
    shader.ok_or_else(|| "no frame shader".to_string())
}

fn buffer(device: &ID3D11Device, desc: &D3D11_BUFFER_DESC, initial: Option<&[i32]>) -> Result<ID3D11Buffer, String> {
    let data = initial.map(|values| D3D11_SUBRESOURCE_DATA {
        pSysMem: values.as_ptr().cast(),
        SysMemPitch: 0,
        SysMemSlicePitch: 0,
    });
    let mut out = None;
    unsafe { device.CreateBuffer(desc, data.as_ref().map(|data| data as *const _), Some(&mut out)) }
        .map_err(failed("a GPU buffer"))?;
    out.ok_or_else(|| "no GPU buffer".to_string())
}

/// A buffer the shader writes bytes into (a raw view), `bytes` long.
fn output_buffer(device: &ID3D11Device, bytes: usize) -> Result<(ID3D11Buffer, ID3D11UnorderedAccessView), String> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: bytes as u32,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS.0 as u32,
        StructureByteStride: 0,
    };
    let out = buffer(device, &desc, None)?;
    let view_desc = D3D11_UNORDERED_ACCESS_VIEW_DESC {
        Format: DXGI_FORMAT_R32_TYPELESS,
        ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
        Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
            Buffer: D3D11_BUFFER_UAV {
                FirstElement: 0,
                NumElements: (bytes / 4) as u32,
                Flags: D3D11_BUFFER_UAV_FLAG_RAW.0 as u32,
            },
        },
    };
    let mut view = None;
    unsafe { device.CreateUnorderedAccessView(&out, Some(&view_desc), Some(&mut view)) }
        .map_err(failed("a GPU buffer's view"))?;
    Ok((out, view.ok_or("no GPU buffer view")?))
}

fn staging_buffer(device: &ID3D11Device, bytes: usize) -> Result<ID3D11Buffer, String> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: bytes as u32,
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    buffer(device, &desc, None)
}

fn texture_view(
    device: &ID3D11Device,
    texture: &ID3D11Texture2D,
    format: DXGI_FORMAT,
) -> Result<ID3D11ShaderResourceView, String> {
    let desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: format,
        ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_SRV { MostDetailedMip: 0, MipLevels: 1 } },
    };
    let mut view = None;
    unsafe { device.CreateShaderResourceView(texture, Some(&desc), Some(&mut view)) }
        .map_err(failed("a frame's view"))?;
    view.ok_or_else(|| "no frame view".to_string())
}

/// The tables in a buffer the shader reads, and its view.
fn table_view(device: &ID3D11Device, matrix: Matrix, full_range: bool) -> Result<ID3D11ShaderResourceView, String> {
    let values = tables(matrix, full_range);
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: (values.len() * 4) as u32,
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
        StructureByteStride: 4,
    };
    let table_buffer = buffer(device, &desc, Some(&values))?;
    let view_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_UNKNOWN,
        ViewDimension: D3D11_SRV_DIMENSION_BUFFER,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D11_BUFFER_SRV {
                Anonymous1: D3D11_BUFFER_SRV_0 { FirstElement: 0 },
                Anonymous2: D3D11_BUFFER_SRV_1 { NumElements: values.len() as u32 },
            },
        },
    };
    let mut view = None;
    unsafe { device.CreateShaderResourceView(&table_buffer, Some(&view_desc), Some(&mut view)) }
        .map_err(failed("the tables' view"))?;
    view.ok_or_else(|| "no tables view".to_string())
}

/// One slot of the read-back ring: where a frame's RGB and Y plane wait for the CPU, and its decoded sample, held so
/// the decoder cannot reuse its surface before the GPU has copied it.
struct Readback {
    rgb: ID3D11Buffer,
    luma: ID3D11Buffer,
    sample: Option<IMFSample>,
}

/// A run's frames from the GPU, in order: `next_into` gives each one's RGB and Y plane.
pub struct GpuFrames {
    context: ID3D11DeviceContext,
    reader: IMFSourceReader,
    nv12: ID3D11Texture2D,
    shader: ID3D11ComputeShader,
    views: [Option<ID3D11ShaderResourceView>; 3],
    outputs: [Option<ID3D11UnorderedAccessView>; 2],
    rgb: ID3D11Buffer,
    luma: ID3D11Buffer,
    ring: Vec<Readback>,
    /// The ring's slots holding frames not read back yet, oldest first.
    waiting: VecDeque<usize>,
    next_slot: usize,
    /// The first frame's time on Media Foundation's clock (100 ns, less half a frame), and the frames still to give.
    start: i64,
    left: Option<usize>,
    ended: bool,
    // kept alive for the reader
    _device: ID3D11Device,
}

impl GpuFrames {
    /// Every frame, or from a key frame's time on (`from`, seconds from the first shown frame), at most `count` of
    /// them, as video.rs's `Frames::open` gives them.
    pub fn open(video: &Path, info: &VideoInfo, from: Option<f64>, count: Option<usize>) -> Result<GpuFrames, String> {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let mut started = Ok(());
        MF_STARTED.call_once(|| started = unsafe { MFStartup(MF_VERSION, MF_START_FULL) });
        started.map_err(failed("Media Foundation"))?;
        let (device, context) = device()?;
        let reader = reader(&device, video).map_err(failed("Media Foundation's reader"))?;
        // the run's first frame on Media Foundation's clock
        let shown_from = from.unwrap_or_else(|| info.times.first().copied().unwrap_or(0.0)) - info.earliest;
        if from.is_some() {
            let position = PROPVARIANT::from((shown_from * UNITS_PER_SECOND).round() as i64);
            unsafe { reader.SetCurrentPosition(&GUID::zeroed(), &position) }.map_err(failed("seeking"))?;
        }
        let half_frame = UNITS_PER_SECOND / info.fps.max(1.0) / 2.0;
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
        unsafe { device.CreateTexture2D(&nv12_desc, None, Some(&mut nv12)) }.map_err(failed("a frame texture"))?;
        let nv12 = nv12.ok_or("no frame texture")?;
        let views = [
            Some(texture_view(&device, &nv12, DXGI_FORMAT_R8_UINT)?),
            Some(texture_view(&device, &nv12, DXGI_FORMAT_R8G8_UINT)?),
            Some(table_view(&device, info.matrix, info.full)?),
        ];
        let (rgb, rgb_view) = output_buffer(&device, RGB_BYTES)?;
        let (luma, luma_view) = output_buffer(&device, LUMA_BYTES)?;
        let ring = (0..RING)
            .map(|_| {
                let (rgb, luma) = (staging_buffer(&device, RGB_BYTES)?, staging_buffer(&device, LUMA_BYTES)?);
                Ok(Readback { rgb, luma, sample: None })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(GpuFrames {
            shader: compile_shader(&device)?,
            context,
            reader,
            nv12,
            views,
            outputs: [Some(rgb_view), Some(luma_view)],
            rgb,
            luma,
            ring,
            waiting: VecDeque::with_capacity(RING),
            next_slot: 0,
            start: (shown_from * UNITS_PER_SECOND - half_frame).round() as i64,
            left: count,
            ended: false,
            _device: device,
        })
    }

    /// The next decoded frame from the start on, or None at the end.
    fn decoded(&mut self) -> Result<Option<(IMFSample, ID3D11Texture2D, u32)>, String> {
        loop {
            let (mut flags, mut time, mut sample) = (0u32, 0i64, None);
            unsafe {
                self.reader.ReadSample(
                    MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                    0,
                    None,
                    Some(&mut flags),
                    Some(&mut time),
                    Some(&mut sample),
                )
            }
            .map_err(failed("decoding"))?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(None);
            }
            let Some(sample) = sample else { continue };
            if time < self.start {
                continue;
            }
            unsafe {
                let buffer: IMFDXGIBuffer = sample
                    .GetBufferByIndex(0)
                    .and_then(|buffer| buffer.cast())
                    .map_err(failed("a decoded frame"))?;
                let mut raw: *mut c_void = std::ptr::null_mut();
                buffer.GetResource(&ID3D11Texture2D::IID, &mut raw).map_err(failed("a decoded frame"))?;
                let slice = buffer.GetSubresourceIndex().map_err(failed("a decoded frame"))?;
                return Ok(Some((sample, ID3D11Texture2D::from_raw(raw), slice)));
            }
        }
    }

    /// Gives the GPU the next frame: copied where the shader reads it, converted, and copied to a free slot of the
    /// ring. False at the end.
    fn submit(&mut self) -> Result<bool, String> {
        if self.left == Some(0) {
            return Ok(false);
        }
        let Some((sample, texture, slice)) = self.decoded()? else { return Ok(false) };
        let whole = D3D11_BOX { left: 0, top: 0, front: 0, right: SRC_W, bottom: SRC_H, back: 1 };
        let slot = &self.ring[self.next_slot];
        let threads = (DST_W as u32 / PIXELS_PER_THREAD) * DST_H as u32;
        unsafe {
            self.context.CopySubresourceRegion(&self.nv12, 0, 0, 0, 0, &texture, slice, Some(&whole));
            self.context.CSSetShader(&self.shader, None);
            self.context.CSSetShaderResources(0, Some(&self.views));
            self.context.CSSetUnorderedAccessViews(0, 2, Some(self.outputs.as_ptr()), None);
            self.context.Dispatch(threads.div_ceil(THREADS_PER_GROUP), 1, 1);
            self.context.CopyResource(&slot.rgb, &self.rgb);
            self.context.CopyResource(&slot.luma, &self.luma);
        }
        self.ring[self.next_slot].sample = Some(sample);
        self.waiting.push_back(self.next_slot);
        self.next_slot = (self.next_slot + 1) % RING;
        if let Some(left) = &mut self.left {
            *left -= 1;
        }
        Ok(true)
    }

    /// The next frame's RGB (1280 x 720 x 3) into `rgb` and its Y plane (2560 x 1440) into `luma`; false when there
    /// are no more.
    pub fn next_into(&mut self, rgb: &mut [u8], luma: &mut [u8]) -> Result<bool, String> {
        while !self.ended && self.waiting.len() < RING {
            if !self.submit()? {
                self.ended = true;
            }
        }
        let Some(slot) = self.waiting.pop_front() else { return Ok(false) };
        let readback = &mut self.ring[slot];
        for (staging, out) in [(&readback.rgb, rgb), (&readback.luma, luma)] {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            unsafe {
                self.context.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(failed("reading a frame"))?;
                out.copy_from_slice(std::slice::from_raw_parts(mapped.pData as *const u8, out.len()));
                self.context.Unmap(staging, 0);
            }
        }
        readback.sample = None;
        Ok(true)
    }
}
