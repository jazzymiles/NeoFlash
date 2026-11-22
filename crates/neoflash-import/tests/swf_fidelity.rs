//! Comprehensive SWF import, native bytecode playback (`SwfBytecodePlayer`), editable
//! `.nflash` reconstruction, determinism, and located diagnostics suite (`T-325`).

use flate2::{Compression, write::ZlibEncoder};
use neoflash_core::{BlendMode, FilterSpec, LayerRole, MaskMode, Project};
use neoflash_import::{
    ImportFormat, Severity, SwfBytecodePlayer, SwfCompression, SwfVmKind, import_project, inspect,
};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct TempDir(PathBuf);
impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "neoflash-swf-fidelity-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct BitWriter {
    buf: u128,
    len: u32,
}
impl BitWriter {
    fn new() -> Self {
        Self { buf: 0, len: 0 }
    }
    fn push(&mut self, val: u64, n: u32) {
        let mask = if n == 64 { u64::MAX } else { (1u64 << n) - 1 };
        self.buf = (self.buf << n) | u128::from(val & mask);
        self.len += n;
    }
    fn finish(mut self) -> Vec<u8> {
        let pad = (8 - (self.len % 8)) % 8;
        if pad > 0 {
            self.push(0, pad);
        }
        let total_bytes = (self.len / 8) as usize;
        let mut out = Vec::with_capacity(total_bytes);
        for i in (0..total_bytes).rev() {
            out.push(((self.buf >> (i * 8)) & 0xff) as u8);
        }
        out
    }
}

struct LzmaRangeEncoder {
    out: Vec<u8>,
    low: u64,
    range: u32,
    cache_size: u32,
    cache: u8,
}
impl LzmaRangeEncoder {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            low: 0,
            range: 0xFFFF_FFFF,
            cache_size: 1,
            cache: 0,
        }
    }
    fn shift_low(&mut self) {
        let low_hi = (self.low >> 32) as u32;
        if (self.low as u32) < 0xFF00_0000 || low_hi != 0 {
            let mut temp = self.cache;
            loop {
                self.out.push(temp.wrapping_add(low_hi as u8));
                temp = 0xFF;
                self.cache_size -= 1;
                if self.cache_size == 0 {
                    break;
                }
            }
            self.cache = ((self.low as u32) >> 24) as u8;
        }
        self.cache_size += 1;
        self.low = (self.low & 0x00FF_FFFF) << 8;
    }
    fn encode_bit(&mut self, prob: &mut u16, bit: bool) {
        let bound = (self.range >> 11) * u32::from(*prob);
        if !bit {
            self.range = bound;
            *prob += (0x800_u16 - *prob) >> 5;
        } else {
            self.low += u64::from(bound);
            self.range -= bound;
            *prob -= *prob >> 5;
        }
        if self.range < 0x0100_0000 {
            self.range <<= 8;
            self.shift_low();
        }
    }
    fn finish(mut self) -> Vec<u8> {
        for _ in 0..5 {
            self.shift_low();
        }
        self.out
    }
}

fn compress_zws_lzma(uncompressed: &[u8]) -> ([u8; 5], Vec<u8>) {
    let props = [0x5d, 0x00, 0x00, 0x01, 0x00];
    let mut rc = LzmaRangeEncoder::new();
    let mut is_match = [0x400_u16; 192];
    let mut literal_probs = [[0x400_u16; 0x300]; 8];
    for (pos, &byte) in uncompressed.iter().enumerate() {
        let pos_state = pos & 3;
        rc.encode_bit(&mut is_match[pos_state], false);
        let prev_byte = if pos > 0 { uncompressed[pos - 1] } else { 0 };
        let lit_state = usize::from(prev_byte >> 5);
        let probs = &mut literal_probs[lit_state];
        let mut symbol = 1usize;
        for bit_idx in (0..8).rev() {
            let bit = ((byte >> bit_idx) & 1) != 0;
            rc.encode_bit(&mut probs[symbol], bit);
            symbol = (symbol << 1) ^ usize::from(bit);
        }
    }
    (props, rc.finish())
}

