//! Pure-Rust SWF AVM1 (`DoAction`, `DoInitAction`, `DefineButton`/`DefineButton2`
//! `BUTTONCONDACTION`, and `PlaceObject2`/`PlaceObject3` `CLIPACTIONS`) bytecode
//! decompiler into ActionScript 1 / ActionScript 2 source.

use super::bits::SwfSlice;
use neoflash_core::{ObjectEvent, ObjectScript, Script, ScriptLanguage};
use std::collections::HashMap;

const MAX_AVM1_INSNS: usize = 50_000;
const MAX_AVM1_DEPTH: usize = 24;

#[derive(Clone, Debug)]
pub(crate) struct DecompiledObjectHandler {
    pub script: ObjectScript,
    pub approximated_event: bool,
    pub unrecognized_opcodes: usize,
}

pub(crate) fn script_language_for_swf(swf_version: u8) -> ScriptLanguage {
    if swf_version <= 5 {
        ScriptLanguage::ActionScript1
    } else {
        ScriptLanguage::ActionScript2
    }
}

/// Decompile a raw AVM1 action block (`DoAction`, `DoInitAction`, or `DefineButton` actions),
/// returning `(source, unrecognized_opcode_count)`.
pub(crate) fn decompile_actions(
    bytes: &[u8],
    swf_version: u8,
    legacy_cp_warned: &mut bool,
) -> (String, usize) {
    let mut ctx = Avm1Context {
        swf_version,
        constants: Vec::new(),
        reg_names: HashMap::new(),
        unrecognized: 0,
        legacy_cp_warned,
    };
    let insns = decode_action_stream(
        bytes,
        swf_version,
        ctx.legacy_cp_warned,
        &mut ctx.unrecognized,
        0,
    );
    let mut stack = Vec::new();
    let lines = decompile_range(&insns, 0, insns.len(), &mut ctx, &mut stack, 0);
    (join_lines(&lines, 0), ctx.unrecognized)
}

/// Parse and decompile `DefineButton2` (tag 34) chained `BUTTONCONDACTION` records.
pub(crate) fn parse_and_decompile_button2_actions(
    bytes: &[u8],
    swf_version: u8,
    legacy_cp_warned: &mut bool,
) -> Vec<DecompiledObjectHandler> {
    let lang = script_language_for_swf(swf_version);
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut guard = 0usize;
    while pos + 4 <= bytes.len() && guard < 1024 {
        guard += 1;
        let cond_size = usize::from(u16::from_le_bytes([bytes[pos], bytes[pos + 1]]));
        let cond_b0 = bytes[pos + 2];
        let cond_b1 = bytes[pos + 3];
        let end = if cond_size == 0 {
            bytes.len()
        } else {
            pos.saturating_add(cond_size).min(bytes.len())
        };
        if end <= pos + 4 {
            if cond_size == 0 {
                break;
            }
            pos = end;
            continue;
        }
        let action_slice = &bytes[pos + 4..end];
        let (body, unrecognized) = decompile_actions(action_slice, swf_version, legacy_cp_warned);
        if !body.trim().is_empty() {
            let conds = describe_button_conditions(cond_b0, cond_b1);
            let is_direct_click =
                !conds.is_empty() && conds.iter().all(|c| c == "release" || c == "press");
            let source = if is_direct_click {
                body
            } else {
                let header = if conds.is_empty() {
                    "release".to_owned()
                } else {
                    conds.join(", ")
                };
                format!("// on({header})\n{body}")
            };
            out.push(DecompiledObjectHandler {
                script: ObjectScript {
                    event: ObjectEvent::Click,
                    script: Script {
                        language: lang,
                        source,
                    },
                },
                approximated_event: !is_direct_click,
                unrecognized_opcodes: unrecognized,
            });
        }
        if cond_size == 0 || end >= bytes.len() {
            break;
        }
        pos = end;
    }
    out
}

/// Parse and decompile `CLIPACTIONS` on `PlaceObject2` (tag 26) or `PlaceObject3` (tag 70).
pub(crate) fn parse_and_decompile_clip_actions(
    r: &mut SwfSlice<'_>,
    swf_version: u8,
    legacy_cp_warned: &mut bool,
) -> Vec<DecompiledObjectHandler> {
    let lang = script_language_for_swf(swf_version);
    let mut out = Vec::new();
    let Some(_reserved) = r.u16() else {
        return out;
    };
    let wide_flags = swf_version >= 6;
    let Some(_all_flags) = read_clip_event_flags(r, wide_flags) else {
        return out;
    };

    for _ in 0..1024 {
        let Some(flags) = read_clip_event_flags(r, wide_flags) else {
            break;
        };
        if flags == 0 {
            break;
        }
        let Some(record_size) = r.u32().map(|v| v as usize) else {
            break;
        };
        let Some(record_bytes) = r.take(record_size) else {
            break;
        };
        // `ClipEventKeyPress` (`b2 & 0x02`, i.e., `0x0002_0000` in LE u32) prefixes a 1-byte KeyCode.
        let has_key_press = (flags & 0x0002_0000) != 0;
        let (key_code, action_bytes) = if has_key_press {
            if record_bytes.is_empty() {
                continue;
            }
            (Some(record_bytes[0]), &record_bytes[1..])
        } else {
            (None, record_bytes)
        };
        let (body, unrecognized) = decompile_actions(action_bytes, swf_version, legacy_cp_warned);
        if body.trim().is_empty() {
            continue;
        }
        let (event, approx, label) = classify_clip_event_flags(flags, key_code);
        let source = if !approx {
            body
        } else {
            format!("// onClipEvent({label})\n{body}")
        };
        out.push(DecompiledObjectHandler {
            script: ObjectScript {
                event,
                script: Script {
                    language: lang,
                    source,
                },
            },
            approximated_event: approx,
            unrecognized_opcodes: unrecognized,
        });
    }
    out
}

fn read_clip_event_flags(r: &mut SwfSlice<'_>, wide: bool) -> Option<u32> {
    if wide {
        r.u32()
    } else {
        r.u16().map(u32::from)
    }
}

fn classify_clip_event_flags(flags: u32, key_code: Option<u8>) -> (ObjectEvent, bool, String) {
    // Exact single-event matches that map 1:1 to NeoFlash's ObjectEvent variants:
    match flags {
        0x0000_0001 => return (ObjectEvent::Load, false, "load".into()),
        0x0000_0002 => return (ObjectEvent::EnterFrame, false, "enterFrame".into()),
        0x0000_0400 | 0x0000_0800 | 0x0000_0C00 => {
            return (ObjectEvent::Click, false, "release".into());
        }
        _ => {}
    }

    let mut names = Vec::new();
    let table: &[(u32, &str)] = &[
        (0x0000_0001, "load"),
        (0x0000_0002, "enterFrame"),
        (0x0000_0004, "unload"),
        (0x0000_0008, "mouseMove"),
        (0x0000_0010, "mouseDown"),
        (0x0000_0020, "mouseUp"),
        (0x0000_0040, "keyDown"),
        (0x0000_0080, "keyUp"),
        (0x0000_0100, "data"),
        (0x0000_0200, "initialize"),
        (0x0000_0400, "press"),
        (0x0000_0800, "release"),
        (0x0000_1000, "releaseOutside"),
        (0x0000_2000, "rollOver"),
        (0x0000_4000, "rollOut"),
        (0x0000_8000, "dragOver"),
        (0x0001_0000, "dragOut"),
        (0x0004_0000, "construct"),
    ];
    for &(mask, name) in table {
        if (flags & mask) != 0 {
            names.push(name.to_owned());
        }
    }
    if (flags & 0x0002_0000) != 0 {
        let key_desc = format_button_key(key_code.unwrap_or(0));
        names.push(format!("keyPress \"{key_desc}\""));
    }
    let primary = if (flags & 0x0000_0002) != 0 {
        ObjectEvent::EnterFrame
    } else if (flags & (0x0000_0400 | 0x0000_0800 | 0x0000_0010 | 0x0000_0020)) != 0 {
        ObjectEvent::Click
    } else {
        ObjectEvent::Load
    };
    let label = if names.is_empty() {
        format!("0x{flags:x}")
    } else {
        names.join(", ")
    };
    (primary, true, label)
}

