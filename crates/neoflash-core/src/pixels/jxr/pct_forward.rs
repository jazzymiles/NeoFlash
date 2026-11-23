use super::tables::*;
use super::*;

// ============================================================================
// Reversible Photo Core Transform (PCT) & Overlap Filtering Operators
// ============================================================================

#[inline]
pub(super) fn idx_at(base: isize, offset: isize) -> usize {
    let pos = base + offset;
    debug_assert!(pos >= 0);
    pos as usize
}

#[inline]
pub(super) fn rotate1(a: &mut i32, b: &mut i32) {
    *b -= (*a + 1) >> 1;
    *a += (*b + 1) >> 1;
}

#[inline]
pub(super) fn rotate2(a: &mut i32, b: &mut i32) {
    *b -= (*a * 3 + 4) >> 3;
    *a += (*b * 3 + 4) >> 3;
}

#[inline]
pub(super) fn irotate1(a: &mut i32, b: &mut i32) {
    *a -= (*b + 1) >> 1;
    *b += (*a + 1) >> 1;
}

#[inline]
pub(super) fn irotate2(a: &mut i32, b: &mut i32) {
    *a -= (*b * 3 + 4) >> 3;
    *b += (*a * 3 + 4) >> 3;
}

#[inline]
pub(super) fn dct_2x2_dn_vals(
    mut a: i32,
    mut b: i32,
    c_in: i32,
    mut d: i32,
) -> (i32, i32, i32, i32) {
    a += d;
    b -= c_in;
    let t = (a - b) >> 1;
    let c = t - d;
    d = t - c_in;
    a -= d;
    b += c;
    (a, b, c, d)
}

#[inline]
pub(super) fn dct_2x2_dn(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (a, b, c, d) = dct_2x2_dn_vals(buf[i0], buf[i1], buf[i2], buf[i3]);
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
pub(super) fn dct_2x2_dn_enc(
    buf: &mut [i32],
    base: isize,
    o0: isize,
    o1: isize,
    o2: isize,
    o3: isize,
) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (a, b, c, d) = dct_2x2_dn_vals(buf[i0] >> 1, buf[i1] >> 1, buf[i2] >> 1, buf[i3] >> 1);
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
pub(super) fn dct_2x2_dn_dec(
    buf: &mut [i32],
    base: isize,
    o0: isize,
    o1: isize,
    o2: isize,
    o3: isize,
) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (a, b, c, d) = dct_2x2_dn_vals(buf[i0], buf[i1], buf[i2], buf[i3]);
    buf[i0] = a * 2;
    buf[i1] = b * 2;
    buf[i2] = c * 2;
    buf[i3] = d * 2;
}

