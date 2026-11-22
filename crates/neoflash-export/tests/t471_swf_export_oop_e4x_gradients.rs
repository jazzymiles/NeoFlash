//! `[T-471]` Track 1 — AS3/SWF Exporter Deepening Integration Suite:
//! 1. Multi-file custom AS3 `interface` (`INSTANCE_INFO_INTERFACE`, `0x05` `ClassSealed | ClassInterface`,
//!    `0x08` interface namespace `Trait_Method` without code body, `0x09` `CONSTANT_Multiname` in
//!    `InstanceInfo.interfaces`), `class Child extends Base implements IFoo` across project source units,
//!    `document_class` & symbol `class_name` linkages, topological `DoABC2` `sinit` `newclass` creation
//!    order, `constructsuper` (`0x49`) explicit & implicit, `getsuper` (`0x04`) / `setsuper` (`0x05`) /
//!    `callsuper` (`0x45`) / `callsupervoid` (`0x4E`), `TRAIT_Override` (`0x21`), and `is` / `as` /
//!    `instanceof` — verified via raw ABC binary inspection, `SwfBytecodePlayer` execution, and
//!    `import_swf_bytes` (`import_project`) decompilation + re-export round-trip, plus strict preflight
//!    negative checks.
//! 2. AS3 E4X XML & XMLList literals (`<catalog ...><item id={k}>{label}</item></catalog>`, `<>...</>`),
//!    `esc_xelem` (`0x71`) & `esc_xattr` (`0x72`) + `construct` `XML`/`XMLList` (`0x42`), `@id` / `@["id"]`
//!    read/write/delete (`QNameA` `0x0D`, `MultinameA` `0x0E`, `MultinameLA` `0x1C`), `..item` & `..@id`
//!    descendants (`getdescendants` `0x59`), and `default xml namespace = ...` (`dxns` `0x06`, `dxnslate`
//!    `0x07`, `SET_DXNS = 0x40` method flag) — verified via raw ABC inspection, `SwfBytecodePlayer`
//!    execution, and `import_swf_bytes` round-trip.
//! 3. `>15` gradient stops deterministic resampling in non-strict SWF export (`15` strictly monotonic
//!    unique `GRADRECORD` stops preserving exact first/last endpoints via greedy Douglas-Peucker RGBA
//!    error reduction + located `swf_gradient_stops_approximated` warning + clean re-import), while
//!    strict mode (`SwfExportOptions { strict: true, .. }`) blocks with `swf_gradient_stop_count_limit`.

use neoflash_core::{
    Color, Element, ElementKind, FillRule, FrameScript, GradientKind, GradientPaint,
    GradientSpread, GradientStop, InstancePlayback, Keyframe, Layer, LayerProperties, PathCommand,
    Project, Script, ScriptLanguage, SourceUnit, Symbol, SymbolKind, Timeline,
};
use neoflash_export::{
    ExportError, FlaCompatibilityStatus, SwfCompression, SwfExportOptions, SwfPublishOptions,
    SwfPublishProfile, SwfVmKind, export_swf, export_swf_bytes, import_swf_bytes, play_swf_bytes,
    preflight_swf_export, preflight_swf_publish, publish_swf_bytes,
};
use neoflash_import::SwfBytecodePlayer;
use std::collections::{HashMap, HashSet};

fn collect_uncompressed_swf_tags(swf_bytes: &[u8]) -> Vec<(u16, Vec<u8>)> {
    assert_eq!(&swf_bytes[0..3], b"FWS", "expected uncompressed FWS header");
    let nbits = (swf_bytes[8] >> 3) as usize;
    let rect_bits = 5 + nbits * 4;
    let rect_bytes = rect_bits.div_ceil(8);
    let mut pos = 8 + rect_bytes + 4;
    let mut out = Vec::new();
    while pos + 2 <= swf_bytes.len() {
        let hdr = u16::from_le_bytes([swf_bytes[pos], swf_bytes[pos + 1]]);
        pos += 2;
        let code = hdr >> 6;
        let mut len = usize::from(hdr & 0x3f);
        if len == 0x3f {
            assert!(pos + 4 <= swf_bytes.len());
            len = u32::from_le_bytes([
                swf_bytes[pos],
                swf_bytes[pos + 1],
                swf_bytes[pos + 2],
                swf_bytes[pos + 3],
            ]) as usize;
            pos += 4;
        }
        assert!(pos + len <= swf_bytes.len());
        out.push((code, swf_bytes[pos..pos + len].to_vec()));
        pos += len;
        if code == 0 {
            break;
        }
    }
    out
}

fn extract_do_abc2_bytes(swf_bytes: &[u8]) -> Vec<u8> {
    let tags = collect_uncompressed_swf_tags(swf_bytes);
    let (_, payload) = tags
        .into_iter()
        .find(|(code, _)| *code == 82)
        .expect("expected DoABC2 tag (82)");
    let nul_off = payload[4..]
        .iter()
        .position(|&b| b == 0)
        .expect("DoABC2 C-string name");
    payload[4 + nul_off + 1..].to_vec()
}

fn read_u30(buf: &[u8], pos: &mut usize) -> u32 {
    let mut out = 0u32;
    let mut shift = 0u32;
    for _ in 0..5 {
        let b = buf[*pos];
        *pos += 1;
        out |= u32::from(b & 0x7f) << shift;
        if (b & 0x80) == 0 {
            break;
        }
        shift += 7;
    }
    out
}

#[derive(Clone, Debug)]
struct AbcTraitSummary {
    name_mn: u32,
    kind_byte: u8,
    method_idx: Option<u32>,
}

#[derive(Clone, Debug)]
struct AbcInstanceSummary {
    name_mn: u32,
    super_mn: u32,
    flags: u8,
    interfaces: Vec<u32>,
    iinit: u32,
    traits: Vec<AbcTraitSummary>,
    cinit: u32,
}

#[derive(Clone, Debug)]
struct AbcBodySummary {
    opcodes: Vec<u8>,
    newclass_order: Vec<u32>,
}

#[derive(Clone, Debug)]
struct ParsedAbcSummary {
    strings: Vec<String>,
    namespaces: Vec<(u8, String)>,
    multinames: Vec<(u8, u32, u32)>,
    method_flags: Vec<u8>,
    instances: Vec<AbcInstanceSummary>,
    sinit_method: u32,
    bodies: HashMap<u32, AbcBodySummary>,
}

impl ParsedAbcSummary {
    fn mn_short_name(&self, mn_idx: u32) -> String {
        if mn_idx == 0 {
            return "*".to_owned();
        }
        let (kind, a, b) = self.multinames[mn_idx as usize];
        match kind {
            0x07 | 0x0D => {
                if b == 0 {
                    "*".to_owned()
                } else {
                    self.strings[b as usize].clone()
                }
            }
            0x09 | 0x0E => {
                if a == 0 {
                    "*".to_owned()
                } else {
                    self.strings[a as usize].clone()
                }
            }
            _ => format!("mn#{mn_idx}"),
        }
    }

    fn mn_qname_ns(&self, mn_idx: u32) -> (u8, String) {
        let (kind, a, _) = self.multinames[mn_idx as usize];
        assert!(matches!(kind, 0x07 | 0x0D));
        self.namespaces[a as usize].clone()
    }
}