fn describe_button_conditions(b0: u8, b1: u8) -> Vec<String> {
    let mut conds = Vec::new();
    if (b0 & 0x04) != 0 {
        conds.push("press".into());
    }
    if (b0 & 0x08) != 0 {
        conds.push("release".into());
    }
    if (b0 & 0x40) != 0 {
        conds.push("releaseOutside".into());
    }
    if (b0 & 0x01) != 0 {
        conds.push("rollOver".into());
    }
    if (b0 & 0x02) != 0 {
        conds.push("rollOut".into());
    }
    if (b0 & 0xA0) != 0 {
        conds.push("dragOver".into());
    }
    if (b0 & 0x10) != 0 || (b1 & 0x01) != 0 {
        conds.push("dragOut".into());
    }
    let key = b1 >> 1;
    if key != 0 {
        conds.push(format!("keyPress \"{}\"", format_button_key(key)));
    }
    conds
}

fn format_button_key(code: u8) -> String {
    match code {
        1 => "<Left>".into(),
        2 => "<Right>".into(),
        3 => "<Home>".into(),
        4 => "<End>".into(),
        5 => "<Insert>".into(),
        6 => "<Delete>".into(),
        8 => "<Backspace>".into(),
        13 => "<Enter>".into(),
        14 => "<Up>".into(),
        15 => "<Down>".into(),
        16 => "<PageUp>".into(),
        17 => "<PageDown>".into(),
        18 => "<Tab>".into(),
        19 => "<Escape>".into(),
        32 => "<Space>".into(),
        c @ 33..=126 => (c as char).to_string(),
        other => format!("key#{other}"),
    }
}

#[derive(Clone, Debug)]
enum PushItem {
    String(String),
    Float(f32),
    Null,
    Undefined,
    Register(u8),
    Bool(bool),
    Double(f64),
    Int(i32),
    Const(u16),
}

#[derive(Clone, Debug)]
struct Avm1Insn {
    offset: usize,
    next_offset: usize,
    op: Avm1Op,
}

#[derive(Clone, Debug)]
enum Avm1Op {
    Simple(u8),
    GotoFrame(u16),
    GetUrl {
        url: String,
        target: String,
    },
    StoreRegister(u8),
    ConstantPool(Vec<String>),
    WaitForFrame {
        frame: u16,
        skip: u8,
    },
    SetTarget(String),
    GotoLabel(String),
    WaitForFrame2 {
        skip: u8,
    },
    Push(Vec<PushItem>),
    Jump(i16),
    GetUrl2(u8),
    If(i16),
    GotoFrame2 {
        play: bool,
        scene_bias: Option<u16>,
    },
    With {
        body: Vec<Avm1Insn>,
    },
    DefineFunction {
        name: String,
        params: Vec<(u8, String)>,
        preload_regs: Vec<(u8, String)>,
        body: Vec<Avm1Insn>,
    },
    Try {
        catch_var: Option<String>,
        catch_reg: Option<u8>,
        try_body: Vec<Avm1Insn>,
        catch_body: Option<Vec<Avm1Insn>>,
        finally_body: Option<Vec<Avm1Insn>>,
    },
    Unknown {
        code: u8,
        len: usize,
    },
}

