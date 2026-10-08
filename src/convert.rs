//! A decoded frame (YUV 4:2:0, any size) converted as ffmpeg 8.1 converts it for the review:
//! `scale=1280:720:flags=area` to `rgb24` (the detector's input) or `yuv420p` (the fixed map's), byte for byte as the
//! ffmpeg CLI does on x86-64 (the old review, python/retired/review.py: `rgb_frames`, `_frames`). The model was trained
//! on these exact bytes: GPU color conversion once changed detections at the crosshair. Every step mirrors libswscale's
//! integer code (utils.c `initFilter`, hscale.c, vscale.c, output.c, yuv2rgb.c, and the x86 kernels where ffmpeg uses
//! them), and the names follow swscale's where it has them.
//!
//! In: a frame's Y, U and V planes as the decoder gives them (the browser's decoder through src/wasm.rs and
//! src/session.rs, ffmpeg's pipe in service/src/review.rs), with the recording's color matrix and range. Out: 720p
//! RGB24 for the detector, 720p YUV 4:2:0 for the fixed map and the area finder, and the 720p luma alone for the
//! camera watch.
//!
//! Things vf_scale does in 8.1 that this depends on: the flags are exactly SWS_AREA; the matrix and range come from
//! the frame's tags; chroma is taken as center-sited whatever the tag says (vf_scale overwrites it with its
//! `in_chroma_loc` option, "unspecified" by default).

/// The width the review works in (pixels).
pub const DST_W: usize = 1280;
/// The height the review works in (pixels).
pub const DST_H: usize = 720;

/// 1.0 in 16.16 fixed point: a luma weight of one (yuv2rgb.c's `cy`), and a step of one input sample per output sample
/// (utils.c's `xInc`).
const ONE_16_16: i64 = 1 << 16;
/// Half of ONE_16_16: rounds a 16.16 value to the nearest integer.
const ROUND_16_16: i64 = 1 << 15;

/// The range adjustment of yuv2rgb.c `ff_yuv2rgb_c_init_tables`: limited ("tv") range luma runs from 16 (black) over
/// 219 levels and is stretched to 255 (`cy = (cy * 255) / 219`, `oy = 16 << 16`); in full range the chroma weights
/// shrink by 224 / 255 instead (limited chroma's 224 levels). This one is full range's levels.
const FULL_LEVELS: i64 = 255;
/// Limited range luma's levels, from black to white.
const LIMITED_LUMA_LEVELS: i64 = 219;
/// Limited range chroma's levels.
const LIMITED_CHROMA_LEVELS: i64 = 224;
/// Limited range luma's black.
const LIMITED_BLACK: i64 = 16;

/// The headroom before and after yuv2rgb.c's y table, in entries (swscale_internal.h).
const YUVRGB_TABLE_LUMA_HEADROOM: i64 = 512;
/// The y table's length for 24-bit RGB: 1024 entries and the headroom on both sides.
const Y_TABLE_LEN: usize = 1024 + 2 * YUVRGB_TABLE_LUMA_HEADROOM as usize;
/// The y table's first entry after its headroom is this many luma levels below 0 (yuv2rgb.c: `yb = -(384 << 16) ...`).
const Y_TABLE_START: i64 = 384;
/// Where the chroma tables point into the y table after its headroom (yuv2rgb.c's `yoffs`), in full range.
const FULL_Y_OFFSET: i64 = 384;
/// Where the chroma tables point into the y table after its headroom, in limited range.
const LIMITED_Y_OFFSET: i64 = 326;
/// A weight times chroma's center, 128, in 16.16: `weight >> 9` (yuv2rgb.c `fill_table`'s `inc >> 9`).
const CHROMA_CENTER_SHIFT: u32 = 9;

/// ff_yuv_420_rgb24_ssse3's samples carry 3 extra bits in their 16-bit lanes (`psllw 3`), and its coefficients are the
/// 16.16 weights shifted up 13 (yuv2rgb.c: `roundToInt16(cy * (1 << 13))`), so its products need no shift back.
const SSSE3_EXTRA_BITS: u32 = 3;
/// How far ff_yuv_420_rgb24_ssse3's coefficients are shifted up from the 16.16 weights (`SSSE3_EXTRA_BITS`).
const SSSE3_COEFF_SHIFT: u32 = 13;
/// The kernel's U and V offset, chroma's center with the extra bits (yuv2rgb.c: `c->uOffset = 0x0400...`).
const SSSE3_CHROMA_CENTER: i32 = 128 << SSSE3_EXTRA_BITS;

/// What a horizontal filter's coefficients sum to (utils.c `sws_init_context`: `initFilter(..., 1 << 14, ...)`).
const ACROSS_ONE: i64 = 1 << 14;
/// What a vertical filter's coefficients sum to (`1 << 12`): output.c's kernels blend two rows by shares of it.
const DOWN_ONE: i64 = 1 << 12;
/// swscale's filterAlign on x86 (utils.c `sws_init_context`): the coefficients per output are rounded up to a multiple
/// of these, across and down. The plain C code aligns to 1. This one is across.
const X86_ACROSS_ALIGN: usize = 4;
/// swscale's filterAlign on x86, down.
const X86_DOWN_ALIGN: usize = 2;
/// A step within this of ONE_16_16, between samples at the same place, is unscaled (utils.c `initFilter`).
const UNSCALED_STEP_TOLERANCE: i64 = 10;
/// initFilter's `fone`, the raw filters' one, is 1 << 54 made smaller by the downscale's log2, by 8 bits at most.
const RAW_ONE_BITS: i64 = 54;
/// The most bits the downscale's log2 takes off `fone`.
const MAX_RAW_ONE_SHRINK: i64 = 8;
/// SWS_AREA's coefficient in initFilter: an output's edge is half an output sample (1 << 29 in initFilter's
/// 1/(1 << 30) of a sample) from its center, and an input's share of it runs from FULL_SHARE (all of the input inside
/// the output) to 0, in 16.16 steps. This one is half an output sample.
const HALF_OUTPUT: i64 = 1 << 29;
/// Half of FULL_SHARE: the share falls from FULL_SHARE to 0 over this either side of the output's edge.
const HALF_SHARE: i64 = 1 << (29 + 16);
/// FULL_SHARE's bits: a share in units of `raw_one` is the share times `raw_one >> FULL_SHARE_BITS`.
const FULL_SHARE_BITS: u32 = 30 + 16;
/// An input's whole share of an output, when all of the input lies inside it.
const FULL_SHARE: i64 = 1 << FULL_SHARE_BITS;
/// Taps at a filter's ends are dropped while they add up to less than this share of one (swscale_internal.h).
const SWS_MAX_REDUCE_CUTOFF: f64 = 0.002;
/// A chroma position swscale takes as unspecified (its default `src_h_chr_pos` and the others: utils.c).
const UNSPECIFIED_POSITION: i64 = -513;
/// The source chroma's position (1/256 pixel) as this port takes it: centered, whatever the frame's tag says.
const SOURCE_CHROMA_POSITION: i64 = 128;
/// Filters stay below this many coefficients per output: with coefficients of at most ACROSS_ONE, their sums of
/// 15-bit samples fit in 32 bits.
const FILTER_SIZE_LIMIT: usize = 16;

/// hScale8To15_c (hscale.c): a sum of 8-bit samples times coefficients that sum to ACROSS_ONE, shifted down to a
/// 15-bit sample and capped. This one is the shift down.
const ACROSS_SHIFT: u32 = 7;
/// The largest 15-bit sample, where hScale8To15_c caps a sum.
const MAX_15_BIT: i32 = (1 << 15) - 1;
/// A 15-bit sample back to 8 bits, rounded (output.c's `(buf0[i] + 64) >> 7`, yuv2plane1_8_c's dither of 64). This
/// one is the rounding.
const SAMPLE_15_ROUND: i32 = 64;
/// The shift from a 15-bit sample down to 8 bits.
const SAMPLE_15_SHIFT: u32 = 7;
/// A vertical sum (15-bit samples times coefficients that sum to DOWN_ONE) back to 8 bits: down 19 bits, rounded by
/// half of that (output.c's `1 << 18` and `128 << 11`, yuv2planeX_8_c's dither `64 << 12`). This one is the shift.
const DOWN_SHIFT: u32 = 19;
/// A vertical sum's rounding before its shift down: half of 1 << DOWN_SHIFT.
const DOWN_ROUND: i32 = 1 << 18;
/// ff_yuv2yuvX (x86/yuv2yuvX.asm) starts each sum at the dither (64) plus 8 per tap after the first, shifted down 4,
/// and shifts the sum down 3 at the end. This one is the dither.
const YUVX_DITHER: i32 = 64;
/// What ff_yuv2yuvX adds to a sum's start for each tap after the first.
const YUVX_ROUND_PER_TAP: i32 = 8;
/// The shift down of ff_yuv2yuvX's start value.
const YUVX_START_SHIFT: u32 = 4;
/// The shift down of ff_yuv2yuvX's sum at the end.
const YUVX_SHIFT: u32 = 3;
/// swscale falls back to its C kernels for the last two output rows (swscale.c `swscale`: `dstY >= c->dstH - 2`), so
/// ff_yuv2yuvX writes the luma rows before this, and the chroma rows written with them (one per two luma rows).
const X86_LUMA_ROWS: usize = DST_H - 2;
/// The chroma rows ff_yuv2yuvX writes: those written with its luma rows.
const X86_CHROMA_ROWS: usize = X86_LUMA_ROWS / 2;

/// The color matrices ffmpeg knows (AVColorSpace).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Matrix {
    /// ITU-R BT.709, the HD matrix.
    Bt709,
    /// ITU-R BT.601, the SD matrix; also taken for a frame with no matrix tag.
    Bt601,
    /// The FCC's matrix (FCC 73.682).
    Fcc,
    /// SMPTE 240M.
    Smpte240m,
    /// ITU-R BT.2020.
    Bt2020,
}

impl Matrix {
    /// The matrix by the number the browser gives it (ui/src/app/modes/wasm/core.ts: `matrixNumber`): 0 BT.709,
    /// 1 BT.601 (also unspecified), 2 FCC, 3 SMPTE 240M, 4 BT.2020.
    pub fn from_code(code: u32) -> Matrix {
        match code {
            0 => Matrix::Bt709,
            2 => Matrix::Fcc,
            3 => Matrix::Smpte240m,
            4 => Matrix::Bt2020,
            _ => Matrix::Bt601,
        }
    }