fn parse_abc_summary(abc: &[u8]) -> ParsedAbcSummary {
    let mut pos = 4usize; // minor(2) + major(2)
    let int_cnt = read_u30(abc, &mut pos);
    for _ in 1..int_cnt {
        let _ = read_u30(abc, &mut pos);
    }
    let uint_cnt = read_u30(abc, &mut pos);
    for _ in 1..uint_cnt {
        let _ = read_u30(abc, &mut pos);
    }
    let dbl_cnt = read_u30(abc, &mut pos);
    if dbl_cnt > 1 {
        pos += ((dbl_cnt - 1) as usize) * 8;
    }

    let str_cnt = read_u30(abc, &mut pos);
    let mut strings = vec![String::new()];
    for _ in 1..str_cnt {
        let slen = read_u30(abc, &mut pos) as usize;
        strings.push(
            std::str::from_utf8(&abc[pos..pos + slen])
                .unwrap()
                .to_owned(),
        );
        pos += slen;
    }

    let ns_cnt = read_u30(abc, &mut pos);
    let mut namespaces = vec![(0u8, String::new())];
    for _ in 1..ns_cnt {
        let kind = abc[pos];
        pos += 1;
        let s_idx = read_u30(abc, &mut pos) as usize;
        namespaces.push((kind, strings.get(s_idx).cloned().unwrap_or_default()));
    }

    let nss_cnt = read_u30(abc, &mut pos);
    for _ in 1..nss_cnt {
        let c = read_u30(abc, &mut pos);
        for _ in 0..c {
            let _ = read_u30(abc, &mut pos);
        }
    }

    let mn_cnt = read_u30(abc, &mut pos);
    let mut multinames = vec![(0u8, 0u32, 0u32)];
    for _ in 1..mn_cnt {
        let kind = abc[pos];
        pos += 1;
        match kind {
            0x07 | 0x0D | 0x09 | 0x0E => {
                let a = read_u30(abc, &mut pos);
                let b = read_u30(abc, &mut pos);
                multinames.push((kind, a, b));
            }
            0x0F | 0x10 | 0x1B | 0x1C => {
                let a = read_u30(abc, &mut pos);
                multinames.push((kind, a, 0));
            }
            0x11 | 0x12 => {
                multinames.push((kind, 0, 0));
            }
            0x1D => {
                let base = read_u30(abc, &mut pos);
                let tc = read_u30(abc, &mut pos);
                let mut first = 0;
                for i in 0..tc {
                    let p = read_u30(abc, &mut pos);
                    if i == 0 {
                        first = p;
                    }
                }
                multinames.push((kind, base, first));
            }
            other => panic!("unexpected multiname kind 0x{other:02X}"),
        }
    }

    let method_cnt = read_u30(abc, &mut pos);
    let mut method_flags = Vec::with_capacity(method_cnt as usize);
    for _ in 0..method_cnt {
        let param_cnt = read_u30(abc, &mut pos);
        let _ret_type = read_u30(abc, &mut pos);
        for _ in 0..param_cnt {
            let _ = read_u30(abc, &mut pos);
        }
        let _name = read_u30(abc, &mut pos);
        let flags = abc[pos];
        pos += 1;
        method_flags.push(flags);
        if (flags & 0x08) != 0 {
            let opt_cnt = read_u30(abc, &mut pos);
            for _ in 0..opt_cnt {
                let _ = read_u30(abc, &mut pos);
                pos += 1;
            }
        }
        if (flags & 0x80) != 0 {
            for _ in 0..param_cnt {
                let _ = read_u30(abc, &mut pos);
            }
        }
    }

    let meta_cnt = read_u30(abc, &mut pos);
    for _ in 0..meta_cnt {
        let _ = read_u30(abc, &mut pos);
        let ic = read_u30(abc, &mut pos);
        for _ in 0..ic {
            let _ = read_u30(abc, &mut pos);
            let _ = read_u30(abc, &mut pos);
        }
    }

    let read_traits = |abc: &[u8], pos: &mut usize| -> Vec<AbcTraitSummary> {
        let tcnt = read_u30(abc, pos);
        let mut out = Vec::with_capacity(tcnt as usize);
        for _ in 0..tcnt {
            let name_mn = read_u30(abc, pos);
            let kind_byte = abc[*pos];
            *pos += 1;
            let kind = kind_byte & 0x0f;
            let attr = kind_byte >> 4;
            let mut method_idx = None;
            match kind {
                0 | 6 => {
                    let _slot_id = read_u30(abc, pos);
                    let _type_name = read_u30(abc, pos);
                    let vindex = read_u30(abc, pos);
                    if vindex != 0 {
                        *pos += 1;
                    }
                }
                1..=3 | 5 => {
                    let _disp_id = read_u30(abc, pos);
                    method_idx = Some(read_u30(abc, pos));
                }
                4 => {
                    let _slot_id = read_u30(abc, pos);
                    let _classi = read_u30(abc, pos);
                }
                other => panic!("unexpected trait kind {other}"),
            }
            if (attr & 0x04) != 0 {
                let mc = read_u30(abc, pos);
                for _ in 0..mc {
                    let _ = read_u30(abc, pos);
                }
            }
            out.push(AbcTraitSummary {
                name_mn,
                kind_byte,
                method_idx,
            });
        }
        out
    };

    let class_cnt = read_u30(abc, &mut pos) as usize;
    let mut instances = Vec::with_capacity(class_cnt);
    for _ in 0..class_cnt {
        let name_mn = read_u30(abc, &mut pos);
        let super_mn = read_u30(abc, &mut pos);
        let flags = abc[pos];
        pos += 1;
        if (flags & 0x08) != 0 {
            let _ = read_u30(abc, &mut pos);
        }
        let intf_cnt = read_u30(abc, &mut pos);
        let mut interfaces = Vec::with_capacity(intf_cnt as usize);
        for _ in 0..intf_cnt {
            interfaces.push(read_u30(abc, &mut pos));
        }
        let iinit = read_u30(abc, &mut pos);
        let traits = read_traits(abc, &mut pos);
        instances.push(AbcInstanceSummary {
            name_mn,
            super_mn,
            flags,
            interfaces,
            iinit,
            traits,
            cinit: 0,
        });
    }
    for inst in &mut instances {
        inst.cinit = read_u30(abc, &mut pos);
        let _ = read_traits(abc, &mut pos);
    }

    let script_cnt = read_u30(abc, &mut pos);
    assert_eq!(script_cnt, 1);
    let sinit_method = read_u30(abc, &mut pos);
    let _ = read_traits(abc, &mut pos);

    let body_cnt = read_u30(abc, &mut pos);
    let mut bodies = HashMap::new();
    for _ in 0..body_cnt {
        let method_idx = read_u30(abc, &mut pos);
        let _max_stack = read_u30(abc, &mut pos);
        let _local_cnt = read_u30(abc, &mut pos);
        let _init_scope_depth = read_u30(abc, &mut pos);
        let _max_scope_depth = read_u30(abc, &mut pos);
        let code_len = read_u30(abc, &mut pos) as usize;
        let code = &abc[pos..pos + code_len];
        pos += code_len;

        let mut opcodes = Vec::new();
        let mut newclass_order = Vec::new();
        let mut cpos = 0usize;
        while cpos < code.len() {
            let op = code[cpos];
            cpos += 1;
            opcodes.push(op);
            match op {
                0x0C..=0x1A => cpos += 3,
                0x1B => {
                    cpos += 3;
                    let cnt = read_u30(code, &mut cpos) as usize;
                    cpos += (cnt + 1) * 3;
                }
                0x24 | 0x65 => cpos += 1,
                0x58 => {
                    newclass_order.push(read_u30(code, &mut cpos));
                }
                0x04..=0x06
                | 0x08
                | 0x25
                | 0x2C..=0x2F
                | 0x31
                | 0x40..=0x42
                | 0x49
                | 0x53
                | 0x55
                | 0x56
                | 0x59..=0x63
                | 0x66..=0x68
                | 0x6A
                | 0x6C..=0x6F
                | 0x80
                | 0x86
                | 0x92
                | 0x94
                | 0xB2
                | 0xC2
                | 0xC3 => {
                    let _ = read_u30(code, &mut cpos);
                }
                0x32 | 0x43..=0x46 | 0x4A | 0x4C | 0x4E | 0x4F => {
                    let _ = read_u30(code, &mut cpos);
                    let _ = read_u30(code, &mut cpos);
                }
                0xEF => {
                    cpos += 1;
                    let _ = read_u30(code, &mut cpos);
                    cpos += 1;
                    let _ = read_u30(code, &mut cpos);
                }
                _ => {}
            }
        }

        let exc_cnt = read_u30(abc, &mut pos);
        for _ in 0..exc_cnt {
            for _ in 0..5 {
                let _ = read_u30(abc, &mut pos);
            }
        }
        let _ = read_traits(abc, &mut pos);
        bodies.insert(
            method_idx,
            AbcBodySummary {
                opcodes,
                newclass_order,
            },
        );
    }

    ParsedAbcSummary {
        strings,
        namespaces,
        multinames,
        method_flags,
        instances,
        sinit_method,
        bodies,
    }
}

struct SwfBitSliceReader<'a> {
    buf: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,
}

impl<'a> SwfBitSliceReader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self {
            buf,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    fn align(&mut self) {
        if self.bit_pos != 0 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
    }

    fn u8(&mut self) -> u8 {
        self.align();
        let b = self.buf[self.byte_pos];
        self.byte_pos += 1;
        b
    }

    fn u16(&mut self) -> u16 {
        let lo = self.u8();
        let hi = self.u8();
        u16::from_le_bytes([lo, hi])
    }

    fn i16(&mut self) -> i16 {
        self.u16() as i16
    }

    fn ub(&mut self, n: usize) -> u32 {
        let mut v = 0u32;
        for _ in 0..n {
            let bit = (self.buf[self.byte_pos] >> (7 - self.bit_pos)) & 1;
            v = (v << 1) | u32::from(bit);
            self.bit_pos += 1;
            if self.bit_pos == 8 {
                self.bit_pos = 0;
                self.byte_pos += 1;
            }
        }
        v
    }

    fn skip_rect(&mut self) {
        self.align();
        let nbits = self.ub(5) as usize;
        for _ in 0..4 {
            let _ = self.ub(nbits);
        }
        self.align();
    }

    fn skip_matrix(&mut self) {
        self.align();
        if self.ub(1) == 1 {
            let n = self.ub(5) as usize;
            let _ = self.ub(n);
            let _ = self.ub(n);
        }
        if self.ub(1) == 1 {
            let n = self.ub(5) as usize;
            let _ = self.ub(n);
            let _ = self.ub(n);
        }
        let nt = self.ub(5) as usize;
        let _ = self.ub(nt);
        let _ = self.ub(nt);
        self.align();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExtractedSwfGradient {
    fill_type: u8,
    spread_bits: u8,
    interp_bits: u8,
    stops: Vec<(u8, Color)>,
    focal_fixed8: Option<i16>,
}

fn extract_shape_gradients_from_tag(tag_code: u16, payload: &[u8]) -> Vec<ExtractedSwfGradient> {
    assert!(matches!(tag_code, 32 | 83));
    let mut r = SwfBitSliceReader::new(payload);
    let _shape_id = r.u16();
    r.skip_rect();
    if tag_code == 83 {
        r.skip_rect();
        let _flags = r.u8();
    }

    let mut out = Vec::new();
    let mut fill_cnt = usize::from(r.u8());
    if fill_cnt == 0xff {
        fill_cnt = usize::from(r.u16());
    }
    let read_grad = |r: &mut SwfBitSliceReader<'_>, fill_type: u8| -> ExtractedSwfGradient {
        r.skip_matrix();
        let hdr = r.u8();
        let spread_bits = (hdr >> 6) & 0x03;
        let interp_bits = (hdr >> 4) & 0x03;
        let count = usize::from(hdr & 0x0f);
        let mut stops = Vec::with_capacity(count);
        for _ in 0..count {
            let ratio = r.u8();
            let rgba = [r.u8(), r.u8(), r.u8(), r.u8()];
            stops.push((ratio, rgba));
        }
        let focal_fixed8 = (fill_type == 0x13).then(|| r.i16());
        ExtractedSwfGradient {
            fill_type,
            spread_bits,
            interp_bits,
            stops,
            focal_fixed8,
        }
    };

    for _ in 0..fill_cnt {
        let ft = r.u8();
        match ft {
            0x00 => {
                for _ in 0..4 {
                    let _ = r.u8();
                }
            }
            0x10 | 0x12 | 0x13 => out.push(read_grad(&mut r, ft)),
            0x40..=0x43 => {
                let _ = r.u16();
                r.skip_matrix();
            }
            other => panic!("unexpected fill style 0x{other:02X}"),
        }
    }

    let mut line_cnt = usize::from(r.u8());
    if line_cnt == 0xff {
        line_cnt = usize::from(r.u16());
    }
    for _ in 0..line_cnt {
        let _width = r.u16();
        if tag_code == 83 {
            let flags_hi = r.u8();
            let _flags_lo = r.u8();
            let join_bits = (flags_hi >> 4) & 0x03;
            let has_fill = (flags_hi & 0x08) != 0;
            if join_bits == 2 {
                let _miter = r.u16();
            }
            if has_fill {
                let ft = r.u8();
                assert!(matches!(ft, 0x10 | 0x12 | 0x13));
                out.push(read_grad(&mut r, ft));
            } else {
                for _ in 0..4 {
                    let _ = r.u8();
                }
            }
        } else {
            for _ in 0..4 {
                let _ = r.u8();
            }
        }
    }

    out
}

