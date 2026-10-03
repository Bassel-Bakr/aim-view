//! A decoded frame (YUV 4:2:0, any size) as ffmpeg 8.1 gives it to the review: `scale=1280:720:flags=area` to
//! `rgb24` (the detector's input) or `yuv420p` (the fixed map's), byte for byte as the ffmpeg CLI does on x86-64
//! (python/review.py: `rgb_frames`, `_frames`). The model was trained on these exact bytes: GPU colour conversion once
//! changed detections at the crosshair. Every step mirrors libswscale's integer code (utils.c `initFilter`,
//! hscale.c, output.c, yuv2rgb.c, and the x86 kernels where ffmpeg uses them).
//!
//! Things vf_scale does in 8.1 that this depends on: the flags are exactly SWS_AREA; the matrix and range come from
//! the frame's tags; chroma is taken as center-sited whatever the tag says (vf_scale overwrites it with its
//! `in_chroma_loc` option, "unspecified" by default).

/// The colour matrices ffmpeg knows (AVColorSpace), as ff_yuv2rgb_coeffs gives them: {crv, cbu, cgu, cgv}.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Matrix {
    Bt709,
    Bt601,
    Fcc,
    Smpte240m,
    Bt2020,
}

impl Matrix {
    fn coeffs(self) -> [i64; 4] {
        match self {
            Matrix::Bt709 => [117489, 138438, 13975, 34925],
            Matrix::Bt601 => [104597, 132201, 25675, 53279],
            Matrix::Fcc => [104448, 132798, 24759, 53109],
            Matrix::Smpte240m => [117579, 136230, 16907, 35559],
            Matrix::Bt2020 => [110013, 140363, 12277, 42626],
        }
    }
}

/// The size the review works in.
pub const DST_W: usize = 1280;
pub const DST_H: usize = 720;

/// C99 integer division: truncates toward zero.
fn cdiv(a: i64, b: i64) -> i64 {
    a / b
}

/// libavutil's ROUNDED_DIV.
fn rounded_div(a: i64, b: i64) -> i64 {
    if a >= 0 { (a + (b >> 1)) / b } else { (a - (b >> 1)) / b }
}

fn av_log2(v: i64) -> i64 {
    if v > 0 { 63 - v.leading_zeros() as i64 } else { 0 }
}

/// The 16.16 step from source to destination (utils.c).
fn xinc(src: usize, dst: usize) -> i64 {
    (((src as i64) << 16) + (dst as i64 >> 1)) / dst as i64
}

/// A chroma position in 1/256 pixel of the subsampled plane (get_local_pos); -513 means unspecified (center).
fn local_pos(chr_subsample: i64, pos: i64) -> i64 {
    let pos = if pos == -1 || pos <= -513 { (128 << chr_subsample) - 128 } else { pos };
    (pos + 128) >> chr_subsample
}

/// One direction's filter: per output sample, the first input sample and the coefficients (`size` of them). Worked
/// out once for a frame size: each output's taps (`taps[tap_at[i]..tap_at[i + 1]]`), the input sample clamped to the
/// line as ffmpeg reads it and the zero coefficients left out, and each tap's input clamped (`at`).
#[derive(Clone, Debug)]
struct Filter {
    pos: Vec<i64>,
    coef: Vec<i64>,
    size: usize,
    taps: Vec<(u32, i32)>,
    tap_at: Vec<u32>,
    /// The input samples in the line.
    src: usize,
}

impl Filter {
    fn new(pos: Vec<i64>, coef: Vec<i64>, size: usize, src: usize) -> Filter {
        // the coefficients are shares of one (1 << 14 at most), a few to a sample: 15-bit samples and their sums fit in
        // 32 bits
        assert!(size < 16 && coef.iter().all(|c| c.abs() <= 1 << 14), "a filter too large for 32-bit sums");
        let mut taps = Vec::new();
        let mut tap_at = vec![0];
        for (i, &p) in pos.iter().enumerate() {
            for (j, &c) in coef[i * size..(i + 1) * size].iter().enumerate() {
                if c != 0 {
                    taps.push((((p + j as i64) as usize).min(src - 1) as u32, c as i32));
                }
            }
            tap_at.push(taps.len() as u32);
        }
        Filter { pos, coef, size, taps, tap_at, src }
    }

    fn row(&self, i: usize) -> &[i64] {
        &self.coef[i * self.size..(i + 1) * self.size]
    }

    /// Output `i` of a line through its clamped taps, as a 15-bit sample.
    fn tap_sum(&self, line: &[u8], i: usize) -> i32 {
        let taps = &self.taps[self.tap_at[i] as usize..self.tap_at[i + 1] as usize];
        let acc: i32 = taps.iter().map(|&(at, c)| line[at as usize] as i32 * c).sum();
        (acc >> 7).min(32767)
    }

    /// Where output `i`'s tap `j` reads, clamped to the line.
    fn at(&self, i: usize, j: usize) -> usize {
        ((self.pos[i] + j as i64) as usize).min(self.src - 1)
    }
}

