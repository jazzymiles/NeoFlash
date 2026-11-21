//! Unit tests for `swf_loader` timeline OOM budgets, JS literal escaping, sandbox-hardened codegen, and actionable warnings.

use super::vm::js_string_literal;
use super::{MAX_SWF_KEYFRAMES, decode_swf_bundle};
use std::time::Instant;

fn make_tag(code: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + payload.len());
    if payload.len() < 0x3F {
        let hdr = (code << 6) | (payload.len() as u16);
        out.extend_from_slice(&hdr.to_le_bytes());
    } else {
        let hdr = (code << 6) | 0x3F;
        out.extend_from_slice(&hdr.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    }
    out.extend_from_slice(payload);
    out
}

fn wrap_fws(version: u8, frame_count: u16, tags: &[Vec<u8>]) -> Vec<u8> {
    // RECT(0, 2000, 0, 2000) with nbits = 13:
    // 5 + 4*13 = 57 bits -> 8 bytes; simpler: nbits = 0 -> 1 byte `0x00` (clamped to 1x1 px in decoder)
    let mut body = vec![0x00];
    body.extend_from_slice(&24u16.wrapping_shl(8).to_le_bytes());
    body.extend_from_slice(&frame_count.to_le_bytes());
    for t in tags {
        body.extend_from_slice(t);
    }
    body.extend_from_slice(&[0x00, 0x00]); // End tag
    let total_len = (8 + body.len()) as u32;
    let mut swf = Vec::with_capacity(total_len as usize);
    swf.extend_from_slice(b"FWS");
    swf.push(version);
    swf.extend_from_slice(&total_len.to_le_bytes());
    swf.extend_from_slice(&body);
    swf
}

fn minimal_define_shape_tag(char_id: u16) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&char_id.to_le_bytes());
    p.push(0x00); // empty RECT (nbits = 0)
    p.push(0x00); // 0 fill styles
    p.push(0x00); // 0 line styles
    p.push(0x00); // numFillBits = 0, numLineBits = 0
    p.push(0x00); // EndShapeRecord
    make_tag(2, &p)
}

fn minimal_place_object2_tag(depth: u16, char_id: u16) -> Vec<u8> {
    let mut p = Vec::with_capacity(6);
    p.push(0x02); // HasCharacter only (no matrix)
    p.extend_from_slice(&depth.to_le_bytes());
    p.extend_from_slice(&char_id.to_le_bytes());
    make_tag(26, &p)
}

#[test]
fn swf_loader_bounds_keyframes_without_oom_on_thousands_of_placements_and_4096_frames() {
    let mut tags = Vec::with_capacity(8_010);
    tags.push(minimal_define_shape_tag(1));
    for depth in 1..=8_000u16 {
        tags.push(minimal_place_object2_tag(depth, 1));
    }
    tags.push(make_tag(1, &[])); // 1 ShowFrame, header declares 4096 frames!
    // Add 4095 empty ShowFrame tags as well to verify unchanged frames never clone BTreeMap
    for _ in 1..4096 {
        tags.push(make_tag(1, &[]));
    }

    let swf = wrap_fws(8, 4096, &tags);
    let started = Instant::now();
    let bundle = decode_swf_bundle(&swf).expect("decode 8000-depth x 4096-frame SWF cleanly");
    let elapsed = started.elapsed();

    assert!(
        elapsed.as_secs_f64() < 2.0,
        "expected fast O(changes) timeline decode, took {elapsed:?}"
    );
    assert_eq!(bundle.swf_info.frame_count, 4096);
    let root_sym = bundle
        .project
        .symbols
        .iter()
        .find(|s| s.id == bundle.project.root)
        .expect("root symbol");
    assert_eq!(root_sym.timeline.frame_count, 4096);
    assert_eq!(root_sym.timeline.layers.len(), 8_000);
    let total_kfs: usize = bundle
        .project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .map(|l| l.keyframes.len())
        .sum();
    assert!(
        total_kfs <= MAX_SWF_KEYFRAMES + 16,
        "expected bounded keyframes <= {MAX_SWF_KEYFRAMES}, got {total_kfs}"
    );
    // Each unchanged depth must hold a single keyframe at frame 0 across all 4096 frames
    assert!(
        root_sym
            .timeline
            .layers
            .iter()
            .all(|l| l.keyframes.len() == 1 && l.keyframes[0].frame == 0)
    );
}