#[test]
fn t471_as3_multifile_interfaces_inheritance_super_override_and_roundtrip() {
    let expected_traces = vec![
        "CTOR:Base(10,hero:alpha)->HeroBadge(bonus=5)".to_owned(),
        "COMPUTE:39|AS_SCORABLE:39".to_owned(),
        "SUPER_PROP_VOID:seed=28|desc=hero:alpha#28|tag=TAG:hero:alpha#28".to_owned(),
        "IMPLICIT_SUPER:Sidekick[ok]|overrideToString=true".to_owned(),
        "STATIC_IS:scorable=true,taggable=true,ident=true,base=true,sidekick=false".to_owned(),
        "LATE_IS_AS:dynTaggable=true,dynScorable=true,dynSidekick=false,castTag=TAG:hero:alpha#28,nullCast=null".to_owned(),
        "INSTANCEOF:base=true,hero=true,sidekick=false".to_owned(),
        "STAGE_LINKED:placedHero=true,placedCompute=24".to_owned(),
    ];

    for profile in [
        SwfPublishProfile::FlashPlayer9Avm2,
        SwfPublishProfile::FlashPlayer10Avm2,
    ] {
        let mut project = Project::blank();
        project.name = "T471 Multi-File AS3 OOP".to_owned();
        project.document_class = Some("app.MainDoc".into());

        let badge_sym = Symbol {
            id: "sym_badge".into(),
            name: "HeroBadgeClip".into(),
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            linkage_id: None,
            class_name: Some("pkg.entities.HeroBadge".into()),
            timeline: Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: "badge_shape_layer".into(),
                    name: "Art".into(),
                    visible: true,
                    locked: false,
                    properties: LayerProperties::default(),
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![Element {
                            id: "badge_rect".into(),
                            kind: ElementKind::Rectangle {
                                width: 24.0,
                                height: 24.0,
                                fill: [40, 120, 220, 255],
                                stroke: [0, 0, 0, 0],
                                stroke_width: 0.0,
                            },
                            ..Element::default()
                        }],
                        ..Keyframe::default()
                    }],
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            },
        };
        project.symbols.push(badge_sym);

        let root_id = project.root.clone();
        {
            let root_sym = project.symbol_mut(&root_id).expect("root symbol");
            root_sym.timeline.frame_count = 1;
            root_sym.timeline.layers = vec![Layer {
                id: "layer_stage".into(),
                name: "Stage".into(),
                visible: true,
                locked: false,
                properties: LayerProperties::default(),
                keyframes: vec![Keyframe {
                    frame: 0,
                    elements: vec![Element {
                        id: "placed_badge_inst".into(),
                        name: "placedBadge".into(),
                        kind: ElementKind::Instance {
                            symbol_id: "sym_badge".into(),
                            playback: InstancePlayback::Independent,
                        },
                        ..Element::default()
                    }],
                    ..Keyframe::default()
                }],
            }];
        }

        // Deliberately register sources in reverse dependency order (MainDoc -> Child -> SubInterface -> Base -> RootInterfaces)
        // to prove `DoABC2` `sinit` orders `newclass` topologically across superclasses and interfaces!
        project.sources = vec![
            SourceUnit {
                id: "src_main".into(),
                path: "src/app/MainDoc.as".into(),
                language: ScriptLanguage::ActionScript3,
                source: r##"package app {
    import flash.display.MovieClip;
    import pkg.contracts.*;
    import pkg.core.BaseEntity;
    import pkg.entities.HeroBadge;
    import pkg.entities.SidekickBadge;

    public class MainDoc extends MovieClip {
        public function MainDoc() {
            super();
            var hero:HeroBadge = new HeroBadge(10, "alpha", 5);
            trace("CTOR:" + hero.ctorLog);
            var asScorable:IScorable = hero as IScorable;
            trace("COMPUTE:" + hero.compute(12) + "|AS_SCORABLE:" + asScorable.compute(12));
            hero.resetSeed(17);
            trace("SUPER_PROP_VOID:seed=" + hero.seed + "|desc=" + hero.describe() + "|tag=" + hero.badgeTag());

            var side:SidekickBadge = new SidekickBadge();
            trace("IMPLICIT_SUPER:" + side.toString() + "|overrideToString=" + (side.toString() == "Sidekick[ok]"));

            trace(
                "STATIC_IS:scorable=" + (hero is IScorable)
                + ",taggable=" + (hero is ITaggable)
                + ",ident=" + (hero is IIdentifiable)
                + ",base=" + (hero is BaseEntity)
                + ",sidekick=" + (hero is SidekickBadge)
            );

            var dynTaggable:* = ITaggable;
            var dynScorable:* = IScorable;
            var dynSide:* = SidekickBadge;
            var dynCast:* = hero as dynTaggable;
            var badCast:* = hero as SidekickBadge;
            trace(
                "LATE_IS_AS:dynTaggable=" + (hero is dynTaggable)
                + ",dynScorable=" + (hero is dynScorable)
                + ",dynSidekick=" + (hero is dynSide)
                + ",castTag=" + dynCast.badgeTag()
                + ",nullCast=" + badCast
            );

            trace(
                "INSTANCEOF:base=" + (hero instanceof BaseEntity)
                + ",hero=" + (hero instanceof HeroBadge)
                + ",sidekick=" + (hero instanceof SidekickBadge)
            );

            var placed:* = getChildByName("placedBadge");
            trace(
                "STAGE_LINKED:placedHero=" + (placed is ITaggable)
                + ",placedCompute=" + (placed as IScorable).compute(7)
            );
        }
    }
}
"##
                .into(),
            },
            SourceUnit {
                id: "src_hero".into(),
                path: "src/pkg/entities/HeroBadge.as".into(),
                language: ScriptLanguage::ActionScript3,
                source: r##"package pkg.entities {
    import pkg.contracts.ITaggable;
    import pkg.core.BaseEntity;

    public class HeroBadge extends BaseEntity implements ITaggable {
        public var bonus:int = 3;
        public var ctorLog:String = "";

        public function HeroBadge(base:int, rank:String, extraBonus:int) {
            var b:int = (base ? base : 7);
            var rk:String = (rank ? rank : "stage");
            super(b, "hero:" + rk);
            if (extraBonus) {
                this.bonus = extraBonus;
            }
            this.ctorLog = "Base(" + b + ",hero:" + rk + ")->HeroBadge(bonus=" + this.bonus + ")";
        }

        override public function compute(x:int):int {
            return super.compute(x * 2) + this.bonus;
        }

        override public function resetSeed(v:int):void {
            super.resetSeed(v + 1);
            super.seed = super.seed + 10;
        }

        public function badgeTag():String {
            return "TAG:" + super.describe();
        }
    }
}
"##
                .into(),
            },
            SourceUnit {
                id: "src_sidekick".into(),
                path: "src/pkg/entities/SidekickBadge.as".into(),
                language: ScriptLanguage::ActionScript3,
                source: r##"package pkg.entities {
    import flash.display.Sprite;

    public class SidekickBadge extends Sprite {
        public var status:String = "ok";

        override public function toString():String {
            return "Sidekick[" + this.status + "]";
        }
    }
}
"##
                .into(),
            },
            SourceUnit {
                id: "src_itaggable".into(),
                path: "src/pkg/contracts/ITaggable.as".into(),
                language: ScriptLanguage::ActionScript3,
                source: r##"package pkg.contracts {
    public interface ITaggable extends IScorable {
        function badgeTag():String;
    }
}
"##
                .into(),
            },
            SourceUnit {
                id: "src_base".into(),
                path: "src/pkg/core/BaseEntity.as".into(),
                language: ScriptLanguage::ActionScript3,
                source: r##"package pkg.core {
    import flash.display.MovieClip;
    import pkg.contracts.IIdentifiable;

    public class BaseEntity extends MovieClip implements IIdentifiable {
        public var seed:int = 5;
        public var baseTitle:String = "unset";

        public function BaseEntity(s:int, title:String) {
            super();
            this.seed = s;
            this.baseTitle = title;
        }

        public function compute(x:int):int {
            return this.seed + x;
        }

        public function resetSeed(v:int):void {
            this.seed = v;
        }

        public function describe():String {
            return this.baseTitle + "#" + this.seed;
        }
    }
}
"##
                .into(),
            },
            SourceUnit {
                id: "src_iscorable".into(),
                path: "src/pkg/contracts/IScorable.as".into(),
                language: ScriptLanguage::ActionScript3,
                source: r##"package pkg.contracts {
    public interface IScorable {
        function compute(x:int):int;
    }
}
"##
                .into(),
            },
            SourceUnit {
                id: "src_iident".into(),
                path: "src/pkg/contracts/IIdentifiable.as".into(),
                language: ScriptLanguage::ActionScript3,
                source: r##"package pkg.contracts {
    public interface IIdentifiable {
        function describe():String;
    }
}
"##
                .into(),
            },
        ];

        let opts = SwfExportOptions {
            publish: SwfPublishOptions {
                profile,
                compression: SwfCompression::UncompressedFws,
                ..SwfPublishOptions::default()
            },
            strict: true,
        };
        let preflight = preflight_swf_export(&project, &opts);
        assert!(
            preflight.can_proceed(),
            "strict preflight must accept multi-file AS3 interfaces + inheritance ({profile:?}): {:?}",
            preflight.blocking_items()
        );

        let (swf_bytes, _) = export_swf_bytes(&project, &opts).expect("export multi-file OOP SWF");
        let abc_bytes = extract_do_abc2_bytes(&swf_bytes);
        let abc = parse_abc_summary(&abc_bytes);

        let pos_of = |short_name: &str| -> usize {
            abc.instances
                .iter()
                .position(|inst| abc.mn_short_name(inst.name_mn) == short_name)
                .unwrap_or_else(|| panic!("missing class/interface {short_name} in DoABC2"))
        };

        let i_scorable = pos_of("IScorable");
        let i_taggable = pos_of("ITaggable");
        let i_ident = pos_of("IIdentifiable");
        let c_base = pos_of("BaseEntity");
        let c_hero = pos_of("HeroBadge");
        let c_side = pos_of("SidekickBadge");

        // 1. Verify interface InstanceInfo binary contract (`0x05`, `super_mn == 0`, no iinit/method bodies, `0x08` interface namespace)
        for &(idx, iface_ns_expected, m_expected) in &[
            (i_scorable, "pkg.contracts:IScorable", "compute"),
            (i_taggable, "pkg.contracts:ITaggable", "badgeTag"),
            (i_ident, "pkg.contracts:IIdentifiable", "describe"),
        ] {
            let inst = &abc.instances[idx];
            assert_eq!(
                inst.flags, 0x05,
                "interface must set ClassSealed | ClassInterface (0x05)"
            );
            assert_eq!(inst.super_mn, 0, "interface super_name must be 0");
            assert!(
                !abc.bodies.contains_key(&inst.iinit),
                "interface iinit must not emit a MethodBodyInfo"
            );
            assert_eq!(
                abc.bodies[&inst.cinit].opcodes,
                vec![0x47],
                "interface cinit must emit minimal returnvoid body"
            );
            assert_eq!(inst.traits.len(), 1);
            let tr = &inst.traits[0];
            assert_eq!(abc.mn_short_name(tr.name_mn), m_expected);
            assert_eq!(
                abc.mn_qname_ns(tr.name_mn),
                (0x08, iface_ns_expected.to_owned()),
                "interface method trait must live in CONSTANT_Namespace (0x08)"
            );
            assert!(
                !abc.bodies.contains_key(&tr.method_idx.unwrap()),
                "interface method trait must not have a MethodBodyInfo"
            );
        }

        // 2. Verify `InstanceInfo.interfaces` uses CONSTANT_Multiname (0x09)
        assert_eq!(abc.instances[i_taggable].interfaces.len(), 1);
        let itag_sup_mn = abc.instances[i_taggable].interfaces[0];
        assert_eq!(abc.multinames[itag_sup_mn as usize].0, 0x09);
        assert_eq!(abc.mn_short_name(itag_sup_mn), "IScorable");

        assert_eq!(abc.instances[c_hero].interfaces.len(), 1);
        let hero_iface_mn = abc.instances[c_hero].interfaces[0];
        assert_eq!(abc.multinames[hero_iface_mn as usize].0, 0x09);
        assert_eq!(abc.mn_short_name(hero_iface_mn), "ITaggable");

        // 3. Verify topological `newclass` creation order in `sinit`
        let sinit_body = &abc.bodies[&abc.sinit_method];
        let expected_seq: Vec<u32> = (0..abc.instances.len() as u32).collect();
        assert_eq!(sinit_body.newclass_order, expected_seq);
        assert!(i_scorable < i_taggable, "IScorable before ITaggable");
        assert!(i_taggable < c_hero, "ITaggable before HeroBadge");
        assert!(i_ident < c_base, "IIdentifiable before BaseEntity");
        assert!(c_base < c_hero, "BaseEntity before HeroBadge");
        assert!(
            sinit_body.opcodes.contains(&0x20),
            "sinit must pushnull (0x20) for interface newclass"
        );

        // 4. Verify TRAIT_Override (0x21) and super opcodes (0x49, 0x04, 0x05, 0x45, 0x4E)
        for mname in ["compute", "resetSeed"] {
            let tr = abc.instances[c_hero]
                .traits
                .iter()
                .find(|t| abc.mn_short_name(t.name_mn) == mname)
                .unwrap();
            assert_eq!(
                tr.kind_byte, 0x21,
                "HeroBadge.{mname} must set TRAIT_Override (0x21)"
            );
        }
        let side_tostring = abc.instances[c_side]
            .traits
            .iter()
            .find(|t| abc.mn_short_name(t.name_mn) == "toString")
            .unwrap();
        assert_eq!(
            side_tostring.kind_byte, 0x21,
            "SidekickBadge.toString must set TRAIT_Override (0x21)"
        );

        let all_ops: HashSet<u8> = abc
            .bodies
            .values()
            .flat_map(|b| b.opcodes.iter().copied())
            .collect();
        for &(op, label) in &[
            (0x49, "constructsuper"),
            (0x04, "getsuper"),
            (0x05, "setsuper"),
            (0x45, "callsuper"),
            (0x4E, "callsupervoid"),
            (0xB1, "instanceof"),
            (0xB2, "istype"),
            (0xB3, "istypelate"),
            (0x86, "astype"),
            (0x87, "astypelate"),
        ] {
            assert!(
                all_ops.contains(&op),
                "expected {label} (0x{op:02X}) in DoABC2 ({profile:?})"
            );
        }

        // 5. Execute in SwfBytecodePlayer!
        let playback = play_swf_bytes(&swf_bytes, 1).expect("execute OOP SWF in SwfBytecodePlayer");
        assert_eq!(playback.vm_kind, SwfVmKind::Avm2);
        assert!(
            playback.unsupported_diagnostics.is_empty(),
            "zero unsupported diagnostics ({profile:?}): {:?}",
            playback.unsupported_diagnostics
        );
        assert_eq!(
            playback.trace_output, expected_traces,
            "profile={profile:?}"
        );

        // 6. Round-trip via `import_swf_bytes` (`import_project`) -> validate -> re-export -> SwfBytecodePlayer!
        let reimported = import_swf_bytes(&swf_bytes).expect("decompile OOP SWF to Project");
        reimported
            .validate()
            .expect("reimported multi-file AS3 OOP project must validate cleanly");
        assert!(
            reimported
                .sources
                .iter()
                .any(|s| s.source.contains("interface IScorable")),
            "decompiled sources must preserve interface IScorable"
        );
        assert!(
            reimported
                .sources
                .iter()
                .any(|s| s.source.contains("interface ITaggable extends IScorable")),
            "decompiled sources must preserve interface ITaggable extends IScorable"
        );
        assert!(
            reimported.sources.iter().any(|s| {
                s.source
                    .contains("class HeroBadge extends BaseEntity implements ITaggable")
            }),
            "decompiled sources must preserve HeroBadge extends BaseEntity implements ITaggable"
        );

        let (re_swf, _) = export_swf_bytes(&reimported, &opts).expect("re-export decompiled OOP");
        let mut player2 = SwfBytecodePlayer::from_bytes(&re_swf).expect("load round-trip OOP SWF");
        let _ = player2.step_frame();
        let playback2 = play_swf_bytes(&re_swf, 1).expect("play round-trip OOP SWF");
        assert_eq!(
            playback2.trace_output, expected_traces,
            "round-trip execution traces must match ({profile:?})"
        );
    }
}