    /// The matrix's number (`from_code`).
    pub fn code(self) -> u32 {
        match self {
            Matrix::Bt709 => 0,
            Matrix::Bt601 => 1,
            Matrix::Fcc => 2,
            Matrix::Smpte240m => 3,
            Matrix::Bt2020 => 4,
        }
    }

    /// The matrix's inverse as yuv2rgb.c's `ff_yuv2rgb_coeffs` gives it: {crv, cbu, cgu, cgv}.
    fn coefficients(self) -> [i64; 4] {
        match self {
            Matrix::Bt709 => [117489, 138438, 13975, 34925],
            Matrix::Bt601 => [104597, 132201, 25675, 53279],
            Matrix::Fcc => [104448, 132798, 24759, 53109],
            Matrix::Smpte240m => [117579, 136230, 16907, 35559],
            Matrix::Bt2020 => [110013, 140363, 12277, 42626],
        }
    }
}

/// C99 integer division, which truncates toward zero (as Rust's does): marks the divisions the C code makes.
fn div_toward_zero(a: i64, b: i64) -> i64 {
    a / b
}

/// libavutil's ROUNDED_DIV.
fn rounded_div(a: i64, b: i64) -> i64 {
    if a >= 0 { (a + (b >> 1)) / b } else { (a - (b >> 1)) / b }
}

/// libavutil's av_log2 (0 for 0).
fn av_log2(value: i64) -> i64 {
    if value > 0 { 63 - value.leading_zeros() as i64 } else { 0 }
}

/// utils.c `sws_init_context`'s lumXInc and chrXInc: the 16.16 step in the input per output sample, rounded.
fn scale_step(input_len: usize, output_len: usize) -> i64 {
    (((input_len as i64) << 16) + (output_len as i64 >> 1)) / output_len as i64
}

/// utils.c `get_local_pos`: a sample position (1/256 pixel, UNSPECIFIED_POSITION for the center) in 1/256 of a sample
/// of a plane subsampled by `subsample_log2`.
fn local_position(subsample_log2: i64, position: i64) -> i64 {
    let position =
        if position == -1 || position <= UNSPECIFIED_POSITION { (128 << subsample_log2) - 128 } else { position };
    (position + 128) >> subsample_log2
}

/// A filter's coefficient and the input sample it weighs, clamped to the line as ffmpeg reads it.
#[derive(Clone, Copy, Debug)]
struct Tap {
    /// The input sample it reads, as its index along the line, clamped to the line.
    input: u32,
    /// The sample's weight, of the filter's one (ACROSS_ONE or DOWN_ONE).
    coefficient: i32,
}

/// One direction's filter for one plane, as utils.c `initFilter` leaves it: per output sample, the first input sample
/// it reads (swscale's filterPos) and its `size` coefficients (swscale's filter and filterSize). Worked out once for a
/// frame size: each output's taps, with the zero coefficients left out.
#[derive(Clone, Debug)]
struct Filter {
    /// Per output, the first input sample it reads (swscale's filterPos).
    first_input: Box<[i64]>,
    /// Per output, its `size` coefficients in turn, zeros and all (swscale's filter).
    coefficients: Box<[i64]>,
    /// The coefficients per output (swscale's filterSize).
    size: usize,
    /// Every output's taps in turn: output `i`'s are `taps[tap_starts[i]..tap_starts[i + 1]]`.
    taps: Box<[Tap]>,
    /// Where each output's taps start in `taps`, then where the last one's end.
    tap_starts: Box<[u32]>,
    /// The input samples in the line.
    input_len: usize,
}

/// Where a filter's tap `tap` reads after its first input `first`, clamped to the line's last sample.
fn clamped_input(first: i64, tap: usize, input_len: usize) -> usize {
    ((first + tap as i64) as usize).min(input_len - 1)
}

impl Filter {
    /// The filter initFilter leaves: per output its first input, its `size` coefficients, and the line's length in
    /// samples. Panics on a filter whose sums could overflow 32 bits.
    fn new(first_input: Vec<i64>, coefficients: Box<[i64]>, size: usize, input_len: usize) -> Filter {
        assert!(
            size < FILTER_SIZE_LIMIT && coefficients.iter().all(|coefficient| coefficient.abs() <= ACROSS_ONE),
            "a filter too large for 32-bit sums"
        );
        let mut taps = Vec::with_capacity(coefficients.len());
        let mut tap_starts = Vec::with_capacity(first_input.len() + 1);
        tap_starts.push(0);
        for (i, &first) in first_input.iter().enumerate() {
            for (j, &coefficient) in coefficients[i * size..(i + 1) * size].iter().enumerate() {
                if coefficient != 0 {
                    let input = clamped_input(first, j, input_len) as u32;
                    taps.push(Tap { input, coefficient: coefficient as i32 });
                }
            }
            tap_starts.push(taps.len() as u32);
        }
        Filter {
            first_input: first_input.into_boxed_slice(),
            coefficients,
            size,
            taps: taps.into_boxed_slice(),
            tap_starts: tap_starts.into_boxed_slice(),
            input_len,
        }
    }

    /// The output samples (one per first input).
    fn output_len(&self) -> usize {
        self.first_input.len()
    }

    /// Output `i`'s coefficients, zeros and all.
    fn coefficients_of(&self, i: usize) -> &[i64] {
        &self.coefficients[i * self.size..(i + 1) * self.size]
    }

    /// Output `i`'s nonzero taps.
    fn taps_of(&self, i: usize) -> &[Tap] {
        &self.taps[self.tap_starts[i] as usize..self.tap_starts[i + 1] as usize]
    }

    /// Output `i` of a line through its taps, as a 15-bit sample.
    fn sum_across(&self, line: &[u8], i: usize) -> i32 {
        to_15_bit(self.taps_of(i).iter().map(|tap| line[tap.input as usize] as i32 * tap.coefficient).sum())
    }

    /// Where output `i`'s tap `j` reads, clamped to the line.
    fn input_at(&self, i: usize, j: usize) -> usize {
        clamped_input(self.first_input[i], j, self.input_len)
    }
}

/// The way a filter runs over a plane: across its rows (hScale) or down them (the vertical kernels).
#[derive(Clone, Copy)]
enum Direction {
    /// Along each row: the line is a row's samples.
    Across,
    /// Down the rows: the line is a column, one sample from each row.
    Down,
}

/// What utils.c `initFilter` is asked for: one direction of one plane, SWS_AREA, no user filter vectors.
struct FilterSpec {
    /// The 16.16 step in the input per output sample (swscale's xInc: `scale_step`).
    x_inc: i64,
    /// The input samples in the line.
    input_len: usize,
    /// The output samples in the line.
    output_len: usize,
    /// The coefficients per output are rounded up to a multiple of this (swscale's filterAlign).
    align: usize,
    /// What each output's coefficients sum to (swscale's `one`): ACROSS_ONE or DOWN_ONE.
    one: i64,
    /// Where the first input sample sits (swscale's srcPos: `local_position`).
    input_position: i64,
    /// Where the first output sample sits (swscale's dstPos: `local_position`).
    output_position: i64,
    /// Built for ffmpeg's x86 kernels (its default), else for its plain C code.
    x86: bool,
}

impl FilterSpec {
    /// The filter `direction` of a plane from `lengths` (input, output) samples, the samples at `positions` (input,
    /// output), as utils.c `sws_init_context` asks for it.
    fn new(direction: Direction, lengths: (usize, usize), positions: (i64, i64), x86: bool) -> FilterSpec {
        let ((input_len, output_len), (input_position, output_position)) = (lengths, positions);
        let (x86_align, one) = match direction {
            Direction::Across => (X86_ACROSS_ALIGN, ACROSS_ONE),
            Direction::Down => (X86_DOWN_ALIGN, DOWN_ONE),
        };
        FilterSpec {
            x_inc: scale_step(input_len, output_len),
            input_len,
            output_len,
            align: if x86 { x86_align } else { 1 },
            one,
            input_position,
            output_position,
            x86,
        }
    }
}

/// A filter before initFilter trims and normalizes it: per output, its first input and its raw coefficients (all
/// rows the same length, summing to about `raw_one`).
struct RawFilter {
    /// Per output, the first input sample it reads.
    first_input: Vec<i64>,
    /// Per output, its raw coefficients from its first input on.
    rows: Vec<Vec<i64>>,
}

impl RawFilter {
    /// An empty raw filter with room for `output_len` outputs.
    fn with_capacity(output_len: usize) -> RawFilter {
        RawFilter { first_input: Vec::with_capacity(output_len), rows: Vec::with_capacity(output_len) }
    }
}

/// utils.c `initFilter`: the raw filter, its near-zero end taps dropped and its size aligned, its taps outside the line
/// moved onto its ends, then normalized to `spec.one`. Each step keeps the C code's arithmetic and order, so the two
/// can be read side by side.
fn init_filter(spec: &FilterSpec) -> Filter {
    let raw_one = raw_one(spec.input_len, spec.output_len);
    let mut raw = raw_filter(spec, raw_one);
    let min_size = trim_near_zero_taps(&mut raw, raw_one);
    // swscale's x86 special case for unscaled vertical filtering
    let align = if spec.x86 && min_size == 1 && spec.align == X86_DOWN_ALIGN { 1 } else { spec.align };
    let size = (min_size + align - 1) & !(align - 1);
    for (first, row) in raw.first_input.iter_mut().zip(&mut raw.rows) {
        row.resize(size, 0);
        fix_borders(first, row, spec.input_len as i64);
    }
    Filter::new(raw.first_input, normalized(&raw.rows, spec.one), size, spec.input_len)
}

/// initFilter's `fone`: the raw filters' one, small enough for their sums to fit 64 bits.
fn raw_one(input_len: usize, output_len: usize) -> i64 {
    1 << (RAW_ONE_BITS - av_log2(input_len as i64 / output_len as i64).min(MAX_RAW_ONE_SHRINK))
}