#[test]
fn js_string_literal_escapes_nul_digits_and_line_separators_for_strict_mode() {
    let s1 = js_string_literal("\x001");
    assert_eq!(s1, "\"\\u00001\"");
    assert!(!s1.contains("\\0"));

    let s8 = js_string_literal("\x008");
    assert_eq!(s8, "\"\\u00008\"");

    let sep = js_string_literal("a\u{2028}b\u{2029}c");
    assert_eq!(sep, "\"a\\u2028b\\u2029c\"");
}

#[test]
fn swf_loader_emits_deduplicated_warnings_and_never_emits_unguarded_js_member_access() {
    // DoAction with GetMember (0x4E), GetURL (0x83), and duplicate unsupported 0x7F opcodes
    let mut do_action = Vec::new();
    // Push "obj", "prop" -> GetMember
    do_action.extend_from_slice(&[0x96, 10, 0, 0, b'o', b'b', b'j', 0, 0, b'p', b'r', b'o', 0]);
    do_action.push(0x4E);
    // GetURL("https://example.com", "_blank") -> 0x83 (now supported via __nfSwfGetURL!)
    do_action.extend_from_slice(&[0x83, 4, 0, b'a', 0, b'b', 0]);
    // Duplicate unsupported opcode 0x7F to verify warning deduplication
    do_action.push(0x7F);
    do_action.push(0x7F);
    do_action.push(0x00);

    // PlaceObject3 with unsupported blendMode = 99
    let place3 = make_tag(70, &[0x02, 0x02, 1, 0, 1, 0, 99]);
    let sym_class = make_tag(76, &[1, 0, 1, 0, b'C', b'h', b'i', b'l', b'd', 0]);

    let swf = wrap_fws(
        9,
        1,
        &[
            minimal_define_shape_tag(1),
            place3,
            make_tag(12, &do_action),
            sym_class,
            make_tag(1, &[]),
        ],
    );
    let bundle = decode_swf_bundle(&swf).expect("decode SWF with unsupported opcodes/features");
    assert!(
        !bundle.warnings.iter().any(|w| w.contains("GetURL")),
        "GetURL (0x83) is supported and must not warn: {:?}",
        bundle.warnings
    );
    assert!(
        bundle
            .warnings
            .iter()
            .any(|w| w.contains("bytecode offset") && w.contains("root frame 1 DoAction"))
    );
    assert_eq!(
        bundle
            .warnings
            .iter()
            .filter(|w| w.contains("0x7F"))
            .count(),
        1,
        "unsupported opcode warnings must be deduplicated: {:?}",
        bundle.warnings
    );
    assert!(
        bundle
            .warnings
            .iter()
            .any(|w| w.contains("PlaceObject3 blendMode 99")),
        "expected invalid blendMode warning, got {:?}",
        bundle.warnings
    );
    assert!(
        bundle
            .warnings
            .iter()
            .any(|w| w.contains("SymbolClass binding 'Child'")),
        "unbound SymbolClass on shape must warn, got {:?}",
        bundle.warnings
    );

    for unit in &bundle.scripts {
        assert!(!unit.code.contains("__o[__m]"));
        assert!(!unit.code.contains(".apply("));
        assert!(!unit.code.contains("function(){}"));
        assert!(unit.code.contains("__nfSwfGet"));
        assert!(unit.code.contains("__nfSwfGetURL"));
    }
}

fn push_u30(out: &mut Vec<u8>, mut val: u32) {
    loop {
        let mut b = (val & 0x7F) as u8;
        val >>= 7;
        if val != 0 {
            b |= 0x80;
        }
        out.push(b);
        if val == 0 {
            break;
        }
    }
}