fn swf_tag(out: &mut Vec<u8>, code: u16, payload: &[u8]) {
    if payload.len() < 0x3f {
        let header = (code << 6) | (payload.len() as u16);
        out.extend_from_slice(&header.to_le_bytes());
    } else {
        let header = (code << 6) | 0x3f;
        out.extend_from_slice(&header.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    }
    out.extend_from_slice(payload);
}

fn swf_body(frame_count: u16, tags: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    let mut bw = BitWriter::new();
    bw.push(16, 5);
    bw.push(0, 16);
    bw.push(8000, 16); // 400 px
    bw.push(0, 16);
    bw.push(6000, 16); // 300 px
    body.extend_from_slice(&bw.finish());
    body.extend_from_slice(&0x1800_u16.to_le_bytes()); // 24 fps
    body.extend_from_slice(&frame_count.to_le_bytes());
    body.extend_from_slice(tags);
    swf_tag(&mut body, 0, &[]);
    body
}

fn wrap_swf(compression: SwfCompression, version: u8, frame_count: u16, tags: &[u8]) -> Vec<u8> {
    let body = swf_body(frame_count, tags);
    let declared_len = (8 + body.len()) as u32;
    match compression {
        SwfCompression::Uncompressed => {
            let mut out = vec![b'F', b'W', b'S', version];
            out.extend_from_slice(&declared_len.to_le_bytes());
            out.extend_from_slice(&body);
            out
        }
        SwfCompression::Zlib => {
            let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
            enc.write_all(&body).unwrap();
            let compressed = enc.finish().unwrap();
            let mut out = vec![b'C', b'W', b'S', version.max(6)];
            out.extend_from_slice(&declared_len.to_le_bytes());
            out.extend_from_slice(&compressed);
            out
        }
        SwfCompression::Lzma => {
            let (props, compressed) = compress_zws_lzma(&body);
            let mut out = vec![b'Z', b'W', b'S', version.max(13)];
            out.extend_from_slice(&declared_len.to_le_bytes());
            out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
            out.extend_from_slice(&props);
            out.extend_from_slice(&compressed);
            out
        }
    }
}

fn swf_matrix_bytes(sx: Option<f64>, sy: Option<f64>, tx_px: f64, ty_px: f64) -> Vec<u8> {
    let mut bw = BitWriter::new();
    if let (Some(a), Some(d)) = (sx, sy) {
        bw.push(1, 1);
        bw.push(20, 5);
        let a_fb = (a * 65536.0).round() as i32 as i64 as u64;
        let d_fb = (d * 65536.0).round() as i32 as i64 as u64;
        bw.push(a_fb, 20);
        bw.push(d_fb, 20);
    } else {
        bw.push(0, 1);
    }
    bw.push(0, 1);
    let tx_twips = (tx_px * 20.0).round() as i32;
    let ty_twips = (ty_px * 20.0).round() as i32;
    bw.push(16, 5);
    bw.push(tx_twips as i64 as u64, 16);
    bw.push(ty_twips as i64 as u64, 16);
    bw.finish()
}

fn define_square_shape_tag(shape_id: u16, size_px: u16, rgb: [u8; 3]) -> Vec<u8> {
    let tw = i32::from(size_px) * 20;
    let mut p = Vec::new();
    p.extend_from_slice(&shape_id.to_le_bytes());
    let mut rect = BitWriter::new();
    rect.push(15, 5);
    rect.push(0, 15);
    rect.push(tw as u64, 15);
    rect.push(0, 15);
    rect.push(tw as u64, 15);
    p.extend_from_slice(&rect.finish());
    p.push(1); // 1 fill style
    p.push(0x00); // solid
    p.extend_from_slice(&rgb);
    p.push(0); // 0 line styles
    p.push(0x10); // NumFillBits=1, NumLineBits=0

    let mut bw = BitWriter::new();
    bw.push(0b0_00101, 6); // FillStyle1 + MoveTo
    bw.push(0, 5);
    bw.push(1, 1); // FillStyle1 = 1
    for (vert, delta) in [(0u64, tw), (1, tw), (0, -tw), (1, -tw)] {
        bw.push(0b11, 2);
        bw.push(12, 4); // nbits = 14
        bw.push(0, 1);
        bw.push(vert, 1);
        bw.push(delta as i64 as u64, 14);
    }
    bw.push(0, 6);
    p.extend_from_slice(&bw.finish());
    p
}

fn avm1_push_str(out: &mut Vec<u8>, s: &str) {
    out.push(0x96);
    let mut p = vec![0x00];
    p.extend_from_slice(s.as_bytes());
    p.push(0);
    out.extend_from_slice(&(p.len() as u16).to_le_bytes());
    out.extend_from_slice(&p);
}

fn avm1_push_int(out: &mut Vec<u8>, v: i32) {
    out.push(0x96);
    out.extend_from_slice(&5_u16.to_le_bytes());
    out.push(0x07);
    out.extend_from_slice(&v.to_le_bytes());
}

fn avm1_push_f64(out: &mut Vec<u8>, v: f64) {
    out.push(0x96);
    out.extend_from_slice(&9_u16.to_le_bytes());
    out.push(0x06);
    let b = v.to_le_bytes();
    // SWF ActionPush type 6 swaps the two 32-bit halves of little-endian IEEE-754 double.
    out.extend_from_slice(&[b[4], b[5], b[6], b[7], b[0], b[1], b[2], b[3]]);
}

fn avm1_push_null(out: &mut Vec<u8>) {
    out.push(0x96);
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.push(0x02);
}

fn avm1_push_undef(out: &mut Vec<u8>) {
    out.push(0x96);
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.push(0x03);
}

fn avm1_push_reg(out: &mut Vec<u8>, reg: u8) {
    out.push(0x96);
    out.extend_from_slice(&2_u16.to_le_bytes());
    out.push(0x04);
    out.push(reg);
}

fn write_u30(out: &mut Vec<u8>, mut val: u32) {
    loop {
        let mut byte = (val & 0x7F) as u8;
        val >>= 7;
        if val != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if val == 0 {
            break;
        }
    }
}

fn write_s24(out: &mut Vec<u8>, offset: i32) {
    let b = offset.to_le_bytes();
    out.extend_from_slice(&[b[0], b[1], b[2]]);
}

#[derive(Default)]
struct MiniAbcBuilder {
    ints: Vec<i32>,
    uints: Vec<u32>,
    doubles: Vec<f64>,
    strings: Vec<String>,
    namespaces: Vec<(u8, u32)>,
    ns_sets: Vec<Vec<u32>>,
    multinames: Vec<Vec<u8>>,
}

impl MiniAbcBuilder {
    fn str_idx(&mut self, s: &str) -> u32 {
        if let Some(pos) = self.strings.iter().position(|x| x == s) {
            return (pos + 1) as u32;
        }
        self.strings.push(s.to_owned());
        self.strings.len() as u32
    }

    fn int_idx(&mut self, v: i32) -> u32 {
        if let Some(pos) = self.ints.iter().position(|&x| x == v) {
            return (pos + 1) as u32;
        }
        self.ints.push(v);
        self.ints.len() as u32
    }

    fn uint_idx(&mut self, v: u32) -> u32 {
        if let Some(pos) = self.uints.iter().position(|&x| x == v) {
            return (pos + 1) as u32;
        }
        self.uints.push(v);
        self.uints.len() as u32
    }

    fn double_idx(&mut self, v: f64) -> u32 {
        if let Some(pos) = self.doubles.iter().position(|x| x.to_bits() == v.to_bits()) {
            return (pos + 1) as u32;
        }
        self.doubles.push(v);
        self.doubles.len() as u32
    }

    fn pub_ns(&mut self, pkg: &str) -> u32 {
        let s = self.str_idx(pkg);
        if let Some(pos) = self
            .namespaces
            .iter()
            .position(|&(k, id)| k == 0x16 && id == s)
        {
            return (pos + 1) as u32;
        }
        self.namespaces.push((0x16, s));
        self.namespaces.len() as u32
    }

    fn ns_set(&mut self, items: &[u32]) -> u32 {
        if let Some(pos) = self.ns_sets.iter().position(|x| x.as_slice() == items) {
            return (pos + 1) as u32;
        }
        self.ns_sets.push(items.to_vec());
        self.ns_sets.len() as u32
    }

    fn qname(&mut self, pkg: &str, name: &str) -> u32 {
        let ns = self.pub_ns(pkg);
        let nm = self.str_idx(name);
        let mut entry = vec![0x07];
        write_u30(&mut entry, ns);
        write_u30(&mut entry, nm);
        if let Some(pos) = self.multinames.iter().position(|x| x == &entry) {
            return (pos + 1) as u32;
        }
        self.multinames.push(entry);
        self.multinames.len() as u32
    }

    fn multiname_l(&mut self, pkg: &str) -> u32 {
        let ns = self.pub_ns(pkg);
        let set = self.ns_set(&[ns]);
        let mut entry = vec![0x1B]; // MultinameL
        write_u30(&mut entry, set);
        if let Some(pos) = self.multinames.iter().position(|x| x == &entry) {
            return (pos + 1) as u32;
        }
        self.multinames.push(entry);
        self.multinames.len() as u32
    }

    fn write_constant_pool(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&16_u16.to_le_bytes());
        out.extend_from_slice(&46_u16.to_le_bytes());
        if self.ints.is_empty() {
            write_u30(out, 0);
        } else {
            write_u30(out, (self.ints.len() + 1) as u32);
            for &v in &self.ints {
                write_u30(out, v as u32);
            }
        }
        if self.uints.is_empty() {
            write_u30(out, 0);
        } else {
            write_u30(out, (self.uints.len() + 1) as u32);
            for &v in &self.uints {
                write_u30(out, v);
            }
        }
        if self.doubles.is_empty() {
            write_u30(out, 0);
        } else {
            write_u30(out, (self.doubles.len() + 1) as u32);
            for &v in &self.doubles {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        write_u30(out, (self.strings.len() + 1) as u32);
        for s in &self.strings {
            write_u30(out, s.len() as u32);
            out.extend_from_slice(s.as_bytes());
        }
        write_u30(out, (self.namespaces.len() + 1) as u32);
        for &(kind, s_idx) in &self.namespaces {
            out.push(kind);
            write_u30(out, s_idx);
        }
        if self.ns_sets.is_empty() {
            write_u30(out, 0);
        } else {
            write_u30(out, (self.ns_sets.len() + 1) as u32);
            for set in &self.ns_sets {
                write_u30(out, set.len() as u32);
                for &ns in set {
                    write_u30(out, ns);
                }
            }
        }
        write_u30(out, (self.multinames.len() + 1) as u32);
        for mn in &self.multinames {
            out.extend_from_slice(mn);
        }
    }
}

#[test]
fn fws_cws_zws_containers_across_as1_as2_as3_import_validate_save_reopen_and_play() {
    let temp = TempDir::new("containers");

    // 1. AS1 (SWF 5 base): ActionAdd + ActionTrace
    let mut as1_tags = Vec::new();
    swf_tag(
        &mut as1_tags,
        2,
        &define_square_shape_tag(1, 30, [220, 50, 50]),
    );
    let mut po2 = vec![0x26]; // HasName | HasMatrix | HasCharacter
    po2.extend_from_slice(&1_u16.to_le_bytes());
    po2.extend_from_slice(&1_u16.to_le_bytes());
    po2.extend_from_slice(&swf_matrix_bytes(None, None, 15.0, 20.0));
    po2.extend_from_slice(b"sq\0");
    swf_tag(&mut as1_tags, 26, &po2);
    let mut as1_act = Vec::new();
    avm1_push_int(&mut as1_act, 19);
    avm1_push_int(&mut as1_act, 23);
    as1_act.extend_from_slice(&[0x0A, 0x26, 0x07, 0x00]); // ActionAdd -> 42, Trace, Stop
    swf_tag(&mut as1_tags, 12, &as1_act);
    swf_tag(&mut as1_tags, 1, &[]);

    // 2. AS2 (SWF 8 base): DefineFunction2 + CallFunction
    let mut as2_tags = Vec::new();
    swf_tag(&mut as2_tags, 69, &[0x00, 0x00, 0x00, 0x00]);
    swf_tag(
        &mut as2_tags,
        2,
        &define_square_shape_tag(1, 24, [40, 180, 90]),
    );
    swf_tag(&mut as2_tags, 26, &po2);
    let mut fn_body = Vec::new();
    avm1_push_reg(&mut fn_body, 1);
    avm1_push_int(&mut fn_body, 8);
    fn_body.extend_from_slice(&[0x47, 0x3E]); // Add2, Return
    let mut as2_act = vec![0x8E]; // DefineFunction2
    let mut fn_hdr = Vec::new();
    fn_hdr.extend_from_slice(b"bump\0");
    fn_hdr.extend_from_slice(&1_u16.to_le_bytes()); // num_params = 1
    fn_hdr.push(2); // register_count = 2
    fn_hdr.extend_from_slice(&0_u16.to_le_bytes()); // flags = 0
    fn_hdr.push(1); // param 0 in reg 1
    fn_hdr.extend_from_slice(b"v\0");
    fn_hdr.extend_from_slice(&(fn_body.len() as u16).to_le_bytes());
    as2_act.extend_from_slice(&(fn_hdr.len() as u16).to_le_bytes());
    as2_act.extend_from_slice(&fn_hdr);
    as2_act.extend_from_slice(&fn_body);
    avm1_push_int(&mut as2_act, 50);
    avm1_push_int(&mut as2_act, 1);
    avm1_push_str(&mut as2_act, "bump");
    as2_act.extend_from_slice(&[0x3D, 0x26, 0x07, 0x00]); // CallFunction, Trace -> "58", Stop
    swf_tag(&mut as2_tags, 12, &as2_act);
    swf_tag(&mut as2_tags, 1, &[]);

    // 3. AS3 (SWF 10 base): DoABC2 + SymbolClass
    let mut pool = MiniAbcBuilder::default();
    let mn_main = pool.qname("", "MainTimeline");
    let mn_mc = pool.qname("flash.display", "MovieClip");
    let mn_trace = pool.qname("", "trace");
    let s_ok = pool.str_idx("as3-77");
    let mut abc = Vec::new();
    pool.write_constant_pool(&mut abc);
    write_u30(&mut abc, 3);
    for _ in 0..3 {
        abc.extend_from_slice(&[0, 0, 0, 0]);
    }
    write_u30(&mut abc, 0);
    write_u30(&mut abc, 1);
    write_u30(&mut abc, mn_main);
    write_u30(&mut abc, mn_mc);
    abc.extend_from_slice(&[0x01, 0, 0, 0]); // dynamic, 0 ifaces, iinit=0, 0 traits
    abc.extend_from_slice(&[1, 0]); // cinit=1, 0 traits
    abc.extend_from_slice(&[1, 2, 1]); // 1 script, init=2, 1 trait
    write_u30(&mut abc, mn_main);
    abc.extend_from_slice(&[0x04, 1, 0]); // class trait slot 1 class 0
    write_u30(&mut abc, 3);
    let mut iinit = vec![0xD0, 0x30, 0x5D];
    write_u30(&mut iinit, mn_trace);
    iinit.push(0x2C);
    write_u30(&mut iinit, s_ok);
    iinit.push(0x4F);
    write_u30(&mut iinit, mn_trace);
    write_u30(&mut iinit, 1);
    iinit.push(0x47);
    abc.extend_from_slice(&[0, 4, 1, 1, 3]);
    write_u30(&mut abc, iinit.len() as u32);
    abc.extend_from_slice(&iinit);
    abc.extend_from_slice(&[0, 0]);
    abc.extend_from_slice(&[1, 1, 1, 1, 2, 1, 0x47, 0, 0]);
    abc.extend_from_slice(&[2, 1, 1, 1, 2, 1, 0x47, 0, 0]);
    let mut do_abc = vec![1, 0, 0, 0];
    do_abc.extend_from_slice(b"frame\0");
    do_abc.extend_from_slice(&abc);
    let mut as3_tags = Vec::new();
    swf_tag(&mut as3_tags, 69, &[0x08, 0x00, 0x00, 0x00]);
    swf_tag(
        &mut as3_tags,
        2,
        &define_square_shape_tag(1, 24, [60, 110, 230]),
    );
    swf_tag(&mut as3_tags, 26, &po2);
    swf_tag(&mut as3_tags, 82, &do_abc);
    let mut sym_cls = vec![1, 0, 0, 0];
    sym_cls.extend_from_slice(b"MainTimeline\0");
    swf_tag(&mut as3_tags, 76, &sym_cls);
    swf_tag(&mut as3_tags, 1, &[]);

    for (lang_label, base_ver, tags, expected_vm, expected_trace) in [
        ("as1", 5_u8, &as1_tags, SwfVmKind::Avm1, "42"),
        ("as2", 8_u8, &as2_tags, SwfVmKind::Avm1, "58"),
        ("as3", 10_u8, &as3_tags, SwfVmKind::Avm2, "as3-77"),
    ] {
        for (comp_label, comp) in [
            ("fws", SwfCompression::Uncompressed),
            ("cws", SwfCompression::Zlib),
            ("zws", SwfCompression::Lzma),
        ] {
            let swf_bytes = wrap_swf(comp, base_ver, 1, tags);
            let path = temp.0.join(format!("{lang_label}-{comp_label}.swf"));
            fs::write(&path, &swf_bytes).unwrap();

            let report = inspect(&path).unwrap();
            assert_eq!(report.format, ImportFormat::Swf);
            assert!(
                report.can_import,
                "{lang_label}/{comp_label}: {:?}",
                report.diagnostics
            );
            assert!(
                !report
                    .diagnostics
                    .iter()
                    .any(|d| d.severity == Severity::Error),
                "{:?}",
                report.diagnostics
            );
            let swf_info = report.swf.as_ref().unwrap();
            assert_eq!(swf_info.compression, comp);

            let project = import_project(&path).unwrap();
            project.validate().unwrap();

            // .nflash JSON save / reopen round-trip fidelity
            let nflash_json = project.to_json().unwrap();
            let reopened = Project::from_json(&nflash_json).unwrap();
            reopened.validate().unwrap();
            assert_eq!(reopened.symbols.len(), project.symbols.len());
            assert_eq!(reopened.sources.len(), project.sources.len());

            // Native SWF bytecode playback across FWS/CWS/ZWS
            let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes).unwrap();
            assert_eq!(player.vm_kind(), expected_vm);
            let state = player.step_frame();
            assert_eq!(
                state.traces,
                vec![expected_trace.to_owned()],
                "{lang_label}/{comp_label} trace mismatch"
            );
            assert_eq!(state.display_list.len(), 1);
        }
    }
}

#[test]
fn filters_blend_modes_static_clip_depth_and_avm1_avm2_dynamic_masks() {
    let temp = TempDir::new("filters-masks");

    let fixed16 = |v: f64| -> [u8; 4] { ((v * 65536.0).round() as i32).to_le_bytes() };
    let fixed8 = |v: f64| -> [u8; 2] { ((v * 256.0).round() as i16).to_le_bytes() };

    let mut f0 = vec![0_u8, 10, 20, 30, 204]; // DropShadow
    f0.extend_from_slice(&fixed16(4.0));
    f0.extend_from_slice(&fixed16(6.0));
    f0.extend_from_slice(&fixed16(std::f64::consts::FRAC_PI_4));
    f0.extend_from_slice(&fixed16(5.0));
    f0.extend_from_slice(&fixed8(1.5));
    f0.push(0x20 | 2); // CompositeSource + 2 passes

    let mut f1 = vec![1_u8]; // Blur
    f1.extend_from_slice(&fixed16(8.0));
    f1.extend_from_slice(&fixed16(4.0));
    f1.push(3 << 3);

    let mut f2 = vec![2_u8, 255, 80, 40, 255]; // Glow
    f2.extend_from_slice(&fixed16(6.0));
    f2.extend_from_slice(&fixed16(6.0));
    f2.extend_from_slice(&fixed8(2.0));
    f2.push(0x80 | 0x20 | 2); // Inner + CompositeSource + 2 passes

    let mut f3 = vec![3_u8, 0, 0, 0, 255, 255, 255, 255, 255]; // Bevel
    f3.extend_from_slice(&fixed16(3.0));
    f3.extend_from_slice(&fixed16(3.0));
    f3.extend_from_slice(&fixed16(0.5));
    f3.extend_from_slice(&fixed16(4.0));
    f3.extend_from_slice(&fixed8(1.0));
    f3.push(0x10 | 0x20 | 2); // OnTop ("full") + 2 passes

    let build_grad_filter = |id: u8| -> Vec<u8> {
        let mut g = vec![id, 2, 255, 0, 0, 255, 0, 128, 255, 200, 0, 255];
        g.extend_from_slice(&fixed16(5.0));
        g.extend_from_slice(&fixed16(5.0));
        g.extend_from_slice(&fixed16(0.25));
        g.extend_from_slice(&fixed16(3.0));
        g.extend_from_slice(&fixed8(1.25));
        g.push(0x80 | 2); // inner, 2 passes
        g
    };
    let f4 = build_grad_filter(4); // GradientGlow
    let f7 = build_grad_filter(7); // GradientBevel

    let mut f5 = vec![5_u8, 3, 3]; // Convolution 3x3
    f5.extend_from_slice(&1.0_f32.to_le_bytes()); // divisor
    f5.extend_from_slice(&0.0_f32.to_le_bytes()); // bias
    for idx in 0..9 {
        let v = if idx == 4 { 1.0_f32 } else { 0.0_f32 };
        f5.extend_from_slice(&v.to_le_bytes());
    }
    f5.extend_from_slice(&[0, 0, 0, 255]); // default RGBA
    f5.push(0x03); // Clamp | PreserveAlpha

    let mut f6 = vec![6_u8]; // ColorMatrix (20 floats)
    for r in 0..4 {
        for c in 0..5 {
            let v = if r == c { 1.0_f32 } else { 0.0_f32 };
            f6.extend_from_slice(&v.to_le_bytes());
        }
    }

    let mut tags = Vec::new();
    swf_tag(&mut tags, 2, &define_square_shape_tag(1, 40, [240, 60, 60]));
    // Sprite 2 wraps shape 1 so instance placements receive filters & blend modes
    let mut sp2 = Vec::new();
    sp2.extend_from_slice(&2_u16.to_le_bytes());
    sp2.extend_from_slice(&1_u16.to_le_bytes());
    let mut sp2_po = vec![0x06];
    sp2_po.extend_from_slice(&1_u16.to_le_bytes());
    sp2_po.extend_from_slice(&1_u16.to_le_bytes());
    sp2_po.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    swf_tag(&mut sp2, 26, &sp2_po);
    swf_tag(&mut sp2, 1, &[]);
    swf_tag(&mut sp2, 0, &[]);
    swf_tag(&mut tags, 39, &sp2);

    // DefineButton2 (char 3) with HasFilterList (0x10) + HasBlendMode (0x20) on button record
    let mut btn = Vec::new();
    btn.extend_from_slice(&3_u16.to_le_bytes());
    btn.push(0x00); // TrackAsMenu=0
    btn.extend_from_slice(&0_u16.to_le_bytes()); // ActionOffset=0
    btn.push(0x3F); // HasBlendMode(0x20) | HasFilterList(0x10) | Hit|Down|Over|Up
    btn.extend_from_slice(&1_u16.to_le_bytes()); // char 1
    btn.extend_from_slice(&1_u16.to_le_bytes()); // depth 1
    btn.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    btn.push(0x00); // empty CXFORMWITHALPHA
    btn.push(1); // 1 filter on button record (Blur)
    btn.extend_from_slice(&f1);
    btn.push(4); // BlendMode::Screen
    btn.push(0x00); // end button records
    swf_tag(&mut tags, 34, &btn);

    let place_po3 = |depth: u16,
                     cid: u16,
                     name: &str,
                     clip_depth: Option<u16>,
                     raw_filters: &[Vec<u8>],
                     blend: Option<u8>|
     -> Vec<u8> {
        let mut flags1 = 0x26_u8; // HasName | HasMatrix | HasCharacter
        if clip_depth.is_some() {
            flags1 |= 0x40;
        }
        let mut flags2 = 0_u8;
        if !raw_filters.is_empty() {
            flags2 |= 0x01;
        }
        if blend.is_some() {
            flags2 |= 0x02;
        }
        let mut p = vec![flags1, flags2];
        p.extend_from_slice(&depth.to_le_bytes());
        p.extend_from_slice(&cid.to_le_bytes());
        p.extend_from_slice(&swf_matrix_bytes(None, None, f64::from(depth) * 10.0, 10.0));
        p.extend_from_slice(name.as_bytes());
        p.push(0);
        if let Some(cd) = clip_depth {
            p.extend_from_slice(&cd.to_le_bytes());
        }
        if !raw_filters.is_empty() {
            p.push(raw_filters.len() as u8);
            for rf in raw_filters {
                p.extend_from_slice(rf);
            }
        }
        if let Some(bm) = blend {
            p.push(bm);
        }
        p
    };

    // Place two sprites carrying all 8 representable filter kinds (4 each)
    swf_tag(
        &mut tags,
        70,
        &place_po3(1, 2, "flt_a", None, &[f0, f1, f2, f3], Some(3)),
    );
    swf_tag(
        &mut tags,
        70,
        &place_po3(2, 2, "flt_b", None, &[f4, f5, f6, f7], Some(14)),
    );
    // Place button 3 at depth 3
    swf_tag(&mut tags, 70, &place_po3(3, 3, "btn_flt", None, &[], None));

    // Place all 14 SWF blend modes across depths 10..=23
    for bm_code in 1_u8..=14 {
        swf_tag(
            &mut tags,
            70,
            &place_po3(
                9 + u16::from(bm_code),
                2,
                &format!("bm_{bm_code}"),
                None,
                &[],
                Some(bm_code),
            ),
        );
    }

    // Static clipDepth mask at depth 30 clipping depths 31 and 32
    swf_tag(
        &mut tags,
        70,
        &place_po3(30, 1, "static_mask", Some(32), &[], None),
    );
    swf_tag(
        &mut tags,
        70,
        &place_po3(31, 2, "masked_1", None, &[], None),
    );
    swf_tag(
        &mut tags,
        70,
        &place_po3(32, 2, "masked_2", None, &[], None),
    );

    // Empty clipDepth range (depth 40..=40) + unknown filter (99) + unknown blend (99) -> located diagnostics!
    swf_tag(
        &mut tags,
        70,
        &place_po3(40, 1, "empty_mask", Some(40), &[], None),
    );
    swf_tag(
        &mut tags,
        70,
        &place_po3(41, 2, "unk_flt_bm", None, &[vec![99_u8]], Some(99)),
    );
    // Out-of-range ConvolutionFilter (divisor = 100_000.0, matrix = [100_000.0]) -> clamped to +/-65536 so validate() succeeds!
    let mut big_conv = vec![5_u8, 1, 1];
    big_conv.extend_from_slice(&100_000.0_f32.to_le_bytes()); // divisor
    big_conv.extend_from_slice(&100_000.0_f32.to_le_bytes()); // bias
    big_conv.extend_from_slice(&100_000.0_f32.to_le_bytes()); // cell[0]
    big_conv.extend_from_slice(&[0, 0, 0, 255, 0x03]);
    swf_tag(
        &mut tags,
        70,
        &place_po3(42, 2, "clamped_conv", None, &[big_conv], None),
    );
    swf_tag(&mut tags, 1, &[]);

    let swf_bytes = wrap_swf(SwfCompression::Uncompressed, 8, 1, &tags);
    let swf_path = temp.0.join("filters_blends_masks.swf");
    fs::write(&swf_path, &swf_bytes).unwrap();

    let report = inspect(&swf_path).unwrap();
    assert!(report.can_import, "{:?}", report.diagnostics);
    for expected_code in [
        "swf_filter_approximated",
        "swf_blend_mode_approximated",
        "swf_mask_approximated",
    ] {
        let diag = report
            .diagnostics
            .iter()
            .find(|d| d.code == expected_code)
            .unwrap_or_else(|| panic!("missing {expected_code} in {:?}", report.diagnostics));
        assert!(
            diag.location.contains("#tag=") || diag.location.contains("depth-"),
            "diagnostic {expected_code} must carry concrete location, got: {}",
            diag.location
        );
    }

    let project = import_project(&swf_path).unwrap();
    project.validate().unwrap();
    let root = project.symbol(&project.root).unwrap();

    // Verify all 8 filter kinds reconstructed on root elements
    let all_filters: Vec<&FilterSpec> = root
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .flat_map(|e| &e.filters)
        .collect();
    assert!(
        all_filters
            .iter()
            .any(|f| matches!(f, FilterSpec::DropShadow { .. }))
    );
    assert!(
        all_filters
            .iter()
            .any(|f| matches!(f, FilterSpec::Blur { .. }))
    );
    assert!(
        all_filters
            .iter()
            .any(|f| matches!(f, FilterSpec::Glow { .. }))
    );
    assert!(
        all_filters
            .iter()
            .any(|f| matches!(f, FilterSpec::Bevel { .. }))
    );
    assert!(
        all_filters
            .iter()
            .any(|f| matches!(f, FilterSpec::GradientGlow { .. }))
    );
    assert!(
        all_filters
            .iter()
            .any(|f| matches!(f, FilterSpec::Convolution { .. }))
    );
    assert!(
        all_filters
            .iter()
            .any(|f| matches!(f, FilterSpec::ColorMatrix { .. }))
    );
    assert!(
        all_filters
            .iter()
            .any(|f| matches!(f, FilterSpec::GradientBevel { .. }))
    );

    // Verify DefineButton2 record preserved its Blur filter & Screen blend mode
    let btn_sym = project
        .symbols
        .iter()
        .find(|s| s.id == "swf-sym-3")
        .expect("button symbol");
    let btn_has_filter = btn_sym
        .timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .any(|e| {
            e.blend_mode == BlendMode::Screen
                && e.filters
                    .iter()
                    .any(|f| matches!(f, FilterSpec::Blur { .. }))
        });
    assert!(
        btn_has_filter,
        "DefineButton2 state filters/blend must be preserved"
    );

    // Verify all 14 blend modes reconstructed
    for expected_bm in [
        BlendMode::Normal,
        BlendMode::Layer,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Lighten,
        BlendMode::Darken,
        BlendMode::Difference,
        BlendMode::Add,
        BlendMode::Subtract,
        BlendMode::Invert,
        BlendMode::Alpha,
        BlendMode::Erase,
        BlendMode::Overlay,
        BlendMode::Hardlight,
    ] {
        assert!(
            root.timeline
                .layers
                .iter()
                .flat_map(|l| &l.keyframes)
                .flat_map(|k| &k.elements)
                .any(|e| e.blend_mode == expected_bm),
            "missing blend mode {expected_bm:?}"
        );
    }

    // Verify static clipDepth reconstructed as LayerRole::Mask + 2 masked layers
    let mask_layer = root
        .timeline
        .layers
        .iter()
        .find(|l| l.properties.role == LayerRole::Mask)
        .expect("static clipDepth mask layer");
    assert_eq!(mask_layer.properties.mask_mode, MaskMode::Geometry);
    let masked_count = root
        .timeline
        .layers
        .iter()
        .filter(|l| l.properties.mask.as_deref() == Some(mask_layer.id.as_str()))
        .count();
    assert_eq!(masked_count, 2);

    // Verify player matches filters, blend modes, and static clip_depth
    let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes).unwrap();
    let st = player.step_frame();
    assert_eq!(st.display_list[0].filters.len(), 4);
    assert_eq!(st.display_list[1].filters.len(), 4);
    let mask_slot = st
        .display_list
        .iter()
        .find(|e| e.name == "static_mask")
        .unwrap();
    assert_eq!(mask_slot.clip_depth, Some(32));
    let overlay = player.project_with_display_list_overlay(&project, &project.root, 0);
    overlay.validate().unwrap();

    // --- Dynamic masks in AVM1 (`MovieClip.setMask`) ---
    let mut dyn_avm1_tags = Vec::new();
    swf_tag(
        &mut dyn_avm1_tags,
        2,
        &define_square_shape_tag(1, 20, [255, 0, 0]),
    );
    let mut sp_clip = Vec::new();
    sp_clip.extend_from_slice(&2_u16.to_le_bytes());
    sp_clip.extend_from_slice(&1_u16.to_le_bytes());
    let mut sp_po = vec![0x06];
    sp_po.extend_from_slice(&1_u16.to_le_bytes());
    sp_po.extend_from_slice(&1_u16.to_le_bytes());
    sp_po.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    swf_tag(&mut sp_clip, 26, &sp_po);
    swf_tag(&mut sp_clip, 1, &[]);
    swf_tag(&mut sp_clip, 0, &[]);
    swf_tag(&mut dyn_avm1_tags, 39, &sp_clip);

    for (d, nm, x) in [(1_u16, "hero", 0.0), (2, "other", 10.0), (3, "masker", 5.0)] {
        let mut p = vec![0x26];
        p.extend_from_slice(&d.to_le_bytes());
        p.extend_from_slice(&2_u16.to_le_bytes());
        p.extend_from_slice(&swf_matrix_bytes(None, None, x, 0.0));
        p.extend_from_slice(nm.as_bytes());
        p.push(0);
        swf_tag(&mut dyn_avm1_tags, 26, &p);
    }
    // Frame 0: hero.setMask(masker)
    let call_set_mask = |out: &mut Vec<u8>, recv: &str, arg_clip: Option<&str>| {
        if let Some(m) = arg_clip {
            avm1_push_str(out, m);
            out.push(0x1C); // GetVariable
        } else {
            avm1_push_null(out);
        }
        avm1_push_int(out, 1);
        avm1_push_str(out, recv);
        out.push(0x1C); // GetVariable
        avm1_push_str(out, "setMask");
        out.extend_from_slice(&[0x52, 0x17]); // CallMethod, Pop
    };
    let mut f0_act = Vec::new();
    call_set_mask(&mut f0_act, "hero", Some("masker"));
    f0_act.push(0x00);
    swf_tag(&mut dyn_avm1_tags, 12, &f0_act);
    swf_tag(&mut dyn_avm1_tags, 1, &[]);

    // Frame 1: other.setMask(masker) -> 1-to-1 reassignment detaches hero!
    let mut f1_act = Vec::new();
    call_set_mask(&mut f1_act, "other", Some("masker"));
    f1_act.push(0x00);
    swf_tag(&mut dyn_avm1_tags, 12, &f1_act);
    swf_tag(&mut dyn_avm1_tags, 1, &[]);

    // Frame 2: other.setMask(null) -> detaches other!
    let mut f2_act = Vec::new();
    call_set_mask(&mut f2_act, "other", None);
    f2_act.push(0x07); // Stop
    f2_act.push(0x00);
    swf_tag(&mut dyn_avm1_tags, 12, &f2_act);
    swf_tag(&mut dyn_avm1_tags, 1, &[]);

    let dyn_avm1_bytes = wrap_swf(SwfCompression::Uncompressed, 8, 3, &dyn_avm1_tags);
    let dyn_avm1_path = temp.0.join("dyn_mask_avm1.swf");
    fs::write(&dyn_avm1_path, &dyn_avm1_bytes).unwrap();
    let dyn_proj = import_project(&dyn_avm1_path).unwrap();
    dyn_proj.validate().unwrap();

    let mut dyn_player = SwfBytecodePlayer::from_bytes(&dyn_avm1_bytes).unwrap();
    let s0 = dyn_player.step_frame();
    assert_eq!(s0.display_list[0].masked_by.as_deref(), Some("depth3"));
    assert_eq!(s0.display_list[1].masked_by, None);
    let ov0 = dyn_player.project_with_display_list_overlay(&dyn_proj, &dyn_proj.root, 0);
    ov0.validate().unwrap();
    let masked_scene = dyn_player.evaluate_stage_scene(&dyn_proj, &dyn_proj.root, 0, true);
    let unmasked_scene = dyn_player.evaluate_stage_scene(&dyn_proj, &dyn_proj.root, 0, false);
    assert!(
        format!("{masked_scene:?}") != format!("{unmasked_scene:?}"),
        "dynamic setMask must affect evaluated stage scene when masking=true"
    );

    let s1 = dyn_player.step_frame();
    assert_eq!(
        s1.display_list[0].masked_by, None,
        "reassigning masker to 'other' must clear 'hero.masked_by'"
    );
    assert_eq!(s1.display_list[1].masked_by.as_deref(), Some("depth3"));
    dyn_player
        .project_with_display_list_overlay(&dyn_proj, &dyn_proj.root, 1)
        .validate()
        .unwrap();

    let s2 = dyn_player.step_frame();
    assert_eq!(s2.display_list[0].masked_by, None);
    assert_eq!(s2.display_list[1].masked_by, None);

    // --- Dynamic masks in AVM2 (`DisplayObject.mask`) ---
    let mut pool = MiniAbcBuilder::default();
    let mn_main = pool.qname("", "MainTimeline");
    let mn_mc = pool.qname("flash.display", "MovieClip");
    let mn_hero = pool.qname("", "hero");
    let mn_other = pool.qname("", "other");
    let mn_masker = pool.qname("", "masker");
    let mn_mask_prop = pool.qname("", "mask");
    let mn_trace = pool.qname("", "trace");

    let mut iinit = vec![0xD0, 0x30]; // getlocal_0, pushscope
    // this.hero.mask = this.masker
    iinit.extend_from_slice(&[0xD0, 0x66]);
    write_u30(&mut iinit, mn_hero);
    iinit.extend_from_slice(&[0xD0, 0x66]);
    write_u30(&mut iinit, mn_masker);
    iinit.push(0x61); // setproperty mask
    write_u30(&mut iinit, mn_mask_prop);
    // trace(this.hero.mask === this.masker) -> true
    iinit.push(0x5D);
    write_u30(&mut iinit, mn_trace);
    iinit.extend_from_slice(&[0xD0, 0x66]);
    write_u30(&mut iinit, mn_hero);
    iinit.push(0x66);
    write_u30(&mut iinit, mn_mask_prop);
    iinit.extend_from_slice(&[0xD0, 0x66]);
    write_u30(&mut iinit, mn_masker);
    iinit.push(0xAC); // strictequals
    iinit.push(0x4F);
    write_u30(&mut iinit, mn_trace);
    write_u30(&mut iinit, 1);
    // Reassign: this.other.mask = this.masker; trace(this.hero.mask === null) -> true
    iinit.extend_from_slice(&[0xD0, 0x66]);
    write_u30(&mut iinit, mn_other);
    iinit.extend_from_slice(&[0xD0, 0x66]);
    write_u30(&mut iinit, mn_masker);
    iinit.push(0x61);
    write_u30(&mut iinit, mn_mask_prop);
    iinit.push(0x5D);
    write_u30(&mut iinit, mn_trace);
    iinit.extend_from_slice(&[0xD0, 0x66]);
    write_u30(&mut iinit, mn_hero);
    iinit.push(0x66);
    write_u30(&mut iinit, mn_mask_prop);
    iinit.extend_from_slice(&[0x20, 0xAC, 0x4F]); // pushnull, strictequals, callpropvoid
    write_u30(&mut iinit, mn_trace);
    write_u30(&mut iinit, 1);
    iinit.push(0x47);

    let mut abc = Vec::new();
    pool.write_constant_pool(&mut abc);
    write_u30(&mut abc, 3);
    for _ in 0..3 {
        abc.extend_from_slice(&[0, 0, 0, 0]);
    }
    write_u30(&mut abc, 0);
    write_u30(&mut abc, 1);
    write_u30(&mut abc, mn_main);
    write_u30(&mut abc, mn_mc);
    abc.extend_from_slice(&[0x01, 0, 0, 0]);
    abc.extend_from_slice(&[1, 0]);
    abc.extend_from_slice(&[1, 2, 1]);
    write_u30(&mut abc, mn_main);
    abc.extend_from_slice(&[0x04, 1, 0]);
    write_u30(&mut abc, 3);
    abc.extend_from_slice(&[0, 5, 1, 1, 3]);
    write_u30(&mut abc, iinit.len() as u32);
    abc.extend_from_slice(&iinit);
    abc.extend_from_slice(&[0, 0]);
    abc.extend_from_slice(&[1, 1, 1, 1, 2, 1, 0x47, 0, 0]);
    abc.extend_from_slice(&[2, 1, 1, 1, 2, 1, 0x47, 0, 0]);
    let mut do_abc = vec![1, 0, 0, 0];
    do_abc.extend_from_slice(b"dynmask\0");
    do_abc.extend_from_slice(&abc);

    let mut dyn_avm2_tags = Vec::new();
    swf_tag(&mut dyn_avm2_tags, 69, &[0x08, 0x00, 0x00, 0x00]);
    swf_tag(
        &mut dyn_avm2_tags,
        2,
        &define_square_shape_tag(1, 20, [0, 160, 255]),
    );
    swf_tag(&mut dyn_avm2_tags, 39, &sp_clip);
    for (d, nm, x) in [(1_u16, "hero", 0.0), (2, "other", 10.0), (3, "masker", 5.0)] {
        let mut p = vec![0x26];
        p.extend_from_slice(&d.to_le_bytes());
        p.extend_from_slice(&2_u16.to_le_bytes());
        p.extend_from_slice(&swf_matrix_bytes(None, None, x, 0.0));
        p.extend_from_slice(nm.as_bytes());
        p.push(0);
        swf_tag(&mut dyn_avm2_tags, 26, &p);
    }
    swf_tag(&mut dyn_avm2_tags, 82, &do_abc);
    let mut sym_cls = vec![1, 0, 0, 0];
    sym_cls.extend_from_slice(b"MainTimeline\0");
    swf_tag(&mut dyn_avm2_tags, 76, &sym_cls);
    swf_tag(&mut dyn_avm2_tags, 1, &[]);

    let dyn_avm2_bytes = wrap_swf(SwfCompression::Uncompressed, 10, 1, &dyn_avm2_tags);
    let dyn_avm2_path = temp.0.join("dyn_mask_avm2.swf");
    fs::write(&dyn_avm2_path, &dyn_avm2_bytes).unwrap();
    let dyn_avm2_proj = import_project(&dyn_avm2_path).unwrap();
    dyn_avm2_proj.validate().unwrap();
    let mut p2 = SwfBytecodePlayer::from_bytes(&dyn_avm2_bytes).unwrap();
    let st2 = p2.step_frame();
    assert_eq!(st2.traces, vec!["true", "true"]);
    assert_eq!(st2.display_list[0].masked_by, None);
    assert_eq!(st2.display_list[1].masked_by.as_deref(), Some("depth3"));
    p2.project_with_display_list_overlay(&dyn_avm2_proj, &dyn_avm2_proj.root, 0)
        .validate()
        .unwrap();
}