/// initFilter() for SWS_AREA with no user filter vectors. Kept close to the C, loop counters and all, so the two can be
/// read side by side. `one` is 1 << 14 (horizontal) or 1 << 12 (vertical);
/// `align` 4 (horizontal) or 2 (vertical) on x86 SIMD builds, 1 for the plain C code.
#[allow(clippy::too_many_arguments, clippy::explicit_counter_loop)]
fn init_filter(x_inc: i64, src_w: usize, dst_w: usize, align: usize, one: i64, src_pos: i64, dst_pos: i64, x86: bool) -> Filter {
    let (sw, dw) = (src_w as i64, dst_w as i64);
    let fone: i64 = 1 << (54 - av_log2(sw / dw).min(8));
    let mut pos = vec![0i64; dst_w];
    let mut filt: Vec<Vec<i64>> = Vec::with_capacity(dst_w);
    let mut size: usize;
    if (x_inc - 0x10000).abs() < 10 && src_pos == dst_pos {
        size = 1;
        for (i, p) in pos.iter_mut().enumerate() {
            *p = i as i64;
            filt.push(vec![fone]);
        }
    } else if x_inc <= 1 << 16 {
        // SWS_AREA when upscaling is bilinear
        size = 2;
        let mut x = ((dst_pos * x_inc) >> 8) - ((src_pos * 0x8000) >> 7);
        for p in pos.iter_mut() {
            let mut xx = (x - (((size as i64) - 1) << 15) + (1 << 15)) >> 16;
            *p = xx;
            let mut row = Vec::with_capacity(size);
            for _ in 0..size {
                row.push((fone - (xx * (1 << 16) - x).abs() * (fone >> 16)).max(0));
                xx += 1;
            }
            filt.push(row);
            x += x_inc;
        }
    } else {
        // SWS_AREA downscale
        size = (1 + (sw + dw - 1) / dw) as usize;
        size = size.min(src_w.saturating_sub(2)).max(1);
        let mut x = ((dst_pos * x_inc) >> 7) - ((src_pos * 0x10000) >> 7);
        for p in pos.iter_mut() {
            let mut xx = cdiv(x - (size as i64 - 2) * (1 << 16), 1 << 17);
            *p = xx;
            let mut row = Vec::with_capacity(size);
            for _ in 0..size {
                let d = (((xx * (1 << 17) - x).abs()) << 13) * dw / sw;
                let d2 = d - (1 << 29);
                let coeff = if d2 * x_inc < -(1 << 45) {
                    1i64 << 46
                } else if d2 * x_inc < (1 << 45) {
                    -d2 * x_inc + (1 << 45)
                } else {
                    0
                };
                row.push(coeff * (fone >> 46));
                xx += 1;
            }
            filt.push(row);
            x += 2 * x_inc;
        }
    }
    let f2 = size;
    // drop near-zero taps: from the left by moving the position, from the right by counting
    let cut_limit = 0.002 * fone as f64;
    let mut min_size = 0;
    for i in (0..dst_w).rev() {
        let mut mn = f2;
        let mut cut = 0i64;
        for _ in 0..f2 {
            cut += filt[i][0].abs();
            if cut as f64 > cut_limit {
                break;
            }
            if i < dst_w - 1 && pos[i] >= pos[i + 1] {
                break;
            }
            filt[i].remove(0);
            filt[i].push(0);
            pos[i] += 1;
        }
        let mut cut = 0i64;
        for j in (1..f2).rev() {
            cut += filt[i][j].abs();
            if cut as f64 > cut_limit {
                break;
            }
            mn -= 1;
        }
        min_size = min_size.max(mn);
    }
    let align = if x86 && min_size == 1 && align == 2 { 1 } else { align };
    let size = (min_size + align - 1) & !(align - 1);
    let mut filt: Vec<Vec<i64>> =
        filt.into_iter().map(|r| (0..size).map(|j| if j < f2 { r[j] } else { 0 }).collect()).collect();
    // fix the borders
    for i in 0..dst_w {
        let f = &mut filt[i];
        if pos[i] < 0 {
            for j in 1..size {
                let left = (j as i64 + pos[i]).max(0) as usize;
                f[left] += f[j];
                f[j] = 0;
            }
            pos[i] = 0;
        }
        if pos[i] + size as i64 > sw {
            let shift = (pos[i] + (size as i64 - sw).min(0)) as usize;
            let mut acc = 0;
            for j in (0..size).rev() {
                if pos[i] + j as i64 >= sw {
                    acc += f[j];
                    f[j] = 0;
                }
            }
            for j in (0..size).rev() {
                f[j] = if j < shift { 0 } else { f[j - shift] };
            }
            pos[i] -= shift as i64;
            f[(sw - 1 - pos[i]) as usize] += acc;
        }
    }
    // normalize to `one`, carrying the rounding error along
    let mut coef = vec![0i64; dst_w * size];
    for i in 0..dst_w {
        let sum: i64 = filt[i].iter().sum();
        let s = ((sum + one / 2) / one).max(1);
        let mut err = 0;
        for j in 0..size {
            let v = filt[i][j] + err;
            let iv = rounded_div(v, s);
            coef[i * size + j] = iv;
            err = v - iv * s;
        }
    }
    Filter::new(pos, coef, size, src_w)
}

/// hScale8To15_c: one plane's rows, 8-bit samples to 15-bit, into `out` (kept from frame to frame).
fn hscale(src: &[u8], w: usize, h: usize, f: &Filter, out: &mut Vec<i32>) {
    out.resize(h * f.pos.len(), 0);
    match f.size {
        1 => hscale_n::<1>(src, w, h, f, out),
        2 => hscale_n::<2>(src, w, h, f, out),
        3 => hscale_n::<3>(src, w, h, f, out),
        4 => hscale_n::<4>(src, w, h, f, out),
        _ => {
            for (row, o) in src.chunks_exact(w).take(h).zip(out.chunks_exact_mut(f.pos.len())) {
                for (i, o) in o.iter_mut().enumerate() {
                    *o = f.tap_sum(row, i);
                }
            }
        }
    }
}

/// `hscale` for a filter of N coefficients: an output whose N samples all lie in the line reads them at once (the
/// same sum, in an order the compiler can run side by side), one at the edge through its clamped taps.
fn hscale_n<const N: usize>(src: &[u8], w: usize, h: usize, f: &Filter, out: &mut [i32]) {
    let coefs: Vec<[i32; N]> =
        f.coef.chunks_exact(N).map(|c| std::array::from_fn(|j| c[j] as i32)).collect();
    for (row, o) in src.chunks_exact(w).take(h).zip(out.chunks_exact_mut(f.pos.len())) {
        for (i, (o, c)) in o.iter_mut().zip(&coefs).enumerate() {
            let p = f.pos[i];
            *o = if p >= 0 && p as usize + N <= w {
                let x: &[u8; N] = row[p as usize..p as usize + N].try_into().unwrap();
                let acc: i32 = (0..N).map(|j| x[j] as i32 * c[j]).sum();
                (acc >> 7).min(32767)
            } else {
                f.tap_sum(row, i)
            };
        }
    }
}

/// The rounded 8-bit sample of a 19-bit sum.
fn to_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// A vertical filter's rows for output row `r` (`taps`: its input rows, clamped, with their coefficients, zeros left
/// out) summed into `acc` (`width` samples, each started at `start`), from the horizontally scaled plane `hs`.
fn vsum(hs: &[i32], width: usize, f: &Filter, r: usize, start: i32, acc: &mut [i32]) {
    acc.fill(start);
    for &(at, c) in &f.taps[f.tap_at[r] as usize..f.tap_at[r + 1] as usize] {
        let line = &hs[at as usize * width..][..width];
        for (a, &v) in acc.iter_mut().zip(line) {
            *a += v * c;
        }
    }
}