#[test]
fn swf_loader_clamps_avm1_and_avm2_inner_opcode_arity_and_uses_null_proto_locals() {
    // 1. AVM1 DoAction with InitArray (0x42), InitObject (0x43), CallFunction (0x3D), CallMethod (0x52)
    let mut avm1 = Vec::new();
    avm1.extend_from_slice(&[0x96, 5, 0, 7, 0xFF, 0xFF, 0xFF, 0x7F]); // Push i32::MAX
    avm1.push(0x42); // InitArray
    avm1.push(0x43); // InitObject
    avm1.push(0x3D); // CallFunction
    avm1.push(0x52); // CallMethod
    avm1.push(0x00);

    // 2. AVM2 DoABC2 with 0x56 newarray(0x3FFF_FFFF), 0x55 newobject(0x3FFF_FFFF),
    //    0x41 call(0x3FFF_FFFF), 0x4F callpropvoid(1, 0x3FFF_FFFF), 0x63 setlocal(0x3FFF_FFFF)
    let mut abc = vec![16, 0, 46, 0];
    push_u30(&mut abc, 1); // ints
    push_u30(&mut abc, 1); // uints
    push_u30(&mut abc, 1); // doubles
    push_u30(&mut abc, 4); // strings: [0], "", "Doc", "Object"
    abc.push(0); // str 1: ""
    abc.extend_from_slice(&[3, b'D', b'o', b'c']); // str 2
    abc.extend_from_slice(&[6, b'O', b'b', b'j', b'e', b'c', b't']); // str 3
    push_u30(&mut abc, 2);
    abc.extend_from_slice(&[0x16, 1]); // ns 1
    push_u30(&mut abc, 1); // ns_sets
    push_u30(&mut abc, 3);
    abc.extend_from_slice(&[0x07, 1, 2, 0x07, 1, 3]); // mn 1=Doc, 2=Object
    push_u30(&mut abc, 3);
    for _ in 0..3 {
        abc.extend_from_slice(&[0, 0, 0, 0]);
    }
    push_u30(&mut abc, 0); // metadata
    push_u30(&mut abc, 1); // 1 class
    abc.extend_from_slice(&[1, 2, 0, 0, 1, 0, 0, 0]); // instance + class
    push_u30(&mut abc, 1);
    abc.extend_from_slice(&[2, 0]); // script

    let mut code = Vec::new();
    code.push(0x56);
    push_u30(&mut code, 0x3FFF_FFFF); // newarray(1B)
    code.push(0x55);
    push_u30(&mut code, 0x3FFF_FFFF); // newobject(1B)
    code.push(0x41);
    push_u30(&mut code, 0x3FFF_FFFF); // call(1B)
    code.push(0x4F);
    push_u30(&mut code, 1);
    push_u30(&mut code, 0x3FFF_FFFF); // callpropvoid(1, 1B)
    code.push(0x49);
    push_u30(&mut code, 0x3FFF_FFFF); // constructsuper(1B)
    code.push(0x63);
    push_u30(&mut code, 0x3FFF_FFFF); // setlocal(1B)
    code.push(0x47); // returnvoid

    push_u30(&mut abc, 1); // 1 body (method 1 = iinit)
    push_u30(&mut abc, 1);
    abc.extend_from_slice(&[8, 4, 0, 4]);
    push_u30(&mut abc, code.len() as u32);
    abc.extend_from_slice(&code);
    abc.extend_from_slice(&[0, 0]);

    let mut do_abc2 = vec![1, 0, 0, 0, b't', 0];
    do_abc2.extend_from_slice(&abc);
    let sym_doc = make_tag(76, &[1, 0, 0, 0, b'D', b'o', b'c', 0]);

    let swf_avm1 = wrap_fws(8, 1, &[make_tag(12, &avm1), make_tag(1, &[])]);
    let b1 = decode_swf_bundle(&swf_avm1).expect("decode AVM1 arity clamp SWF");
    let c1 = &b1.scripts[0].code;
    assert_eq!(c1.matches("Math.min(4096,").count(), 4, "{c1}");

    let swf_avm2 = wrap_fws(
        13,
        1,
        &[
            make_tag(69, &[0x08, 0, 0, 0]),
            make_tag(82, &do_abc2),
            sym_doc,
            make_tag(1, &[]),
        ],
    );
    let b2 = decode_swf_bundle(&swf_avm2).expect("decode AVM2 arity clamp SWF");
    let c2 = &b2.scripts[0].code;
    assert!(
        !c2.contains("1073741823") || c2.contains("__loc[1073741823]"),
        "{c2}"
    );
    assert!(!c2.contains("new Array(1073741823)"), "{c2}");
    assert!(c2.contains("new Array(4096)"), "{c2}");
    assert!(
        c2.contains("__loc = Object.create(null); __loc[0] = this;"),
        "{c2}"
    );
}

