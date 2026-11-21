//! Bounded AVM1 (`DoAction` / `DoInitAction`) and AVM2 (`DoABC` / `DoABC2`) bytecode-to-JS compilers.

use super::{SwfDecodeCtx, attach_compiled_frame_script};
use neoflash_core::Timeline;
use std::collections::{HashMap, HashSet};

pub(super) fn js_string_literal(s: &str) -> String {
    serde_json::to_string(s)
        .unwrap_or_else(|_| "\"\"".into())
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn avm1_unsupported_warning(op: u8) -> String {
    match op {
        0x83 => {
            "SWF AVM1 opcode GetURL (0x83) is not supported in the bounded SWF loader and was skipped."
                .into()
        }
        0x9A => {
            "SWF AVM1 opcode GetURL2 (0x9A) is not supported in the bounded SWF loader and was skipped."
                .into()
        }
        0x9B => {
            "SWF AVM1 opcode DefineFunction (0x9B) is not supported in the bounded SWF loader and was skipped."
                .into()
        }
        0x8E => {
            "SWF AVM1 opcode DefineFunction2 (0x8E) is not supported in the bounded SWF loader and was skipped."
                .into()
        }
        0x94 => {
            "SWF AVM1 opcode With (0x94) is not supported in the bounded SWF loader and was skipped."
                .into()
        }
        0x8F => {
            "SWF AVM1 opcode Try (0x8F) is not supported in the bounded SWF loader and was skipped."
                .into()
        }
        other => format!(
            "SWF AVM1 opcode 0x{other:02X} is not supported in the bounded SWF loader and was skipped."
        ),
    }
}

// ============================================================================
// Bounded AVM1 (`DoAction` / `DoInitAction`) Bytecode-to-JS Compiler
// ============================================================================

#[derive(Debug)]
struct Avm1Inst {
    offset: usize,
    op: u8,
    payload: Vec<u8>,
}

pub(super) fn compile_avm1_actions(ctx: &mut SwfDecodeCtx, bytes: &[u8]) -> Option<String> {
    let mut insts = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() && insts.len() < 4096 {
        let offset = pos;
        let op = bytes[pos];
        pos += 1;
        if op == 0x00 {
            break;
        }
        let payload = if op >= 0x80 {
            if pos + 2 > bytes.len() {
                break;
            }
            let len = usize::from(u16::from_le_bytes([bytes[pos], bytes[pos + 1]]));
            pos += 2;
            let end = (pos + len).min(bytes.len());
            let slice = bytes[pos..end].to_vec();
            pos = end;
            if matches!(op, 0x9B | 0x8E) && slice.len() >= 2 {
                let code_size = usize::from(u16::from_le_bytes([
                    slice[slice.len() - 2],
                    slice[slice.len() - 1],
                ]));
                pos = pos.saturating_add(code_size).min(bytes.len());
            }
            slice
        } else {
            Vec::new()
        };
        insts.push(Avm1Inst {
            offset,
            op,
            payload,
        });
    }
    if insts.is_empty() {
        return None;
    }

    let mut offset_to_idx = HashMap::new();
    for (idx, inst) in insts.iter().enumerate() {
        offset_to_idx.insert(inst.offset, idx);
    }
    let end_offset = pos;
    offset_to_idx.insert(end_offset, insts.len());

    let mut js = String::from(
        "const __st = [], __regs = Object.create(null), __locals = Object.create(null);\nlet __cp = [], __pc = 0, __steps = 0;\nconst __propNames = ['x','y','scaleX','scaleY','currentFrame','totalFrames','alpha','visible','width','height','rotation'];\nconst __isSafeLocal = k => k !== '__proto__' && k !== 'constructor' && k !== 'prototype';\nconst __readVar = k => (__isSafeLocal(k) && Object.hasOwn(__locals, k) ? __locals[k] : k === '_root' ? _root : k === '_parent' ? _parent : k === 'this' ? this : __nfSwfGet(this, k));\nconst __writeVar = (k, v) => { if (__isSafeLocal(k) && Object.hasOwn(__locals, k)) __locals[k] = v; else __nfSwfSet(this, k, v); };\nwhile (__pc < ",
    );
    js.push_str(&insts.len().to_string());
    js.push_str(") {\n  if (++__steps > 10000) break;\n  switch (__pc) {\n");

    for (idx, inst) in insts.iter().enumerate() {
        js.push_str(&format!("    case {idx}: {{\n"));
        let next_idx = idx + 1;
        let p = &inst.payload;
        match inst.op {
            0x04 => js.push_str("      if (typeof nextFrame === 'function') nextFrame();\n"),
            0x05 => js.push_str("      if (typeof prevFrame === 'function') prevFrame();\n"),
            0x06 => js.push_str("      if (typeof play === 'function') play();\n"),
            0x07 => js.push_str("      if (typeof stop === 'function') stop();\n"),
            0x81 if p.len() >= 2 => {
                let frame0 = u32::from(u16::from_le_bytes([p[0], p[1]]));
                let next_is_play = insts.get(idx + 1).is_some_and(|n| n.op == 0x06);
                if next_is_play {
                    js.push_str(&format!(
                        "      if (typeof gotoAndPlay === 'function') gotoAndPlay({});\n",
                        frame0 + 1
                    ));
                } else {
                    js.push_str(&format!(
                        "      if (typeof gotoAndStop === 'function') gotoAndStop({});\n",
                        frame0 + 1
                    ));
                }
            }
            0x8C => {
                let label = js_string_literal(&cstr_from_bytes(p));
                js.push_str(&format!(
                    "      if (typeof gotoAndStop === 'function') gotoAndStop({label});\n"
                ));
            }
            0x9F if !p.is_empty() => {
                let play_flag = (p[0] & 0x01) != 0;
                if play_flag {
                    js.push_str("      { const __f = __st.pop(); if (typeof gotoAndPlay === 'function') gotoAndPlay(__f); }\n");
                } else {
                    js.push_str("      { const __f = __st.pop(); if (typeof gotoAndStop === 'function') gotoAndStop(__f); }\n");
                }
            }
            0x88 if p.len() >= 2 => {
                let count = usize::from(u16::from_le_bytes([p[0], p[1]]));
                let mut off = 2usize;
                let mut strs = Vec::new();
                for _ in 0..count.min(2048) {
                    if off >= p.len() {
                        break;
                    }
                    let start = off;
                    while off < p.len() && p[off] != 0 {
                        off += 1;
                    }
                    strs.push(String::from_utf8_lossy(&p[start..off]).into_owned());
                    off += 1;
                }
                let arr_lit = strs
                    .iter()
                    .map(|s| js_string_literal(s))
                    .collect::<Vec<_>>()
                    .join(",");
                js.push_str(&format!("      __cp = [{arr_lit}];\n"));
            }
            0x96 => {
                let mut off = 0usize;
                while off < p.len() {
                    let tp = p[off];
                    off += 1;
                    match tp {
                        0 => {
                            let start = off;
                            while off < p.len() && p[off] != 0 {
                                off += 1;
                            }
                            let s = js_string_literal(&String::from_utf8_lossy(&p[start..off]));
                            off = (off + 1).min(p.len());
                            js.push_str(&format!("      __st.push({s});\n"));
                        }
                        1 if off + 4 <= p.len() => {
                            let v = f32::from_le_bytes([p[off], p[off + 1], p[off + 2], p[off + 3]]);
                            off += 4;
                            js.push_str(&format!("      __st.push({});\n", format_js_f64(f64::from(v))));
                        }
                        2 => js.push_str("      __st.push(null);\n"),
                        3 => js.push_str("      __st.push(undefined);\n"),
                        4 if off < p.len() => {
                            let reg = p[off];
                            off += 1;
                            js.push_str(&format!("      __st.push(__regs[{reg}]);\n"));
                        }
                        5 if off < p.len() => {
                            let b = p[off] != 0;
                            off += 1;
                            js.push_str(&format!("      __st.push({b});\n"));
                        }
                        6 if off + 8 <= p.len() => {
                            // SWF double swaps the two 32-bit words
                            let v = f64::from_le_bytes([
                                p[off + 4],
                                p[off + 5],
                                p[off + 6],
                                p[off + 7],
                                p[off],
                                p[off + 1],
                                p[off + 2],
                                p[off + 3],
                            ]);
                            off += 8;
                            js.push_str(&format!("      __st.push({});\n", format_js_f64(v)));
                        }
                        7 if off + 4 <= p.len() => {
                            let v = i32::from_le_bytes([p[off], p[off + 1], p[off + 2], p[off + 3]]);
                            off += 4;
                            js.push_str(&format!("      __st.push({v});\n"));
                        }
                        8 if off < p.len() => {
                            let idx8 = p[off];
                            off += 1;
                            js.push_str(&format!("      __st.push(__cp[{idx8}]);\n"));
                        }
                        9 if off + 2 <= p.len() => {
                            let idx16 = u16::from_le_bytes([p[off], p[off + 1]]);
                            off += 2;
                            js.push_str(&format!("      __st.push(__cp[{idx16}]);\n"));
                        }
                        _ => break,
                    }
                }
            }
            0x17 => js.push_str("      __st.pop();\n"),
            0x26 => js.push_str("      trace(__st.pop());\n"),
            0x1C => js.push_str("      { const __k = String(__st.pop() ?? ''); __st.push(__readVar(__k)); }\n"),
            0x1D => js.push_str("      { const __v = __st.pop(), __k = String(__st.pop() ?? ''); __writeVar(__k, __v); }\n"),
            0x3C => js.push_str("      { const __v = __st.pop(), __k = String(__st.pop() ?? ''); if (__isSafeLocal(__k)) __locals[__k] = __v; }\n"),
            0x41 => js.push_str("      { const __k = String(__st.pop() ?? ''); if (__isSafeLocal(__k) && !Object.hasOwn(__locals, __k)) __locals[__k] = undefined; }\n"),
            0x4E => js.push_str("      { const __m = String(__st.pop() ?? ''), __o = __st.pop(); __st.push(__nfSwfGet(__o, __m)); }\n"),
            0x4F => js.push_str("      { const __v = __st.pop(), __m = String(__st.pop() ?? ''), __o = __st.pop(); __nfSwfSet(__o, __m, __v); }\n"),
            0x22 => js.push_str("      { const __pi = Number(__st.pop()) | 0, __t = __st.pop() || this, __pn = __propNames[__pi]; __st.push(__pn ? (__pi === 2 || __pi === 3 || __pi === 6 ? __nfSwfGet(__t, __pn) * 100 : __nfSwfGet(__t, __pn)) : undefined); }\n"),
            0x23 => js.push_str("      { const __v = __st.pop(), __pi = Number(__st.pop()) | 0, __t = __st.pop() || this, __pn = __propNames[__pi]; if (__pn) __nfSwfSet(__t, __pn, (__pi === 2 || __pi === 3 || __pi === 6) ? Number(__v) / 100 : __v); }\n"),
            0x0A => js.push_str("      { const __b = Number(__st.pop()) || 0, __a = Number(__st.pop()) || 0; __st.push(__a + __b); }\n"),
            0x47 => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(typeof __a === 'string' || typeof __b === 'string' ? String(__a ?? '') + String(__b ?? '') : (Number(__a) || 0) + (Number(__b) || 0)); }\n"),
            0x0B => js.push_str("      { const __b = Number(__st.pop()) || 0, __a = Number(__st.pop()) || 0; __st.push(__a - __b); }\n"),
            0x0C => js.push_str("      { const __b = Number(__st.pop()) || 0, __a = Number(__st.pop()) || 0; __st.push(__a * __b); }\n"),
            0x0D => js.push_str("      { const __b = Number(__st.pop()), __a = Number(__st.pop()); __st.push(__a / __b); }\n"),
            0x3F => js.push_str("      { const __b = Number(__st.pop()), __a = Number(__st.pop()); __st.push(__a % __b); }\n"),
            0x0E | 0x49 => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a == __b); }\n"),
            0x66 => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a === __b); }\n"),
            0x0F | 0x48 => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a < __b); }\n"),
            0x67 => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a > __b); }\n"),
            0x12 => js.push_str("      __st.push(!__st.pop());\n"),
            0x10 => js.push_str("      { const __b = Boolean(__st.pop()), __a = Boolean(__st.pop()); __st.push(__a && __b); }\n"),
            0x11 => js.push_str("      { const __b = Boolean(__st.pop()), __a = Boolean(__st.pop()); __st.push(__a || __b); }\n"),
            0x50 => js.push_str("      __st.push((Number(__st.pop()) || 0) + 1);\n"),
            0x51 => js.push_str("      __st.push((Number(__st.pop()) || 0) - 1);\n"),
            0x18 => js.push_str("      __st.push(Number(__st.pop()) | 0);\n"),
            0x4A => js.push_str("      __st.push(Number(__st.pop()));\n"),
            0x4B => js.push_str("      __st.push(String(__st.pop() ?? ''));\n"),
            0x4C => js.push_str("      __st.push(__st[__st.length - 1]);\n"),
            0x4D => js.push_str("      if (__st.length >= 2) { const __t = __st[__st.length - 1]; __st[__st.length - 1] = __st[__st.length - 2]; __st[__st.length - 2] = __t; }\n"),
            0x87 if !p.is_empty() => {
                let reg = p[0];
                js.push_str(&format!("      __regs[{reg}] = __st[__st.length - 1];\n"));
            }
            0x42 => js.push_str("      { const __n = Math.max(0, Number(__st.pop()) | 0), __arr = []; for (let __i = 0; __i < __n; __i++) __arr.push(__st.pop()); __st.push(__arr); }\n"),
            0x43 => js.push_str("      { const __n = Math.max(0, Number(__st.pop()) | 0), __obj = {}; for (let __i = 0; __i < __n; __i++) { const __v = __st.pop(), __k = String(__st.pop() ?? ''); __nfSwfSet(__obj, __k, __v); } __st.push(__obj); }\n"),
            0x3D => js.push_str("      { const __fnName = String(__st.pop() ?? ''), __argc = Math.max(0, Number(__st.pop()) | 0), __args = []; for (let __i = 0; __i < __argc; __i++) __args.push(__st.pop()); const __fn = __fnName === 'stop' ? stop : __fnName === 'play' ? play : __fnName === 'gotoAndStop' ? gotoAndStop : __fnName === 'gotoAndPlay' ? gotoAndPlay : __fnName === 'nextFrame' ? nextFrame : __fnName === 'prevFrame' ? prevFrame : __fnName === 'trace' ? trace : __readVar(__fnName); __st.push(__nfSwfApply(__fn, this, __args)); }\n"),
            0x52 => js.push_str("      { const __mName = String(__st.pop() ?? ''), __obj = __st.pop() ?? this, __argc = Math.max(0, Number(__st.pop()) | 0), __args = []; for (let __i = 0; __i < __argc; __i++) __args.push(__st.pop()); __st.push(__nfSwfCall(__obj, __mName, __args)); }\n"),
            0x99 if p.len() >= 2 => {
                let rel = i16::from_le_bytes([p[0], p[1]]) as isize;
                let base = (inst.offset + 3 + p.len()) as isize;
                let target_off = (base + rel).max(0) as usize;
                let target_pc = offset_to_idx.get(&target_off).copied().unwrap_or(insts.len());
                js.push_str(&format!("      __pc = {target_pc}; continue;\n"));
            }
            0x9D if p.len() >= 2 => {
                let rel = i16::from_le_bytes([p[0], p[1]]) as isize;
                let base = (inst.offset + 3 + p.len()) as isize;
                let target_off = (base + rel).max(0) as usize;
                let target_pc = offset_to_idx.get(&target_off).copied().unwrap_or(insts.len());
                js.push_str(&format!(
                    "      if (__st.pop()) {{ __pc = {target_pc}; continue; }}\n"
                ));
            }
            0x3E => js.push_str("      return __st.pop();\n"),
            other => {
                ctx.warn(avm1_unsupported_warning(other));
            }
        }
        js.push_str(&format!("      __pc = {next_idx}; break;\n    }}\n"));
    }
    js.push_str("    default: __pc = ");
    js.push_str(&insts.len().to_string());
    js.push_str("; break;\n  }\n}\n");
    Some(format!("(() => {{\n{js}}})();\n"))
}