#[test]
fn avm1_preload_registers_calls_oop_enumeration_bitwise_exceptions_and_movieclip_api() {
    let temp = TempDir::new("avm1-opcodes");

    let mut tags = Vec::new();
    swf_tag(&mut tags, 2, &define_square_shape_tag(1, 16, [255, 120, 0]));
    let mut sp2 = Vec::new();
    sp2.extend_from_slice(&2_u16.to_le_bytes());
    sp2.extend_from_slice(&2_u16.to_le_bytes());
    let mut sp_po = vec![0x06];
    sp_po.extend_from_slice(&1_u16.to_le_bytes());
    sp_po.extend_from_slice(&1_u16.to_le_bytes());
    sp_po.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    swf_tag(&mut sp2, 26, &sp_po);
    swf_tag(&mut sp2, 1, &[]);
    swf_tag(&mut sp2, 1, &[]);
    swf_tag(&mut sp2, 0, &[]);
    swf_tag(&mut tags, 39, &sp2);

    // ExportAssets: sprite 2 exported as "Badge" for attachMovie
    let mut exp = Vec::new();
    exp.extend_from_slice(&1_u16.to_le_bytes());
    exp.extend_from_slice(&2_u16.to_le_bytes());
    exp.extend_from_slice(b"Badge\0");
    swf_tag(&mut tags, 56, &exp);

    // Place sprite 2 at depth 1 named "hero"
    let mut place_hero = vec![0x26];
    place_hero.extend_from_slice(&1_u16.to_le_bytes());
    place_hero.extend_from_slice(&2_u16.to_le_bytes());
    place_hero.extend_from_slice(&swf_matrix_bytes(None, None, 12.0, 24.0));
    place_hero.extend_from_slice(b"hero\0");
    swf_tag(&mut tags, 26, &place_hero);

    let mut act = Vec::new();

    // 1. Store global register 1 = 999, then run DefineFunction2 with register_count = 8 and
    //    preload_this(0x01) | preload_arguments(0x04) | preload_super(0x10) | preload_root(0x40)
    //    | preload_parent(0x80) | preload_global(0x0100) => regs 1..=6, param "p" in reg 7!
    //    Inside body, mutate reg 1..7 and verify outer global reg 1 stays 999 (local register isolation).
    avm1_push_int(&mut act, 999);
    act.extend_from_slice(&[0x87, 0x01, 0x00, 0x01, 0x17]); // StoreRegister(1), Pop
    avm1_push_int(&mut act, 77);
    avm1_push_str(&mut act, "_global");
    act.push(0x1C);
    avm1_push_str(&mut act, "gval");
    // Stack for SetMember is [obj, prop, val]
    act.pop();
    act.clear();

    // Let's build the action stream cleanly with helper closures:
    let set_member = |out: &mut Vec<u8>, obj_var: &str, prop: &str, val: i32| {
        avm1_push_str(out, obj_var);
        out.push(0x1C);
        avm1_push_str(out, prop);
        avm1_push_int(out, val);
        out.push(0x4F); // ActionSetMember
    };
    avm1_push_int(&mut act, 999);
    act.extend_from_slice(&[0x87, 0x01, 0x00, 0x01, 0x17]); // global reg 1 = 999
    set_member(&mut act, "_global", "gval", 77);

    // DefineFunction2 "checkPreload"(p -> reg 7), reg_count=8, flags = 0x01 | 0x04 | 0x10 | 0x40 | 0x80 | 0x0100 = 0x01D5
    // Preloaded registers in SWF spec order:
    // r1 = this, r2 = arguments, r3 = super, r4 = _root, r5 = _parent, r6 = _global, r7 = p
    let mut preload_body = Vec::new();
    // Return: arguments.length (r2.length) + _global.gval (r6.gval) + p (r7)
    avm1_push_reg(&mut preload_body, 2);
    avm1_push_str(&mut preload_body, "length");
    preload_body.push(0x4E); // GetMember -> 2
    avm1_push_reg(&mut preload_body, 6);
    avm1_push_str(&mut preload_body, "gval");
    preload_body.push(0x4E); // GetMember -> 77
    preload_body.push(0x47); // Add2 -> 79
    avm1_push_reg(&mut preload_body, 7);
    preload_body.push(0x47); // Add2 -> 79 + p
    // Clobber local r1 = 12345 to verify outer global r1 (999) is untouched
    avm1_push_int(&mut preload_body, 12345);
    preload_body.extend_from_slice(&[0x87, 0x01, 0x00, 0x01, 0x17]);
    preload_body.push(0x3E); // Return

    act.push(0x8E);
    let mut fn2_hdr = Vec::new();
    fn2_hdr.extend_from_slice(b"checkPreload\0");
    fn2_hdr.extend_from_slice(&1_u16.to_le_bytes()); // 1 param
    fn2_hdr.push(8); // register_count = 8
    fn2_hdr.extend_from_slice(&0x01D5_u16.to_le_bytes());
    fn2_hdr.push(7); // param in reg 7
    fn2_hdr.extend_from_slice(b"p\0");
    fn2_hdr.extend_from_slice(&(preload_body.len() as u16).to_le_bytes());
    act.extend_from_slice(&(fn2_hdr.len() as u16).to_le_bytes());
    act.extend_from_slice(&fn2_hdr);
    act.extend_from_slice(&preload_body);

    // Call checkPreload(21, 99) -> argc=2 (pushed reverse: arg1=99, arg0=21) => 2 + 77 + 21 = 100
    avm1_push_int(&mut act, 99);
    avm1_push_int(&mut act, 21);
    avm1_push_int(&mut act, 2);
    avm1_push_str(&mut act, "checkPreload");
    act.extend_from_slice(&[0x3D, 0x26]); // CallFunction, Trace -> "100"
    // Trace outer global reg 1 -> still "999"
    avm1_push_reg(&mut act, 1);
    act.push(0x26); // Trace -> "999"

    // Also test Function.prototype.call & .apply on checkPreload:
    // checkPreload.call(hero, 10) -> 1 + 77 + 10 = 88
    avm1_push_int(&mut act, 10);
    avm1_push_str(&mut act, "hero");
    act.push(0x1C);
    avm1_push_int(&mut act, 2);
    avm1_push_str(&mut act, "checkPreload");
    act.push(0x1C);
    avm1_push_str(&mut act, "call");
    act.extend_from_slice(&[0x52, 0x26]); // CallMethod, Trace -> "88"

    // 2. OOP: Base, Derived extends Base (0x69), ImplementsOp (0x2C), NewObject (0x40),
    //    InstanceOf (0x54), CastOp (0x2B), super constructor call!
    // Iface = function() {}
    let define_fn1 = |out: &mut Vec<u8>, name: &str, params: &[&str], body: &[u8]| {
        out.push(0x9B); // DefineFunction
        let mut h = Vec::new();
        h.extend_from_slice(name.as_bytes());
        h.push(0);
        h.extend_from_slice(&(params.len() as u16).to_le_bytes());
        for p in params {
            h.extend_from_slice(p.as_bytes());
            h.push(0);
        }
        h.extend_from_slice(&(body.len() as u16).to_le_bytes());
        out.extend_from_slice(&(h.len() as u16).to_le_bytes());
        out.extend_from_slice(&h);
        out.extend_from_slice(body);
    };
    define_fn1(&mut act, "Iface", &[], &[]);
    // Base = function(x) { this.baseX = x; }
    let mut base_body = Vec::new();
    avm1_push_str(&mut base_body, "this");
    base_body.push(0x1C);
    avm1_push_str(&mut base_body, "baseX");
    avm1_push_str(&mut base_body, "x");
    base_body.push(0x1C);
    base_body.push(0x4F); // SetMember
    define_fn1(&mut act, "Base", &["x"], &base_body);

    // Derived = function(x) { super(x + 5); } (DefineFunction2 with preload_super=0x10 in r1)
    let mut derived_body = Vec::new();
    avm1_push_str(&mut derived_body, "x");
    derived_body.push(0x1C);
    avm1_push_int(&mut derived_body, 5);
    derived_body.push(0x47); // x + 5
    avm1_push_int(&mut derived_body, 1); // argc = 1
    avm1_push_reg(&mut derived_body, 1); // r1 = super
    avm1_push_undef(&mut derived_body); // method name undefined -> invoke super constructor!
    derived_body.extend_from_slice(&[0x52, 0x17]); // CallMethod, Pop
    act.push(0x8E);
    let mut d_hdr = Vec::new();
    d_hdr.extend_from_slice(b"Derived\0");
    d_hdr.extend_from_slice(&1_u16.to_le_bytes());
    d_hdr.push(2); // register_count = 2
    d_hdr.extend_from_slice(&0x0010_u16.to_le_bytes()); // preload_super -> r1
    d_hdr.push(0); // param "x" as normal local
    d_hdr.extend_from_slice(b"x\0");
    d_hdr.extend_from_slice(&(derived_body.len() as u16).to_le_bytes());
    act.extend_from_slice(&(d_hdr.len() as u16).to_le_bytes());
    act.extend_from_slice(&d_hdr);
    act.extend_from_slice(&derived_body);

    // ActionExtends (0x69): stack [ subclass, superclass ] (superclass on top)
    avm1_push_str(&mut act, "Derived");
    act.push(0x1C);
    avm1_push_str(&mut act, "Base");
    act.push(0x1C);
    act.push(0x69); // Extends

    // ActionImplementsOp (0x2C): stack [ iface1, count=1, ctor ] (ctor on top)
    avm1_push_str(&mut act, "Iface");
    act.push(0x1C);
    avm1_push_int(&mut act, 1);
    avm1_push_str(&mut act, "Derived");
    act.push(0x1C);
    act.push(0x2C); // ImplementsOp

    // var inst = new Derived(10) -> inst.baseX == 15
    avm1_push_str(&mut act, "inst");
    avm1_push_int(&mut act, 10);
    avm1_push_int(&mut act, 1);
    avm1_push_str(&mut act, "Derived");
    act.push(0x40); // NewObject
    act.push(0x3C); // DefineLocal

    // Trace inst.baseX -> "15"
    avm1_push_str(&mut act, "inst");
    act.push(0x1C);
    avm1_push_str(&mut act, "baseX");
    act.extend_from_slice(&[0x4E, 0x26]);

    // Trace (inst instanceof Base) && (inst instanceof Iface) -> "true"
    avm1_push_str(&mut act, "inst");
    act.push(0x1C);
    avm1_push_str(&mut act, "Base");
    act.push(0x1C);
    act.push(0x54); // InstanceOf -> true
    act.push(0x26);
    avm1_push_str(&mut act, "inst");
    act.push(0x1C);
    avm1_push_str(&mut act, "Iface");
    act.push(0x1C);
    act.push(0x54); // InstanceOf -> true
    act.push(0x26);

    // CastOp (0x2B): [constr, obj] -> non-null for Base, Null for unrelated Iface2
    define_fn1(&mut act, "OtherCtor", &[], &[]);
    avm1_push_str(&mut act, "OtherCtor");
    act.push(0x1C);
    avm1_push_str(&mut act, "inst");
    act.push(0x1C);
    act.push(0x2B); // CastOp -> null
    avm1_push_null(&mut act);
    act.extend_from_slice(&[0x66, 0x26]); // StrictEquals(null) -> "true"

    // 3. InitArray (0x42), InitObject (0x43), Enumerate2 (0x55)
    // var obj = { a: 3, b: 4 }; sum keys via Enumerate2
    avm1_push_str(&mut act, "obj");
    avm1_push_str(&mut act, "a");
    avm1_push_int(&mut act, 3);
    avm1_push_str(&mut act, "b");
    avm1_push_int(&mut act, 4);
    avm1_push_int(&mut act, 2);
    act.push(0x43); // InitObject
    act.push(0x3C); // DefineLocal

    avm1_push_str(&mut act, "sum");
    avm1_push_int(&mut act, 0);
    act.push(0x3C); // var sum = 0

    avm1_push_str(&mut act, "obj");
    act.push(0x1C);
    act.push(0x55); // Enumerate2 -> pushes Null sentinel + keys
    // Loop: StoreRegister(2), PushNull, Equals2, If(+end), sum += obj[reg2], Jump(-loop)
    let mut loop_body = Vec::new();
    avm1_push_str(&mut loop_body, "sum");
    avm1_push_str(&mut loop_body, "sum");
    loop_body.push(0x1C);
    avm1_push_str(&mut loop_body, "obj");
    loop_body.push(0x1C);
    avm1_push_reg(&mut loop_body, 2);
    loop_body.push(0x4E); // GetMember obj[key]
    loop_body.push(0x47); // Add2
    loop_body.push(0x1D); // SetVariable
    // Head check: [StoreRegister 2 (4B), Push null (4B), Equals2 (1B), If (5B)] = 14 bytes
    // Jump back = -(loop_body.len() + 5 + 14)
    let jump_back = -((loop_body.len() as i16) + 5 + 14);
    loop_body.push(0x99);
    loop_body.extend_from_slice(&2_u16.to_le_bytes());
    loop_body.extend_from_slice(&jump_back.to_le_bytes());

    act.extend_from_slice(&[0x87, 0x01, 0x00, 0x02]); // StoreRegister 2
    avm1_push_null(&mut act);
    act.push(0x49); // Equals2
    act.push(0x9D); // ActionIf
    act.extend_from_slice(&2_u16.to_le_bytes());
    act.extend_from_slice(&(loop_body.len() as i16).to_le_bytes());
    act.extend_from_slice(&loop_body);

    avm1_push_str(&mut act, "sum");
    act.extend_from_slice(&[0x1C, 0x26]); // Trace -> "7"

    // 4. Bitwise ops, StrictEquals, Equals2, Greater, Less2, ToInteger/ToNumber/ToString/TypeOf
    // ((0x0F & 0x33) | 0x40) ^ 0x01 = (3 | 64) ^ 1 = 66; (66 << 1) = 132; (-8 >> 1) = -4; (-1 >>> 29) = 7
    avm1_push_int(&mut act, 0x0F);
    avm1_push_int(&mut act, 0x33);
    act.push(0x60); // BitAnd -> 3
    avm1_push_int(&mut act, 0x40);
    act.push(0x61); // BitOr -> 67
    avm1_push_int(&mut act, 0x01);
    act.push(0x62); // BitXor -> 66
    avm1_push_int(&mut act, 1);
    act.push(0x63); // BitLShift -> 132
    act.push(0x26); // Trace -> "132"

    avm1_push_int(&mut act, -8);
    avm1_push_int(&mut act, 1);
    act.extend_from_slice(&[0x64, 0x26]); // BitRShift -> "-4"

    avm1_push_int(&mut act, -1);
    avm1_push_int(&mut act, 29);
    act.extend_from_slice(&[0x65, 0x26]); // BitURShift -> "7"

    // null == undefined -> true; null === undefined -> false; NaN === NaN -> false; 9 > 4 -> true
    avm1_push_null(&mut act);
    avm1_push_undef(&mut act);
    act.extend_from_slice(&[0x49, 0x26]); // Equals2 -> "true"
    avm1_push_null(&mut act);
    avm1_push_undef(&mut act);
    act.extend_from_slice(&[0x66, 0x26]); // StrictEquals -> "false"
    avm1_push_f64(&mut act, f64::NAN);
    avm1_push_f64(&mut act, f64::NAN);
    act.extend_from_slice(&[0x66, 0x26]); // StrictEquals -> "false"
    avm1_push_int(&mut act, 9);
    avm1_push_int(&mut act, 4);
    act.extend_from_slice(&[0x67, 0x26]); // Greater -> "true"

    // ToInteger(3.9) -> 3, TypeOf(hero) -> "movieclip", TargetPath(hero) -> "_level0.hero"
    avm1_push_f64(&mut act, 3.9);
    act.extend_from_slice(&[0x18, 0x26]); // ToInteger -> "3"
    avm1_push_str(&mut act, "hero");
    act.extend_from_slice(&[0x1C, 0x44, 0x26]); // TypeOf -> "movieclip"
    avm1_push_str(&mut act, "hero");
    act.extend_from_slice(&[0x1C, 0x45, 0x26]); // TargetPath -> "_level0.hero"

    // 5. With (0x94), Delete (0x3A) / Delete2 (0x3B), Try / Catch / Finally (0x8F) + Throw (0x2A)
    let mut with_body = Vec::new();
    avm1_push_str(&mut with_body, "b");
    with_body.extend_from_slice(&[0x1C, 0x26]); // reads obj.b -> "4"
    avm1_push_str(&mut act, "obj");
    act.push(0x1C);
    act.push(0x94); // ActionWith
    act.extend_from_slice(&2_u16.to_le_bytes());
    act.extend_from_slice(&(with_body.len() as u16).to_le_bytes());
    act.extend_from_slice(&with_body);

    // Delete obj.a -> true, then obj.a is undefined
    avm1_push_str(&mut act, "obj");
    act.push(0x1C);
    avm1_push_str(&mut act, "a");
    act.extend_from_slice(&[0x3A, 0x26]); // ActionDelete -> "true"

    // Try { throw "boom"; } catch (reg 3) { trace(reg 3); } finally { trace("fin"); }
    let mut try_block = Vec::new();
    avm1_push_str(&mut try_block, "boom");
    try_block.push(0x2A); // ActionThrow
    let mut catch_block = Vec::new();
    avm1_push_reg(&mut catch_block, 3);
    catch_block.push(0x26); // Trace "boom"
    let mut finally_block = Vec::new();
    avm1_push_str(&mut finally_block, "fin");
    finally_block.push(0x26); // Trace "fin"

    act.push(0x8F); // ActionTry
    let mut try_hdr = vec![0x07]; // CatchInRegister(0x04) | FinallyBlock(0x02) | CatchBlock(0x01)
    try_hdr.extend_from_slice(&(try_block.len() as u16).to_le_bytes());
    try_hdr.extend_from_slice(&(catch_block.len() as u16).to_le_bytes());
    try_hdr.extend_from_slice(&(finally_block.len() as u16).to_le_bytes());
    try_hdr.push(3); // catch register 3
    act.extend_from_slice(&(try_hdr.len() as u16).to_le_bytes());
    act.extend_from_slice(&try_hdr);
    act.extend_from_slice(&try_block);
    act.extend_from_slice(&catch_block);
    act.extend_from_slice(&finally_block);

    // 6. MovieClip dynamic methods & properties:
    //    attachMovie("Badge", "attached", 5), createEmptyMovieClip("emptyMc", 6),
    //    attached.swapDepths(emptyMc), attached.getDepth(), getNextHighestDepth(), removeMovieClip
    avm1_push_int(&mut act, 5);
    avm1_push_str(&mut act, "attached");
    avm1_push_str(&mut act, "Badge");
    avm1_push_int(&mut act, 3);
    avm1_push_str(&mut act, "_root");
    act.push(0x1C);
    avm1_push_str(&mut act, "attachMovie");
    act.extend_from_slice(&[0x52, 0x17]); // _root.attachMovie("Badge", "attached", 5)

    avm1_push_int(&mut act, 6);
    avm1_push_str(&mut act, "emptyMc");
    avm1_push_int(&mut act, 2);
    avm1_push_str(&mut act, "_root");
    act.push(0x1C);
    avm1_push_str(&mut act, "createEmptyMovieClip");
    act.extend_from_slice(&[0x52, 0x17]); // _root.createEmptyMovieClip("emptyMc", 6)

    // attached.swapDepths(emptyMc) -> attached moves from AS depth 5 to AS depth 6!
    avm1_push_str(&mut act, "emptyMc");
    act.push(0x1C);
    avm1_push_int(&mut act, 1);
    avm1_push_str(&mut act, "attached");
    act.push(0x1C);
    avm1_push_str(&mut act, "swapDepths");
    act.extend_from_slice(&[0x52, 0x17]);

    // Trace attached.getDepth() -> "6"
    avm1_push_int(&mut act, 0);
    avm1_push_str(&mut act, "attached");
    act.push(0x1C);
    avm1_push_str(&mut act, "getDepth");
    act.extend_from_slice(&[0x52, 0x26]);

    // Trace _root.getNextHighestDepth() -> "7"
    avm1_push_int(&mut act, 0);
    avm1_push_str(&mut act, "_root");
    act.push(0x1C);
    avm1_push_str(&mut act, "getNextHighestDepth");
    act.extend_from_slice(&[0x52, 0x26]);

    // Mutate & read hero._alpha = 50, hero._rotation = 45, hero._totalframes -> 2
    avm1_push_str(&mut act, "hero");
    act.push(0x1C);
    avm1_push_str(&mut act, "_alpha");
    avm1_push_int(&mut act, 50);
    act.push(0x4F);
    avm1_push_str(&mut act, "hero");
    act.push(0x1C);
    avm1_push_str(&mut act, "_totalframes");
    act.extend_from_slice(&[0x4E, 0x26]); // "2"

    act.extend_from_slice(&[0x07, 0x00]); // Stop, End
    swf_tag(&mut tags, 12, &act);
    swf_tag(&mut tags, 1, &[]);

    let swf_bytes = wrap_swf(SwfCompression::Uncompressed, 8, 1, &tags);
    let swf_path = temp.0.join("avm1_full.swf");
    fs::write(&swf_path, &swf_bytes).unwrap();

    let project = import_project(&swf_path).unwrap();
    project.validate().unwrap();
    let root_script = &project.symbol(&project.root).unwrap().timeline.scripts[0]
        .script
        .source;
    assert!(
        root_script.contains("/* enumerate obj */"),
        "Enumerate2 must decompile to valid AS2 loop expression, got:\n{root_script}"
    );
    assert!(
        !root_script.contains("__enum_sentinel__"),
        "raw internal enum sentinel must never leak into decompiled source"
    );

    let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes).unwrap();
    let st = player.step_frame();
    assert_eq!(
        st.traces,
        vec![
            "100",
            "999",
            "88",
            "15",
            "true",
            "true",
            "true",
            "7",
            "132",
            "-4",
            "7",
            "true",
            "false",
            "false",
            "true",
            "3",
            "movieclip",
            "_level0.hero",
            "4",
            "true",
            "boom",
            "fin",
            "6",
            "7",
            "2",
        ]
    );
    let hero_slot = st.display_list.iter().find(|e| e.name == "hero").unwrap();
    assert!((hero_slot.alpha - 50.0).abs() < 1e-3);
    player
        .project_with_display_list_overlay(&project, &project.root, 0)
        .validate()
        .unwrap();
}