/// initFilter's first filter for SWS_AREA: each output its input when unscaled, bilinear when upscaling, the mean of
/// the inputs its span covers when downscaling.
fn raw_filter(spec: &FilterSpec, raw_one: i64) -> RawFilter {
    if (spec.x_inc - ONE_16_16).abs() < UNSCALED_STEP_TOLERANCE && spec.input_position == spec.output_position {
        RawFilter { first_input: (0..spec.output_len as i64).collect(), rows: vec![vec![raw_one]; spec.output_len] }
    } else if spec.x_inc <= ONE_16_16 {
        bilinear_filter(spec, raw_one)
    } else {
        area_filter(spec, raw_one)
    }
}

/// SWS_AREA when upscaling is bilinear: each output from the two inputs around its center.
fn bilinear_filter(spec: &FilterSpec, raw_one: i64) -> RawFilter {
    /// The inputs each output reads: the two around its center.
    const SIZE: i64 = 2;
    // the output's center in the input, in 16.16 (initFilter's xDstInSrc; the positions are in 1/256 of a sample)
    let mut center = ((spec.output_position * spec.x_inc) >> 8) - ((spec.input_position * 0x8000) >> 7);
    let mut raw = RawFilter::with_capacity(spec.output_len);
    for _ in 0..spec.output_len {
        let first = (center - ((SIZE - 1) << 15) + (1 << 15)) >> 16;
        raw.first_input.push(first);
        let weight = |input: i64| (raw_one - (input * (1 << 16) - center).abs() * (raw_one >> 16)).max(0);
        raw.rows.push((first..first + SIZE).map(weight).collect());
        center += spec.x_inc;
    }
    raw
}

/// SWS_AREA when downscaling: each output the mean of the inputs its span covers, those it covers in part by the
/// part it covers.
fn area_filter(spec: &FilterSpec, raw_one: i64) -> RawFilter {
    let (input_len, output_len) = (spec.input_len as i64, spec.output_len as i64);
    let size = ((1 + (input_len + output_len - 1) / output_len) as usize).min(spec.input_len.saturating_sub(2)).max(1);
    // the output's center in the input, in 1/(1 << 17) of a sample (initFilter's xDstInSrc)
    let mut center = ((spec.output_position * spec.x_inc) >> 7) - ((spec.input_position * 0x10000) >> 7);
    let mut raw = RawFilter::with_capacity(spec.output_len);
    for _ in 0..spec.output_len {
        let first = div_toward_zero(center - (size as i64 - 2) * (1 << 16), 1 << 17);
        raw.first_input.push(first);
        let share = |input: i64| area_share(input * (1 << 17) - center, spec, raw_one);
        raw.rows.push((first..first + size as i64).map(share).collect());
        center += 2 * spec.x_inc;
    }
    raw
}

/// An input's share of an output (SWS_AREA in initFilter) in units of `raw_one`, from the input's offset from the
/// output's center (1/(1 << 17) of an input sample).
fn area_share(offset: i64, spec: &FilterSpec, raw_one: i64) -> i64 {
    // the distance in 1/(1 << 30) of an output sample (initFilter's `d`), then past the output's edge (`d2`)
    let distance = (offset.abs() << 13) * spec.output_len as i64 / spec.input_len as i64;
    let past_edge = distance - HALF_OUTPUT;
    let share = if past_edge * spec.x_inc < -HALF_SHARE {
        FULL_SHARE
    } else if past_edge * spec.x_inc < HALF_SHARE {
        -past_edge * spec.x_inc + HALF_SHARE
    } else {
        0
    };
    share * (raw_one >> FULL_SHARE_BITS)
}

/// initFilter's first reduction, from the last output to the first: each row's near-zero taps on the left shifted out,
/// and the size the rows need without their near-zero taps on the right.
fn trim_near_zero_taps(raw: &mut RawFilter, raw_one: i64) -> usize {
    let cut_limit = SWS_MAX_REDUCE_CUTOFF * raw_one as f64;
    let mut min_size = 0;
    for i in (0..raw.rows.len()).rev() {
        let next_first = raw.first_input.get(i + 1).copied();
        shift_out_left_zeros(&mut raw.rows[i], &mut raw.first_input[i], next_first, cut_limit);
        min_size = min_size.max(size_without_right_zeros(&raw.rows[i], cut_limit));
    }
    min_size
}

/// Shifts out a row's near-zero taps on the left, moving its first input right while it stays before `next_first`.
fn shift_out_left_zeros(row: &mut [i64], first: &mut i64, next_first: Option<i64>, cut_limit: f64) {
    let mut cut = 0i64;
    for _ in 0..row.len() {
        cut += row[0].abs();
        if cut as f64 > cut_limit {
            break;
        }
        // swscale's kernels need the first inputs in order
        if next_first.is_some_and(|next| *first >= next) {
            break;
        }
        row.copy_within(1.., 0);
        let last = row.len() - 1;
        row[last] = 0;
        *first += 1;
    }
}

/// The size a row needs without its near-zero taps on the right.
fn size_without_right_zeros(row: &[i64], cut_limit: f64) -> usize {
    let mut size = row.len();
    let mut cut = 0i64;
    for coefficient in row[1..].iter().rev() {
        cut += coefficient.abs();
        if cut as f64 > cut_limit {
            break;
        }
        size -= 1;
    }
    size
}

/// initFilter's "fix borders": taps before the line's start move onto its first sample, and a row reaching past its
/// end shifts left, its taps past the end moved onto the last sample.
fn fix_borders(first: &mut i64, row: &mut [i64], input_len: i64) {
    let size = row.len();
    if *first < 0 {
        for j in 1..size {
            let left = (j as i64 + *first).max(0) as usize;
            row[left] += row[j];
            row[j] = 0;
        }
        *first = 0;
    }
    if *first + size as i64 > input_len {
        let shift = (*first + (size as i64 - input_len).min(0)) as usize;
        let mut past_end = 0;
        for j in (0..size).rev() {
            if *first + j as i64 >= input_len {
                past_end += row[j];
                row[j] = 0;
            }
        }
        for j in (0..size).rev() {
            row[j] = if j < shift { 0 } else { row[j - shift] };
        }
        *first -= shift as i64;
        row[(input_len - 1 - *first) as usize] += past_end;
    }
}

/// initFilter's last step: each row normalized to `one`, carrying each coefficient's rounding error to the next.
fn normalized(rows: &[Vec<i64>], one: i64) -> Box<[i64]> {
    let mut coefficients = Vec::with_capacity(rows.iter().map(Vec::len).sum());
    for row in rows {
        let sum: i64 = row.iter().sum();
        let divisor = ((sum + one / 2) / one).max(1);
        let mut error = 0;
        for &raw in row {
            let carried = raw + error;
            let coefficient = rounded_div(carried, divisor);
            coefficients.push(coefficient);
            error = carried - coefficient * divisor;
        }
    }
    coefficients.into_boxed_slice()
}

/// hScale8To15_c's last step: a sum of 8-bit samples times coefficients as a 15-bit sample.
fn to_15_bit(sum: i32) -> i32 {
    (sum >> ACROSS_SHIFT).min(MAX_15_BIT)
}

/// hScale8To15_c: a plane's rows (`width` x `height` 8-bit samples) scaled across to 15-bit samples, into `scaled`
/// (`height` rows of the filter's outputs; kept from frame to frame).
fn scale_across(plane: &[u8], width: usize, height: usize, filter: &Filter, scaled: &mut Vec<i32>) {
    scaled.resize(height * filter.output_len(), 0);
    match filter.size {
        1 => scale_across_n::<1>(plane, width, height, filter, scaled),
        2 => scale_across_n::<2>(plane, width, height, filter, scaled),
        3 => scale_across_n::<3>(plane, width, height, filter, scaled),
        4 => scale_across_n::<4>(plane, width, height, filter, scaled),
        _ => {
            let lines = plane.chunks_exact(width).take(height);
            for (line, outputs) in lines.zip(scaled.chunks_exact_mut(filter.output_len())) {
                for (i, output) in outputs.iter_mut().enumerate() {
                    *output = filter.sum_across(line, i);
                }
            }
        }
    }
}

/// `scale_across` for a filter of N coefficients: an output whose N inputs all lie in the line reads them at once (the
/// same sum, in an order the compiler can run side by side), one at the edge through its clamped taps.
fn scale_across_n<const N: usize>(plane: &[u8], width: usize, height: usize, filter: &Filter, scaled: &mut [i32]) {
    let coefficients: Vec<[i32; N]> =
        filter.coefficients.chunks_exact(N).map(|row| std::array::from_fn(|j| row[j] as i32)).collect();
    let lines = plane.chunks_exact(width).take(height);
    for (line, outputs) in lines.zip(scaled.chunks_exact_mut(filter.output_len())) {
        for (i, (output, weights)) in outputs.iter_mut().zip(&coefficients).enumerate() {
            let first = filter.first_input[i];
            *output = if first >= 0 && first as usize + N <= width {
                let inputs: &[u8; N] = line[first as usize..first as usize + N].try_into().unwrap();
                to_15_bit((0..N).map(|j| inputs[j] as i32 * weights[j]).sum())
            } else {
                filter.sum_across(line, i)
            };
        }
    }
}