/// ff_yuv2rgb_c_init_tables() for 24 bits, no brightness, contrast and saturation 1: for 8-bit Y, U, V,
/// R = y[rv[V] + Y], G = y[gu[U] + gv[V] + Y], B = y[bu[U] + Y].
struct RgbTables {
    y: Vec<u8>,
    rv: [i64; 256],
    gu: [i64; 256],
    bu: [i64; 256],
    gv: [i64; 256],
    /// The same tables as formulas, for code that computes them (the 2:1 rows with SIMD).
    #[cfg_attr(not(any(target_arch = "x86_64", all(target_arch = "wasm32", target_feature = "simd128"))), allow(dead_code))]
    formula: RgbFormula,
}

/// RgbTables as formulas: y[i] = ((y_base + i * y_step) >> 16) clamped to 0..255, and each chroma table
/// t[c] = a + ((c * k) >> 16) (`tab` in RgbTables::new), with the green one the sum of gu and gv. All in i32: every value
/// they reach fits.
#[derive(Clone, Copy)]
#[cfg_attr(not(any(target_arch = "x86_64", all(target_arch = "wasm32", target_feature = "simd128"))), allow(dead_code))]
struct RgbFormula {
    y_base: i32,
    y_step: i32,
    rv: (i32, i32),
    gu: (i32, i32),
    bu: (i32, i32),
    gv: (i32, i32),
}

impl RgbTables {
    fn new(matrix: Matrix, full: bool) -> RgbTables {
        let inv = matrix.coeffs();
        let (mut crv, mut cbu, mut cgu, mut cgv) = (inv[0], inv[1], -inv[2], -inv[3]);
        let (mut cy, mut oy) = (1i64 << 16, 0i64);
        if !full {
            cy = cdiv(cy * 255, 219);
            oy = 16 << 16;
        } else {
            (crv, cbu, cgu, cgv) = (cdiv(crv * 224, 255), cdiv(cbu * 224, 255), cdiv(cgu * 224, 255), cdiv(cgv * 224, 255));
        }
        let scale = |k: i64| cdiv(k * 65536 + 0x8000, cy.max(1));
        let (crv, cbu, cgu, cgv) = (scale(crv), scale(cbu), scale(cgu), scale(cgv));
        let yoffs = if full { 384 } else { 326 } + 512;
        let y_base = -(384i64 << 16) - 512 * cy - oy + 0x8000;
        let y = (0..2048i64).map(|i| ((y_base + i * cy) >> 16).clamp(0, 255) as u8).collect();
        let tab = |k: i64, off: i64| {
            let mut t = [0i64; 256];
            for (c, v) in t.iter_mut().enumerate() {
                *v = off - (k >> 9) + ((c as i64 * k) >> 16);
            }
            t
        };
        let line = |k: i64, off: i64| ((off - (k >> 9)) as i32, k as i32);
        RgbTables {
            y,
            rv: tab(crv, yoffs),
            gu: tab(cgu, yoffs),
            bu: tab(cbu, yoffs),
            gv: tab(cgv, 0),
            formula: RgbFormula {
                y_base: y_base as i32,
                y_step: cy as i32,
                rv: line(crv, yoffs),
                gu: line(cgu, yoffs),
                bu: line(cbu, yoffs),
                gv: line(cgv, 0),
            },
        }
    }

    fn rgb(&self, y: u8, u: u8, v: u8, out: &mut [u8]) {
        let y = y as i64;
        out[0] = self.y[(self.rv[v as usize] + y) as usize];
        out[1] = self.y[(self.gu[u as usize] + self.gv[v as usize] + y) as usize];
        out[2] = self.y[(self.bu[u as usize] + y) as usize];
    }
}

/// The 16-bit constants the x86 yuv2rgb kernel (ff_yuv_420_rgb24_ssse3) uses, as ff_yuv2rgb_c_init_tables stores
/// them (roundToInt16).
struct SimdCoeffs {
    y: i64,
    vr: i64,
    ub: i64,
    vg: i64,
    ug: i64,
    y_off: i64,
}

impl SimdCoeffs {
    fn new(matrix: Matrix, full: bool) -> SimdCoeffs {
        let r16 = |f: i64| {
            let r = (f + (1 << 15)) >> 16;
            if r < -0x7fff { -0x8000 } else { r.min(0x7fff) }
        };
        let inv = matrix.coeffs();
        let (mut crv, mut cbu, mut cgu, mut cgv) = (inv[0], inv[1], -inv[2], -inv[3]);
        let (mut cy, mut oy) = (1i64 << 16, 0i64);
        if !full {
            cy = cdiv(cy * 255, 219);
            oy = 16 << 16;
        } else {
            (crv, cbu, cgu, cgv) = (cdiv(crv * 224, 255), cdiv(cbu * 224, 255), cdiv(cgu * 224, 255), cdiv(cgv * 224, 255));
        }
        SimdCoeffs { y: r16(cy << 13), vr: r16(crv << 13), ub: r16(cbu << 13), vg: r16(cgv << 13), ug: r16(cgu << 13), y_off: r16(oy << 3) }
    }
}

fn sat16(a: i64) -> i64 {
    a.clamp(-32768, 32767)
}

fn wrap16(a: i64) -> i64 {
    ((a + 32768) & 0xffff) - 32768
}

/// The four filters swscale builds for a frame size: luma and chroma, horizontal and vertical.
struct Filters {
    lh: Filter,
    lv: Filter,
    ch: Filter,
    cv: Filter,
}

fn filters(w: usize, h: usize, rgb_out: bool, x86: bool) -> Filters {
    let (cw, chh) = (w.div_ceil(2), h.div_ceil(2));
    let src_c = (local_pos(1, 128), local_pos(1, 128)); // center, whatever the tag
    let (cdw, cdh, dst_c) = if rgb_out {
        // packed RGB keeps chroma at half width and full height
        (DST_W.div_ceil(2), DST_H, (local_pos(1, -513), local_pos(0, -513)))
    } else {
        (DST_W.div_ceil(2), DST_H.div_ceil(2), src_c)
    };
    let (ha, va) = if x86 { (4, 2) } else { (1, 1) };
    Filters {
        lh: init_filter(xinc(w, DST_W), w, DST_W, ha, 1 << 14, 128, 128, x86),
        lv: init_filter(xinc(h, DST_H), h, DST_H, va, 1 << 12, 128, 128, x86),
        ch: init_filter(xinc(cw, cdw), cw, cdw, ha, 1 << 14, src_c.0, dst_c.0, x86),
        cv: init_filter(xinc(chh, cdh), chh, cdh, va, 1 << 12, src_c.1, dst_c.1, x86),
    }
}