#[test]
fn avm2_equality_coercions_types_iteration_lookupswitch_multinames_and_classes() {
    let temp = TempDir::new("avm2-opcodes");

    let mut pool = MiniAbcBuilder::default();
    let mn_main = pool.qname("", "MainTimeline");
    let mn_mc = pool.qname("flash.display", "MovieClip");
    let mn_obj = pool.qname("", "Object");
    let mn_int = pool.qname("", "int");
    let mn_uint = pool.qname("", "uint");
    let mn_str = pool.qname("", "String");
    let mn_any = pool.qname("", "*");
    let mn_base = pool.qname("", "Base");
    let mn_derived = pool.qname("", "Derived");
    let mn_val = pool.qname("", "val");
    let mn_score = pool.qname("", "score");
    let mn_speak = pool.qname("", "speak");
    let mn_trace = pool.qname("", "trace");
    let mn_rt = pool.multiname_l("");

    let d_3_9 = pool.double_idx(3.9);
    let d_neg_3_9 = pool.double_idx(-3.9);
    let d_u32_max = pool.double_idx(4_294_967_295.0);
    let s_42 = pool.str_idx("42");
    let s_hex = pool.str_idx("0x10");
    let s_empty = pool.str_idx("");
    let s_zero = pool.str_idx("0");
    let s_dyn_k = pool.str_idx("dynKey");
    let s_case0 = pool.str_idx("sw0");
    let s_case1 = pool.str_idx("sw1");
    let s_casedef = pool.str_idx("swdef");
    let s_base_tag = pool.str_idx("B:");
    let s_value_of = pool.str_idx("valueOf");
    let mn_undefined = pool.qname("", "undefined");
    let s_undefined = pool.str_idx("undefined");
    let s_der_tag = pool.str_idx("+D");
    let _ = (pool.int_idx(-1), pool.uint_idx(16));

    let emit_trace_top = |code: &mut Vec<u8>| {
        // Store top in local 1, findpropstrict trace, getlocal 1, callpropvoid trace(1)
        code.push(0xD5); // setlocal_1
        code.push(0x5D);
        write_u30(code, mn_trace);
        code.push(0xD1); // getlocal_1
        code.push(0x4F);
        write_u30(code, mn_trace);
        write_u30(code, 1);
    };

    // Methods:
    // 0: Base$iinit(v) -> this.val = v
    // 1: Base$cinit
    // 2: Base.speak() -> return "B:" + this.val
    // 3: Derived$iinit(v) -> super(v + 2)
    // 4: Derived$cinit
    // 5: Derived.get score() -> return this.val * 3
    // 6: Derived.set score(n) -> this.val = n
    // 7: Derived.speak() -> return super.speak() + "+D"
    // 8: MainTimeline$iinit -> full equality, coercion, type, iteration, lookupswitch & OOP checks
    // 9: MainTimeline$cinit
    // 10: script$init

    let mut m0_base_iinit = vec![0xD0, 0x30, 0xD0, 0x49, 0x00]; // getlocal_0, pushscope, getlocal_0, constructsuper(0)
    m0_base_iinit.extend_from_slice(&[0xD0, 0xD1, 0x61]); // this.val = arg1
    write_u30(&mut m0_base_iinit, mn_val);
    m0_base_iinit.push(0x47);

    let mut m2_base_speak = vec![0xD0, 0x30, 0x2C];
    write_u30(&mut m2_base_speak, s_base_tag);
    m2_base_speak.extend_from_slice(&[0xD0, 0x66]);
    write_u30(&mut m2_base_speak, mn_val);
    m2_base_speak.extend_from_slice(&[0xA0, 0x48]); // add, returnvalue

    let m3_der_iinit = vec![0xD0, 0x30, 0xD0, 0xD1, 0x24, 2, 0xA0, 0x49, 0x01, 0x47]; // super(arg1 + 2)

    let mut m5_der_get = vec![0xD0, 0x30, 0xD0, 0x66];
    write_u30(&mut m5_der_get, mn_val);
    m5_der_get.extend_from_slice(&[0x24, 3, 0xA2, 0x48]); // * 3, returnvalue

    let mut m6_der_set = vec![0xD0, 0x30, 0xD0, 0xD1, 0x61];
    write_u30(&mut m6_der_set, mn_val);
    m6_der_set.push(0x47);

    let mut m7_der_speak = vec![0xD0, 0x30, 0xD0, 0x45]; // callsuper speak(0)
    write_u30(&mut m7_der_speak, mn_speak);
    write_u30(&mut m7_der_speak, 0);
    m7_der_speak.push(0x2C);
    write_u30(&mut m7_der_speak, s_der_tag);
    m7_der_speak.extend_from_slice(&[0xA0, 0x48]);

    let mut m8 = vec![0xD0, 0x30]; // getlocal_0, pushscope

    // --- 1. Equality (== 0xAB) vs StrictEquals (=== 0xAC) ---
    // null == undefined -> true; null === undefined -> false
    m8.extend_from_slice(&[0x20, 0x21, 0xAB]);
    emit_trace_top(&mut m8);
    m8.extend_from_slice(&[0x20, 0x21, 0xAC]);
    emit_trace_top(&mut m8);
    // NaN == NaN -> false; null == 0 -> false
    m8.extend_from_slice(&[0x28, 0x28, 0xAB]);
    emit_trace_top(&mut m8);
    m8.extend_from_slice(&[0x20, 0x24, 0, 0xAB]);
    emit_trace_top(&mut m8);
    // "42" == 42 -> true; "42" === 42 -> false; "0x10" == 16 -> true; "" == 0 -> true; true == 1 -> true
    m8.push(0x2C);
    write_u30(&mut m8, s_42);
    m8.extend_from_slice(&[0x24, 42, 0xAB]);
    emit_trace_top(&mut m8);
    m8.push(0x2C);
    write_u30(&mut m8, s_42);
    m8.extend_from_slice(&[0x24, 42, 0xAC]);
    emit_trace_top(&mut m8);
    m8.push(0x2C);
    write_u30(&mut m8, s_hex);
    m8.extend_from_slice(&[0x24, 16, 0xAB]);
    emit_trace_top(&mut m8);
    m8.push(0x2C);
    write_u30(&mut m8, s_empty);
    m8.extend_from_slice(&[0x24, 0, 0xAB]);
    emit_trace_top(&mut m8);
    m8.extend_from_slice(&[0x26, 0x24, 1, 0xAB]);
    emit_trace_top(&mut m8);

    // --- 2. Explicit Coercions ---
    // convert_s(null) === "null" -> "null"; coerce_s(null) === null -> true; coerce_s(undefined) === null -> true
    m8.extend_from_slice(&[0x20, 0x70]); // pushnull, convert_s -> "null"
    emit_trace_top(&mut m8);
    m8.extend_from_slice(&[0x21, 0x70]); // pushundefined, convert_s -> "undefined"
    emit_trace_top(&mut m8);
    m8.extend_from_slice(&[0x20, 0x85, 0x20, 0xAC]); // coerce_s(null) === null -> true
    emit_trace_top(&mut m8);
    m8.extend_from_slice(&[0x21, 0x85, 0x20, 0xAC]); // coerce_s(undefined) === null -> true
    emit_trace_top(&mut m8);

    // convert_i(3.9) -> 3, coerce_i(-3.9) -> -3, convert_i(4294967295.0) -> -1, convert_u(-1) -> 4294967295
    m8.push(0x2F);
    write_u30(&mut m8, d_3_9);
    m8.push(0x73); // convert_i
    emit_trace_top(&mut m8);
    m8.push(0x2F);
    write_u30(&mut m8, d_neg_3_9);
    m8.push(0x83); // coerce_i
    emit_trace_top(&mut m8);
    m8.push(0x2F);
    write_u30(&mut m8, d_u32_max);
    m8.push(0x73); // convert_i -> -1
    emit_trace_top(&mut m8);
    m8.extend_from_slice(&[0x24, 0xFF, 0x74]); // pushbyte -1, convert_u -> 4294967295
    emit_trace_top(&mut m8);

    // convert_d(undefined) -> NaN (self !== self -> false), convert_d(null) -> 0
    m8.extend_from_slice(&[0x21, 0x75, 0x2A, 0xAC]); // pushundef, convert_d, dup, strictequals -> false
    emit_trace_top(&mut m8);
    m8.extend_from_slice(&[0x20, 0x84]); // pushnull, coerce_d -> 0
    emit_trace_top(&mut m8);

    // convert_b("0") -> true, coerce_b("") -> false, coerce_a(undefined) === undefined -> true
    m8.push(0x2C);
    write_u30(&mut m8, s_zero);
    m8.push(0x76);
    emit_trace_top(&mut m8);
    m8.push(0x2C);
    write_u30(&mut m8, s_empty);
    m8.push(0x81); // coerce_b
    emit_trace_top(&mut m8);
    m8.extend_from_slice(&[0x21, 0x82, 0x21, 0xAC]); // coerce_a(undefined) === undefined -> true
    emit_trace_top(&mut m8);

    // istype / astype / istypelate / astypelate / typeof / bitnot
    // (-1 is int) -> true, (-1 is uint) -> false, (null is Object) -> false, (null is *) -> true
    m8.extend_from_slice(&[0x24, 0xFF, 0xB2]);
    write_u30(&mut m8, mn_int);
    emit_trace_top(&mut m8);
    m8.extend_from_slice(&[0x24, 0xFF, 0xB2]);
    write_u30(&mut m8, mn_uint);
    emit_trace_top(&mut m8);
    m8.extend_from_slice(&[0x20, 0xB2]);
    write_u30(&mut m8, mn_obj);
    emit_trace_top(&mut m8);
    m8.extend_from_slice(&[0x20, 0xB2]);
    write_u30(&mut m8, mn_any);
    emit_trace_top(&mut m8);
    // (42 as String) === null -> true
    m8.extend_from_slice(&[0x24, 42, 0x86]);
    write_u30(&mut m8, mn_str);
    m8.extend_from_slice(&[0x20, 0xAC]);
    emit_trace_top(&mut m8);
    // typeof(undefined) -> "undefined", ~5 -> -6
    m8.extend_from_slice(&[0x21, 0x95]);
    emit_trace_top(&mut m8);
    m8.extend_from_slice(&[0x24, 5, 0x97]);
    emit_trace_top(&mut m8);

    // --- 3. Runtime MultinameL, `in`, `deleteproperty`, `hasnext2` + `nextname` + `nextvalue` ---
    // local2 = {}; local2["dynKey"] = 77; trace("dynKey" in local2); sum values via hasnext2; delete local2["dynKey"]
    m8.extend_from_slice(&[0x55, 0x00, 0xD6]); // newobject(0), setlocal_2
    m8.push(0xD2); // getlocal_2
    m8.push(0x2C);
    write_u30(&mut m8, s_dyn_k);
    m8.extend_from_slice(&[0x24, 77, 0x61]); // setproperty MultinameL
    write_u30(&mut m8, mn_rt);
    // "dynKey" in local2 -> true
    m8.push(0x2C);
    write_u30(&mut m8, s_dyn_k);
    m8.extend_from_slice(&[0xD2, 0xB4]); // in
    emit_trace_top(&mut m8);
    // hasnext2(reg3=obj, reg4=idx): reg3 = local2, reg4 = 0; hasnext2 3, 4; reg3 = local2; nextvalue(local2, reg4) -> 77
    m8.extend_from_slice(&[0xD2, 0xD7, 0x24, 0, 0x63, 4]); // reg3=local2, reg4=0
    m8.extend_from_slice(&[0x32, 3, 4, 0x29]); // hasnext2(3, 4), pop bool
    m8.extend_from_slice(&[0xD2, 0x62, 4, 0x23]); // getlocal_2, getlocal 4, nextvalue -> 77
    emit_trace_top(&mut m8);
    // deleteproperty local2["dynKey"] -> true; ("dynKey" in local2) -> false
    m8.push(0xD2);
    m8.push(0x2C);
    write_u30(&mut m8, s_dyn_k);
    m8.push(0x6A); // deleteproperty MultinameL
    write_u30(&mut m8, mn_rt);
    emit_trace_top(&mut m8);
    m8.push(0x2C);
    write_u30(&mut m8, s_dyn_k);
    m8.extend_from_slice(&[0xD2, 0xB4]);
    emit_trace_top(&mut m8);

    // --- 4. lookupswitch (0x1B) with offsets relative to `insn_start`! ---
    // Switch on index 1 -> jumps to case 1 ("sw1")!
    m8.extend_from_slice(&[0x24, 1]); // selector = 1
    let mut c0 = vec![0x2C];
    write_u30(&mut c0, s_case0);
    c0.extend_from_slice(&[0x10, 0, 0, 0]); // placeholder jump to end
    let mut c1 = vec![0x2C];
    write_u30(&mut c1, s_case1);
    c1.extend_from_slice(&[0x10, 0, 0, 0]); // placeholder jump to end
    let mut cdef = vec![0x2C];
    write_u30(&mut cdef, s_casedef);
    // lookupswitch layout: [0x1B, default_s24(3B), max_index_u30=1(1B), case0_s24(3B), case1_s24(3B)] => 11 bytes!
    let sw_len = 11_i32;
    let off_c0 = sw_len;
    let off_c1 = sw_len + (c0.len() as i32);
    let off_def = off_c1 + (c1.len() as i32);
    let end_after_c0 = (c1.len() + cdef.len()) as i32;
    let end_after_c1 = cdef.len() as i32;
    let c0_len = c0.len();
    c0[c0_len - 3..].copy_from_slice(&end_after_c0.to_le_bytes()[..3]);
    let c1_len = c1.len();
    c1[c1_len - 3..].copy_from_slice(&end_after_c1.to_le_bytes()[..3]);
    m8.push(0x1B);
    write_s24(&mut m8, off_def);
    write_u30(&mut m8, 1); // max_index = 1 -> 2 cases (0 and 1)
    write_s24(&mut m8, off_c0);
    write_s24(&mut m8, off_c1);
    m8.extend_from_slice(&c0);
    m8.extend_from_slice(&c1);
    m8.extend_from_slice(&cdef);
    emit_trace_top(&mut m8); // -> "sw1"

    // --- 5. User class inheritance, constructprop, getter/setter, callsuper, istype ---
    // local2 = new Derived(10) -> Base sets val = 12; trace(local2.score) -> 36;
    // local2.score = 20 -> setter sets val = 20; trace(local2.speak()) -> "B:20+D"; trace(local2 is Base) -> true
    m8.push(0x5D);
    write_u30(&mut m8, mn_derived);
    m8.extend_from_slice(&[0x24, 10, 0x4A]); // constructprop Derived(1)
    write_u30(&mut m8, mn_derived);
    write_u30(&mut m8, 1);
    m8.push(0xD6); // setlocal_2
    m8.extend_from_slice(&[0xD2, 0x66]);
    write_u30(&mut m8, mn_score);
    emit_trace_top(&mut m8); // "36"
    m8.extend_from_slice(&[0xD2, 0x24, 20, 0x61]);
    write_u30(&mut m8, mn_score);
    m8.extend_from_slice(&[0xD2, 0x46]); // callproperty speak(0)
    write_u30(&mut m8, mn_speak);
    write_u30(&mut m8, 0);
    emit_trace_top(&mut m8); // "B:20+D"
    m8.extend_from_slice(&[0xD2, 0xB2]);
    write_u30(&mut m8, mn_base);
    emit_trace_top(&mut m8); // "true"
    // Addition resolves an ordinary object's default string and calls an authored valueOf.
    m8.extend_from_slice(&[0x55, 0, 0x24, 2, 0xa0]);
    emit_trace_top(&mut m8);
    m8.push(0x2c);
    write_u30(&mut m8, s_value_of);
    m8.extend_from_slice(&[0x40, 2, 0x55, 1, 0x24, 2, 0xa0]);
    emit_trace_top(&mut m8);
    m8.push(0x60); // The global undefined binding plus a number.
    write_u30(&mut m8, mn_undefined);
    m8.extend_from_slice(&[0x24, 2, 0xa0]);
    emit_trace_top(&mut m8);
    // An authored nearer scope can shadow that global, including with a primitive.
    m8.push(0x2c);
    write_u30(&mut m8, s_undefined);
    m8.extend_from_slice(&[0x24, 5, 0x55, 1, 0x30, 0x60]);
    write_u30(&mut m8, mn_undefined);
    m8.extend_from_slice(&[0x1d, 0x24, 2, 0xa0]);
    emit_trace_top(&mut m8);
    m8.push(0x47);

    let mut abc = Vec::new();
    pool.write_constant_pool(&mut abc);
    // 11 methods:
    // 0: Base$iinit(1 param), 1: Base$cinit(0), 2: Base.speak(0),
    // 3: Derived$iinit(1 param), 4: Derived$cinit(0), 5: Derived.get score(0),
    // 6: Derived.set score(1 param), 7: Derived.speak(0),
    // 8: MainTimeline$iinit(0), 9: MainTimeline$cinit(0), 10: script$init(0)
    let param_counts = [1_u32, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0];
    write_u30(&mut abc, param_counts.len() as u32);
    for &pc in &param_counts {
        write_u30(&mut abc, pc);
        write_u30(&mut abc, 0); // return_type *
        for _ in 0..pc {
            write_u30(&mut abc, 0); // param_type *
        }
        abc.extend_from_slice(&[0, 0]); // name=0, flags=0
    }
    write_u30(&mut abc, 0); // metadata

    // 3 classes: 0=Base, 1=Derived, 2=MainTimeline
    write_u30(&mut abc, 3);
    // Instance 0: Base extends Object, iinit=0, 2 traits (slot `val`, method `speak`)
    write_u30(&mut abc, mn_base);
    write_u30(&mut abc, mn_obj);
    abc.extend_from_slice(&[0x00, 0, 0]); // sealed, 0 ifaces, iinit=0
    write_u30(&mut abc, 2);
    write_u30(&mut abc, mn_val);
    abc.extend_from_slice(&[0x00, 1]); // Slot kind=0, slot_id=1
    write_u30(&mut abc, mn_int);
    abc.push(0); // vindex=0
    write_u30(&mut abc, mn_speak);
    abc.extend_from_slice(&[0x01, 0, 2]); // Method kind=1, disp_id=0, method=2

    // Instance 1: Derived extends Base, iinit=3, 3 traits (getter `score`, setter `score`, override method `speak`)
    write_u30(&mut abc, mn_derived);
    write_u30(&mut abc, mn_base);
    abc.extend_from_slice(&[0x00, 0, 3]); // sealed, 0 ifaces, iinit=3
    write_u30(&mut abc, 3);
    write_u30(&mut abc, mn_score);
    abc.extend_from_slice(&[0x02, 0, 5]); // Getter kind=2, disp_id=0, method=5
    write_u30(&mut abc, mn_score);
    abc.extend_from_slice(&[0x03, 0, 6]); // Setter kind=3, disp_id=0, method=6
    write_u30(&mut abc, mn_speak);
    abc.extend_from_slice(&[0x21, 0, 7]); // Override(0x20)|Method(0x01), disp_id=0, method=7

    // Instance 2: MainTimeline extends MovieClip, iinit=8, 0 traits
    write_u30(&mut abc, mn_main);
    write_u30(&mut abc, mn_mc);
    abc.extend_from_slice(&[0x01, 0, 8, 0]);

    // Class cinit entries (1, 4, 9)
    abc.extend_from_slice(&[1, 0, 4, 0, 9, 0]);

    // 1 script: init=10, 3 class traits
    abc.extend_from_slice(&[1, 10, 3]);
    for (slot_id, (mn, cls_id)) in [(mn_base, 0_u8), (mn_derived, 1), (mn_main, 2)]
        .into_iter()
        .enumerate()
    {
        write_u30(&mut abc, mn);
        abc.extend_from_slice(&[0x04, (slot_id + 1) as u8, cls_id]);
    }

    // 11 method bodies
    write_u30(&mut abc, 11);
    let empty_ret = vec![0x47_u8];
    let bodies: [(u32, u32, &[u8]); 11] = [
        (0, 3, &m0_base_iinit),
        (1, 1, &empty_ret),
        (2, 2, &m2_base_speak),
        (3, 3, &m3_der_iinit),
        (4, 1, &empty_ret),
        (5, 2, &m5_der_get),
        (6, 3, &m6_der_set),
        (7, 2, &m7_der_speak),
        (8, 6, &m8),
        (9, 1, &empty_ret),
        (10, 1, &empty_ret),
    ];
    for (m_idx, local_cnt, code) in bodies {
        write_u30(&mut abc, m_idx);
        write_u30(&mut abc, 8); // max_stack
        write_u30(&mut abc, local_cnt);
        write_u30(&mut abc, 1);
        write_u30(&mut abc, 4);
        write_u30(&mut abc, code.len() as u32);
        abc.extend_from_slice(code);
        abc.extend_from_slice(&[0, 0]);
    }

    let mut do_abc = vec![1, 0, 0, 0];
    do_abc.extend_from_slice(b"avm2_suite\0");
    do_abc.extend_from_slice(&abc);

    let mut tags = Vec::new();
    swf_tag(&mut tags, 69, &[0x08, 0x00, 0x00, 0x00]);
    swf_tag(&mut tags, 82, &do_abc);
    let mut sym_cls = vec![1, 0, 0, 0];
    sym_cls.extend_from_slice(b"MainTimeline\0");
    swf_tag(&mut tags, 76, &sym_cls);
    swf_tag(&mut tags, 1, &[]);

    let swf_bytes = wrap_swf(SwfCompression::Uncompressed, 10, 1, &tags);
    let swf_path = temp.0.join("avm2_full.swf");
    fs::write(&swf_path, &swf_bytes).unwrap();

    let project = import_project(&swf_path).unwrap();
    project.validate().unwrap();
    assert!(
        project.sources.len() >= 2,
        "Base and Derived user classes must reconstruct into editable AS3 SourceUnits"
    );

    let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes).unwrap();
    let st = player.step_frame();
    assert_eq!(
        st.traces,
        vec![
            // 1. Equality vs StrictEquals
            "true",  // null == undefined
            "false", // null === undefined
            "false", // NaN == NaN
            "false", // null == 0
            "true",  // "42" == 42
            "false", // "42" === 42
            "true",  // "0x10" == 16
            "true",  // "" == 0
            "true",  // true == 1
            // 2. Coercions & type checks
            "null",       // convert_s(null)
            "undefined",  // convert_s(undefined)
            "true",       // coerce_s(null) === null
            "true",       // coerce_s(undefined) === null
            "3",          // convert_i(3.9)
            "-3",         // coerce_i(-3.9)
            "-1",         // convert_i(4294967295.0)
            "4294967295", // convert_u(-1)
            "false",      // convert_d(undefined) -> NaN !== NaN
            "0",          // coerce_d(null)
            "true",       // convert_b("0")
            "false",      // coerce_b("")
            "true",       // coerce_a(undefined) === undefined
            "true",       // -1 is int
            "false",      // -1 is uint
            "false",      // null is Object
            "true",       // null is *
            "true",       // (42 as String) === null
            "undefined",  // typeof undefined
            "-6",         // ~5
            // 3. MultinameL + in + hasnext2/nextvalue + deleteproperty
            "true",  // "dynKey" in local2
            "77",    // nextvalue
            "true",  // delete local2["dynKey"]
            "false", // "dynKey" in local2 after delete
            // 4. lookupswitch
            "sw1",
            // 5. User class inheritance, super, getter/setter, istype
            "36",
            "B:20+D",
            "true",
            "[object Object]2",
            "B:undefined2",
            "NaN",
            "7",
        ]
    );
}