#[test]
fn t471_as3_oop_strict_preflight_rejects_invalid_hierarchies() {
    let base_project = |sources: Vec<(&str, &str)>, doc_cls: Option<&str>| -> Project {
        let mut p = Project::blank();
        p.name = "OOP Negative Preflight".into();
        p.document_class = doc_cls.map(str::to_owned);
        p.sources = sources
            .into_iter()
            .enumerate()
            .map(|(i, (path, src))| SourceUnit {
                id: format!("s{i}"),
                path: path.to_owned(),
                language: ScriptLanguage::ActionScript3,
                source: src.to_owned(),
            })
            .collect();
        p
    };
    let opts = SwfExportOptions::strict_for_profile(SwfPublishProfile::FlashPlayer10Avm2);

    // 1. Concrete class missing required method from transitive super-interface
    let missing_method = base_project(
        vec![
            (
                "IBase.as",
                "package { public interface IBase { function alpha():int; } }",
            ),
            (
                "ISub.as",
                "package { public interface ISub extends IBase { function beta():int; } }",
            ),
            (
                "Broken.as",
                "package { public class Broken implements ISub { public function beta():int { return 1; } } }",
            ),
        ],
        None,
    );
    let rep1 = preflight_swf_export(&missing_method, &opts);
    assert!(!rep1.can_proceed());
    assert!(
        rep1.items
            .iter()
            .any(|i| i.code == "swf_as3_unimplemented_interface_method"
                && i.disposition.is_blocking())
    );

    // 2. Class `extends` an interface instead of `implements`
    let extends_iface = base_project(
        vec![
            (
                "IFoo.as",
                "package { public interface IFoo { function run():void; } }",
            ),
            (
                "BadCls.as",
                "package { public class BadCls extends IFoo { public function run():void {} } }",
            ),
        ],
        None,
    );
    let rep2 = preflight_swf_export(&extends_iface, &opts);
    assert!(!rep2.can_proceed());
    assert!(
        rep2.items
            .iter()
            .any(|i| i.code == "swf_as3_class_extends_interface" && i.disposition.is_blocking())
    );

    // 3. Interface `extends` a concrete class
    let iface_extends_cls = base_project(
        vec![
            ("Concrete.as", "package { public class Concrete {} }"),
            (
                "BadIface.as",
                "package { public interface BadIface extends Concrete {} }",
            ),
        ],
        None,
    );
    let rep3 = preflight_swf_export(&iface_extends_cls, &opts);
    assert!(!rep3.can_proceed());
    assert!(
        rep3.items
            .iter()
            .any(|i| i.code == "swf_as3_interface_extends_class" && i.disposition.is_blocking())
    );

    // 4. Class `implements` a concrete class
    let impl_cls = base_project(
        vec![
            ("Concrete.as", "package { public class Concrete {} }"),
            (
                "BadImpl.as",
                "package { public class BadImpl implements Concrete {} }",
            ),
        ],
        None,
    );
    let rep4 = preflight_swf_export(&impl_cls, &opts);
    assert!(!rep4.can_proceed());
    assert!(
        rep4.items
            .iter()
            .any(|i| i.code == "swf_as3_implements_non_interface" && i.disposition.is_blocking())
    );

    // 5. `document_class` bound to an interface
    let doc_is_iface = base_project(
        vec![(
            "IRoot.as",
            "package { public interface IRoot { function go():void; } }",
        )],
        Some("IRoot"),
    );
    let rep5 = preflight_swf_export(&doc_is_iface, &opts);
    assert!(!rep5.can_proceed());
    assert!(
        rep5.items
            .iter()
            .any(|i| i.code == "swf_document_class_is_interface" && i.disposition.is_blocking())
    );

    // 6. Interface method with an implementation body rejected at syntax preflight
    let iface_with_body = base_project(
        vec![(
            "IBadBody.as",
            "package { public interface IBadBody { function f():int { return 1; } } }",
        )],
        None,
    );
    let rep6 = preflight_swf_export(&iface_with_body, &opts);
    assert!(!rep6.can_proceed());
    assert!(
        rep6.items
            .iter()
            .any(|i| i.code == "swf_as3_source_unsupported_syntax" && i.disposition.is_blocking())
    );
}