/// Converts a recording's frames: built once per recording (its size and colour tags), then one call per frame.
pub struct Converter {
    w: usize,
    h: usize,
    tables: RgbTables,
    simd: SimdCoeffs,
    /// x86 kernels where ffmpeg uses them (its default); false: the plain C code (`-cpuflags 0`).
    x86: bool,
    /// The 2:1 shortcut is allowed (it gives the same bytes; tests turn it off to compare).
    shortcut: bool,
    rgb_filters: Option<Filters>,
    yuv_filters: Option<Filters>,
    scratch: Scratch,
}

impl Converter {
    /// For frames of w x h, with the recording's colour matrix and range (full: "pc", JPEG range).
    pub fn new(w: usize, h: usize, matrix: Matrix, full: bool) -> Converter {
        Converter::with_kernels(w, h, matrix, full, true)
    }

    /// As `new`, choosing ffmpeg's x86 kernels (true, its default) or its plain C code (false, `-cpuflags 0`).
    pub fn with_kernels(w: usize, h: usize, matrix: Matrix, full: bool, x86: bool) -> Converter {
        Converter {
            w,
            h,
            tables: RgbTables::new(matrix, full),
            simd: SimdCoeffs::new(matrix, full),
            x86,
            shortcut: true,
            rgb_filters: None,
            yuv_filters: None,
            scratch: Scratch::default(),
        }
    }

    /// As `with_kernels`, always through ffmpeg's full pipeline: no 2:1 shortcut. For checking the shortcut.
    pub fn without_shortcut(w: usize, h: usize, matrix: Matrix, full: bool, x86: bool) -> Converter {
        Converter { shortcut: false, ..Converter::with_kernels(w, h, matrix, full, x86) }
    }