#[test]
fn swf_player_seek_step_restart_determinism_and_located_diagnostics() {
    let temp = TempDir::new("determinism-diagnostics");

    // 1. Multi-frame SWF with dynamic clips, dynamic masks, root/clip properties, and variables
    let mut tags = Vec::new();
    swf_tag(
        &mut tags,
        2,
        &define_square_shape_tag(1, 20, [50, 200, 120]),
    );
    let mut sp2 = Vec::new();
    sp2.extend_from_slice(&2_u16.to_le_bytes());
    sp2.extend_from_slice(&1_u16.to_le_bytes());
    let mut sp_po = vec![0x06];
    sp_po.extend_from_slice(&1_u16.to_le_bytes());
    sp_po.extend_from_slice(&1_u16.to_le_bytes());
    sp_po.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    swf_tag(&mut sp2, 26, &sp_po);
    swf_tag(&mut sp2, 1, &[]);
    swf_tag(&mut sp2, 0, &[]);
    swf_tag(&mut tags, 39, &sp2);

    // DefineScalingGrid (tag 78) on sprite 2 + DefineBinaryData (tag 87) char 9
    let mut grid_tag = Vec::new();
    grid_tag.extend_from_slice(&2_u16.to_le_bytes());
    let mut g_rect = BitWriter::new();
    g_rect.push(12, 5);
    g_rect.push(40, 12); // x=2px
    g_rect.push(360, 12); // x2=18px -> w=16px
    g_rect.push(40, 12); // y=2px
    g_rect.push(360, 12); // y2=18px -> h=16px
    grid_tag.extend_from_slice(&g_rect.finish());
    swf_tag(&mut tags, 78, &grid_tag);

    let mut bin_tag = Vec::new();
    bin_tag.extend_from_slice(&9_u16.to_le_bytes());
    bin_tag.extend_from_slice(&0_u32.to_le_bytes());
    bin_tag.extend_from_slice(b"NEOFLASH_BIN_PAYLOAD");
    swf_tag(&mut tags, 87, &bin_tag);

    let mut exp = Vec::new();
    exp.extend_from_slice(&1_u16.to_le_bytes());
    exp.extend_from_slice(&2_u16.to_le_bytes());
    exp.extend_from_slice(b"Tile\0");
    swf_tag(&mut tags, 56, &exp);

    for (d, nm, x) in [(1_u16, "hero", 10.0), (2, "masker", 10.0)] {
        let mut p = vec![0x26];
        p.extend_from_slice(&d.to_le_bytes());
        p.extend_from_slice(&2_u16.to_le_bytes());
        p.extend_from_slice(&swf_matrix_bytes(None, None, x, 5.0));
        p.extend_from_slice(nm.as_bytes());
        p.push(0);
        swf_tag(&mut tags, 26, &p);
    }

    // Frame 0: counter = 10; _root._x = 5; trace("f0:" + counter + ":" + _root._x)
    let mut f0 = Vec::new();
    avm1_push_str(&mut f0, "counter");
    avm1_push_int(&mut f0, 10);
    f0.push(0x1D);
    avm1_push_str(&mut f0, "_x");
    avm1_push_int(&mut f0, 5);
    f0.push(0x1D);
    avm1_push_str(&mut f0, "f0:");
    avm1_push_str(&mut f0, "counter");
    f0.extend_from_slice(&[0x1C, 0x47]);
    avm1_push_str(&mut f0, ":");
    f0.push(0x47);
    avm1_push_str(&mut f0, "_x");
    f0.extend_from_slice(&[0x1C, 0x47, 0x26, 0x00]);
    swf_tag(&mut tags, 12, &f0);
    swf_tag(&mut tags, 1, &[]);

    // Frame 1: attachMovie("Tile", "dyn1", 10); hero.setMask(masker); counter += 5; _root._x += 10
    let mut f1 = Vec::new();
    avm1_push_int(&mut f1, 10);
    avm1_push_str(&mut f1, "dyn1");
    avm1_push_str(&mut f1, "Tile");
    avm1_push_int(&mut f1, 3);
    avm1_push_str(&mut f1, "_root");
    f1.push(0x1C);
    avm1_push_str(&mut f1, "attachMovie");
    f1.extend_from_slice(&[0x52, 0x17]);

    avm1_push_str(&mut f1, "masker");
    f1.push(0x1C);
    avm1_push_int(&mut f1, 1);
    avm1_push_str(&mut f1, "hero");
    f1.push(0x1C);
    avm1_push_str(&mut f1, "setMask");
    f1.extend_from_slice(&[0x52, 0x17]);

    avm1_push_str(&mut f1, "counter");
    avm1_push_str(&mut f1, "counter");
    f1.push(0x1C);
    avm1_push_int(&mut f1, 5);
    f1.extend_from_slice(&[0x47, 0x1D]);

    avm1_push_str(&mut f1, "_x");
    avm1_push_str(&mut f1, "_x");
    f1.push(0x1C);
    avm1_push_int(&mut f1, 10);
    f1.extend_from_slice(&[0x47, 0x1D]);

    avm1_push_str(&mut f1, "f1:");
    avm1_push_str(&mut f1, "counter");
    f1.extend_from_slice(&[0x1C, 0x47]);
    avm1_push_str(&mut f1, ":");
    f1.push(0x47);
    avm1_push_str(&mut f1, "_x");
    f1.extend_from_slice(&[0x1C, 0x47, 0x26, 0x00]);
    swf_tag(&mut tags, 12, &f1);
    swf_tag(&mut tags, 1, &[]);

    // Frame 2: createEmptyMovieClip("dyn2", 11); counter += 20; stop()
    let mut f2 = Vec::new();
    avm1_push_int(&mut f2, 11);
    avm1_push_str(&mut f2, "dyn2");
    avm1_push_int(&mut f2, 2);
    avm1_push_str(&mut f2, "_root");
    f2.push(0x1C);
    avm1_push_str(&mut f2, "createEmptyMovieClip");
    f2.extend_from_slice(&[0x52, 0x17]);

    avm1_push_str(&mut f2, "counter");
    avm1_push_str(&mut f2, "counter");
    f2.push(0x1C);
    avm1_push_int(&mut f2, 20);
    f2.extend_from_slice(&[0x47, 0x1D]);

    avm1_push_str(&mut f2, "f2:");
    avm1_push_str(&mut f2, "counter");
    f2.extend_from_slice(&[0x1C, 0x47, 0x26, 0x07, 0x00]);
    swf_tag(&mut tags, 12, &f2);
    swf_tag(&mut tags, 1, &[]);

    let det_bytes = wrap_swf(SwfCompression::Zlib, 8, 3, &tags);
    let det_path = temp.0.join("determinism.swf");
    fs::write(&det_path, &det_bytes).unwrap();
    let det_proj = import_project(&det_path).unwrap();
    det_proj.validate().unwrap();
    // Verify DefineScalingGrid & DefineBinaryData reconstructed onto Project
    assert_eq!(
        det_proj.symbol("swf-sym-2").unwrap().scale9_grid(),
        Some([2.0, 2.0, 16.0, 16.0])
    );
    let bin_res = det_proj
        .resources
        .iter()
        .find(|r| r.id == "swf-binary-9")
        .expect("DefineBinaryData resource");
    assert_eq!(bin_res.decoded_bytes().unwrap(), b"NEOFLASH_BIN_PAYLOAD");

    let mut player = SwfBytecodePlayer::from_bytes(&det_bytes).unwrap();
    let step0 = player.step_frame();
    let step1 = player.step_frame();
    let step2 = player.step_frame();
    assert_eq!(step0.traces, vec!["f0:10:5"]);
    assert_eq!(step1.traces, vec!["f1:15:15"]);
    assert_eq!(step2.traces, vec!["f2:35"]);
    assert_eq!(step0.display_list.len(), 2); // hero, masker
    assert_eq!(step1.display_list.len(), 3); // + dyn1
    assert_eq!(step2.display_list.len(), 4); // + dyn2
    assert_eq!(step1.display_list[0].masked_by.as_deref(), Some("depth2"));

    // Seek backward to frame 0: dynamic clips (dyn1, dyn2), dynamic mask on hero, counter, and _root._x
    // MUST reset deterministically without leaks!
    let seek0 = player.seek_to_frame(0);
    assert_eq!(
        seek0, step0,
        "backward seek_to_frame(0) must match initial step0"
    );
    assert_eq!(seek0.display_list[0].masked_by, None);

    // Seek forward to frame 1 & frame 2, then reset() + step again:
    let seek1 = player.seek_to_frame(1);
    assert_eq!(seek1, step1, "seek_to_frame(1) must match step1");
    let seek2 = player.seek_to_frame(2);
    assert_eq!(seek2, step2, "seek_to_frame(2) must match step2");

    player.reset();
    assert!(player.display_list().is_empty());
    assert!(player.trace_output().is_empty());
    assert_eq!(player.step_frame(), step0);
    assert_eq!(player.step_frame(), step1);
    assert_eq!(player.step_frame(), step2);

    // 2. Structured located diagnostics: unsupported tags + unrecognized AVM1 & AVM2 opcodes
    let mut diag_avm1_tags = Vec::new();
    // Tag 57 (ImportAssets) + unknown tag 250
    swf_tag(&mut diag_avm1_tags, 57, b"ext.swf\0\x00\x00");
    swf_tag(&mut diag_avm1_tags, 250, &[1, 2, 3, 4]);
    // Unrecognized AVM1 opcode 0x7D at known bytecode offset after a Push
    let mut bad_act = Vec::new();
    avm1_push_str(&mut bad_act, "before_unknown");
    bad_act.push(0x26);
    bad_act.push(0x7D); // unrecognized AVM1 opcode
    bad_act.push(0x00);
    swf_tag(&mut diag_avm1_tags, 12, &bad_act);
    swf_tag(&mut diag_avm1_tags, 1, &[]);

    let diag_avm1_bytes = wrap_swf(SwfCompression::Uncompressed, 8, 1, &diag_avm1_tags);
    let diag_avm1_path = temp.0.join("diag_avm1.swf");
    fs::write(&diag_avm1_path, &diag_avm1_bytes).unwrap();
    let rep1 = inspect(&diag_avm1_path).unwrap();
    assert!(
        rep1.diagnostics.iter().any(|d| {
            d.code == "swf_import_assets_unsupported"
                && d.location.contains("#tag=57@")
                && d.location.contains(":symbol=root:frame=0")
        }),
        "expected located swf_import_assets_unsupported: {:?}",
        rep1.diagnostics
    );
    assert!(
        rep1.diagnostics.iter().any(|d| {
            d.code == "swf_tag_unsupported"
                && d.location.contains("#tag=250@")
                && d.location.contains(":symbol=root:frame=0")
        }),
        "expected located swf_tag_unsupported: {:?}",
        rep1.diagnostics
    );
    assert!(
        rep1.diagnostics.iter().any(|d| {
            d.code == "swf_avm1_partial"
                && d.location.contains("#tag=12@")
                && d.location.contains(":symbol=root:frame=0:offset=0x")
                && d.message.to_ascii_lowercase().contains("0x7d")
        }),
        "expected located swf_avm1_partial with bytecode offset: {:?}",
        rep1.diagnostics
    );
    let mut p_diag1 = SwfBytecodePlayer::from_bytes(&diag_avm1_bytes).unwrap();
    let st_diag1 = p_diag1.step_frame();
    assert_eq!(st_diag1.traces, vec!["before_unknown"]);
    assert!(
        st_diag1.unsupported_diagnostics.iter().any(|m| {
            m.contains("0x7D") && m.contains("target=_root") && m.contains("offset=0x")
        }),
        "expected located runtime diagnostic in player: {:?}",
        st_diag1.unsupported_diagnostics
    );

    // Unrecognized AVM2 opcode (0xFB) in DoABC2
    let mut pool = MiniAbcBuilder::default();
    let mn_main = pool.qname("", "MainTimeline");
    let mn_mc = pool.qname("flash.display", "MovieClip");
    let mut abc = Vec::new();
    pool.write_constant_pool(&mut abc);
    write_u30(&mut abc, 3);
    for _ in 0..3 {
        abc.extend_from_slice(&[0, 0, 0, 0]);
    }
    write_u30(&mut abc, 0);
    write_u30(&mut abc, 1);
    write_u30(&mut abc, mn_main);
    write_u30(&mut abc, mn_mc);
    abc.extend_from_slice(&[0x01, 0, 0, 0, 1, 0, 1, 2, 1]);
    write_u30(&mut abc, mn_main);
    abc.extend_from_slice(&[0x04, 1, 0]);
    write_u30(&mut abc, 3);
    let bad_iinit = [0xD0_u8, 0x30, 0xFB, 0x47]; // getlocal_0, pushscope, unknown 0xFB, returnvoid
    abc.extend_from_slice(&[0, 2, 1, 1, 3, bad_iinit.len() as u8]);
    abc.extend_from_slice(&bad_iinit);
    abc.extend_from_slice(&[0, 0]);
    abc.extend_from_slice(&[1, 1, 1, 1, 2, 1, 0x47, 0, 0]);
    abc.extend_from_slice(&[2, 1, 1, 1, 2, 1, 0x47, 0, 0]);
    let mut do_abc = vec![1, 0, 0, 0];
    do_abc.extend_from_slice(b"bad_abc\0");
    do_abc.extend_from_slice(&abc);
    let mut diag_avm2_tags = Vec::new();
    swf_tag(&mut diag_avm2_tags, 69, &[0x08, 0x00, 0x00, 0x00]);
    swf_tag(&mut diag_avm2_tags, 82, &do_abc);
    let mut sym_cls = vec![1, 0, 0, 0];
    sym_cls.extend_from_slice(b"MainTimeline\0");
    swf_tag(&mut diag_avm2_tags, 76, &sym_cls);
    swf_tag(&mut diag_avm2_tags, 1, &[]);

    let diag_avm2_bytes = wrap_swf(SwfCompression::Uncompressed, 10, 1, &diag_avm2_tags);
    let diag_avm2_path = temp.0.join("diag_avm2.swf");
    fs::write(&diag_avm2_path, &diag_avm2_bytes).unwrap();
    let rep2 = inspect(&diag_avm2_path).unwrap();
    assert!(
        rep2.diagnostics.iter().any(|d| {
            d.code == "swf_avm2_partial"
                && d.location.contains("#tag=82@")
                && d.location
                    .contains(":symbol=MainTimeline:method=0:frame=0:offset=0x2")
                && d.message.contains("0xfb")
        }),
        "expected located swf_avm2_partial with class name, method, and offset: {:?}",
        rep2.diagnostics
    );
    let mut p_diag2 = SwfBytecodePlayer::from_bytes(&diag_avm2_bytes).unwrap();
    let st_diag2 = p_diag2.step_frame();
    assert!(
        st_diag2
            .unsupported_diagnostics
            .iter()
            .any(|m| { m.contains("0xFB") && m.contains("abc#0 method#0 offset=0x2") }),
        "expected located AVM2 runtime diagnostic: {:?}",
        st_diag2.unsupported_diagnostics
    );
}