fn decode_action_stream(
    bytes: &[u8],
    swf_version: u8,
    legacy_cp_warned: &mut bool,
    unrecognized: &mut usize,
    depth: usize,
) -> Vec<Avm1Insn> {
    let mut out = Vec::new();
    if depth > MAX_AVM1_DEPTH {
        return out;
    }
    let mut pos = 0usize;
    while pos < bytes.len() && out.len() < MAX_AVM1_INSNS {
        let offset = pos;
        let code = bytes[pos];
        pos += 1;
        if code == 0x00 {
            break;
        }
        if code < 0x80 {
            let known = is_known_simple_op(code);
            if !known {
                *unrecognized += 1;
            }
            out.push(Avm1Insn {
                offset,
                next_offset: pos,
                op: if known {
                    Avm1Op::Simple(code)
                } else {
                    Avm1Op::Unknown { code, len: 0 }
                },
            });
            continue;
        }
        if pos + 2 > bytes.len() {
            break;
        }
        let len = usize::from(u16::from_le_bytes([bytes[pos], bytes[pos + 1]]));
        pos += 2;
        let payload_end = pos.saturating_add(len).min(bytes.len());
        let payload = &bytes[pos..payload_end];
        pos = payload_end;

        let op = match code {
            0x81 if payload.len() >= 2 => {
                Avm1Op::GotoFrame(u16::from_le_bytes([payload[0], payload[1]]))
            }
            0x83 => {
                let mut pr = SwfSlice::new(payload);
                let (url, cp1) = pr.c_string(swf_version).unwrap_or_default();
                let (target, cp2) = pr.c_string(swf_version).unwrap_or_default();
                *legacy_cp_warned |= cp1 || cp2;
                Avm1Op::GetUrl { url, target }
            }
            0x87 if !payload.is_empty() => Avm1Op::StoreRegister(payload[0]),
            0x88 if payload.len() >= 2 => {
                let mut pr = SwfSlice::new(payload);
                let count = usize::from(pr.u16().unwrap_or(0));
                let mut pool = Vec::with_capacity(count.min(4096));
                for _ in 0..count {
                    let Some((s, cp)) = pr.c_string(swf_version) else {
                        break;
                    };
                    *legacy_cp_warned |= cp;
                    pool.push(s);
                }
                Avm1Op::ConstantPool(pool)
            }
            0x8A if payload.len() >= 3 => Avm1Op::WaitForFrame {
                frame: u16::from_le_bytes([payload[0], payload[1]]),
                skip: payload[2],
            },
            0x8B => {
                let mut pr = SwfSlice::new(payload);
                let (target, cp) = pr.c_string(swf_version).unwrap_or_default();
                *legacy_cp_warned |= cp;
                Avm1Op::SetTarget(target)
            }
            0x8C => {
                let mut pr = SwfSlice::new(payload);
                let (label, cp) = pr.c_string(swf_version).unwrap_or_default();
                *legacy_cp_warned |= cp;
                Avm1Op::GotoLabel(label)
            }
            0x8D if !payload.is_empty() => Avm1Op::WaitForFrame2 { skip: payload[0] },
            0x8E => {
                // ActionDefineFunction2: body of `code_size` bytes immediately follows header.
                let mut pr = SwfSlice::new(payload);
                let (name, cp) = pr.c_string(swf_version).unwrap_or_default();
                *legacy_cp_warned |= cp;
                let num_params = usize::from(pr.u16().unwrap_or(0));
                let _reg_count = pr.u8().unwrap_or(0);
                let flags_lo = pr.u8().unwrap_or(0);
                let flags_hi = pr.u8().unwrap_or(0);
                let mut params = Vec::with_capacity(num_params.min(256));
                for _ in 0..num_params {
                    let reg = pr.u8().unwrap_or(0);
                    let (pname, pcp) = pr.c_string(swf_version).unwrap_or_default();
                    *legacy_cp_warned |= pcp;
                    params.push((reg, pname));
                }
                let code_size = usize::from(pr.u16().unwrap_or(0));
                let body_end = pos.saturating_add(code_size).min(bytes.len());
                let body_bytes = &bytes[pos..body_end];
                pos = body_end;

                // Implicit preload registers starting at register 1 in SWF19 specification order.
                let mut preload_regs = Vec::new();
                let mut next_reg = 1u8;
                for (cond, ident) in [
                    ((flags_lo & 0x01) != 0, "this"),
                    ((flags_lo & 0x04) != 0, "arguments"),
                    ((flags_lo & 0x10) != 0, "super"),
                    ((flags_lo & 0x40) != 0, "_root"),
                    ((flags_lo & 0x80) != 0, "_parent"),
                    ((flags_hi & 0x01) != 0, "_global"),
                ] {
                    if cond {
                        preload_regs.push((next_reg, ident.to_owned()));
                        next_reg = next_reg.saturating_add(1);
                    }
                }
                let body = decode_action_stream(
                    body_bytes,
                    swf_version,
                    legacy_cp_warned,
                    unrecognized,
                    depth + 1,
                );
                Avm1Op::DefineFunction {
                    name,
                    params,
                    preload_regs,
                    body,
                }
            }
            0x8F if payload.len() >= 7 => {
                // ActionTry: try/catch/finally bodies immediately follow the header.
                let mut pr = SwfSlice::new(payload);
                let flags = pr.u8().unwrap_or(0);
                let has_catch = (flags & 0x01) != 0;
                let has_finally = (flags & 0x02) != 0;
                let catch_in_reg = (flags & 0x04) != 0;
                let try_size = usize::from(pr.u16().unwrap_or(0));
                let catch_size = usize::from(pr.u16().unwrap_or(0));
                let finally_size = usize::from(pr.u16().unwrap_or(0));
                let (catch_var, catch_reg) = if catch_in_reg {
                    (None, pr.u8())
                } else {
                    let (cname, cp) = pr.c_string(swf_version).unwrap_or_default();
                    *legacy_cp_warned |= cp;
                    (Some(cname), None)
                };
                let try_end = pos.saturating_add(try_size).min(bytes.len());
                let try_bytes = &bytes[pos..try_end];
                pos = try_end;
                let catch_bytes = if has_catch {
                    let c_end = pos.saturating_add(catch_size).min(bytes.len());
                    let s = &bytes[pos..c_end];
                    pos = c_end;
                    Some(s)
                } else {
                    pos = pos.saturating_add(catch_size).min(bytes.len());
                    None
                };
                let finally_bytes = if has_finally {
                    let f_end = pos.saturating_add(finally_size).min(bytes.len());
                    let s = &bytes[pos..f_end];
                    pos = f_end;
                    Some(s)
                } else {
                    pos = pos.saturating_add(finally_size).min(bytes.len());
                    None
                };
                Avm1Op::Try {
                    catch_var,
                    catch_reg,
                    try_body: decode_action_stream(
                        try_bytes,
                        swf_version,
                        legacy_cp_warned,
                        unrecognized,
                        depth + 1,
                    ),
                    catch_body: catch_bytes.map(|b| {
                        decode_action_stream(
                            b,
                            swf_version,
                            legacy_cp_warned,
                            unrecognized,
                            depth + 1,
                        )
                    }),
                    finally_body: finally_bytes.map(|b| {
                        decode_action_stream(
                            b,
                            swf_version,
                            legacy_cp_warned,
                            unrecognized,
                            depth + 1,
                        )
                    }),
                }
            }
            0x94 if payload.len() >= 2 => {
                let with_size = usize::from(u16::from_le_bytes([payload[0], payload[1]]));
                let body_end = pos.saturating_add(with_size).min(bytes.len());
                let body_bytes = &bytes[pos..body_end];
                pos = body_end;
                Avm1Op::With {
                    body: decode_action_stream(
                        body_bytes,
                        swf_version,
                        legacy_cp_warned,
                        unrecognized,
                        depth + 1,
                    ),
                }
            }
            0x96 => Avm1Op::Push(parse_push_items(payload, swf_version, legacy_cp_warned)),
            0x99 if payload.len() >= 2 => {
                Avm1Op::Jump(i16::from_le_bytes([payload[0], payload[1]]))
            }
            0x9A if !payload.is_empty() => Avm1Op::GetUrl2(payload[0]),
            0x9B => {
                // ActionDefineFunction: body of `code_size` bytes immediately follows header.
                let mut pr = SwfSlice::new(payload);
                let (name, cp) = pr.c_string(swf_version).unwrap_or_default();
                *legacy_cp_warned |= cp;
                let num_params = usize::from(pr.u16().unwrap_or(0));
                let mut params = Vec::with_capacity(num_params.min(256));
                for _ in 0..num_params {
                    let (pname, pcp) = pr.c_string(swf_version).unwrap_or_default();
                    *legacy_cp_warned |= pcp;
                    params.push((0, pname));
                }
                let code_size = usize::from(pr.u16().unwrap_or(0));
                let body_end = pos.saturating_add(code_size).min(bytes.len());
                let body_bytes = &bytes[pos..body_end];
                pos = body_end;
                let body = decode_action_stream(
                    body_bytes,
                    swf_version,
                    legacy_cp_warned,
                    unrecognized,
                    depth + 1,
                );
                Avm1Op::DefineFunction {
                    name,
                    params,
                    preload_regs: Vec::new(),
                    body,
                }
            }
            0x9D if payload.len() >= 2 => Avm1Op::If(i16::from_le_bytes([payload[0], payload[1]])),
            0x9E => Avm1Op::Simple(0x9E),
            0x9F if !payload.is_empty() => {
                let flags = payload[0];
                let play = (flags & 0x01) != 0;
                let scene_bias = if (flags & 0x02) != 0 && payload.len() >= 3 {
                    Some(u16::from_le_bytes([payload[1], payload[2]]))
                } else {
                    None
                };
                Avm1Op::GotoFrame2 { play, scene_bias }
            }
            _ => {
                *unrecognized += 1;
                Avm1Op::Unknown { code, len }
            }
        };

        out.push(Avm1Insn {
            offset,
            next_offset: pos,
            op,
        });
    }
    out
}

fn is_known_simple_op(code: u8) -> bool {
    matches!(
        code,
        0x04..=0x15
            | 0x17
            | 0x18
            | 0x1C
            | 0x1D
            | 0x20..=0x2C
            | 0x30..=0x37
            | 0x3A..=0x4F
            | 0x50..=0x55
            | 0x60..=0x69
            | 0x9E
    )
}