    fn planes<'a>(&self, yuv: &'a [u8]) -> (&'a [u8], &'a [u8], &'a [u8]) {
        let (cw, ch) = (self.w.div_ceil(2), self.h.div_ceil(2));
        let (y, rest) = yuv.split_at(self.w * self.h);
        let (u, rest) = rest.split_at(cw * ch);
        (y, u, &rest[..cw * ch])
    }

    fn exact_half(&self) -> bool {
        self.shortcut && self.w == 2 * DST_W && self.h == 2 * DST_H
    }

    /// The frame as RGB24 at 1280 x 720 (row by row, R, G, B), into `out` (DST_W * DST_H * 3 bytes).
    pub fn rgb24(&mut self, yuv: &[u8], out: &mut [u8]) {
        let (y, u, v) = self.planes(yuv);
        if self.w == DST_W && self.h == DST_H {
            return self.unscaled_rgb24(y, u, v, out);
        }
        if self.exact_half() {
            return self.half_rgb24(y, u, v, out);
        }
        let f = self.rgb_filters.get_or_insert_with(|| filters(self.w, self.h, true, self.x86));
        let s = &mut self.scratch;
        let (cw, ch) = (self.w.div_ceil(2), self.h.div_ceil(2));
        hscale(y, self.w, self.h, &f.lh, &mut s.y);
        hscale(u, cw, ch, &f.ch, &mut s.u);
        hscale(v, cw, ch, &f.ch, &mut s.v);
        let cdw = f.ch.pos.len();
        let (lfs, cfs) = (f.lv.size, f.cv.size);
        s.ys.resize(DST_W, 0);
        s.us.resize(cdw, 0);
        s.vs.resize(cdw, 0);
        s.acc.resize(DST_W.max(cdw), 0);
        // the vertical pass: swscale picks its kernel per row (yuv2rgb24_1/2/X), and so does this, then runs it over
        // the row; the same integer arithmetic, so the same bytes
        for r in 0..DST_H {
            let (l, c) = (f.lv.row(r), f.cv.row(r));
            let two_c = cfs == 2 && c[0] + c[1] == 4096 && (0..=4096).contains(&c[1]);
            let lrow = |j: usize| &s.y[f.lv.at(r, j) * DST_W..][..DST_W];
            let crow = |j: usize| f.cv.at(r, j) * cdw;
            let two_l = lfs == 2 && two_c && l[0] + l[1] == 4096 && (0..=4096).contains(&l[1]);
            if lfs == 1 && (cfs == 1 || two_c) {
                for (o, &a) in s.ys.iter_mut().zip(lrow(0)) {
                    *o = to_u8((a + 64) >> 7);
                }
                let a = if cfs == 2 { c[1] as i32 } else { 0 };
                for (p, out) in [(&s.u, &mut s.us), (&s.v, &mut s.vs)] {
                    if a == 0 {
                        for (o, &x) in out.iter_mut().zip(&p[crow(0)..][..cdw]) {
                            *o = to_u8((x + 64) >> 7);
                        }
                    } else {
                        for ((o, &x0), &x1) in out.iter_mut().zip(&p[crow(0)..][..cdw]).zip(&p[crow(1)..][..cdw]) {
                            *o = to_u8((x0 * (4096 - a) + x1 * a + (128 << 11)) >> 19);
                        }
                    }
                }
            } else if two_l {
                let b = l[1] as i32;
                for ((o, &a0), &a1) in s.ys.iter_mut().zip(lrow(0)).zip(lrow(1)) {
                    *o = to_u8((a0 * (4096 - b) + a1 * b) >> 19);
                }
                let b = c[1] as i32;
                for (p, out) in [(&s.u, &mut s.us), (&s.v, &mut s.vs)] {
                    for ((o, &x0), &x1) in out.iter_mut().zip(&p[crow(0)..][..cdw]).zip(&p[crow(1)..][..cdw]) {
                        *o = to_u8((x0 * (4096 - b) + x1 * b) >> 19);
                    }
                }
            } else {
                vsum(&s.y, DST_W, &f.lv, r, 1 << 18, &mut s.acc[..DST_W]);
                for (o, &a) in s.ys.iter_mut().zip(&s.acc) {
                    *o = to_u8(a >> 19);
                }
                for (p, out) in [(&s.u, &mut s.us), (&s.v, &mut s.vs)] {
                    vsum(p, cdw, &f.cv, r, 1 << 18, &mut s.acc[..cdw]);
                    for (o, &a) in out.iter_mut().zip(&s.acc) {
                        *o = to_u8(a >> 19);
                    }
                }
            }
            let row = &mut out[r * DST_W * 3..(r + 1) * DST_W * 3];
            for (i, px) in row.chunks_exact_mut(3).enumerate() {
                self.tables.rgb(s.ys[i], s.us[i / 2], s.vs[i / 2], px);
            }
        }
    }

    /// Exactly 2:1 (2560 x 1440): the filters reduce to plain rounded means, the same bytes much faster. A row at a
    /// time (lanes::half_row).
    fn half_rgb24(&self, y: &[u8], u: &[u8], v: &[u8], out: &mut [u8]) {
        let (w, cw) = (self.w, self.w / 2);
        for r in 0..DST_H {
            let (y0, y1) = (&y[2 * r * w..(2 * r + 1) * w], &y[(2 * r + 1) * w..(2 * r + 2) * w]);
            let (ur, vr) = (&u[r * cw..(r + 1) * cw], &v[r * cw..(r + 1) * cw]);
            lanes::half_row(y0, y1, ur, vr, &mut out[r * DST_W * 3..(r + 1) * DST_W * 3], &self.tables);
        }
    }

    /// The same size: swscale's special converter (x86: ff_yuv_420_rgb24_ssse3; C: the tables).
    fn unscaled_rgb24(&self, y: &[u8], u: &[u8], v: &[u8], out: &mut [u8]) {
        let cw = self.w.div_ceil(2);
        let k = &self.simd;
        for r in 0..DST_H {
            for i in 0..DST_W {
                let (yy, uu, vv) = (y[r * self.w + i], u[(r / 2) * cw + i / 2], v[(r / 2) * cw + i / 2]);
                let px = &mut out[(r * DST_W + i) * 3..(r * DST_W + i) * 3 + 3];
                if !self.x86 {
                    self.tables.rgb(yy, uu, vv, px);
                    continue;
                }
                let uq = sat16(uu as i64 * 8 - 0x400);
                let vq = sat16(vv as i64 * 8 - 0x400);
                let yv = (wrap16(yy as i64 * 8 - k.y_off) * k.y) >> 16;
                let g = sat16(((uq * k.ug) >> 16) + ((vq * k.vg) >> 16));
                px[0] = sat16(yv + ((vq * k.vr) >> 16)).clamp(0, 255) as u8;
                px[1] = sat16(yv + g).clamp(0, 255) as u8;
                px[2] = sat16(yv + ((uq * k.ub) >> 16)).clamp(0, 255) as u8;
            }
        }
    }

    /// The frame as YUV 4:2:0 at 1280 x 720 (Y, then U, then V), into `out` (DST_W * DST_H * 3 / 2 bytes). The range
    /// and matrix stay as they are.
    pub fn yuv420p(&mut self, yuv: &[u8], out: &mut [u8]) {
        let (cw, ch) = (self.w.div_ceil(2), self.h.div_ceil(2));
        if self.w == DST_W && self.h == DST_H {
            out.copy_from_slice(&yuv[..DST_W * DST_H + 2 * cw * ch]);
            return;
        }
        let (y, u, v) = self.planes(yuv);
        let (ocw, och) = (DST_W / 2, DST_H / 2);
        let (oy, rest) = out.split_at_mut(DST_W * DST_H);
        let (ou, ov) = rest.split_at_mut(ocw * och);
        if self.exact_half() {
            mean2x2(y, self.w, oy, DST_W, DST_H);
            mean2x2(u, cw, ou, ocw, och);
            mean2x2(v, cw, ov, ocw, och);
            return;
        }
        let f = self.yuv_filters.get_or_insert_with(|| filters(self.w, self.h, false, self.x86));
        let s = &mut self.scratch;
        scale_plane(self.x86, s, y, self.w, self.h, &f.lh, &f.lv, oy, DST_W, DST_H, true);
        scale_plane(self.x86, s, u, cw, ch, &f.ch, &f.cv, ou, ocw, och, false);
        scale_plane(self.x86, s, v, cw, ch, &f.ch, &f.cv, ov, ocw, och, false);
    }

    /// The frame's luma at 1280 x 720 from its Y plane alone (`y`: w x h bytes), into `out` (DST_W * DST_H bytes):
    /// the Y plane yuv420p gives, for what reads only the luma (the camera watch), without scaling the chroma.
    pub fn luma(&mut self, y: &[u8], out: &mut [u8]) {
        if self.w == DST_W && self.h == DST_H {
            out.copy_from_slice(&y[..DST_W * DST_H]);
            return;
        }
        if self.exact_half() {
            mean2x2(y, self.w, out, DST_W, DST_H);
            return;
        }
        let f = self.yuv_filters.get_or_insert_with(|| filters(self.w, self.h, false, self.x86));
        scale_plane(self.x86, &mut self.scratch, y, self.w, self.h, &f.lh, &f.lv, out, DST_W, DST_H, true);
    }

}

/// One plane scaled to dw x dh as yuv420p scales it (`luma`: the Y plane, else U or V), with `x86` kernels or ffmpeg's
/// plain C code.
#[allow(clippy::too_many_arguments)]
fn scale_plane(
    x86: bool,
    s: &mut Scratch,
    src: &[u8],
    w: usize,
    h: usize,
    hf: &Filter,
    vf: &Filter,
    dst: &mut [u8],
    dw: usize,
    dh: usize,
    luma: bool,
) {
    hscale(src, w, h, hf, &mut s.y);
    s.acc.resize(dw, 0);
    // the x86 vertical kernel (ff_yuv2yuvX) runs on every row but the last two luma rows and last chroma row
    let simd_rows = if luma { DST_H - 2 } else { (DST_H - 1) / 2 };
    for (r, out) in dst.chunks_exact_mut(dw).take(dh).enumerate() {
        if vf.size == 1 {
            for (o, &a) in out.iter_mut().zip(&s.y[vf.at(r, 0) * dw..][..dw]) {
                *o = to_u8((a + 64) >> 7);
            }
        } else if x86 && r < simd_rows {
            // each tap's product is shifted down before the sum, as the SIMD kernel's 16-bit multiplies do
            s.acc.fill((64 + 8 * (vf.size as i32 - 1)) >> 4);
            for &(at, c) in &vf.taps[vf.tap_at[r] as usize..vf.tap_at[r + 1] as usize] {
                for (a, &v) in s.acc.iter_mut().zip(&s.y[at as usize * dw..][..dw]) {
                    *a += (v * c) >> 16;
                }
            }
            for (o, &a) in out.iter_mut().zip(&s.acc) {
                *o = to_u8((((a + 32768) & 0xffff) - 32768) >> 3);
            }
        } else {
            vsum(&s.y, dw, vf, r, 64 << 12, &mut s.acc);
            for (o, &a) in out.iter_mut().zip(&s.acc) {
                *o = to_u8(a >> 19);
            }
        }
    }
}