/// libavutil's av_clip_uint8.
fn clip_u8(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

/// 15-bit samples to 8 bits, rounded and clipped.
fn round_to_u8(out: &mut [u8], samples: &[i32]) {
    for (byte, &sample) in out.iter_mut().zip(samples) {
        *byte = clip_u8((sample + SAMPLE_15_ROUND) >> SAMPLE_15_SHIFT);
    }
}

/// Vertical sums to 8 bits, clipped (their rounding is in the sums).
fn sums_to_u8(out: &mut [u8], sums: &[i32]) {
    for (byte, &sum) in out.iter_mut().zip(sums) {
        *byte = clip_u8(sum >> DOWN_SHIFT);
    }
}

/// Two rows of 15-bit samples blended to 8 bits: `share` (of DOWN_ONE) of `bottom` and the rest of `top`, plus
/// `round`.
fn blend_to_u8(out: &mut [u8], top: &[i32], bottom: &[i32], share: i32, round: i32) {
    let top_share = DOWN_ONE as i32 - share;
    for ((byte, &upper), &lower) in out.iter_mut().zip(top).zip(bottom) {
        *byte = clip_u8((upper * top_share + lower * share + round) >> DOWN_SHIFT);
    }
}

/// Output row `row` of a vertical filter over a plane scaled across (`scaled`: rows of `width` 15-bit samples): its
/// taps' rows times their coefficients, summed into `sums` (`width` of them, each started at `start`).
fn sum_down(scaled: &[i32], width: usize, filter: &Filter, row: usize, start: i32, sums: &mut [i32]) {
    sums.fill(start);
    for tap in filter.taps_of(row) {
        let line = &scaled[tap.input as usize * width..][..width];
        for (sum, &sample) in sums.iter_mut().zip(line) {
            *sum += sample * tap.coefficient;
        }
    }
}

/// A chroma table as a line (`RgbFormula`): entry c is offset + ((c * slope) >> 16).
type ChromaLine = (i32, i32);

/// A matrix's weights adjusted for the range as yuv2rgb.c `ff_yuv2rgb_c_init_tables` adjusts them (no brightness,
/// contrast and saturation 1), under its names: `crv` V's weight in red, `cbu` U's in blue, `cgu` and `cgv` U's and V's
/// in green (negative), `cy` luma's weight, all 16.16; `oy` luma's offset (black).
struct RangeWeights {
    /// V's weight in red (16.16).
    crv: i64,
    /// U's weight in blue (16.16).
    cbu: i64,
    /// U's weight in green (16.16, negative).
    cgu: i64,
    /// V's weight in green (16.16, negative).
    cgv: i64,
    /// Luma's weight (16.16): 1 in full range, 255 / 219 in limited range.
    cy: i64,
    /// Luma's black (16.16): 0 in full range, 16 in limited range.
    oy: i64,
}

impl RangeWeights {
    /// The weights for a matrix and a range (`full_range`: full, else limited).
    fn new(matrix: Matrix, full_range: bool) -> RangeWeights {
        let [crv, cbu, cgu, cgv] = matrix.coefficients();
        let (cgu, cgv) = (-cgu, -cgv);
        if full_range {
            let to_full = |weight: i64| div_toward_zero(weight * LIMITED_CHROMA_LEVELS, FULL_LEVELS);
            let (crv, cbu, cgu, cgv) = (to_full(crv), to_full(cbu), to_full(cgu), to_full(cgv));
            RangeWeights { crv, cbu, cgu, cgv, cy: ONE_16_16, oy: 0 }
        } else {
            let cy = div_toward_zero(ONE_16_16 * FULL_LEVELS, LIMITED_LUMA_LEVELS);
            RangeWeights { crv, cbu, cgu, cgv, cy, oy: LIMITED_BLACK << 16 }
        }
    }
}

/// ff_yuv2rgb_c_init_tables() for 24 bits, no brightness, contrast and saturation 1: for 8-bit Y, U, V,
/// R = y_table[red_v[V] + Y], G = y_table[green_u[U] + green_v[V] + Y], B = y_table[blue_u[U] + Y] (swscale's yuvTable,
/// table_rV, table_gU, table_gV and table_bU).
struct RgbTables {
    /// A level's 8-bit value, by a chroma table's entry plus the luma (swscale's yuvTable).
    y_table: [u8; Y_TABLE_LEN],
    /// Per V, red's entry into y_table before the luma is added (table_rV).
    red_v: [i64; 256],
    /// Per U, its part of green's entry into y_table (table_gU).
    green_u: [i64; 256],
    /// Per U, blue's entry into y_table before the luma is added (table_bU).
    blue_u: [i64; 256],
    /// Per V, its part of green's entry into y_table (table_gV).
    green_v: [i64; 256],
    /// The same tables as formulas, for code that computes them (the 2:1 rows with SIMD).
    #[cfg_attr(
        not(any(target_arch = "x86_64", all(target_arch = "wasm32", target_feature = "simd128"))),
        allow(dead_code)
    )]
    formula: RgbFormula,
}

/// RgbTables as formulas: y_table[i] = ((y_base + i * y_step) >> 16) clamped to 0..255, and each chroma table a
/// `ChromaLine`, with the green one the sum of green_u's and green_v's. All in i32: every value they reach fits.
#[derive(Clone, Copy)]
#[cfg_attr(not(any(target_arch = "x86_64", all(target_arch = "wasm32", target_feature = "simd128"))), allow(dead_code))]
struct RgbFormula {
    /// y_table's entry 0 before its shift down (16.16).
    y_base: i32,
    /// The step from one y_table entry to the next before the shift (16.16: luma's weight, `cy`).
    y_step: i32,
    /// red_v as a line.
    red_v: ChromaLine,
    /// green_u as a line.
    green_u: ChromaLine,
    /// blue_u as a line.
    blue_u: ChromaLine,
    /// green_v as a line.
    green_v: ChromaLine,
}

impl RgbTables {
    /// The tables for a matrix and a range (`full_range`: full, else limited).
    fn new(matrix: Matrix, full_range: bool) -> RgbTables {
        let weights = RangeWeights::new(matrix, full_range);
        let cy = weights.cy;
        // the chroma weights relative to luma's, rounded (`crv = ((crv * (1 << 16)) + 0x8000) / FFMAX(cy, 1)`)
        let per_luma = |weight: i64| div_toward_zero(weight * ONE_16_16 + ROUND_16_16, cy.max(1));
        let (crv, cbu, cgu, cgv) =
            (per_luma(weights.crv), per_luma(weights.cbu), per_luma(weights.cgu), per_luma(weights.cgv));
        let y_offset = (if full_range { FULL_Y_OFFSET } else { LIMITED_Y_OFFSET }) + YUVRGB_TABLE_LUMA_HEADROOM;
        let y_base = -(Y_TABLE_START << 16) - YUVRGB_TABLE_LUMA_HEADROOM * cy - weights.oy + ROUND_16_16;
        let y_table = std::array::from_fn(|i| ((y_base + i as i64 * cy) >> 16).clamp(0, 255) as u8);
        // yuv2rgb.c `fill_table`: entry c is the offset plus `(c * weight) >> 16`, less the center's (`weight >> 9`)
        let table = |weight: i64, offset: i64| -> [i64; 256] {
            std::array::from_fn(|chroma| offset - (weight >> CHROMA_CENTER_SHIFT) + ((chroma as i64 * weight) >> 16))
        };
        let line = |weight: i64, offset: i64| ((offset - (weight >> CHROMA_CENTER_SHIFT)) as i32, weight as i32);
        RgbTables {
            y_table,
            red_v: table(crv, y_offset),
            green_u: table(cgu, y_offset),
            blue_u: table(cbu, y_offset),
            green_v: table(cgv, 0),
            formula: RgbFormula {
                y_base: y_base as i32,
                y_step: cy as i32,
                red_v: line(crv, y_offset),
                green_u: line(cgu, y_offset),
                blue_u: line(cbu, y_offset),
                green_v: line(cgv, 0),
            },
        }
    }

    /// One pixel's R, G and B through the tables, into `rgb` (3 bytes).
    fn pixel(&self, luma: u8, chroma_u: u8, chroma_v: u8, rgb: &mut [u8]) {
        let luma = luma as i64;
        let (chroma_u, chroma_v) = (chroma_u as usize, chroma_v as usize);
        rgb[0] = self.y_table[(self.red_v[chroma_v] + luma) as usize];
        rgb[1] = self.y_table[(self.green_u[chroma_u] + self.green_v[chroma_v] + luma) as usize];
        rgb[2] = self.y_table[(self.blue_u[chroma_u] + luma) as usize];
    }
}

/// yuv2rgb.c `roundToInt16`: a 16.16 value rounded to an integer and saturated to 16 bits.
fn round_to_int16(value: i64) -> i32 {
    let rounded = (value + ROUND_16_16) >> 16;
    if rounded < -i64::from(i16::MAX) { i32::from(i16::MIN) } else { rounded.min(i64::from(i16::MAX)) as i32 }
}

/// A 16-bit lane's saturating result (paddsw, packssdw).
fn saturate_i16(value: i32) -> i32 {
    value.clamp(i16::MIN.into(), i16::MAX.into())
}

/// A 16-bit lane's wrapping result (paddw, psubw).
fn wrap_i16(value: i32) -> i32 {
    i32::from(value as i16)
}

/// pmulhw: the top 16 bits of two 16-bit lanes' product.
fn pmulhw(a: i32, b: i32) -> i32 {
    (a * b) >> 16
}

/// The 16-bit coefficients of ffmpeg's x86 yuv2rgb kernel (ff_yuv_420_rgb24_ssse3), as yuv2rgb.c
/// `ff_yuv2rgb_c_init_tables` stores them (roundToInt16), under its names (yCoeff, vrCoeff, ubCoeff, vgCoeff, ugCoeff,
/// yOffset).
struct Ssse3Coefficients {
    /// Luma's weight.
    y_coeff: i32,
    /// V's weight in red.
    vr_coeff: i32,
    /// U's weight in blue.
    ub_coeff: i32,
    /// V's weight in green.
    vg_coeff: i32,
    /// U's weight in green.
    ug_coeff: i32,
    /// Luma's black, with the kernel's extra bits.
    y_offset: i32,
}

impl Ssse3Coefficients {
    /// The kernel's coefficients for a matrix and a range (`full_range`: full, else limited).
    fn new(matrix: Matrix, full_range: bool) -> Ssse3Coefficients {
        let weights = RangeWeights::new(matrix, full_range);
        let coeff = |weight: i64| round_to_int16(weight << SSSE3_COEFF_SHIFT);
        Ssse3Coefficients {
            y_coeff: coeff(weights.cy),
            vr_coeff: coeff(weights.crv),
            ub_coeff: coeff(weights.cbu),
            vg_coeff: coeff(weights.cgv),
            ug_coeff: coeff(weights.cgu),
            y_offset: round_to_int16(weights.oy << SSSE3_EXTRA_BITS),
        }
    }