#[test]
fn t471_as3_e4x_xml_literals_attributes_descendants_dxns_and_roundtrip() {
    let expected_traces = vec![
        "CATALOG_ATTR:NorthHub|FIRST_LABEL:AlphaModule".to_owned(),
        "DESC_COUNT:3|IDS:101,202,303".to_owned(),
        "MUTATE_ATTR:id0=999,dynId=999,extra=gold,deleted=true,afterDel=".to_owned(),
        "XMLLIST:2|FIRST_CODE:X1".to_owned(),
        "DXNS_STATIC:http://neoflash.dev/catalog/v1|DXNS_LATE:http://neoflash.dev/catalog/v2"
            .to_owned(),
    ];

    for profile in [
        SwfPublishProfile::FlashPlayer9Avm2,
        SwfPublishProfile::FlashPlayer10Avm2,
    ] {
        let mut project = Project::blank();
        project.name = "T471 AS3 E4X Suite".into();
        let root_id = project.root.clone();
        project.symbol_mut(&root_id).unwrap().timeline.scripts = vec![FrameScript {
            layer: None,
            frame: 0,
            script: Script {
                language: ScriptLanguage::ActionScript3,
                source: r#"var storeName:String = "NorthHub";
var firstId:int = 101;
var rawLabel:String = "AlphaModule";
var catalog:XML = <catalog store={storeName}>
    <item id={firstId}><label>{rawLabel}</label><tag code="A1"/></item>
    <item id="202"><label>Beta</label><nested><item id="303"><label>Deep</label></item></nested></item>
</catalog>;

trace("CATALOG_ATTR:" + catalog.@store + "|FIRST_LABEL:" + catalog.item[0].label);

var allItems:XMLList = catalog..item;
var allIds:XMLList = catalog..@id;
trace("DESC_COUNT:" + allItems.length() + "|IDS:" + allIds[0] + "," + allIds[1] + "," + allIds[2]);

var firstItem:XML = catalog.item[0];
firstItem.@id = "999";
var idKey:String = "id";
var extraKey:String = "tier";
firstItem.@[extraKey] = "gold";
var extraBefore:String = String(firstItem.@tier);
var wasDeleted:Boolean = delete firstItem.@[extraKey];
trace(
    "MUTATE_ATTR:id0=" + firstItem.@id
    + ",dynId=" + firstItem.@[idKey]
    + ",extra=" + extraBefore
    + ",deleted=" + wasDeleted
    + ",afterDel=" + firstItem.@tier
);

var frag:XMLList = <><badge code="X1"/><badge code="X2"/></>;
trace("XMLLIST:" + frag.length() + "|FIRST_CODE:" + frag[0].@code);

function buildNamespacedSummary(dynUri:String):String {
    default xml namespace = "http://neoflash.dev/catalog/v1";
    var nsDoc1:XML = <manifest><entry/></manifest>;
    var u1:String = nsDoc1.name().uri;
    default xml namespace = dynUri;
    var nsDoc2:XML = <manifest><entry/></manifest>;
    var u2:String = nsDoc2.name().uri;
    return "DXNS_STATIC:" + u1 + "|DXNS_LATE:" + u2;
}
trace(buildNamespacedSummary("http://neoflash.dev/catalog/v2"));
"#
                .into(),
            },
        }];

        let pub_opts = SwfPublishOptions {
            profile,
            compression: SwfCompression::UncompressedFws,
            ..SwfPublishOptions::default()
        };
        let preflight = preflight_swf_publish(&project, &pub_opts);
        assert!(
            preflight.can_proceed(),
            "E4X preflight must succeed ({profile:?}): {:?}",
            preflight.blocking_items()
        );

        let (swf_bytes, _) = publish_swf_bytes(&project, &pub_opts).expect("publish E4X SWF");
        let abc_bytes = extract_do_abc2_bytes(&swf_bytes);
        let abc = parse_abc_summary(&abc_bytes);

        // 1. Verify constant pool includes QNameA (0x0D), MultinameA (0x0E), and MultinameLA (0x1C)
        let mn_kinds: HashSet<u8> = abc.multinames.iter().map(|(k, _, _)| *k).collect();
        for &(kind, name) in &[
            (0x0D, "CONSTANT_QNameA"),
            (0x0E, "CONSTANT_MultinameA"),
            (0x1C, "CONSTANT_MultinameLA"),
        ] {
            assert!(
                mn_kinds.contains(&kind),
                "expected {name} (0x{kind:02X}) in ABC constant pool ({profile:?})"
            );
        }

        // 2. Verify E4X opcodes (esc_xelem 0x71, esc_xattr 0x72, construct 0x42, getdescendants 0x59, dxns 0x06, dxnslate 0x07, deleteproperty 0x6A)
        let all_ops: HashSet<u8> = abc
            .bodies
            .values()
            .flat_map(|b| b.opcodes.iter().copied())
            .collect();
        for &(op, label) in &[
            (0x71, "esc_xelem"),
            (0x72, "esc_xattr"),
            (0x42, "construct"),
            (0x59, "getdescendants"),
            (0x06, "dxns"),
            (0x07, "dxnslate"),
            (0x6A, "deleteproperty"),
        ] {
            assert!(
                all_ops.contains(&op),
                "expected {label} (0x{op:02X}) in E4X DoABC2 ({profile:?})"
            );
        }

        // 3. Verify SET_DXNS (0x40) flag on method_info ONLY for the method containing dxns/dxnslate!
        for (m_idx, body) in &abc.bodies {
            let has_dxns_op = body.opcodes.iter().any(|&op| op == 0x06 || op == 0x07);
            let has_dxns_flag = (abc.method_flags[*m_idx as usize] & 0x40) != 0;
            assert_eq!(
                has_dxns_flag, has_dxns_op,
                "method #{m_idx} SET_DXNS (0x40) flag must match dxns/dxnslate emission"
            );
        }
        assert!(
            abc.method_flags.iter().any(|&f| (f & 0x40) != 0),
            "at least one method must carry SET_DXNS (0x40)"
        );

        // 4. Execute in SwfBytecodePlayer!
        let playback = play_swf_bytes(&swf_bytes, 1).expect("execute E4X SWF in SwfBytecodePlayer");
        assert!(
            playback.unsupported_diagnostics.is_empty(),
            "zero unsupported diagnostics ({profile:?}): {:?}",
            playback.unsupported_diagnostics
        );
        assert_eq!(
            playback.trace_output, expected_traces,
            "profile={profile:?}"
        );

        // 5. Round-trip via `import_swf_bytes` -> validate -> re-export -> SwfBytecodePlayer!
        let reimported = import_swf_bytes(&swf_bytes).expect("decompile E4X SWF to Project");
        reimported
            .validate()
            .expect("reimported E4X project must validate cleanly");
        let (re_swf, _) = publish_swf_bytes(&reimported, &pub_opts).expect("re-publish E4X SWF");
        let playback2 = play_swf_bytes(&re_swf, 1).expect("execute round-trip E4X SWF");
        assert_eq!(
            playback2.trace_output, expected_traces,
            "round-trip E4X traces must match ({profile:?})"
        );
    }
}

#[test]
fn t471_as3_e4x_esc_xelem_and_esc_xattr_escape_special_xml_chars() {
    let mut project = Project::blank();
    project.name = "T471 E4X Entity Escaping".into();
    let root_id = project.root.clone();
    project.symbol_mut(&root_id).unwrap().timeline.scripts = vec![FrameScript {
        layer: None,
        frame: 0,
        script: Script {
            language: ScriptLanguage::ActionScript3,
            source: r#"var rawAttr:String = "North & \"South\" <West>";
var rawElem:String = "R&D <Core> & >Edge";
var node:XML = <record title={rawAttr}>{rawElem}</record>;
trace("ESCAPED_ATTR:" + node.@title + "|ESCAPED_ELEM:" + node.toString());
"#
            .into(),
        },
    }];

    let (swf_bytes, _) = publish_swf_bytes(
        &project,
        &SwfPublishOptions {
            profile: SwfPublishProfile::FlashPlayer10Avm2,
            compression: SwfCompression::UncompressedFws,
            ..SwfPublishOptions::default()
        },
    )
    .expect("publish E4X escaping SWF");

    let playback = play_swf_bytes(&swf_bytes, 1).expect("execute E4X escaping SWF");
    assert_eq!(
        playback.trace_output,
        vec!["ESCAPED_ATTR:North & \"South\" <West>|ESCAPED_ELEM:R&D <Core> & >Edge".to_owned()]
    );
}

