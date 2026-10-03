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

/// One direction's filter: per output sample, the first input sample and the coefficients (`size` of them).
#[derive(Clone, Debug)]
struct Filter {
    pos: Vec<i64>,
    coef: Vec<i64>,
    size: usize,
}

impl Filter {
    fn row(&self, i: usize) -> &[i64] {
        &self.coef[i * self.size..(i + 1) * self.size]
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
    Filter { pos, coef, size }
}

/// hScale8To15_c: one plane's rows, 8-bit samples to 15-bit.
fn hscale(src: &[u8], w: usize, h: usize, f: &Filter) -> Vec<i64> {
    let dw = f.pos.len();
    let mut out = vec![0i64; h * dw];
    for r in 0..h {
        let row = &src[r * w..(r + 1) * w];
        for i in 0..dw {
            let mut acc = 0i64;
            for (j, c) in f.row(i).iter().enumerate() {
                acc += row[((f.pos[i] + j as i64) as usize).min(w - 1)] as i64 * c;
            }
            out[r * dw + i] = (acc >> 7).min(32767);
        }
    }
    out
}

/// ff_yuv2rgb_c_init_tables() for 24 bits, no brightness, contrast and saturation 1: for 8-bit Y, U, V,
/// R = y[rv[V] + Y], G = y[gu[U] + gv[V] + Y], B = y[bu[U] + Y].
struct RgbTables {
    y: Vec<u8>,
    rv: [i64; 256],
    gu: [i64; 256],
    bu: [i64; 256],
    gv: [i64; 256],
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
        let y = (0..2048i64)
            .map(|i| ((-(384i64 << 16) - 512 * cy - oy + i * cy + 0x8000) >> 16).clamp(0, 255) as u8)
            .collect();
        let tab = |k: i64, off: i64| {
            let mut t = [0i64; 256];
            for (c, v) in t.iter_mut().enumerate() {
                *v = off - (k >> 9) + ((c as i64 * k) >> 16);
            }
            t
        };
        RgbTables { y, rv: tab(crv, yoffs), gu: tab(cgu, yoffs), bu: tab(cbu, yoffs), gv: tab(cgv, 0) }
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
        let (cw, ch) = (self.w.div_ceil(2), self.h.div_ceil(2));
        let yh = hscale(y, self.w, self.h, &f.lh);
        let uh = hscale(u, cw, ch, &f.ch);
        let vh = hscale(v, cw, ch, &f.ch);
        let cdw = f.ch.pos.len();
        let (lfs, cfs) = (f.lv.size, f.cv.size);
        let mut ys = vec![0u8; DST_W];
        let mut us = vec![0u8; cdw];
        let mut vs = vec![0u8; cdw];
        let line = |width: usize, rows: usize, at: i64| (at as usize).min(rows - 1) * width;
        for r in 0..DST_H {
            let (l, c) = (f.lv.row(r), f.cv.row(r));
            let (lp, cp) = (f.lv.pos[r], f.cv.pos[r]);
            let two_c = cfs == 2 && c[0] + c[1] == 4096 && (0..=4096).contains(&c[1]);
            let lrow = |j: usize| line(DST_W, self.h, lp + j as i64);
            let crow = |j: usize| line(cdw, ch, cp + j as i64);
            for i in 0..DST_W {
                let yv = if lfs == 1 && (cfs == 1 || two_c) {
                    (yh[lrow(0) + i] + 64) >> 7
                } else if lfs == 2 && two_c && l[0] + l[1] == 4096 && (0..=4096).contains(&l[1]) {
                    (yh[lrow(0) + i] * (4096 - l[1]) + yh[lrow(1) + i] * l[1]) >> 19
                } else {
                    ((1 << 18) + (0..lfs).map(|j| yh[lrow(j) + i] * l[j]).sum::<i64>()) >> 19
                };
                ys[i] = yv.clamp(0, 255) as u8;
            }
            for (k, (us, vs)) in us.iter_mut().zip(vs.iter_mut()).enumerate() {
                let (uv, vv) = if lfs == 1 && (cfs == 1 || two_c) {
                    let a = if cfs == 2 { c[1] } else { 0 };
                    if a == 0 {
                        ((uh[crow(0) + k] + 64) >> 7, (vh[crow(0) + k] + 64) >> 7)
                    } else {
                        (
                            (uh[crow(0) + k] * (4096 - a) + uh[crow(1) + k] * a + (128 << 11)) >> 19,
                            (vh[crow(0) + k] * (4096 - a) + vh[crow(1) + k] * a + (128 << 11)) >> 19,
                        )
                    }
                } else if lfs == 2 && two_c && l[0] + l[1] == 4096 && (0..=4096).contains(&l[1]) {
                    (
                        (uh[crow(0) + k] * (4096 - c[1]) + uh[crow(1) + k] * c[1]) >> 19,
                        (vh[crow(0) + k] * (4096 - c[1]) + vh[crow(1) + k] * c[1]) >> 19,
                    )
                } else {
                    (
                        ((1 << 18) + (0..cfs).map(|j| uh[crow(j) + k] * c[j]).sum::<i64>()) >> 19,
                        ((1 << 18) + (0..cfs).map(|j| vh[crow(j) + k] * c[j]).sum::<i64>()) >> 19,
                    )
                };
                *us = uv.clamp(0, 255) as u8;
                *vs = vv.clamp(0, 255) as u8;
            }
            let row = &mut out[r * DST_W * 3..(r + 1) * DST_W * 3];
            for i in 0..DST_W {
                self.tables.rgb(ys[i], us[i / 2], vs[i / 2], &mut row[i * 3..i * 3 + 3]);
            }
        }
    }