fn parse_push_items(payload: &[u8], swf_version: u8, legacy_cp_warned: &mut bool) -> Vec<PushItem> {
    let mut r = SwfSlice::new(payload);
    let mut items = Vec::new();
    while let Some(ty) = r.u8() {
        let item = match ty {
            0 => {
                let Some((s, cp)) = r.c_string(swf_version) else {
                    break;
                };
                *legacy_cp_warned |= cp;
                PushItem::String(s)
            }
            1 => {
                let Some(b) = r.take(4) else { break };
                PushItem::Float(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            }
            2 => PushItem::Null,
            3 => PushItem::Undefined,
            4 => {
                let Some(reg) = r.u8() else { break };
                PushItem::Register(reg)
            }
            5 => {
                let Some(v) = r.u8() else { break };
                PushItem::Bool(v != 0)
            }
            6 => {
                let Some(b) = r.take(8) else { break };
                // SWF ActionPush double stores the high 32-bit LE word first, then low 32-bit LE word.
                let val = f64::from_le_bytes([b[4], b[5], b[6], b[7], b[0], b[1], b[2], b[3]]);
                PushItem::Double(val)
            }
            7 => {
                let Some(v) = r.i32() else { break };
                PushItem::Int(v)
            }
            8 => {
                let Some(idx) = r.u8() else { break };
                PushItem::Const(u16::from(idx))
            }
            9 => {
                let Some(idx) = r.u16() else { break };
                PushItem::Const(idx)
            }
            _ => break,
        };
        items.push(item);
    }
    items
}

struct Avm1Context<'a> {
    swf_version: u8,
    constants: Vec<String>,
    reg_names: HashMap<u8, String>,
    unrecognized: usize,
    legacy_cp_warned: &'a mut bool,
}

impl Avm1Context<'_> {
    fn reg_expr(&self, reg: u8) -> String {
        self.reg_names
            .get(&reg)
            .cloned()
            .unwrap_or_else(|| format!("__reg{reg}"))
    }
}

#[derive(Clone, Debug)]
struct Expr {
    code: String,
    raw_string: Option<String>,
    int_val: Option<i64>,
    negated_of: Option<String>,
}

impl Expr {
    fn atom(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            raw_string: None,
            int_val: None,
            negated_of: None,
        }
    }

    fn string_lit(s: String) -> Self {
        Self {
            code: quote_as_string(&s),
            raw_string: Some(s),
            int_val: None,
            negated_of: None,
        }
    }

    fn int_lit(v: i64) -> Self {
        Self {
            code: v.to_string(),
            raw_string: None,
            int_val: Some(v),
            negated_of: None,
        }
    }

    fn inverted_condition(&self) -> String {
        if let Some(inner) = &self.negated_of {
            inner.clone()
        } else {
            format!("!({})", self.code)
        }
    }
}

fn pop_expr(stack: &mut Vec<Expr>) -> Expr {
    stack.pop().unwrap_or_else(|| Expr::atom("undefined"))
}

fn target_offset(insn: &Avm1Insn, rel: i16) -> isize {
    insn.next_offset as isize + isize::from(rel)
}

fn find_insn_at(insns: &[Avm1Insn], start: usize, end: usize, target_off: isize) -> Option<usize> {
    if target_off < 0 {
        return None;
    }
    let target = target_off as usize;
    if let Some(last) = insns.get(end.saturating_sub(1))
        && target == last.next_offset
    {
        return Some(end);
    }
    (start..end).find(|&idx| insns[idx].offset == target)
}