fn cstr_from_bytes(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn format_js_f64(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v == f64::INFINITY {
        "Infinity".into()
    } else if v == f64::NEG_INFINITY {
        "-Infinity".into()
    } else if v == 0.0 && v.is_sign_negative() {
        "-0".into()
    } else {
        v.to_string()
    }
}

// ============================================================================
// Bounded AVM2 (`DoABC` / `DoABC2`) Parser & Frame Action / Document Class Compiler
// ============================================================================

#[derive(Clone, Debug)]
struct AbcMultiname {
    name: String,
    runtime_name: bool,
}

#[derive(Clone, Debug)]
struct AbcInstanceInfo {
    name: String,
    iinit: u32,
}

#[derive(Clone, Debug)]
pub(super) struct AbcFile {
    ints: Vec<i32>,
    uints: Vec<u32>,
    doubles: Vec<f64>,
    strings: Vec<String>,
    multinames: Vec<AbcMultiname>,
    instances: Vec<AbcInstanceInfo>,
    script_inits: Vec<u32>,
    bodies: HashMap<u32, Vec<u8>>,
}

struct AbcReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> AbcReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        let b = *self.data.get(self.pos).ok_or("Truncated ABC")?;
        self.pos += 1;
        Ok(b)
    }

    fn read_u16(&mut self) -> Result<u16, String> {
        let lo = u16::from(self.read_u8()?);
        let hi = u16::from(self.read_u8()?);
        Ok(lo | (hi << 8))
    }

    fn read_s24(&mut self) -> Result<i32, String> {
        let b0 = i32::from(self.read_u8()?);
        let b1 = i32::from(self.read_u8()?);
        let b2 = i32::from(self.read_u8()? as i8);
        Ok(b0 | (b1 << 8) | (b2 << 16))
    }

    fn read_u30(&mut self) -> Result<u32, String> {
        let mut result = 0u32;
        for shift in (0..35).step_by(7) {
            let b = self.read_u8()?;
            result |= u32::from(b & 0x7F) << shift;
            if (b & 0x80) == 0 {
                return Ok(result);
            }
        }
        Ok(result)
    }

    fn read_s32(&mut self) -> Result<i32, String> {
        let mut result = 0u32;
        let mut shift = 0u32;
        for _ in 0..5 {
            let b = self.read_u8()?;
            result |= u32::from(b & 0x7F) << shift;
            shift += 7;
            if (b & 0x80) == 0 {
                if shift < 32 && (b & 0x40) != 0 {
                    result |= !0u32 << shift;
                }
                return Ok(result as i32);
            }
        }
        Ok(result as i32)
    }

    fn read_f64(&mut self) -> Result<f64, String> {
        if self.pos + 8 > self.data.len() {
            return Err("Truncated ABC f64".into());
        }
        let mut arr = [0u8; 8];
        arr.copy_from_slice(&self.data[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(f64::from_le_bytes(arr))
    }

    fn skip_traits(&mut self) -> Result<(), String> {
        let count = self.read_u30()? as usize;
        for _ in 0..count.min(4096) {
            let _name = self.read_u30()?;
            let kind_byte = self.read_u8()?;
            let kind = kind_byte & 0x0F;
            let attrs = kind_byte >> 4;
            match kind {
                0 | 6 => {
                    let _slot_id = self.read_u30()?;
                    let _type_name = self.read_u30()?;
                    let vindex = self.read_u30()?;
                    if vindex != 0 {
                        let _vkind = self.read_u8()?;
                    }
                }
                1 | 2 | 3 | 5 => {
                    let _disp_id = self.read_u30()?;
                    let _method = self.read_u30()?;
                }
                4 => {
                    let _slot_id = self.read_u30()?;
                    let _classi = self.read_u30()?;
                }
                _ => return Err("Unknown ABC trait kind".into()),
            }
            if (attrs & 0x04) != 0 {
                let meta_count = self.read_u30()? as usize;
                for _ in 0..meta_count {
                    let _ = self.read_u30()?;
                }
            }
        }
        Ok(())
    }
}

pub(super) fn parse_abc_tag(data: &[u8], is_do_abc2: bool) -> Result<AbcFile, String> {
    let mut start = 0usize;
    if is_do_abc2 {
        if data.len() < 5 {
            return Err("Truncated DoABC2".into());
        }
        start = 4;
        while start < data.len() && data[start] != 0 {
            start += 1;
        }
        start = (start + 1).min(data.len());
    }
    let abc_bytes = &data[start..];
    if abc_bytes.len() < 4 {
        return Err("Truncated ABC header".into());
    }
    let mut r = AbcReader::new(abc_bytes);
    let _minor = r.read_u16()?;
    let _major = r.read_u16()?;

    let int_count = r.read_u30()? as usize;
    let mut ints = vec![0i32];
    for _ in 1..int_count.min(16384) {
        ints.push(r.read_s32()?);
    }

    let uint_count = r.read_u30()? as usize;
    let mut uints = vec![0u32];
    for _ in 1..uint_count.min(16384) {
        uints.push(r.read_u30()?);
    }

    let dbl_count = r.read_u30()? as usize;
    let mut doubles = vec![0.0f64];
    for _ in 1..dbl_count.min(16384) {
        doubles.push(r.read_f64()?);
    }

    let str_count = r.read_u30()? as usize;
    let mut strings = vec![String::new()];
    for _ in 1..str_count.min(32768) {
        let len = r.read_u30()? as usize;
        if r.pos + len > r.data.len() {
            return Err("Truncated ABC string".into());
        }
        strings.push(String::from_utf8_lossy(&r.data[r.pos..r.pos + len]).into_owned());
        r.pos += len;
    }

    let ns_count = r.read_u30()? as usize;
    let mut ns_names = vec![String::new()];
    for _ in 1..ns_count.min(16384) {
        let _kind = r.read_u8()?;
        let name_idx = r.read_u30()? as usize;
        ns_names.push(strings.get(name_idx).cloned().unwrap_or_default());
    }

    let nsset_count = r.read_u30()? as usize;
    for _ in 1..nsset_count.min(16384) {
        let cnt = r.read_u30()? as usize;
        for _ in 0..cnt {
            let _ = r.read_u30()?;
        }
    }

    let mn_count = r.read_u30()? as usize;
    let mut multinames = vec![AbcMultiname {
        name: String::new(),
        runtime_name: false,
    }];
    for _ in 1..mn_count.min(32768) {
        let kind = r.read_u8()?;
        match kind {
            0x07 | 0x0D => {
                let ns_idx = r.read_u30()? as usize;
                let name_idx = r.read_u30()? as usize;
                let local = strings.get(name_idx).cloned().unwrap_or_default();
                let pkg = ns_names.get(ns_idx).cloned().unwrap_or_default();
                let full = if pkg.is_empty() {
                    local
                } else {
                    format!("{pkg}.{local}")
                };
                multinames.push(AbcMultiname {
                    name: full,
                    runtime_name: false,
                });
            }
            0x0F | 0x10 => {
                let name_idx = r.read_u30()? as usize;
                multinames.push(AbcMultiname {
                    name: strings.get(name_idx).cloned().unwrap_or_default(),
                    runtime_name: false,
                });
            }
            0x11 | 0x12 => {
                multinames.push(AbcMultiname {
                    name: String::new(),
                    runtime_name: true,
                });
            }
            0x09 | 0x0E => {
                let name_idx = r.read_u30()? as usize;
                let _ns_set = r.read_u30()?;
                multinames.push(AbcMultiname {
                    name: strings.get(name_idx).cloned().unwrap_or_default(),
                    runtime_name: false,
                });
            }
            0x1B | 0x1C => {
                let _ns_set = r.read_u30()?;
                multinames.push(AbcMultiname {
                    name: String::new(),
                    runtime_name: true,
                });
            }
            0x1D => {
                let base_idx = r.read_u30()? as usize;
                let pcnt = r.read_u30()? as usize;
                for _ in 0..pcnt {
                    let _ = r.read_u30()?;
                }
                let base_name = multinames
                    .get(base_idx)
                    .map_or_else(String::new, |m| m.name.clone());
                multinames.push(AbcMultiname {
                    name: base_name,
                    runtime_name: false,
                });
            }
            _ => return Err("Unknown ABC multiname kind".into()),
        }
    }

    let method_count = r.read_u30()? as usize;
    for _ in 0..method_count.min(32768) {
        let param_count = r.read_u30()? as usize;
        let _ret_type = r.read_u30()?;
        for _ in 0..param_count {
            let _ = r.read_u30()?;
        }
        let _name = r.read_u30()?;
        let flags = r.read_u8()?;
        if (flags & 0x08) != 0 {
            let opt_count = r.read_u30()? as usize;
            for _ in 0..opt_count {
                let _val = r.read_u30()?;
                let _kind = r.read_u8()?;
            }
        }
        if (flags & 0x80) != 0 {
            for _ in 0..param_count {
                let _ = r.read_u30()?;
            }
        }
    }

    let meta_count = r.read_u30()? as usize;
    for _ in 0..meta_count.min(16384) {
        let _name = r.read_u30()?;
        let icnt = r.read_u30()? as usize;
        for _ in 0..icnt {
            let _k = r.read_u30()?;
            let _v = r.read_u30()?;
        }
    }

    let class_count = r.read_u30()? as usize;
    let mut instances = Vec::with_capacity(class_count.min(4096));
    for _ in 0..class_count.min(4096) {
        let name_mn = r.read_u30()? as usize;
        let _super_mn = r.read_u30()?;
        let flags = r.read_u8()?;
        if (flags & 0x08) != 0 {
            let _protected_ns = r.read_u30()?;
        }
        let intrf_count = r.read_u30()? as usize;
        for _ in 0..intrf_count {
            let _ = r.read_u30()?;
        }
        let iinit = r.read_u30()?;
        r.skip_traits()?;
        let cls_name = multinames
            .get(name_mn)
            .map_or_else(String::new, |m| m.name.clone());
        instances.push(AbcInstanceInfo {
            name: cls_name,
            iinit,
        });
    }
    for _ in 0..class_count.min(4096) {
        let _cinit = r.read_u30()?;
        r.skip_traits()?;
    }

    let script_count = r.read_u30()? as usize;
    let mut script_inits = Vec::with_capacity(script_count.min(4096));
    for _ in 0..script_count.min(4096) {
        let init = r.read_u30()?;
        r.skip_traits()?;
        script_inits.push(init);
    }

    let body_count = r.read_u30()? as usize;
    let mut bodies = HashMap::new();
    for _ in 0..body_count.min(32768) {
        let method_idx = r.read_u30()?;
        let _max_stack = r.read_u30()?;
        let _local_count = r.read_u30()?;
        let _init_scope = r.read_u30()?;
        let _max_scope = r.read_u30()?;
        let code_len = r.read_u30()? as usize;
        if r.pos + code_len > r.data.len() {
            break;
        }
        let code = r.data[r.pos..r.pos + code_len].to_vec();
        r.pos += code_len;
        let exc_count = r.read_u30()? as usize;
        for _ in 0..exc_count {
            let _from = r.read_u30()?;
            let _to = r.read_u30()?;
            let _target = r.read_u30()?;
            let _exc_type = r.read_u30()?;
            let _var_name = r.read_u30()?;
        }
        r.skip_traits()?;
        bodies.insert(method_idx, code);
    }

    Ok(AbcFile {
        ints,
        uints,
        doubles,
        strings,
        multinames,
        instances,
        script_inits,
        bodies,
    })
}

fn short_multiname(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

pub(super) fn apply_abc_scripts(ctx: &mut SwfDecodeCtx, root_timeline: &mut Timeline) {
    let doc_class = ctx.symbol_classes.get(&0).cloned();
    let abc_list = ctx.abc_blocks.clone();
    for abc in &abc_list {
        let chosen_inst = if let Some(dc) = &doc_class {
            abc.instances
                .iter()
                .find(|i| &i.name == dc || short_multiname(&i.name) == short_multiname(dc))
        } else {
            abc.instances.first()
        };

        if let Some(inst) = chosen_inst
            && let Some(iinit_code) = abc.bodies.get(&inst.iinit)
        {
            let (frame_hooks, hook_fn_indices) = extract_avm2_add_frame_scripts(abc, iinit_code);
            if let Some(ctor_js) =
                compile_avm2_method_body(ctx, abc, iinit_code, true, &hook_fn_indices)
            {
                attach_compiled_frame_script(ctx, root_timeline, 0, &ctor_js, "avm2-doc-ctor");
            }
            let empty_hooks = HashSet::new();
            for (frame_idx, method_id) in frame_hooks {
                if frame_idx < root_timeline.frame_count
                    && let Some(m_code) = abc.bodies.get(&method_id)
                    && let Some(f_js) =
                        compile_avm2_method_body(ctx, abc, m_code, false, &empty_hooks)
                {
                    attach_compiled_frame_script(
                        ctx,
                        root_timeline,
                        frame_idx,
                        &f_js,
                        "avm2-framescript",
                    );
                }
            }
        } else if let Some(&script_init) = abc.script_inits.last()
            && let Some(s_code) = abc.bodies.get(&script_init)
        {
            let empty_hooks = HashSet::new();
            if let Some(s_js) = compile_avm2_method_body(ctx, abc, s_code, false, &empty_hooks) {
                attach_compiled_frame_script(ctx, root_timeline, 0, &s_js, "avm2-script");
            }
        }
    }
}

#[derive(Debug)]
struct Avm2Inst {
    offset: usize,
    op: u8,
    args: Vec<i64>,
}

fn decode_avm2_instructions(code: &[u8]) -> Vec<Avm2Inst> {
    let mut r = AbcReader::new(code);
    let mut out = Vec::new();
    while r.pos < r.data.len() && out.len() < 4096 {
        let offset = r.pos;
        let Ok(op) = r.read_u8() else {
            break;
        };
        let args_res: Result<Vec<i64>, String> = (|| match op {
            // 1 u30
            0x04 | 0x05 | 0x08 | 0x2C | 0x2D | 0x2E | 0x2F | 0x31 | 0x40 | 0x42 | 0x49 | 0x55
            | 0x56 | 0x58 | 0x59 | 0x5A | 0x5D | 0x5E | 0x60 | 0x61 | 0x62 | 0x63 | 0x65 | 0x66
            | 0x68 | 0x6C | 0x6D | 0x78 | 0x80 | 0x86 | 0x92 | 0x94 | 0xB2 | 0xB4 | 0xC2 | 0xC3
            | 0xF0 | 0xF1 => Ok(vec![i64::from(r.read_u30()?)]),
            // 1 u8
            0x24 | 0xF2 => Ok(vec![i64::from(r.read_u8()?)]),
            // 1 s32 (pushshort)
            0x25 => Ok(vec![i64::from(r.read_s32()?)]),
            // 2 u30 (callproperty, callpropvoid, callproplex, constructprop, callmethod, callstatic, callsuper, callsupervoid)
            0x43 | 0x44 | 0x45 | 0x46 | 0x4A | 0x4C | 0x4E | 0x4F => {
                Ok(vec![i64::from(r.read_u30()?), i64::from(r.read_u30()?)])
            }
            // call (1 u30 argc)
            0x41 => Ok(vec![i64::from(r.read_u30()?)]),
            // s24 branch
            0x0C..=0x19 => Ok(vec![i64::from(r.read_s24()?)]),
            // lookupswitch
            0x1B => {
                let def_off = i64::from(r.read_s24()?);
                let cnt = r.read_u30()? as usize;
                let mut v = vec![def_off];
                for _ in 0..=cnt.min(1024) {
                    v.push(i64::from(r.read_s24()?));
                }
                Ok(v)
            }
            // debug: 1 u8, 1 u30, 1 u8, 1 u30
            0xEF => {
                let _ = r.read_u8()?;
                let _ = r.read_u30()?;
                let _ = r.read_u8()?;
                let _ = r.read_u30()?;
                Ok(Vec::new())
            }
            _ => Ok(Vec::new()),
        })();
        match args_res {
            Ok(args) => out.push(Avm2Inst { offset, op, args }),
            Err(_) => break,
        }
    }
    out
}

fn extract_avm2_add_frame_scripts(abc: &AbcFile, code: &[u8]) -> (Vec<(u32, u32)>, HashSet<usize>) {
    let insts = decode_avm2_instructions(code);
    let mut hooks = Vec::new();
    let mut fn_inst_indices = HashSet::new();
    for (idx, inst) in insts.iter().enumerate() {
        if matches!(inst.op, 0x46 | 0x4F) && inst.args.len() >= 2 {
            let mn_idx = inst.args[0] as usize;
            let argc = inst.args[1] as usize;
            let mn_name = abc
                .multinames
                .get(mn_idx)
                .map_or("", |m| short_multiname(&m.name));
            if mn_name == "addFrameScript" && argc >= 2 && argc.is_multiple_of(2) {
                let pairs = argc / 2;
                if idx >= argc {
                    for p in 0..pairs {
                        let fn_pos = idx - argc + p * 2 + 1;
                        let f_inst = &insts[idx - argc + p * 2];
                        let fn_inst = &insts[fn_pos];
                        let frame_opt = match f_inst.op {
                            0x24 => Some(f_inst.args[0] as i8 as i32),
                            0x25 => Some(f_inst.args[0] as i32),
                            0x2D => abc.ints.get(f_inst.args[0] as usize).copied(),
                            0x2E => abc.uints.get(f_inst.args[0] as usize).map(|&u| u as i32),
                            _ => None,
                        };
                        let method_opt = match fn_inst.op {
                            0x40 => Some(fn_inst.args[0] as u32),
                            _ => None,
                        };
                        if let (Some(f), Some(mid)) = (frame_opt, method_opt)
                            && f >= 0
                        {
                            hooks.push((f as u32, mid));
                            fn_inst_indices.insert(fn_pos);
                        }
                    }
                }
            }
        }
    }
    (hooks, fn_inst_indices)
}

fn compile_avm2_method_body(
    ctx: &mut SwfDecodeCtx,
    abc: &AbcFile,
    code: &[u8],
    skip_add_frame_script: bool,
    extracted_fn_indices: &HashSet<usize>,
) -> Option<String> {
    let insts = decode_avm2_instructions(code);
    if insts.is_empty() {
        return None;
    }
    let mut offset_to_idx = HashMap::new();
    for (idx, inst) in insts.iter().enumerate() {
        offset_to_idx.insert(inst.offset, idx);
    }
    offset_to_idx.insert(code.len(), insts.len());

    let mut js = String::from(
        "const __st = [], __sc = [this], __loc = [this];\nlet __pc = 0, __steps = 0;\nconst __callGlobal = (n, a) => n === 'stop' ? stop() : n === 'play' ? play() : n === 'gotoAndStop' ? gotoAndStop(...a) : n === 'gotoAndPlay' ? gotoAndPlay(...a) : n === 'nextFrame' ? nextFrame() : n === 'prevFrame' ? prevFrame() : n === 'trace' ? trace(...a) : __nfSwfCall(this, n, a);\nwhile (__pc < ",
    );
    js.push_str(&insts.len().to_string());
    js.push_str(") {\n  if (++__steps > 10000) break;\n  switch (__pc) {\n");

    for (idx, inst) in insts.iter().enumerate() {
        js.push_str(&format!("    case {idx}: {{\n"));
        let next_idx = idx + 1;
        let mn = |arg_i: usize| -> (String, bool) {
            let mi = inst.args.get(arg_i).copied().unwrap_or(0) as usize;
            abc.multinames.get(mi).map_or_else(
                || (String::new(), false),
                |m| (short_multiname(&m.name).to_owned(), m.runtime_name),
            )
        };
        let branch_target = |rel_arg: i64| -> usize {
            let next_byte_off = insts.get(idx + 1).map_or(code.len(), |n| n.offset);
            let target_off = ((next_byte_off as i64) + rel_arg).max(0) as usize;
            offset_to_idx
                .get(&target_off)
                .copied()
                .unwrap_or(insts.len())
        };

        match inst.op {
            0xD0 => js.push_str("      __st.push(__loc[0]);\n"),
            0xD1 => js.push_str("      __st.push(__loc[1]);\n"),
            0xD2 => js.push_str("      __st.push(__loc[2]);\n"),
            0xD3 => js.push_str("      __st.push(__loc[3]);\n"),
            0x62 => js.push_str(&format!("      __st.push(__loc[{}]);\n", inst.args[0])),
            0xD4 => js.push_str("      __loc[0] = __st.pop();\n"),
            0xD5 => js.push_str("      __loc[1] = __st.pop();\n"),
            0xD6 => js.push_str("      __loc[2] = __st.pop();\n"),
            0xD7 => js.push_str("      __loc[3] = __st.pop();\n"),
            0x63 => js.push_str(&format!("      __loc[{}] = __st.pop();\n", inst.args[0])),
            0x08 => js.push_str(&format!("      __loc[{}] = undefined;\n", inst.args[0])),
            0x30 => js.push_str("      __sc.push(__st.pop());\n"),
            0x1D => js.push_str("      __sc.pop();\n"),
            0x20 => js.push_str("      __st.push(null);\n"),
            0x21 => js.push_str("      __st.push(undefined);\n"),
            0x26 => js.push_str("      __st.push(true);\n"),
            0x27 => js.push_str("      __st.push(false);\n"),
            0x28 => js.push_str("      __st.push(NaN);\n"),
            0x24 => js.push_str(&format!("      __st.push({});\n", inst.args[0] as i8)),
            0x25 => js.push_str(&format!("      __st.push({});\n", inst.args[0])),
            0x2C => {
                let s = js_string_literal(
                    abc.strings
                        .get(inst.args[0] as usize)
                        .map_or("", String::as_str),
                );
                js.push_str(&format!("      __st.push({s});\n"));
            }
            0x2D => {
                let v = abc.ints.get(inst.args[0] as usize).copied().unwrap_or(0);
                js.push_str(&format!("      __st.push({v});\n"));
            }
            0x2E => {
                let v = abc.uints.get(inst.args[0] as usize).copied().unwrap_or(0);
                js.push_str(&format!("      __st.push({v});\n"));
            }
            0x2F => {
                let v = abc.doubles.get(inst.args[0] as usize).copied().unwrap_or(0.0);
                js.push_str(&format!("      __st.push({});\n", format_js_f64(v)));
            }
            0x29 => js.push_str("      __st.pop();\n"),
            0x2A => js.push_str("      __st.push(__st[__st.length - 1]);\n"),
            0x2B => js.push_str("      if (__st.length >= 2) { const __t = __st[__st.length - 1]; __st[__st.length - 1] = __st[__st.length - 2]; __st[__st.length - 2] = __t; }\n"),
            0x40 => {
                if !extracted_fn_indices.contains(&idx) {
                    ctx.warn("SWF AVM2 opcode newfunction (0x40) closure is not supported in the bounded SWF loader; substituted a hardened no-op stub.");
                }
                js.push_str("      __st.push(__nfSwfStubFn());\n");
            }
            0x41 => {
                let argc = inst.args.first().copied().unwrap_or(0).max(0);
                js.push_str(&format!(
                    "      {{ const __args = new Array({argc}); for (let __i = {argc} - 1; __i >= 0; __i--) __args[__i] = __st.pop(); const __recv = __st.pop(), __fn = __st.pop(); __st.push(__nfSwfApply(__fn, __recv, __args)); }}\n"
                ));
            }
            0x55 => {
                let cnt = inst.args[0].max(0);
                js.push_str(&format!("      {{ const __o = {{}}; for (let __i = 0; __i < {cnt}; __i++) {{ const __v = __st.pop(), __k = String(__st.pop() ?? ''); __nfSwfSet(__o, __k, __v); }} __st.push(__o); }}\n"));
            }
            0x56 => {
                let cnt = inst.args[0].max(0);
                js.push_str(&format!("      {{ const __a = new Array({cnt}); for (let __i = {cnt} - 1; __i >= 0; __i--) __a[__i] = __st.pop(); __st.push(__a); }}\n"));
            }
            0x5D | 0x5E => {
                let (_name, rt) = mn(0);
                if rt {
                    js.push_str("      __st.pop();\n");
                }
                js.push_str("      __st.push(this);\n");
            }
            0x60 => {
                let (name, _) = mn(0);
                let name_lit = js_string_literal(&name);
                js.push_str(&format!("      __st.push(__nfSwfGet(this, {name_lit}));\n"));
            }
            0x66 => {
                let (name, rt) = mn(0);
                if rt {
                    js.push_str("      { const __k = String(__st.pop() ?? ''), __o = __st.pop() ?? this; __st.push(__nfSwfGet(__o, __k)); }\n");
                } else {
                    let name_lit = js_string_literal(&name);
                    js.push_str(&format!(
                        "      {{ const __o = __st.pop() ?? this; __st.push(__nfSwfGet(__o, {name_lit})); }}\n"
                    ));
                }
            }
            0x61 | 0x68 => {
                let (name, rt) = mn(0);
                if rt {
                    js.push_str("      { const __v = __st.pop(), __k = String(__st.pop() ?? ''), __o = __st.pop() ?? this; __nfSwfSet(__o, __k, __v); }\n");
                } else {
                    let name_lit = js_string_literal(&name);
                    js.push_str(&format!(
                        "      {{ const __v = __st.pop(), __o = __st.pop() ?? this; __nfSwfSet(__o, {name_lit}, __v); }}\n"
                    ));
                }
            }
            0x46 | 0x4C | 0x4F => {
                let (name, rt) = mn(0);
                let argc = inst.args[1].max(0);
                let push_ret = inst.op != 0x4F;
                js.push_str(&format!("      {{ const __args = new Array({argc}); for (let __i = {argc} - 1; __i >= 0; __i--) __args[__i] = __st.pop();\n"));
                if rt {
                    js.push_str("        const __m = String(__st.pop() ?? ''), __recv = __st.pop() ?? this;\n        const __r = (__recv === this) ? __callGlobal(__m, __args) : __nfSwfCall(__recv, __m, __args);\n");
                } else if skip_add_frame_script && name == "addFrameScript" {
                    js.push_str("        __st.pop(); const __r = undefined;\n");
                } else {
                    let name_lit = js_string_literal(&name);
                    js.push_str(&format!(
                        "        const __recv = __st.pop() ?? this;\n        const __r = (__recv === this && !Object.hasOwn(this, {name_lit})) ? __callGlobal({name_lit}, __args) : __nfSwfCall(__recv, {name_lit}, __args);\n"
                    ));
                }
                if push_ret {
                    js.push_str("        __st.push(__r);\n");
                }
                js.push_str("      }\n");
            }
            0x49 => {
                ctx.warn("SWF AVM2 opcode constructsuper (0x49) superclass constructor chaining is not supported in the bounded SWF loader and was skipped.");
                let argc = inst.args[0].max(0);
                js.push_str(&format!(
                    "      for (let __i = 0; __i < {argc}; __i++) __st.pop(); __st.pop();\n"
                ));
            }
            0xA0 => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(typeof __a === 'string' || typeof __b === 'string' ? String(__a ?? '') + String(__b ?? '') : (Number(__a) || 0) + (Number(__b) || 0)); }\n"),
            0xA1 => js.push_str("      { const __b = Number(__st.pop()) || 0, __a = Number(__st.pop()) || 0; __st.push(__a - __b); }\n"),
            0xA2 => js.push_str("      { const __b = Number(__st.pop()) || 0, __a = Number(__st.pop()) || 0; __st.push(__a * __b); }\n"),
            0xA3 => js.push_str("      { const __b = Number(__st.pop()), __a = Number(__st.pop()); __st.push(__a / __b); }\n"),
            0xA4 => js.push_str("      { const __b = Number(__st.pop()), __a = Number(__st.pop()); __st.push(__a % __b); }\n"),
            0x90 => js.push_str("      __st.push(-(Number(__st.pop()) || 0));\n"),
            0x91 | 0xC0 => js.push_str("      __st.push((Number(__st.pop()) || 0) + 1);\n"),
            0x93 | 0xC1 => js.push_str("      __st.push((Number(__st.pop()) || 0) - 1);\n"),
            0x92 | 0xC2 => js.push_str(&format!("      __loc[{}] = (Number(__loc[{}]) || 0) + 1;\n", inst.args[0], inst.args[0])),
            0x94 | 0xC3 => js.push_str(&format!("      __loc[{}] = (Number(__loc[{}]) || 0) - 1;\n", inst.args[0], inst.args[0])),
            0x96 => js.push_str("      __st.push(!__st.pop());\n"),
            0x73 => js.push_str("      __st.push(Number(__st.pop()) | 0);\n"),
            0x74 => js.push_str("      __st.push(Number(__st.pop()) >>> 0);\n"),
            0x75 => js.push_str("      __st.push(Number(__st.pop()));\n"),
            0x76 => js.push_str("      __st.push(Boolean(__st.pop()));\n"),
            0x70 => js.push_str("      { const __v = __st.pop(); __st.push(String(__v ?? '')); }\n"),
            0x85 => js.push_str("      { const __v = __st.pop(); __st.push(__v == null ? null : String(__v)); }\n"),
            0xAB => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a == __b); }\n"),
            0xAC => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a === __b); }\n"),
            0xAD => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a < __b); }\n"),
            0xAE => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a <= __b); }\n"),
            0xAF => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a > __b); }\n"),
            0xB0 => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a >= __b); }\n"),
            0x10 => {
                let t = branch_target(inst.args[0]);
                js.push_str(&format!("      __pc = {t}; continue;\n"));
            }
            0x11 => {
                let t = branch_target(inst.args[0]);
                js.push_str(&format!("      if (__st.pop()) {{ __pc = {t}; continue; }}\n"));
            }
            0x12 => {
                let t = branch_target(inst.args[0]);
                js.push_str(&format!("      if (!__st.pop()) {{ __pc = {t}; continue; }}\n"));
            }
            0x13 => {
                let t = branch_target(inst.args[0]);
                js.push_str(&format!("      {{ const __b = __st.pop(), __a = __st.pop(); if (__a == __b) {{ __pc = {t}; continue; }} }}\n"));
            }
            0x14 => {
                let t = branch_target(inst.args[0]);
                js.push_str(&format!("      {{ const __b = __st.pop(), __a = __st.pop(); if (__a != __b) {{ __pc = {t}; continue; }} }}\n"));
            }
            0x15 | 0x0F => {
                let t = branch_target(inst.args[0]);
                let cmp = if inst.op == 0x15 { "<" } else { ">=" };
                js.push_str(&format!("      {{ const __b = __st.pop(), __a = __st.pop(); if (__a {cmp} __b) {{ __pc = {t}; continue; }} }}\n"));
            }
            0x16 | 0x0E => {
                let t = branch_target(inst.args[0]);
                let cmp = if inst.op == 0x16 { "<=" } else { ">" };
                js.push_str(&format!("      {{ const __b = __st.pop(), __a = __st.pop(); if (__a {cmp} __b) {{ __pc = {t}; continue; }} }}\n"));
            }
            0x17 | 0x0D => {
                let t = branch_target(inst.args[0]);
                let cmp = if inst.op == 0x17 { ">" } else { "<=" };
                js.push_str(&format!("      {{ const __b = __st.pop(), __a = __st.pop(); if (__a {cmp} __b) {{ __pc = {t}; continue; }} }}\n"));
            }
            0x18 | 0x0C => {
                let t = branch_target(inst.args[0]);
                let cmp = if inst.op == 0x18 { ">=" } else { "<" };
                js.push_str(&format!("      {{ const __b = __st.pop(), __a = __st.pop(); if (__a {cmp} __b) {{ __pc = {t}; continue; }} }}\n"));
            }
            0x19 => {
                let t = branch_target(inst.args[0]);
                js.push_str(&format!("      {{ const __b = __st.pop(), __a = __st.pop(); if (__a === __b) {{ __pc = {t}; continue; }} }}\n"));
            }
            0x47 => js.push_str("      return;\n"),
            0x48 => js.push_str("      return __st.pop();\n"),
            0xEF | 0xF0 | 0xF1 | 0xF2 | 0x02 => {}
            other => {
                ctx.warn(format!(
                    "SWF AVM2 opcode 0x{other:02X} is not supported in the bounded SWF loader and was skipped."
                ));
            }
        }
        js.push_str(&format!("      __pc = {next_idx}; break;\n    }}\n"));
    }

    js.push_str("    default: __pc = ");
    js.push_str(&insts.len().to_string());
    js.push_str("; break;\n  }\n}\n");
    Some(format!("(() => {{\n{js}}})();\n"))
}