#[test]
fn t471_gradient_over_15_stops_resampled_in_nonstrict_and_blocked_in_strict() {
    // 20-stop gradient with distinct ratios in 0..=255 and a sharp interior magenta spike at stop 9 (ratio 120)
    // so Douglas-Peucker piecewise-linear RGBA error selection demonstrably preserves the high-error peak
    // as well as the exact first (ratio 0) and last (ratio 255) stops!
    let stops_20: Vec<GradientStop> = (0..20)
        .map(|i| {
            let ratio = ((i * 255) / 19) as u8;
            let color = if i == 0 {
                [12, 34, 56, 255]
            } else if i == 19 {
                [240, 210, 180, 128]
            } else if i == 9 {
                [255, 0, 250, 255] // High-error spike!
            } else {
                let t = (i as f64) / 19.0;
                [
                    (12.0 + (240.0 - 12.0) * t).round() as u8,
                    (34.0 + (210.0 - 34.0) * t).round() as u8,
                    (56.0 + (180.0 - 56.0) * t).round() as u8,
                    (255.0 + (128.0 - 255.0) * t).round() as u8,
                ]
            };
            GradientStop { ratio, color }
        })
        .collect();
    assert_eq!(stops_20.len(), 20);

    let stroke_stops_18: Vec<GradientStop> = (0..18)
        .map(|i| GradientStop {
            ratio: ((i * 255) / 17) as u8,
            color: [(i * 14) as u8, (255 - i * 12) as u8, 90, 255],
        })
        .collect();

    let rect_cmds = vec![
        PathCommand::MoveTo { x: 10.0, y: 10.0 },
        PathCommand::LineTo { x: 210.0, y: 10.0 },
        PathCommand::LineTo { x: 210.0, y: 150.0 },
        PathCommand::LineTo { x: 10.0, y: 150.0 },
        PathCommand::Close,
    ];

    let mut project = Project::blank();
    project.name = "T471 20-Stop Gradient Resampling".into();
    let root_id = project.root.clone();
    {
        let root_sym = project.symbol_mut(&root_id).expect("root symbol");
        let root_name = root_sym.name.clone();
        let _ = root_name;
        root_sym.timeline.frame_count = 1;
        root_sym.timeline.layers = vec![Layer {
            id: "grad_layer".into(),
            name: "Gradients".into(),
            visible: true,
            locked: false,
            properties: LayerProperties::default(),
            keyframes: vec![Keyframe {
                frame: 0,
                elements: vec![
                    Element {
                        id: "focal_grad_20".into(),
                        name: "focalGrad20".into(),
                        kind: ElementKind::PaintedPath {
                            commands: rect_cmds.clone(),
                            fill_rule: FillRule::EvenOdd,
                            fill: [0, 0, 0, 255],
                            stroke: [0, 0, 0, 255],
                            fill_gradient: Some(GradientPaint {
                                kind: GradientKind::Radial,
                                matrix: [0.15, 0.0, 0.0, 0.15, 110.0, 80.0],
                                stops: stops_20.clone(),
                                focal_point_ratio: -0.35,
                                spread: GradientSpread::Reflect,
                                linear_rgb: false,
                            }),
                            stroke_gradient: Some(GradientPaint {
                                kind: GradientKind::Linear,
                                matrix: [0.15, 0.0, 0.0, 0.15, 110.0, 80.0],
                                stops: stroke_stops_18,
                                focal_point_ratio: 0.0,
                                spread: GradientSpread::Extend,
                                linear_rgb: true,
                            }),
                            stroke_width: 4.0,
                        },
                        ..Element::default()
                    },
                    Element {
                        id: "linear_grad_20".into(),
                        name: "linearGrad20".into(),
                        kind: ElementKind::PaintedPath {
                            commands: rect_cmds,
                            fill_rule: FillRule::EvenOdd,
                            fill: [0, 0, 0, 255],
                            stroke: [0, 0, 0, 0],
                            fill_gradient: Some(GradientPaint {
                                kind: GradientKind::Linear,
                                matrix: [0.15, 0.0, 0.0, 0.15, 110.0, 80.0],
                                stops: stops_20.clone(),
                                focal_point_ratio: 0.0,
                                spread: GradientSpread::Repeat,
                                linear_rgb: false,
                            }),
                            stroke_gradient: None,
                            stroke_width: 0.0,
                        },
                        ..Element::default()
                    },
                ],
                ..Keyframe::default()
            }],
        }];
    }
    project.upgrade_format_for_features();
    let root_sym_name = project.symbol(&root_id).unwrap().name.clone();

    for profile in [
        SwfPublishProfile::FlashPlayer9Avm2,
        SwfPublishProfile::FlashPlayer10Avm2,
    ] {
        // 1. Strict mode MUST refuse >15 gradient stops with `swf_gradient_stop_count_limit`!
        let strict_opts = SwfExportOptions {
            publish: SwfPublishOptions {
                profile,
                compression: SwfCompression::UncompressedFws,
                ..SwfPublishOptions::default()
            },
            strict: true,
        };
        let strict_rep = preflight_swf_export(&project, &strict_opts);
        assert!(
            !strict_rep.can_proceed(),
            "strict preflight must reject >15 gradient stops ({profile:?})"
        );
        assert!(
            strict_rep
                .items
                .iter()
                .any(|i| i.code == "swf_gradient_stop_count_limit" && i.disposition.is_blocking()),
            "strict preflight must report swf_gradient_stop_count_limit"
        );
        let err = export_swf_bytes(&project, &strict_opts).expect_err("strict export must fail");
        assert!(matches!(err, ExportError::CompatibilityBlocked { .. }));

        // 2. Non-strict mode (`strict: false` and `publish_swf_bytes`) MUST succeed, resample to 15 monotonic stops, and emit located warnings!
        let non_strict_opts = SwfExportOptions {
            strict: false,
            ..strict_opts.clone()
        };
        let (swf_bytes, report) =
            export_swf_bytes(&project, &non_strict_opts).expect("non-strict 20-stop export");
        let (pub_bytes, pub_report) =
            publish_swf_bytes(&project, &strict_opts.publish).expect("publish_swf_bytes 20-stop");
        assert_eq!(swf_bytes, pub_bytes, "deterministic resampling output");
        assert_eq!(report.items.len(), pub_report.items.len());

        let approx_warnings: Vec<_> = report
            .items
            .iter()
            .filter(|i| i.code == "swf_gradient_stops_approximated")
            .collect();
        assert_eq!(
            approx_warnings.len(),
            3,
            "expected 3 located swf_gradient_stops_approximated warnings (2 fills + 1 stroke)"
        );
        let expected_loc_prefix =
            format!("symbol:{root_sym_name}:layer:Gradients:frame[0]:element:");
        for w in &approx_warnings {
            assert_eq!(w.disposition, FlaCompatibilityStatus::Approximated);
            assert!(
                w.location.starts_with(&expected_loc_prefix),
                "unexpected warning location: {}",
                w.location
            );
        }

        // 3. Binary-inspect `DefineShape3` (FP9) / `DefineShape4` (FP10) `GRADRECORD`s!
        let tags = collect_uncompressed_swf_tags(&swf_bytes);
        let expected_shape_tag = if profile == SwfPublishProfile::FlashPlayer10Avm2 {
            83
        } else {
            32
        };
        let shape_grads: Vec<ExtractedSwfGradient> = tags
            .iter()
            .filter(|(c, _)| *c == expected_shape_tag)
            .flat_map(|(c, payload)| extract_shape_gradients_from_tag(*c, payload))
            .collect();
        let expected_grad_count = if profile == SwfPublishProfile::FlashPlayer10Avm2 {
            3 // focal fill + stroke gradient + linear fill
        } else {
            2 // DefineShape3 line styles are solid RGBA; 2 fill gradients
        };
        assert_eq!(shape_grads.len(), expected_grad_count);

        for grad in &shape_grads {
            assert_eq!(
                grad.stops.len(),
                15,
                "every resampled gradient must emit exact 15 GRADRECORD stops ({profile:?})"
            );
            for pair in grad.stops.windows(2) {
                assert!(
                    pair[0].0 < pair[1].0,
                    "GRADRECORD ratios must be strictly monotonic and unique: {:?}",
                    grad.stops
                );
            }
        }

        // First shape's fill gradient is our 20-stop focal radial gradient!
        let focal = &shape_grads[0];
        assert_eq!(focal.fill_type, 0x13);
        assert_eq!(focal.spread_bits, 1); // Reflect
        assert_eq!(focal.interp_bits, 0);
        assert!(focal.focal_fixed8.unwrap() < 0);
        assert_eq!(focal.stops.first().unwrap(), &(0, [12, 34, 56, 255]));
        assert_eq!(focal.stops.last().unwrap(), &(255, [240, 210, 180, 128]));
        assert!(
            focal
                .stops
                .contains(&(stops_20[9].ratio, [255, 0, 250, 255])),
            "Douglas-Peucker error selection must retain high-error interior spike stop: {:?}",
            focal.stops
        );

        // 4. Re-import via `import_swf_bytes` and validate!
        let reimported =
            import_swf_bytes(&swf_bytes).expect("re-import resampled 15-stop gradient SWF");
        reimported
            .validate()
            .expect("re-imported resampled gradient project must pass Project::validate()");
        let imported_grad_lens: Vec<usize> = reimported
            .symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .flat_map(|l| &l.keyframes)
            .flat_map(|kf| &kf.elements)
            .filter_map(|el| match &el.kind {
                ElementKind::PaintedPath { fill_gradient, .. } => {
                    fill_gradient.as_ref().map(|g| g.stops.len())
                }
                _ => None,
            })
            .collect();
        assert!(
            imported_grad_lens.contains(&15),
            "re-imported shapes must carry the 15 resampled gradient stops: {imported_grad_lens:?}"
        );
    }
}