fn decompile_range(
    insns: &[Avm1Insn],
    start: usize,
    end: usize,
    ctx: &mut Avm1Context<'_>,
    stack: &mut Vec<Expr>,
    depth: usize,
) -> Vec<String> {
    let mut lines = Vec::new();
    if depth > MAX_AVM1_DEPTH || start >= end {
        return lines;
    }

    // Build backward-jump loop header lookup: `header_idx -> back_jump_idx`.
    let mut loop_back_jumps: HashMap<usize, usize> = HashMap::new();
    for idx in start..end {
        if let Avm1Op::Jump(rel) = insns[idx].op
            && rel < 0
            && let Some(hdr) = find_insn_at(insns, start, idx + 1, target_offset(&insns[idx], rel))
        {
            loop_back_jumps.entry(hdr).or_insert(idx);
        }
    }

    let mut pc = start;
    while pc < end {
        // Check for a `while (cond) { ... }` loop starting at `pc`.
        if let Some(&back_idx) = loop_back_jumps.get(&pc)
            && back_idx > pc
            && let Some((cond_idx, exit_idx)) = find_loop_exit_if(insns, pc, back_idx, end)
        {
            // Evaluate loop condition instructions `[pc..cond_idx]` on a cloned/local stack.
            let mut cond_stack = stack.clone();
            let mut pre_lines =
                decompile_linear_no_branch(insns, pc, cond_idx, ctx, &mut cond_stack, depth + 1);
            lines.append(&mut pre_lines);
            let cond_expr = pop_expr(&mut cond_stack);
            let while_cond = cond_expr.inverted_condition();
            let mut body_stack = Vec::new();
            let body_lines = decompile_range(
                insns,
                cond_idx + 1,
                back_idx,
                ctx,
                &mut body_stack,
                depth + 1,
            );
            lines.push(format!("while ({while_cond}) {{"));
            for bl in body_lines {
                lines.push(format!("    {bl}"));
            }
            lines.push("}".into());
            pc = exit_idx.max(back_idx + 1);
            continue;
        }

        let insn = &insns[pc];
        match &insn.op {
            // Fold `GotoFrame(f)` + `Play` into `gotoAndPlay(f + 1);`
            Avm1Op::GotoFrame(frame) => {
                let one_based = u32::from(*frame) + 1;
                if matches!(insns.get(pc + 1).map(|i| &i.op), Some(Avm1Op::Simple(0x06))) {
                    lines.push(format!("gotoAndPlay({one_based});"));
                    pc += 2;
                } else {
                    lines.push(format!("gotoAndStop({one_based});"));
                    pc += 1;
                }
            }
            // Fold `GotoLabel(lbl)` + `Play` into `gotoAndPlay("lbl");`
            Avm1Op::GotoLabel(label) => {
                let q = quote_as_string(label);
                if matches!(insns.get(pc + 1).map(|i| &i.op), Some(Avm1Op::Simple(0x06))) {
                    lines.push(format!("gotoAndPlay({q});"));
                    pc += 2;
                } else {
                    lines.push(format!("gotoAndStop({q});"));
                    pc += 1;
                }
            }
            // Structured `SetTarget("mc") ... SetTarget("")` -> `tellTarget("mc") { ... }`
            Avm1Op::SetTarget(target) if !target.is_empty() => {
                if let Some(close_idx) = (pc + 1..end)
                    .find(|&k| matches!(&insns[k].op, Avm1Op::SetTarget(t) if t.is_empty()))
                {
                    let inner = decompile_range(insns, pc + 1, close_idx, ctx, stack, depth + 1);
                    lines.push(format!("tellTarget({}) {{", quote_as_string(target)));
                    for il in inner {
                        lines.push(format!("    {il}"));
                    }
                    lines.push("}".into());
                    pc = close_idx + 1;
                } else {
                    lines.push(format!("tellTarget({});", quote_as_string(target)));
                    pc += 1;
                }
            }
            Avm1Op::SetTarget(_) => {
                pc += 1;
            }
            // Structured forward `If(rel)` -> short-circuit `&&`/`||`, `if (...) { ... } [else { ... }]`, or ternary expression.
            Avm1Op::If(rel) if *rel > 0 => {
                let cond = pop_expr(stack);
                if let Some(target_idx) =
                    find_insn_at(insns, pc + 1, end, target_offset(insn, *rel))
                    && target_idx > pc + 1
                {
                    let is_short_circuit_or = pc > start
                        && matches!(insns[pc - 1].op, Avm1Op::Simple(0x4C))
                        && matches!(insns[pc + 1].op, Avm1Op::Simple(0x17));
                    let is_short_circuit_and = pc >= start + 2
                        && matches!(insns[pc - 2].op, Avm1Op::Simple(0x4C))
                        && matches!(insns[pc - 1].op, Avm1Op::Simple(0x12))
                        && matches!(insns[pc + 1].op, Avm1Op::Simple(0x17));
                    if (is_short_circuit_or || is_short_circuit_and)
                        && target_idx > pc + 2
                        && !stack.is_empty()
                    {
                        let mut rhs_stack = stack.clone();
                        let lhs = pop_expr(&mut rhs_stack);
                        let rhs_lines = decompile_range(
                            insns,
                            pc + 2,
                            target_idx,
                            ctx,
                            &mut rhs_stack,
                            depth + 1,
                        );
                        if rhs_lines.is_empty() && rhs_stack.len() == stack.len() {
                            let rhs = pop_expr(&mut rhs_stack);
                            let _ = pop_expr(stack);
                            let op_str = if is_short_circuit_and { "&&" } else { "||" };
                            stack.push(Expr::atom(format!("({} {op_str} {})", lhs.code, rhs.code)));
                            pc = target_idx;
                            continue;
                        }
                    }

                    let if_cond = cond.inverted_condition();
                    // Check if the last instruction of the `then` block is a forward `Jump` over an `else` block.
                    if target_idx >= pc + 2
                        && let Avm1Op::Jump(else_rel) = insns[target_idx - 1].op
                        && else_rel > 0
                        && let Some(else_end) = find_insn_at(
                            insns,
                            target_idx,
                            end,
                            target_offset(&insns[target_idx - 1], else_rel),
                        )
                        && else_end > target_idx
                    {
                        let mut then_stack = stack.clone();
                        let then_lines = decompile_range(
                            insns,
                            pc + 1,
                            target_idx - 1,
                            ctx,
                            &mut then_stack,
                            depth + 1,
                        );
                        let mut else_stack = stack.clone();
                        let else_lines = decompile_range(
                            insns,
                            target_idx,
                            else_end,
                            ctx,
                            &mut else_stack,
                            depth + 1,
                        );
                        // Ternary / short-circuit expression (`cond ? a : b`) leaves one extra item on both stacks and emits no statements!
                        if then_lines.is_empty()
                            && else_lines.is_empty()
                            && then_stack.len() == stack.len() + 1
                            && else_stack.len() == stack.len() + 1
                        {
                            let t_val = pop_expr(&mut then_stack);
                            let e_val = pop_expr(&mut else_stack);
                            stack.push(Expr::atom(format!(
                                "({if_cond} ? {} : {})",
                                t_val.code, e_val.code
                            )));
                        } else {
                            lines.push(format!("if ({if_cond}) {{"));
                            for tl in then_lines {
                                lines.push(format!("    {tl}"));
                            }
                            lines.push("} else {".into());
                            for el in else_lines {
                                lines.push(format!("    {el}"));
                            }
                            lines.push("}".into());
                        }
                        pc = else_end;
                        continue;
                    }

                    let mut then_stack = stack.clone();
                    let then_lines =
                        decompile_range(insns, pc + 1, target_idx, ctx, &mut then_stack, depth + 1);
                    lines.push(format!("if ({if_cond}) {{"));
                    for tl in then_lines {
                        lines.push(format!("    {tl}"));
                    }
                    lines.push("}".into());
                    pc = target_idx;
                    continue;
                }
                lines.push(format!("if ({}) {{ /* jump {:+} */ }}", cond.code, rel));
                pc += 1;
            }
            Avm1Op::If(rel) => {
                let cond = pop_expr(stack);
                lines.push(format!("if ({}) {{ /* loop {:+} */ }}", cond.code, rel));
                pc += 1;
            }
            Avm1Op::Jump(rel) => {
                if *rel > 0
                    && let Some(target_idx) =
                        find_insn_at(insns, pc + 1, end, target_offset(insn, *rel))
                {
                    pc = target_idx;
                    continue;
                }
                pc += 1;
            }
            _ => {
                let consumed = step_single_insn(insns, pc, end, ctx, stack, &mut lines, depth);
                pc += consumed.max(1);
            }
        }
    }
    lines
}

fn find_loop_exit_if(
    insns: &[Avm1Insn],
    hdr: usize,
    back_idx: usize,
    end: usize,
) -> Option<(usize, usize)> {
    let after_back_off = insns[back_idx].next_offset as isize;
    for idx in hdr..back_idx {
        if let Avm1Op::If(rel) = insns[idx].op
            && rel > 0
        {
            let target_off = target_offset(&insns[idx], rel);
            if target_off == after_back_off
                && let Some(exit_idx) = find_insn_at(insns, back_idx + 1, end, target_off)
            {
                return Some((idx, exit_idx));
            }
            if target_off == after_back_off {
                return Some((idx, back_idx + 1));
            }
        }
    }
    None
}

fn decompile_linear_no_branch(
    insns: &[Avm1Insn],
    start: usize,
    end: usize,
    ctx: &mut Avm1Context<'_>,
    stack: &mut Vec<Expr>,
    depth: usize,
) -> Vec<String> {
    let mut lines = Vec::new();
    let mut pc = start;
    while pc < end {
        let consumed = step_single_insn(insns, pc, end, ctx, stack, &mut lines, depth);
        pc += consumed.max(1);
    }
    lines
}