/// The buffers a conversion through the full pipeline fills, kept from frame to frame: the planes scaled across
/// (15-bit), a row's 8-bit Y, U and V, and a row's sums.
#[derive(Default)]
struct Scratch {
    y: Vec<i32>,
    u: Vec<i32>,
    v: Vec<i32>,
    ys: Vec<u8>,
    us: Vec<u8>,
    vs: Vec<u8>,
    acc: Vec<i32>,
}

/// One row of the 2:1 conversion: each output pixel the rounded mean of a 2 x 2 luma block, each pair of pixels the
/// rounded mean of two chroma samples, through RgbTables. Where the CPU has WebAssembly SIMD (the browser), 16 pixels
/// at a time with the tables as formulas (RgbFormula); where it has AVX2 (the desktop app on x86-64), 32 at a time the
/// same way; else a pixel at a time with the tables. The same integer arithmetic every way, so the same bytes (tests:
/// convert_parity and avx2_rows_give_the_tables natively, test_out/browser_check/rgb-bench.html in the browser).
mod lanes {
    use super::{DST_W, RgbTables};

    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    pub fn half_row(y0: &[u8], y1: &[u8], ur: &[u8], vr: &[u8], row: &mut [u8], t: &RgbTables) {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the CPU has AVX2
            return unsafe { avx2::half_row(y0, y1, ur, vr, row, t) };
        }
        table_row(y0, y1, ur, vr, row, t)
    }

    /// A pixel at a time, through the tables.
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    pub fn table_row(y0: &[u8], y1: &[u8], ur: &[u8], vr: &[u8], row: &mut [u8], t: &RgbTables) {
        for k in 0..DST_W / 2 {
            let uu = ((ur[2 * k] as u32 + ur[2 * k + 1] as u32 + 1) >> 1) as u8;
            let vv = ((vr[2 * k] as u32 + vr[2 * k + 1] as u32 + 1) >> 1) as u8;
            for i in [2 * k, 2 * k + 1] {
                let s = y0[2 * i] as u32 + y0[2 * i + 1] as u32 + y1[2 * i] as u32 + y1[2 * i + 1] as u32;
                t.rgb(((s + 2) >> 2) as u8, uu, vv, &mut row[i * 3..i * 3 + 3]);
            }
        }
    }

    /// The browser's 16 lanes twice over: AVX2 works on two 128-bit halves, so each half takes 16 of the 32 pixels
    /// and does what the WebAssembly code does, and only the loads and stores cross between the halves.
    #[cfg(target_arch = "x86_64")]
    pub mod avx2 {
        use super::{DST_W, RgbTables};
        use std::arch::x86_64::*;

        /// The byte shuffles that put one channel of 16 pixels at its places in bytes 16m to 16m + 16 of their 48 RGB
        /// bytes ([m][channel], the same in both halves; 0x80 gives a zero).
        const SPREAD: [[[u8; 32]; 3]; 3] = {
            let mut s = [[[0x80u8; 32]; 3]; 3];
            let mut at = 0;
            while at < 48 {
                let (m, j) = (at / 16, at % 16);
                s[m][at % 3][j] = (at / 3) as u8;
                s[m][at % 3][j + 16] = (at / 3) as u8;
                at += 1;
            }
            s
        };

        #[target_feature(enable = "avx2")]
        pub fn half_row(y0: &[u8], y1: &[u8], ur: &[u8], vr: &[u8], row: &mut [u8], t: &RgbTables) {
            assert!(y0.len() >= 2 * DST_W && y1.len() >= 2 * DST_W && ur.len() >= DST_W && vr.len() >= DST_W);
            assert!(row.len() >= 3 * DST_W && DST_W.is_multiple_of(32));
            let f = t.formula;
            let s32 = _mm256_set1_epi32;
            let (base, step, zero, top, ones) = (s32(f.y_base), s32(f.y_step), s32(0), s32(255), _mm256_set1_epi8(1));
            // SAFETY: each load reads the 32 bytes its slice is cut to, each store writes the 32 bytes its slice is cut to
            let load = |p: &[u8]| unsafe { _mm256_loadu_si256(p[..32].as_ptr().cast()) };
            let store = |p: &mut [u8], v: __m256i| unsafe { _mm256_storeu_si256(p[..32].as_mut_ptr().cast(), v) };
            let spread: [[__m256i; 3]; 3] = SPREAD.map(|m| m.map(|c| load(&c)));
            // a chroma table at 8 values: a + ((c * k) >> 16)
            let table = |(a, k): (i32, i32), c: __m256i| _mm256_add_epi32(s32(a), _mm256_srai_epi32::<16>(_mm256_mullo_epi32(c, s32(k))));
            // the y table at 8 indexes
            let level = |i: __m256i| {
                let l = _mm256_srai_epi32::<16>(_mm256_add_epi32(base, _mm256_mullo_epi32(i, step)));
                _mm256_min_epi32(_mm256_max_epi32(l, zero), top)
            };
            let px = |off: __m256i, y: __m256i| level(_mm256_add_epi32(off, y));
            // pairs of bytes summed into 16 bits
            let pairs = |a: __m256i| _mm256_maddubs_epi16(a, ones);
            let mean4 = |a: __m256i, b: __m256i| {
                _mm256_srli_epi16::<2>(_mm256_add_epi16(_mm256_add_epi16(pairs(a), pairs(b)), _mm256_set1_epi16(2)))
            };
            let mean2 = |a: __m256i| _mm256_srli_epi16::<1>(_mm256_add_epi16(pairs(a), _mm256_set1_epi16(1)));
            for k in 0..DST_W / 32 {
                // pixels 0 to 15 of the 32 (8 in each half), then 16 to 31
                let ma = mean4(load(&y0[64 * k..]), load(&y1[64 * k..]));
                let mb = mean4(load(&y0[64 * k + 32..]), load(&y1[64 * k + 32..]));
                // each half's 16 pixels: its first 8 (0 to 7, 16 to 23), then its last 8 (8 to 15, 24 to 31)
                let ya = _mm256_permute2x128_si256::<0x20>(ma, mb);
                let yb = _mm256_permute2x128_si256::<0x31>(ma, mb);
                // the 16 pairs' chroma: 8 in each half
                let (uu, vv) = (mean2(load(&ur[32 * k..])), mean2(load(&vr[32 * k..])));
                let (u0, u1) = (_mm256_unpacklo_epi16(uu, zero), _mm256_unpackhi_epi16(uu, zero));
                let (v0, v1) = (_mm256_unpacklo_epi16(vv, zero), _mm256_unpackhi_epi16(vv, zero));
                // each pair's offsets, pairs 0 to 3 and 4 to 7 of each half
                let (r0, r1) = (table(f.rv, v0), table(f.rv, v1));
                let g0 = _mm256_add_epi32(table(f.gu, u0), table(f.gv, v0));
                let g1 = _mm256_add_epi32(table(f.gu, u1), table(f.gv, v1));
                let (b0, b1) = (table(f.bu, u0), table(f.bu, u1));
                // each half's 16 pixels' luma, 4 at a time
                let y = [
                    _mm256_unpacklo_epi16(ya, zero),
                    _mm256_unpackhi_epi16(ya, zero),
                    _mm256_unpacklo_epi16(yb, zero),
                    _mm256_unpackhi_epi16(yb, zero),
                ];
                let channel = |p0: __m256i, p1: __m256i| {
                    let l0 = px(_mm256_unpacklo_epi32(p0, p0), y[0]);
                    let l1 = px(_mm256_unpackhi_epi32(p0, p0), y[1]);
                    let l2 = px(_mm256_unpacklo_epi32(p1, p1), y[2]);
                    let l3 = px(_mm256_unpackhi_epi32(p1, p1), y[3]);
                    _mm256_packus_epi16(_mm256_packs_epi32(l0, l1), _mm256_packs_epi32(l2, l3))
                };
                let rgb = [channel(r0, r1), channel(g0, g1), channel(b0, b1)];
                // each half's 48 RGB bytes, 16 at a time
                let out = spread.map(|s| {
                    let r = _mm256_shuffle_epi8(rgb[0], s[0]);
                    _mm256_or_si256(_mm256_or_si256(r, _mm256_shuffle_epi8(rgb[1], s[1])), _mm256_shuffle_epi8(rgb[2], s[2]))
                });
                // the first half's 48 bytes, then the second's
                store(&mut row[96 * k..], _mm256_permute2x128_si256::<0x20>(out[0], out[1]));
                store(&mut row[96 * k + 32..], _mm256_permute2x128_si256::<0x30>(out[2], out[0]));
                store(&mut row[96 * k + 64..], _mm256_permute2x128_si256::<0x31>(out[1], out[2]));
            }
        }
    }

    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    use core::arch::wasm32::*;

    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    pub fn half_row(y0: &[u8], y1: &[u8], ur: &[u8], vr: &[u8], row: &mut [u8], t: &RgbTables) {
        assert!(y0.len() >= 2 * DST_W && y1.len() >= 2 * DST_W && ur.len() >= DST_W && vr.len() >= DST_W);
        assert!(row.len() >= 3 * DST_W && DST_W.is_multiple_of(16));
        let f = t.formula;
        let s32 = i32x4_splat;
        let (base, step, zero, top) = (s32(f.y_base), s32(f.y_step), s32(0), s32(255));
        // a chroma table at 4 values: a + ((c * k) >> 16)
        let table = |(a, k): (i32, i32), c: v128| i32x4_add(s32(a), i32x4_shr(i32x4_mul(c, s32(k)), 16));
        // the y table at 4 indexes
        let level = |i: v128| i32x4_min(i32x4_max(i32x4_shr(i32x4_add(base, i32x4_mul(i, step)), 16), zero), top);
        // 4 pixels' channel from their pairs' offsets (pairs p, p, p+1, p+1) and their luma
        let px = |off: v128, y: v128| level(i32x4_add(off, y));
        for k in 0..DST_W / 16 {
            // SAFETY: the loads read 32 bytes at 32k of y0 and y1 and 16 bytes at 16k of ur and vr, and the stores
            // write 48 bytes at 48k of row, all within the lengths asserted above; unaligned access is allowed
            unsafe {
                let load = |p: *const u8| v128_load(p as *const v128);
                let mean4 = |a: v128, b: v128| {
                    let s = u16x8_add(u16x8_extadd_pairwise_u8x16(a), u16x8_extadd_pairwise_u8x16(b));
                    u16x8_shr(u16x8_add(s, u16x8_splat(2)), 2)
                };
                let ya = mean4(load(y0.as_ptr().add(32 * k)), load(y1.as_ptr().add(32 * k)));
                let yb = mean4(load(y0.as_ptr().add(32 * k + 16)), load(y1.as_ptr().add(32 * k + 16)));
                let mean2 = |a: v128| u16x8_shr(u16x8_add(u16x8_extadd_pairwise_u8x16(a), u16x8_splat(1)), 1);
                let (uu, vv) = (mean2(load(ur.as_ptr().add(16 * k))), mean2(load(vr.as_ptr().add(16 * k))));
                let (u0, u1) = (u32x4_extend_low_u16x8(uu), u32x4_extend_high_u16x8(uu));
                let (v0, v1) = (u32x4_extend_low_u16x8(vv), u32x4_extend_high_u16x8(vv));
                // each pair's offsets, pairs 0 to 3 and 4 to 7
                let (r0, r1) = (table(f.rv, v0), table(f.rv, v1));
                let (g0, g1) = (i32x4_add(table(f.gu, u0), table(f.gv, v0)), i32x4_add(table(f.gu, u1), table(f.gv, v1)));
                let (b0, b1) = (table(f.bu, u0), table(f.bu, u1));
                // the 16 pixels' luma, 4 at a time
                let y = [
                    u32x4_extend_low_u16x8(ya),
                    u32x4_extend_high_u16x8(ya),
                    u32x4_extend_low_u16x8(yb),
                    u32x4_extend_high_u16x8(yb),
                ];
                let channel = |p0: v128, p1: v128| {
                    let l0 = px(i32x4_shuffle::<0, 0, 1, 1>(p0, p0), y[0]);
                    let l1 = px(i32x4_shuffle::<2, 2, 3, 3>(p0, p0), y[1]);
                    let l2 = px(i32x4_shuffle::<0, 0, 1, 1>(p1, p1), y[2]);
                    let l3 = px(i32x4_shuffle::<2, 2, 3, 3>(p1, p1), y[3]);
                    u8x16_narrow_i16x8(i16x8_narrow_i32x4(l0, l1), i16x8_narrow_i32x4(l2, l3))
                };
                let (r, g, b) = (channel(r0, r1), channel(g0, g1), channel(b0, b1));
                // R, G and B interleaved: 48 bytes, R G and B of pixel 0, then of pixel 1, and so on
                let rg0 = i8x16_shuffle::<0, 16, 0, 1, 17, 0, 2, 18, 0, 3, 19, 0, 4, 20, 0, 5>(r, g);
                let out0 = i8x16_shuffle::<0, 1, 16, 3, 4, 17, 6, 7, 18, 9, 10, 19, 12, 13, 20, 15>(rg0, b);
                let rg1 = i8x16_shuffle::<21, 0, 6, 22, 0, 7, 23, 0, 8, 24, 0, 9, 25, 0, 10, 26>(r, g);
                let out1 = i8x16_shuffle::<0, 21, 2, 3, 22, 5, 6, 23, 8, 9, 24, 11, 12, 25, 14, 15>(rg1, b);
                let rg2 = i8x16_shuffle::<0, 11, 27, 0, 12, 28, 0, 13, 29, 0, 14, 30, 0, 15, 31, 0>(r, g);
                let out2 = i8x16_shuffle::<26, 1, 2, 27, 4, 5, 28, 7, 8, 29, 10, 11, 30, 13, 14, 31>(rg2, b);
                let dst = row.as_mut_ptr().add(48 * k) as *mut v128;
                v128_store(dst, out0);
                v128_store(dst.add(1), out1);
                v128_store(dst.add(2), out2);
            }
        }
    }
}