#[test]
fn t471_combined_oop_e4x_class_and_duplicate_ratio_20_stop_export_swf_file_roundtrip() {
    let mut project = Project::blank();
    project.name = "T471 Combined OOP + E4X + 20-Stop Duplicate-Ratio Gradient".into();
    project.document_class = Some("svc.CatalogMain".into());

    // 20 stops where many share duplicate ratios (only 6 unique input ratios across 0..=255),
    // verifying `resample_swf_gradient_stops` subdivides intervals to emit 15 strictly monotonic unique stops!
    let mut dup_stops_20 = Vec::with_capacity(20);
    for i in 0..20 {
        let ratio = match i {
            0 => 0,
            1..=5 => 30,
            6..=10 => 90,
            11..=14 => 150,
            15..=18 => 210,
            _ => 255,
        };
        dup_stops_20.push(GradientStop {
            ratio,
            color: [(i * 12) as u8, (240 - i * 10) as u8, 120, 255],
        });
    }
    assert_eq!(dup_stops_20.len(), 20);

    let root_id = project.root.clone();
    project.symbol_mut(&root_id).unwrap().timeline.layers = vec![Layer {
        id: "bg_grad".into(),
        name: "BgGrad".into(),
        visible: true,
        locked: false,
        properties: LayerProperties::default(),
        keyframes: vec![Keyframe {
            frame: 0,
            elements: vec![Element {
                id: "dup_grad_rect".into(),
                name: "dupGrad".into(),
                kind: ElementKind::PaintedPath {
                    commands: vec![
                        PathCommand::MoveTo { x: 0.0, y: 0.0 },
                        PathCommand::LineTo { x: 120.0, y: 0.0 },
                        PathCommand::LineTo { x: 120.0, y: 80.0 },
                        PathCommand::LineTo { x: 0.0, y: 80.0 },
                        PathCommand::Close,
                    ],
                    fill_rule: FillRule::EvenOdd,
                    fill: [0, 0, 0, 255],
                    stroke: [0, 0, 0, 0],
                    fill_gradient: Some(GradientPaint {
                        kind: GradientKind::Linear,
                        matrix: [0.1, 0.0, 0.0, 0.1, 60.0, 40.0],
                        stops: dup_stops_20.clone(),
                        focal_point_ratio: 0.0,
                        spread: GradientSpread::Extend,
                        linear_rgb: false,
                    }),
                    stroke_gradient: None,
                    stroke_width: 0.0,
                },
                ..Element::default()
            }],
            ..Keyframe::default()
        }],
    }];
    project.upgrade_format_for_features();

    project.sources = vec![
        SourceUnit {
            id: "s_iface".into(),
            path: "svc/IXmlFeed.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package svc {
    public interface IXmlFeed {
        function summarize(code:int):String;
    }
}
"#
            .into(),
        },
        SourceUnit {
            id: "s_base".into(),
            path: "svc/BaseFeed.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package svc {
    public class BaseFeed {
        public var prefix:String;
        public function BaseFeed(p:String) {
            this.prefix = p;
        }
        public function buildXml(code:int):XML {
            return <feed env={this.prefix}><entry id={code}>Ready</entry><entry id={code + 1}>Go</entry></feed>;
        }
    }
}
"#
            .into(),
        },
        SourceUnit {
            id: "s_sub".into(),
            path: "svc/LiveFeed.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package svc {
    public class LiveFeed extends BaseFeed implements IXmlFeed {
        public function LiveFeed(p:String) {
            super(p);
        }
        override public function buildXml(code:int):XML {
            var xml:XML = super.buildXml(code);
            xml.entry[0].@id = String(code + 100);
            return xml;
        }
        public function summarize(code:int):String {
            var doc:XML = this.buildXml(code);
            var ids:XMLList = doc..@id;
            return doc.@env + ":" + ids[0] + "," + ids[1];
        }
    }
}
"#
            .into(),
        },
        SourceUnit {
            id: "s_doc".into(),
            path: "svc/CatalogMain.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package svc {
    import flash.display.MovieClip;
    public class CatalogMain extends MovieClip {
        public function CatalogMain() {
            super();
            var feed:IXmlFeed = new LiveFeed("prod") as IXmlFeed;
            trace("FEED_OK=" + (feed is IXmlFeed) + "|" + feed.summarize(7));
        }
    }
}
"#
            .into(),
        },
    ];

    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let out_swf = tmp_dir.path().join("combined_t471.swf");
    let opts = SwfExportOptions {
        publish: SwfPublishOptions {
            profile: SwfPublishProfile::FlashPlayer10Avm2,
            compression: SwfCompression::UncompressedFws,
            ..SwfPublishOptions::default()
        },
        strict: false,
    };
    let report =
        export_swf(&project, &out_swf, &opts).expect("export_swf with verify_swf_publication");
    assert!(
        report
            .items
            .iter()
            .any(|i| i.code == "swf_gradient_stops_approximated")
    );

    let swf_bytes = std::fs::read(&out_swf).unwrap();
    let tags = collect_uncompressed_swf_tags(&swf_bytes);
    let grads: Vec<ExtractedSwfGradient> = tags
        .iter()
        .filter(|(c, _)| *c == 83)
        .flat_map(|(c, p)| extract_shape_gradients_from_tag(*c, p))
        .collect();
    assert_eq!(grads.len(), 1);
    assert_eq!(grads[0].stops.len(), 15);
    assert_eq!(grads[0].stops.first().unwrap().0, 0);
    assert_eq!(grads[0].stops.last().unwrap().0, 255);
    for w in grads[0].stops.windows(2) {
        assert!(w[0].0 < w[1].0);
    }

    let play = play_swf_bytes(&swf_bytes, 1).expect("play combined SWF");
    assert_eq!(
        play.trace_output,
        vec!["FEED_OK=true|prod:107,8".to_owned()]
    );
}

#[test]
fn t471_deeply_nested_500_expr_stmt_and_e4x_interpolations_fail_cleanly_without_stack_overflow() {
    let depth = 500usize;

    // 1) 500-deep parenthesized expression `((((...1...))))`
    let deep_parens = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
    // 2) 500-deep unary expression `!!!!...true`
    let deep_unary = format!("{}true", "!".repeat(depth));
    // 3) 500-deep nested block statement `{{{...var x:int = 1;...}}}`
    let deep_blocks = format!("{}var x:int = 1;{}", "{".repeat(depth), "}".repeat(depth));
    // 4) 500-deep E4X attribute interpolation `<a x={<a x={...}/>}/>`
    let mut deep_e4x_attr = "\"leaf\"".to_owned();
    for _ in 0..depth {
        deep_e4x_attr = format!("<a x={{{deep_e4x_attr}}}/>");
    }
    // 5) 500-deep E4X element content interpolation `<a>{<a>{...}</a>}</a>`
    let mut deep_e4x_elem = "\"leaf\"".to_owned();
    for _ in 0..depth {
        deep_e4x_elem = format!("<a>{{{deep_e4x_elem}}}</a>");
    }

    let publish = SwfPublishOptions {
        profile: SwfPublishProfile::FlashPlayer10Avm2,
        compression: SwfCompression::UncompressedFws,
        ..SwfPublishOptions::default()
    };

    for (label, body_stmt) in [
        ("deep_parens", format!("var v:* = {deep_parens};")),
        ("deep_unary", format!("var v:* = {deep_unary};")),
        ("deep_blocks", deep_blocks),
        ("deep_e4x_attr", format!("var v:XML = {deep_e4x_attr};")),
        ("deep_e4x_elem", format!("var v:XML = {deep_e4x_elem};")),
    ] {
        let mut p = Project::blank();
        p.document_class = Some("DeepProbe".into());
        p.sources = vec![SourceUnit {
            id: format!("src_{label}"),
            path: "DeepProbe.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: format!(
                "package {{ import flash.display.MovieClip; public class DeepProbe extends MovieClip {{ public function DeepProbe() {{ super(); {body_stmt} }} }} }}"
            ),
        }];

        let preflight = preflight_swf_publish(&p, &publish);
        assert!(
            preflight.has_blocking(),
            "expected clean blocking preflight diagnostic for 500-deep '{label}', got: {:?}",
            preflight.items
        );
        assert!(
            preflight
                .items
                .iter()
                .any(|i| i.message.contains("nesting depth")),
            "expected nesting depth diagnostic for '{label}', got: {:?}",
            preflight.items
        );

        let err = publish_swf_bytes(&p, &publish)
            .expect_err("500-deep script must return clean Err without stack overflow");
        assert!(
            err.to_string().contains("nesting depth") || err.to_string().contains("Blocking"),
            "unexpected error message for '{label}': {err}"
        );
    }
}