fn step_single_insn(
    insns: &[Avm1Insn],
    pc: usize,
    end: usize,
    ctx: &mut Avm1Context<'_>,
    stack: &mut Vec<Expr>,
    lines: &mut Vec<String>,
    depth: usize,
) -> usize {
    let insn = &insns[pc];
    match &insn.op {
        Avm1Op::ConstantPool(pool) => {
            ctx.constants.clone_from(pool);
        }
        Avm1Op::Push(items) => {
            for item in items {
                let expr = match item {
                    PushItem::String(s) => Expr::string_lit(s.clone()),
                    PushItem::Float(f) => Expr::atom(format_number(f64::from(*f))),
                    PushItem::Null => Expr::atom("null"),
                    PushItem::Undefined => Expr::atom("undefined"),
                    PushItem::Register(r) => Expr::atom(ctx.reg_expr(*r)),
                    PushItem::Bool(b) => Expr::atom(if *b { "true" } else { "false" }),
                    PushItem::Double(d) => Expr::atom(format_number(*d)),
                    PushItem::Int(i) => Expr::int_lit(i64::from(*i)),
                    PushItem::Const(idx) => {
                        if let Some(s) = ctx.constants.get(usize::from(*idx)) {
                            Expr::string_lit(s.clone())
                        } else {
                            Expr::atom("undefined")
                        }
                    }
                };
                stack.push(expr);
            }
        }
        Avm1Op::StoreRegister(reg) => {
            let val = stack
                .last()
                .cloned()
                .unwrap_or_else(|| Expr::atom("undefined"));
            let rname = ctx.reg_expr(*reg);
            // If immediately followed by `Pop` (`0x17`), emit clean assignment statement `reg = val;`.
            if pc + 1 < end && matches!(insns[pc + 1].op, Avm1Op::Simple(0x17)) {
                let _ = stack.pop();
                if rname != val.code {
                    lines.push(format!("{rname} = {};", val.code));
                }
                return 2;
            }
            if let Some(top) = stack.last_mut()
                && rname != val.code
            {
                *top = Expr::atom(format!("({rname} = {})", val.code));
            }
        }
        Avm1Op::GetUrl { url, target } => {
            if let Some(cmd) = url.strip_prefix("FSCommand:") {
                lines.push(format!(
                    "fscommand({}, {});",
                    quote_as_string(cmd),
                    quote_as_string(target)
                ));
            } else if target.is_empty() {
                lines.push(format!("getURL({});", quote_as_string(url)));
            } else {
                lines.push(format!(
                    "getURL({}, {});",
                    quote_as_string(url),
                    quote_as_string(target)
                ));
            }
        }
        Avm1Op::GetUrl2(flags) => {
            let target = pop_expr(stack);
            let url = pop_expr(stack);
            let load_vars = (*flags & 0x80) != 0;
            let load_target = (*flags & 0x40) != 0;
            if load_vars {
                lines.push(format!("loadVariables({}, {});", url.code, target.code));
            } else if load_target {
                lines.push(format!("loadMovie({}, {});", url.code, target.code));
            } else if target.raw_string.as_deref() == Some("") {
                lines.push(format!("getURL({});", url.code));
            } else {
                lines.push(format!("getURL({}, {});", url.code, target.code));
            }
        }
        Avm1Op::GotoFrame2 { play, scene_bias } => {
            let frame = pop_expr(stack);
            let fn_name = if *play { "gotoAndPlay" } else { "gotoAndStop" };
            let arg = if let Some(bias) = scene_bias
                && *bias > 0
            {
                format!("{} + {bias}", frame.code)
            } else {
                frame.code
            };
            lines.push(format!("{fn_name}({arg});"));
        }
        Avm1Op::WaitForFrame { frame, skip } => {
            let skip_n = usize::from(*skip);
            let body_end = (pc + 1 + skip_n).min(end);
            let inner = decompile_range(insns, pc + 1, body_end, ctx, stack, depth + 1);
            lines.push(format!("ifFrameLoaded({}) {{", u32::from(*frame) + 1));
            for il in inner {
                lines.push(format!("    {il}"));
            }
            lines.push("}".into());
            return 1 + (body_end - (pc + 1));
        }
        Avm1Op::WaitForFrame2 { skip } => {
            let frame = pop_expr(stack);
            let skip_n = usize::from(*skip);
            let body_end = (pc + 1 + skip_n).min(end);
            let inner = decompile_range(insns, pc + 1, body_end, ctx, stack, depth + 1);
            lines.push(format!("ifFrameLoaded({}) {{", frame.code));
            for il in inner {
                lines.push(format!("    {il}"));
            }
            lines.push("}".into());
            return 1 + (body_end - (pc + 1));
        }
        Avm1Op::With { body } => {
            let obj = pop_expr(stack);
            let mut inner_stack = Vec::new();
            let inner = decompile_range(body, 0, body.len(), ctx, &mut inner_stack, depth + 1);
            lines.push(format!("with ({}) {{", obj.code));
            for il in inner {
                lines.push(format!("    {il}"));
            }
            lines.push("}".into());
        }
        Avm1Op::DefineFunction {
            name,
            params,
            preload_regs,
            body,
        } => {
            let saved_regs = ctx.reg_names.clone();
            ctx.reg_names.clear();
            for (reg, ident) in preload_regs {
                ctx.reg_names.insert(*reg, ident.clone());
            }
            let mut param_list = Vec::with_capacity(params.len());
            for (i, (reg, pname)) in params.iter().enumerate() {
                let clean = if pname.is_empty() {
                    format!("arg{i}")
                } else {
                    sanitize_ident(pname)
                };
                if *reg != 0 {
                    ctx.reg_names.insert(*reg, clean.clone());
                }
                param_list.push(clean);
            }
            let mut fn_stack = Vec::new();
            let fn_lines = decompile_range(body, 0, body.len(), ctx, &mut fn_stack, depth + 1);
            ctx.reg_names = saved_regs;
            let params_joined = param_list.join(", ");
            if name.is_empty() {
                let mut expr_str = format!("function({params_joined}) {{");
                if fn_lines.is_empty() {
                    expr_str.push('}');
                } else {
                    expr_str.push('\n');
                    for fl in fn_lines {
                        expr_str.push_str("    ");
                        expr_str.push_str(&fl);
                        expr_str.push('\n');
                    }
                    expr_str.push('}');
                }
                stack.push(Expr::atom(expr_str));
            } else {
                lines.push(format!(
                    "function {}({params_joined}) {{",
                    sanitize_ident(name)
                ));
                for fl in fn_lines {
                    lines.push(format!("    {fl}"));
                }
                lines.push("}".into());
            }
        }
        Avm1Op::Try {
            catch_var,
            catch_reg,
            try_body,
            catch_body,
            finally_body,
        } => {
            let mut t_stack = Vec::new();
            let t_lines =
                decompile_range(try_body, 0, try_body.len(), ctx, &mut t_stack, depth + 1);
            lines.push("try {".into());
            for tl in t_lines {
                lines.push(format!("    {tl}"));
            }
            if let Some(cb) = catch_body {
                let saved = ctx.reg_names.clone();
                let err_name = if let Some(v) = catch_var
                    && !v.is_empty()
                {
                    sanitize_ident(v)
                } else if let Some(r) = catch_reg {
                    let n = "err".to_owned();
                    ctx.reg_names.insert(*r, n.clone());
                    n
                } else {
                    "err".to_owned()
                };
                let mut c_stack = Vec::new();
                let c_lines = decompile_range(cb, 0, cb.len(), ctx, &mut c_stack, depth + 1);
                ctx.reg_names = saved;
                lines.push(format!("}} catch ({err_name}) {{"));
                for cl in c_lines {
                    lines.push(format!("    {cl}"));
                }
            }
            if let Some(fb) = finally_body {
                let mut f_stack = Vec::new();
                let f_lines = decompile_range(fb, 0, fb.len(), ctx, &mut f_stack, depth + 1);
                lines.push("} finally {".into());
                for fl in f_lines {
                    lines.push(format!("    {fl}"));
                }
            }
            lines.push("}".into());
        }
        Avm1Op::Simple(code) => {
            step_simple_op(*code, ctx.swf_version, stack, lines);
        }
        Avm1Op::Unknown { code, len } => {
            lines.push(format!(
                "/* unsupported AVM1 action 0x{code:02X} ({len} bytes) */"
            ));
        }
        Avm1Op::GotoFrame(_)
        | Avm1Op::GotoLabel(_)
        | Avm1Op::SetTarget(_)
        | Avm1Op::If(_)
        | Avm1Op::Jump(_) => {}
    }
    1
}