    /// Exactly 2:1 (2560 x 1440): the filters reduce to plain rounded means, the same bytes much faster.
    fn half_rgb24(&self, y: &[u8], u: &[u8], v: &[u8], out: &mut [u8]) {
        let (w, cw) = (self.w, self.w / 2);
        for r in 0..DST_H {
            let (y0, y1) = (&y[2 * r * w..(2 * r + 1) * w], &y[(2 * r + 1) * w..(2 * r + 2) * w]);
            let (ur, vr) = (&u[r * cw..(r + 1) * cw], &v[r * cw..(r + 1) * cw]);
            let row = &mut out[r * DST_W * 3..(r + 1) * DST_W * 3];
            for k in 0..DST_W / 2 {
                let uu = ((ur[2 * k] as u32 + ur[2 * k + 1] as u32 + 1) >> 1) as u8;
                let vv = ((vr[2 * k] as u32 + vr[2 * k + 1] as u32 + 1) >> 1) as u8;
                for i in [2 * k, 2 * k + 1] {
                    let s = y0[2 * i] as u32 + y0[2 * i + 1] as u32 + y1[2 * i] as u32 + y1[2 * i + 1] as u32;
                    self.tables.rgb(((s + 2) >> 2) as u8, uu, vv, &mut row[i * 3..i * 3 + 3]);
                }
            }
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
        self.yuv_filters.get_or_insert_with(|| filters(self.w, self.h, false, self.x86));
        let f = self.yuv_filters.as_ref().unwrap();
        self.scale_plane(y, self.w, self.h, &f.lh, &f.lv, oy, DST_W, DST_H, true);
        self.scale_plane(u, cw, ch, &f.ch, &f.cv, ou, ocw, och, false);
        self.scale_plane(v, cw, ch, &f.ch, &f.cv, ov, ocw, och, false);
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
        self.yuv_filters.get_or_insert_with(|| filters(self.w, self.h, false, self.x86));
        let f = self.yuv_filters.as_ref().unwrap();
        self.scale_plane(y, self.w, self.h, &f.lh, &f.lv, out, DST_W, DST_H, true);
    }

    /// One plane scaled to dw x dh as yuv420p scales it (`luma`: the Y plane, else U or V).
    #[allow(clippy::too_many_arguments)]
    fn scale_plane(&self, src: &[u8], w: usize, h: usize, hf: &Filter, vf: &Filter, dst: &mut [u8], dw: usize, dh: usize, luma: bool) {
        let hs = hscale(src, w, h, hf);
        // the x86 vertical kernel (ff_yuv2yuvX) runs on every row but the last two luma rows and last chroma row
        let simd_rows = if luma { DST_H - 2 } else { (DST_H - 1) / 2 };
        for r in 0..dh {
            let c = vf.row(r);
            let at = |j: usize| ((vf.pos[r] + j as i64) as usize).min(h - 1) * dw;
            for i in 0..dw {
                let o = if vf.size == 1 {
                    (hs[at(0) + i] + 64) >> 7
                } else if self.x86 && r < simd_rows {
                    let mut acc = (64 + 8 * (vf.size as i64 - 1)) >> 4;
                    for (j, cj) in c.iter().enumerate() {
                        acc += (hs[at(j) + i] * cj) >> 16;
                    }
                    wrap16(acc) >> 3
                } else {
                    let mut acc = 64i64 << 12;
                    for (j, cj) in c.iter().enumerate() {
                        acc += hs[at(j) + i] * cj;
                    }
                    acc >> 19
                };
                dst[r * dw + i] = o.clamp(0, 255) as u8;
            }
        }
    }
}

/// The rounded mean of each 2 x 2 block of a plane (src is 2w x 2h).
fn mean2x2(src: &[u8], sw: usize, dst: &mut [u8], w: usize, h: usize) {
    for r in 0..h {
        let (a, b) = (&src[2 * r * sw..], &src[(2 * r + 1) * sw..]);
        for i in 0..w {
            let s = a[2 * i] as u32 + a[2 * i + 1] as u32 + b[2 * i] as u32 + b[2 * i + 1] as u32;
            dst[r * w + i] = ((s + 2) >> 2) as u8;
        }
    }
}