#[test]
fn swf_loader_decodes_avm1_define_function2_preload_flags_with_try_and_geturl() {
    let push_str = |s: &str| -> Vec<u8> {
        let len = (1 + s.len() + 1) as u16;
        let mut out = vec![0x96];
        out.extend_from_slice(&len.to_le_bytes());
        out.push(0);
        out.extend_from_slice(s.as_bytes());
        out.push(0);
        out
    };

    let mut actions = Vec::new();
    // 1. Named ActionDefineFunction (0x9B): function legacyAdd(a, b) { return a + b; }
    let mut fn1_body = Vec::new();
    fn1_body.extend_from_slice(&push_str("a"));
    fn1_body.push(0x1C); // GetVariable("a")
    fn1_body.extend_from_slice(&push_str("b"));
    fn1_body.push(0x1C); // GetVariable("b")
    fn1_body.push(0x47); // ActionAdd2
    fn1_body.push(0x3E); // ActionReturn
    let mut fn1_hdr = Vec::new();
    fn1_hdr.extend_from_slice(b"legacyAdd\0");
    fn1_hdr.extend_from_slice(&2u16.to_le_bytes());
    fn1_hdr.extend_from_slice(b"a\0b\0");
    fn1_hdr.extend_from_slice(&(fn1_body.len() as u16).to_le_bytes());
    actions.push(0x9B);
    actions.extend_from_slice(&(fn1_hdr.len() as u16).to_le_bytes());
    actions.extend_from_slice(&fn1_hdr);
    actions.extend_from_slice(&fn1_body);

    // 2. Anonymous ActionDefineFunction2 (0x8E) with preload_this(0x01) | preload_arguments(0x04)
    //    | preload_super(0x10) | preload_root(0x40) | preload_parent(0x80) | preload_global(0x100) = 0x01D5
    let fn2_body = vec![0x96, 2, 0, 4, 7, 0x3E]; // Push reg[7], Return
    let mut fn2_hdr = Vec::new();
    fn2_hdr.push(0); // anonymous ""
    fn2_hdr.extend_from_slice(&1u16.to_le_bytes()); // 1 param
    fn2_hdr.push(10); // register_count = 10
    fn2_hdr.extend_from_slice(&0x01D5u16.to_le_bytes()); // all 6 preload flags
    fn2_hdr.push(7); // param in reg 7
    fn2_hdr.extend_from_slice(b"x\0");
    fn2_hdr.extend_from_slice(&(fn2_body.len() as u16).to_le_bytes());
    actions.push(0x8E);
    actions.extend_from_slice(&(fn2_hdr.len() as u16).to_le_bytes());
    actions.extend_from_slice(&fn2_hdr);
    actions.extend_from_slice(&fn2_body);
    actions.push(0x17); // Pop

    // 3. ActionWith (0x94) + ActionTry (0x8F catch in register 2 + finally) + ActionGetURL2 (0x9A)
    actions.extend_from_slice(&[0x96, 5, 0, 7, 0, 0, 0, 0, 0x43]); // {}
    let mut with_inner = push_str("k");
    with_inner.extend_from_slice(&[0x1C, 0x17]);
    actions.push(0x94);
    actions.extend_from_slice(&2u16.to_le_bytes());
    actions.extend_from_slice(&(with_inner.len() as u16).to_le_bytes());
    actions.extend_from_slice(&with_inner);

    let mut try_b = push_str("e!");
    try_b.push(0x2A); // Throw "e!"
    let catch_b = vec![0x96, 2, 0, 4, 2, 0x17]; // Push reg[2], Pop
    let mut fin_b = push_str("f");
    fin_b.push(0x17);
    let mut try_hdr = vec![0x07]; // has_catch | has_finally | catch_in_register
    try_hdr.extend_from_slice(&(try_b.len() as u16).to_le_bytes());
    try_hdr.extend_from_slice(&(catch_b.len() as u16).to_le_bytes());
    try_hdr.extend_from_slice(&(fin_b.len() as u16).to_le_bytes());
    try_hdr.push(2); // catch register 2
    actions.push(0x8F);
    actions.extend_from_slice(&(try_hdr.len() as u16).to_le_bytes());
    actions.extend_from_slice(&try_hdr);
    actions.extend_from_slice(&try_b);
    actions.extend_from_slice(&catch_b);
    actions.extend_from_slice(&fin_b);

    // Push "FSCommand:ping", "ok" -> GetURL2(0)
    actions.extend_from_slice(&push_str("FSCommand:ping"));
    actions.extend_from_slice(&push_str("ok"));
    actions.extend_from_slice(&[0x9A, 1, 0, 0]);
    actions.push(0x00);

    let swf = wrap_fws(8, 1, &[make_tag(12, &actions), make_tag(1, &[])]);
    let bundle = decode_swf_bundle(&swf).expect("decode AVM1 functions/with/try/geturl");
    assert!(
        bundle.warnings.is_empty(),
        "unexpected warnings: {:?}",
        bundle.warnings
    );
    let code = &bundle.scripts[0].code;
    assert!(code.contains("__nfSwfFn(function(...__callArgs)"), "{code}");
    assert!(code.contains("__regs[1] = __curThis;"), "{code}");
    assert!(code.contains("__regs[2] = __argsObj;"), "{code}");
    assert!(code.contains("__regs[3] = __superCtor;"), "{code}");
    assert!(code.contains("__regs[4] = _root;"), "{code}");
    assert!(code.contains("__regs[5] = "), "{code}");
    assert!(code.contains("__regs[6] = __global;"), "{code}");
    assert!(code.contains("__regs[7] = __callArgs[0];"), "{code}");
    assert!(code.contains("__withStack.length < 16"), "{code}");
    assert!(code.contains("catchReg: 2"), "{code}");
    assert!(code.contains("__nfSwfGetURL("), "{code}");
    assert!(!code.contains("Function("), "{code}");
}