fn step_simple_op(code: u8, _swf_version: u8, stack: &mut Vec<Expr>, lines: &mut Vec<String>) {
    match code {
        0x04 => lines.push("nextFrame();".into()),
        0x05 => lines.push("prevFrame();".into()),
        0x06 => lines.push("play();".into()),
        0x07 => lines.push("stop();".into()),
        0x08 => lines.push("toggleHighQuality();".into()),
        0x09 => lines.push("stopAllSounds();".into()),
        0x0A | 0x47 => binary_op(stack, "+"),
        0x0B => binary_op(stack, "-"),
        0x0C => binary_op(stack, "*"),
        0x0D => binary_op(stack, "/"),
        0x0E | 0x13 | 0x49 => binary_op(stack, "=="),
        0x0F | 0x29 | 0x48 => binary_op(stack, "<"),
        0x10 => binary_op(stack, "&&"),
        0x11 => binary_op(stack, "||"),
        0x12 => {
            let a = pop_expr(stack);
            let mut e = Expr::atom(a.inverted_condition());
            e.negated_of = Some(a.code);
            stack.push(e);
        }
        0x14 | 0x31 => {
            let s = pop_expr(stack);
            stack.push(Expr::atom(format!("length({})", s.code)));
        }
        0x15 | 0x35 => {
            let count = pop_expr(stack);
            let index = pop_expr(stack);
            let s = pop_expr(stack);
            stack.push(Expr::atom(format!(
                "substring({}, {}, {})",
                s.code, index.code, count.code
            )));
        }
        0x17 => {
            if let Some(expr) = stack.pop()
                && looks_like_side_effect(&expr.code)
            {
                lines.push(format!("{};", expr.code));
            }
        }
        0x18 => {
            let a = pop_expr(stack);
            stack.push(Expr::atom(format!("int({})", a.code)));
        }
        0x1C => {
            let name = pop_expr(stack);
            stack.push(Expr::atom(format_variable_ref(&name)));
        }
        0x1D => {
            let val = pop_expr(stack);
            let name = pop_expr(stack);
            lines.push(format!("{} = {};", format_variable_lhs(&name), val.code));
        }
        0x20 => {
            let target = pop_expr(stack);
            if target.raw_string.as_deref() != Some("") {
                lines.push(format!("tellTarget({});", target.code));
            }
        }
        0x21 => binary_op(stack, "+"),
        0x22 => {
            let prop_idx = pop_expr(stack);
            let target = pop_expr(stack);
            let prop = avm1_property_name(&prop_idx);
            let lhs = format_target_property(&target, &prop);
            stack.push(Expr::atom(lhs));
        }
        0x23 => {
            let val = pop_expr(stack);
            let prop_idx = pop_expr(stack);
            let target = pop_expr(stack);
            let prop = avm1_property_name(&prop_idx);
            let lhs = format_target_property(&target, &prop);
            lines.push(format!("{lhs} = {};", val.code));
        }
        0x24 => {
            let depth = pop_expr(stack);
            let new_name = pop_expr(stack);
            let source = pop_expr(stack);
            lines.push(format!(
                "duplicateMovieClip({}, {}, {});",
                source.code, new_name.code, depth.code
            ));
        }
        0x25 => {
            let target = pop_expr(stack);
            lines.push(format!("removeMovieClip({});", target.code));
        }
        0x26 => {
            let val = pop_expr(stack);
            lines.push(format!("trace({});", val.code));
        }
        0x27 => {
            let target = pop_expr(stack);
            let lock = pop_expr(stack);
            let constrain = pop_expr(stack);
            let has_bounds = constrain.int_val.unwrap_or(0) != 0 || constrain.code == "true";
            if has_bounds {
                let y2 = pop_expr(stack);
                let x2 = pop_expr(stack);
                let y1 = pop_expr(stack);
                let x1 = pop_expr(stack);
                lines.push(format!(
                    "startDrag({}, {}, {}, {}, {}, {});",
                    target.code, lock.code, x1.code, y1.code, x2.code, y2.code
                ));
            } else {
                lines.push(format!("startDrag({}, {});", target.code, lock.code));
            }
        }
        0x28 => lines.push("stopDrag();".into()),
        0x2A => {
            let err = pop_expr(stack);
            lines.push(format!("throw {};", err.code));
        }
        0x2B => {
            let obj = pop_expr(stack);
            let ctor = pop_expr(stack);
            stack.push(Expr::atom(format!("{}({})", ctor.code, obj.code)));
        }
        0x2C => {
            let ctor = pop_expr(stack);
            let count = pop_arg_count(stack);
            let mut ifaces = Vec::with_capacity(count);
            for _ in 0..count {
                ifaces.push(pop_expr(stack).code);
            }
            lines.push(format!(
                "/* {} implements {} */",
                ctor.code,
                ifaces.join(", ")
            ));
        }
        0x30 => {
            let max = pop_expr(stack);
            stack.push(Expr::atom(format!("random({})", max.code)));
        }
        0x32 | 0x36 => {
            let c = pop_expr(stack);
            stack.push(Expr::atom(format!("ord({})", c.code)));
        }
        0x33 | 0x37 => {
            let c = pop_expr(stack);
            stack.push(Expr::atom(format!("chr({})", c.code)));
        }
        0x34 => stack.push(Expr::atom("getTimer()")),
        0x3A => {
            let prop = pop_expr(stack);
            let obj = pop_expr(stack);
            stack.push(Expr::atom(format!(
                "delete {}",
                format_member_access(&obj, &prop)
            )));
        }
        0x3B => {
            let name = pop_expr(stack);
            stack.push(Expr::atom(format!("delete {}", format_variable_ref(&name))));
        }
        0x3C => {
            let val = pop_expr(stack);
            let name = pop_expr(stack);
            lines.push(format!(
                "var {} = {};",
                format_variable_lhs(&name),
                val.code
            ));
        }
        0x3D => {
            let fn_name = pop_expr(stack);
            let args = pop_call_args(stack);
            let callee = format_variable_ref(&fn_name);
            stack.push(Expr::atom(format!("{callee}({})", args.join(", "))));
        }
        0x3E => {
            if let Some(val) = stack.pop() {
                if val.code == "undefined" {
                    lines.push("return;".into());
                } else {
                    lines.push(format!("return {};", val.code));
                }
            } else {
                lines.push("return;".into());
            }
        }
        0x3F => binary_op(stack, "%"),
        0x40 => {
            let class_name = pop_expr(stack);
            let args = pop_call_args(stack);
            let callee = format_variable_ref(&class_name);
            stack.push(Expr::atom(format!("new {callee}({})", args.join(", "))));
        }
        0x41 => {
            let name = pop_expr(stack);
            lines.push(format!("var {};", format_variable_lhs(&name)));
        }
        0x42 => {
            let elems = pop_call_args(stack);
            stack.push(Expr::atom(format!("[{}]", elems.join(", "))));
        }
        0x43 => {
            let count = pop_arg_count(stack);
            let mut props = Vec::with_capacity(count);
            for _ in 0..count {
                let val = pop_expr(stack);
                let key = pop_expr(stack);
                let key_str = match &key.raw_string {
                    Some(k) if is_valid_as_ident(k) => k.clone(),
                    _ => key.code,
                };
                props.push(format!("{key_str}: {}", val.code));
            }
            props.reverse();
            stack.push(Expr::atom(format!("{{{}}}", props.join(", "))));
        }
        0x44 => {
            let v = pop_expr(stack);
            stack.push(Expr::atom(format!("typeof({})", v.code)));
        }
        0x45 => {
            let v = pop_expr(stack);
            stack.push(Expr::atom(format!("targetPath({})", v.code)));
        }
        0x46 | 0x55 => {
            let obj = pop_expr(stack);
            stack.push(Expr::atom("null"));
            stack.push(Expr::atom(format!("/* enumerate {} */", obj.code)));
        }
        0x4A => {
            let v = pop_expr(stack);
            stack.push(Expr::atom(format!("Number({})", v.code)));
        }
        0x4B => {
            let v = pop_expr(stack);
            stack.push(Expr::atom(format!("String({})", v.code)));
        }
        0x4C => {
            if let Some(top) = stack.last().cloned() {
                stack.push(top);
            }
        }
        0x4D => {
            let len = stack.len();
            if len >= 2 {
                stack.swap(len - 1, len - 2);
            }
        }
        0x4E => {
            let prop = pop_expr(stack);
            let obj = pop_expr(stack);
            stack.push(Expr::atom(format_member_access(&obj, &prop)));
        }
        0x4F => {
            let val = pop_expr(stack);
            let prop = pop_expr(stack);
            let obj = pop_expr(stack);
            lines.push(format!(
                "{} = {};",
                format_member_access(&obj, &prop),
                val.code
            ));
        }
        0x50 => {
            let v = pop_expr(stack);
            stack.push(Expr::atom(format!("({} + 1)", v.code)));
        }
        0x51 => {
            let v = pop_expr(stack);
            stack.push(Expr::atom(format!("({} - 1)", v.code)));
        }
        0x52 => {
            let method = pop_expr(stack);
            let obj = pop_expr(stack);
            let args = pop_call_args(stack);
            let call = if method.raw_string.as_deref().is_some_and(str::is_empty)
                || method.code == "undefined"
            {
                format!("{}({})", obj.code, args.join(", "))
            } else {
                format!(
                    "{}({})",
                    format_member_access(&obj, &method),
                    args.join(", ")
                )
            };
            stack.push(Expr::atom(call));
        }
        0x53 => {
            let method = pop_expr(stack);
            let obj = pop_expr(stack);
            let args = pop_call_args(stack);
            let call = if method.raw_string.as_deref().is_some_and(str::is_empty)
                || method.code == "undefined"
            {
                format!("new {}({})", obj.code, args.join(", "))
            } else {
                format!(
                    "new {}({})",
                    format_member_access(&obj, &method),
                    args.join(", ")
                )
            };
            stack.push(Expr::atom(call));
        }
        0x54 => binary_op(stack, "instanceof"),
        0x60 => binary_op(stack, "&"),
        0x61 => binary_op(stack, "|"),
        0x62 => binary_op(stack, "^"),
        0x63 => binary_op(stack, "<<"),
        0x64 => binary_op(stack, ">>"),
        0x65 => binary_op(stack, ">>>"),
        0x66 => binary_op(stack, "==="),
        0x67 | 0x68 => binary_op(stack, ">"),
        0x69 => {
            let sup = pop_expr(stack);
            let sub = pop_expr(stack);
            lines.push(format!("/* {} extends {} */", sub.code, sup.code));
        }
        0x9E => {
            let frame = pop_expr(stack);
            lines.push(format!("call({});", frame.code));
        }
        _ => {}
    }
}