    /// One pixel as the kernel computes it, in 16-bit lanes, into `rgb` (3 bytes).
    fn pixel(&self, luma: u8, chroma_u: u8, chroma_v: u8, rgb: &mut [u8]) {
        let u_lane = saturate_i16((i32::from(chroma_u) << SSSE3_EXTRA_BITS) - SSSE3_CHROMA_CENTER);
        let v_lane = saturate_i16((i32::from(chroma_v) << SSSE3_EXTRA_BITS) - SSSE3_CHROMA_CENTER);
        let luma_lane = pmulhw(wrap_i16((i32::from(luma) << SSSE3_EXTRA_BITS) - self.y_offset), self.y_coeff);
        let green = saturate_i16(pmulhw(u_lane, self.ug_coeff) + pmulhw(v_lane, self.vg_coeff));
        rgb[0] = clip_u8(saturate_i16(luma_lane + pmulhw(v_lane, self.vr_coeff)));
        rgb[1] = clip_u8(saturate_i16(luma_lane + green));
        rgb[2] = clip_u8(saturate_i16(luma_lane + pmulhw(u_lane, self.ub_coeff)));
    }
}

/// A plane's two filters: across its rows and down them.
struct PlaneFilters {
    /// The filter along each row.
    across: Filter,
    /// The filter down the rows.
    down: Filter,
}

/// The filters swscale builds for a frame size: luma's and chroma's.
struct Filters {
    /// The Y plane's filters.
    luma: PlaneFilters,
    /// The U and V planes' filters, the same for both.
    chroma: PlaneFilters,
}

impl Filters {
    /// For frames of `width` x `height`, to rgb24 (`rgb_out`) or yuv420p, for ffmpeg's `x86` kernels or its C code.
    fn new(width: usize, height: usize, rgb_out: bool, x86: bool) -> Filters {
        let filter = |direction: Direction, lengths: (usize, usize), positions: (i64, i64)| {
            init_filter(&FilterSpec::new(direction, lengths, positions, x86))
        };
        // luma samples sit at their pixels; the source chroma is centered
        let luma = local_position(0, 0);
        let source_chroma = local_position(1, SOURCE_CHROMA_POSITION);
        // the output chroma's height and positions across (x) and down (y): packed RGB keeps chroma at half width and
        // full height (yuv2rgb24's pairs of pixels), yuv420p halves both
        let (out_chroma_height, out_chroma_x, out_chroma_y) = if rgb_out {
            (DST_H, local_position(1, UNSPECIFIED_POSITION), local_position(0, UNSPECIFIED_POSITION))
        } else {
            (DST_H.div_ceil(2), source_chroma, source_chroma)
        };
        let (chroma_width, chroma_height) = (width.div_ceil(2), height.div_ceil(2));
        Filters {
            luma: PlaneFilters {
                across: filter(Direction::Across, (width, DST_W), (luma, luma)),
                down: filter(Direction::Down, (height, DST_H), (luma, luma)),
            },
            chroma: PlaneFilters {
                across: filter(Direction::Across, (chroma_width, DST_W.div_ceil(2)), (source_chroma, out_chroma_x)),
                down: filter(Direction::Down, (chroma_height, out_chroma_height), (source_chroma, out_chroma_y)),
            },
        }
    }
}

/// Converts a recording's frames: built once per recording (its size and color tags), then one call per frame.
pub struct Converter {
    /// The decoded frames' width (pixels).
    width: usize,
    /// The decoded frames' height (pixels).
    height: usize,
    /// The tables swscale's C code turns YUV into RGB with.
    rgb_tables: RgbTables,
    /// The coefficients of ffmpeg's x86 kernel for frames already 1280 x 720.
    ssse3: Ssse3Coefficients,
    /// x86 kernels where ffmpeg uses them (its default); false: the plain C code (`-cpuflags 0`).
    x86: bool,
    /// The 2:1 shortcut is allowed (it gives the same bytes; tests turn it off to compare).
    shortcut: bool,
    /// The filters to rgb24, built for the first frame that needs them.
    rgb_filters: Option<Filters>,
    /// The filters to yuv420p (and to the luma alone), built for the first frame that needs them.
    yuv_filters: Option<Filters>,
    /// The full pipeline's buffers, kept from frame to frame.
    scratch: Scratch,
}

impl Converter {
    /// For frames of `width` x `height`, with the recording's color matrix and range (full: "pc", JPEG range).
    pub fn new(width: usize, height: usize, matrix: Matrix, full_range: bool) -> Converter {
        Converter::with_kernels(width, height, matrix, full_range, true)
    }

    /// As `new`, choosing ffmpeg's x86 kernels (true, its default) or its plain C code (false, `-cpuflags 0`).
    pub fn with_kernels(width: usize, height: usize, matrix: Matrix, full_range: bool, x86: bool) -> Converter {
        Converter {
            width,
            height,
            rgb_tables: RgbTables::new(matrix, full_range),
            ssse3: Ssse3Coefficients::new(matrix, full_range),
            x86,
            shortcut: true,
            rgb_filters: None,
            yuv_filters: None,
            scratch: Scratch::default(),
        }
    }

    /// As `with_kernels`, always through ffmpeg's full pipeline: no 2:1 shortcut. For checking the shortcut.
    pub fn without_shortcut(width: usize, height: usize, matrix: Matrix, full_range: bool, x86: bool) -> Converter {
        Converter { shortcut: false, ..Converter::with_kernels(width, height, matrix, full_range, x86) }
    }

    /// The chroma planes' width and height.
    fn chroma_size(&self) -> (usize, usize) {
        (self.width.div_ceil(2), self.height.div_ceil(2))
    }

    /// A frame's Y, U and V planes.
    fn planes<'a>(&self, yuv: &'a [u8]) -> (&'a [u8], &'a [u8], &'a [u8]) {
        let (chroma_width, chroma_height) = self.chroma_size();
        let (y_plane, rest) = yuv.split_at(self.width * self.height);
        let (u_plane, rest) = rest.split_at(chroma_width * chroma_height);
        (y_plane, u_plane, &rest[..chroma_width * chroma_height])
    }

    /// Whether the frames are 1280 x 720 already.
    fn is_unscaled(&self) -> bool {
        self.width == DST_W && self.height == DST_H
    }

    /// Whether the frames are exactly 2560 x 1440 and the 2:1 shortcut is allowed.
    fn takes_half_shortcut(&self) -> bool {
        self.shortcut && self.width == 2 * DST_W && self.height == 2 * DST_H
    }

    /// The luma and chroma rows ffmpeg's x86 vertical kernel writes (none with its C code).
    fn x86_rows(&self) -> (usize, usize) {
        if self.x86 { (X86_LUMA_ROWS, X86_CHROMA_ROWS) } else { (0, 0) }
    }

    /// The frame as RGB24 at 1280 x 720 (row by row, R, G, B), into `out` (DST_W * DST_H * 3 bytes).
    pub fn rgb24(&mut self, yuv: &[u8], out: &mut [u8]) {
        let (y_plane, u_plane, v_plane) = self.planes(yuv);
        if self.is_unscaled() {
            return self.unscaled_rgb24(y_plane, u_plane, v_plane, out);
        }
        if self.takes_half_shortcut() {
            return self.half_rgb24(y_plane, u_plane, v_plane, out);
        }
        self.scaled_rgb24(y_plane, u_plane, v_plane, out);
    }

    /// swscale's full pipeline to RGB24: each plane scaled across, then each output row down by the kernel swscale
    /// picks for it (`PackedKernel`), then through the tables. The same integer arithmetic, so the same bytes.
    fn scaled_rgb24(&mut self, y_plane: &[u8], u_plane: &[u8], v_plane: &[u8], out: &mut [u8]) {
        let (chroma_width, chroma_height) = self.chroma_size();
        let filters = self.rgb_filters.get_or_insert_with(|| Filters::new(self.width, self.height, true, self.x86));
        let scratch = &mut self.scratch;
        scale_across(y_plane, self.width, self.height, &filters.luma.across, &mut scratch.across);
        scale_across(u_plane, chroma_width, chroma_height, &filters.chroma.across, &mut scratch.across_u);
        scale_across(v_plane, chroma_width, chroma_height, &filters.chroma.across, &mut scratch.across_v);
        let out_chroma_width = filters.chroma.across.output_len();
        scratch.row_y.resize(DST_W, 0);
        scratch.row_u.resize(out_chroma_width, 0);
        scratch.row_v.resize(out_chroma_width, 0);
        scratch.sums.resize(DST_W.max(out_chroma_width), 0);
        for row in 0..DST_H {
            scratch.down_rgb_row(filters, row, out_chroma_width);
            let rgb_row = &mut out[row * DST_W * 3..(row + 1) * DST_W * 3];
            for (i, pixel) in rgb_row.chunks_exact_mut(3).enumerate() {
                self.rgb_tables.pixel(scratch.row_y[i], scratch.row_u[i / 2], scratch.row_v[i / 2], pixel);
            }
        }
    }

    /// Exactly 2:1 (2560 x 1440): the filters reduce to plain rounded means, the same bytes much faster. A row at a
    /// time (lanes::half_row).
    fn half_rgb24(&self, y_plane: &[u8], u_plane: &[u8], v_plane: &[u8], out: &mut [u8]) {
        let (width, chroma_width) = (self.width, self.width / 2);
        for row in 0..DST_H {
            let top_luma = &y_plane[2 * row * width..(2 * row + 1) * width];
            let bottom_luma = &y_plane[(2 * row + 1) * width..(2 * row + 2) * width];
            let u_row = &u_plane[row * chroma_width..(row + 1) * chroma_width];
            let v_row = &v_plane[row * chroma_width..(row + 1) * chroma_width];
            let rgb_row = &mut out[row * DST_W * 3..(row + 1) * DST_W * 3];
            lanes::half_row(top_luma, bottom_luma, u_row, v_row, rgb_row, &self.rgb_tables);
        }
    }

    /// The same size: swscale's special converter (x86: ff_yuv_420_rgb24_ssse3; C: the tables).
    fn unscaled_rgb24(&self, y_plane: &[u8], u_plane: &[u8], v_plane: &[u8], out: &mut [u8]) {
        let chroma_width = self.width.div_ceil(2);
        for row in 0..DST_H {
            for i in 0..DST_W {
                let chroma_at = (row / 2) * chroma_width + i / 2;
                let luma = y_plane[row * self.width + i];
                let (chroma_u, chroma_v) = (u_plane[chroma_at], v_plane[chroma_at]);
                let pixel = &mut out[(row * DST_W + i) * 3..(row * DST_W + i) * 3 + 3];
                if self.x86 {
                    self.ssse3.pixel(luma, chroma_u, chroma_v, pixel);
                } else {
                    self.rgb_tables.pixel(luma, chroma_u, chroma_v, pixel);
                }
            }
        }
    }

    /// The frame as YUV 4:2:0 at 1280 x 720 (Y, then U, then V), into `out` (DST_W * DST_H * 3 / 2 bytes). The range
    /// and matrix stay as they are.
    pub fn yuv420p(&mut self, yuv: &[u8], out: &mut [u8]) {
        let (chroma_width, chroma_height) = self.chroma_size();
        if self.is_unscaled() {
            out.copy_from_slice(&yuv[..DST_W * DST_H + 2 * chroma_width * chroma_height]);
            return;
        }
        let (y_plane, u_plane, v_plane) = self.planes(yuv);
        let (out_chroma_width, out_chroma_height) = (DST_W / 2, DST_H / 2);
        let (out_y, rest) = out.split_at_mut(DST_W * DST_H);
        let (out_u, out_v) = rest.split_at_mut(out_chroma_width * out_chroma_height);
        if self.takes_half_shortcut() {
            mean_2x2(y_plane, self.width, out_y, DST_W, DST_H);
            mean_2x2(u_plane, chroma_width, out_u, out_chroma_width, out_chroma_height);
            mean_2x2(v_plane, chroma_width, out_v, out_chroma_width, out_chroma_height);
            return;
        }
        let (luma_x86_rows, chroma_x86_rows) = self.x86_rows();
        let filters = self.yuv_filters.get_or_insert_with(|| Filters::new(self.width, self.height, false, self.x86));
        let scratch = &mut self.scratch;
        scratch.scale_plane(y_plane, (self.width, self.height), &filters.luma, out_y, luma_x86_rows);
        scratch.scale_plane(u_plane, (chroma_width, chroma_height), &filters.chroma, out_u, chroma_x86_rows);
        scratch.scale_plane(v_plane, (chroma_width, chroma_height), &filters.chroma, out_v, chroma_x86_rows);
    }

    /// The frame's luma at 1280 x 720 from its Y plane alone (`y_plane`: width x height bytes), into `out`
    /// (DST_W * DST_H bytes): the Y plane yuv420p gives, for what reads only the luma (the camera watch), without
    /// scaling the chroma.
    pub fn luma(&mut self, y_plane: &[u8], out: &mut [u8]) {
        if self.is_unscaled() {
            out.copy_from_slice(&y_plane[..DST_W * DST_H]);
            return;
        }
        if self.takes_half_shortcut() {
            mean_2x2(y_plane, self.width, out, DST_W, DST_H);
            return;
        }
        let (luma_x86_rows, _) = self.x86_rows();
        let filters = self.yuv_filters.get_or_insert_with(|| Filters::new(self.width, self.height, false, self.x86));
        self.scratch.scale_plane(y_plane, (self.width, self.height), &filters.luma, out, luma_x86_rows);
    }
}