#[test]
fn swf_loader_decodes_place_object3_filters_blend_modes_and_cache_as_bitmap() {
    use neoflash_core::{BlendMode, FilterSpec};

    let mut po3 = Vec::new();
    po3.push(0x02); // flags1: HasCharacter
    po3.push(0x07); // flags2: HasFilterList(0x01) | HasBlendMode(0x02) | HasCacheAsBitmap(0x04)
    po3.extend_from_slice(&1u16.to_le_bytes()); // depth 1
    po3.extend_from_slice(&1u16.to_le_bytes()); // char_id 1

    // FilterList: 3 filters (Blur=1, Glow=2, ColorMatrix=6)
    po3.push(3);
    // 1) BlurFilter (id = 1): blur_x = 4.0 (FIXED 16.16), blur_y = 6.0, flags = 2 << 3 (passes = 2)
    po3.push(1);
    po3.extend_from_slice(&(4i32 << 16).to_le_bytes());
    po3.extend_from_slice(&(6i32 << 16).to_le_bytes());
    po3.push(2 << 3);
    // 2) GlowFilter (id = 2): rgba(255, 64, 32, 255), blur_x = 8.0, blur_y = 8.0, strength = 2.0 (FIXED8 8.8 = 512), flags = inner(0x80) | knockout(0x40) | passes=1(0x01)
    po3.push(2);
    po3.extend_from_slice(&[255, 64, 32, 255]);
    po3.extend_from_slice(&(8i32 << 16).to_le_bytes());
    po3.extend_from_slice(&(8i32 << 16).to_le_bytes());
    po3.extend_from_slice(&(2i16 << 8).to_le_bytes());
    po3.push(0x80 | 0x40 | 0x01);
    // 3) ColorMatrixFilter (id = 6): 20 * f32 identity
    po3.push(6);
    let mut ident = [0.0f32; 20];
    ident[0] = 1.0;
    ident[6] = 1.0;
    ident[12] = 1.0;
    ident[18] = 1.0;
    for v in ident {
        po3.extend_from_slice(&v.to_le_bytes());
    }

    // BlendMode = 8 (Add), bitmap_cache = 1
    po3.push(8);
    po3.push(1);

    let swf = wrap_fws(
        10,
        1,
        &[
            minimal_define_shape_tag(1),
            make_tag(70, &po3),
            make_tag(1, &[]),
        ],
    );
    let bundle = decode_swf_bundle(&swf).expect("decode PlaceObject3 filters/blend/cache");
    assert!(
        bundle.warnings.is_empty(),
        "unexpected warnings: {:?}",
        bundle.warnings
    );
    bundle.project.validate().expect("project must validate");
    let root_sym = bundle
        .project
        .symbols
        .iter()
        .find(|s| s.id == bundle.project.root)
        .unwrap();
    let el = &root_sym.timeline.layers[0].keyframes[0].elements[0];
    assert!(el.cache_as_bitmap);
    assert_eq!(el.blend_mode, BlendMode::Add);
    assert_eq!(el.filters.len(), 3);
    assert!(
        matches!(el.filters[0], FilterSpec::Blur { blur_x, blur_y, quality: 2, .. } if (blur_x - 4.0).abs() < 1e-3 && (blur_y - 6.0).abs() < 1e-3)
    );
    assert!(matches!(
        el.filters[1],
        FilterSpec::Glow {
            color: 0xFF4020,
            inner: true,
            knockout: true,
            ..
        }
    ));
    assert!(matches!(el.filters[2], FilterSpec::ColorMatrix { .. }));
}