// Original synthetic SWF with editable field flags and multi-run static text.
// This helper also supplies the delivery's HTML5 browser fixture.
fn t345_text_fields_swf() -> Vec<u8> {
    fn rect_bytes() -> Vec<u8> {
        let mut bits = BitWriter::new();
        bits.push(16, 5);
        for value in [0, 6000, 0, 600] {
            bits.push(value, 16);
        }
        bits.finish()
    }
    fn edit(id: u16, flags0: u8, flags1: u8, align: u8, variable: &str, text: &str) -> Vec<u8> {
        let mut bytes = id.to_le_bytes().to_vec();
        bytes.extend(rect_bytes());
        bytes.extend([flags0, flags1]);
        bytes.extend(2u16.to_le_bytes()); // HasFont: plain italic base, independent of bold HTML spans
        bytes.extend(240u16.to_le_bytes());
        bytes.extend([20, 40, 60, 128]); // HasTextColor
        if flags0 & 2 != 0 {
            bytes.extend(5u16.to_le_bytes());
        }
        bytes.push(align); // HasLayout
        for value in [20i16, 40, 60, 80] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(variable.as_bytes());
        bytes.push(0);
        bytes.extend(text.as_bytes());
        bytes.push(0);
        bytes
    }
    fn static_record(bytes: &mut Vec<u8>, font: u16, rgba: [u8; 4], height: u16, glyphs: &[u64]) {
        bytes.push(0x8c); // font + color, existing baseline/position
        bytes.extend(font.to_le_bytes());
        bytes.extend(rgba);
        bytes.extend(height.to_le_bytes());
        bytes.push(glyphs.len() as u8);
        let mut bits = BitWriter::new();
        for &glyph in glyphs {
            bits.push(glyph, 3);
            bits.push(200, 10);
        }
        bytes.extend(bits.finish());
    }
    let mut tags = Vec::new();
    for (id, name, flags, codes) in [
        (1u16, "Synthetic Serif", 3, vec![65u16, 0x0416, 13]),
        (2u16, "Synthetic Sans", 5, vec![66u16, 10]),
    ] {
        let mut font = id.to_le_bytes().to_vec();
        font.push(name.len() as u8);
        font.extend(name.as_bytes());
        font.push(flags);
        for code in codes {
            font.extend(code.to_le_bytes());
        }
        swf_tag(&mut tags, 13, &font);
    }
    for (id, f0, f1, align, variable, text) in [
        (10, 0xb7, 0x69, 1, "form.value", "A😀B"),
        (
            11,
            0xed,
            0x32,
            3,
            "",
            "<P ALIGN=\"JUSTIFY\"><FONT FACE=\"Synthetic Sans\" SIZE=\"18\" COLOR=\"#204060\"><B>A😀</B>Ж</FONT></P>",
        ),
        (12, 0x8d, 0x60, 2, "", "center"),
        (
            13,
            0x8d,
            0x22,
            0,
            "",
            "<P>kept<IMG SRC=\"missing.png\" /></P>",
        ),
    ] {
        swf_tag(&mut tags, 37, &edit(id, f0, f1, align, variable, text));
    }
    let mut static_text = 20u16.to_le_bytes().to_vec();
    static_text.extend(rect_bytes());
    static_text.extend(swf_matrix_bytes(None, None, 0., 0.));
    static_text.extend([3, 10]);
    static_record(&mut static_text, 1, [200, 20, 30, 255], 240, &[0, 1, 2]);
    static_record(&mut static_text, 2, [30, 40, 210, 255], 400, &[1, 0]);
    static_text.push(0);
    swf_tag(&mut tags, 33, &static_text);
    let mut unmapped = 21u16.to_le_bytes().to_vec();
    unmapped.extend(rect_bytes());
    unmapped.extend(swf_matrix_bytes(None, None, 0., 0.));
    unmapped.extend([3, 10]);
    static_record(&mut unmapped, 1, [0, 0, 0, 255], 240, &[0, 7]);
    unmapped.push(0);
    swf_tag(&mut tags, 33, &unmapped);
    for (depth, id) in [10u16, 11, 12, 13, 20, 21].into_iter().enumerate() {
        let mut place = vec![0x26];
        place.extend((depth as u16 + 1).to_le_bytes());
        place.extend(id.to_le_bytes());
        place.extend(swf_matrix_bytes(None, None, 10., depth as f64 * 42.));
        place.extend(format!("field{id}\0").as_bytes());
        swf_tag(&mut tags, 26, &place);
    }
    swf_tag(&mut tags, 1, &[]);
    wrap_swf(SwfCompression::Uncompressed, 8, 1, &tags)
}