/// swscale's vertical kernel for a row of packed RGB (vscale.c `packed_vscale`; output.c `yuv2rgb24_1`, `_2`, `_X`).
#[derive(Clone, Copy)]
enum PackedKernel {
    /// yuv2rgb24_1: one luma row, and one chroma row (`chroma_share` 0) or two blended, `chroma_share` (of DOWN_ONE)
    /// of the second.
    One {
        /// The second chroma row's share, out of DOWN_ONE; 0 for one chroma row.
        chroma_share: i32,
    },
    /// yuv2rgb24_2: two luma rows and two chroma rows, each pair blended by its second row's share.
    Two {
        /// The second luma row's share of the blend.
        luma_share: i32,
        /// The second chroma row's share of the blend.
        chroma_share: i32,
    },
    /// yuv2rgb24_X: every row of the filters, summed.
    Many,
}

impl PackedKernel {
    /// The kernel for output row `row`, by its vertical filters' coefficients.
    fn for_row(filters: &Filters, row: usize) -> PackedKernel {
        let luma = filters.luma.down.coefficients_of(row);
        let chroma = filters.chroma.down.coefficients_of(row);
        let chroma_blends = blends_two_rows(chroma);
        if luma.len() == 1 && (chroma.len() == 1 || chroma_blends) {
            PackedKernel::One { chroma_share: if chroma.len() == 2 { chroma[1] as i32 } else { 0 } }
        } else if chroma_blends && blends_two_rows(luma) {
            PackedKernel::Two { luma_share: luma[1] as i32, chroma_share: chroma[1] as i32 }
        } else {
            PackedKernel::Many
        }
    }
}

/// A vertical filter's row blends two input rows: two coefficients that sum to DOWN_ONE, the second within
/// 0..=DOWN_ONE.
fn blends_two_rows(coefficients: &[i64]) -> bool {
    coefficients.len() == 2
        && coefficients[0] + coefficients[1] == DOWN_ONE
        && (0..=DOWN_ONE).contains(&coefficients[1])
}

/// The buffers a conversion through the full pipeline fills, kept from frame to frame.
#[derive(Default)]
struct Scratch {
    /// A plane scaled across (15-bit): the Y plane for rgb24, each plane in turn for yuv420p.
    across: Vec<i32>,
    /// rgb24's U plane scaled across (15-bit).
    across_u: Vec<i32>,
    /// rgb24's V plane scaled across (15-bit).
    across_v: Vec<i32>,
    /// rgb24's current output row: its 8-bit Y.
    row_y: Vec<u8>,
    /// rgb24's current output row's 8-bit U, one per two pixels.
    row_u: Vec<u8>,
    /// rgb24's current output row's 8-bit V, one per two pixels.
    row_v: Vec<u8>,
    /// A row's vertical sums.
    sums: Vec<i32>,
}

impl Scratch {
    /// Output row `row` of rgb24's vertical pass into row_y, row_u and row_v (`chroma_width` samples), by the kernel
    /// swscale picks for it.
    fn down_rgb_row(&mut self, filters: &Filters, row: usize, chroma_width: usize) {
        let luma_rows = |j: usize| &self.across[filters.luma.down.input_at(row, j) * DST_W..][..DST_W];
        let chroma_start = |j: usize| filters.chroma.down.input_at(row, j) * chroma_width;
        let chroma_planes = [(&self.across_u, &mut self.row_u), (&self.across_v, &mut self.row_v)];
        match PackedKernel::for_row(filters, row) {
            PackedKernel::One { chroma_share } => {
                round_to_u8(&mut self.row_y, luma_rows(0));
                for (scaled, out) in chroma_planes {
                    let top = &scaled[chroma_start(0)..][..chroma_width];
                    if chroma_share == 0 {
                        round_to_u8(out, top);
                    } else {
                        let bottom = &scaled[chroma_start(1)..][..chroma_width];
                        blend_to_u8(out, top, bottom, chroma_share, DOWN_ROUND);
                    }
                }
            }
            PackedKernel::Two { luma_share, chroma_share } => {
                blend_to_u8(&mut self.row_y, luma_rows(0), luma_rows(1), luma_share, 0);
                for (scaled, out) in chroma_planes {
                    let top = &scaled[chroma_start(0)..][..chroma_width];
                    let bottom = &scaled[chroma_start(1)..][..chroma_width];
                    blend_to_u8(out, top, bottom, chroma_share, 0);
                }
            }
            PackedKernel::Many => {
                sum_down(&self.across, DST_W, &filters.luma.down, row, DOWN_ROUND, &mut self.sums[..DST_W]);
                sums_to_u8(&mut self.row_y, &self.sums);
                for (scaled, out) in chroma_planes {
                    let sums = &mut self.sums[..chroma_width];
                    sum_down(scaled, chroma_width, &filters.chroma.down, row, DOWN_ROUND, sums);
                    sums_to_u8(out, &self.sums);
                }
            }
        }
    }

    /// One plane (`size`: width, height) scaled as yuv420p scales it, into `out` (the filters' output size): across,
    /// then down by ffmpeg's x86 vertical kernel on its first `x86_rows` rows and its C code on the rest.
    fn scale_plane(
        &mut self,
        plane: &[u8],
        size: (usize, usize),
        filters: &PlaneFilters,
        out: &mut [u8],
        x86_rows: usize,
    ) {
        let (width, height) = size;
        scale_across(plane, width, height, &filters.across, &mut self.across);
        let (out_width, out_height) = (filters.across.output_len(), filters.down.output_len());
        self.sums.resize(out_width, 0);
        for (row, out_row) in out.chunks_exact_mut(out_width).take(out_height).enumerate() {
            if filters.down.size == 1 {
                round_to_u8(out_row, &self.across[filters.down.input_at(row, 0) * out_width..][..out_width]);
            } else if row < x86_rows {
                self.yuvx_row(&filters.down, row, out_width, out_row);
            } else {
                sum_down(&self.across, out_width, &filters.down, row, DOWN_ROUND, &mut self.sums);
                sums_to_u8(out_row, &self.sums);
            }
        }
    }

    /// Output row `row` as ff_yuv2yuvX writes it: each tap's product shifted down before the sum (pmulhw), in 16-bit
    /// lanes that wrap.
    fn yuvx_row(&mut self, down: &Filter, row: usize, width: usize, out: &mut [u8]) {
        self.sums.fill((YUVX_DITHER + YUVX_ROUND_PER_TAP * (down.size as i32 - 1)) >> YUVX_START_SHIFT);
        for tap in down.taps_of(row) {
            for (sum, &sample) in self.sums.iter_mut().zip(&self.across[tap.input as usize * width..][..width]) {
                *sum += pmulhw(sample, tap.coefficient);
            }
        }
        for (byte, &sum) in out.iter_mut().zip(&self.sums) {
            *byte = clip_u8(wrap_i16(sum) >> YUVX_SHIFT);
        }
    }
}

