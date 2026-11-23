use super::pct_forward::{
    dct_2x2_dn, dct_2x2_dn_dec, dct_2x2_dn_vals, dct_2x2_up, idx_at, irotate1, irotate2,
};
use super::tables::*;
use super::*;

#[inline]
pub(super) fn inv_odd_odd(
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
    let (mut a, mut b, mut c, mut d) = (buf[i0], buf[i1], buf[i2], buf[i3]);
    d += a;
    c -= b;
    let t1 = d >> 1;
    a -= t1;
    let t2 = c >> 1;
    b += t2;
    a -= (b * 3 + 3) >> 3;
    b += (a * 3 + 3) >> 2;
    a -= (b * 3 + 4) >> 3;
    b -= t2;
    a += t1;
    c += b;
    d -= a;
    buf[i0] = a;
    buf[i1] = -b;
    buf[i2] = -c;
    buf[i3] = d;
}

#[inline]
pub(super) fn inv_odd_odd_post_vals(
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
    a -= (b * 3 + 6) >> 3;
    b += (a * 3 + 2) >> 2;
    a -= (b * 3 + 4) >> 3;
    b -= t2;
    a += t1;
    c += b;
    d -= a;
    (a, b, c, d)
}

#[inline]
pub(super) fn inv_odd(buf: &mut [i32], base: isize, o0: isize, o1: isize, o2: isize, o3: isize) {
    let (i0, i1, i2, i3) = (
        idx_at(base, o0),
        idx_at(base, o1),
        idx_at(base, o2),
        idx_at(base, o3),
    );
    let (mut a, mut b, mut c, mut d) = (buf[i0], buf[i1], buf[i2], buf[i3]);
    b += d;
    a -= c;
    d -= b >> 1;
    c += (a + 1) >> 1;
    irotate2(&mut a, &mut b);
    irotate2(&mut c, &mut d);
    c -= (b + 1) >> 1;
    d = ((a + 1) >> 1) - d;
    b += c;
    a -= d;
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

pub(super) fn str_idct_4x4_stage1(buf: &mut [i32], base: isize) {
    dct_2x2_up(buf, base, 0, 1, 2, 3);
    inv_odd(buf, base, 5, 4, 7, 6);
    inv_odd(buf, base, 10, 8, 11, 9);
    inv_odd_odd(buf, base, 15, 14, 13, 12);
    dct_2x2_dn(buf, base, 0, 4, 8, 12);
    dct_2x2_dn(buf, base, 1, 5, 9, 13);
    dct_2x2_dn(buf, base, 2, 6, 10, 14);
    dct_2x2_dn(buf, base, 3, 7, 11, 15);
}

pub(super) fn str_idct_4x4_stage2(buf: &mut [i32], base: isize) {
    inv_odd(buf, base, 32, 48, 96, 112);
    inv_odd(buf, base, 128, 192, 144, 208);
    inv_odd_odd(buf, base, 160, 224, 176, 240);
    dct_2x2_up(buf, base, 0, 64, 16, 80);
    dct_2x2_dn(buf, base, 0, 192, 48, 240);
    dct_2x2_dn(buf, base, 64, 128, 112, 176);
    dct_2x2_dn(buf, base, 16, 208, 32, 224);
    dct_2x2_dn(buf, base, 80, 144, 96, 160);
}

pub(super) fn str_normalize_dec(buf: &mut [i32], base: isize, is_chroma: bool) {
    if is_chroma {
        for off in (0..256isize).step_by(16) {
            let idx = idx_at(base, off);
            buf[idx] *= 2;
        }
    }
}

#[inline]
pub(super) fn str_hst_dec1_alt_vals(mut a: i32, mut d: i32) -> (i32, i32) {
    a += d;
    d = (a >> 1) - d;
    a += (d * 3) >> 3;
    d += (a * 3) >> 4;
    d += a >> 7;
    d -= a >> 10;
    (a, d)
}

#[inline]
pub(super) fn str_hst_dec1_edge(pa: &mut i32, pd: &mut i32) {
    let mut a = *pa;
    let mut d = *pd;
    a += d;
    d = (a >> 1) - d;
    a += (d * 3) >> 3;
    d += (a * 3) >> 4;
    d += a >> 7;
    d -= a >> 10;
    a += (d * 3 + 4) >> 3;
    d -= a >> 1;
    a += d;
    *pa = a;
    *pd = -d;
}

#[inline]
pub(super) fn str_hst_dec_vals(
    mut a: i32,
    mut b: i32,
    mut c: i32,
    mut d: i32,
) -> (i32, i32, i32, i32) {
    b -= c;
    a += (d * 3 + 4) >> 3;
    d -= b >> 1;
    c = ((a - b) >> 1) - c;
    (a - c, b + d, d, c)
}

#[inline]
pub(super) fn str_post4_alt_vals(
    mut a: i32,
    mut b: i32,
    mut c: i32,
    mut d: i32,
) -> (i32, i32, i32, i32) {
    a += d;
    b += c;
    d -= (a + 1) >> 1;
    c -= (b + 1) >> 1;
    str_hst_dec1_edge(&mut a, &mut d);
    str_hst_dec1_edge(&mut b, &mut c);
    irotate1(&mut c, &mut d);
    d += (a + 1) >> 1;
    c += (b + 1) >> 1;
    a -= d;
    b -= c;
    (a, b, c, d)
}

#[inline]
pub(super) fn str_post4_alt(
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
    let (a, b, c, d) = str_post4_alt_vals(buf[i0], buf[i1], buf[i2], buf[i3]);
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
pub(super) fn str_post4_alt_split(
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
    let (a, b, c, d) = str_post4_alt_vals(a0[i0], a0[i1], a1[i2], a1[i3]);
    a0[i0] = a;
    a0[i1] = b;
    a1[i2] = c;
    a1[i3] = d;
}

#[inline]
pub(super) fn str_post2_alt_vals(mut a: i32, mut b: i32) -> (i32, i32) {
    b += (a + 2) >> 2;
    a += (b + 1) >> 1;
    a += b >> 5;
    a += b >> 9;
    a += b >> 13;
    b += (a + 2) >> 2;
    (a, b)
}

#[inline]
pub(super) fn str_post2_alt(buf: &mut [i32], base: isize, o0: isize, o1: isize) {
    let (i0, i1) = (idx_at(base, o0), idx_at(base, o1));
    let (a, b) = str_post2_alt_vals(buf[i0], buf[i1]);
    buf[i0] = a;
    buf[i1] = b;
}

#[inline]
pub(super) fn str_post2_alt_split(
    a0: &mut [i32],
    b0: isize,
    o0: isize,
    a1: &mut [i32],
    b1: isize,
    o1: isize,
) {
    let (i0, i1) = (idx_at(b0, o0), idx_at(b1, o1));
    let (a, b) = str_post2_alt_vals(a0[i0], a1[i1]);
    a0[i0] = a;
    a1[i1] = b;
}

#[inline]
pub(super) fn str_post2x2_alt_vals(
    mut a: i32,
    mut b: i32,
    mut c: i32,
    mut d: i32,
) -> (i32, i32, i32, i32) {
    a += d;
    b += c;
    d -= (a + 1) >> 1;
    c -= (b + 1) >> 1;
    b += (a + 2) >> 2;
    a += (b + 1) >> 1;
    a += b >> 5;
    a += b >> 9;
    a += b >> 13;
    b += (a + 2) >> 2;
    d += (a + 1) >> 1;
    c += (b + 1) >> 1;
    a -= d;
    b -= c;
    (a, b, c, d)
}

#[inline]
pub(super) fn str_post2x2_alt(
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
    let (a, b, c, d) = str_post2x2_alt_vals(buf[i0], buf[i1], buf[i2], buf[i3]);
    buf[i0] = a;
    buf[i1] = b;
    buf[i2] = c;
    buf[i3] = d;
}

#[inline]
pub(super) fn str_post2x2_alt_split(
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
    let (a, b, c, d) = str_post2x2_alt_vals(a0[i0], a0[i1], a1[i2], a1[i3]);
    a0[i0] = a;
    a0[i1] = b;
    a1[i2] = c;
    a1[i3] = d;
}

pub(super) fn str_post_4x4_core_alt(
    q0: &mut [i32; 4],
    q1: &mut [i32; 4],
    q2: &mut [i32; 4],
    q3: &mut [i32; 4],
) {
    for k in 0..4 {
        let (a, b, c, d) = dct_2x2_dn_vals(q0[k], q2[k], q1[k], q3[k]);
        q0[k] = a;
        q2[k] = b;
        q1[k] = c;
        q3[k] = d;
    }

    let (d0, d1, d2, d3) = inv_odd_odd_post_vals(q3[0], q3[1], q3[2], q3[3]);
    *q3 = [d0, d1, d2, d3];

    let (mut c12, mut c13) = (q1[2], q1[3]);
    irotate1(&mut c12, &mut c13);
    q1[2] = c12;
    q1[3] = c13;

    let (mut c10, mut c11) = (q1[0], q1[1]);
    irotate1(&mut c10, &mut c11);
    q1[0] = c10;
    q1[1] = c11;

    let (mut c21, mut c23) = (q2[1], q2[3]);
    irotate1(&mut c21, &mut c23);
    q2[1] = c21;
    q2[3] = c23;

    let (mut c20, mut c22) = (q2[0], q2[2]);
    irotate1(&mut c20, &mut c22);
    q2[0] = c20;
    q2[2] = c22;

    for k in 0..4 {
        let (a, d) = str_hst_dec1_alt_vals(q0[k], q3[k]);
        let (a, b, c, d) = str_hst_dec_vals(a, q2[k], q1[k], d);
        q0[k] = a;
        q2[k] = b;
        q1[k] = c;
        q3[k] = d;
    }
}

pub(super) fn str_post_4x4_stage1_alt(buf: &mut [i32], base: isize, offset: isize) {
    let p0 = idx_at(base, 12);
    let p1 = idx_at(base, 20);
    let p2 = idx_at(base, 72 - offset);
    let p3 = idx_at(base, 80 - offset);
    let mut q0 = [buf[p0], buf[p0 + 1], buf[p0 + 2], buf[p0 + 3]];
    let mut q1 = [buf[p1], buf[p1 + 1], buf[p1 + 2], buf[p1 + 3]];
    let mut q2 = [buf[p2], buf[p2 + 1], buf[p2 + 2], buf[p2 + 3]];
    let mut q3 = [buf[p3], buf[p3 + 1], buf[p3 + 2], buf[p3 + 3]];
    str_post_4x4_core_alt(&mut q0, &mut q1, &mut q2, &mut q3);
    buf[p0..p0 + 4].copy_from_slice(&q0);
    buf[p1..p1 + 4].copy_from_slice(&q1);
    buf[p2..p2 + 4].copy_from_slice(&q2);
    buf[p3..p3 + 4].copy_from_slice(&q3);
}

pub(super) fn str_post_4x4_stage1_split_alt(
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
    str_post_4x4_core_alt(&mut q0, &mut q1, &mut q2, &mut q3);
    a0[p0..p0 + 4].copy_from_slice(&q0);
    a0[p2..p2 + 4].copy_from_slice(&q2);
    a1[p1..p1 + 4].copy_from_slice(&q1);
    a1[p3..p3 + 4].copy_from_slice(&q3);
}

pub(super) fn str_post_4x4_stage2_split_alt(a0: &mut [i32], p0: isize, a1: &mut [i32], p1: isize) {
    let o0 = [-96isize, -32, -80, -16];
    let o2 = [96isize, 32, 112, 48];
    let o1 = [-112isize, -48, -128, -64];
    let o3 = [80isize, 16, 64, 0];

    let mut q0 = [0i32; 4];
    let mut q1 = [0i32; 4];
    let mut q2 = [0i32; 4];
    let mut q3 = [0i32; 4];
    for k in 0..4 {
        let (a, b, c, d) = dct_2x2_dn_vals(
            a0[idx_at(p0, o0[k])],
            a0[idx_at(p0, o2[k])],
            a1[idx_at(p1, o1[k])],
            a1[idx_at(p1, o3[k])],
        );
        q0[k] = a;
        q2[k] = b;
        q1[k] = c;
        q3[k] = d;
    }

    let (d0, d1, d2, d3) = inv_odd_odd_post_vals(q3[3], q3[2], q3[1], q3[0]);
    q3[3] = d0;
    q3[2] = d1;
    q3[1] = d2;
    q3[0] = d3;

    let (mut r0, mut r1) = (q2[3], q2[1]);
    irotate1(&mut r0, &mut r1);
    q2[3] = r0;
    q2[1] = r1;

    let (mut r2, mut r3) = (q2[2], q2[0]);
    irotate1(&mut r2, &mut r3);
    q2[2] = r2;
    q2[0] = r3;

    let (mut r4, mut r5) = (q1[3], q1[2]);
    irotate1(&mut r4, &mut r5);
    q1[3] = r4;
    q1[2] = r5;

    let (mut r6, mut r7) = (q1[1], q1[0]);
    irotate1(&mut r6, &mut r7);
    q1[1] = r6;
    q1[0] = r7;

    for k in 0..4 {
        let (a, d) = str_hst_dec1_alt_vals(q0[k], q3[k]);
        let (a, b, c, d) = str_hst_dec_vals(a, q1[k], q2[k], d);
        a0[idx_at(p0, o0[k])] = a;
        a1[idx_at(p1, o1[k])] = b;
        a0[idx_at(p0, o2[k])] = c;
        a1[idx_at(p1, o3[k])] = d;
    }
}

/// Inverse 2-stage PCT + overlap filter (`invTransformMacroblock_alteredOperators_hard` in `strInvTransform.c`).
#[allow(clippy::too_many_arguments)]
pub(super) fn inv_transform_macroblock_dec(
    cf: ColorFormat,
    channels: usize,
    overlap: Overlap,
    scaled_arith: bool,
    cmb_width: usize,
    cmb_height: usize,
    c_row: usize,
    c_column: usize,
    b_vert_tb: bool,
    b_hori_tb: bool,
    b_one_left_vtb: bool,
    b_one_right_vtb: bool,
    a0: &mut [Vec<i32>; MAX_CHANNELS],
    a1: &mut [Vec<i32>; MAX_CHANNELS],
    pred_before: &mut [[i32; 2]; 2],
    pred_after: &mut [[i32; 2]; 2],
) {
    let left = c_column == 0;
    let right = c_column == cmb_width;
    let top = c_row == 0;
    let bottom = c_row == cmb_height;
    let top_or_bottom = top || bottom;
    let left_or_right = left || right;
    let bottom_or_right = bottom || right;
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

        if !bottom_or_right {
            str_idct_4x4_stage2(buf1, p1);
            if scaled_arith {
                str_normalize_dec(buf1, p1, i != 0);
            }
        }

        if overlap == Overlap::Two {
            if (top || b_hori_tb) && (left || b_vert_tb) {
                str_post4_alt(buf1, p1, 0, 64, 16, 80);
            }
            if (top || b_hori_tb) && (right || b_vert_tb) {
                str_post4_alt(buf1, p1, -128, -64, -112, -48);
            }
            if (bottom || b_hori_tb) && (left || b_vert_tb) {
                str_post4_alt(buf0, p0, 32, 96, 48, 112);
            }
            if (bottom || b_hori_tb) && (right || b_vert_tb) {
                str_post4_alt(buf0, p0, -96, -32, -80, -16);
            }
            if (left_or_right || b_vert_tb) && !top_or_bottom && !b_hori_tb {
                if left || b_vert_tb {
                    str_post4_alt_split(buf0, p0, 32, 48, buf1, p1, 0, 16);
                    str_post4_alt_split(buf0, p0, 96, 112, buf1, p1, 64, 80);
                }
                if right || b_vert_tb {
                    str_post4_alt_split(buf0, p0 - 128, 32, 48, buf1, p1 - 128, 0, 16);
                    str_post4_alt_split(buf0, p0 - 128, 96, 112, buf1, p1 - 128, 64, 80);
                }
            }
            if !left_or_right {
                if (top_or_bottom || b_hori_tb) && !b_vert_tb {
                    if top || b_hori_tb {
                        str_post4_alt(buf1, p1, -128, -64, 0, 64);
                        str_post4_alt(buf1, p1, -112, -48, 16, 80);
                    }
                    if bottom || b_hori_tb {
                        str_post4_alt(buf0, p0 + 32, -128, -64, 0, 64);
                        str_post4_alt(buf0, p0 + 32, -112, -48, 16, 80);
                    }
                }
                if !top_or_bottom && !b_hori_tb && !b_vert_tb {
                    str_post_4x4_stage2_split_alt(buf0, p0, buf1, p1);
                }
            }
        }

        if !top {
            let mut j = if left { 32isize } else { -96isize };
            let j_end = if right { 32isize } else { 160isize };
            while j < j_end {
                str_idct_4x4_stage1(buf0, p0 + j);
                str_idct_4x4_stage1(buf0, p0 + j + 16);
                j += 64;
            }
        }

        if !bottom {
            let mut j = if left { 0isize } else { -128isize };
            let j_end = if right { 0isize } else { 128isize };
            while j < j_end {
                str_idct_4x4_stage1(buf1, p1 + j);
                str_idct_4x4_stage1(buf1, p1 + j + 16);
                j += 64;
            }
        }

        if overlap != Overlap::None {
            if left_or_right || b_vert_tb {
                if (top || b_hori_tb) && (left || b_vert_tb) {
                    str_post4_alt(buf1, p1, 0, 1, 2, 3);
                }
                if (top || b_hori_tb) && (right || b_vert_tb) {
                    str_post4_alt(buf1, p1, -59, -60, -57, -58);
                }
                if (bottom || b_hori_tb) && (left || b_vert_tb) {
                    str_post4_alt(buf0, p0, 58, 59, 56, 57);
                }
                if (bottom || b_hori_tb) && (right || b_vert_tb) {
                    str_post4_alt(buf0, p0, -1, -2, -3, -4);
                }
                if left || b_vert_tb {
                    let j = 10isize;
                    if !top {
                        let base = p0 + 16 + j;
                        str_post4_alt(buf0, base, 0, -2, 6, 8);
                        str_post4_alt(buf0, base, 1, -1, 7, 9);
                        str_post4_alt(buf0, base, 16, 14, 22, 24);
                        str_post4_alt(buf0, base, 17, 15, 23, 25);
                    }
                    if !bottom {
                        let base = p1 + j;
                        str_post4_alt(buf1, base, 0, -2, 6, 8);
                        str_post4_alt(buf1, base, 1, -1, 7, 9);
                    }
                    if !top_or_bottom && !b_hori_tb {
                        str_post4_alt_split(buf0, p0, 48 + j, 48 + j - 2, buf1, p1, j - 10, j - 8);
                        str_post4_alt_split(
                            buf0,
                            p0,
                            48 + j + 1,
                            48 + j - 1,
                            buf1,
                            p1,
                            j - 9,
                            j - 7,
                        );
                    }
                }
                if right || b_vert_tb {
                    let j = -64isize + 14;
                    if !top {
                        let base = p0 + 16 + j;
                        str_post4_alt(buf0, base, 0, -2, 6, 8);
                        str_post4_alt(buf0, base, 1, -1, 7, 9);
                        str_post4_alt(buf0, base, 16, 14, 22, 24);
                        str_post4_alt(buf0, base, 17, 15, 23, 25);
                    }
                    if !bottom {
                        let base = p1 + j;
                        str_post4_alt(buf1, base, 0, -2, 6, 8);
                        str_post4_alt(buf1, base, 1, -1, 7, 9);
                    }
                    if !top_or_bottom && !b_hori_tb {
                        str_post4_alt_split(buf0, p0, 48 + j, 48 + j - 2, buf1, p1, j - 10, j - 8);
                        str_post4_alt_split(
                            buf0,
                            p0,
                            48 + j + 1,
                            48 + j - 1,
                            buf1,
                            p1,
                            j - 9,
                            j - 7,
                        );
                    }
                }
            }

            if top || b_hori_tb {
                let mut j = if left { 0isize } else { -192isize };
                let j_end = if right { -64isize } else { 64isize };
                while j < j_end {
                    if !b_vert_tb || j != -64 {
                        str_post4_alt(buf1, p1 + j, 5, 4, 64, 65);
                        str_post4_alt(buf1, p1 + j, 7, 6, 66, 67);
                        str_post_4x4_stage1_alt(buf1, p1 + j, 0);
                    }
                    j += 64;
                }
            }

            if bottom || b_hori_tb {
                let mut j = if left { 0isize } else { -192isize };
                let j_end = if right { -64isize } else { 64isize };
                while j < j_end {
                    if !b_vert_tb || j != -64 {
                        str_post_4x4_stage1_alt(buf0, p0 + 16 + j, 0);
                        str_post_4x4_stage1_alt(buf0, p0 + 32 + j, 0);
                        str_post4_alt(buf0, p0 + 48 + j, 15, 14, 74, 75);
                        str_post4_alt(buf0, p0 + 48 + j, 13, 12, 72, 73);
                    }
                    j += 64;
                }
            }

            if !top && !bottom && !b_hori_tb {
                let mut j = if left { 0isize } else { -192isize };
                let j_end = if right { -64isize } else { 64isize };
                while j < j_end {
                    if !b_vert_tb || j != -64 {
                        str_post_4x4_stage1_alt(buf0, p0 + 16 + j, 0);
                        str_post_4x4_stage1_alt(buf0, p0 + 32 + j, 0);
                        str_post_4x4_stage1_split_alt(buf0, p0 + 48 + j, buf1, p1 + j, 0);
                        str_post_4x4_stage1_alt(buf1, p1 + j, 0);
                    }
                    j += 64;
                }
            }
        }
    }

    if cf == ColorFormat::Yuv420 {
        for i in 0..2 {
            let p0 = (c_column * 64) as isize;
            let p1 = (c_column * 64) as isize;
            let buf0 = &mut a0[1 + i];
            let buf1 = &mut a1[1 + i];

            if !bottom_or_right {
                if !scaled_arith {
                    dct_2x2_dn(buf1, p1, 0, 32, 16, 48);
                } else {
                    dct_2x2_dn_dec(buf1, p1, 0, 32, 16, 48);
                }
            }

            if overlap == Overlap::Two {
                if (left_adj || b_one_right_vtb) && (top || b_hori_tb) {
                    let v = buf1[idx_at(p1, -32)];
                    buf1[idx_at(p1, -64)] -= v;
                }
                if (right_adj || b_one_left_vtb) && (top || b_hori_tb) {
                    pred_before[i][0] = buf1[idx_at(p1, 0)];
                }
                if (right || b_vert_tb) && (top || b_hori_tb) {
                    buf1[idx_at(p1, -32)] -= pred_before[i][0];
                }
                if (left_adj || b_one_right_vtb) && (bottom || b_hori_tb) {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -48)] -= v;
                }
                if (right_adj || b_one_left_vtb) && (bottom || b_hori_tb) {
                    pred_before[i][1] = buf0[idx_at(p0, 16)];
                }
                if (right || b_vert_tb) && (bottom || b_hori_tb) {
                    buf0[idx_at(p0, -16)] -= pred_before[i][1];
                }

                if (left_or_right || b_vert_tb) && !top_or_bottom && !b_hori_tb {
                    if left || b_vert_tb {
                        str_post2_alt_split(buf0, p0, 16, buf1, p1, 0);
                    }
                    if right || b_vert_tb {
                        str_post2_alt_split(buf0, p0, -16, buf1, p1, -32);
                    }
                }
                if !left_or_right {
                    if (top_or_bottom || b_hori_tb) && !b_vert_tb {
                        if top || b_hori_tb {
                            str_post2_alt(buf1, p1, -32, 0);
                        }
                        if bottom || b_hori_tb {
                            str_post2_alt(buf0, p0, -16, 16);
                        }
                    } else if !top_or_bottom && !b_hori_tb && !b_vert_tb {
                        str_post2x2_alt_split(buf0, p0, -16, 16, buf1, p1, -32, 0);
                    }
                }

                if (left_adj || b_one_right_vtb) && (top || b_hori_tb) {
                    let v = buf1[idx_at(p1, -32)];
                    buf1[idx_at(p1, -64)] += v;
                }
                if (right_adj || b_one_left_vtb) && (top || b_hori_tb) {
                    pred_after[i][0] = buf1[idx_at(p1, 0)];
                }
                if (right || b_vert_tb) && (top || b_hori_tb) {
                    buf1[idx_at(p1, -32)] += pred_after[i][0];
                }
                if (left_adj || b_one_right_vtb) && (bottom || b_hori_tb) {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -48)] += v;
                }
                if (right_adj || b_one_left_vtb) && (bottom || b_hori_tb) {
                    pred_after[i][1] = buf0[idx_at(p0, 16)];
                }
                if (right || b_vert_tb) && (bottom || b_hori_tb) {
                    buf0[idx_at(p0, -16)] += pred_after[i][1];
                }
            }

            if !top {
                let mut j = if left {
                    48isize
                } else if left_adj || b_one_right_vtb {
                    -48isize
                } else {
                    -16isize
                };
                let j_end = if right || b_vert_tb { 16isize } else { 48isize };
                while j < j_end {
                    str_idct_4x4_stage1(buf0, p0 + j);
                    j += 32;
                }
            }

            if !bottom {
                let mut j = if left {
                    32isize
                } else if left_adj || b_one_right_vtb {
                    -64isize
                } else {
                    -32isize
                };
                let j_end = if right || b_vert_tb { 0isize } else { 32isize };
                while j < j_end {
                    str_idct_4x4_stage1(buf1, p1 + j);
                    j += 32;
                }
            }

            if overlap != Overlap::None {
                if (top || b_hori_tb) && (left_adj || b_one_right_vtb) {
                    str_post4_alt(buf1, p1 - 64, 0, 1, 2, 3);
                }
                if (top || b_hori_tb) && (right || b_vert_tb) {
                    str_post4_alt(buf1, p1, -27, -28, -25, -26);
                }
                if (bottom || b_hori_tb) && (left_adj || b_one_right_vtb) {
                    str_post4_alt(buf0, p0 - 64, 26, 27, 24, 25);
                }
                if (bottom || b_hori_tb) && (right || b_vert_tb) {
                    str_post4_alt(buf0, p0, -1, -2, -3, -4);
                }
                if !left && !top {
                    if left_adj || b_one_right_vtb {
                        if !bottom && !b_hori_tb {
                            str_post4_alt_split(buf0, p0 - 64, 26, 24, buf1, p1 - 64, 0, 2);
                            str_post4_alt_split(buf0, p0 - 64, 27, 25, buf1, p1 - 64, 1, 3);
                        }
                        str_post4_alt(buf0, p0 - 64, 10, 8, 16, 18);
                        str_post4_alt(buf0, p0 - 64, 11, 9, 17, 19);
                    }
                    if bottom || b_hori_tb {
                        str_post4_alt(buf0, p0 - 48, 15, 14, 42, 43);
                        str_post4_alt(buf0, p0 - 48, 13, 12, 40, 41);
                        if !right && !b_vert_tb {
                            str_post4_alt(buf0, p0 - 16, 15, 14, 42, 43);
                            str_post4_alt(buf0, p0 - 16, 13, 12, 40, 41);
                        }
                    } else {
                        str_post_4x4_stage1_split_alt(buf0, p0 - 48, buf1, p1 - 64, 32);
                        if !right && !b_vert_tb {
                            str_post_4x4_stage1_split_alt(buf0, p0 - 16, buf1, p1 - 32, 32);
                        }
                    }

                    if right || b_vert_tb {
                        if !bottom && !b_hori_tb {
                            str_post4_alt_split(buf0, p0, -2, -4, buf1, p1, -28, -26);
                            str_post4_alt_split(buf0, p0, -1, -3, buf1, p1, -27, -25);
                        }
                        str_post4_alt(buf0, p0, -18, -20, -12, -10);
                        str_post4_alt(buf0, p0, -17, -19, -11, -9);
                    } else {
                        str_post_4x4_stage1_alt(buf0, p0 - 32, 32);
                    }
                    str_post_4x4_stage1_alt(buf0, p0 - 64, 32);
                }

                if top || b_hori_tb {
                    if !left {
                        let base = p1 - 64 + 4;
                        str_post4_alt(buf1, base, 1, 0, 28, 29);
                        str_post4_alt(buf1, base, 3, 2, 30, 31);
                    }
                    if !left && !right && !b_vert_tb {
                        let base = p1 - 32 + 4;
                        str_post4_alt(buf1, base, 1, 0, 28, 29);
                        str_post4_alt(buf1, base, 3, 2, 30, 31);
                    }
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

            if !bottom_or_right {
                let i0 = idx_at(p1, 0);
                let i32 = idx_at(p1, 32);
                buf1[i0] -= (buf1[i32] + 1) >> 1;
                buf1[i32] += buf1[i0];

                if !scaled_arith {
                    dct_2x2_dn(buf1, p1, 0, 64, 16, 80);
                    dct_2x2_dn(buf1, p1, 32, 96, 48, 112);
                } else {
                    dct_2x2_dn_dec(buf1, p1, 0, 64, 16, 80);
                    dct_2x2_dn_dec(buf1, p1, 32, 96, 48, 112);
                }
            }

            if overlap == Overlap::Two {
                if (left_adj || b_one_right_vtb) && (top || b_hori_tb) {
                    let v = buf1[idx_at(p1, -64)];
                    buf1[idx_at(p1, -128)] -= v;
                }
                if (right_adj || b_one_left_vtb) && (top || b_hori_tb) {
                    pred_before[i][0] = buf1[idx_at(p1, 0)];
                }
                if (right || b_vert_tb) && (top || b_hori_tb) {
                    buf1[idx_at(p1, -64)] -= pred_before[i][0];
                }
                if (left_adj || b_one_right_vtb) && (bottom || b_hori_tb) {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -80)] -= v;
                }
                if (right_adj || b_one_left_vtb) && (bottom || b_hori_tb) {
                    pred_before[i][1] = buf0[idx_at(p0, 48)];
                }
                if (right || b_vert_tb) && (bottom || b_hori_tb) {
                    buf0[idx_at(p0, -16)] -= pred_before[i][1];
                }

                if !bottom {
                    if left_or_right || b_vert_tb {
                        if !top && !b_hori_tb {
                            if left || b_vert_tb {
                                str_post2_alt_split(buf0, p0, 48, buf1, p1, 0);
                            }
                            if right || b_vert_tb {
                                str_post2_alt_split(buf0, p0, -16, buf1, p1, -64);
                            }
                        }
                        if left || b_vert_tb {
                            str_post2_alt(buf1, p1, 16, 32);
                        }
                        if right || b_vert_tb {
                            str_post2_alt(buf1, p1, -48, -32);
                        }
                    }
                    if !left_or_right && !b_vert_tb {
                        if top || b_hori_tb {
                            str_post2_alt(buf1, p1, -64, 0);
                        } else {
                            str_post2x2_alt_split(buf0, p0, -16, 48, buf1, p1, -64, 0);
                        }
                        str_post2x2_alt(buf1, p1, -48, 16, -32, 32);
                    }
                }
                if (bottom || b_hori_tb) && !left_or_right && !b_vert_tb {
                    str_post2_alt(buf0, p0, -16, 48);
                }

                if (left_adj || b_one_right_vtb) && (top || b_hori_tb) {
                    let v = buf1[idx_at(p1, -64)];
                    buf1[idx_at(p1, -128)] += v;
                }
                if (right_adj || b_one_left_vtb) && (top || b_hori_tb) {
                    pred_after[i][0] = buf1[idx_at(p1, 0)];
                }
                if (right || b_vert_tb) && (top || b_hori_tb) {
                    buf1[idx_at(p1, -64)] += pred_after[i][0];
                }
                if (left_adj || b_one_right_vtb) && (bottom || b_hori_tb) {
                    let v = buf0[idx_at(p0, -16)];
                    buf0[idx_at(p0, -80)] += v;
                }
                if (right_adj || b_one_left_vtb) && (bottom || b_hori_tb) {
                    pred_after[i][1] = buf0[idx_at(p0, 48)];
                }
                if (right || b_vert_tb) && (bottom || b_hori_tb) {
                    buf0[idx_at(p0, -16)] += pred_after[i][1];
                }
            }

            if !top {
                let mut j = if left {
                    112isize
                } else if left_adj || b_one_right_vtb {
                    -80isize
                } else {
                    -16isize
                };
                let j_end = if right || b_vert_tb {
                    48isize
                } else {
                    112isize
                };
                while j < j_end {
                    str_idct_4x4_stage1(buf0, p0 + j);
                    j += 64;
                }
            }

            if !bottom {
                let mut j = if left {
                    64isize
                } else if left_adj || b_one_right_vtb {
                    -128isize
                } else {
                    -64isize
                };
                let j_end = if right || b_vert_tb { 0isize } else { 64isize };
                while j < j_end {
                    str_idct_4x4_stage1(buf1, p1 + j);
                    str_idct_4x4_stage1(buf1, p1 + j + 16);
                    str_idct_4x4_stage1(buf1, p1 + j + 32);
                    j += 64;
                }
            }

            if overlap != Overlap::None {
                if (top || b_hori_tb) && (left_adj || b_one_right_vtb) {
                    str_post4_alt(buf1, p1 - 128, 0, 1, 2, 3);
                }
                if (top || b_hori_tb) && (right || b_vert_tb) {
                    str_post4_alt(buf1, p1, -59, -60, -57, -58);
                }
                if (bottom || b_hori_tb) && (left_adj || b_one_right_vtb) {
                    str_post4_alt(buf0, p0 - 128, 58, 59, 56, 57);
                }
                if (bottom || b_hori_tb) && (right || b_vert_tb) {
                    str_post4_alt(buf0, p0, -1, -2, -3, -4);
                }
                if !top {
                    if left_adj || b_one_right_vtb {
                        let base = p0 + 42 - 128;
                        str_post4_alt(buf0, base, 0, -2, 6, 8);
                        str_post4_alt(buf0, base, 1, -1, 7, 9);
                    }
                    if right || b_vert_tb {
                        let base = p0 - 18;
                        str_post4_alt(buf0, base, 0, -2, 6, 8);
                        str_post4_alt(buf0, base, 1, -1, 7, 9);
                    }
                    let mut j = if left { 0isize } else { -128isize };
                    let j_end = if right || b_vert_tb { -64isize } else { 0isize };
                    while j < j_end {
                        str_post_4x4_stage1_alt(buf0, p0 + j + 32, 0);
                        j += 64;
                    }
                }

                if !bottom {
                    if left_adj || b_one_right_vtb {
                        let base = p1 + 10 - 128;
                        str_post4_alt(buf1, base, 0, -2, 6, 8);
                        str_post4_alt(buf1, base, 1, -1, 7, 9);
                        str_post4_alt(buf1, base + 16, 0, -2, 6, 8);
                        str_post4_alt(buf1, base + 16, 1, -1, 7, 9);
                    }
                    if right || b_vert_tb {
                        let base = p1 - 50;
                        str_post4_alt(buf1, base, 0, -2, 6, 8);
                        str_post4_alt(buf1, base, 1, -1, 7, 9);
                        str_post4_alt(buf1, base + 16, 0, -2, 6, 8);
                        str_post4_alt(buf1, base + 16, 1, -1, 7, 9);
                    }
                    let mut j = if left { 0isize } else { -128isize };
                    let j_end = if right || b_vert_tb { -64isize } else { 0isize };
                    while j < j_end {
                        str_post_4x4_stage1_alt(buf1, p1 + j, 0);
                        str_post_4x4_stage1_alt(buf1, p1 + j + 16, 0);
                        j += 64;
                    }
                }

                if top_or_bottom || b_hori_tb {
                    if top || b_hori_tb {
                        let mut j = if left { 0isize } else { -128isize };
                        let j_end = if right || b_vert_tb { -64isize } else { 0isize };
                        while j < j_end {
                            let base = p1 + 5 + j;
                            str_post4_alt(buf1, base, 0, -1, 59, 60);
                            str_post4_alt(buf1, base, 2, 1, 61, 62);
                            j += 64;
                        }
                    }
                    if bottom || b_hori_tb {
                        let mut j = if left { 0isize } else { -128isize };
                        let j_end = if right || b_vert_tb { -64isize } else { 0isize };
                        while j < j_end {
                            let base = p0 + 61 + j;
                            str_post4_alt(buf0, base, 0, -1, 59, 60);
                            str_post4_alt(buf0, base, 2, 1, 61, 62);
                            j += 64;
                        }
                    }
                } else {
                    if left_adj || b_one_right_vtb {
                        let j = -128isize;
                        str_post4_alt_split(buf0, p0 + j, 58, 56, buf1, p1 + j, 0, 2);
                        str_post4_alt_split(buf0, p0 + j, 59, 57, buf1, p1 + j, 1, 3);
                    }
                    if right || b_vert_tb {
                        let j = -60isize;
                        str_post4_alt_split(buf0, p0 + j, 58, 56, buf1, p1 + j, 0, 2);
                        str_post4_alt_split(buf0, p0 + j, 59, 57, buf1, p1 + j, 1, 3);
                    }
                    let mut j = if left { 0isize } else { -128isize };
                    let j_end = if right || b_vert_tb { -64isize } else { 0isize };
                    while j < j_end {
                        str_post_4x4_stage1_split_alt(buf0, p0 + j + 48, buf1, p1 + j, 0);
                        j += 64;
                    }
                }
            }
        }
    }
}
