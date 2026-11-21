//! Bounded AVM1 (`DoAction` / `DoInitAction`) and AVM2 (`DoABC` / `DoABC2`) bytecode-to-JS compilers.

use super::{SwfDecodeCtx, attach_compiled_frame_script};
use neoflash_core::{SymbolKind, Timeline};
use std::collections::{HashMap, HashSet};

const MAX_AVM_BLOCK_INSTS: usize = 4096;
const MAX_AVM_NESTED_FN_DEPTH: usize = 16;
const MAX_SWF_OPCODE_ARITY: i64 = 4096;

pub(super) fn js_string_literal(s: &str) -> String {
    serde_json::to_string(s)
        .unwrap_or_else(|_| "\"\"".into())
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn cstr_from_bytes(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn read_cstr_at(bytes: &[u8], off: &mut usize) -> String {
    if *off >= bytes.len() {
        return String::new();
    }
    let start = *off;
    while *off < bytes.len() && bytes[*off] != 0 {
        *off += 1;
    }
    let out = String::from_utf8_lossy(&bytes[start..*off]).into_owned();
    *off = (*off + 1).min(bytes.len());
    out
}

fn read_u8_at(bytes: &[u8], off: &mut usize) -> Option<u8> {
    let b = *bytes.get(*off)?;
    *off += 1;
    Some(b)
}

fn read_u16_at(bytes: &[u8], off: &mut usize) -> Option<u16> {
    if *off + 2 > bytes.len() {
        return None;
    }
    let v = u16::from_le_bytes([bytes[*off], bytes[*off + 1]]);
    *off += 2;
    Some(v)
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
// Bounded AVM1 (`DoAction` / `DoInitAction`) Bytecode-to-JS Compiler
// ============================================================================

#[derive(Debug)]
struct Avm1Inst {
    offset: usize,
    op: u8,
    payload: Vec<u8>,
    fn_body: Vec<u8>,
}

pub(super) const AVM1_RUNTIME_INIT_JS: &str = "\
const __avm1 = (() => {\n\
  if (!__nfSwfEnv) return null;\n\
  if (!__nfSwfEnv.avm1) {\n\
    __nfSwfEnv.avm1 = {\n\
      tick: 0,\n\
      pendingQueue: [],\n\
      enterFrameClips: [],\n\
      unloadClips: [],\n\
      onEnterFrameMap: new WeakMap(),\n\
      onUnloadMap: new WeakMap(),\n\
      calcPathDepth(target) {\n\
        let d = 0, cur = target;\n\
        while (cur && d < 64) {\n\
          let next = null;\n\
          try { next = __nfDisplay?.parentSpace?.(cur) || null; } catch (_) { break; }\n\
          if (!next || next === cur) break;\n\
          d++;\n\
          cur = next;\n\
        }\n\
        return d;\n\
      },\n\
      ensureEnterFrameSlot(target, placedTick) {\n\
        let slot = this.enterFrameClips.find(it => it.target === target);\n\
        if (!slot) {\n\
          slot = { target, fns: [], placedTick, pathDepth: this.calcPathDepth(target) };\n\
          this.enterFrameClips.push(slot);\n\
        } else if (slot.placedTick === 0 && placedTick > 0) {\n\
          slot.placedTick = placedTick;\n\
        }\n\
        return slot;\n\
      },\n\
      ensureUnloadSlot(target) {\n\
        let slot = this.unloadClips.find(it => it.target === target && !it.fired);\n\
        if (!slot) {\n\
          slot = { target, fns: [], pathDepth: this.calcPathDepth(target), fired: false };\n\
          this.unloadClips.push(slot);\n\
        }\n\
        return slot;\n\
      },\n\
      registerClip(target, inits, constructs, loads, enterFrames, unloads) {\n\
        if (inits.length || constructs.length || loads.length) {\n\
          this.pendingQueue.push({ target, placedTick: 0, initRan: false, inits, constructs, loads });\n\
        }\n\
        if (enterFrames.length) {\n\
          const slot = this.ensureEnterFrameSlot(target, 0);\n\
          slot.fns.push(...enterFrames);\n\
        }\n\
        if (unloads && unloads.length) {\n\
          const uslot = this.ensureUnloadSlot(target);\n\
          uslot.fns.push(...unloads);\n\
        }\n\
      },\n\
      flushUnloads() {\n\
        const dead = this.unloadClips.filter(it => !it.fired && !__nfDisplay?.alive?.(it.target));\n\
        if (!dead.length) return;\n\
        for (const it of dead) it.fired = true;\n\
        this.unloadClips = this.unloadClips.filter(it => !it.fired);\n\
        dead.sort((a, b) => b.pathDepth - a.pathDepth);\n\
        for (const it of dead) {\n\
          for (const fn of it.fns) __nfSwfApply(fn, it.target, []);\n\
          const dynFn = this.onUnloadMap.get(it.target);\n\
          if (typeof dynFn === 'function') __nfSwfApply(dynFn, it.target, []);\n\
        }\n\
      },\n\
      stampPlacedTick(curTick) {\n\
        for (const it of this.pendingQueue) {\n\
          if (it.placedTick === 0) it.placedTick = curTick;\n\
        }\n\
        for (const sl of this.enterFrameClips) {\n\
          if (sl.placedTick === 0) sl.placedTick = curTick;\n\
        }\n\
      },\n\
      flushInitConstruct(curTick) {\n\
        this.stampPlacedTick(curTick);\n\
        const batch = this.pendingQueue.filter(it => !it.initRan);\n\
        if (!batch.length) return;\n\
        for (const it of batch) it.initRan = true;\n\
        for (const it of batch) {\n\
          if (__nfDisplay?.alive?.(it.target)) {\n\
            for (const fn of it.inits) { if (__nfDisplay?.alive?.(it.target)) __nfSwfApply(fn, it.target, []); }\n\
          }\n\
        }\n\
        for (const it of batch) {\n\
          if (__nfDisplay?.alive?.(it.target)) {\n\
            for (const fn of it.constructs) { if (__nfDisplay?.alive?.(it.target)) __nfSwfApply(fn, it.target, []); }\n\
          }\n\
        }\n\
      },\n\
      flushLoadsFor(target) {\n\
        if (!this.pendingQueue.length) return;\n\
        const runItems = [];\n\
        const keep = [];\n\
        for (const it of this.pendingQueue) {\n\
          if (!target || it.target === target) runItems.push(it);\n\
          else keep.push(it);\n\
        }\n\
        if (!runItems.length) return;\n\
        if (runItems.some(it => !it.initRan)) this.flushInitConstruct(this.tick || 1);\n\
        this.pendingQueue = keep;\n\
        for (const it of runItems) {\n\
          if (__nfDisplay?.alive?.(it.target)) {\n\
            for (const fn of it.loads) { if (__nfDisplay?.alive?.(it.target)) __nfSwfApply(fn, it.target, []); }\n\
          }\n\
        }\n\
      },\n\
      runEnterFrames(curTick) {\n\
        this.enterFrameClips = this.enterFrameClips.filter(it => __nfDisplay?.alive?.(it.target));\n\
        const ordered = this.enterFrameClips\n\
          .filter(it => it.placedTick > 0 && it.placedTick < curTick)\n\
          .slice()\n\
          .sort((a, b) => b.pathDepth - a.pathDepth);\n\
        for (const it of ordered) {\n\
          if (!__nfDisplay?.alive?.(it.target)) continue;\n\
          for (const fn of it.fns) {\n\
            if (__nfDisplay?.alive?.(it.target)) __nfSwfApply(fn, it.target, []);\n\
          }\n\
          const dynFn = this.onEnterFrameMap.get(it.target);\n\
          if (typeof dynFn === 'function' && __nfDisplay?.alive?.(it.target)) {\n\
            __nfSwfApply(dynFn, it.target, []);\n\
          }\n\
        }\n\
      },\n\
      setProp(obj, k, v) {\n\
        const ks = String(k);\n\
        if (obj != null && __nfDisplay?.alive?.(obj) && (v == null || typeof v === 'function')) {\n\
          if (ks === 'onEnterFrame') {\n\
            if (typeof v === 'function') {\n\
              this.onEnterFrameMap.set(obj, v);\n\
              this.ensureEnterFrameSlot(obj, this.tick || 1);\n\
            } else {\n\
              this.onEnterFrameMap.delete(obj);\n\
            }\n\
            return __nfSwfSet(obj, ks, v);\n\
          }\n\
          if (ks === 'onUnload') {\n\
            if (typeof v === 'function') {\n\
              this.onUnloadMap.set(obj, v);\n\
              this.ensureUnloadSlot(obj);\n\
            } else {\n\
              this.onUnloadMap.delete(obj);\n\
            }\n\
            return __nfSwfSet(obj, ks, v);\n\
          }\n\
          if (/^on(?:Load|Data|Press|Release|ReleaseOutside|RollOver|RollOut|DragOver|DragOut|MouseDown|MouseUp|MouseMove|KeyDown|KeyUp|SetFocus|KillFocus)$/.test(ks)) {\n\
            try { __nfDisplay.setLegacyCallback?.(obj, ks, v); } catch (_) {}\n\
            return __nfSwfSet(obj, ks, v);\n\
          }\n\
        }\n\
        return __nfSwfSet(obj, ks, v);\n\
      },\n\
    };\n\
  }\n\
  return __nfSwfEnv.avm1;\n\
})();\n";

pub(super) fn compile_avm1_actions(
    ctx: &mut SwfDecodeCtx,
    bytes: &[u8],
    location: &str,
) -> Option<String> {
    let first_warning = ctx.warnings.len();
    let body = compile_avm1_block(ctx, bytes, false, 0);
    for warning in &mut ctx.warnings[first_warning..] {
        warning.push_str(&format!(" Source: {location}."));
    }
    let body = body?;
    Some(format!(
        "(() => {{\n{AVM1_RUNTIME_INIT_JS}const __baseTarget = this;\nconst __global = (__nfSwfEnv && __nfSwfEnv.avm1Global) || (__nfSwfEnv ? (__nfSwfEnv.avm1Global = Object.create(null)) : Object.create(null));\nlet __callDepth = 0;\nconst __outerLocals = [];\nconst __parentCp = [];\nconst __curThis = this;\nconst __superCtor = undefined;\ntry {{\n{body}}} catch (__topErr) {{\n  if (!(__topErr && typeof __topErr === 'object' && '__nfSwfThrown' in __topErr)) throw __topErr;\n}}\n}})();\n"
    ))
}

pub(super) fn compile_avm1_handler_fn_expr(
    ctx: &mut SwfDecodeCtx,
    bytes: &[u8],
    location: &str,
) -> Option<String> {
    let first_warning = ctx.warnings.len();
    let body = compile_avm1_block(ctx, bytes, false, 0);
    for warning in &mut ctx.warnings[first_warning..] {
        warning.push_str(&format!(" Source: {location}."));
    }
    let body = body.unwrap_or_default();
    Some(format!(
        "__nfSwfFn(function() {{\n{AVM1_RUNTIME_INIT_JS}const __baseTarget = this;\nconst __global = (__nfSwfEnv && __nfSwfEnv.avm1Global) || (__nfSwfEnv ? (__nfSwfEnv.avm1Global = Object.create(null)) : Object.create(null));\nlet __callDepth = 0;\nconst __outerLocals = [];\nconst __parentCp = [];\nconst __curThis = this;\nconst __superCtor = undefined;\ntry {{\n{body}}} catch (__topErr) {{\n  if (!(__topErr && typeof __topErr === 'object' && '__nfSwfThrown' in __topErr)) throw __topErr;\n}}\n}})"
    ))
}

fn compile_avm1_block(
    ctx: &mut SwfDecodeCtx,
    bytes: &[u8],
    in_fn: bool,
    depth: usize,
) -> Option<String> {
    let mut insts = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() && insts.len() < MAX_AVM_BLOCK_INSTS {
        let offset = pos;
        let op = bytes[pos];
        pos += 1;
        if op == 0x00 {
            break;
        }
        let mut fn_body = Vec::new();
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
                let body_end = pos.saturating_add(code_size).min(bytes.len());
                fn_body = bytes[pos..body_end].to_vec();
                pos = body_end;
            }
            slice
        } else {
            Vec::new()
        };
        insts.push(Avm1Inst {
            offset,
            op,
            payload,
            fn_body,
        });
    }
    if insts.is_empty() && !in_fn {
        return None;
    }

    let mut offset_to_idx = HashMap::new();
    for (idx, inst) in insts.iter().enumerate() {
        offset_to_idx.insert(inst.offset, idx);
    }
    let end_offset = pos;
    offset_to_idx.insert(end_offset, insts.len());

    let resolve_pc_at_or_after = |byte_off: usize| -> usize {
        if let Some(&idx) = offset_to_idx.get(&byte_off) {
            return idx;
        }
        insts
            .iter()
            .position(|inst| inst.offset >= byte_off)
            .unwrap_or(insts.len())
    };

    let mut js = String::new();
    if !in_fn {
        js.push_str(
            "const __st = [], __regs = Object.create(null), __locals = Object.create(null);\n",
        );
    } else {
        js.push_str("const __st = [];\n");
    }
    js.push_str(
        "const __withStack = [], __tryStack = [];\n\
let __cp = __parentCp, __pc = 0, __steps = 0, __curTarget = __baseTarget;\n\
const __propNames = ['x','y','scaleX','scaleY','currentFrame','totalFrames','alpha','visible','width','height','rotation','_target','_framesloaded','name','_droptarget','_url','_highquality','_focusrect','_soundbuftime','_quality','_xmouse','_ymouse'];\n\
const __builtinCtors = new Set(['Object','Array','String','Number','Boolean','Date','Error','TypeError','RangeError','ReferenceError','SyntaxError','URIError','EvalError','ArgumentError']);\n\
const __isSafeLocal = k => k !== '__proto__' && k !== 'constructor' && k !== 'prototype';\n\
const __readVar = k => {\n\
  if (!__isSafeLocal(k)) return undefined;\n\
  for (let __wi = __withStack.length - 1; __wi >= 0; __wi--) {\n\
    const __wo = __withStack[__wi].obj;\n\
    if (__wo != null && (typeof __wo === 'object' || typeof __wo === 'function') && __nfSwfHas(__wo, k)) return __nfSwfGet(__wo, k);\n\
  }\n\
  if (Object.hasOwn(__locals, k)) return __locals[k];\n\
  for (let __si = __outerLocals.length - 1; __si >= 0; __si--) {\n\
    if (Object.hasOwn(__outerLocals[__si], k)) return __outerLocals[__si][k];\n\
  }\n\
  if (k === 'this') return __curThis;\n\
  if (k === '_root' || k === '_level0') return _root;\n\
  if (k === '_parent') return (__curTarget && __nfSwfGet(__curTarget, '_parent')) ?? (__curThis && __nfSwfGet(__curThis, '_parent')) ?? _parent;\n\
  if (k === '_global') return __global;\n\
  if (k === 'Math') return Math;\n\
  if (k === 'NaN') return NaN;\n\
  if (k === 'Infinity') return Infinity;\n\
  const __tv = __nfSwfGet(__curTarget ?? __baseTarget, k);\n\
  if (__tv !== undefined) return __tv;\n\
  if (Object.hasOwn(__global, k)) return __global[k];\n\
  if (__builtinCtors.has(k)) return k;\n\
  return undefined;\n\
};\n\
const __avm1SetProp = (o, k, v) => __avm1 ? __avm1.setProp(o, k, v) : __nfSwfSet(o, k, v);\n\
const __writeVar = (k, v) => {\n\
  if (!__isSafeLocal(k)) return;\n\
  for (let __wi = __withStack.length - 1; __wi >= 0; __wi--) {\n\
    const __wo = __withStack[__wi].obj;\n\
    if (__wo != null && (typeof __wo === 'object' || typeof __wo === 'function') && __nfSwfHas(__wo, k)) { __avm1SetProp(__wo, k, v); return; }\n\
  }\n\
  if (Object.hasOwn(__locals, k)) { __locals[k] = v; return; }\n\
  for (let __si = __outerLocals.length - 1; __si >= 0; __si--) {\n\
    if (Object.hasOwn(__outerLocals[__si], k)) { __outerLocals[__si][k] = v; return; }\n\
  }\n\
  __avm1SetProp(__curTarget ?? __baseTarget, k, v);\n\
};\n\
const __routeThrow = __raw => {\n\
  const __val = (__raw && typeof __raw === 'object' && '__nfSwfThrown' in __raw) ? __raw.__nfSwfThrown : __raw;\n\
  while (__tryStack.length > 0) {\n\
    const __top = __tryStack[__tryStack.length - 1];\n\
    __st.length = __top.stLen;\n\
    if (__withStack.length > __top.withLen) __withStack.length = __top.withLen;\n\
    if (__top.phase === 'try') {\n\
      if (__top.hasCatch) {\n\
        if (__top.catchReg >= 0) __regs[__top.catchReg] = __val;\n\
        else if (__top.catchVar && __isSafeLocal(__top.catchVar)) {\n",
    );
    if in_fn {
        js.push_str("          __locals[__top.catchVar] = __val;\n");
    } else {
        js.push_str("          __writeVar(__top.catchVar, __val);\n");
    }
    js.push_str(
        "        }\n\
        __top.phase = 'catch'; __pc = __top.catchStart; return;\n\
      }\n\
      if (__top.hasFinally) {\n\
        __top.phase = 'finally'; __top.hasErr = true; __top.err = __val; __pc = __top.finStart; return;\n\
      }\n\
      __tryStack.pop();\n\
    } else if (__top.phase === 'catch') {\n\
      if (__top.hasFinally) {\n\
        __top.phase = 'finally'; __top.hasErr = true; __top.err = __val; __pc = __top.finStart; return;\n\
      }\n\
      __tryStack.pop();\n\
    } else {\n\
      __tryStack.pop();\n\
    }\n\
  }\n\
  throw { __nfSwfThrown: __val };\n\
};\n\
let __pendingRetVal = undefined;\n\
const __routeReturn = __rv => {\n\
  while (__tryStack.length > 0) {\n\
    const __top = __tryStack[__tryStack.length - 1];\n\
    __st.length = __top.stLen;\n\
    if (__withStack.length > __top.withLen) __withStack.length = __top.withLen;\n\
    if ((__top.phase === 'try' || __top.phase === 'catch') && __top.hasFinally) {\n\
      __top.phase = 'finally';\n\
      __top.hasErr = false;\n\
      __top.err = undefined;\n\
      __top.hasRet = true;\n\
      __top.ret = __rv;\n\
      __pc = __top.finStart;\n\
      return false;\n\
    }\n\
    __tryStack.pop();\n\
  }\n\
  __pendingRetVal = __rv;\n\
  return true;\n\
};\n\
while (__pc < ",
    );
    js.push_str(&insts.len().to_string());
    js.push_str(
        " || __tryStack.length > 0) {\n\
  if (++__steps > 20000) break;\n\
  while (__withStack.length > 0) {\n\
    const __wt = __withStack[__withStack.length - 1];\n\
    if (__pc < __wt.start || __pc >= __wt.end) __withStack.pop(); else break;\n\
  }\n\
  let __tryAdvanced = false;\n\
  while (__tryStack.length > 0) {\n\
    const __tt = __tryStack[__tryStack.length - 1];\n\
    if (__tt.phase === 'try') {\n\
      if (__pc === __tt.tryEnd) {\n\
        if (__tt.hasFinally) { __tt.phase = 'finally'; __pc = __tt.finStart; __tryAdvanced = true; break; }\n\
        __tryStack.pop(); __pc = __tt.afterPc; __tryAdvanced = true; continue;\n\
      }\n\
      if (__pc < __tt.tryStart || __pc > __tt.tryEnd) { __tryStack.pop(); continue; }\n\
      break;\n\
    } else if (__tt.phase === 'catch') {\n\
      if (__pc === __tt.catchEnd) {\n\
        if (__tt.hasFinally) { __tt.phase = 'finally'; __pc = __tt.finStart; __tryAdvanced = true; break; }\n\
        __tryStack.pop(); __pc = __tt.afterPc; __tryAdvanced = true; continue;\n\
      }\n\
      if (__pc < __tt.catchStart || __pc > __tt.catchEnd) { __tryStack.pop(); continue; }\n\
      break;\n\
    } else {\n\
      if (__pc === __tt.finEnd) {\n\
        __tryStack.pop();\n\
        if (__tt.hasErr) { __routeThrow({ __nfSwfThrown: __tt.err }); __tryAdvanced = true; break; }\n\
        if (__tt.hasRet) { if (__routeReturn(__tt.ret)) return __pendingRetVal; __tryAdvanced = true; break; }\n\
        __pc = __tt.afterPc; __tryAdvanced = true; continue;\n\
      }\n\
      if (__pc < __tt.finStart || __pc > __tt.finEnd) { __tryStack.pop(); continue; }\n\
      break;\n\
    }\n\
  }\n\
  if (__tryAdvanced) continue;\n\
  if (__pc >= ",
    );
    js.push_str(&insts.len().to_string());
    js.push_str(
        ") break;\n\
  try {\n\
  switch (__pc) {\n",
    );

    for (idx, inst) in insts.iter().enumerate() {
        js.push_str(&format!("    case {idx}: {{\n"));
        let next_idx = idx + 1;
        let p = &inst.payload;
        match inst.op {
            0x04 => js.push_str("      if (__curTarget !== __baseTarget && __curTarget) __nfSwfCall(__curTarget, 'nextFrame', []); else if (typeof nextFrame === 'function') nextFrame();\n"),
            0x05 => js.push_str("      if (__curTarget !== __baseTarget && __curTarget) __nfSwfCall(__curTarget, 'prevFrame', []); else if (typeof prevFrame === 'function') prevFrame();\n"),
            0x06 => js.push_str("      if (__curTarget !== __baseTarget && __curTarget) __nfSwfCall(__curTarget, 'play', []); else if (typeof play === 'function') play();\n"),
            0x07 => js.push_str("      if (__curTarget !== __baseTarget && __curTarget) __nfSwfCall(__curTarget, 'stop', []); else if (typeof stop === 'function') stop();\n"),
            0x08 => {} // ActionToggleQuality
            0x09 => {} // ActionStopSounds
            0x13 => js.push_str("      { const __b = String(__st.pop() ?? ''), __a = String(__st.pop() ?? ''); __st.push(__a === __b); }\n"),
            0x14 | 0x31 => js.push_str("      __st.push(String(__st.pop() ?? '').length);\n"),
            0x15 | 0x35 => js.push_str("      { const __cnt = Number(__st.pop()) | 0, __idx = Math.max(0, (Number(__st.pop()) | 0) - 1), __s = String(__st.pop() ?? ''); __st.push(__cnt < 0 ? __s.slice(__idx) : __s.slice(__idx, __idx + __cnt)); }\n"),
            0x20 => js.push_str("      { const __t = __st.pop(); __curTarget = (!__t || __t === '') ? __baseTarget : (typeof __t === 'string' && __nfDisplay && typeof __nfDisplay.resolveTarget === 'function' ? (__nfDisplay.resolveTarget(__t, __baseTarget) ?? __baseTarget) : __t); }\n"),
            0x21 => js.push_str("      { const __b = String(__st.pop() ?? ''), __a = String(__st.pop() ?? ''); __st.push(__a + __b); }\n"),
            0x24 => js.push_str("      { const __d = Number(__st.pop()) | 0, __t = String(__st.pop() ?? ''), __s = __st.pop(); if (__nfAs2Method) { try { __nfAs2Method(__baseTarget, 'duplicateMovieClip')(__s, __t, __d - 16384); } catch (_) {} } }\n"),
            0x25 => js.push_str("      { const __t = __st.pop(); if (__nfAs2Method) { try { __nfAs2Method(__baseTarget, 'removeMovieClip')(__t); } catch (_) {} } }\n"),
            0x27 => js.push_str("      { const __tgt = __st.pop(), __lock = Boolean(__st.pop()), __constrain = Number(__st.pop()) !== 0; let __b = []; if (__constrain) { const __y2 = Number(__st.pop()), __x2 = Number(__st.pop()), __y1 = Number(__st.pop()), __x1 = Number(__st.pop()); __b = [__x1, __y1, __x2, __y2]; } if (__tgt) __nfSwfCall(__tgt, 'startDrag', [__lock, ...__b]); }\n"),
            0x28 => js.push_str("      if (__curTarget) __nfSwfCall(__curTarget, 'stopDrag', []);\n"),
            0x29 => js.push_str("      { const __b = String(__st.pop() ?? ''), __a = String(__st.pop() ?? ''); __st.push(__a < __b); }\n"),
            0x30 => js.push_str("      { const __m = Number(__st.pop()) | 0; __st.push(__m > 0 ? Math.floor(Math.random() * __m) : 0); }\n"),
            0x32 | 0x36 => js.push_str("      { const __s = String(__st.pop() ?? ''); __st.push(__s.length > 0 ? __s.charCodeAt(0) : 0); }\n"),
            0x33 | 0x37 => js.push_str("      __st.push(String.fromCharCode(Number(__st.pop()) & 0xffff));\n"),
            0x34 => js.push_str("      __st.push(Math.floor(performance.now()));\n"),
            0x45 => js.push_str("      { const __o = __st.pop(); __st.push(__nfDisplay && typeof __nfDisplay.targetPath === 'function' ? __nfDisplay.targetPath(__o) : undefined); }\n"),
            0x68 => js.push_str("      { const __b = String(__st.pop() ?? ''), __a = String(__st.pop() ?? ''); __st.push(__a > __b); }\n"),
            0x8B => {
                let tgt = cstr_from_bytes(p);
                if tgt.is_empty() {
                    js.push_str("      __curTarget = __baseTarget;\n");
                } else {
                    let t_lit = js_string_literal(&tgt);
                    js.push_str(&format!(
                        "      __curTarget = (__nfDisplay && typeof __nfDisplay.resolveTarget === 'function') ? (__nfDisplay.resolveTarget({t_lit}, __baseTarget) ?? __baseTarget) : __baseTarget;\n"
                    ));
                }
            }
            0x8A | 0x8D => {} // ActionWaitForFrame / ActionWaitForFrame2 (all frames decoded synchronously in memory)
            0x9E => js.push_str("      { const __f = __st.pop(); if (__nfAs2Method) { try { __nfAs2Method(__curTarget ?? __baseTarget, 'callFrame')(__f); } catch (_) {} } }\n"),
            0x81 if p.len() >= 2 => {
                let frame0 = u32::from(u16::from_le_bytes([p[0], p[1]]));
                let next_is_play = insts.get(idx + 1).is_some_and(|n| n.op == 0x06);
                if next_is_play {
                    js.push_str(&format!(
                        "      if (__curTarget !== __baseTarget && __curTarget) __nfSwfCall(__curTarget, 'gotoAndPlay', [{}]); else if (typeof gotoAndPlay === 'function') gotoAndPlay({});\n",
                        frame0 + 1,
                        frame0 + 1
                    ));
                } else {
                    js.push_str(&format!(
                        "      if (__curTarget !== __baseTarget && __curTarget) __nfSwfCall(__curTarget, 'gotoAndStop', [{}]); else if (typeof gotoAndStop === 'function') gotoAndStop({});\n",
                        frame0 + 1,
                        frame0 + 1
                    ));
                }
            }
            0x8C => {
                let label = js_string_literal(&cstr_from_bytes(p));
                let next_is_play = insts.get(idx + 1).is_some_and(|n| n.op == 0x06);
                if next_is_play {
                    js.push_str(&format!(
                        "      if (__curTarget !== __baseTarget && __curTarget) __nfSwfCall(__curTarget, 'gotoAndPlay', [{label}]); else if (typeof gotoAndPlay === 'function') gotoAndPlay({label});\n"
                    ));
                } else {
                    js.push_str(&format!(
                        "      if (__curTarget !== __baseTarget && __curTarget) __nfSwfCall(__curTarget, 'gotoAndStop', [{label}]); else if (typeof gotoAndStop === 'function') gotoAndStop({label});\n"
                    ));
                }
            }
            0x9F if !p.is_empty() => {
                let play_flag = (p[0] & 0x01) != 0;
                if play_flag {
                    js.push_str("      { const __f = __st.pop(); if (__curTarget !== __baseTarget && __curTarget) __nfSwfCall(__curTarget, 'gotoAndPlay', [__f]); else if (typeof gotoAndPlay === 'function') gotoAndPlay(__f); }\n");
                } else {
                    js.push_str("      { const __f = __st.pop(); if (__curTarget !== __baseTarget && __curTarget) __nfSwfCall(__curTarget, 'gotoAndStop', [__f]); else if (typeof gotoAndStop === 'function') gotoAndStop(__f); }\n");
                }
            }
            0x83 => {
                // ActionGetURL (url\0target\0)
                let mut off = 0usize;
                let url_s = js_string_literal(&read_cstr_at(p, &mut off));
                let tgt_s = js_string_literal(&read_cstr_at(p, &mut off));
                js.push_str(&format!(
                    "      __nfSwfGetURL(__curThis ?? __baseTarget, {url_s}, {tgt_s}, 0);\n"
                ));
            }
            0x9A => {
                // ActionGetURL2 (stack: url, target)
                let flags = p.first().copied().unwrap_or(0);
                js.push_str(&format!(
                    "      {{ const __tgt = __st.pop(), __url = __st.pop(); __nfSwfGetURL(__curThis ?? __baseTarget, __url, __tgt, {flags}); }}\n"
                ));
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
                            js.push_str(&format!(
                                "      __st.push({});\n",
                                format_js_f64(f64::from(v))
                            ));
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
            0x3A => js.push_str("      { const __k = String(__st.pop() ?? ''), __o = __st.pop(); if (__k === 'onEnterFrame' && __o != null && __avm1) __avm1.onEnterFrameMap.delete(__o); __st.push(__nfSwfDelete(__o, __k)); }\n"),
            0x3B => js.push_str("      { const __k = String(__st.pop() ?? ''); let __d = false; if (__isSafeLocal(__k)) { if (Object.hasOwn(__locals, __k)) { delete __locals[__k]; __d = true; } else { if (__k === 'onEnterFrame' && __avm1) __avm1.onEnterFrameMap.delete(__baseTarget); __d = __nfSwfDelete(__baseTarget, __k); } } __st.push(__d); }\n"),
            0x3C => js.push_str("      { const __v = __st.pop(), __k = String(__st.pop() ?? ''); if (__isSafeLocal(__k)) __locals[__k] = __v; }\n"),
            0x41 => js.push_str("      { const __k = String(__st.pop() ?? ''); if (__isSafeLocal(__k) && !Object.hasOwn(__locals, __k)) __locals[__k] = undefined; }\n"),
            0x44 => js.push_str("      { const __v = __st.pop(); __st.push(__v === null ? 'null' : typeof __v === 'function' ? 'function' : typeof __v); }\n"),
            0x4E => js.push_str("      { const __m = String(__st.pop() ?? ''), __o = __st.pop(); __st.push(__nfSwfGet(__o, __m)); }\n"),
            0x4F => js.push_str("      { const __v = __st.pop(), __m = String(__st.pop() ?? ''), __o = __st.pop(); __avm1SetProp(__o, __m, __v); }\n"),
            0x22 => js.push_str("      { const __pi = Number(__st.pop()) | 0, __t = __st.pop() || __baseTarget, __pn = __propNames[__pi]; __st.push(__pn ? (__pi === 2 || __pi === 3 || __pi === 6 ? __nfSwfGet(__t, __pn) * 100 : __nfSwfGet(__t, __pn)) : undefined); }\n"),
            0x23 => js.push_str("      { const __v = __st.pop(), __pi = Number(__st.pop()) | 0, __t = __st.pop() || __baseTarget, __pn = __propNames[__pi]; if (__pn) __nfSwfSet(__t, __pn, (__pi === 2 || __pi === 3 || __pi === 6) ? Number(__v) / 100 : __v); }\n"),
            0x0A => js.push_str("      { const __b = Number(__st.pop()) || 0, __a = Number(__st.pop()) || 0; __st.push(__a + __b); }\n"),
            0x47 => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(typeof __a === 'string' || typeof __b === 'string' ? String(__a ?? '') + String(__b ?? '') : (Number(__a) || 0) + (Number(__b) || 0)); }\n"),
            0x0B => js.push_str("      { const __b = Number(__st.pop()) || 0, __a = Number(__st.pop()) || 0; __st.push(__a - __b); }\n"),
            0x0C => js.push_str("      { const __b = Number(__st.pop()) || 0, __a = Number(__st.pop()) || 0; __st.push(__a * __b); }\n"),
            0x0D => js.push_str("      { const __b = Number(__st.pop()), __a = Number(__st.pop()); __st.push(__a / __b); }\n"),
            0x3F => js.push_str("      { const __b = Number(__st.pop()), __a = Number(__st.pop()); __st.push(__a % __b); }\n"),
            0x60 => js.push_str("      { const __b = Number(__st.pop()) | 0, __a = Number(__st.pop()) | 0; __st.push(__a & __b); }\n"),
            0x61 => js.push_str("      { const __b = Number(__st.pop()) | 0, __a = Number(__st.pop()) | 0; __st.push(__a | __b); }\n"),
            0x62 => js.push_str("      { const __b = Number(__st.pop()) | 0, __a = Number(__st.pop()) | 0; __st.push(__a ^ __b); }\n"),
            0x63 => js.push_str("      { const __b = Number(__st.pop()) & 31, __a = Number(__st.pop()) | 0; __st.push(__a << __b); }\n"),
            0x64 => js.push_str("      { const __b = Number(__st.pop()) & 31, __a = Number(__st.pop()) | 0; __st.push(__a >> __b); }\n"),
            0x65 => js.push_str("      { const __b = Number(__st.pop()) & 31, __a = Number(__st.pop()) >>> 0; __st.push(__a >>> __b); }\n"),
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
            0x42 => js.push_str("      { const __n = Math.min(4096, Math.max(0, Number(__st.pop()) | 0)), __arr = []; for (let __i = 0; __i < __n; __i++) __arr.push(__st.pop()); __st.push(__arr); }\n"),
            0x43 => js.push_str("      { const __n = Math.min(4096, Math.max(0, Number(__st.pop()) | 0)), __obj = {}; for (let __i = 0; __i < __n; __i++) { const __v = __st.pop(), __k = String(__st.pop() ?? ''); __nfSwfSet(__obj, __k, __v); } __st.push(__obj); }\n"),
            0x40 => js.push_str("      { const __cName = String(__st.pop() ?? ''), __argc = Math.min(4096, Math.max(0, Number(__st.pop()) | 0)), __args = []; for (let __i = 0; __i < __argc; __i++) __args.push(__st.pop()); const __ctor = __readVar(__cName) ?? __cName; __st.push(__nfSwfNew(__ctor, undefined, __args)); }\n"),
            0x53 => js.push_str("      { const __mRaw = __st.pop(), __obj = __st.pop(), __argc = Math.min(4096, Math.max(0, Number(__st.pop()) | 0)), __args = []; for (let __i = 0; __i < __argc; __i++) __args.push(__st.pop()); const __mName = (__mRaw === undefined || __mRaw === null || __mRaw === '') ? undefined : String(__mRaw); __st.push(__nfSwfNew(__obj, __mName, __args)); }\n"),
            0x54 => js.push_str("      { const __ctor = __st.pop(), __obj = __st.pop(); __st.push(__nfSwfInstanceOf(__obj, __ctor)); }\n"),
            0x46 => js.push_str("      { const __k = String(__st.pop() ?? ''), __obj = __readVar(__k); __st.push(null); if (__obj != null && (typeof __obj === 'object' || typeof __obj === 'function')) { const __eks = __nfSwfKeys(__obj); for (let __ei = __eks.length - 1; __ei >= 0; __ei--) __st.push(__eks[__ei]); } }\n"),
            0x55 => js.push_str("      { const __obj = __st.pop(); __st.push(null); if (__obj != null && (typeof __obj === 'object' || typeof __obj === 'function')) { const __eks = __nfSwfKeys(__obj); for (let __ei = __eks.length - 1; __ei >= 0; __ei--) __st.push(__eks[__ei]); } }\n"),
            0x69 => js.push_str("      { const __sup = __st.pop(), __sub = __st.pop(); __nfSwfExtends(__sub, __sup); }\n"),
            0x3D => js.push_str("      { const __fnName = String(__st.pop() ?? ''), __argc = Math.min(4096, Math.max(0, Number(__st.pop()) | 0)), __args = []; for (let __i = 0; __i < __argc; __i++) __args.push(__st.pop()); if (__fnName === 'fscommand') { __nfSwfGetURL(__curThis ?? __baseTarget, 'FSCommand:' + String(__args[0] ?? ''), String(__args[1] ?? ''), 0); __st.push(undefined); } else if (__fnName === 'getURL') { __nfSwfGetURL(__curThis ?? __baseTarget, __args[0], __args[1], 0); __st.push(undefined); } else { const __fn = __fnName === 'stop' ? stop : __fnName === 'play' ? play : __fnName === 'gotoAndStop' ? gotoAndStop : __fnName === 'gotoAndPlay' ? gotoAndPlay : __fnName === 'nextFrame' ? nextFrame : __fnName === 'prevFrame' ? prevFrame : __fnName === 'trace' ? trace : __fnName === 'Number' ? (v => Number(v)) : __fnName === 'String' ? (v => String(v ?? '')) : __fnName === 'Boolean' ? (v => Boolean(v)) : __fnName === 'int' ? (v => Number(v) | 0) : __fnName === 'parseInt' ? parseInt : __fnName === 'parseFloat' ? parseFloat : __fnName === 'isNaN' ? isNaN : __fnName === 'isFinite' ? isFinite : __readVar(__fnName); __st.push((__fn === parseInt || __fn === parseFloat || __fn === isNaN || __fn === isFinite) ? __fn(...__args) : __nfSwfApply(__fn, __curThis ?? __baseTarget, __args)); } }\n"),
            0x52 => js.push_str("      { const __mRaw = __st.pop(), __obj = __st.pop(), __argc = Math.min(4096, Math.max(0, Number(__st.pop()) | 0)), __args = []; for (let __i = 0; __i < __argc; __i++) __args.push(__st.pop()); if (__mRaw === undefined || __mRaw === null || __mRaw === '') { __st.push(__nfSwfApply(__obj, __curThis ?? __baseTarget, __args)); } else { const __mStr = String(__mRaw); if (__obj === 'String' && __mStr === 'fromCharCode') { __st.push(String.fromCharCode(...__args.map(__c => Number(__c) & 0xffff))); } else if (__obj === 'Object' && __mStr === 'registerClass') { __st.push(__nfLegacy?.Object?.registerClass ? __nfLegacy.Object.registerClass(__args[0], __args[1]) : false); } else { const __tObj = (typeof __obj === 'string' && !__builtinCtors.has(__obj)) ? __obj : (__obj ?? __curThis ?? __baseTarget); __st.push(__nfSwfCall(__tObj, __mStr, __args)); } } }\n"),
            0x9B | 0x8E => {
                emit_avm1_define_function(ctx, &mut js, inst, in_fn, depth);
            }
            0x94 if p.len() >= 2 => {
                // ActionWith (bounded scope stack <= 16)
                let with_size = usize::from(u16::from_le_bytes([p[0], p[1]]));
                let start_off = inst.offset + 3 + p.len();
                let end_off = start_off.saturating_add(with_size);
                let start_pc = next_idx;
                let end_pc = resolve_pc_at_or_after(end_off);
                js.push_str(&format!(
                    "      {{ const __wo = __st.pop(); if (__wo != null && __withStack.length < 16) __withStack.push({{ obj: __wo, start: {start_pc}, end: {end_pc} }}); }}\n"
                ));
            }
            0x8F if p.len() >= 7 => {
                // ActionTry
                let flags = p[0];
                let try_size = usize::from(u16::from_le_bytes([p[1], p[2]]));
                let catch_size = usize::from(u16::from_le_bytes([p[3], p[4]]));
                let fin_size = usize::from(u16::from_le_bytes([p[5], p[6]]));
                let has_catch = (flags & 0x01) != 0;
                let has_finally = (flags & 0x02) != 0;
                let catch_in_reg = (flags & 0x04) != 0;
                let mut off = 7usize;
                let (catch_reg, catch_var) = if catch_in_reg {
                    let reg = read_u8_at(p, &mut off).map_or(-1i32, i32::from);
                    (reg, String::new())
                } else {
                    (-1i32, read_cstr_at(p, &mut off))
                };
                let try_start_off = inst.offset + 3 + p.len();
                let catch_start_off = try_start_off.saturating_add(try_size);
                let fin_start_off = catch_start_off.saturating_add(catch_size);
                let after_off = fin_start_off.saturating_add(fin_size);
                let try_start_pc = next_idx;
                let catch_start_pc = resolve_pc_at_or_after(catch_start_off);
                let fin_start_pc = resolve_pc_at_or_after(fin_start_off);
                let after_pc = resolve_pc_at_or_after(after_off);
                let catch_var_lit = js_string_literal(&catch_var);
                js.push_str(&format!(
                    "      if (__tryStack.length < 32) __tryStack.push({{ phase: 'try', tryStart: {try_start_pc}, tryEnd: {catch_start_pc}, hasCatch: {has_catch}, catchStart: {catch_start_pc}, catchEnd: {fin_start_pc}, catchReg: {catch_reg}, catchVar: {catch_var_lit}, hasFinally: {has_finally}, finStart: {fin_start_pc}, finEnd: {after_pc}, afterPc: {after_pc}, stLen: __st.length, withLen: __withStack.length, hasErr: false, err: undefined, hasRet: false, ret: undefined }});\n"
                ));
            }
            0x2A => {
                // ActionThrow
                js.push_str("      __routeThrow({ __nfSwfThrown: __st.pop() }); continue;\n");
            }
            0x99 if p.len() >= 2 => {
                let rel = i16::from_le_bytes([p[0], p[1]]) as isize;
                let base = (inst.offset + 3 + p.len()) as isize;
                let target_off = (base + rel).max(0) as usize;
                let target_pc = offset_to_idx
                    .get(&target_off)
                    .copied()
                    .unwrap_or(insts.len());
                js.push_str(&format!("      __pc = {target_pc}; continue;\n"));
            }
            0x9D if p.len() >= 2 => {
                let rel = i16::from_le_bytes([p[0], p[1]]) as isize;
                let base = (inst.offset + 3 + p.len()) as isize;
                let target_off = (base + rel).max(0) as usize;
                let target_pc = offset_to_idx
                    .get(&target_off)
                    .copied()
                    .unwrap_or(insts.len());
                js.push_str(&format!(
                    "      if (__st.pop()) {{ __pc = {target_pc}; continue; }}\n"
                ));
            }
            0x3E => js.push_str(
                "      if (__routeReturn(__st.pop())) return __pendingRetVal; continue;\n",
            ),
            other => {
                ctx.warn_opcode("AVM1", other, inst.offset);
            }
        }
        js.push_str(&format!("      __pc = {next_idx}; break;\n    }}\n"));
    }
    js.push_str("    default: __pc = ");
    js.push_str(&insts.len().to_string());
    js.push_str(
        "; break;\n  }\n  } catch (__stepErr) {\n    __routeThrow(__stepErr); continue;\n  }\n}\n",
    );
    Some(js)
}

fn emit_avm1_define_function(
    ctx: &mut SwfDecodeCtx,
    js: &mut String,
    inst: &Avm1Inst,
    in_fn: bool,
    depth: usize,
) {
    let is_v2 = inst.op == 0x8E;
    let p = &inst.payload;
    if p.len() < 5 {
        return;
    }
    let header_end = p.len().saturating_sub(2);
    let hdr = &p[..header_end];
    let mut off = 0usize;
    let fn_name = read_cstr_at(hdr, &mut off);
    let Some(num_params_raw) = read_u16_at(hdr, &mut off) else {
        return;
    };
    let num_params = usize::from(num_params_raw).min(4096);
    let (register_count, flags) = if is_v2 {
        let Some(rc) = read_u8_at(hdr, &mut off) else {
            return;
        };
        let Some(fl) = read_u16_at(hdr, &mut off) else {
            return;
        };
        (usize::from(rc).min(256), fl)
    } else {
        (0usize, 0u16)
    };

    let mut params: Vec<(u8, String)> = Vec::with_capacity(num_params.min(256));
    for _ in 0..num_params {
        if off >= hdr.len() {
            break;
        }
        let reg = if is_v2 {
            read_u8_at(hdr, &mut off).unwrap_or(0)
        } else {
            0
        };
        let pname = read_cstr_at(hdr, &mut off);
        params.push((reg, pname));
    }

    if depth >= MAX_AVM_NESTED_FN_DEPTH {
        ctx.warn(
            "SWF AVM1 nested DefineFunction depth exceeds 16 levels; substituted hardened stub.",
        );
        if fn_name.is_empty() {
            js.push_str("      __st.push(__nfSwfStubFn());\n");
        } else {
            let name_lit = js_string_literal(&fn_name);
            if in_fn {
                js.push_str(&format!(
                    "      if (__isSafeLocal({name_lit})) __locals[{name_lit}] = __nfSwfStubFn();\n"
                ));
            } else {
                js.push_str(&format!(
                    "      __nfSwfSet(__baseTarget, {name_lit}, __nfSwfStubFn());\n"
                ));
            }
        }
        return;
    }

    let inner_body = compile_avm1_block(ctx, &inst.fn_body, true, depth + 1).unwrap_or_default();

    let preload_this = is_v2 && (flags & 0x0001) != 0;
    let suppress_this = is_v2 && (flags & 0x0002) != 0;
    let preload_arguments = is_v2 && (flags & 0x0004) != 0;
    let suppress_arguments = is_v2 && (flags & 0x0008) != 0;
    let preload_super = is_v2 && (flags & 0x0010) != 0;
    let suppress_super = is_v2 && (flags & 0x0020) != 0;
    let preload_root = is_v2 && (flags & 0x0040) != 0;
    let preload_parent = is_v2 && (flags & 0x0080) != 0;
    let preload_global = is_v2 && (flags & 0x0100) != 0;

    let mut setup =
        String::from("const __regs = Object.create(null), __locals = Object.create(null);\n");
    if register_count > 1 {
        setup.push_str(&format!(
            "for (let __ri = 1; __ri < {register_count}; __ri++) __regs[__ri] = undefined;\n"
        ));
    }
    let needs_args_obj = !suppress_arguments || preload_arguments;
    if needs_args_obj {
        setup.push_str(
            "const __argsObj = [...__callArgs]; __nfSwfSet(__argsObj, 'callee', __self);\n",
        );
    }

    let mut next_preload_reg = 1u8;
    if preload_this {
        setup.push_str(&format!("__regs[{next_preload_reg}] = __curThis;\n"));
        next_preload_reg = next_preload_reg.saturating_add(1);
    } else if !suppress_this {
        setup.push_str("__locals['this'] = __curThis;\n");
    }
    if preload_arguments {
        setup.push_str(&format!("__regs[{next_preload_reg}] = __argsObj;\n"));
        next_preload_reg = next_preload_reg.saturating_add(1);
    } else if !suppress_arguments {
        setup.push_str("__locals['arguments'] = __argsObj;\n");
    }
    if preload_super {
        setup.push_str(&format!("__regs[{next_preload_reg}] = __superCtor;\n"));
        next_preload_reg = next_preload_reg.saturating_add(1);
    } else if !suppress_super {
        setup.push_str("__locals['super'] = __superCtor;\n");
    }
    if preload_root {
        setup.push_str(&format!("__regs[{next_preload_reg}] = _root;\n"));
        next_preload_reg = next_preload_reg.saturating_add(1);
    }
    if preload_parent {
        setup.push_str(&format!(
            "__regs[{next_preload_reg}] = (__curThis && __nfSwfGet(__curThis, '_parent')) ?? _parent;\n"
        ));
        next_preload_reg = next_preload_reg.saturating_add(1);
    }
    if preload_global {
        setup.push_str(&format!("__regs[{next_preload_reg}] = __global;\n"));
    }

    for (idx, (preg, pname)) in params.iter().enumerate() {
        if is_v2 && *preg > 0 && usize::from(*preg) < register_count.max(256) {
            setup.push_str(&format!("__regs[{preg}] = __callArgs[{idx}];\n"));
        } else if !pname.is_empty()
            && pname != "__proto__"
            && pname != "constructor"
            && pname != "prototype"
        {
            let p_lit = js_string_literal(pname);
            setup.push_str(&format!("__locals[{p_lit}] = __callArgs[{idx}];\n"));
        }
    }

    let fn_expr = format!(
        "(() => {{\n\
  const __capturedScopes = [...__outerLocals, __locals].slice(-16);\n\
  const __capturedCp = __cp;\n\
  let __self = null;\n\
  __self = __nfSwfFn(function(...__callArgs) {{\n\
    if (++__callDepth > 64) {{ __callDepth--; return undefined; }}\n\
    try {{\n\
      const __curThis = this ?? __baseTarget;\n\
      const __superCtor = __nfSwfSuper(__self, __curThis);\n\
      const __outerLocals = __capturedScopes;\n\
      const __parentCp = __capturedCp;\n\
{setup}{inner_body}    }} finally {{\n\
      __callDepth--;\n\
    }}\n\
    return undefined;\n\
  }});\n\
  return __self;\n\
}})()"
    );

    if fn_name.is_empty() {
        js.push_str(&format!("      __st.push({fn_expr});\n"));
    } else {
        let name_lit = js_string_literal(&fn_name);
        if in_fn {
            js.push_str(&format!(
                "      {{ const __f = {fn_expr}; if (__isSafeLocal({name_lit})) __locals[{name_lit}] = __f; }}\n"
            ));
        } else {
            js.push_str(&format!(
                "      __nfSwfSet(__baseTarget, {name_lit}, {fn_expr});\n"
            ));
        }
    }
}

// ============================================================================
// Bounded AVM2 (`DoABC` / `DoABC2`) Parser & Class / Closure / Sprite Compiler
// ============================================================================

#[derive(Clone, Debug)]
struct AbcMultiname {
    name: String,
    runtime_name: bool,
}

#[derive(Clone, Debug)]
enum AbcDefaultVal {
    Undefined,
    Null,
    Bool(bool),
    Int(i32),
    Uint(u32),
    Double(f64),
    Str(String),
}

#[derive(Clone, Debug)]
struct AbcMethodInfo {
    param_count: usize,
    flags: u8,
    defaults: Vec<AbcDefaultVal>,
}

#[derive(Clone, Debug)]
struct AbcTrait {
    name: String,
    kind: u8,
    slot_id: u32,
    type_name: String,
    method_or_class_id: u32,
    default_val: Option<AbcDefaultVal>,
}

#[derive(Clone, Debug)]
struct AbcInstanceInfo {
    name: String,
    super_name: String,
    iinit: u32,
    traits: Vec<AbcTrait>,
    cinit: u32,
    static_traits: Vec<AbcTrait>,
}

#[derive(Clone, Debug)]
struct AbcException {
    from: u32,
    to: u32,
    target: u32,
    exc_type: String,
    var_name: String,
}

#[derive(Clone, Debug)]
struct AbcMethodBody {
    init_scope_depth: u32,
    code: Vec<u8>,
    exceptions: Vec<AbcException>,
    traits: Vec<AbcTrait>,
}

#[derive(Clone, Debug)]
pub(crate) struct AbcFile {
    ints: Vec<i32>,
    uints: Vec<u32>,
    doubles: Vec<f64>,
    strings: Vec<String>,
    multinames: Vec<AbcMultiname>,
    methods: Vec<AbcMethodInfo>,
    instances: Vec<AbcInstanceInfo>,
    script_inits: Vec<u32>,
    script_traits: Vec<Vec<AbcTrait>>,
    bodies: HashMap<u32, AbcMethodBody>,
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
        Ok(self.read_u30()? as i32)
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

    fn read_traits(
        &mut self,
        multinames: &[AbcMultiname],
        ints: &[i32],
        uints: &[u32],
        doubles: &[f64],
        strings: &[String],
    ) -> Result<Vec<AbcTrait>, String> {
        let count = self.read_u30()? as usize;
        let mut out = Vec::with_capacity(count.min(1024));
        for _ in 0..count.min(4096) {
            let name_mn = self.read_u30()? as usize;
            let name = multinames
                .get(name_mn)
                .map_or_else(String::new, |m| m.name.clone());
            let kind_byte = self.read_u8()?;
            let kind = kind_byte & 0x0F;
            let attrs = kind_byte >> 4;
            let tr = match kind {
                0 | 6 => {
                    let slot_id = self.read_u30()?;
                    let type_mn = self.read_u30()? as usize;
                    let type_name = multinames
                        .get(type_mn)
                        .map_or_else(String::new, |m| m.name.clone());
                    let vindex = self.read_u30()?;
                    let default_val = if vindex != 0 {
                        let vkind = self.read_u8()?;
                        Some(decode_abc_const_val(
                            vkind,
                            vindex as usize,
                            ints,
                            uints,
                            doubles,
                            strings,
                        ))
                    } else {
                        None
                    };
                    AbcTrait {
                        name,
                        kind,
                        slot_id,
                        type_name,
                        method_or_class_id: 0,
                        default_val,
                    }
                }
                1 | 2 | 3 | 5 => {
                    let slot_or_disp = self.read_u30()?;
                    let method_id = self.read_u30()?;
                    AbcTrait {
                        name,
                        kind,
                        slot_id: slot_or_disp,
                        type_name: String::new(),
                        method_or_class_id: method_id,
                        default_val: None,
                    }
                }
                4 => {
                    let slot_id = self.read_u30()?;
                    let classi = self.read_u30()?;
                    AbcTrait {
                        name,
                        kind,
                        slot_id,
                        type_name: String::new(),
                        method_or_class_id: classi,
                        default_val: None,
                    }
                }
                _ => return Err("Unknown ABC trait kind".into()),
            };
            if (attrs & 0x04) != 0 {
                let meta_count = self.read_u30()? as usize;
                for _ in 0..meta_count {
                    let _ = self.read_u30()?;
                }
            }
            out.push(tr);
        }
        Ok(out)
    }
}

fn decode_abc_const_val(
    vkind: u8,
    vindex: usize,
    ints: &[i32],
    uints: &[u32],
    doubles: &[f64],
    strings: &[String],
) -> AbcDefaultVal {
    match vkind {
        0x03 => AbcDefaultVal::Int(ints.get(vindex).copied().unwrap_or(0)),
        0x04 => AbcDefaultVal::Uint(uints.get(vindex).copied().unwrap_or(0)),
        0x06 => AbcDefaultVal::Double(doubles.get(vindex).copied().unwrap_or(0.0)),
        0x01 => AbcDefaultVal::Str(strings.get(vindex).cloned().unwrap_or_default()),
        0x0B => AbcDefaultVal::Bool(true),
        0x0A => AbcDefaultVal::Bool(false),
        0x0C => AbcDefaultVal::Null,
        _ => AbcDefaultVal::Undefined,
    }
}

fn default_val_js(v: &AbcDefaultVal) -> String {
    match v {
        AbcDefaultVal::Undefined => "undefined".into(),
        AbcDefaultVal::Null => "null".into(),
        AbcDefaultVal::Bool(b) => b.to_string(),
        AbcDefaultVal::Int(i) => i.to_string(),
        AbcDefaultVal::Uint(u) => u.to_string(),
        AbcDefaultVal::Double(d) => format_js_f64(*d),
        AbcDefaultVal::Str(s) => js_string_literal(s),
    }
}

fn trait_slot_default_js(tr: &AbcTrait) -> String {
    if let Some(dv) = &tr.default_val {
        return default_val_js(dv);
    }
    match short_multiname(&tr.type_name) {
        "int" | "uint" => "0".into(),
        "Number" => "NaN".into(),
        "Boolean" => "false".into(),
        "*" | "" => "undefined".into(),
        _ => "null".into(),
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
    let mut methods = Vec::with_capacity(method_count.min(32768));
    for _ in 0..method_count.min(32768) {
        let param_count = (r.read_u30()? as usize).min(4096);
        let _ret_type = r.read_u30()?;
        for _ in 0..param_count {
            let _ = r.read_u30()?;
        }
        let _name = r.read_u30()?;
        let flags = r.read_u8()?;
        let mut defaults = Vec::new();
        if (flags & 0x08) != 0 {
            let opt_count = (r.read_u30()? as usize).min(4096);
            for _ in 0..opt_count {
                let val = r.read_u30()? as usize;
                let kind = r.read_u8()?;
                defaults.push(decode_abc_const_val(
                    kind, val, &ints, &uints, &doubles, &strings,
                ));
            }
        }
        if (flags & 0x80) != 0 {
            for _ in 0..param_count {
                let _ = r.read_u30()?;
            }
        }
        methods.push(AbcMethodInfo {
            param_count,
            flags,
            defaults,
        });
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

    let class_count = (r.read_u30()? as usize).min(4096);
    let mut instances = Vec::with_capacity(class_count);
    for _ in 0..class_count {
        let name_mn = r.read_u30()? as usize;
        let super_mn = r.read_u30()? as usize;
        let flags = r.read_u8()?;
        if (flags & 0x08) != 0 {
            let _protected_ns = r.read_u30()?;
        }
        let intrf_count = r.read_u30()? as usize;
        for _ in 0..intrf_count {
            let _ = r.read_u30()?;
        }
        let iinit = r.read_u30()?;
        let traits = r.read_traits(&multinames, &ints, &uints, &doubles, &strings)?;
        let cls_name = multinames
            .get(name_mn)
            .map_or_else(String::new, |m| m.name.clone());
        let sup_name = multinames
            .get(super_mn)
            .map_or_else(String::new, |m| m.name.clone());
        instances.push(AbcInstanceInfo {
            name: cls_name,
            super_name: sup_name,
            iinit,
            traits,
            cinit: 0,
            static_traits: Vec::new(),
        });
    }
    for inst in &mut instances {
        inst.cinit = r.read_u30()?;
        inst.static_traits = r.read_traits(&multinames, &ints, &uints, &doubles, &strings)?;
    }

    let script_count = (r.read_u30()? as usize).min(4096);
    let mut script_inits = Vec::with_capacity(script_count);
    let mut script_traits = Vec::with_capacity(script_count);
    for _ in 0..script_count {
        let init = r.read_u30()?;
        let st = r.read_traits(&multinames, &ints, &uints, &doubles, &strings)?;
        script_inits.push(init);
        script_traits.push(st);
    }

    let body_count = r.read_u30()? as usize;
    let mut bodies = HashMap::new();
    for _ in 0..body_count.min(32768) {
        let method_idx = r.read_u30()?;
        let _max_stack = r.read_u30()?;
        let _local_count = r.read_u30()?;
        let init_scope_depth = r.read_u30()?;
        let _max_scope = r.read_u30()?;
        let code_len = r.read_u30()? as usize;
        if r.pos + code_len > r.data.len() {
            break;
        }
        let code = r.data[r.pos..r.pos + code_len].to_vec();
        r.pos += code_len;
        let exc_count = (r.read_u30()? as usize).min(1024);
        let mut exceptions = Vec::with_capacity(exc_count);
        for _ in 0..exc_count {
            let from = r.read_u30()?;
            let to = r.read_u30()?;
            let target = r.read_u30()?;
            let exc_type_mn = r.read_u30()? as usize;
            let var_name_mn = r.read_u30()? as usize;
            let exc_type = multinames
                .get(exc_type_mn)
                .map_or_else(String::new, |m| m.name.clone());
            let var_name = multinames
                .get(var_name_mn)
                .map_or_else(String::new, |m| short_multiname(&m.name).to_owned());
            exceptions.push(AbcException {
                from,
                to,
                target,
                exc_type,
                var_name,
            });
        }
        let traits = r.read_traits(&multinames, &ints, &uints, &doubles, &strings)?;
        bodies.insert(
            method_idx,
            AbcMethodBody {
                init_scope_depth,
                code,
                exceptions,
                traits,
            },
        );
    }

    Ok(AbcFile {
        ints,
        uints,
        doubles,
        strings,
        multinames,
        methods,
        instances,
        script_inits,
        script_traits,
        bodies,
    })
}

fn short_multiname(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

fn resolve_trait_slot_id(raw_slot: u32, next_auto: &mut u32) -> u32 {
    if raw_slot > 0 {
        if raw_slot >= *next_auto {
            *next_auto = raw_slot + 1;
        }
        raw_slot
    } else {
        let assigned = *next_auto;
        *next_auto += 1;
        assigned
    }
}

fn find_abc_class<'a>(abc: &'a AbcFile, target: &str) -> Option<(usize, &'a AbcInstanceInfo)> {
    abc.instances
        .iter()
        .enumerate()
        .find(|(_, i)| i.name == target)
        .or_else(|| {
            let short = short_multiname(target);
            abc.instances
                .iter()
                .enumerate()
                .find(|(_, i)| short_multiname(&i.name) == short)
        })
}

fn scan_static_frame_script_indices(abc: &AbcFile, leaf_idx: usize) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut visited = HashSet::new();
    let mut cur_opt = Some(leaf_idx);
    while let Some(ci) = cur_opt {
        if !visited.insert(ci) {
            break;
        }
        chain.push(ci);
        let sup = &abc.instances[ci].super_name;
        if sup.is_empty() || short_multiname(sup) == "Object" {
            break;
        }
        cur_opt = find_abc_class(abc, sup).map(|(pi, _)| pi);
    }
    chain.reverse();
    let mut frames = Vec::new();
    for ci in chain {
        if let Some(body) = abc.bodies.get(&abc.instances[ci].iinit) {
            let insts = decode_avm2_instructions(&body.code);
            for (idx, inst) in insts.iter().enumerate() {
                if matches!(inst.op, 0x46 | 0x4F) && inst.args.len() >= 2 {
                    let mn_idx = inst.args[0] as usize;
                    let argc = inst.args[1] as usize;
                    let mn_name = abc
                        .multinames
                        .get(mn_idx)
                        .map_or("", |m| short_multiname(&m.name));
                    if mn_name == "addFrameScript"
                        && argc >= 2
                        && argc.is_multiple_of(2)
                        && idx >= argc
                    {
                        let pairs = argc / 2;
                        for p in 0..pairs {
                            let f_inst = &insts[idx - argc + p * 2];
                            let frame_opt = match f_inst.op {
                                0x24 => Some(f_inst.args[0] as i8 as i32),
                                0x25 => Some(f_inst.args[0] as i32),
                                0x2D => abc.ints.get(f_inst.args[0] as usize).copied(),
                                0x2E => abc.uints.get(f_inst.args[0] as usize).map(|&u| u as i32),
                                _ => None,
                            };
                            if let Some(f) = frame_opt
                                && f >= 0
                            {
                                let fu = f as u32;
                                if !frames.contains(&fu) {
                                    frames.push(fu);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    frames.sort_unstable();
    frames
}

pub(super) fn apply_abc_scripts(
    ctx: &mut SwfDecodeCtx,
    root_timeline: &mut Timeline,
) -> Option<String> {
    let doc_class = ctx.symbol_classes.get(&0).cloned();
    let abc_list = ctx.abc_blocks.clone();

    let mut bound_classes_by_abc: Vec<HashSet<usize>> = vec![HashSet::new(); abc_list.len()];
    let mut valid_char_classes: HashMap<u16, String> = HashMap::new();
    let mut sprite_frame_attachments: Vec<(String, u32, String)> = Vec::new();

    // 1. Validate non-root SymbolClass (tag 76) sprite/button classes
    let mut non_root_bindings: Vec<(u16, String)> = ctx
        .symbol_classes
        .iter()
        .filter(|&(&cid, _)| cid != 0)
        .map(|(&cid, name)| (cid, name.clone()))
        .collect();
    non_root_bindings.sort_by_key(|&(cid, _)| cid);

    for (cid, cls_name) in non_root_bindings {
        let found_abc = abc_list
            .iter()
            .enumerate()
            .find_map(|(ai, abc)| find_abc_class(abc, &cls_name).map(|(ci, _)| (ai, ci)));
        let sym_id_opt = ctx.char_symbols.get(&cid).cloned();
        let valid_sym = sym_id_opt
            .as_ref()
            .and_then(|sid| ctx.symbols.iter().find(|s| &s.id == sid))
            .is_some_and(|s| matches!(s.kind, SymbolKind::MovieClip | SymbolKind::Button));

        if let (Some((abc_idx, cls_idx)), Some(sym_id)) = (found_abc, sym_id_opt)
            && valid_sym
        {
            bound_classes_by_abc[abc_idx].insert(cls_idx);
            valid_char_classes.insert(cid, cls_name.clone());
            let static_frames = scan_static_frame_script_indices(&abc_list[abc_idx], cls_idx);
            if let Some(sym) = ctx.symbols.iter_mut().find(|s| s.id == sym_id) {
                sym.class_name = Some(cls_name.clone());
                if sym.linkage_id.is_none() {
                    sym.linkage_id = Some(cls_name.clone());
                }
                let sym_fc = sym.timeline.frame_count;
                for f_idx in static_frames {
                    if f_idx < sym_fc {
                        sprite_frame_attachments.push((sym_id.clone(), f_idx, cls_name.clone()));
                    }
                }
            }
        } else {
            ctx.warn(format!(
                "SWF AVM2 SymbolClass binding '{cls_name}' (character {cid}) is not executed in the bounded SWF loader."
            ));
        }
    }

    // 2. Identify root Document Class (cid == 0, or primary ABC instance ONLY when SymbolClass is empty)
    let mut root_doc_target: Option<(usize, String)> = None;
    for (abc_idx, abc) in abc_list.iter().enumerate() {
        let chosen = if let Some(dc) = &doc_class {
            find_abc_class(abc, dc).map(|(ci, _)| ci)
        } else if ctx.symbol_classes.is_empty()
            && root_doc_target.is_none()
            && !abc.instances.is_empty()
            && !bound_classes_by_abc[abc_idx].contains(&0)
        {
            Some(0)
        } else {
            None
        };
        if let Some(ci) = chosen {
            bound_classes_by_abc[abc_idx].insert(ci);
            root_doc_target = Some((abc_idx, abc.instances[ci].name.clone()));
        }
    }

    if abc_list.is_empty() {
        return None;
    }

    // Build symbol/character metadata for the AVM2 display-tree coordinator (`__nfSwfEnv`).
    // Note: `MainTimeline` is inserted at `project.symbols[0]`, so `ctx.symbols[idx]` mounts at `s{idx + 1}`.
    let mut sym_id_to_mounted_slot: HashMap<String, usize> = HashMap::new();
    for (idx, sym) in ctx.symbols.iter().enumerate() {
        sym_id_to_mounted_slot.insert(sym.id.clone(), idx + 1);
    }

    let mut char_entries_js = Vec::new();
    let mut sym_entries_js = Vec::new();
    let mut all_cids: Vec<u16> = ctx
        .char_symbols
        .keys()
        .chain(ctx.char_edit_texts.keys())
        .chain(ctx.char_morphs.keys())
        .copied()
        .collect();
    all_cids.sort_unstable();
    all_cids.dedup();

    for cid in all_cids {
        let kind_str = if ctx.char_button_records.contains_key(&cid)
            || ctx
                .char_symbols
                .get(&cid)
                .and_then(|sid| ctx.symbols.iter().find(|s| &s.id == sid))
                .is_some_and(|s| s.kind == SymbolKind::Button)
        {
            "button"
        } else if ctx.char_sprite_frame0_children.contains_key(&cid) {
            "movie_clip"
        } else if ctx.char_edit_texts.contains_key(&cid) {
            "text"
        } else if ctx.char_bitmaps.contains_key(&cid) {
            "bitmap"
        } else if ctx.char_morphs.contains_key(&cid) {
            "morph"
        } else {
            "shape"
        };
        let cls_js = valid_char_classes
            .get(&cid)
            .map_or_else(|| "null".to_owned(), |c| js_string_literal(c));
        let btn_js = ctx
            .char_button_records
            .get(&cid)
            .map(|recs| {
                recs.iter()
                    .map(|&(m, rc)| format!("[{m},{rc}]"))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let f0_js = ctx
            .char_sprite_frame0_children
            .get(&cid)
            .map(|kids| {
                kids.iter()
                    .map(|(kcid, knm)| format!("[{kcid},{}]", js_string_literal(knm)))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let entry_obj = format!(
            "{{ cid: {cid}, kind: '{kind_str}', className: {cls_js}, btnRecords: [{btn_js}], f0Kids: [{f0_js}] }}"
        );
        char_entries_js.push(format!("__nfSwfEnv.charTable[{cid}] = {entry_obj};"));
        if let Some(sid) = ctx.char_symbols.get(&cid)
            && let Some(&slot) = sym_id_to_mounted_slot.get(sid)
        {
            sym_entries_js.push(format!(
                "__nfSwfEnv.symTable['s{slot}'] = __nfSwfEnv.charTable[{cid}];"
            ));
        }
    }
    // Also map morph ratio symbols (`morph_ratio_symbols`) to their morph character info
    for (&(mcid, _), msid) in &ctx.morph_ratio_symbols {
        if let Some(&slot) = sym_id_to_mounted_slot.get(msid) {
            sym_entries_js.push(format!(
                "if (__nfSwfEnv.charTable[{mcid}]) __nfSwfEnv.symTable['s{slot}'] = __nfSwfEnv.charTable[{mcid}];"
            ));
        }
    }

    // 3. Emit bundle-level AVM2 ABC registry (`avm2-abc-init`)
    let mut init_blocks = Vec::new();
    for (abc_idx, abc) in abc_list.iter().enumerate() {
        if let Some(block_js) = compile_abc_module_js(ctx, abc_idx, abc) {
            init_blocks.push(block_js);
        }
    }

    let mut meta_js = String::from(
        "(() => {\nif (!__nfSwfEnv || !__nfSwfEnv.abcs) return;\n__nfSwfEnv.rootClip = this;\n",
    );
    for line in char_entries_js {
        meta_js.push_str(&line);
        meta_js.push('\n');
    }
    for line in sym_entries_js {
        meta_js.push_str(&line);
        meta_js.push('\n');
    }
    let mut standalone_inits = Vec::new();
    if let Some((abc_idx, cls_qname)) = &root_doc_target {
        let cls_lit = js_string_literal(cls_qname);
        meta_js.push_str(&format!(
            "__nfSwfEnv.rootDocTarget = {{ abcIdx: {abc_idx}, clsName: {cls_lit} }};\n"
        ));
    } else {
        for (abc_idx, abc) in abc_list.iter().enumerate() {
            if abc.instances.is_empty()
                && let Some(&script_init) = abc.script_inits.last()
                && abc.bodies.contains_key(&script_init)
            {
                standalone_inits.push((abc_idx, script_init));
                meta_js.push_str(&format!(
                    "__nfSwfEnv.standaloneInits.push([{abc_idx}, {script_init}]);\n"
                ));
            }
        }
    }
    meta_js.push_str("})();\n");
    init_blocks.push(meta_js);

    if !init_blocks.is_empty() {
        let source_id = format!("/*swf-{}-avm2-abc-init*/", ctx.mount_seq);
        ctx.scripts.push(super::CompiledSwfScriptUnit {
            language: neoflash_core::ScriptLanguage::JavaScript,
            source: source_id,
            code: init_blocks.join("\n"),
            location: "avm2-abc-init".into(),
        });
    }

    // 4. Attach idempotent sprite `addFrameScript` fallback entries
    for (sym_id, frame_idx, cls_name) in sprite_frame_attachments {
        let cls_lit = js_string_literal(&cls_name);
        let call_js = format!(
            "if (__nfSwfEnv && typeof __nfSwfEnv.onSpriteFrameEntry === 'function') __nfSwfEnv.onSpriteFrameEntry(this, {frame_idx}, {cls_lit});\n"
        );
        let mut sym_tl = ctx
            .symbols
            .iter()
            .find(|s| s.id == sym_id)
            .map(|s| s.timeline.clone());
        if let Some(mut tl) = sym_tl.take() {
            attach_compiled_frame_script(
                ctx,
                &mut tl,
                frame_idx,
                &call_js,
                "avm2-sprite-frame",
                neoflash_core::ScriptLanguage::ActionScript3,
            );
            if let Some(sym) = ctx.symbols.iter_mut().find(|s| s.id == sym_id) {
                sym.timeline = tl;
            }
        }
    }

    // 5. Attach root Document Class or standalone script_init FrameScript ONLY when present
    if let Some((_, cls_qname)) = root_doc_target {
        attach_compiled_frame_script(
            ctx,
            root_timeline,
            0,
            "if (__nfSwfEnv && typeof __nfSwfEnv.bootAvm2Mount === 'function') __nfSwfEnv.bootAvm2Mount(this);\n",
            "avm2-doc-ctor",
            neoflash_core::ScriptLanguage::ActionScript3,
        );
        Some(cls_qname)
    } else {
        if !standalone_inits.is_empty() {
            attach_compiled_frame_script(
                ctx,
                root_timeline,
                0,
                "if (__nfSwfEnv && typeof __nfSwfEnv.bootAvm2Mount === 'function') __nfSwfEnv.bootAvm2Mount(this);\n",
                "avm2-script",
                neoflash_core::ScriptLanguage::ActionScript3,
            );
        }
        None
    }
}

fn compile_abc_module_js(ctx: &mut SwfDecodeCtx, abc_idx: usize, abc: &AbcFile) -> Option<String> {
    let mut out = String::from("(() => {\n");
    out.push_str(
        "if (!__nfSwfEnv) return;\n\
if (!__nfSwfEnv.abcs) {\n\
  __nfSwfEnv.abcs = Object.create(null);\n\
  __nfSwfEnv.classes = Object.create(null);\n\
  __nfSwfEnv.globals = Object.create(null);\n\
  __nfSwfEnv.symTable = Object.create(null);\n\
  __nfSwfEnv.charTable = Object.create(null);\n\
  __nfSwfEnv.rootDocTarget = null;\n\
  __nfSwfEnv.standaloneInits = [];\n\
  __nfSwfEnv.rootClip = null;\n\
  __nfSwfEnv.booted = false;\n\
  __nfSwfEnv.internalDepth = 0;\n\
  __nfSwfEnv.nextInstId = 0;\n\
  __nfSwfEnv.docAttached = new WeakSet();\n\
  __nfSwfEnv.entryOf = new WeakMap();\n\
  __nfSwfEnv.classOf = new WeakMap();\n\
  __nfSwfEnv.preSuper = new WeakSet();\n\
  __nfSwfEnv.superConstructed = new WeakSet();\n\
  __nfSwfEnv.constructingNow = new WeakSet();\n\
  __nfSwfEnv.constructingChildrenOf = new WeakSet();\n\
  __nfSwfEnv.pendingDisplay = new WeakSet();\n\
  __nfSwfEnv.constructedDisplay = new WeakSet();\n\
  __nfSwfEnv.explicitNames = new WeakMap();\n\
  __nfSwfEnv.assignedChildProps = new WeakMap();\n\
  __nfSwfEnv.synthParent = new WeakMap();\n\
  __nfSwfEnv.synthF0Kids = new WeakMap();\n\
  __nfSwfEnv.frameScripts = new WeakMap();\n\
  __nfSwfEnv.lastScriptFrame = new WeakMap();\n\
  __nfSwfEnv.evListeners = new WeakMap();\n\
  __nfSwfEnv.trackedClips = [];\n\
  __nfSwfEnv.stageProxy = null;\n\
  __nfSwfEnv.callDepth = 0;\n\
  const __strCtor = __nfSwfFn(v => String(v ?? ''));\n\
  __nfSwfSet(__strCtor, 'fromCharCode', __nfSwfFn((...codes) => String.fromCharCode(...codes.map(c => Number(c) & 0xffff))));\n\
  const __mkEventObj = (type, bubbles = false, cancelable = false) => {\n\
    const ev = Object.create(null);\n\
    ev.type = String(type ?? '');\n\
    ev.bubbles = Boolean(bubbles);\n\
    ev.cancelable = Boolean(cancelable);\n\
    ev.target = null;\n\
    ev.currentTarget = null;\n\
    ev.eventPhase = 2;\n\
    let stopped = false, immStopped = false, prevented = false;\n\
    __nfSwfSet(ev, 'stopPropagation', __nfSwfFn(() => { stopped = true; }));\n\
    __nfSwfSet(ev, 'stopImmediatePropagation', __nfSwfFn(() => { stopped = true; immStopped = true; }));\n\
    __nfSwfSet(ev, 'preventDefault', __nfSwfFn(() => { if (ev.cancelable) prevented = true; }));\n\
    __nfSwfSet(ev, 'isDefaultPrevented', __nfSwfFn(() => prevented));\n\
    const fmt = () => '[Event type=\"' + ev.type + '\" bubbles=' + ev.bubbles + ' cancelable=' + ev.cancelable + ' eventPhase=' + ev.eventPhase + ']';\n\
    __nfSwfSet(ev, 'toString', __nfSwfFn(fmt));\n\
    ev.__nfEvFmt = fmt;\n\
    ev.__nfState = () => ({ stopped, immStopped });\n\
    if (__evCtor) __nfSwfTagInstance(ev, __evCtor);\n\
    return ev;\n\
  };\n\
  let __evCtor = null;\n\
  const __mkEventCtor = (qname, shortName, consts) => {\n\
    const ctor = __nfSwfFn(function(type, bubbles = false, cancelable = false) {\n\
      return __mkEventObj(type, bubbles, cancelable);\n\
    });\n\
    for (const [ck, cv] of Object.entries(consts)) __nfSwfSet(ctor, ck, cv);\n\
    __nfSwfSetClassMeta(ctor, qname, shortName, shortName === 'Event' ? 'Object' : 'Event');\n\
    return ctor;\n\
  };\n\
  __evCtor = __mkEventCtor('flash.events.Event', 'Event', {\n\
    ADDED: 'added', ADDED_TO_STAGE: 'addedToStage', REMOVED: 'removed', REMOVED_FROM_STAGE: 'removedFromStage',\n\
    ENTER_FRAME: 'enterFrame', FRAME_CONSTRUCTED: 'frameConstructed', EXIT_FRAME: 'exitFrame',\n\
    COMPLETE: 'complete', INIT: 'init', OPEN: 'open', RENDER: 'render', UNLOAD: 'unload',\n\
    CHANGE: 'change', SELECT: 'select', ACTIVATE: 'activate', DEACTIVATE: 'deactivate'\n\
  });\n\
  const __mouseEvCtor = __mkEventCtor('flash.events.MouseEvent', 'MouseEvent', {\n\
    CLICK: 'click', DOUBLE_CLICK: 'doubleClick', MOUSE_DOWN: 'mouseDown', MOUSE_UP: 'mouseUp',\n\
    MOUSE_MOVE: 'mouseMove', MOUSE_OVER: 'mouseOver', MOUSE_OUT: 'mouseOut',\n\
    ROLL_OVER: 'rollOver', ROLL_OUT: 'rollOut', MOUSE_WHEEL: 'mouseWheel', RELEASE_OUTSIDE: 'releaseOutside'\n\
  });\n\
  const __keyEvCtor = __mkEventCtor('flash.events.KeyboardEvent', 'KeyboardEvent', {\n\
    KEY_DOWN: 'keyDown', KEY_UP: 'keyUp'\n\
  });\n\
  __nfSwfEnv.builtins = Object.assign(Object.create(null), {\n\
    Object: 'Object', Array: 'Array', String: __strCtor,\n\
    Number: __nfSwfFn(v => Number(v)), Boolean: __nfSwfFn(v => Boolean(v)),\n\
    int: __nfSwfFn(v => Number(v) | 0), uint: __nfSwfFn(v => Number(v) >>> 0),\n\
    parseInt: __nfSwfFn((s, r) => parseInt(s, r)), parseFloat: __nfSwfFn(s => parseFloat(s)),\n\
    isNaN: __nfSwfFn(v => Number.isNaN(Number(v))), isFinite: __nfSwfFn(v => Number.isFinite(Number(v))),\n\
    Math: Math,\n\
    Event: __evCtor, MouseEvent: __mouseEvCtor, KeyboardEvent: __keyEvCtor,\n\
    Error: 'Error', TypeError: 'TypeError', RangeError: 'RangeError',\n\
    ReferenceError: 'ReferenceError', ArgumentError: 'ArgumentError',\n\
    SyntaxError: 'SyntaxError', URIError: 'URIError', EvalError: 'EvalError', SecurityError: 'SecurityError', VerifyError: 'VerifyError',\n\
    MovieClip: 'MovieClip', Sprite: 'Sprite', SimpleButton: 'SimpleButton', Shape: 'Shape',\n\
    TextField: 'TextField', Bitmap: 'Bitmap', Stage: 'Stage',\n\
    DisplayObjectContainer: 'DisplayObjectContainer', InteractiveObject: 'InteractiveObject',\n\
    DisplayObject: 'DisplayObject', EventDispatcher: 'EventDispatcher', Function: 'Function',\n\
    fscommand: __nfSwfFn((c, a) => __nfSwfGetURL(this, 'FSCommand:' + String(c ?? ''), String(a ?? ''), 0)),\n\
    navigateToURL: __nfSwfFn((req, tgt) => { const u = req && typeof req === 'object' ? __nfSwfGet(req, 'url') : req; __nfSwfGetURL(this, u, tgt, 0); }),\n\
  });\n\
  __nfSwfEnv.nextInstanceName = () => 'instance' + (++__nfSwfEnv.nextInstId);\n\
  __nfSwfEnv.getSymInfo = proxy => {\n\
    if (!proxy || !__nfDisplay?.alive?.(proxy)) return null;\n\
    const desc = __nfDisplay.descriptor(proxy);\n\
    const sid = desc?.symbol_id || '';\n\
    if (!sid) return null;\n\
    const key = (__nfSwfEnv.prefix && sid.startsWith(__nfSwfEnv.prefix)) ? sid.slice(__nfSwfEnv.prefix.length) : sid;\n\
    return __nfSwfEnv.symTable[key] || null;\n\
  };\n\
  __nfSwfEnv.getStageProxy = () => {\n\
    if (__nfSwfEnv.stageProxy) return __nfSwfEnv.stageProxy;\n\
    const realStage = __nfDisplay ? (typeof __nfDisplay.stageContainer === 'function' ? __nfDisplay.stageContainer() : __nfDisplay.stageContainer) : null;\n\
    const sp = Object.create(null);\n\
    __nfSwfEnv.classOf.set(sp, 'Stage');\n\
    __nfSwfEnv.stageProxy = sp;\n\
    const sEntry = { getters: Object.create(null), setters: Object.create(null) };\n\
    sEntry.getters.numChildren = __nfSwfFn(() => __nfSwfEnv.rootClip && __nfDisplay?.alive?.(__nfSwfEnv.rootClip) ? 1 : 0);\n\
    for (const p of ['stageWidth', 'stageHeight', 'frameRate', 'scaleMode', 'align', 'quality', 'color', 'focus']) {\n\
      sEntry.getters[p] = __nfSwfFn(() => realStage ? __nfSwfGet(realStage, p) : undefined);\n\
      sEntry.setters[p] = __nfSwfFn(v => { if (realStage) __nfSwfSet(realStage, p, v); });\n\
    }\n\
    __nfSwfEnv.entryOf.set(sp, sEntry);\n\
    __nfSwfSet(sp, 'getChildAt', __nfSwfFn(idx => {\n\
      if ((idx | 0) === 0 && __nfSwfEnv.rootClip && __nfDisplay?.alive?.(__nfSwfEnv.rootClip)) return __nfSwfEnv.rootClip;\n\
      throw new RangeError('Child index is outside Stage.');\n\
    }));\n\
    __nfSwfSet(sp, 'getChildByName', __nfSwfFn(nm => {\n\
      if (__nfSwfEnv.rootClip && __nfDisplay?.alive?.(__nfSwfEnv.rootClip) && __nfSwfGet(__nfSwfEnv.rootClip, 'name') === String(nm)) return __nfSwfEnv.rootClip;\n\
      return null;\n\
    }));\n\
    return sp;\n\
  };\n\
  __nfSwfEnv.isUnconstructedChild = v => {\n\
    if (!v || (typeof v !== 'object' && typeof v !== 'function')) return false;\n\
    if (__nfSwfEnv.pendingDisplay.has(v)) return true;\n\
    if (__nfSwfEnv.constructedDisplay.has(v)) return false;\n\
    const sinfo = __nfSwfEnv.getSymInfo(v);\n\
    if (sinfo && sinfo.className && __nfSwfEnv.hasClass(sinfo.className)) return true;\n\
    return false;\n\
  };\n\
  __nfSwfEnv.toStr = v => {\n\
    if (v === undefined) return 'undefined';\n\
    if (v === null) return 'null';\n\
    if (typeof v === 'string') return v;\n\
    if (typeof v === 'number' || typeof v === 'boolean') return String(v);\n\
    if (Array.isArray(v)) return v.map(x => x == null ? '' : __nfSwfEnv.toStr(x)).join(',');\n\
    if (typeof v === 'object' || typeof v === 'function') {\n\
      if (typeof v.__nfEvFmt === 'function') return v.__nfEvFmt();\n\
      const ent = __nfSwfEnv.entryOf.get(v);\n\
      if (ent && ent.hasCustomToString) return String(__nfSwfCall(v, 'toString', []));\n\
      const cname = __nfSwfEnv.classOf.get(v);\n\
      if (cname) return '[object ' + cname + ']';\n\
      if (__nfDisplay && typeof __nfDisplay.alive === 'function' && __nfDisplay.alive(v)) {\n\
        const sinfo = __nfSwfEnv.getSymInfo(v);\n\
        if (sinfo?.kind === 'bitmap') return '[object Bitmap]';\n\
        if (sinfo?.kind === 'morph') return '[object MorphShape]';\n\
        const own = __nfDisplay.ownerOf?.(v);\n\
        if (own === 'movie_clip') return '[object MovieClip]';\n\
        if (own === 'button') return '[object SimpleButton]';\n\
        if (own === 'text') return '[object TextField]';\n\
        if (own === 'bitmap') return '[object Bitmap]';\n\
        return '[object Shape]';\n\
      }\n\
      if (__nfDisplay && v === (typeof __nfDisplay.stageContainer === 'function' ? __nfDisplay.stageContainer() : __nfDisplay.stageContainer)) return '[object Stage]';\n\
    }\n\
    return String(v);\n\
  };\n\
  __nfSwfEnv.avm2Get = (obj, prop) => {\n\
    if (obj == null) return undefined;\n\
    const k = String(prop);\n\
    if (k === 'stage' && __nfDisplay?.alive?.(obj)) {\n\
      return __nfDisplay.descriptor(obj)?.on_stage === false ? null : __nfSwfEnv.getStageProxy();\n\
    }\n\
    if (k === 'root' && __nfDisplay?.alive?.(obj)) {\n\
      return __nfDisplay.descriptor(obj)?.on_stage === false ? null : __nfSwfEnv.rootClip;\n\
    }\n\
    if (k === 'parent' && obj === __nfSwfEnv.rootClip) {\n\
      return __nfSwfEnv.getStageProxy();\n\
    }\n\
    const ent = (typeof obj === 'object' || typeof obj === 'function') ? __nfSwfEnv.entryOf.get(obj) : null;\n\
    if (ent) {\n\
      const gf = ent.getters[k];\n\
      if (typeof gf === 'function') return __nfSwfApply(gf, obj, []);\n\
    }\n\
    if ((typeof obj === 'object' || typeof obj === 'function') && __nfSwfEnv.preSuper.has(obj) && !__nfSwfHasOwn(obj, k)) {\n\
      if (!['stage','parent','root','name','x','y','scaleX','scaleY','alpha','visible','rotation','currentFrame','totalFrames','graphics'].includes(k)) {\n\
        if (__nfDisplay?.alive?.(obj) && __nfSwfCall(obj, 'getChildByName', [k]) != null) return undefined;\n\
      }\n\
    }\n\
    const raw = __nfSwfGet(obj, k);\n\
    if (__nfSwfEnv.isUnconstructedChild(raw)) return undefined;\n\
    return raw;\n\
  };\n\
  __nfSwfEnv.avm2Set = (obj, prop, val) => {\n\
    if (obj == null) return val;\n\
    const k = String(prop);\n\
    const ent = (typeof obj === 'object' || typeof obj === 'function') ? __nfSwfEnv.entryOf.get(obj) : null;\n\
    if (ent) {\n\
      const sf = ent.setters[k];\n\
      if (typeof sf === 'function') { __nfSwfApply(sf, obj, [val]); return val; }\n\
    }\n\
    return __nfSwfSet(obj, k, val);\n\
  };\n\
  const __avm2LifecycleTypes = new Set(['added', 'addedToStage', 'removed', 'removedFromStage', 'frameConstructed', 'exitFrame', 'enterFrame']);\n\
  __nfSwfEnv.addAvm2Listener = (target, type, fn, useCapture = false) => {\n\
    if (!target || typeof fn !== 'function') return;\n\
    const t = String(type ?? ''), cap = Boolean(useCapture);\n\
    let byType = __nfSwfEnv.evListeners.get(target);\n\
    if (!byType) { byType = Object.create(null); __nfSwfEnv.evListeners.set(target, byType); }\n\
    let list = byType[t];\n\
    if (!list) { list = []; byType[t] = list; }\n\
    if (!list.some(it => it.fn === fn && it.cap === cap) && list.length < 256) list.push({ fn, cap });\n\
  };\n\
  __nfSwfEnv.removeAvm2Listener = (target, type, fn, useCapture = false) => {\n\
    const byType = target ? __nfSwfEnv.evListeners.get(target) : null;\n\
    const list = byType ? byType[String(type ?? '')] : null;\n\
    if (!list) return;\n\
    const cap = Boolean(useCapture);\n\
    const idx = list.findIndex(it => it.fn === fn && it.cap === cap);\n\
    if (idx >= 0) list.splice(idx, 1);\n\
  };\n\
  __nfSwfEnv.hasAvm2Listener = (target, type) => {\n\
    const byType = target ? __nfSwfEnv.evListeners.get(target) : null;\n\
    const list = byType ? byType[String(type ?? '')] : null;\n\
    return Boolean(list && list.length > 0);\n\
  };\n\
  __nfSwfEnv.parentOfTarget = cur => {\n\
    if (!cur || cur === __nfSwfEnv.stageProxy) return null;\n\
    if (__nfSwfEnv.synthParent.has(cur)) return __nfSwfEnv.synthParent.get(cur) || null;\n\
    if (cur === __nfSwfEnv.rootClip) {\n\
      return (__nfDisplay?.alive?.(cur) && __nfDisplay.descriptor(cur)?.on_stage !== false) ? __nfSwfEnv.getStageProxy() : null;\n\
    }\n\
    if (__nfDisplay?.alive?.(cur)) {\n\
      try { return __nfDisplay.parentSpace(cur) || null; } catch (_) { return null; }\n\
    }\n\
    return null;\n\
  };\n\
  __nfSwfEnv.dispatchAvm2EventObj = (target, ev) => {\n\
    if (!target || !ev) return true;\n\
    const ancestors = [];\n\
    let p = __nfSwfEnv.parentOfTarget(target), guard = 0;\n\
    while (p && guard++ < 64) {\n\
      ancestors.push(p);\n\
      if (p === __nfSwfEnv.stageProxy) break;\n\
      p = __nfSwfEnv.parentOfTarget(p);\n\
    }\n\
    ev.target = target;\n\
    const fireOn = (node, phase, wantCap) => {\n\
      const byType = __nfSwfEnv.evListeners.get(node);\n\
      const list = byType ? byType[ev.type] : null;\n\
      if (!list || !list.length) return;\n\
      ev.currentTarget = node;\n\
      ev.eventPhase = phase;\n\
      for (const item of [...list]) {\n\
        if (item.cap !== wantCap) continue;\n\
        __nfSwfApply(item.fn, node, [ev]);\n\
        if (ev.__nfState?.().immStopped) break;\n\
      }\n\
    };\n\
    for (let i = ancestors.length - 1; i >= 0; i--) {\n\
      fireOn(ancestors[i], 1, true);\n\
      if (ev.__nfState?.().stopped) return true;\n\
    }\n\
    fireOn(target, 2, false);\n\
    if (ev.__nfState?.().stopped || !ev.bubbles) return true;\n\
    for (let i = 0; i < ancestors.length; i++) {\n\
      fireOn(ancestors[i], 3, false);\n\
      if (ev.__nfState?.().stopped) break;\n\
    }\n\
    return true;\n\
  };\n\
  __nfSwfEnv.dispatchAvm2Event = (target, type, bubbles = false) => {\n\
    return __nfSwfEnv.dispatchAvm2EventObj(target, __mkEventObj(type, bubbles, false));\n\
  };\n\
  __nfSwfEnv.registerFrameScripts = (target, args) => {\n\
    if (!target || (typeof target !== 'object' && typeof target !== 'function')) return;\n\
    let map = __nfSwfEnv.frameScripts.get(target);\n\
    if (!map) { map = new Map(); __nfSwfEnv.frameScripts.set(target, map); }\n\
    for (let i = 0; i + 1 < args.length; i += 2) {\n\
      const fIdx = Number(args[i]) | 0;\n\
      const fn = args[i + 1];\n\
      if (fIdx < 0 || fIdx > 16000) continue;\n\
      if (typeof fn === 'function') map.set(fIdx, fn);\n\
      else map.delete(fIdx);\n\
    }\n\
  };\n\
  __nfSwfEnv.trackClip = clip => {\n\
    if (clip && !__nfSwfEnv.trackedClips.includes(clip)) __nfSwfEnv.trackedClips.push(clip);\n\
  };\n\
  __nfSwfEnv.pruneTracked = () => {\n\
    __nfSwfEnv.trackedClips = __nfSwfEnv.trackedClips.filter(c => __nfDisplay?.alive?.(c));\n\
  };\n\
  __nfSwfEnv.broadcastPhase = phaseType => {\n\
    __nfSwfEnv.pruneTracked();\n\
    for (const clip of [...__nfSwfEnv.trackedClips]) {\n\
      if (__nfDisplay?.alive?.(clip) && __nfSwfEnv.hasAvm2Listener(clip, phaseType)) {\n\
        __nfSwfEnv.dispatchAvm2EventObj(clip, __mkEventObj(phaseType, false, false));\n\
      }\n\
    }\n\
    if (__nfSwfEnv.stageProxy && __nfSwfEnv.hasAvm2Listener(__nfSwfEnv.stageProxy, phaseType)) {\n\
      __nfSwfEnv.dispatchAvm2EventObj(__nfSwfEnv.stageProxy, __mkEventObj(phaseType, false, false));\n\
    }\n\
  };\n\
  __nfSwfEnv.runPendingFrameScripts = () => {\n\
    __nfSwfEnv.pruneTracked();\n\
    for (const clip of [...__nfSwfEnv.trackedClips]) {\n\
      if (!__nfDisplay?.alive?.(clip)) continue;\n\
      const curF = ((Number(__nfSwfGet(clip, 'currentFrame')) || 1) - 1) | 0;\n\
      if (__nfSwfEnv.lastScriptFrame.get(clip) === curF) continue;\n\
      __nfSwfEnv.lastScriptFrame.set(clip, curF);\n\
      const map = __nfSwfEnv.frameScripts.get(clip);\n\
      const fn = map ? map.get(curF) : null;\n\
      if (typeof fn === 'function') __nfSwfApply(fn, clip, []);\n\
    }\n\
  };\n\
  __nfSwfEnv.runFramePhase = includeExit => {\n\
    __nfSwfEnv.broadcastPhase('frameConstructed');\n\
    __nfSwfEnv.runPendingFrameScripts();\n\
    if (includeExit) __nfSwfEnv.broadcastPhase('exitFrame');\n\
  };\n\
  __nfSwfEnv.constructSynthChar = (cid, parentTarget, dispatchAddedOnSelf) => {\n\
    const cinfo = __nfSwfEnv.charTable[cid] || null;\n\
    const sobj = Object.create(null);\n\
    const defName = __nfSwfEnv.nextInstanceName();\n\
    __nfSwfSet(sobj, 'name', defName);\n\
    __nfSwfSet(sobj, 'x', 0); __nfSwfSet(sobj, 'y', 0);\n\
    __nfSwfSet(sobj, 'scaleX', 1); __nfSwfSet(sobj, 'scaleY', 1);\n\
    __nfSwfSet(sobj, 'alpha', 1); __nfSwfSet(sobj, 'visible', true); __nfSwfSet(sobj, 'rotation', 0);\n\
    __nfSwfEnv.synthParent.set(sobj, parentTarget);\n\
    const shortKind = cinfo?.className ? cinfo.className.split('.').pop() : (cinfo?.kind === 'movie_clip' ? 'MovieClip' : cinfo?.kind === 'button' ? 'SimpleButton' : cinfo?.kind === 'text' ? 'TextField' : cinfo?.kind === 'bitmap' ? 'Bitmap' : 'Shape');\n\
    __nfSwfEnv.classOf.set(sobj, shortKind);\n\
    if (cinfo?.kind === 'movie_clip') {\n\
      __nfSwfEnv.synthF0Kids.set(sobj, cinfo.f0Kids || []);\n\
      if (cinfo.className && __nfSwfEnv.hasClass(cinfo.className)) {\n\
        __nfSwfEnv.attachClassToInstance(cinfo.className, sobj, []);\n\
      } else {\n\
        __nfSwfEnv.superConstruct(sobj);\n\
      }\n\
    } else if (cinfo?.kind === 'button') {\n\
      __nfSwfEnv.constructButtonNode(sobj, cinfo);\n\
    }\n\
    __nfSwfEnv.constructedDisplay.add(sobj);\n\
    if (dispatchAddedOnSelf) __nfSwfEnv.dispatchAvm2Event(sobj, 'added', true);\n\
    return sobj;\n\
  };\n\
  __nfSwfEnv.constructButtonNode = (btnTarget, symInfo) => {\n\
    const recs = symInfo?.btnRecords || [];\n\
    const instantiated = [];\n\
    for (const [stateMask, recCid] of recs) {\n\
      const recObj = __nfSwfEnv.constructSynthChar(recCid, btnTarget, false);\n\
      instantiated.push([stateMask, recObj]);\n\
    }\n\
    let firstUp = null;\n\
    for (const [stateMask, recObj] of instantiated) {\n\
      if ((stateMask & 1) !== 0) { if (!firstUp) firstUp = recObj; }\n\
      else __nfSwfEnv.synthParent.set(recObj, null);\n\
    }\n\
    if (firstUp) {\n\
      __nfSwfEnv.dispatchAvm2Event(firstUp, 'added', true);\n\
      __nfSwfEnv.dispatchAvm2Event(firstUp, 'addedToStage', false);\n\
    }\n\
    __nfSwfEnv.runFramePhase(true);\n\
    if (symInfo?.className && __nfSwfEnv.hasClass(symInfo.className)) {\n\
      __nfSwfEnv.attachClassToInstance(symInfo.className, btnTarget, []);\n\
    }\n\
  };\n\
  __nfSwfEnv.superConstruct = target => {\n\
    if (!target || (typeof target !== 'object' && typeof target !== 'function')) return;\n\
    if (__nfSwfEnv.superConstructed.has(target)) return;\n\
    __nfSwfEnv.superConstructed.add(target);\n\
    __nfSwfEnv.preSuper.delete(target);\n\
    if (__nfSwfEnv.synthF0Kids.has(target)) {\n\
      const f0 = __nfSwfEnv.synthF0Kids.get(target) || [];\n\
      for (const [kcid, knm] of f0) {\n\
        const sk = __nfSwfEnv.constructSynthChar(kcid, target, true);\n\
        if (knm) { __nfSwfSet(sk, 'name', knm); __nfSwfSet(target, knm, sk); }\n\
      }\n\
      return;\n\
    }\n\
    __nfSwfEnv.constructChildren(target);\n\
  };\n\
  __nfSwfEnv.constructChildren = parentProxy => {\n\
    if (!parentProxy || !__nfDisplay?.alive?.(parentProxy)) return;\n\
    if (__nfSwfEnv.constructingChildrenOf.has(parentProxy)) return;\n\
    const pDesc = __nfDisplay.descriptor(parentProxy);\n\
    if (!pDesc || (pDesc.owner !== 'movie_clip' && pDesc.owner !== 'sprite')) return;\n\
    __nfSwfEnv.constructingChildrenOf.add(parentProxy);\n\
    try {\n\
    const rawKids = __nfDisplay.children(parentProxy) || [];\n\
    const unconstructed = [];\n\
    for (const knode of rawKids) {\n\
      const kproxy = __nfDisplay.nodeProxy(knode);\n\
      if (!kproxy || __nfSwfEnv.constructedDisplay.has(kproxy) || __nfSwfEnv.constructingNow.has(kproxy)) continue;\n\
      if (!knode.name) {\n\
        const defName = __nfSwfEnv.nextInstanceName();\n\
        try { __nfSwfSet(kproxy, 'name', defName); } catch (_) {}\n\
      } else {\n\
        __nfSwfEnv.explicitNames.set(kproxy, knode.name);\n\
      }\n\
      __nfSwfEnv.pendingDisplay.add(kproxy);\n\
      unconstructed.push({ knode, kproxy });\n\
    }\n\
    for (const { knode, kproxy } of unconstructed) {\n\
      if (!__nfDisplay.alive(kproxy)) { __nfSwfEnv.pendingDisplay.delete(kproxy); continue; }\n\
      __nfSwfEnv.constructingNow.add(kproxy);\n\
      __nfSwfEnv.trackClip(kproxy);\n\
      const symInfo = __nfSwfEnv.getSymInfo(kproxy);\n\
      try {\n\
        if (knode.owner === 'button' || symInfo?.kind === 'button') {\n\
          __nfSwfEnv.constructButtonNode(kproxy, symInfo);\n\
        } else if (symInfo?.className && __nfSwfEnv.hasClass(symInfo.className)) {\n\
          __nfSwfEnv.attachClassToInstance(symInfo.className, kproxy, []);\n\
        } else {\n\
          __nfSwfEnv.superConstruct(kproxy);\n\
        }\n\
      } finally {\n\
        __nfSwfEnv.constructingNow.delete(kproxy);\n\
        __nfSwfEnv.pendingDisplay.delete(kproxy);\n\
        __nfSwfEnv.constructedDisplay.add(kproxy);\n\
      }\n\
      const expName = __nfSwfEnv.explicitNames.get(kproxy);\n\
      if (expName && __nfDisplay.alive(parentProxy)) {\n\
        __nfSwfSet(parentProxy, expName, kproxy);\n\
        let assigned = __nfSwfEnv.assignedChildProps.get(parentProxy);\n\
        if (!assigned) { assigned = new Map(); __nfSwfEnv.assignedChildProps.set(parentProxy, assigned); }\n\
        assigned.set(expName, kproxy);\n\
      }\n\
      if (__nfDisplay.alive(kproxy)) {\n\
        __nfSwfEnv.dispatchAvm2Event(kproxy, 'added', true);\n\
        if (__nfDisplay.descriptor(kproxy)?.on_stage !== false) {\n\
          __nfSwfEnv.dispatchAvm2Event(kproxy, 'addedToStage', false);\n\
        }\n\
      }\n\
    }\n\
    } finally { __nfSwfEnv.constructingChildrenOf.delete(parentProxy); }\n\
  };\n\
  __nfSwfEnv.pruneRetiredChildren = container => {\n\
    const assigned = container ? __nfSwfEnv.assignedChildProps.get(container) : null;\n\
    if (!assigned) return;\n\
    for (const [nm, childProxy] of [...assigned.entries()]) {\n\
      if (!__nfDisplay?.alive?.(childProxy)) {\n\
        assigned.delete(nm);\n\
        if (__nfSwfGet(container, nm) === childProxy) __nfSwfDelete(container, nm);\n\
      }\n\
    }\n\
  };\n\
  __nfSwfEnv.scanAndConstruct = container => {\n\
    if (!container || !__nfDisplay?.alive?.(container)) return;\n\
    const desc = __nfDisplay.descriptor(container);\n\
    if (!desc || (desc.owner !== 'movie_clip' && desc.owner !== 'sprite')) return;\n\
    __nfSwfEnv.pruneRetiredChildren(container);\n\
    if (__nfSwfEnv.superConstructed.has(container) && !__nfSwfEnv.constructingChildrenOf.has(container)) {\n\
      __nfSwfEnv.constructChildren(container);\n\
    }\n\
    for (const knode of (__nfDisplay.children(container) || [])) {\n\
      const kp = __nfDisplay.nodeProxy(knode);\n\
      if (kp && (__nfSwfEnv.constructedDisplay.has(kp) || __nfSwfEnv.superConstructed.has(kp))) __nfSwfEnv.scanAndConstruct(kp);\n\
    }\n\
  };\n\
  __nfSwfEnv.avm2Goto = (clip, method, args) => {\n\
    if (!clip || !__nfDisplay?.alive?.(clip)) return undefined;\n\
    __nfSwfEnv.internalDepth++;\n\
    try {\n\
      const prevF = __nfSwfGet(clip, 'currentFrame');\n\
      __nfSwfEnv.inHostNav = true;\n\
      try { __nfSwfCall(clip, method, args); } finally { __nfSwfEnv.inHostNav = false; }\n\
      const nextF = __nfSwfGet(clip, 'currentFrame');\n\
      if (nextF === prevF) return undefined;\n\
      __nfSwfEnv.scanAndConstruct(__nfSwfEnv.rootClip || clip);\n\
      __nfSwfEnv.runFramePhase(true);\n\
      return undefined;\n\
    } finally {\n\
      __nfSwfEnv.internalDepth--;\n\
    }\n\
  };\n\
  __nfSwfEnv.avm2Call = (rawRecv, methodName, args, curThis) => {\n\
    const m = String(methodName ?? '');\n\
    const isGlobalRecv = rawRecv == null || rawRecv === __nfSwfEnv.globals || rawRecv === __nfSwfEnv.builtins;\n\
    if (m === 'addFrameScript') {\n\
      const tgt = isGlobalRecv ? curThis : rawRecv;\n\
      __nfSwfEnv.registerFrameScripts(tgt, args);\n\
      return undefined;\n\
    }\n\
    if (m === 'stop' || m === 'play') {\n\
      const tgt = isGlobalRecv ? ((curThis && __nfDisplay?.alive?.(curThis)) ? curThis : __nfSwfEnv.rootClip) : rawRecv;\n\
      if (!tgt) return undefined;\n\
      __nfSwfEnv.internalDepth++;\n\
      try { return __nfSwfCall(tgt, m, []); } finally { __nfSwfEnv.internalDepth--; }\n\
    }\n\
    if (m === 'gotoAndStop' || m === 'gotoAndPlay' || m === 'nextFrame' || m === 'prevFrame') {\n\
      const tgt = isGlobalRecv ? ((curThis && __nfDisplay?.alive?.(curThis)) ? curThis : __nfSwfEnv.rootClip) : rawRecv;\n\
      return __nfSwfEnv.avm2Goto(tgt, m, args);\n\
    }\n\
    if (m === 'addEventListener' && args.length >= 2) {\n\
      const tgt = isGlobalRecv ? curThis : rawRecv;\n\
      const evType = String(args[0] ?? '');\n\
      if (__avm2LifecycleTypes.has(evType) || tgt === __nfSwfEnv.stageProxy || __nfSwfEnv.synthParent.has(tgt)) {\n\
        __nfSwfEnv.addAvm2Listener(tgt, evType, args[1], Boolean(args[2]));\n\
        return undefined;\n\
      }\n\
    }\n\
    if (m === 'removeEventListener' && args.length >= 2) {\n\
      const tgt = isGlobalRecv ? curThis : rawRecv;\n\
      const evType = String(args[0] ?? '');\n\
      if (__avm2LifecycleTypes.has(evType) || tgt === __nfSwfEnv.stageProxy || __nfSwfEnv.synthParent.has(tgt)) {\n\
        __nfSwfEnv.removeAvm2Listener(tgt, evType, args[1], Boolean(args[2]));\n\
        return undefined;\n\
      }\n\
    }\n\
    if (m === 'hasEventListener' && args.length >= 1) {\n\
      const tgt = isGlobalRecv ? curThis : rawRecv;\n\
      const evType = String(args[0] ?? '');\n\
      if (__avm2LifecycleTypes.has(evType) || tgt === __nfSwfEnv.stageProxy || __nfSwfEnv.synthParent.has(tgt)) {\n\
        return __nfSwfEnv.hasAvm2Listener(tgt, evType);\n\
      }\n\
    }\n\
    if (m === 'dispatchEvent' && args.length >= 1) {\n\
      const tgt = isGlobalRecv ? curThis : rawRecv;\n\
      const ev = args[0];\n\
      if (ev && typeof ev === 'object' && ev.__nfEvFmt && (__avm2LifecycleTypes.has(ev.type) || tgt === __nfSwfEnv.stageProxy || __nfSwfEnv.synthParent.has(tgt))) {\n\
        return __nfSwfEnv.dispatchAvm2EventObj(tgt, ev);\n\
      }\n\
    }\n\
    const recv = isGlobalRecv ? curThis : rawRecv;\n\
    const r = __nfSwfCall(recv, m, args);\n\
    if ((m === 'getChildAt' || m === 'getChildByName') && __nfSwfEnv.isUnconstructedChild(r)) return null;\n\
    return r;\n\
  };\n\
  __nfSwfEnv.bootAvm2Mount = rootTarget => {\n\
    if (!rootTarget || __nfSwfEnv.booted) return;\n\
    __nfSwfEnv.booted = true;\n\
    __nfSwfEnv.rootClip = rootTarget;\n\
    __nfSwfEnv.internalDepth++;\n\
    try {\n\
      try { __nfSwfSet(rootTarget, 'name', 'root1'); } catch (_) {}\n\
      __nfSwfEnv.trackClip(rootTarget);\n\
      if (__nfSwfEnv.rootDocTarget) {\n\
        __nfSwfEnv.attachDocClass(__nfSwfEnv.rootDocTarget.abcIdx, __nfSwfEnv.rootDocTarget.clsName, rootTarget);\n\
      } else {\n\
        __nfSwfEnv.superConstruct(rootTarget);\n\
        __nfSwfEnv.constructedDisplay.add(rootTarget);\n\
        for (const [aIdx, mId] of __nfSwfEnv.standaloneInits) {\n\
          __nfSwfEnv.invokeMethod(aIdx, mId, rootTarget, '');\n\
        }\n\
      }\n\
      if (__nfDisplay?.alive?.(rootTarget)) {\n\
        __nfSwfEnv.dispatchAvm2Event(rootTarget, 'added', true);\n\
        if (__nfDisplay.descriptor(rootTarget)?.on_stage !== false) {\n\
          __nfSwfEnv.dispatchAvm2Event(rootTarget, 'addedToStage', false);\n\
        }\n\
      }\n\
      __nfSwfEnv.runFramePhase(true);\n\
      if (__nfDisplay?.alive?.(rootTarget)) {\n\
        __nfSwfCall(rootTarget, 'addEventListener', ['enterFrame', __nfSwfFn(() => {\n\
          if (__nfSwfEnv.internalDepth > 0 || !__nfDisplay?.alive?.(rootTarget)) return;\n\
          __nfSwfEnv.broadcastPhase('enterFrame');\n\
        })]);\n\
        __nfSwfCall(rootTarget, 'addEventListener', ['frameConstructed', __nfSwfFn(() => {\n\
          if (__nfSwfEnv.internalDepth > 0 || !__nfDisplay?.alive?.(rootTarget)) return;\n\
          __nfSwfEnv.internalDepth++;\n\
          try {\n\
            __nfSwfEnv.scanAndConstruct(rootTarget);\n\
            __nfSwfEnv.broadcastPhase('frameConstructed');\n\
            __nfSwfEnv.runPendingFrameScripts();\n\
          } finally { __nfSwfEnv.internalDepth--; }\n\
        })]);\n\
        __nfSwfCall(rootTarget, 'addEventListener', ['exitFrame', __nfSwfFn(() => {\n\
          if (__nfSwfEnv.internalDepth > 0 || !__nfDisplay?.alive?.(rootTarget)) return;\n\
          __nfSwfEnv.broadcastPhase('exitFrame');\n\
        })]);\n\
      }\n\
    } finally {\n\
      __nfSwfEnv.internalDepth--;\n\
    }\n\
  };\n\
  __nfSwfEnv.hasClass = clsName => {\n\
    if (!clsName || !__nfSwfEnv.classes) return false;\n\
    const s = String(clsName), short = s.split('.').pop();\n\
    return Boolean(__nfSwfEnv.classes[s] || (short && __nfSwfEnv.classes[short]));\n\
  };\n\
  __nfSwfEnv.onSpriteFrameEntry = (target, fIdx) => {\n\
    if (!target || __nfSwfEnv.internalDepth > 0 || !__nfDisplay?.alive?.(target)) return;\n\
    if (__nfSwfEnv.lastScriptFrame.get(target) === fIdx) return;\n\
    __nfSwfEnv.lastScriptFrame.set(target, fIdx);\n\
    const map = __nfSwfEnv.frameScripts.get(target);\n\
    const fn = map ? map.get(fIdx) : null;\n\
    if (typeof fn === 'function') __nfSwfApply(fn, target, []);\n\
  };\n\
  __nfSwfEnv.attachClassToInstance = (clsName, target, args) => {\n\
    if (!target || __nfSwfEnv.inHostNav || __nfSwfEnv.internalDepth === 0 || __nfSwfEnv.docAttached.has(target) || __nfSwfEnv.constructedDisplay.has(target)) return;\n\
    const s = String(clsName || ''), short = s.split('.').pop();\n\
    const entry = __nfSwfEnv.classes[s] || (short ? __nfSwfEnv.classes[short] : null);\n\
    if (!entry) return;\n\
    __nfSwfEnv.docAttached.add(target);\n\
    entry.bindToDisplay(target, Array.isArray(args) ? args : []);\n\
  };\n\
  __nfSwfEnv.attachDocClass = (aIdx, clsName, target) => {\n\
    if (!target || __nfSwfEnv.docAttached.has(target)) return;\n\
    __nfSwfEnv.docAttached.add(target);\n\
    const abcMod = __nfSwfEnv.abcs[aIdx];\n\
    const entry = (abcMod && abcMod.classesByName[clsName]) || __nfSwfEnv.classes[clsName];\n\
    if (entry) entry.bindToDisplay(target, []);\n\
  };\n\
  __nfSwfEnv.invokeMethod = (aIdx, mId, target, clsName) => {\n\
    const abcMod = __nfSwfEnv.abcs[aIdx];\n\
    if (!abcMod) return undefined;\n\
    const factory = abcMod.methods[mId];\n\
    if (!factory) return undefined;\n\
    const entry = clsName ? (abcMod.classesByName[clsName] || __nfSwfEnv.classes[clsName] || null) : null;\n\
    const outer = entry ? entry.methodScopes : [__nfSwfEnv.globals];\n\
    const fn = factory(outer, entry);\n\
    return __nfSwfApply(fn, target, []);\n\
  };\n\
}\n",
    );

    out.push_str(&format!(
        "const __abc = {{ methods: Object.create(null), specs: Object.create(null), built: Object.create(null), classesByName: Object.create(null) }};\n__nfSwfEnv.abcs[{abc_idx}] = __abc;\n"
    ));

    // Script traits (Trait_Slot / Trait_Const / Trait_Class on __nfSwfEnv.globals)
    let mut global_slot_pairs = Vec::new();
    let mut next_global_slot = 1u32;
    for st_list in &abc.script_traits {
        for tr in st_list {
            if matches!(tr.kind, 0 | 4 | 6) && !tr.name.is_empty() {
                let sid = resolve_trait_slot_id(tr.slot_id, &mut next_global_slot);
                global_slot_pairs.push(format!(
                    "[{sid}, {}]",
                    js_string_literal(short_multiname(&tr.name))
                ));
            }
        }
    }
    if !global_slot_pairs.is_empty() {
        out.push_str(&format!(
            "__nfSwfSlotMeta(__nfSwfEnv.globals, [{}]);\n",
            global_slot_pairs.join(",")
        ));
    }

    // Sort method IDs deterministically
    let mut body_ids: Vec<u32> = abc.bodies.keys().copied().collect();
    body_ids.sort_unstable();
    for mid in body_ids {
        let body = &abc.bodies[&mid];
        let minfo = abc.methods.get(mid as usize);
        let first_warning = ctx.warnings.len();
        if let Some(fn_js) = compile_avm2_method_factory(ctx, abc, body, minfo) {
            out.push_str(&format!("__abc.methods[{mid}] = {fn_js};\n"));
        }
        for warning in &mut ctx.warnings[first_warning..] {
            warning.push_str(&format!(" Source: ABC {abc_idx} method {mid}."));
        }
    }

    // Emit class specs
    for (ci, inst) in abc.instances.iter().enumerate() {
        let qname_lit = js_string_literal(&inst.name);
        let short_lit = js_string_literal(short_multiname(&inst.name));
        let sup_qname_lit = js_string_literal(&inst.super_name);
        let sup_short_lit = js_string_literal(short_multiname(&inst.super_name));

        let mut inst_slots_js = Vec::new();
        let mut inst_methods_js = Vec::new();
        let mut inst_getters_js = Vec::new();
        let mut inst_setters_js = Vec::new();
        let mut next_inst_slot = 1u32;
        for tr in &inst.traits {
            let sname = js_string_literal(short_multiname(&tr.name));
            match tr.kind {
                0 | 6 => {
                    let sid = resolve_trait_slot_id(tr.slot_id, &mut next_inst_slot);
                    let dv = trait_slot_default_js(tr);
                    inst_slots_js.push(format!("[{sname}, {sid}, {dv}]"));
                }
                1 | 5 => {
                    inst_methods_js.push(format!("[{sname}, {}]", tr.method_or_class_id));
                }
                2 => {
                    inst_getters_js.push(format!("[{sname}, {}]", tr.method_or_class_id));
                }
                3 => {
                    inst_setters_js.push(format!("[{sname}, {}]", tr.method_or_class_id));
                }
                _ => {}
            }
        }
        let mut static_slots_js = Vec::new();
        let mut static_methods_js = Vec::new();
        let mut static_getters_js = Vec::new();
        let mut static_setters_js = Vec::new();
        let mut next_static_slot = 1u32;
        for tr in &inst.static_traits {
            let sname = js_string_literal(short_multiname(&tr.name));
            match tr.kind {
                0 | 6 => {
                    let sid = resolve_trait_slot_id(tr.slot_id, &mut next_static_slot);
                    let dv = trait_slot_default_js(tr);
                    static_slots_js.push(format!("[{sname}, {sid}, {dv}]"));
                }
                1 | 5 => {
                    static_methods_js.push(format!("[{sname}, {}]", tr.method_or_class_id));
                }
                2 => {
                    static_getters_js.push(format!("[{sname}, {}]", tr.method_or_class_id));
                }
                3 => {
                    static_setters_js.push(format!("[{sname}, {}]", tr.method_or_class_id));
                }
                _ => {}
            }
        }

        out.push_str(&format!(
            "__abc.specs[{ci}] = {{ qname: {qname_lit}, shortName: {short_lit}, superQname: {sup_qname_lit}, superShort: {sup_short_lit}, iinit: {}, cinit: {}, instSlots: [{}], instMethods: [{}], instGetters: [{}], instSetters: [{}], staticSlots: [{}], staticMethods: [{}], staticGetters: [{}], staticSetters: [{}] }};\n",
            inst.iinit,
            inst.cinit,
            inst_slots_js.join(","),
            inst_methods_js.join(","),
            inst_getters_js.join(","),
            inst_setters_js.join(","),
            static_slots_js.join(","),
            static_methods_js.join(","),
            static_getters_js.join(","),
            static_setters_js.join(",")
        ));
    }

    out.push_str(
        "__abc.buildClass = (cIdx, baseScopes, explicitSuper) => {\n\
  if (__abc.built[cIdx]) return __abc.built[cIdx].ctor;\n\
  const spec = __abc.specs[cIdx];\n\
  if (!spec) return __nfSwfStubFn();\n\
  let superEntry = null;\n\
  if (explicitSuper && typeof explicitSuper === 'function') {\n\
    for (const k of Object.keys(__nfSwfEnv.classes)) {\n\
      if (__nfSwfEnv.classes[k].ctor === explicitSuper) { superEntry = __nfSwfEnv.classes[k]; break; }\n\
    }\n\
  }\n\
  if (!superEntry && spec.superQname && spec.superShort !== 'Object') {\n\
    superEntry = __abc.classesByName[spec.superQname] || __abc.classesByName[spec.superShort] || __nfSwfEnv.classes[spec.superQname] || __nfSwfEnv.classes[spec.superShort] || null;\n\
    if (!superEntry) {\n\
      for (const sk of Object.keys(__abc.specs)) {\n\
        const cand = __abc.specs[sk];\n\
        if (cand && (cand.qname === spec.superQname || cand.shortName === spec.superShort) && Number(sk) !== cIdx) {\n\
          __abc.buildClass(Number(sk), baseScopes, null);\n\
          superEntry = __abc.built[Number(sk)] || null;\n\
          break;\n\
        }\n\
      }\n\
    }\n\
  }\n\
  const entry = Object.create(null);\n\
  entry.spec = spec;\n\
  entry.superEntry = superEntry;\n\
  entry.getters = Object.assign(Object.create(null), superEntry ? superEntry.getters : null);\n\
  entry.setters = Object.assign(Object.create(null), superEntry ? superEntry.setters : null);\n\
  entry.hasCustomToString = Boolean(superEntry && superEntry.hasCustomToString);\n\
  const scopes = Array.isArray(baseScopes) && baseScopes.length ? [...baseScopes] : [__nfSwfEnv.globals];\n\
  entry.initChain = target => {\n\
    if (entry.superEntry) entry.superEntry.initChain(target);\n\
    const slotPairs = [];\n\
    const isDisp = Boolean(__nfDisplay && typeof __nfDisplay.alive === 'function' && __nfDisplay.alive(target));\n\
    for (const [sn, sid, dv] of spec.instSlots) {\n\
      if (sid > 0) slotPairs.push([sid, sn]);\n\
      if (isDisp && (dv === null || dv === undefined) && __nfSwfCall(target, 'getChildByName', [sn]) != null) continue;\n\
      if (!__nfSwfHasOwn(target, sn) && !(dv === undefined && __nfSwfGet(target, sn) !== undefined)) __nfSwfSet(target, sn, dv);\n\
    }\n\
    if (slotPairs.length) __nfSwfSlotMeta(target, slotPairs);\n\
  };\n\
  entry.bindMethodsToTarget = target => {\n\
    if (entry.superEntry) entry.superEntry.bindMethodsToTarget(target);\n\
    for (const [mn] of spec.instMethods) {\n\
      const fn = entry.proto[mn];\n\
      if (typeof fn === 'function') {\n\
        const bound = __nfSwfFn(function(...a) { return __nfSwfApply(fn, target, a); });\n\
        __nfSwfSet(target, mn, bound);\n\
      }\n\
    }\n\
  };\n\
  entry.runIinit = (target, args) => {\n\
    const ii = __abc.methods[spec.iinit];\n\
    if (ii) __nfSwfApply(ii(entry.methodScopes, entry), target, args);\n\
  };\n\
  entry.bindToDisplay = (target, args) => {\n\
    __nfSwfTagInstance(target, entry.ctor);\n\
    __nfSwfEnv.entryOf.set(target, entry);\n\
    __nfSwfEnv.classOf.set(target, spec.shortName);\n\
    entry.initChain(target);\n\
    entry.bindMethodsToTarget(target);\n\
    __nfSwfEnv.preSuper.add(target);\n\
    try {\n\
      entry.runIinit(target, args);\n\
      if (!__nfSwfEnv.superConstructed.has(target)) {\n\
        __nfSwfEnv.superConstruct(target);\n\
      }\n\
    } finally {\n\
      __nfSwfEnv.preSuper.delete(target);\n\
      __nfSwfEnv.constructedDisplay.add(target);\n\
    }\n\
  };\n\
  const ctor = __nfSwfFn(function(...cArgs) {\n\
    __nfSwfEnv.entryOf.set(this, entry);\n\
    __nfSwfEnv.classOf.set(this, spec.shortName);\n\
    entry.initChain(this);\n\
    entry.runIinit(this, cArgs);\n\
  });\n\
  entry.ctor = ctor;\n\
  if (superEntry) __nfSwfExtends(ctor, superEntry.ctor);\n\
  entry.proto = __nfSwfGet(ctor, 'prototype');\n\
  __nfSwfSetClassMeta(ctor, spec.qname, spec.shortName, spec.superShort);\n\
  entry.cinitScopes = [...scopes, ctor];\n\
  entry.methodScopes = [...scopes, ctor, entry.proto];\n\
  __abc.built[cIdx] = entry;\n\
  __abc.classesByName[spec.qname] = entry;\n\
  __abc.classesByName[spec.shortName] = entry;\n\
  __nfSwfEnv.classes[spec.qname] = entry;\n\
  __nfSwfEnv.classes[spec.shortName] = entry;\n\
  __nfSwfEnv.globals[spec.qname] = ctor;\n\
  __nfSwfEnv.globals[spec.shortName] = ctor;\n\
  for (const [mn, mid] of spec.instMethods) {\n\
    if (mn === 'toString') entry.hasCustomToString = true;\n\
    const mf = __abc.methods[mid];\n\
    if (mf) __nfSwfSet(entry.proto, mn, mf(entry.methodScopes, entry));\n\
  }\n\
  for (const [gn, gmid] of spec.instGetters) {\n\
    const gf = __abc.methods[gmid];\n\
    if (gf) entry.getters[gn] = gf(entry.methodScopes, entry);\n\
  }\n\
  for (const [sn, smid] of spec.instSetters) {\n\
    const sf = __abc.methods[smid];\n\
    if (sf) entry.setters[sn] = sf(entry.methodScopes, entry);\n\
  }\n\
  const sEntry = { getters: Object.create(null), setters: Object.create(null) };\n\
  __nfSwfEnv.entryOf.set(ctor, sEntry);\n\
  const sSlotPairs = [];\n\
  for (const [sn, sid, dv] of spec.staticSlots) {\n\
    if (sid > 0) sSlotPairs.push([sid, sn]);\n\
    __nfSwfSet(ctor, sn, dv);\n\
  }\n\
  if (sSlotPairs.length) __nfSwfSlotMeta(ctor, sSlotPairs);\n\
  for (const [smn, smid] of spec.staticMethods) {\n\
    const smf = __abc.methods[smid];\n\
    if (smf) {\n\
      const sfn = smf(entry.cinitScopes, entry);\n\
      __nfSwfSet(ctor, smn, __nfSwfFn(function(...a) { return __nfSwfApply(sfn, ctor, a); }));\n\
    }\n\
  }\n\
  for (const [sgn, sgmid] of spec.staticGetters) {\n\
    const sgf = __abc.methods[sgmid];\n\
    if (sgf) sEntry.getters[sgn] = sgf(entry.cinitScopes, entry);\n\
  }\n\
  for (const [ssn, ssmid] of spec.staticSetters) {\n\
    const ssf = __abc.methods[ssmid];\n\
    if (ssf) sEntry.setters[ssn] = ssf(entry.cinitScopes, entry);\n\
  }\n\
  const ciFn = __abc.methods[spec.cinit];\n\
  if (ciFn) __nfSwfApply(ciFn(entry.cinitScopes, entry), ctor, []);\n\
  return ctor;\n\
};\n",
    );

    if !abc.instances.is_empty() {
        for &sinit in &abc.script_inits {
            if abc.bodies.contains_key(&sinit) {
                out.push_str(&format!(
                    "if (__abc.methods[{sinit}]) __nfSwfApply(__abc.methods[{sinit}]([__nfSwfEnv.globals], null), __nfSwfEnv.globals, []);\n"
                ));
            }
        }
        for ci in 0..abc.instances.len() {
            out.push_str(&format!(
                "__abc.buildClass({ci}, [__nfSwfEnv.globals], null);\n"
            ));
        }
    }

    out.push_str("})();\n");
    Some(out)
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
    while r.pos < r.data.len() && out.len() < MAX_AVM_BLOCK_INSTS {
        let offset = r.pos;
        let Ok(op) = r.read_u8() else {
            break;
        };
        let args_res: Result<Vec<i64>, String> = (|| match op {
            // 1 u8
            0x24 | 0x65 | 0xF2 => Ok(vec![i64::from(r.read_u8()?)]),
            // 1 u30 truncated to signed i16 (pushshort)
            0x25 => Ok(vec![i64::from(r.read_u30()? as i16)]),
            // 1 u30
            0x04 | 0x05 | 0x06 | 0x08 | 0x2C | 0x2D | 0x2E | 0x2F | 0x31 | 0x40 | 0x41 | 0x42
            | 0x49 | 0x53 | 0x55 | 0x56 | 0x58 | 0x59 | 0x5A | 0x5B | 0x5C | 0x5D | 0x5E | 0x5F
            | 0x60 | 0x61 | 0x62 | 0x63 | 0x66 | 0x67 | 0x68 | 0x6A | 0x6C | 0x6D | 0x6E | 0x6F
            | 0x78 | 0x80 | 0x86 | 0x92 | 0x94 | 0xB2 | 0xC2 | 0xC3 | 0xF0 | 0xF1 => {
                Ok(vec![i64::from(r.read_u30()?)])
            }
            // 2 u30
            0x32 | 0x43 | 0x44 | 0x45 | 0x46 | 0x4A | 0x4C | 0x4E | 0x4F => {
                Ok(vec![i64::from(r.read_u30()?), i64::from(r.read_u30()?)])
            }
            // s24 branch
            0x0C..=0x1A => Ok(vec![i64::from(r.read_s24()?)]),
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

fn compile_avm2_method_factory(
    ctx: &mut SwfDecodeCtx,
    abc: &AbcFile,
    body: &AbcMethodBody,
    minfo: Option<&AbcMethodInfo>,
) -> Option<String> {
    let code = &body.code;
    let insts = decode_avm2_instructions(code);
    if insts.is_empty() {
        return None;
    }
    let mut offset_to_idx = HashMap::new();
    for (idx, inst) in insts.iter().enumerate() {
        offset_to_idx.insert(inst.offset, idx);
    }
    offset_to_idx.insert(code.len(), insts.len());

    let resolve_pc_at_or_after = |byte_off: usize| -> usize {
        if let Some(&idx) = offset_to_idx.get(&byte_off) {
            return idx;
        }
        insts
            .iter()
            .position(|inst| inst.offset >= byte_off)
            .unwrap_or(insts.len())
    };

    let param_count = minfo.map_or(0, |m| m.param_count);
    let defaults = minfo.map(|m| m.defaults.as_slice()).unwrap_or(&[]);
    let need_rest = minfo.is_some_and(|m| (m.flags & 0x04) != 0);
    let need_args = minfo.is_some_and(|m| (m.flags & 0x01) != 0);

    let mut act_slot_pairs = Vec::new();
    let mut next_act_slot = 1u32;
    for tr in &body.traits {
        if matches!(tr.kind, 0 | 6) && !tr.name.is_empty() {
            let sid = resolve_trait_slot_id(tr.slot_id, &mut next_act_slot);
            act_slot_pairs.push(format!(
                "[{sid}, {}]",
                js_string_literal(short_multiname(&tr.name))
            ));
        }
    }
    let act_slots_lit = format!("[{}]", act_slot_pairs.join(","));

    let mut exc_entries = Vec::new();
    for exc in &body.exceptions {
        let from_pc = resolve_pc_at_or_after(exc.from as usize);
        let to_pc = resolve_pc_at_or_after(exc.to as usize);
        let target_pc = resolve_pc_at_or_after(exc.target as usize);
        let et_short = js_string_literal(short_multiname(&exc.exc_type));
        let vn_lit = js_string_literal(&exc.var_name);
        exc_entries.push(format!(
            "{{ from: {from_pc}, to: {to_pc}, target: {target_pc}, type: {et_short}, varName: {vn_lit} }}"
        ));
    }
    let exc_lit = format!("[{}]", exc_entries.join(","));
    let init_scope_depth = body.init_scope_depth;

    let mut js = String::new();
    js.push_str(&format!(
        "(__rawOuter, __ownerCls) => {{\n\
  const __outerSc = (() => {{\n\
    const __arr = Array.isArray(__rawOuter) ? [...__rawOuter] : [__nfSwfEnv.globals];\n\
    while (__arr.length < {init_scope_depth}) __arr.unshift(__nfSwfEnv.globals);\n\
    return __arr.length > {init_scope_depth} && {init_scope_depth} > 0 ? __arr.slice(__arr.length - {init_scope_depth}) : __arr;\n\
  }})();\n\
  return __nfSwfFn(function(...__callArgs) {{\n\
    if (++__nfSwfEnv.callDepth > 64) {{ __nfSwfEnv.callDepth--; return undefined; }}\n\
    try {{\n\
      const __st = [], __sc = [], __loc = Object.create(null); __loc[0] = this;\n"
    ));

    let opt_start = param_count.saturating_sub(defaults.len());
    for p_idx in 0..param_count {
        let reg = p_idx + 1;
        if p_idx >= opt_start && (p_idx - opt_start) < defaults.len() {
            let def_js = default_val_js(&defaults[p_idx - opt_start]);
            js.push_str(&format!(
                "      __loc[{reg}] = __callArgs.length > {p_idx} ? __callArgs[{p_idx}] : {def_js};\n"
            ));
        } else {
            js.push_str(&format!("      __loc[{reg}] = __callArgs[{p_idx}];\n"));
        }
    }
    if need_rest {
        let rest_reg = param_count + 1;
        js.push_str(&format!(
            "      __loc[{rest_reg}] = __callArgs.slice({param_count});\n"
        ));
    } else if need_args {
        let args_reg = param_count + 1;
        js.push_str(&format!("      __loc[{args_reg}] = [...__callArgs];\n"));
    }

    js.push_str(&format!(
        "      const __excs = {exc_lit};\n\
      let __pc = 0, __steps = 0;\n\
      const __callHostGlobal = (n, a) => n === 'trace' ? trace(...a.map(__nfSwfEnv.toStr)) : undefined;\n\
      const __isHostBuiltin = n => n === 'trace';\n\
      const __isClipCtrl = n => n === 'stop' || n === 'play' || n === 'gotoAndStop' || n === 'gotoAndPlay' || n === 'nextFrame' || n === 'prevFrame' || n === 'addFrameScript';\n\
      const __findScope = (qname, sname) => {{\n\
        for (let __i = __sc.length - 1; __i >= 0; __i--) {{\n\
          const __s = __sc[__i];\n\
          if (__s != null && (typeof __s === 'object' || typeof __s === 'function') && __nfSwfHas(__s, sname)) return __s;\n\
        }}\n\
        for (let __i = __outerSc.length - 1; __i >= 0; __i--) {{\n\
          const __s = __outerSc[__i];\n\
          if (__s != null && (typeof __s === 'object' || typeof __s === 'function')) {{\n\
            if (qname && __nfSwfHas(__s, qname)) return __s;\n\
            if (__nfSwfHas(__s, sname)) return __s;\n\
          }}\n\
        }}\n\
        if (qname && Object.hasOwn(__nfSwfEnv.globals, qname)) return __nfSwfEnv.globals;\n\
        if (Object.hasOwn(__nfSwfEnv.globals, sname)) return __nfSwfEnv.globals;\n\
        if (Object.hasOwn(__nfSwfEnv.builtins, sname)) return __nfSwfEnv.builtins;\n\
        if (__isHostBuiltin(sname) || __isClipCtrl(sname)) return this ?? __nfSwfEnv.globals;\n\
        return __sc[0] ?? this ?? __nfSwfEnv.globals;\n\
      }};\n\
      while (__pc < {}\n      ) {{\n\
        if (++__steps > 20000) break;\n\
        try {{\n\
        switch (__pc) {{\n",
        insts.len()
    ));

    for (idx, inst) in insts.iter().enumerate() {
        js.push_str(&format!("    case {idx}: {{\n"));
        let next_idx = idx + 1;
        let mn_full_short = |arg_i: usize| -> (String, String, bool) {
            let mi = inst.args.get(arg_i).copied().unwrap_or(0) as usize;
            abc.multinames.get(mi).map_or_else(
                || (String::new(), String::new(), false),
                |m| {
                    (
                        m.name.clone(),
                        short_multiname(&m.name).to_owned(),
                        m.runtime_name,
                    )
                },
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
            0x30 | 0x1C => js.push_str("      { const __so = __st.pop(); if (__sc.length < 64) __sc.push(__so); }\n"),
            0x1D => js.push_str("      __sc.pop();\n"),
            0x64 => js.push_str("      __st.push(__outerSc[0] ?? __nfSwfEnv.globals);\n"),
            0x65 => {
                let s_idx = inst.args[0].max(0);
                js.push_str(&format!("      __st.push(__sc[{s_idx}]);\n"));
            }
            0x67 => {
                let o_idx = inst.args[0].max(0);
                js.push_str(&format!("      __st.push(__outerSc[{o_idx}]);\n"));
            }
            0x57 => {
                js.push_str(&format!(
                    "      __st.push(__nfSwfSlotMeta(Object.create(null), {act_slots_lit}));\n"
                ));
            }
            0x5A => {
                let exc_idx = inst.args[0].max(0) as usize;
                let vname = body
                    .exceptions
                    .get(exc_idx)
                    .map_or("", |e| e.var_name.as_str());
                if vname.is_empty() {
                    js.push_str("      __st.push(Object.create(null));\n");
                } else {
                    let vlit = js_string_literal(vname);
                    js.push_str(&format!(
                        "      __st.push(__nfSwfSlotMeta(Object.create(null), [[1, {vlit}]]));\n"
                    ));
                }
            }
            0x6C => {
                let slot_id = inst.args[0].max(0);
                js.push_str(&format!(
                    "      {{ const __so = __st.pop(); __st.push(__nfSwfGetSlot(__so, {slot_id})); }}\n"
                ));
            }
            0x6D => {
                let slot_id = inst.args[0].max(0);
                js.push_str(&format!(
                    "      {{ const __v = __st.pop(), __so = __st.pop(); __nfSwfSetSlot(__so, {slot_id}, __v); }}\n"
                ));
            }
            0x6E => {
                let slot_id = inst.args[0].max(0);
                js.push_str(&format!(
                    "      __st.push(__nfSwfGetSlot(__outerSc[0] ?? __nfSwfEnv.globals, {slot_id}));\n"
                ));
            }
            0x6F => {
                let slot_id = inst.args[0].max(0);
                js.push_str(&format!(
                    "      {{ const __v = __st.pop(); __nfSwfSetSlot(__outerSc[0] ?? __nfSwfEnv.globals, {slot_id}, __v); }}\n"
                ));
            }
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
                let m_id = inst.args[0] as u32;
                if abc.bodies.contains_key(&m_id) {
                    js.push_str(&format!(
                        "      {{ const __mf = __abc.methods[{m_id}]; __st.push(__mf ? __mf([...__outerSc, ...__sc], __ownerCls) : __nfSwfStubFn()); }}\n"
                    ));
                } else {
                    ctx.warn("SWF AVM2 opcode newfunction (0x40) references a method without body; substituted a hardened no-op stub.");
                    js.push_str("      __st.push(__nfSwfStubFn());\n");
                }
            }
            0x58 => {
                // newclass u30
                let c_idx = inst.args[0].max(0);
                js.push_str(&format!(
                    "      {{ const __base = __st.pop(); __st.push(__abc.buildClass({c_idx}, [...__outerSc, ...__sc], __base)); }}\n"
                ));
            }
            0x41 => {
                let argc = inst
                    .args
                    .first()
                    .copied()
                    .unwrap_or(0)
                    .clamp(0, MAX_SWF_OPCODE_ARITY);
                js.push_str(&format!(
                    "      {{ const __args = new Array({argc}); for (let __i = {argc} - 1; __i >= 0; __i--) __args[__i] = __st.pop(); const __recv = __st.pop(), __fn = __st.pop(); __st.push(__nfSwfApply(__fn, __recv, __args)); }}\n"
                ));
            }
            0x42 => {
                // construct u30
                let argc = inst.args[0].clamp(0, MAX_SWF_OPCODE_ARITY);
                js.push_str(&format!(
                    "      {{ const __args = new Array({argc}); for (let __i = {argc} - 1; __i >= 0; __i--) __args[__i] = __st.pop(); const __ctor = __st.pop(); __st.push(__nfSwfNew(__ctor, undefined, __args)); }}\n"
                ));
            }
            0x4A => {
                // constructprop u30 mn, u30 argc
                let (qname, sname, rt) = mn_full_short(0);
                let argc = inst.args[1].clamp(0, MAX_SWF_OPCODE_ARITY);
                js.push_str(&format!(
                    "      {{ const __args = new Array({argc}); for (let __i = {argc} - 1; __i >= 0; __i--) __args[__i] = __st.pop();\n"
                ));
                if rt {
                    js.push_str(
                        "        const __k = String(__st.pop() ?? ''), __o = __st.pop(); __st.push(__nfSwfNew(__o, __k, __args));\n      }\n",
                    );
                } else {
                    let qlit = js_string_literal(&qname);
                    let slit = js_string_literal(&sname);
                    js.push_str(&format!(
                        "        const __o = __st.pop(); const __ctor = (__o && __nfSwfGet(__o, {qlit})) ?? (__o && __nfSwfGet(__o, {slit})) ?? __nfSwfEnv.globals[{qlit}] ?? __nfSwfEnv.globals[{slit}] ?? __nfSwfEnv.builtins[{slit}]; __st.push(__nfSwfNew(__ctor, undefined, __args));\n      }}\n"
                    ));
                }
            }
            0x55 => {
                let cnt = inst.args[0].clamp(0, MAX_SWF_OPCODE_ARITY);
                js.push_str(&format!(
                    "      {{ const __o = {{}}; for (let __i = 0; __i < {cnt}; __i++) {{ const __v = __st.pop(), __k = String(__st.pop() ?? ''); __nfSwfSet(__o, __k, __v); }} __st.push(__o); }}\n"
                ));
            }
            0x56 => {
                let cnt = inst.args[0].clamp(0, MAX_SWF_OPCODE_ARITY);
                js.push_str(&format!(
                    "      {{ const __a = new Array({cnt}); for (let __i = {cnt} - 1; __i >= 0; __i--) __a[__i] = __st.pop(); __st.push(__a); }}\n"
                ));
            }
            0x5B | 0x5C => {
                js.push_str("      __st.push(__nfSwfEnv.globals);\n");
            }
            0x5D..=0x5F => {
                let (qname, sname, rt) = mn_full_short(0);
                if rt {
                    js.push_str("      { const __k = String(__st.pop() ?? ''); __st.push(__findScope('', __k)); }\n");
                } else {
                    let qlit = js_string_literal(&qname);
                    let slit = js_string_literal(&sname);
                    js.push_str(&format!("      __st.push(__findScope({qlit}, {slit}));\n"));
                }
            }
            0x60 => {
                let (qname, sname, _) = mn_full_short(0);
                let qlit = js_string_literal(&qname);
                let slit = js_string_literal(&sname);
                js.push_str(&format!(
                    "      {{ const __so = __findScope({qlit}, {slit}); const __v = ({qlit} && __nfSwfEnv.avm2Get(__so, {qlit}) !== undefined) ? __nfSwfEnv.avm2Get(__so, {qlit}) : __nfSwfEnv.avm2Get(__so, {slit}); __st.push(typeof __v === 'function' && __so && __so !== __nfSwfEnv.globals && __so !== __nfSwfEnv.builtins && __so !== Math ? __nfSwfFn(function(...__ba) {{ return __nfSwfApply(__v, __so, __ba); }}) : __v); }}\n"
                ));
            }
            0x66 => {
                let (_, sname, rt) = mn_full_short(0);
                if rt {
                    js.push_str("      { const __k = String(__st.pop() ?? ''), __o = __st.pop() ?? this; __st.push(__nfSwfEnv.avm2Get(__o, __k)); }\n");
                } else {
                    let name_lit = js_string_literal(&sname);
                    js.push_str(&format!(
                        "      {{ const __o = __st.pop() ?? this; __st.push(__nfSwfEnv.avm2Get(__o, {name_lit})); }}\n"
                    ));
                }
            }
            0x61 | 0x68 => {
                let (qname, sname, rt) = mn_full_short(0);
                if rt {
                    js.push_str("      { const __v = __st.pop(), __k = String(__st.pop() ?? ''), __o = __st.pop() ?? this; __nfSwfEnv.avm2Set(__o, __k, __v); }\n");
                } else {
                    let qlit = js_string_literal(&qname);
                    let slit = js_string_literal(&sname);
                    js.push_str(&format!(
                        "      {{ const __v = __st.pop(), __o = __st.pop() ?? this; if (__o === __nfSwfEnv.globals && {qlit}) __nfSwfSet(__o, {qlit}, __v); __nfSwfEnv.avm2Set(__o, {slit}, __v); }}\n"
                    ));
                }
            }
            0x6A => {
                let (_, sname, rt) = mn_full_short(0);
                if rt {
                    js.push_str("      { const __k = String(__st.pop() ?? ''), __o = __st.pop(); __st.push(__nfSwfDelete(__o, __k)); }\n");
                } else {
                    let slit = js_string_literal(&sname);
                    js.push_str(&format!(
                        "      {{ const __o = __st.pop(); __st.push(__nfSwfDelete(__o, {slit})); }}\n"
                    ));
                }
            }
            0x46 | 0x4C | 0x4F => {
                let (_, sname, rt) = mn_full_short(0);
                let argc = inst.args[1].clamp(0, MAX_SWF_OPCODE_ARITY);
                let push_ret = inst.op != 0x4F;
                js.push_str(&format!(
                    "      {{ const __args = new Array({argc}); for (let __i = {argc} - 1; __i >= 0; __i--) __args[__i] = __st.pop();\n"
                ));
                if rt {
                    js.push_str(
                        "        const __m = String(__st.pop() ?? ''), __recv = __st.pop() ?? this;\n        const __r = (__isHostBuiltin(__m) && (__recv === this || __recv === __nfSwfEnv.globals)) ? __callHostGlobal(__m, __args) : __nfSwfEnv.avm2Call(__recv, __m, __args, this);\n",
                    );
                } else {
                    let name_lit = js_string_literal(&sname);
                    js.push_str(&format!(
                        "        const __recv = __st.pop() ?? this;\n        let __r;\n        if (__isHostBuiltin({name_lit}) && (__recv === this || __recv === __nfSwfEnv.globals) && !__nfSwfHasOwn(__recv, {name_lit})) {{\n          __r = __callHostGlobal({name_lit}, __args);\n        }} else if ((__recv === __nfSwfEnv.globals || __recv === __nfSwfEnv.builtins) && __args.length === 1 && (__nfSwfEnv.classes[{name_lit}] || typeof __nfSwfEnv.builtins[{name_lit}] === 'string')) {{\n          const __cv = __args[0];\n          __r = (__cv == null) ? ({name_lit} === 'Object' ? {{}} : null) : (__nfSwfInstanceOf(__cv, __nfSwfEnv.globals[{name_lit}] ?? {name_lit}) ? __cv : null);\n        }} else {{\n          __r = __nfSwfEnv.avm2Call(__recv, {name_lit}, __args, this);\n        }}\n"
                    ));
                }
                if push_ret {
                    js.push_str("        __st.push(__r);\n");
                }
                js.push_str("      }\n");
            }
            0x49 => {
                // constructsuper u30 argc
                let argc = inst.args[0].clamp(0, MAX_SWF_OPCODE_ARITY);
                js.push_str(&format!(
                    "      {{ const __args = new Array({argc}); for (let __i = {argc} - 1; __i >= 0; __i--) __args[__i] = __st.pop(); const __recv = __st.pop(); if (__ownerCls && __ownerCls.superEntry) {{ __ownerCls.superEntry.runIinit(__recv, __args); }} else {{ if (__recv && (typeof __recv === 'object' || typeof __recv === 'function')) __nfSwfEnv.preSuper.delete(__recv); if (__ownerCls && __ownerCls.spec && (__ownerCls.spec.superShort === 'Error' || __ownerCls.spec.superShort.endsWith('Error'))) {{ if (__args.length > 0 && __args[0] !== undefined) __nfSwfSet(__recv, 'message', String(__args[0])); __nfSwfSet(__recv, 'name', __ownerCls.spec.shortName); }} __nfSwfEnv.superConstruct(__recv); }} }}\n"
                ));
            }
            0x04 => {
                // getsuper u30 mn
                let (_, sname, rt) = mn_full_short(0);
                if rt {
                    js.push_str("      { const __k = String(__st.pop() ?? ''), __recv = __st.pop(); const __sp = __ownerCls && __ownerCls.superEntry ? __ownerCls.superEntry.proto : null; __st.push(__sp && __nfSwfHas(__sp, __k) ? __nfSwfGet(__sp, __k) : __nfSwfGet(__recv, __k)); }\n");
                } else {
                    let slit = js_string_literal(&sname);
                    js.push_str(&format!(
                        "      {{ const __recv = __st.pop(); const __sp = __ownerCls && __ownerCls.superEntry ? __ownerCls.superEntry.proto : null; __st.push(__sp && __nfSwfHas(__sp, {slit}) ? __nfSwfGet(__sp, {slit}) : __nfSwfGet(__recv, {slit})); }}\n"
                    ));
                }
            }
            0x05 => {
                // setsuper u30 mn
                let (_, sname, rt) = mn_full_short(0);
                if rt {
                    js.push_str("      { const __v = __st.pop(), __k = String(__st.pop() ?? ''), __recv = __st.pop(); __nfSwfSet(__recv, __k, __v); }\n");
                } else {
                    let slit = js_string_literal(&sname);
                    js.push_str(&format!(
                        "      {{ const __v = __st.pop(), __recv = __st.pop(); __nfSwfSet(__recv, {slit}, __v); }}\n"
                    ));
                }
            }
            0x45 | 0x4E => {
                // callsuper / callsupervoid u30 mn, u30 argc
                let (_, sname, rt) = mn_full_short(0);
                let argc = inst.args[1].clamp(0, MAX_SWF_OPCODE_ARITY);
                let push_ret = inst.op == 0x45;
                js.push_str(&format!(
                    "      {{ const __args = new Array({argc}); for (let __i = {argc} - 1; __i >= 0; __i--) __args[__i] = __st.pop();\n"
                ));
                if rt {
                    js.push_str(
                        "        const __k = String(__st.pop() ?? ''), __recv = __st.pop(); const __sp = __ownerCls && __ownerCls.superEntry ? __ownerCls.superEntry.proto : null; const __sfn = __sp ? __nfSwfGet(__sp, __k) : undefined; const __r = typeof __sfn === 'function' ? __nfSwfApply(__sfn, __recv, __args) : __nfSwfCall(__recv, __k, __args);\n",
                    );
                } else {
                    let slit = js_string_literal(&sname);
                    js.push_str(&format!(
                        "        const __recv = __st.pop(); const __sp = __ownerCls && __ownerCls.superEntry ? __ownerCls.superEntry.proto : null; const __sfn = __sp ? __nfSwfGet(__sp, {slit}) : undefined; const __r = typeof __sfn === 'function' ? __nfSwfApply(__sfn, __recv, __args) : __nfSwfCall(__recv, {slit}, __args);\n"
                    ));
                }
                if push_ret {
                    js.push_str("        __st.push(__r);\n");
                }
                js.push_str("      }\n");
            }
            0x32 => {
                // hasnext2 obj_reg, idx_reg
                let obj_r = inst.args[0];
                let idx_r = inst.args[1];
                js.push_str(&format!(
                    "      {{ const __ho = __loc[{obj_r}], __hi = Number(__loc[{idx_r}]) | 0; const __ks = __ho != null ? __nfSwfKeys(__ho) : []; if (__hi < __ks.length) {{ __loc[{idx_r}] = __hi + 1; __st.push(true); }} else {{ __loc[{obj_r}] = null; __loc[{idx_r}] = 0; __st.push(false); }} }}\n"
                ));
            }
            0x1F => {
                // hasnext
                js.push_str("      { const __hi = Number(__st.pop()) | 0, __ho = __st.pop(); const __ks = __ho != null ? __nfSwfKeys(__ho) : []; __st.push(__hi < __ks.length ? __hi + 1 : 0); }\n");
            }
            0x1E => {
                // nextname
                js.push_str("      { const __ni = (Number(__st.pop()) | 0) - 1, __no = __st.pop(); const __ks = __no != null ? __nfSwfKeys(__no) : []; __st.push(__ks[__ni]); }\n");
            }
            0x23 => {
                // nextvalue
                js.push_str("      { const __ni = (Number(__st.pop()) | 0) - 1, __no = __st.pop(); const __ks = __no != null ? __nfSwfKeys(__no) : []; __st.push(__ks[__ni] !== undefined ? __nfSwfEnv.avm2Get(__no, __ks[__ni]) : undefined); }\n");
            }
            0xA0 => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); const __na = typeof __a === 'number' || typeof __a === 'boolean' || __a === null || __a === undefined, __nb = typeof __b === 'number' || typeof __b === 'boolean' || __b === null || __b === undefined; __st.push((__na && __nb && !(__a === undefined && __b === undefined)) ? Number(__a) + Number(__b) : (__a === undefined && __b === undefined ? NaN : __nfSwfEnv.toStr(__a) + __nfSwfEnv.toStr(__b))); }\n"),
            0xA1 | 0xC6 => js.push_str("      { const __b = Number(__st.pop()) || 0, __a = Number(__st.pop()) || 0; __st.push(__a - __b); }\n"),
            0xA2 | 0xC7 => js.push_str("      { const __b = Number(__st.pop()) || 0, __a = Number(__st.pop()) || 0; __st.push(__a * __b); }\n"),
            0xC4 => js.push_str("      __st.push((-(Number(__st.pop()) | 0)) | 0);\n"),
            0xC5 => js.push_str("      { const __b = Number(__st.pop()) | 0, __a = Number(__st.pop()) | 0; __st.push((__a + __b) | 0); }\n"),
            0xA3 => js.push_str("      { const __b = Number(__st.pop()), __a = Number(__st.pop()); __st.push(__a / __b); }\n"),
            0xA4 => js.push_str("      { const __b = Number(__st.pop()), __a = Number(__st.pop()); __st.push(__a % __b); }\n"),
            0xA5 => js.push_str("      { const __b = Number(__st.pop()) & 31, __a = Number(__st.pop()) | 0; __st.push(__a << __b); }\n"),
            0xA6 => js.push_str("      { const __b = Number(__st.pop()) & 31, __a = Number(__st.pop()) | 0; __st.push(__a >> __b); }\n"),
            0xA7 => js.push_str("      { const __b = Number(__st.pop()) & 31, __a = Number(__st.pop()) >>> 0; __st.push(__a >>> __b); }\n"),
            0xA8 => js.push_str("      { const __b = Number(__st.pop()) | 0, __a = Number(__st.pop()) | 0; __st.push(__a & __b); }\n"),
            0xA9 => js.push_str("      { const __b = Number(__st.pop()) | 0, __a = Number(__st.pop()) | 0; __st.push(__a | __b); }\n"),
            0xAA => js.push_str("      { const __b = Number(__st.pop()) | 0, __a = Number(__st.pop()) | 0; __st.push(__a ^ __b); }\n"),
            0x90 => js.push_str("      __st.push(-(Number(__st.pop()) || 0));\n"),
            0x91 | 0xC0 => js.push_str("      __st.push((Number(__st.pop()) || 0) + 1);\n"),
            0x93 | 0xC1 => js.push_str("      __st.push((Number(__st.pop()) || 0) - 1);\n"),
            0x92 | 0xC2 => js.push_str(&format!(
                "      __loc[{}] = (Number(__loc[{}]) || 0) + 1;\n",
                inst.args[0], inst.args[0]
            )),
            0x94 | 0xC3 => js.push_str(&format!(
                "      __loc[{}] = (Number(__loc[{}]) || 0) - 1;\n",
                inst.args[0], inst.args[0]
            )),
            0x95 => js.push_str("      { const __v = __st.pop(); __st.push(__v === null ? 'object' : typeof __v === 'function' ? 'function' : typeof __v); }\n"),
            0x96 => js.push_str("      __st.push(!__st.pop());\n"),
            0x97 => js.push_str("      __st.push(~(Number(__st.pop()) | 0));\n"),
            0x71 | 0x72 => js.push_str("      { const __v = String(__st.pop() ?? ''); __st.push(__v.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/\"/g,'&quot;')); }\n"),
            0x73 | 0x83 => js.push_str("      __st.push(Number(__st.pop()) | 0);\n"),
            0x74 | 0x88 => js.push_str("      __st.push(Number(__st.pop()) >>> 0);\n"),
            0x75 | 0x84 => js.push_str("      __st.push(Number(__st.pop()));\n"),
            0x76 | 0x81 => js.push_str("      __st.push(Boolean(__st.pop()));\n"),
            0x77 => {} // convert_o
            0x70 => js.push_str("      { const __v = __st.pop(); __st.push(__nfSwfEnv.toStr(__v)); }\n"),
            0x80 => {
                let (_, sname, _) = mn_full_short(0);
                if sname == "String" {
                    js.push_str("      { const __v = __st.pop(); __st.push(__v == null ? null : __nfSwfEnv.toStr(__v)); }\n");
                } else if sname == "int" {
                    js.push_str("      __st.push(Number(__st.pop()) | 0);\n");
                } else if sname == "uint" {
                    js.push_str("      __st.push(Number(__st.pop()) >>> 0);\n");
                } else if sname == "Number" {
                    js.push_str("      __st.push(Number(__st.pop()));\n");
                } else if sname == "Boolean" {
                    js.push_str("      __st.push(Boolean(__st.pop()));\n");
                } else if !sname.is_empty() && sname != "*" {
                    js.push_str("      { const __v = __st.pop(); __st.push(__v === undefined ? null : __v); }\n");
                }
            }
            0x82 => {} // coerce_a
            0x89 => js.push_str("      { const __v = __st.pop(); __st.push(__v === undefined ? null : __v); }\n"),
            0x85 => js.push_str("      { const __v = __st.pop(); __st.push(__v == null ? null : __nfSwfEnv.toStr(__v)); }\n"),
            0x86 => {
                let (_, sname, _) = mn_full_short(0);
                let slit = js_string_literal(&sname);
                js.push_str(&format!(
                    "      {{ const __v = __st.pop(); __st.push(__nfSwfInstanceOf(__v, __nfSwfEnv.globals[{slit}] ?? {slit}) ? __v : null); }}\n"
                ));
            }
            0x87 => {
                js.push_str("      { const __tp = __st.pop(), __v = __st.pop(); __st.push(__nfSwfInstanceOf(__v, __tp) ? __v : null); }\n");
            }
            0xAB => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a == __b); }\n"),
            0xAC => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a === __b); }\n"),
            0xAD => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a < __b); }\n"),
            0xAE => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a <= __b); }\n"),
            0xAF => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a > __b); }\n"),
            0xB0 => js.push_str("      { const __b = __st.pop(), __a = __st.pop(); __st.push(__a >= __b); }\n"),
            0xB1 | 0xB3 => {
                js.push_str("      { const __tp = __st.pop(), __v = __st.pop(); __st.push(__nfSwfInstanceOf(__v, __tp)); }\n");
            }
            0xB2 => {
                let (_, sname, _) = mn_full_short(0);
                if sname.is_empty() || sname == "*" {
                    js.push_str("      __st.pop(); __st.push(true);\n");
                } else {
                    let slit = js_string_literal(&sname);
                    js.push_str(&format!(
                        "      {{ const __v = __st.pop(); __st.push(__nfSwfInstanceOf(__v, __nfSwfEnv.globals[{slit}] ?? {slit})); }}\n"
                    ));
                }
            }
            0xB4 => {
                js.push_str("      { const __o = __st.pop(), __k = String(__st.pop() ?? ''); __st.push(__o != null && (typeof __o === 'object' || typeof __o === 'function') && __nfSwfHas(__o, __k)); }\n");
            }
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
            0x19 | 0x1A => {
                let t = branch_target(inst.args[0]);
                let cmp = if inst.op == 0x19 { "===" } else { "!==" };
                js.push_str(&format!("      {{ const __b = __st.pop(), __a = __st.pop(); if (__a {cmp} __b) {{ __pc = {t}; continue; }} }}\n"));
            }
            0x1B if !inst.args.is_empty() => {
                let def_t = {
                    let target_off = ((inst.offset as i64) + inst.args[0]).max(0) as usize;
                    offset_to_idx.get(&target_off).copied().unwrap_or(insts.len())
                };
                let cases_js: Vec<String> = inst.args[1..]
                    .iter()
                    .map(|&rel| {
                        let target_off = ((inst.offset as i64) + rel).max(0) as usize;
                        offset_to_idx
                            .get(&target_off)
                            .copied()
                            .unwrap_or(insts.len())
                            .to_string()
                    })
                    .collect();
                js.push_str(&format!(
                    "      {{ const __sw = Number(__st.pop()) | 0, __tbl = [{}]; __pc = (__sw >= 0 && __sw < __tbl.length) ? __tbl[__sw] : {def_t}; continue; }}\n",
                    cases_js.join(",")
                ));
            }
            0x03 => {
                // throw
                js.push_str("      throw { __nfSwfThrown: __st.pop() };\n");
            }
            0x47 => js.push_str("      return undefined;\n"),
            0x48 => js.push_str("      return __st.pop();\n"),
            0xEF | 0xF0 | 0xF1 | 0xF2 | 0x02 | 0x09 => {}
            other => {
                ctx.warn_opcode("AVM2", other, inst.offset);
            }
        }
        js.push_str(&format!("      __pc = {next_idx}; break;\n    }}\n"));
    }

    js.push_str("    default: __pc = ");
    js.push_str(&insts.len().to_string());
    js.push_str(
        "; break;\n\
        }\n\
        } catch (__rawErr) {\n\
          const __ev = (__rawErr && typeof __rawErr === 'object' && '__nfSwfThrown' in __rawErr) ? __rawErr.__nfSwfThrown : __rawErr;\n\
          let __handled = false;\n\
          for (let __ei = 0; __ei < __excs.length; __ei++) {\n\
            const __ex = __excs[__ei];\n\
            if (__pc >= __ex.from && __pc < __ex.to && (!__ex.type || __ex.type === '*' || __ex.type === 'Object' || __nfSwfInstanceOf(__ev, __nfSwfEnv.globals[__ex.type] ?? __ex.type))) {\n\
              __st.length = 0; __sc.length = 0; __st.push(__ev); __pc = __ex.target; __handled = true; break;\n\
            }\n\
          }\n\
          if (__handled) continue;\n\
          throw { __nfSwfThrown: __ev };\n\
        }\n\
      }\n\
    } finally {\n\
      __nfSwfEnv.callDepth--;\n\
    }\n\
    return undefined;\n\
  });\n\
}"
    );

    Some(js)
}