/// One row of the 2:1 conversion: each output pixel the rounded mean of a 2 x 2 luma block, each pair of pixels the
/// rounded mean of two chroma samples, through RgbTables. Where the CPU has WebAssembly SIMD (the browser), 16 pixels
/// at a time with the tables as formulas (RgbFormula); where it has AVX2 (the desktop app on x86-64), 32 at a time the
/// same way; else a pixel at a time with the tables. The same integer arithmetic every way, so the same bytes (tests:
/// convert_parity and avx2_rows_give_the_tables natively, test_out/browser_check/rgb-bench.html in the browser).
mod lanes {
    use super::{DST_W, RgbTables};

    /// One row of the 2:1 conversion natively: two luma rows (2560 samples each) and a U and a V row (1280 each) into
    /// a row of RGB24 (3840 bytes); with AVX2 where the CPU has it, else through the tables.
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    pub fn half_row(
        top_luma: &[u8],
        bottom_luma: &[u8],
        u_row: &[u8],
        v_row: &[u8],
        rgb_row: &mut [u8],
        tables: &RgbTables,
    ) {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the CPU has AVX2
            return unsafe { avx2::half_row(top_luma, bottom_luma, u_row, v_row, rgb_row, tables) };
        }
        table_row(top_luma, bottom_luma, u_row, v_row, rgb_row, tables)
    }

    /// A pixel at a time, through the tables.
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    pub fn table_row(
        top_luma: &[u8],
        bottom_luma: &[u8],
        u_row: &[u8],
        v_row: &[u8],
        rgb_row: &mut [u8],
        tables: &RgbTables,
    ) {
        for pair in 0..DST_W / 2 {
            let chroma_u = ((u_row[2 * pair] as u32 + u_row[2 * pair + 1] as u32 + 1) >> 1) as u8;
            let chroma_v = ((v_row[2 * pair] as u32 + v_row[2 * pair + 1] as u32 + 1) >> 1) as u8;
            for i in [2 * pair, 2 * pair + 1] {
                let sum = top_luma[2 * i] as u32
                    + top_luma[2 * i + 1] as u32
                    + bottom_luma[2 * i] as u32
                    + bottom_luma[2 * i + 1] as u32;
                tables.pixel(((sum + 2) >> 2) as u8, chroma_u, chroma_v, &mut rgb_row[i * 3..i * 3 + 3]);
            }
        }
    }

    /// The browser's 16 lanes twice over: AVX2 works on two 128-bit halves, so each half takes 16 of the 32 pixels
    /// and does what the WebAssembly code does, and only the loads and stores cross between the halves.
    #[cfg(target_arch = "x86_64")]
    pub mod avx2 {
        use super::{DST_W, RgbTables};
        use crate::convert::ChromaLine;
        use std::arch::x86_64::*;

        /// The byte shuffles that put one channel of 16 pixels at its places in one third of their 48 RGB bytes
        /// ([third][channel], the same in both halves; 0x80 gives a zero).
        const SPREAD: [[[u8; 32]; 3]; 3] = {
            let mut shuffles = [[[0x80u8; 32]; 3]; 3];
            let mut at = 0;
            while at < 48 {
                let (third, j) = (at / 16, at % 16);
                shuffles[third][at % 3][j] = (at / 3) as u8;
                shuffles[third][at % 3][j + 16] = (at / 3) as u8;
                at += 1;
            }
            shuffles
        };

        /// One row of the 2:1 conversion, 32 pixels at a time (`lanes::half_row` takes the same rows). Only for a CPU
        /// with AVX2.
        #[target_feature(enable = "avx2")]
        pub fn half_row(
            top_luma: &[u8],
            bottom_luma: &[u8],
            u_row: &[u8],
            v_row: &[u8],
            rgb_row: &mut [u8],
            tables: &RgbTables,
        ) {
            assert!(top_luma.len() >= 2 * DST_W && bottom_luma.len() >= 2 * DST_W);
            assert!(u_row.len() >= DST_W && v_row.len() >= DST_W);
            assert!(rgb_row.len() >= 3 * DST_W && DST_W.is_multiple_of(32));
            let formula = tables.formula;
            let splat = _mm256_set1_epi32;
            let (base, step, zero, max_level) = (splat(formula.y_base), splat(formula.y_step), splat(0), splat(255));
            let ones = _mm256_set1_epi8(1);
            // SAFETY: each load reads the 32 bytes its slice is cut to, each store writes the 32 bytes its slice is
            // cut to
            let load = |bytes: &[u8]| unsafe { _mm256_loadu_si256(bytes[..32].as_ptr().cast()) };
            let store = |bytes: &mut [u8], value: __m256i| unsafe {
                _mm256_storeu_si256(bytes[..32].as_mut_ptr().cast(), value)
            };
            let spread: [[__m256i; 3]; 3] = SPREAD.map(|third| third.map(|shuffle| load(&shuffle)));
            // a chroma table at 8 values
            let table = |(offset, slope): ChromaLine, chroma: __m256i| {
                _mm256_add_epi32(splat(offset), _mm256_srai_epi32::<16>(_mm256_mullo_epi32(chroma, splat(slope))))
            };
            // the y table at 8 indexes
            let level = |i: __m256i| {
                let unclamped = _mm256_srai_epi32::<16>(_mm256_add_epi32(base, _mm256_mullo_epi32(i, step)));
                _mm256_min_epi32(_mm256_max_epi32(unclamped, zero), max_level)
            };
            let channel_level = |offset: __m256i, luma: __m256i| level(_mm256_add_epi32(offset, luma));
            // pairs of bytes summed into 16 bits
            let pairs = |a: __m256i| _mm256_maddubs_epi16(a, ones);
            let mean4 = |a: __m256i, b: __m256i| {
                _mm256_srli_epi16::<2>(_mm256_add_epi16(_mm256_add_epi16(pairs(a), pairs(b)), _mm256_set1_epi16(2)))
            };
            let mean2 = |a: __m256i| _mm256_srli_epi16::<1>(_mm256_add_epi16(pairs(a), _mm256_set1_epi16(1)));
            for block in 0..DST_W / 32 {
                // pixels 0 to 15 of the 32 (8 in each half), then 16 to 31
                let first_means = mean4(load(&top_luma[64 * block..]), load(&bottom_luma[64 * block..]));
                let last_means = mean4(load(&top_luma[64 * block + 32..]), load(&bottom_luma[64 * block + 32..]));
                // each half's 16 pixels: its first 8 (0 to 7, 16 to 23), then its last 8 (8 to 15, 24 to 31)
                let luma_first = _mm256_permute2x128_si256::<0x20>(first_means, last_means);
                let luma_last = _mm256_permute2x128_si256::<0x31>(first_means, last_means);
                // the 16 pairs' chroma: 8 in each half
                let (u_means, v_means) = (mean2(load(&u_row[32 * block..])), mean2(load(&v_row[32 * block..])));
                let (u0, u1) = (_mm256_unpacklo_epi16(u_means, zero), _mm256_unpackhi_epi16(u_means, zero));
                let (v0, v1) = (_mm256_unpacklo_epi16(v_means, zero), _mm256_unpackhi_epi16(v_means, zero));
                // each pair's offsets, pairs 0 to 3 and 4 to 7 of each half
                let (r0, r1) = (table(formula.red_v, v0), table(formula.red_v, v1));
                let g0 = _mm256_add_epi32(table(formula.green_u, u0), table(formula.green_v, v0));
                let g1 = _mm256_add_epi32(table(formula.green_u, u1), table(formula.green_v, v1));
                let (b0, b1) = (table(formula.blue_u, u0), table(formula.blue_u, u1));
                // each half's 16 pixels' luma, 4 at a time
                let luma = [
                    _mm256_unpacklo_epi16(luma_first, zero),
                    _mm256_unpackhi_epi16(luma_first, zero),
                    _mm256_unpacklo_epi16(luma_last, zero),
                    _mm256_unpackhi_epi16(luma_last, zero),
                ];
                let channel = |p0: __m256i, p1: __m256i| {
                    let l0 = channel_level(_mm256_unpacklo_epi32(p0, p0), luma[0]);
                    let l1 = channel_level(_mm256_unpackhi_epi32(p0, p0), luma[1]);
                    let l2 = channel_level(_mm256_unpacklo_epi32(p1, p1), luma[2]);
                    let l3 = channel_level(_mm256_unpackhi_epi32(p1, p1), luma[3]);
                    _mm256_packus_epi16(_mm256_packs_epi32(l0, l1), _mm256_packs_epi32(l2, l3))
                };
                let rgb = [channel(r0, r1), channel(g0, g1), channel(b0, b1)];
                // each half's 48 RGB bytes, 16 at a time
                let out = spread.map(|shuffle| {
                    let red = _mm256_shuffle_epi8(rgb[0], shuffle[0]);
                    let green = _mm256_shuffle_epi8(rgb[1], shuffle[1]);
                    _mm256_or_si256(_mm256_or_si256(red, green), _mm256_shuffle_epi8(rgb[2], shuffle[2]))
                });
                // the first half's 48 bytes, then the second's
                store(&mut rgb_row[96 * block..], _mm256_permute2x128_si256::<0x20>(out[0], out[1]));
                store(&mut rgb_row[96 * block + 32..], _mm256_permute2x128_si256::<0x30>(out[2], out[0]));
                store(&mut rgb_row[96 * block + 64..], _mm256_permute2x128_si256::<0x31>(out[1], out[2]));
            }
        }
    }

    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    use crate::convert::ChromaLine;
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    use core::arch::wasm32::*;

    /// One row of the 2:1 conversion in the browser, with WebAssembly SIMD, 16 pixels at a time: two luma rows (2560
    /// samples each) and a U and a V row (1280 each) into a row of RGB24 (3840 bytes).
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    pub fn half_row(
        top_luma: &[u8],
        bottom_luma: &[u8],
        u_row: &[u8],
        v_row: &[u8],
        rgb_row: &mut [u8],
        tables: &RgbTables,
    ) {
        assert!(top_luma.len() >= 2 * DST_W && bottom_luma.len() >= 2 * DST_W);
        assert!(u_row.len() >= DST_W && v_row.len() >= DST_W);
        assert!(rgb_row.len() >= 3 * DST_W && DST_W.is_multiple_of(16));
        let formula = tables.formula;
        let splat = i32x4_splat;
        let (base, step, zero, max_level) = (splat(formula.y_base), splat(formula.y_step), splat(0), splat(255));
        // a chroma table at 4 values
        let table = |(offset, slope): ChromaLine, chroma: v128| {
            i32x4_add(splat(offset), i32x4_shr(i32x4_mul(chroma, splat(slope)), 16))
        };
        // the y table at 4 indexes
        let level = |i: v128| i32x4_min(i32x4_max(i32x4_shr(i32x4_add(base, i32x4_mul(i, step)), 16), zero), max_level);
        // 4 pixels' channel from their pairs' offsets (pairs p, p, p+1, p+1) and their luma
        let channel_level = |offset: v128, luma: v128| level(i32x4_add(offset, luma));
        for block in 0..DST_W / 16 {
            // SAFETY: the loads read 32 bytes at 32 * block of top_luma and bottom_luma and 16 bytes at 16 * block of
            // u_row and v_row, and the stores write 48 bytes at 48 * block of rgb_row, all within the lengths asserted
            // above; unaligned access is allowed
            unsafe {
                let load = |bytes: *const u8| v128_load(bytes as *const v128);
                let mean4 = |a: v128, b: v128| {
                    let sum = u16x8_add(u16x8_extadd_pairwise_u8x16(a), u16x8_extadd_pairwise_u8x16(b));
                    u16x8_shr(u16x8_add(sum, u16x8_splat(2)), 2)
                };
                let (top_at, bottom_at) = (top_luma.as_ptr().add(32 * block), bottom_luma.as_ptr().add(32 * block));
                let luma_first = mean4(load(top_at), load(bottom_at));
                let luma_last = mean4(load(top_at.add(16)), load(bottom_at.add(16)));
                let mean2 = |a: v128| u16x8_shr(u16x8_add(u16x8_extadd_pairwise_u8x16(a), u16x8_splat(1)), 1);
                let u_means = mean2(load(u_row.as_ptr().add(16 * block)));
                let v_means = mean2(load(v_row.as_ptr().add(16 * block)));
                let (u0, u1) = (u32x4_extend_low_u16x8(u_means), u32x4_extend_high_u16x8(u_means));
                let (v0, v1) = (u32x4_extend_low_u16x8(v_means), u32x4_extend_high_u16x8(v_means));
                // each pair's offsets, pairs 0 to 3 and 4 to 7
                let (r0, r1) = (table(formula.red_v, v0), table(formula.red_v, v1));
                let g0 = i32x4_add(table(formula.green_u, u0), table(formula.green_v, v0));
                let g1 = i32x4_add(table(formula.green_u, u1), table(formula.green_v, v1));
                let (b0, b1) = (table(formula.blue_u, u0), table(formula.blue_u, u1));
                // the 16 pixels' luma, 4 at a time
                let luma = [
                    u32x4_extend_low_u16x8(luma_first),
                    u32x4_extend_high_u16x8(luma_first),
                    u32x4_extend_low_u16x8(luma_last),
                    u32x4_extend_high_u16x8(luma_last),
                ];
                let channel = |p0: v128, p1: v128| {
                    let l0 = channel_level(i32x4_shuffle::<0, 0, 1, 1>(p0, p0), luma[0]);
                    let l1 = channel_level(i32x4_shuffle::<2, 2, 3, 3>(p0, p0), luma[1]);
                    let l2 = channel_level(i32x4_shuffle::<0, 0, 1, 1>(p1, p1), luma[2]);
                    let l3 = channel_level(i32x4_shuffle::<2, 2, 3, 3>(p1, p1), luma[3]);
                    u8x16_narrow_i16x8(i16x8_narrow_i32x4(l0, l1), i16x8_narrow_i32x4(l2, l3))
                };
                let (red, green, blue) = (channel(r0, r1), channel(g0, g1), channel(b0, b1));
                // R, G and B interleaved: 48 bytes, R G and B of pixel 0, then of pixel 1, and so on
                let rg0 = i8x16_shuffle::<0, 16, 0, 1, 17, 0, 2, 18, 0, 3, 19, 0, 4, 20, 0, 5>(red, green);
                let out0 = i8x16_shuffle::<0, 1, 16, 3, 4, 17, 6, 7, 18, 9, 10, 19, 12, 13, 20, 15>(rg0, blue);
                let rg1 = i8x16_shuffle::<21, 0, 6, 22, 0, 7, 23, 0, 8, 24, 0, 9, 25, 0, 10, 26>(red, green);
                let out1 = i8x16_shuffle::<0, 21, 2, 3, 22, 5, 6, 23, 8, 9, 24, 11, 12, 25, 14, 15>(rg1, blue);
                let rg2 = i8x16_shuffle::<0, 11, 27, 0, 12, 28, 0, 13, 29, 0, 14, 30, 0, 15, 31, 0>(red, green);
                let out2 = i8x16_shuffle::<26, 1, 2, 27, 4, 5, 28, 7, 8, 29, 10, 11, 30, 13, 14, 31>(rg2, blue);
                let dst = rgb_row.as_mut_ptr().add(48 * block) as *mut v128;
                v128_store(dst, out0);
                v128_store(dst.add(1), out1);
                v128_store(dst.add(2), out2);
            }
        }
    }
}