#[test]
fn swf_loader_binds_non_root_symbol_class_sprites_and_avm2_closures() {
    // DefineSprite(char_id=2, frames=2): frame 1 ShowFrame, frame 2 ShowFrame, End
    let mut sprite_payload = Vec::new();
    sprite_payload.extend_from_slice(&2u16.to_le_bytes());
    sprite_payload.extend_from_slice(&2u16.to_le_bytes());
    sprite_payload.extend_from_slice(&make_tag(1, &[]));
    sprite_payload.extend_from_slice(&make_tag(1, &[]));
    sprite_payload.extend_from_slice(&[0, 0]);
    let define_sprite = make_tag(39, &sprite_payload);

    // DoABC2 with 2 classes: ChildSprite (0) and DocMain (1), where ChildSprite iinit registers addFrameScript(1, newfunction 4)
    let mut abc = vec![16, 0, 46, 0];
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 1);
    let strs = [
        "",
        "ChildSprite",
        "DocMain",
        "MovieClip",
        "addFrameScript",
        "hit",
    ];
    push_u30(&mut abc, (strs.len() + 1) as u32);
    for s in strs {
        push_u30(&mut abc, s.len() as u32);
        abc.extend_from_slice(s.as_bytes());
    }
    push_u30(&mut abc, 2);
    abc.extend_from_slice(&[0x16, 1]); // ns 1 = ""
    push_u30(&mut abc, 1);
    // mn: 1=ChildSprite, 2=DocMain, 3=MovieClip, 4=addFrameScript, 5=hit
    push_u30(&mut abc, 6);
    for sid in 2..=6u8 {
        abc.extend_from_slice(&[0x07, 1, sid]);
    }
    // methods: 0=Child.cinit, 1=Child.iinit, 2=Doc.cinit, 3=Doc.iinit, 4=Child.frame2Closure, 5=script_init
    push_u30(&mut abc, 6);
    for _ in 0..6 {
        abc.extend_from_slice(&[0, 0, 0, 0]);
    }
    push_u30(&mut abc, 0); // metadata
    push_u30(&mut abc, 2); // 2 classes
    // instance 0: ChildSprite extends MovieClip, iinit=1
    abc.extend_from_slice(&[1, 3, 0, 0, 1, 0]);
    // instance 1: DocMain extends MovieClip, iinit=3
    abc.extend_from_slice(&[2, 3, 0, 0, 3, 0]);
    // class 0 cinit=0, class 1 cinit=2
    abc.extend_from_slice(&[0, 0, 2, 0]);
    // script 0 init=5
    push_u30(&mut abc, 1);
    abc.extend_from_slice(&[5, 0]);

    // Bodies for 1 (Child.iinit), 3 (Doc.iinit), 4 (frame2Closure)
    let child_iinit = vec![
        0xD0, 0x30, // getlocal_0, pushscope
        0xD0, 0x49, 0, // constructsuper 0
        0x5D, 4, 0x24, 1, 0x40, 4, 0x4F, 4, 2, // addFrameScript(1, newfunction 4)
        0x47,
    ];
    let doc_iinit = vec![0xD0, 0x30, 0xD0, 0x49, 0, 0x47];
    let frame2_fn = vec![0xD0, 0x24, 99, 0x61, 5, 0x47]; // this.hit = 99
    push_u30(&mut abc, 3);
    for (mid, code) in [(1u32, child_iinit), (3, doc_iinit), (4, frame2_fn)] {
        push_u30(&mut abc, mid);
        abc.extend_from_slice(&[6, 4, 4, 6]);
        push_u30(&mut abc, code.len() as u32);
        abc.extend_from_slice(&code);
        abc.extend_from_slice(&[0, 0]);
    }
    let mut do_abc2 = vec![1, 0, 0, 0, b'm', 0];
    do_abc2.extend_from_slice(&abc);

    // SymbolClass: cid 0 -> DocMain, cid 2 -> ChildSprite
    let mut sym_cls = Vec::new();
    sym_cls.extend_from_slice(&2u16.to_le_bytes());
    sym_cls.extend_from_slice(&0u16.to_le_bytes());
    sym_cls.extend_from_slice(b"DocMain\0");
    sym_cls.extend_from_slice(&2u16.to_le_bytes());
    sym_cls.extend_from_slice(b"ChildSprite\0");

    let swf = wrap_fws(
        13,
        1,
        &[
            make_tag(69, &[0x08, 0, 0, 0]),
            define_sprite,
            minimal_place_object2_tag(1, 2),
            make_tag(82, &do_abc2),
            make_tag(76, &sym_cls),
            make_tag(1, &[]),
        ],
    );
    let bundle = decode_swf_bundle(&swf).expect("decode SymbolClass child sprite SWF");
    assert!(
        bundle.warnings.is_empty(),
        "non-root SymbolClass sprite must bind without warnings: {:?}",
        bundle.warnings
    );
    let child_sym = bundle
        .project
        .symbols
        .iter()
        .find(|s| s.class_name.as_deref() == Some("ChildSprite"))
        .expect("DefineSprite symbol bound to ChildSprite");
    assert_eq!(child_sym.linkage_id.as_deref(), Some("ChildSprite"));
    assert_eq!(child_sym.timeline.scripts.len(), 1);
    assert_eq!(child_sym.timeline.scripts[0].frame, 1);
    assert!(bundle.scripts.iter().any(|u| u.location == "avm2-abc-init"));
    assert!(bundle.scripts.iter().any(|u| {
        u.location == "avm2-sprite-frame:frame-2" && u.code.contains("\"ChildSprite\"")
    }));
}