#[test]
fn swf_editable_fields_and_static_utf16_style_runs_preserve_authored_semantics() {
    use neoflash_core::{ElementKind, TextFieldState, TextFieldType};
    let temp = TempDir::new("text-fields");
    let path = temp.0.join("text-fields.swf");
    fs::write(&path, t345_text_fields_swf()).unwrap();
    let report = inspect(&path).unwrap();
    assert!(report.can_import, "{:?}", report.diagnostics);
    let project = import_project(&path).unwrap();
    project.validate().unwrap();
    assert_eq!(
        Project::from_json(&project.to_json().unwrap()).unwrap(),
        project
    );
    let field = |id: u16| {
        let symbol = project.symbol(&format!("swf-sym-{id}")).unwrap();
        TextFieldState::from_kind(&symbol.timeline.layers[0].keyframes[0].elements[0].kind).unwrap()
    };
    let input = field(10);
    assert_eq!(input.text, "A😀B");
    assert_eq!(input.field_type, TextFieldType::Input);
    assert_eq!(input.max_chars, 5);
    assert_eq!(input.variable.as_deref(), Some("form.value"));
    assert!(input.display_as_password && input.selectable && input.multiline);
    assert!(!input.word_wrap);
    assert!(input.embed_fonts && input.as2_viewport.border && input.as2_viewport.background);
    assert_eq!(input.as2_viewport.auto_size, "left");
    assert_eq!(input.format.align, neoflash_core::TextAlign::Right);
    assert_eq!(input.format.left_margin, 1.);
    assert_eq!(input.format.right_margin, 2.);
    assert_eq!(input.format.indent, 3.);
    assert_eq!(input.format.leading, 4.);
    let input_element =
        &project.symbol("swf-sym-10").unwrap().timeline.layers[0].keyframes[0].elements[0];
    assert_eq!(input_element.transform.alpha, 128. / 255.);

    let html = field(11);
    assert_eq!(html.field_type, TextFieldType::Dynamic);
    assert!(!html.selectable && !html.embed_fonts);
    assert!(html.html && html.word_wrap && html.multiline);
    assert_eq!(html.text, "A😀Ж");
    assert_eq!(html.format.align_mode(), Some("justify"));
    assert_eq!(html.format_runs.len(), 2);
    assert_eq!((html.format_runs[0].start, html.format_runs[0].end), (0, 3));
    assert_eq!((html.format_runs[1].start, html.format_runs[1].end), (3, 4));
    assert!(html.format_runs[0].format.bold);
    assert!(!html.format_runs[1].format.bold);
    assert_eq!(html.format_runs[0].format.font, "Synthetic Sans");
    assert_eq!(html.format_runs[0].format.size, 18.);
    assert_eq!(html.format_runs[0].format.color, 0x204060);
    assert_eq!(field(12).as2_viewport.auto_size, "left");
    assert_eq!(field(12).format.align, neoflash_core::TextAlign::Center);
    assert!(!field(12).multiline);
    assert_eq!(field(13).text, "kept");

    let static_text = field(20);
    assert_eq!(static_text.text, "AЖ\nB");
    assert_eq!(static_text.format_runs.len(), 2);
    assert_eq!(
        (
            static_text.format_runs[0].start,
            static_text.format_runs[0].end
        ),
        (0, 2)
    );
    assert_eq!(
        (
            static_text.format_runs[1].start,
            static_text.format_runs[1].end
        ),
        (2, 4)
    );
    assert_eq!(static_text.format_runs[0].format.font, "Synthetic Serif");
    assert!(static_text.format_runs[0].format.bold);
    assert_eq!(static_text.format_runs[0].format.color, 0xc8141e);
    assert_eq!(static_text.format_runs[1].format.font, "Synthetic Sans");
    assert!(static_text.format_runs[1].format.italic);
    assert_eq!(static_text.format_runs[1].format.size, 20.);
    assert_eq!(static_text.format_runs[1].format.color, 0x1e28d2);
    assert_eq!(field(21).text, "A�");
    let approximations: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "swf_text_approximated")
        .collect();
    assert_eq!(approximations.len(), 2, "{approximations:?}");
    assert!(
        approximations
            .iter()
            .any(|d| d.location.ends_with("#edittext:13"))
    );
    assert!(
        approximations
            .iter()
            .any(|d| d.location.ends_with("#text:21"))
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "swf_text_metrics")
    );
    assert!(matches!(input_element.kind, ElementKind::TextBox { .. }));
}