/// The rounded mean of each 2 x 2 block of a plane (`plane`: rows of `width` samples) into `out` (`out_width` x
/// `out_height`, half the plane's size).
fn mean_2x2(plane: &[u8], width: usize, out: &mut [u8], out_width: usize, out_height: usize) {
    for row in 0..out_height {
        let top = &plane[2 * row * width..2 * row * width + 2 * out_width];
        let bottom = &plane[(2 * row + 1) * width..(2 * row + 1) * width + 2 * out_width];
        let out_row = &mut out[row * out_width..(row + 1) * out_width];
        for ((mean, a), b) in out_row.iter_mut().zip(top.chunks_exact(2)).zip(bottom.chunks_exact(2)) {
            *mean = ((a[0] as u32 + a[1] as u32 + b[0] as u32 + b[1] as u32 + 2) >> 2) as u8;
        }
    }
}

/// Checks the 2:1 rows' formulas and AVX2 code against the tables.
#[cfg(test)]
mod tests {
    use super::*;

    /// Every matrix, for the checks that cover them all.
    const MATRICES: [Matrix; 5] = [Matrix::Bt709, Matrix::Bt601, Matrix::Fcc, Matrix::Smpte240m, Matrix::Bt2020];

    /// The formulas the 2:1 rows with SIMD compute must give every table entry, for every matrix and range.
    #[test]
    fn formulas_give_the_tables() {
        for matrix in MATRICES {
            for full_range in [false, true] {
                let tables = RgbTables::new(matrix, full_range);
                let formula = tables.formula;
                for i in 0..Y_TABLE_LEN as i32 {
                    let level = ((formula.y_base + i * formula.y_step) >> 16).clamp(0, 255) as u8;
                    assert_eq!(level, tables.y_table[i as usize], "y[{i}]");
                }
                let line = |(offset, slope): ChromaLine, chroma: i32| offset + ((chroma * slope) >> 16);
                for chroma in 0..256usize {
                    let value = chroma as i32;
                    assert_eq!(line(formula.red_v, value) as i64, tables.red_v[chroma]);
                    assert_eq!(line(formula.green_u, value) as i64, tables.green_u[chroma]);
                    assert_eq!(line(formula.blue_u, value) as i64, tables.blue_u[chroma]);
                    assert_eq!(line(formula.green_v, value) as i64, tables.green_v[chroma]);
                }
            }
        }
    }

    /// The AVX2 rows must give the tables' bytes: for every matrix and range, every luma, U and V mean at every place
    /// in a row (rows of 2 x 2 blocks of one value), and rows of noise for the rounding of the means.
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_rows_give_the_tables() {
        if !std::arch::is_x86_feature_detected!("avx2") {
            eprintln!("no AVX2 on this CPU");
            return;
        }
        let (mut top, mut bottom) = (vec![0u8; 2 * DST_W], vec![0u8; 2 * DST_W]);
        let (mut u_row, mut v_row) = (vec![0u8; DST_W], vec![0u8; DST_W]);
        let (mut got, mut want) = (vec![0u8; 3 * DST_W], vec![0u8; 3 * DST_W]);
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 24) as u8
        };
        for matrix in MATRICES {
            for full_range in [false, true] {
                let tables = RgbTables::new(matrix, full_range);
                let mut check = |top: &[u8], bottom: &[u8], u_row: &[u8], v_row: &[u8], what: &str| {
                    lanes::table_row(top, bottom, u_row, v_row, &mut want, &tables);
                    // SAFETY: the CPU has AVX2
                    unsafe { lanes::avx2::half_row(top, bottom, u_row, v_row, &mut got, &tables) };
                    let at = got.iter().zip(&want).position(|(a, b)| a != b);
                    assert!(at.is_none(), "{matrix:?} full={full_range} {what}: byte {at:?} differs");
                };
                // pair p: U = p % 256, V = (v_start + 3p) % 256; pixel i: luma (2 * step + i % 2 + 7p) % 256. Over
                // v_start and step every U meets every V and every luma.
                for v_start in 0..256 {
                    for pair in 0..DST_W / 2 {
                        u_row[2 * pair..2 * pair + 2].fill((pair % 256) as u8);
                        v_row[2 * pair..2 * pair + 2].fill(((v_start + 3 * pair) % 256) as u8);
                    }
                    for step in 0..128 {
                        for i in 0..DST_W {
                            let luma = ((2 * step + i % 2 + 7 * (i / 2)) % 256) as u8;
                            top[2 * i..2 * i + 2].fill(luma);
                            bottom[2 * i..2 * i + 2].fill(luma);
                        }
                        check(&top, &bottom, &u_row, &v_row, &format!("v={v_start} s={step}"));
                    }
                }
                for round in 0..2000 {
                    let all = top.iter_mut().chain(bottom.iter_mut()).chain(u_row.iter_mut()).chain(v_row.iter_mut());
                    for byte in all {
                        *byte = noise();
                    }
                    check(&top, &bottom, &u_row, &v_row, &format!("noise {round}"));
                }
            }
        }
    }
}