#[test]
fn swf_loader_non_root_only_symbol_class_does_not_attach_child_class_to_root() {
    let mut sprite_payload = Vec::new();
    sprite_payload.extend_from_slice(&2u16.to_le_bytes());
    sprite_payload.extend_from_slice(&2u16.to_le_bytes());
    sprite_payload.extend_from_slice(&make_tag(1, &[]));
    sprite_payload.extend_from_slice(&make_tag(1, &[]));
    sprite_payload.extend_from_slice(&[0, 0]);
    let define_sprite = make_tag(39, &sprite_payload);

    let mut abc = vec![16, 0, 46, 0];
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 1);
    let strs = ["", "ChildOnly", "MovieClip", "addFrameScript", "hit"];
    push_u30(&mut abc, (strs.len() + 1) as u32);
    for s in strs {
        push_u30(&mut abc, s.len() as u32);
        abc.extend_from_slice(s.as_bytes());
    }
    push_u30(&mut abc, 2);
    abc.extend_from_slice(&[0x16, 1]);
    push_u30(&mut abc, 1);
    push_u30(&mut abc, 5);
    for sid in 2..=5u8 {
        abc.extend_from_slice(&[0x07, 1, sid]);
    }
    push_u30(&mut abc, 4);
    for _ in 0..4 {
        abc.extend_from_slice(&[0, 0, 0, 0]);
    }
    push_u30(&mut abc, 0);
    push_u30(&mut abc, 1); // 1 class: ChildOnly (idx 0)
    abc.extend_from_slice(&[1, 2, 0, 0, 1, 0]);
    abc.extend_from_slice(&[0, 0]);
    push_u30(&mut abc, 1);
    abc.extend_from_slice(&[3, 0]);

    let child_iinit = vec![
        0xD0, 0x30, 0xD0, 0x49, 0, 0x5D, 3, 0x24, 1, 0x40, 2, 0x4F, 3, 2, 0x47,
    ];
    let frame2_fn = vec![0xD0, 0x24, 55, 0x61, 4, 0x47];
    let script_init = vec![0xD0, 0x30, 0x47];
    push_u30(&mut abc, 3);
    for (mid, code) in [(1u32, child_iinit), (2, frame2_fn), (3, script_init)] {
        push_u30(&mut abc, mid);
        abc.extend_from_slice(&[6, 4, 4, 6]);
        push_u30(&mut abc, code.len() as u32);
        abc.extend_from_slice(&code);
        abc.extend_from_slice(&[0, 0]);
    }
    let mut do_abc2 = vec![1, 0, 0, 0, b'c', 0];
    do_abc2.extend_from_slice(&abc);

    // SymbolClass maps ONLY cid 2 -> ChildOnly (no cid == 0 Document Class!)
    let mut sym_cls = Vec::new();
    sym_cls.extend_from_slice(&1u16.to_le_bytes());
    sym_cls.extend_from_slice(&2u16.to_le_bytes());
    sym_cls.extend_from_slice(b"ChildOnly\0");

    let swf = wrap_fws(
        13,
        2,
        &[
            make_tag(69, &[0x08, 0, 0, 0]),
            define_sprite,
            minimal_place_object2_tag(1, 2),
            make_tag(82, &do_abc2),
            make_tag(76, &sym_cls),
            make_tag(1, &[]),
            make_tag(1, &[]),
        ],
    );
    let bundle = decode_swf_bundle(&swf).expect("decode non-root-only SymbolClass SWF");
    assert!(bundle.warnings.is_empty(), "{:?}", bundle.warnings);
    let root_sym = bundle
        .project
        .symbols
        .iter()
        .find(|s| s.id == bundle.project.root)
        .unwrap();
    assert!(
        root_sym.timeline.scripts.is_empty(),
        "root timeline must NOT receive ChildOnly doc-ctor or frame scripts when cid 0 is absent: {:?}",
        root_sym.timeline.scripts
    );
    assert!(
        !bundle
            .scripts
            .iter()
            .any(|u| u.location.starts_with("avm2-doc-ctor")
                || u.location.starts_with("avm2-script")),
        "unexpected root AVM2 script unit: {:?}",
        bundle.scripts
    );
}