#[inline]
pub(super) fn dct_2x2_up(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (mut a, mut b, c_in, mut d) = (buf[i0], buf[i1], buf[i2], buf[i3]);
    a += d;
    b -= c_in;
    let t = (a - b + 1) >> 1;
    let c = t - d;
    d = t - c_in;
    a -= d;
    b += c;
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
pub(super) fn fwd_odd_odd(
    buf: &mut [i32],
    base: isize,
    o0: isize,
    o1: isize,
    o2: isize,
    o3: isize,
) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let mut a = buf[i0];
    let mut b = -buf[i1];
    let mut c = -buf[i2];
    let mut d = buf[i3];
    d += a;
    c -= b;
    let t1 = d >> 1;
    a -= t1;
    let t2 = c >> 1;
    b += t2;
    a += (b * 3 + 4) >> 3;
    b -= (a * 3 + 3) >> 2;
    a += (b * 3 + 3) >> 3;
    b -= t2;
    a += t1;
    c += b;
    d -= a;
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
pub(super) fn fwd_odd_odd_pre_vals(
    mut a: i32,
    mut b: i32,
    mut c: i32,
    mut d: i32,
) -> (i32, i32, i32, i32) {
    d += a;
    c -= b;
    let t1 = d >> 1;
    a -= t1;
    let t2 = c >> 1;
    b += t2;
    a += (b * 3 + 4) >> 3;
    b -= (a * 3 + 2) >> 2;
    a += (b * 3 + 6) >> 3;
    b -= t2;
    a += t1;
    c += b;
    d -= a;
    (a, b, c, d)
}

#[inline]
pub(super) fn fwd_odd(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (mut a, mut b, mut c, mut d) = (buf[i0], buf[i1], buf[i2], buf[i3]);
    b -= c;
    a += d;
    c += (b + 1) >> 1;
    d = ((a + 1) >> 1) - d;
    rotate2(&mut a, &mut b);
    rotate2(&mut c, &mut d);
    d += b >> 1;
    c -= (a + 1) >> 1;
    b -= d;
    a += c;
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

pub(super) fn str_dct_4x4_stage1(buf: &mut [i32], base: isize) {
    dct_2x2_dn(buf, base, 0, 4, 8, 12);
    dct_2x2_dn(buf, base, 1, 5, 9, 13);
    dct_2x2_dn(buf, base, 2, 6, 10, 14);
    dct_2x2_dn(buf, base, 3, 7, 11, 15);
    dct_2x2_up(buf, base, 0, 1, 2, 3);
    fwd_odd_odd(buf, base, 15, 14, 13, 12);
    fwd_odd(buf, base, 5, 4, 7, 6);
    fwd_odd(buf, base, 10, 8, 11, 9);
}

pub(super) fn str_dct_4x4_second_stage(buf: &mut [i32], base: isize) {
    dct_2x2_dn(buf, base, 0, 192, 48, 240);
    dct_2x2_dn(buf, base, 64, 128, 112, 176);
    dct_2x2_dn(buf, base, 16, 208, 32, 224);
    dct_2x2_dn(buf, base, 80, 144, 96, 160);
    dct_2x2_up(buf, base, 0, 64, 16, 80);
    fwd_odd_odd(buf, base, 160, 224, 176, 240);
    fwd_odd(buf, base, 128, 192, 144, 208);
    fwd_odd(buf, base, 32, 48, 96, 112);
}

pub(super) fn str_normalize_enc(buf: &mut [i32], base: isize, is_chroma: bool) {
    if is_chroma {
        for off in (0..256isize).step_by(16) {
            buf[idx_at(base, off)] >>= 1;
        }
    }
}

#[inline]
pub(super) fn str_hst_enc_vals(mut a: i32, mut b: i32, pc: i32, pd: i32) -> (i32, i32, i32, i32) {
    let mut d = pc;
    let mut c = pd;
    a += c;
    b -= d;
    c = ((a - b) >> 1) - c;
    d += b >> 1;
    b += c;
    a -= (d * 3 + 4) >> 3;
    (a, b, c, d)
}

#[inline]
pub(super) fn str_hst_enc1_vals(mut a: i32, mut d: i32) -> (i32, i32) {
    d -= a >> 7;
    d += a >> 10;
    d -= (a * 3) >> 4;
    a -= (d * 3) >> 3;
    d = (a >> 1) - d;
    a -= d;
    (a, d)
}

#[inline]
pub(super) fn str_hst_enc1_edge(a: &mut i32, pd: &mut i32) {
    let mut av = *a;
    let mut d = -(*pd);
    av -= d;
    d += av >> 1;
    av -= (d * 3 + 4) >> 3;
    d -= av >> 7;
    d += av >> 10;
    d -= (av * 3) >> 4;
    av -= (d * 3) >> 3;
    d = (av >> 1) - d;
    av -= d;
    *a = av;
    *pd = d;
}

#[inline]
pub(super) fn str_pre4_vals(
    mut a: i32,
    mut b: i32,
    mut c: i32,
    mut d: i32,
) -> (i32, i32, i32, i32) {
    a += d;
    b += c;
    d -= (a + 1) >> 1;
    c -= (b + 1) >> 1;
    rotate1(&mut c, &mut d);
    str_hst_enc1_edge(&mut a, &mut d);
    str_hst_enc1_edge(&mut b, &mut c);
    d += (a + 1) >> 1;
    c += (b + 1) >> 1;
    a -= d;
    b -= c;
    (a, b, c, d)
}

#[inline]
pub(super) fn str_pre4(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (a, b, c, d) = str_pre4_vals(buf[i0], buf[i1], buf[i2], buf[i3]);
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
pub(super) fn str_pre4_split(
    a0: &mut [i32],
    b0: isize,
    o0: isize,
    o1: isize,
    a1: &mut [i32],
    b1: isize,
    o2: isize,
    o3: isize,
) {
    let (i0, i1) = (idx_at(b0, o0), idx_at(b0, o1));
    let (i2, i3) = (idx_at(b1, o2), idx_at(b1, o3));
    let (a, b, c, d) = str_pre4_vals(a0[i0], a0[i1], a1[i2], a1[i3]);
    a0[i0] = a;
    a0[i1] = b;
    a1[i2] = c;
    a1[i3] = d;
}

#[inline]
pub(super) fn str_pre2_vals(mut a: i32, mut b: i32) -> (i32, i32) {
    b -= (a + 2) >> 2;
    a -= (b + 1) >> 1;
    a -= b >> 5;
    a -= b >> 9;
    a -= b >> 13;
    b -= (a + 2) >> 2;
    (a, b)
}

#[inline]
pub(super) fn str_pre2(buf: &mut [i32], base: isize, o0: isize, o1: isize) {
    let (i0, i1) = (idx_at(base, o0), idx_at(base, o1));
    let (a, b) = str_pre2_vals(buf[i0], buf[i1]);
    buf[i0] = a;
    buf[i1] = b;
}

#[inline]
pub(super) fn str_pre2_split(
    a0: &mut [i32],
    b0: isize,
    o0: isize,
    a1: &mut [i32],
    b1: isize,
    o1: isize,
) {
    let (i0, i1) = (idx_at(b0, o0), idx_at(b1, o1));
    let (a, b) = str_pre2_vals(a0[i0], a1[i1]);
    a0[i0] = a;
    a1[i1] = b;
}

#[inline]
pub(super) fn str_pre2x2_vals(
    mut a: i32,
    mut b: i32,
    mut c: i32,
    mut d: i32,
) -> (i32, i32, i32, i32) {
    a += d;
    b += c;
    d -= (a + 1) >> 1;
    c -= (b + 1) >> 1;
    b -= (a + 2) >> 2;
    a -= (b + 1) >> 1;
    a -= b >> 5;
    a -= b >> 9;
    a -= b >> 13;
    b -= (a + 2) >> 2;
    d += (a + 1) >> 1;
    c += (b + 1) >> 1;
    a -= d;
    b -= c;
    (a, b, c, d)
}

#[inline]
pub(super) fn str_pre2x2(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (a, b, c, d) = str_pre2x2_vals(buf[i0], buf[i1], buf[i2], buf[i3]);
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
pub(super) fn str_pre2x2_split(
    a0: &mut [i32],
    b0: isize,
    o0: isize,
    o1: isize,
    a1: &mut [i32],
    b1: isize,
    o2: isize,
    o3: isize,
) {
    let (i0, i1) = (idx_at(b0, o0), idx_at(b0, o1));
    let (i2, i3) = (idx_at(b1, o2), idx_at(b1, o3));
    let (a, b, c, d) = str_pre2x2_vals(a0[i0], a0[i1], a1[i2], a1[i3]);
    a0[i0] = a;
    a0[i1] = b;
    a1[i2] = c;
    a1[i3] = d;
}

pub(super) fn str_pre_4x4_core(
    q0: &mut [i32; 4],
    q1: &mut [i32; 4],
    q2: &mut [i32; 4],
    q3: &mut [i32; 4],
) {
    for k in 0..4 {
        let (a, b, c, d) = str_hst_enc_vals(q0[k], q2[k], q1[k], q3[k]);
        let (a, d) = str_hst_enc1_vals(a, d);
        q0[k] = a;
        q2[k] = b;
        q1[k] = c;
        q3[k] = d;
    }
    let (mut c12, mut c13) = (q1[2], q1[3]);
    rotate1(&mut c12, &mut c13);
    q1[2] = c12;
    q1[3] = c13;

    let (mut c10, mut c11) = (q1[0], q1[1]);
    rotate1(&mut c10, &mut c11);
    q1[0] = c10;
    q1[1] = c11;

    let (mut c21, mut c23) = (q2[1], q2[3]);
    rotate1(&mut c21, &mut c23);
    q2[1] = c21;
    q2[3] = c23;

    let (mut c20, mut c22) = (q2[0], q2[2]);
    rotate1(&mut c20, &mut c22);
    q2[0] = c20;
    q2[2] = c22;

    let (d0, d1, d2, d3) = fwd_odd_odd_pre_vals(q3[0], q3[1], q3[2], q3[3]);
    *q3 = [d0, d1, d2, d3];

    for k in 0..4 {
        let (a, b, c, d) = dct_2x2_dn_vals(q0[k], q2[k], q1[k], q3[k]);
        q0[k] = a;
        q2[k] = b;
        q1[k] = c;
        q3[k] = d;
    }
}

pub(super) fn str_pre_4x4_stage1(buf: &mut [i32], base: isize, offset: isize) {
    let p0 = idx_at(base, 12);
    let p1 = idx_at(base, 20);
    let p2 = idx_at(base, 72 - offset);
    let p3 = idx_at(base, 80 - offset);
    let mut q0 = [buf[p0], buf[p0 + 1], buf[p0 + 2], buf[p0 + 3]];
    let mut q1 = [buf[p1], buf[p1 + 1], buf[p1 + 2], buf[p1 + 3]];
    let mut q2 = [buf[p2], buf[p2 + 1], buf[p2 + 2], buf[p2 + 3]];
    let mut q3 = [buf[p3], buf[p3 + 1], buf[p3 + 2], buf[p3 + 3]];
    str_pre_4x4_core(&mut q0, &mut q1, &mut q2, &mut q3);
    buf[p0..p0 + 4].copy_from_slice(&q0);
    buf[p1..p1 + 4].copy_from_slice(&q1);
    buf[p2..p2 + 4].copy_from_slice(&q2);
    buf[p3..p3 + 4].copy_from_slice(&q3);
}

pub(super) fn str_pre_4x4_stage1_split(
    a0: &mut [i32],
    b0: isize,
    a1: &mut [i32],
    b1: isize,
    offset: isize,
) {
    let p0 = idx_at(b0, 12);
    let p2 = idx_at(b0, 72 - offset);
    let p1 = idx_at(b1, 4);
    let p3 = idx_at(b1, 64 - offset);
    let mut q0 = [a0[p0], a0[p0 + 1], a0[p0 + 2], a0[p0 + 3]];
    let mut q1 = [a1[p1], a1[p1 + 1], a1[p1 + 2], a1[p1 + 3]];
    let mut q2 = [a0[p2], a0[p2 + 1], a0[p2 + 2], a0[p2 + 3]];
    let mut q3 = [a1[p3], a1[p3 + 1], a1[p3 + 2], a1[p3 + 3]];
    str_pre_4x4_core(&mut q0, &mut q1, &mut q2, &mut q3);
    a0[p0..p0 + 4].copy_from_slice(&q0);
    a0[p2..p2 + 4].copy_from_slice(&q2);
    a1[p1..p1 + 4].copy_from_slice(&q1);
    a1[p3..p3 + 4].copy_from_slice(&q3);
}

pub(super) fn str_pre_4x4_stage2_split(a0: &mut [i32], p0: isize, a1: &mut [i32], p1: isize) {
    let o0 = [-96isize, -32, -80, -16];
    let o2 = [96isize, 32, 112, 48];
    let o1 = [-112isize, -48, -128, -64];
    let o3 = [80isize, 16, 64, 0];

    let mut q0 = [0i32; 4];
    let mut q1 = [0i32; 4];
    let mut q2 = [0i32; 4];
    let mut q3 = [0i32; 4];
    for k in 0..4 {
        let (a, b, c, d) = str_hst_enc_vals(
            a0[idx_at(p0, o0[k])],
            a0[idx_at(p0, o2[k])],
            a1[idx_at(p1, o1[k])],
            a1[idx_at(p1, o3[k])],
        );
        let (a, d) = str_hst_enc1_vals(a, d);
        q0[k] = a;
        q2[k] = b;
        q1[k] = c;
        q3[k] = d;
    }

    // ROTATE1(p1[-48], p1[-112]), ROTATE1(p1[-64], p1[-128]), ROTATE1(p0[112], p0[96]), ROTATE1(p0[48], p0[32])
    let (mut r0, mut r1) = (q1[1], q1[0]);
    rotate1(&mut r0, &mut r1);
    q1[1] = r0;
    q1[0] = r1;

    let (mut r2, mut r3) = (q1[3], q1[2]);
    rotate1(&mut r2, &mut r3);
    q1[3] = r2;
    q1[2] = r3;

    let (mut r4, mut r5) = (q2[2], q2[0]);
    rotate1(&mut r4, &mut r5);
    q2[2] = r4;
    q2[0] = r5;

    let (mut r6, mut r7) = (q2[3], q2[1]);
    rotate1(&mut r6, &mut r7);
    q2[3] = r6;
    q2[1] = r7;

    // fwdOddOddPre(p1 + 0, p1 + 64, p1 + 16, p1 + 80) -> indices [3, 2, 1, 0] of q3
    let (d0, d1, d2, d3) = fwd_odd_odd_pre_vals(q3[3], q3[2], q3[1], q3[0]);
    q3[3] = d0;
    q3[2] = d1;
    q3[1] = d2;
    q3[0] = d3;

    // strDCT2x2dn(p0[o0], p1[o1], p0[o2], p1[o3])
    for k in 0..4 {
        let (a, b, c, d) = dct_2x2_dn_vals(q0[k], q1[k], q2[k], q3[k]);
        a0[idx_at(p0, o0[k])] = a;
        a1[idx_at(p1, o1[k])] = b;
        a0[idx_at(p0, o2[k])] = c;
        a1[idx_at(p1, o3[k])] = d;
    }
}

/// Forward 2-stage PCT + overlap filter (`transformMacroblock` in `strFwdTransform.c`) for single-tile soft boundaries.
#[allow(clippy::too_many_arguments)]
pub(super) fn transform_macroblock_enc(
    cf: ColorFormat,
    channels: usize,
    overlap: Overlap,
    scaled_arith: bool,
    cmb_width: usize,
    cmb_height: usize,
    c_row: usize,
    c_column: usize,
    a0: &mut [Vec<i32>; MAX_CHANNELS],
    a1: &mut [Vec<i32>; MAX_CHANNELS],
    pred_before: &mut [[i32; 2]; 2],
    pred_after: &mut [[i32; 2]; 2],
) {
    let left = c_column == 0;
    let right = c_column == cmb_width;
    let top = c_row == 0;
    let bottom = c_row == cmb_height;
    let left_or_right = left || right;
    let top_or_bottom = top || bottom;
    let top_or_left = left || top;
    let left_adj = c_column == 1;
    let right_adj = c_column + 1 == cmb_width;

    let full_planes = if matches!(cf, ColorFormat::Yuv420 | ColorFormat::Yuv422) {
        1
    } else {
        channels
    };

    for i in 0..full_planes {
        let p0 = (c_column * 256) as isize;
        let p1 = (c_column * 256) as isize;
        let buf0 = &mut a0[i];
        let buf1 = &mut a1[i];

        if overlap != Overlap::None {
            if top && left {
                str_pre4(buf1, p1, 0, 1, 2, 3);
            }
            if top && right {
                str_pre4(buf1, p1, -59, -60, -57, -58);
            }
            if bottom && left {
                str_pre4(buf0, p0, 58, 59, 56, 57);
            }
            if bottom && right {
                str_pre4(buf0, p0, -1, -2, -3, -4);
            }
            if !right && !bottom {
                let j_start = if left { 0isize } else { -64isize };
                if top {
                    let mut j = j_start;
                    while j < 192 {
                        str_pre4(buf1, p1 + j, 5, 4, 64, 65);
                        str_pre4(buf1, p1 + j, 7, 6, 66, 67);
                        j += 64;
                    }
                } else {
                    let mut j = j_start;
                    while j < 192 {
                        str_pre_4x4_stage1_split(buf0, p0 + 48 + j, buf1, p1 + j, 0);
                        j += 64;
                    }
                }

                if left {
                    if !top {
                        str_pre4_split(buf0, p0, 58, 56, buf1, p1, 0, 2);
                        str_pre4_split(buf0, p0, 59, 57, buf1, p1, 1, 3);
                    }
                    let mut j = -64isize;
                    while j < -16 {
                        str_pre4(buf1, p1 + j, 74, 72, 80, 82);
                        str_pre4(buf1, p1 + j, 75, 73, 81, 83);
                        j += 16;
                    }
                } else {
                    let mut j = -64isize;
                    while j < -16 {
                        str_pre_4x4_stage1(buf1, p1 + j, 0);
                        j += 16;
                    }
                }

                for off in [0isize, 16, 32, 64, 80, 96, 128, 144, 160] {
                    str_pre_4x4_stage1(buf1, p1 + off, 0);
                }
            }

            if bottom {
                let mut j = if left { 48isize } else { -16isize };
                let j_end = if right { -16isize } else { 240isize };
                while j < j_end {
                    str_pre4(buf0, p0 + j, 15, 14, 74, 75);
                    str_pre4(buf0, p0 + j, 13, 12, 72, 73);
                    j += 64;
                }
            }

            if right && !bottom {
                if !top {
                    str_pre4_split(buf0, p0, -1, -3, buf1, p1, -59, -57);
                    str_pre4_split(buf0, p0, -2, -4, buf1, p1, -60, -58);
                }
                let mut j = -64isize;
                while j < -16 {
                    str_pre4(buf1, p1 + j, 15, 13, 21, 23);
                    str_pre4(buf1, p1 + j, 14, 12, 20, 22);
                    j += 16;
                }
            }
        }

        if !top {
            let mut j = if left { 48isize } else { -16isize };
            let j_end = if right { 48isize } else { 240isize };
            while j < j_end {
                str_dct_4x4_stage1(buf0, p0 + j);
                j += 64;
            }
        }

        if !bottom {
            let mut j = if left { 0isize } else { -64isize };
            let j_end = if right { 0isize } else { 192isize };
            while j < j_end {
                str_dct_4x4_stage1(buf1, p1 + j);
                str_dct_4x4_stage1(buf1, p1 + j + 16);
                str_dct_4x4_stage1(buf1, p1 + j + 32);
                j += 64;
            }
        }

        if overlap == Overlap::Two {
            if top && left {
                str_pre4(buf1, p1, 0, 64, 16, 80);
            }
            if top && right {
                str_pre4(buf1, p1, -128, -64, -112, -48);
            }
            if bottom && left {
                str_pre4(buf0, p0, 32, 96, 48, 112);
            }
            if bottom && right {
                str_pre4(buf0, p0, -96, -32, -80, -16);
            }
            if left_or_right && !top_or_bottom {
                if left {
                    str_pre4_split(buf0, p0, 32, 48, buf1, p1, 0, 16);
                    str_pre4_split(buf0, p0, 96, 112, buf1, p1, 64, 80);
                }
                if right {
                    str_pre4_split(buf0, p0 - 128, 32, 48, buf1, p1 - 128, 0, 16);
                    str_pre4_split(buf0, p0 - 128, 96, 112, buf1, p1 - 128, 64, 80);
                }
            }
            if !left_or_right {
                if top_or_bottom {
                    if top {
                        str_pre4(buf1, p1, -128, -64, 0, 64);
                        str_pre4(buf1, p1, -112, -48, 16, 80);
                    }
                    if bottom {
                        str_pre4(buf0, p0 + 32, -128, -64, 0, 64);
                        str_pre4(buf0, p0 + 32, -112, -48, 16, 80);
                    }
                } else {
                    str_pre_4x4_stage2_split(buf0, p0, buf1, p1);
                }
            }
        }

        if !top_or_left {
            if scaled_arith {
                str_normalize_enc(buf0, p0 - 256, i != 0);
            }
            str_dct_4x4_second_stage(buf0, p0 - 256);
        }
    }

    if cf == ColorFormat::Yuv420 {
        for i in 0..2 {
            let p0 = (c_column * 64) as isize;
            let p1 = (c_column * 64) as isize;
            let buf0 = &mut a0[1 + i];
            let buf1 = &mut a1[1 + i];

            if overlap != Overlap::None {
                if top && left {
                    str_pre4(buf1, p1, 0, 1, 2, 3);
                }
                if top && right {
                    str_pre4(buf1, p1, -27, -28, -25, -26);
                }
                if bottom && left {
                    str_pre4(buf0, p0, 26, 27, 24, 25);
                }
                if bottom && right {
                    str_pre4(buf0, p0, -1, -2, -3, -4);
                }
                if !right && !bottom {
                    let j_start = if left { 0isize } else { -32isize };
                    if top {
                        let mut j = j_start;
                        while j < 32 {
                            str_pre4(buf1, p1 + j, 5, 4, 32, 33);
                            str_pre4(buf1, p1 + j, 7, 6, 34, 35);
                            j += 32;
                        }
                    } else {
                        let mut j = j_start;
                        while j < 32 {
                            str_pre_4x4_stage1_split(buf0, p0 + 16 + j, buf1, p1 + j, 32);
                            j += 32;
                        }
                    }

                    if left {
                        if !top {
                            str_pre4_split(buf0, p0, 26, 24, buf1, p1, 0, 2);
                            str_pre4_split(buf0, p0, 27, 25, buf1, p1, 1, 3);
                        }
                        str_pre4(buf1, p1, 10, 8, 16, 18);
                        str_pre4(buf1, p1, 11, 9, 17, 19);
                    } else {
                        str_pre_4x4_stage1(buf1, p1 - 32, 32);
                    }
                    str_pre_4x4_stage1(buf1, p1, 32);
                }

                if bottom {
                    let mut j = if left { 16isize } else { -16isize };
                    let j_end = if right { -16isize } else { 32isize };
                    while j < j_end {
                        str_pre4(buf0, p0 + j, 15, 14, 42, 43);
                        str_pre4(buf0, p0 + j, 13, 12, 40, 41);
                        j += 32;
                    }
                }

                if right && !bottom {
                    if !top {
                        str_pre4_split(buf0, p0, -1, -3, buf1, p1, -27, -25);
                        str_pre4_split(buf0, p0, -2, -4, buf1, p1, -28, -26);
                    }
                    str_pre4(buf1, p1, -17, -19, -11, -9);
                    str_pre4(buf1, p1, -18, -20, -12, -10);
                }
            }

            if !top {
                let mut j = if left { 16isize } else { -16isize };
                let j_end = if right { 16isize } else { 48isize };
                while j < j_end {
                    str_dct_4x4_stage1(buf0, p0 + j);
                    j += 32;
                }
            }

            if !bottom {
                let mut j = if left { 0isize } else { -32isize };
                let j_end = if right { 0isize } else { 32isize };
                while j < j_end {
                    str_dct_4x4_stage1(buf1, p1 + j);
                    j += 32;
                }
            }

            if overlap == Overlap::Two {
                if left_adj && top {
                    let v = buf1[idx_at(p1, -32)];
                    buf1[idx_at(p1, -64)] -= v;
                }
                if right_adj && top {
                    pred_before[i][0] = buf1[idx_at(p1, 0)];
                }
                if right && top {
                    buf1[idx_at(p1, -32)] -= pred_before[i][0];
                }
                if left_adj && bottom {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -48)] -= v;
                }
                if right_adj && bottom {
                    pred_before[i][1] = buf0[idx_at(p0, 16)];
                }
                if right && bottom {
                    buf0[idx_at(p0, -16)] -= pred_before[i][1];
                }

                if left_or_right && !top_or_bottom {
                    if left {
                        str_pre2_split(buf0, p0, 16, buf1, p1, 0);
                    }
                    if right {
                        str_pre2_split(buf0, p0, -16, buf1, p1, -32);
                    }
                }
                if !left_or_right {
                    if top_or_bottom {
                        if top {
                            str_pre2(buf1, p1, -32, 0);
                        }
                        if bottom {
                            str_pre2(buf0, p0, -16, 16);
                        }
                    } else {
                        str_pre2x2_split(buf0, p0, -16, 16, buf1, p1, -32, 0);
                    }
                }

                if left_adj && top {
                    let v = buf1[idx_at(p1, -32)];
                    buf1[idx_at(p1, -64)] += v;
                }
                if right_adj && top {
                    pred_after[i][0] = buf1[idx_at(p1, 0)];
                }
                if right && top {
                    buf1[idx_at(p1, -32)] += pred_after[i][0];
                }
                if left_adj && bottom {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -48)] += v;
                }
                if right_adj && bottom {
                    pred_after[i][1] = buf0[idx_at(p0, 16)];
                }
                if right && bottom {
                    buf0[idx_at(p0, -16)] += pred_after[i][1];
                }
            }

            if !top_or_left {
                if !scaled_arith {
                    dct_2x2_dn(buf0, p0, -64, -32, -48, -16);
                } else {
                    dct_2x2_dn_enc(buf0, p0, -64, -32, -48, -16);
                }
            }
        }
    }

    if cf == ColorFormat::Yuv422 {
        for i in 0..2 {
            let p0 = (c_column * 128) as isize;
            let p1 = (c_column * 128) as isize;
            let buf0 = &mut a0[1 + i];
            let buf1 = &mut a1[1 + i];

            if overlap != Overlap::None {
                if top && left {
                    str_pre4(buf1, p1, 0, 1, 2, 3);
                }
                if top && right {
                    str_pre4(buf1, p1, -59, -60, -57, -58);
                }
                if bottom && left {
                    str_pre4(buf0, p0, 58, 59, 56, 57);
                }
                if bottom && right {
                    str_pre4(buf0, p0, -1, -2, -3, -4);
                }
                if !right && !bottom {
                    let j_start = if left { 0isize } else { -64isize };
                    if top {
                        let mut j = j_start;
                        while j < 64 {
                            str_pre4(buf1, p1 + j, 5, 4, 64, 65);
                            str_pre4(buf1, p1 + j, 7, 6, 66, 67);
                            j += 64;
                        }
                    } else {
                        let mut j = j_start;
                        while j < 64 {
                            str_pre_4x4_stage1_split(buf0, p0 + 48 + j, buf1, p1 + j, 0);
                            j += 64;
                        }
                    }

                    if left {
                        if !top {
                            str_pre4_split(buf0, p0, 58, 56, buf1, p1, 0, 2);
                            str_pre4_split(buf0, p0, 59, 57, buf1, p1, 1, 3);
                        }
                        let mut j = 0isize;
                        while j < 48 {
                            str_pre4(buf1, p1 + j, 10, 8, 16, 18);
                            str_pre4(buf1, p1 + j, 11, 9, 17, 19);
                            j += 16;
                        }
                    } else {
                        let mut j = -64isize;
                        while j < -16 {
                            str_pre_4x4_stage1(buf1, p1 + j, 0);
                            j += 16;
                        }
                    }
                    str_pre_4x4_stage1(buf1, p1, 0);
                    str_pre_4x4_stage1(buf1, p1 + 16, 0);
                    str_pre_4x4_stage1(buf1, p1 + 32, 0);
                }

                if bottom {
                    let mut j = if left { 48isize } else { -16isize };
                    let j_end = if right { -16isize } else { 112isize };
                    while j < j_end {
                        str_pre4(buf0, p0 + j, 15, 14, 74, 75);
                        str_pre4(buf0, p0 + j, 13, 12, 72, 73);
                        j += 64;
                    }
                }

                if right && !bottom {
                    if !top {
                        str_pre4_split(buf0, p0, -1, -3, buf1, p1, -59, -57);
                        str_pre4_split(buf0, p0, -2, -4, buf1, p1, -60, -58);
                    }
                    let mut j = -64isize;
                    while j < -16 {
                        str_pre4(buf1, p1 + j, 15, 13, 21, 23);
                        str_pre4(buf1, p1 + j, 14, 12, 20, 22);
                        j += 16;
                    }
                }
            }

            if !top {
                let mut j = if left { 48isize } else { -16isize };
                let j_end = if right { 48isize } else { 112isize };
                while j < j_end {
                    str_dct_4x4_stage1(buf0, p0 + j);
                    j += 64;
                }
            }

            if !bottom {
                let mut j = if left { 0isize } else { -64isize };
                let j_end = if right { 0isize } else { 64isize };
                while j < j_end {
                    str_dct_4x4_stage1(buf1, p1 + j);
                    str_dct_4x4_stage1(buf1, p1 + j + 16);
                    str_dct_4x4_stage1(buf1, p1 + j + 32);
                    j += 64;
                }
            }

            if overlap == Overlap::Two {
                if left_adj && top {
                    let v = buf1[idx_at(p1, -64)];
                    buf1[idx_at(p1, -128)] -= v;
                }
                if right_adj && top {
                    pred_before[i][0] = buf1[idx_at(p1, 0)];
                }
                if right && top {
                    buf1[idx_at(p1, -64)] -= pred_before[i][0];
                }
                if left_adj && bottom {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -80)] -= v;
                }
                if right_adj && bottom {
                    pred_before[i][1] = buf0[idx_at(p0, 48)];
                }
                if right && bottom {
                    buf0[idx_at(p0, -16)] -= pred_before[i][1];
                }

                if !bottom {
                    if left_or_right {
                        if !top {
                            if left {
                                str_pre2_split(buf0, p0, 48, buf1, p1, 0);
                            }
                            if right {
                                str_pre2_split(buf0, p0, -16, buf1, p1, -64);
                            }
                        }
                        if left {
                            str_pre2(buf1, p1, 16, 32);
                        }
                        if right {
                            str_pre2(buf1, p1, -48, -32);
                        }
                    }
                    if !left_or_right {
                        if top {
                            str_pre2(buf1, p1, -64, 0);
                        } else {
                            str_pre2x2_split(buf0, p0, -16, 48, buf1, p1, -64, 0);
                        }
                        str_pre2x2(buf1, p1, -48, 16, -32, 32);
                    }
                }
                if bottom && !left_or_right {
                    str_pre2(buf0, p0, -16, 48);
                }

                if left_adj && top {
                    let v = buf1[idx_at(p1, -64)];
                    buf1[idx_at(p1, -128)] += v;
                }
                if right_adj && top {
                    pred_after[i][0] = buf1[idx_at(p1, 0)];
                }
                if right && top {
                    buf1[idx_at(p1, -64)] += pred_after[i][0];
                }
                if left_adj && bottom {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -80)] += v;
                }
                if right_adj && bottom {
                    pred_after[i][1] = buf0[idx_at(p0, 48)];
                }
                if right && bottom {
                    buf0[idx_at(p0, -16)] += pred_after[i][1];
                }
            }

            if !top_or_left {
                if !scaled_arith {
                    dct_2x2_dn(buf0, p0, -128, -64, -112, -48);
                    dct_2x2_dn(buf0, p0, -96, -32, -80, -16);
                } else {
                    dct_2x2_dn_enc(buf0, p0, -128, -64, -112, -48);
                    dct_2x2_dn_enc(buf0, p0, -96, -32, -80, -16);
                }
                let i96 = idx_at(p0, -96);
                let i128 = idx_at(p0, -128);
                buf0[i96] -= buf0[i128];
                buf0[i128] += (buf0[i96] + 1) >> 1;
            }
        }
    }
}

// Inverse PCT & Post-Filtering Operators (invTransformMacroblock_alteredOperators_hard)