#[test]
fn swf_player_backward_seek_step_and_restart_reset_dynamic_clips_tombstones_and_vm_state() {
    // 1. AVM1 3-frame SWF:
    //    - DoInitAction initializes `boot = 1`
    //    - Frame 0 places `hero` (depth 1) and `badge` (depth 2), sets `runs = (runs || 0) + 1`
    //    - Frame 1 duplicates `badge` -> `badge_dup` (dynamic), creates `spark` (dynamic),
    //      and swaps + removes `hero` (`hero.swapDepths(30); hero.removeMovieClip()`), leaving a
    //      removed-depth tombstone on timeline depth 1!
    //    - Frame 2 has a timeline `PlaceObject2(Move)` on depth 1 (which MUST NOT resurrect
    //      removed `hero` during forward playback!) and calls `stop()`.
    let mut tags = Vec::new();
    swf_tag(&mut tags, 2, &define_square_shape_tag(1, 16, [240, 80, 60]));
    let mut sp = Vec::new();
    sp.extend_from_slice(&2_u16.to_le_bytes());
    sp.extend_from_slice(&1_u16.to_le_bytes());
    let mut sp_po = vec![0x06];
    sp_po.extend_from_slice(&1_u16.to_le_bytes());
    sp_po.extend_from_slice(&1_u16.to_le_bytes());
    sp_po.extend_from_slice(&swf_matrix_bytes(None, None, 0.0, 0.0));
    swf_tag(&mut sp, 26, &sp_po);
    swf_tag(&mut sp, 1, &[]);
    swf_tag(&mut sp, 0, &[]);
    swf_tag(&mut tags, 39, &sp);

    // DoInitAction (tag 59): boot = (boot + 1) -> 1 on fresh run
    let mut init_act = Vec::new();
    init_act.extend_from_slice(&2_u16.to_le_bytes());
    avm1_push_str(&mut init_act, "boot");
    avm1_push_str(&mut init_act, "boot");
    init_act.push(0x1C);
    avm1_push_int(&mut init_act, 1);
    init_act.extend_from_slice(&[0x47, 0x1D, 0x00]);
    swf_tag(&mut tags, 59, &init_act);

    for (d, nm, x) in [(1_u16, "hero", 12.0), (2_u16, "badge", 40.0)] {
        let mut p = vec![0x26];
        p.extend_from_slice(&d.to_le_bytes());
        p.extend_from_slice(&2_u16.to_le_bytes());
        p.extend_from_slice(&swf_matrix_bytes(None, None, x, 8.0));
        p.extend_from_slice(nm.as_bytes());
        p.push(0);
        swf_tag(&mut tags, 26, &p);
    }
    // Frame 0 DoAction: count = (count + 10); trace("f0:" + boot + ":" + count)
    let mut f0 = Vec::new();
    avm1_push_str(&mut f0, "count");
    avm1_push_str(&mut f0, "count");
    f0.push(0x1C);
    avm1_push_int(&mut f0, 10);
    f0.extend_from_slice(&[0x47, 0x1D]);
    avm1_push_str(&mut f0, "f0:");
    avm1_push_str(&mut f0, "boot");
    f0.extend_from_slice(&[0x1C, 0x47]);
    avm1_push_str(&mut f0, ":");
    f0.push(0x47);
    avm1_push_str(&mut f0, "count");
    f0.extend_from_slice(&[0x1C, 0x47, 0x26, 0x00]);
    swf_tag(&mut tags, 12, &f0);
    swf_tag(&mut tags, 1, &[]);

    // Frame 1 DoAction:
    //   badge.duplicateMovieClip("badge_dup", 5);
    //   _root.createEmptyMovieClip("spark", 6);
    //   hero.swapDepths(30);
    //   hero.removeMovieClip();
    //   count += 5; trace("f1:" + count);
    let mut f1 = Vec::new();
    avm1_push_int(&mut f1, 5);
    avm1_push_str(&mut f1, "badge_dup");
    avm1_push_int(&mut f1, 2);
    avm1_push_str(&mut f1, "badge");
    f1.push(0x1C);
    avm1_push_str(&mut f1, "duplicateMovieClip");
    f1.extend_from_slice(&[0x52, 0x17]);

    avm1_push_int(&mut f1, 6);
    avm1_push_str(&mut f1, "spark");
    avm1_push_int(&mut f1, 2);
    avm1_push_str(&mut f1, "_root");
    f1.push(0x1C);
    avm1_push_str(&mut f1, "createEmptyMovieClip");
    f1.extend_from_slice(&[0x52, 0x17]);

    avm1_push_int(&mut f1, 30);
    avm1_push_int(&mut f1, 1);
    avm1_push_str(&mut f1, "hero");
    f1.push(0x1C);
    avm1_push_str(&mut f1, "swapDepths");
    f1.extend_from_slice(&[0x52, 0x17]);

    avm1_push_int(&mut f1, 0);
    avm1_push_str(&mut f1, "hero");
    f1.push(0x1C);
    avm1_push_str(&mut f1, "removeMovieClip");
    f1.extend_from_slice(&[0x52, 0x17]);

    avm1_push_str(&mut f1, "count");
    avm1_push_str(&mut f1, "count");
    f1.push(0x1C);
    avm1_push_int(&mut f1, 5);
    f1.extend_from_slice(&[0x47, 0x1D]);
    avm1_push_str(&mut f1, "f1:");
    avm1_push_str(&mut f1, "count");
    f1.extend_from_slice(&[0x1C, 0x47, 0x26, 0x00]);
    swf_tag(&mut tags, 12, &f1);
    swf_tag(&mut tags, 1, &[]);

    // Frame 2: timeline PlaceObject2(Move | HasCharacter | HasMatrix) on depth 1 (tombstoned!)
    // + DoAction: count += 1; trace("f2:" + count); stop();
    let mut move_d1 = vec![0x07]; // HasMatrix | HasCharacter | Move
    move_d1.extend_from_slice(&1_u16.to_le_bytes());
    move_d1.extend_from_slice(&2_u16.to_le_bytes());
    move_d1.extend_from_slice(&swf_matrix_bytes(None, None, 99.0, 8.0));
    swf_tag(&mut tags, 26, &move_d1);

    let mut f2 = Vec::new();
    avm1_push_str(&mut f2, "count");
    avm1_push_str(&mut f2, "count");
    f2.push(0x1C);
    avm1_push_int(&mut f2, 1);
    f2.extend_from_slice(&[0x47, 0x1D]);
    avm1_push_str(&mut f2, "f2:");
    avm1_push_str(&mut f2, "count");
    f2.extend_from_slice(&[0x1C, 0x47, 0x26, 0x07, 0x00]);
    swf_tag(&mut tags, 12, &f2);
    swf_tag(&mut tags, 1, &[]);

    let swf_bytes = wrap_swf(SwfCompression::Zlib, 8, 3, &tags);
    let mut player = SwfBytecodePlayer::from_bytes(&swf_bytes).unwrap();

    let s0 = player.step_frame();
    assert_eq!(s0.traces, vec!["f0:1:10"]);
    assert_eq!(
        s0.display_list
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        vec!["hero", "badge"]
    );

    let s1 = player.step_frame();
    assert_eq!(s1.traces, vec!["f1:15"]);
    assert_eq!(
        s1.display_list
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        vec!["badge", "badge_dup", "spark"]
    );

    let s2 = player.step_frame();
    assert_eq!(s2.traces, vec!["f2:16"]);
    assert!(
        s2.display_list
            .iter()
            .all(|e| e.depth != 1 && e.name != "hero"),
        "removed-depth tombstone on depth 1 must block Move resurrection on frame 2: {:?}",
        s2.display_list
    );

    // Backward seek to frame 0: dynamic/duplicated clips (`badge_dup`, `spark`) and depth-1
    // tombstone MUST be cleared so `hero` reappears at x=12 and `DoInitAction` + `count` reset!
    let seek0 = player.seek_to_frame(0);
    assert_eq!(seek0, s0);
    // Stepping forward from seek0 must deterministically match s1 and s2:
    assert_eq!(player.step_frame(), s1);
    assert_eq!(player.step_frame(), s2);

    // Backward seek to frame 1, then step forward to frame 2:
    let seek1 = player.seek_to_frame(1);
    assert_eq!(seek1, s1);
    assert_eq!(player.step_frame(), s2);

    // Explicit `restart()` from stopped frame 2 must reset all state and evaluate frame 0 cleanly:
    let restarted = player.restart();
    assert_eq!(restarted, s0);
    assert_eq!(player.step_frame(), s1);
    assert_eq!(player.step_frame(), s2);
}