fn binary_op(stack: &mut Vec<Expr>, op: &str) {
    let rhs = pop_expr(stack);
    let lhs = pop_expr(stack);
    stack.push(Expr::atom(format!("({} {op} {})", lhs.code, rhs.code)));
}

fn pop_arg_count(stack: &mut Vec<Expr>) -> usize {
    let n = pop_expr(stack);
    if let Some(v) = n.int_val {
        (v.max(0) as usize).min(256)
    } else {
        n.code.parse::<usize>().unwrap_or(0).min(256)
    }
}

fn pop_call_args(stack: &mut Vec<Expr>) -> Vec<String> {
    let count = pop_arg_count(stack);
    let mut args = Vec::with_capacity(count);
    for _ in 0..count {
        args.push(pop_expr(stack).code);
    }
    args
}

fn avm1_property_name(idx_expr: &Expr) -> String {
    let idx = idx_expr
        .int_val
        .or_else(|| idx_expr.code.parse::<i64>().ok())
        .unwrap_or(-1);
    match idx {
        0 => "_x".into(),
        1 => "_y".into(),
        2 => "_xscale".into(),
        3 => "_yscale".into(),
        4 => "_currentframe".into(),
        5 => "_totalframes".into(),
        6 => "_alpha".into(),
        7 => "_visible".into(),
        8 => "_width".into(),
        9 => "_height".into(),
        10 => "_rotation".into(),
        11 => "_target".into(),
        12 => "_framesloaded".into(),
        13 => "_name".into(),
        14 => "_droptarget".into(),
        15 => "_url".into(),
        16 => "_highquality".into(),
        17 => "_focusrect".into(),
        18 => "_soundbuftime".into(),
        19 => "_quality".into(),
        20 => "_xmouse".into(),
        21 => "_ymouse".into(),
        _ => format!("/*prop {}*/", idx_expr.code),
    }
}

fn format_target_property(target: &Expr, prop: &str) -> String {
    match target.raw_string.as_deref() {
        Some("") => prop.to_owned(),
        Some(t) if is_valid_dotted_path(t) => format!("{t}.{prop}"),
        _ if target.code == "this" => format!("this.{prop}"),
        _ => format!("eval({}).{prop}", target.code),
    }
}

fn format_variable_ref(name: &Expr) -> String {
    if let Some(raw) = &name.raw_string {
        if is_valid_dotted_path(raw) {
            return raw.clone();
        }
        return format!("this[{}]", quote_as_string(raw));
    }
    format!("eval({})", name.code)
}

fn format_variable_lhs(name: &Expr) -> String {
    if let Some(raw) = &name.raw_string {
        if is_valid_dotted_path(raw) {
            return raw.clone();
        }
        return format!("this[{}]", quote_as_string(raw));
    }
    format!("this[{}]", name.code)
}

fn format_member_access(obj: &Expr, prop: &Expr) -> String {
    if let Some(raw) = &prop.raw_string
        && is_valid_as_ident(raw)
    {
        return format!("{}.{raw}", obj.code);
    }
    format!("{}[{}]", obj.code, prop.code)
}

fn looks_like_side_effect(code: &str) -> bool {
    code.ends_with(')')
        || code.starts_with("delete ")
        || code.starts_with("new ")
        || code.contains(" = ")
}

fn is_valid_as_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn is_valid_dotted_path(s: &str) -> bool {
    !s.is_empty() && s.split('.').all(is_valid_as_ident)
}

fn sanitize_ident(s: &str) -> String {
    if is_valid_as_ident(s) {
        return s.to_owned();
    }
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
            if i == 0 && c.is_ascii_digit() {
                out.push('_');
            }
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() { "_fn".into() } else { out }
}

pub(crate) fn quote_as_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(crate) fn format_number(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v == f64::INFINITY {
        "Infinity".into()
    } else if v == f64::NEG_INFINITY {
        "-Infinity".into()
    } else if v == 0.0 {
        "0".into()
    } else if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{:.0}", v)
    } else {
        v.to_string()
    }
}

fn join_lines(lines: &[String], indent: usize) -> String {
    let pad = "    ".repeat(indent);
    lines
        .iter()
        .map(|l| format!("{pad}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}