#[test]
fn t471_as3_private_methods_synthetic_init_freedom_and_override_arity_validation() {
    let publish = SwfPublishOptions {
        profile: SwfPublishProfile::FlashPlayer10Avm2,
        compression: SwfCompression::UncompressedFws,
        ..SwfPublishOptions::default()
    };

    // Part A: Positive end-to-end compile, ABC traits check, execution & round-trip:
    // - `Base` has explicit ctor, 0 public methods except `compute(x:int)`, and `private function secret():int`
    // - `Child extends Base implements IInitable, ICalculable`:
    //   - defines its OWN `private function secret():int` WITHOUT `override` (per-class 0x05 PrivateNs, NO TRAIT_Override!)
    //   - defines `public function init(seed:int = 4):int` (no collision with any synthetic "init"!)
    //   - overrides `compute(x:int, bonus:int = 5):int` (optional default satisfies 1-arg `Base.compute` & `ICalculable.compute`!)
    //   - implements `variadic(tag:String, ...rest):String` (`NEED_REST = 0x04` satisfies 2-arg interface method!)
    let mut project = Project::blank();
    project.document_class = Some("MainF05".into());
    project.sources = vec![
        SourceUnit {
            id: "s_ifaces".into(),
            path: "IInitable.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package {
    public interface IInitable {
        function init(seed:int):int;
    }
}"#
            .into(),
        },
        SourceUnit {
            id: "s_icalc".into(),
            path: "ICalculable.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package {
    public interface ICalculable {
        function compute(x:int):int;
        function acceptPair(a:int, b:int):int;
    }
}"#
            .into(),
        },
        SourceUnit {
            id: "s_base".into(),
            path: "BaseBox.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package {
    public class BaseBox {
        public var baseVal:int = 0;
        public function BaseBox(v:int = 10) {
            this.baseVal = v;
        }
        private function secret():int {
            return 100;
        }
        public function readBaseSecret():int {
            return this.secret() + this.baseVal;
        }
        public function compute(x:int):int {
            return x + this.baseVal;
        }
    }
}"#
            .into(),
        },
        SourceUnit {
            id: "s_child".into(),
            path: "ChildBox.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package {
    public class ChildBox extends BaseBox implements IInitable, ICalculable {
        public function ChildBox(v:int = 20) {
            super(v);
        }
        private function secret():int {
            return 250;
        }
        public function init(seed:int = 4):int {
            return this.secret() + seed;
        }
        override public function compute(x:int, bonus:int = 5):int {
            return super.compute(x) + bonus;
        }
        public function acceptPair(...rest):int {
            return 42;
        }
    }
}"#
            .into(),
        },
        SourceUnit {
            id: "s_main".into(),
            path: "MainF05.as".into(),
            language: ScriptLanguage::ActionScript3,
            source: r#"package {
    import flash.display.MovieClip;
    public class MainF05 extends MovieClip {
        public function MainF05() {
            super();
            var b:BaseBox = new BaseBox();
            var c:ChildBox = new ChildBox();
            var asInit:IInitable = c as IInitable;
            var asCalc:ICalculable = c as ICalculable;
            trace("BASE:" + b.readBaseSecret() + "|COMPUTE1:" + c.compute(7) + "|COMPUTE2:" + c.compute(7, 9));
            trace("INIT_DEF:" + c.init() + "|INIT_IFACE:" + asInit.init(6) + "|PAIR:" + asCalc.acceptPair(1, 2));
        }
    }
}"#
            .into(),
        },
    ];

    let report = preflight_swf_publish(&project, &publish);
    assert!(
        !report.has_blocking(),
        "valid private methods + init() + optional/rest overrides must pass preflight: {:?}",
        report.items
    );

    let (swf_bytes, _) = publish_swf_bytes(&project, &publish).expect("publish MainF05 SWF");
    let abc = parse_abc_summary(&extract_do_abc2_bytes(&swf_bytes));

    // Verify `BaseBox` and `ChildBox` private `secret` traits use 0x05 `CONSTANT_PrivateNs` and NEVER set `TRAIT_Override` (0x20),
    // and `ChildBox.init` does NOT set `TRAIT_Override` (no synthetic "init" on `BaseBox`)!
    let base_inst = abc
        .instances
        .iter()
        .find(|i| abc.mn_short_name(i.name_mn) == "BaseBox")
        .expect("BaseBox instance");
    assert!(
        !base_inst
            .traits
            .iter()
            .any(|tr| abc.mn_short_name(tr.name_mn) == "init"),
        "BaseBox with explicit ctor must NOT have a synthetic 'init' trait: {:?}",
        base_inst.traits
    );
    let child_inst = abc
        .instances
        .iter()
        .find(|i| abc.mn_short_name(i.name_mn) == "ChildBox")
        .expect("ChildBox instance");
    for tr in &child_inst.traits {
        let local_name = abc.mn_short_name(tr.name_mn);
        let (ns_kind, _) = abc.mn_qname_ns(tr.name_mn);
        if local_name == "secret" {
            assert_eq!(
                ns_kind, 0x05,
                "private method trait must use 0x05 CONSTANT_PrivateNs"
            );
            assert_eq!(
                tr.kind_byte & 0x20,
                0,
                "private method must NEVER emit TRAIT_Override (0x20)"
            );
        } else if local_name == "init" {
            assert_eq!(
                tr.kind_byte & 0x20,
                0,
                "ChildBox.init must NOT emit TRAIT_Override when BaseBox has no init()"
            );
        } else if local_name == "compute" {
            assert_ne!(
                tr.kind_byte & 0x20,
                0,
                "ChildBox.compute override must emit TRAIT_Override (0x20)"
            );
        }
    }
    // Verify HAS_OPTIONAL (0x08) and NEED_REST (0x04) flags are emitted in MethodInfo
    assert!(
        abc.method_flags.iter().any(|&f| (f & 0x08) != 0),
        "expected HAS_OPTIONAL (0x08) in ABC MethodInfo flags"
    );
    assert!(
        abc.method_flags.iter().any(|&f| (f & 0x04) != 0),
        "expected NEED_REST (0x04) in ABC MethodInfo flags"
    );

    let play = play_swf_bytes(&swf_bytes, 1).expect("play MainF05 SWF");
    assert_eq!(
        play.trace_output,
        vec![
            "BASE:110|COMPUTE1:32|COMPUTE2:36".to_owned(),
            "INIT_DEF:254|INIT_IFACE:256|PAIR:42".to_owned(),
        ]
    );

    // Re-import & re-export round-trip must preserve HAS_OPTIONAL defaults & arity compatibility!
    let reimported = import_swf_bytes(&swf_bytes).expect("re-import MainF05 SWF");
    reimported.validate().expect("reimported validates");
    let (reexported_swf, _) =
        publish_swf_bytes(&reimported, &publish).expect("re-export decompiled MainF05 SWF");
    let replay = play_swf_bytes(&reexported_swf, 1).expect("play re-exported MainF05 SWF");
    assert_eq!(replay.trace_output, play.trace_output);

    // Part B: Negative strict preflight validations for [F-05]:
    let check_blocking_code = |sources: Vec<(&str, &str)>, expected_code: &str| {
        let mut p = Project::blank();
        p.document_class = Some("Doc".into());
        p.sources = sources
            .into_iter()
            .enumerate()
            .map(|(i, (path, src))| SourceUnit {
                id: format!("u_{i}"),
                path: path.into(),
                language: ScriptLanguage::ActionScript3,
                source: src.into(),
            })
            .collect();
        let rep = preflight_swf_publish(&p, &publish);
        assert!(
            rep.items.iter().any(|it| it.code == expected_code),
            "expected blocking diagnostic '{expected_code}', got: {:?}",
            rep.items
        );
    };

    // 1) Explicit-ctor class implementing interface requiring `init()` without defining `init()` -> unimplemented!
    check_blocking_code(
        vec![
            (
                "IInit.as",
                "package { public interface IInit { function init():void; } }",
            ),
            (
                "Doc.as",
                "package { import flash.display.MovieClip; public class Doc extends MovieClip implements IInit { public function Doc() { super(); } } }",
            ),
        ],
        "swf_as3_unimplemented_interface_method",
    );

    // 2) `private` method does NOT satisfy a public interface requirement!
    check_blocking_code(
        vec![
            (
                "IFoo.as",
                "package { public interface IFoo { function compute(x:int):int; } }",
            ),
            (
                "Doc.as",
                "package { import flash.display.MovieClip; public class Doc extends MovieClip implements IFoo { private function compute(x:int):int { return x; } } }",
            ),
        ],
        "swf_as3_unimplemented_interface_method",
    );

    // 3) `override private` is rejected as invalid AS3 syntax!
    check_blocking_code(
        vec![
            (
                "Base.as",
                "package { public class Base { private function f():void {} } }",
            ),
            (
                "Doc.as",
                "package { public class Doc extends Base { override private function f():void {} } }",
            ),
        ],
        "swf_as3_source_unsupported_syntax",
    );

    // 4) Orphan `override` on a class whose user superclass chain has no such method!
    check_blocking_code(
        vec![(
            "Doc.as",
            "package { public class Doc { override public function nonExistent():void {} } }",
        )],
        "swf_as3_invalid_override",
    );

    // 5) Missing `override` when shadowing a non-private ancestor method!
    check_blocking_code(
        vec![
            (
                "Base.as",
                "package { public class Base { public function step(x:int):int { return x; } } }",
            ),
            (
                "Doc.as",
                "package { public class Doc extends Base { public function step(x:int):int { return x + 1; } } }",
            ),
        ],
        "swf_as3_missing_override",
    );

    // 6) Incompatible override arity (requires 2 args when base accepts 1 arg)!
    check_blocking_code(
        vec![
            (
                "Base.as",
                "package { public class Base { public function step(x:int):int { return x; } } }",
            ),
            (
                "Doc.as",
                "package { public class Doc extends Base { override public function step(x:int, y:int):int { return x + y; } } }",
            ),
        ],
        "swf_as3_incompatible_method_arity",
    );

    // 7) Incompatible interface implementation arity (accepts 0 args when interface requires 1 arg)!
    check_blocking_code(
        vec![
            (
                "IFoo.as",
                "package { public interface IFoo { function compute(x:int):int; } }",
            ),
            (
                "Doc.as",
                "package { import flash.display.MovieClip; public class Doc extends MovieClip implements IFoo { public function compute():int { return 0; } } }",
            ),
        ],
        "swf_as3_incompatible_method_arity",
    );
}