/// The rounded mean of each 2 x 2 block of a plane (src is 2w x 2h).
fn mean2x2(src: &[u8], sw: usize, dst: &mut [u8], w: usize, h: usize) {
    for r in 0..h {
        let (a, b) = (&src[2 * r * sw..2 * r * sw + 2 * w], &src[(2 * r + 1) * sw..(2 * r + 1) * sw + 2 * w]);
        for ((d, a), b) in dst[r * w..(r + 1) * w].iter_mut().zip(a.chunks_exact(2)).zip(b.chunks_exact(2)) {
            *d = ((a[0] as u32 + a[1] as u32 + b[0] as u32 + b[1] as u32 + 2) >> 2) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The formulas the 2:1 rows with SIMD compute must give every table entry, for every matrix and range.
    #[test]
    fn formulas_give_the_tables() {
        for matrix in [Matrix::Bt709, Matrix::Bt601, Matrix::Fcc, Matrix::Smpte240m, Matrix::Bt2020] {
            for full in [false, true] {
                let t = RgbTables::new(matrix, full);
                let f = t.formula;
                for i in 0..2048i32 {
                    let y = ((f.y_base + i * f.y_step) >> 16).clamp(0, 255) as u8;
                    assert_eq!(y, t.y[i as usize], "y[{i}]");
                }
                let line = |(a, k): (i32, i32), c: i32| a + ((c * k) >> 16);
                for c in 0..256usize {
                    let ci = c as i32;
                    assert_eq!(line(f.rv, ci) as i64, t.rv[c]);
                    assert_eq!(line(f.gu, ci) as i64, t.gu[c]);
                    assert_eq!(line(f.bu, ci) as i64, t.bu[c]);
                    assert_eq!(line(f.gv, ci) as i64, t.gv[c]);
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
        let (mut y0, mut y1, mut ur, mut vr) = (vec![0u8; 2 * DST_W], vec![0u8; 2 * DST_W], vec![0u8; DST_W], vec![0u8; DST_W]);
        let (mut got, mut want) = (vec![0u8; 3 * DST_W], vec![0u8; 3 * DST_W]);
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 24) as u8
        };
        for matrix in [Matrix::Bt709, Matrix::Bt601, Matrix::Fcc, Matrix::Smpte240m, Matrix::Bt2020] {
            for full in [false, true] {
                let t = RgbTables::new(matrix, full);
                let mut check = |y0: &[u8], y1: &[u8], ur: &[u8], vr: &[u8], what: &str| {
                    lanes::table_row(y0, y1, ur, vr, &mut want, &t);
                    // SAFETY: the CPU has AVX2
                    unsafe { lanes::avx2::half_row(y0, y1, ur, vr, &mut got, &t) };
                    let at = got.iter().zip(&want).position(|(a, b)| a != b);
                    assert!(at.is_none(), "{matrix:?} full={full} {what}: byte {at:?} differs");
                };
                // pair p: U = p % 256, V = (v + 3p) % 256; pixel i: luma (2s + i % 2 + 7p) % 256. Over v and s every
                // U meets every V and every luma.
                for v in 0..256 {
                    for p in 0..DST_W / 2 {
                        ur[2 * p..2 * p + 2].fill((p % 256) as u8);
                        vr[2 * p..2 * p + 2].fill(((v + 3 * p) % 256) as u8);
                    }
                    for s in 0..128 {
                        for i in 0..DST_W {
                            let y = ((2 * s + i % 2 + 7 * (i / 2)) % 256) as u8;
                            y0[2 * i..2 * i + 2].fill(y);
                            y1[2 * i..2 * i + 2].fill(y);
                        }
                        check(&y0, &y1, &ur, &vr, &format!("v={v} s={s}"));
                    }
                }
                for n in 0..2000 {
                    for b in y0.iter_mut().chain(y1.iter_mut()).chain(ur.iter_mut()).chain(vr.iter_mut()) {
                        *b = noise();
                    }
                    check(&y0, &y1, &ur, &vr, &format!("noise {n}"));
                }
            }
        }
    }
}
