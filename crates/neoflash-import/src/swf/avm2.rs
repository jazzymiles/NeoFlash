//! Pure-Rust SWF AVM2 (`DoABC` tag 72 / `DoABC2` tag 82) ABC parser, ActionScript 3
//! class/method decompiler, and `addFrameScript` timeline script extractor.

use super::avm1::{format_number, quote_as_string};
use super::bits::SwfSlice;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const MAX_ABC_INSNS: usize = 50_000;
const MAX_ABC_DEPTH: usize = 20;

#[derive(Clone, Debug)]
pub(crate) struct RecoveredAs3Class {
    /// Full qualified class name (e.g. `"Main"` or `"com.example.Hero"`).
    pub qualified_name: String,
    /// Relative `.as` source path inside `project.sources` (e.g. `"src/Main.as"` or `"src/com/example/Hero.as"`).
    pub source_path: String,
    /// Reconstructed ActionScript 3 `.as` file source.
    pub source: String,
    /// Zero-based `(frame_index, script_source)` pairs extracted from `addFrameScript(...)` calls.
    pub timeline_frame_scripts: Vec<(u32, String)>,
    /// True when this class is an auto-generated Flash timeline class (e.g. `*_fla.MainTimeline`)
    /// whose constructor only invokes `super()` + `addFrameScript(...)` and whose methods are only
    /// those frame callbacks.
    pub is_pure_timeline_class: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DecompiledAbcBundle {
    pub classes: Vec<RecoveredAs3Class>,
    pub unrecognized_opcodes: usize,
}

pub(crate) fn decompile_do_abc_tag(tag_code: u16, payload: &[u8]) -> Option<DecompiledAbcBundle> {
    let abc_bytes = if tag_code == 82 {
        let mut r = SwfSlice::new(payload);
        let _flags = r.u32()?;
        let _name = r.c_string(9)?;
        r.rest()
    } else {
        payload
    };
    let abc = parse_abc_file(abc_bytes)?;
    Some(reconstruct_classes(&abc))
}

pub(crate) struct AbcReader<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) pos: usize,
}

impl<'a> AbcReader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        let b = *self.bytes.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    pub(crate) fn i8(&mut self) -> Option<i8> {
        Some(self.u8()? as i8)
    }

    pub(crate) fn u16(&mut self) -> Option<u16> {
        let b = self.take(2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    pub(crate) fn s24(&mut self) -> Option<i32> {
        let b = self.take(3)?;
        let raw = u32::from(b[0]) | (u32::from(b[1]) << 8) | (u32::from(b[2]) << 16);
        Some(((raw << 8) as i32) >> 8)
    }

    pub(crate) fn d64(&mut self) -> Option<f64> {
        let b = self.take(8)?;
        Some(f64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub(crate) fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(len)?;
        if end > self.bytes.len() {
            return None;
        }
        let s = &self.bytes[self.pos..end];
        self.pos = end;
        Some(s)
    }

    /// Read AVM2 variable-length `u32` / `u30` (1..=5 bytes, 7 bits/byte).
    /// Note: Tamarin's `AbcParser::readS32` is identical to `readU32() as i32`.
    pub(crate) fn u32_var(&mut self) -> Option<u32> {
        let mut result = 0u32;
        for shift in [0u32, 7, 14, 21, 28] {
            let b = self.u8()?;
            result |= u32::from(b & 0x7f) << shift;
            if (b & 0x80) == 0 {
                return Some(result);
            }
        }
        Some(result)
    }

    pub(crate) fn u30(&mut self) -> Option<usize> {
        Some((self.u32_var()? & 0x3FFF_FFFF) as usize)
    }

    pub(crate) fn s32(&mut self) -> Option<i32> {
        Some(self.u32_var()? as i32)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NsKind {
    Private,
    Namespace,
    Package,
    PackageInternal,
    Protected,
    Explicit,
    StaticProtected,
    Any,
}

#[derive(Clone, Debug)]
pub(crate) struct AbcNamespace {
    pub(crate) kind: NsKind,
    pub(crate) name: String,
}

#[derive(Clone, Debug)]
pub(crate) enum AbcMultiname {
    Any,
    QName { ns: usize, name: usize },
    RTQName { name: usize },
    RTQNameL,
    Multiname { name: usize, ns_set: usize },
    MultinameL,
    TypeName { base: usize, params: Vec<usize> },
}

#[derive(Clone, Debug)]
pub(crate) struct AbcMethod {
    pub(crate) param_types: Vec<usize>,
    pub(crate) return_type: usize,
    pub(crate) flags: u8,
    pub(crate) optional_values: Vec<(usize, u8)>,
    pub(crate) param_names: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct AbcTrait {
    pub(crate) name: usize,
    pub(crate) attr: u8,
    pub(crate) data: AbcTraitData,
}

#[derive(Clone, Debug)]
pub(crate) enum AbcTraitData {
    Slot {
        slot_id: usize,
        type_name: usize,
        vindex: usize,
        vkind: u8,
        is_const: bool,
    },
    Method {
        method: usize,
    },
    Getter {
        method: usize,
    },
    Setter {
        method: usize,
    },
    Class {
        #[allow(dead_code)]
        class_idx: usize,
    },
    Function {
        method: usize,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct AbcInstance {
    pub(crate) name: usize,
    pub(crate) super_name: usize,
    pub(crate) flags: u8,
    pub(crate) interfaces: Vec<usize>,
    pub(crate) iinit: usize,
    pub(crate) traits: Vec<AbcTrait>,
}

#[derive(Clone, Debug)]
pub(crate) struct AbcClass {
    pub(crate) cinit: usize,
    pub(crate) traits: Vec<AbcTrait>,
}

#[derive(Clone, Debug)]
pub(crate) struct AbcException {
    pub(crate) from: usize,
    pub(crate) to: usize,
    pub(crate) target: usize,
    pub(crate) exc_type: usize,
    pub(crate) var_name: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct AbcMethodBody {
    #[allow(dead_code)]
    pub(crate) max_stack: usize,
    pub(crate) local_count: usize,
    pub(crate) code: Vec<u8>,
    pub(crate) exceptions: Vec<AbcException>,
    pub(crate) traits: Vec<AbcTrait>,
}

#[derive(Clone, Debug)]
pub(crate) struct AbcFile {
    pub(crate) ints: Vec<i32>,
    pub(crate) uints: Vec<u32>,
    pub(crate) doubles: Vec<f64>,
    pub(crate) strings: Vec<String>,
    pub(crate) namespaces: Vec<AbcNamespace>,
    pub(crate) ns_sets: Vec<Vec<usize>>,
    pub(crate) multinames: Vec<AbcMultiname>,
    pub(crate) methods: Vec<AbcMethod>,
    pub(crate) instances: Vec<AbcInstance>,
    pub(crate) classes: Vec<AbcClass>,
    pub(crate) scripts: Vec<(usize, Vec<AbcTrait>)>,
    pub(crate) bodies_by_method: HashMap<usize, AbcMethodBody>,
}

impl AbcFile {
    pub(crate) fn string(&self, idx: usize) -> &str {
        self.strings.get(idx).map_or("", String::as_str)
    }

    pub(crate) fn namespace(&self, idx: usize) -> &AbcNamespace {
        static ANY_NS: std::sync::OnceLock<AbcNamespace> = std::sync::OnceLock::new();
        self.namespaces.get(idx).unwrap_or_else(|| {
            ANY_NS.get_or_init(|| AbcNamespace {
                kind: NsKind::Any,
                name: String::new(),
            })
        })
    }

    /// Return `(package_name, short_name)` for a multiname index.
    pub(crate) fn split_qname(&self, idx: usize) -> (String, String) {
        match self.multinames.get(idx) {
            None | Some(AbcMultiname::Any) => (String::new(), "*".into()),
            Some(AbcMultiname::QName { ns, name }) => {
                let pkg = self.namespace(*ns);
                let pkg_str = if matches!(pkg.kind, NsKind::Package | NsKind::PackageInternal) {
                    pkg.name.clone()
                } else {
                    String::new()
                };
                (pkg_str, self.string(*name).to_owned())
            }
            Some(AbcMultiname::RTQName { name }) => (String::new(), self.string(*name).to_owned()),
            Some(AbcMultiname::Multiname { name, ns_set }) => {
                let mut pkg = String::new();
                if let Some(set) = self.ns_sets.get(*ns_set) {
                    for &ns_idx in set {
                        let ns = self.namespace(ns_idx);
                        if ns.kind == NsKind::Package && !ns.name.is_empty() {
                            pkg.clone_from(&ns.name);
                            break;
                        }
                    }
                }
                (pkg, self.string(*name).to_owned())
            }
            Some(AbcMultiname::TypeName { base, params }) => {
                let (bpkg, bname) = self.split_qname(*base);
                let pnames: Vec<String> = params
                    .iter()
                    .map(|&p| self.short_type_name(p, &mut BTreeSet::new()))
                    .collect();
                (bpkg, format!("{bname}.<{}>", pnames.join(", ")))
            }
            Some(AbcMultiname::RTQNameL | AbcMultiname::MultinameL) => (String::new(), "*".into()),
        }
    }

    fn short_type_name(&self, idx: usize, imports: &mut BTreeSet<String>) -> String {
        if idx == 0 {
            return "*".into();
        }
        let (pkg, short) = self.split_qname(idx);
        if !pkg.is_empty() && !short.is_empty() && short != "*" && !short.contains(".<") {
            imports.insert(format!("{pkg}.{short}"));
        }
        if short.is_empty() { "*".into() } else { short }
    }

    fn visibility_keyword(&self, mn_idx: usize, is_interface: bool) -> &'static str {
        if is_interface {
            return "";
        }
        if let Some(AbcMultiname::QName { ns, .. }) = self.multinames.get(mn_idx) {
            match self.namespace(*ns).kind {
                NsKind::Private => "private ",
                NsKind::Protected | NsKind::StaticProtected => "protected ",
                NsKind::PackageInternal => "internal ",
                NsKind::Package | NsKind::Namespace | NsKind::Explicit | NsKind::Any => "public ",
            }
        } else {
            "public "
        }
    }

    fn constant_value(&self, vindex: usize, vkind: u8) -> String {
        match vkind {
            0x03 => self
                .ints
                .get(vindex)
                .map_or_else(|| "0".into(), ToString::to_string),
            0x04 => self
                .uints
                .get(vindex)
                .map_or_else(|| "0".into(), ToString::to_string),
            0x06 => self
                .doubles
                .get(vindex)
                .map_or_else(|| "NaN".into(), |d| format_number(*d)),
            0x01 => quote_as_string(self.string(vindex)),
            0x0B => "true".into(),
            0x0A => "false".into(),
            0x0C => "null".into(),
            0x00 => "undefined".into(),
            _ => "null".into(),
        }
    }

    /// Number of runtime items popped from the stack before the receiver object for a multiname.
    pub(crate) fn runtime_multiname_parts(&self, idx: usize) -> (bool, bool) {
        match self.multinames.get(idx) {
            Some(AbcMultiname::MultinameL) => (false, true),
            Some(AbcMultiname::RTQName { .. }) => (true, false),
            Some(AbcMultiname::RTQNameL) => (true, true),
            _ => (false, false),
        }
    }
}

pub(crate) fn parse_abc_file(bytes: &[u8]) -> Option<AbcFile> {
    let mut r = AbcReader::new(bytes);
    let _minor = r.u16()?;
    let _major = r.u16()?;

    let int_count = r.u30()?;
    let mut ints = vec![0i32];
    for _ in 1..int_count {
        ints.push(r.s32()?);
    }

    let uint_count = r.u30()?;
    let mut uints = vec![0u32];
    for _ in 1..uint_count {
        uints.push(r.u32_var()?);
    }

    let double_count = r.u30()?;
    let mut doubles = vec![f64::NAN];
    for _ in 1..double_count {
        doubles.push(r.d64()?);
    }

    let string_count = r.u30()?;
    let mut strings = vec![String::new()];
    for _ in 1..string_count {
        let len = r.u30()?;
        let raw = r.take(len)?;
        strings.push(String::from_utf8_lossy(raw).into_owned());
    }

    let ns_count = r.u30()?;
    let mut namespaces = vec![AbcNamespace {
        kind: NsKind::Any,
        name: String::new(),
    }];
    for _ in 1..ns_count {
        let kind_byte = r.u8()?;
        let name_idx = r.u30()?;
        let kind = match kind_byte {
            0x05 => NsKind::Private,
            0x08 => NsKind::Namespace,
            0x16 => NsKind::Package,
            0x17 => NsKind::PackageInternal,
            0x18 => NsKind::Protected,
            0x19 => NsKind::Explicit,
            0x1A => NsKind::StaticProtected,
            _ => NsKind::Namespace,
        };
        let name = strings.get(name_idx).cloned().unwrap_or_default();
        namespaces.push(AbcNamespace { kind, name });
    }

    let ns_set_count = r.u30()?;
    let mut ns_sets = vec![Vec::new()];
    for _ in 1..ns_set_count {
        let count = r.u30()?;
        let mut set = Vec::with_capacity(count.min(256));
        for _ in 0..count {
            set.push(r.u30()?);
        }
        ns_sets.push(set);
    }

    let mn_count = r.u30()?;
    let mut multinames = vec![AbcMultiname::Any];
    for _ in 1..mn_count {
        let kind = r.u8()?;
        let mn = match kind {
            0x07 | 0x0D => AbcMultiname::QName {
                ns: r.u30()?,
                name: r.u30()?,
            },
            0x0F | 0x10 => AbcMultiname::RTQName { name: r.u30()? },
            0x11 | 0x12 => AbcMultiname::RTQNameL,
            0x09 | 0x0E => AbcMultiname::Multiname {
                name: r.u30()?,
                ns_set: r.u30()?,
            },
            0x1B | 0x1C => {
                let _ns_set = r.u30()?;
                AbcMultiname::MultinameL
            }
            0x1D => {
                let base = r.u30()?;
                let pcount = r.u30()?;
                let mut params = Vec::with_capacity(pcount.min(16));
                for _ in 0..pcount {
                    params.push(r.u30()?);
                }
                AbcMultiname::TypeName { base, params }
            }
            _ => return None,
        };
        multinames.push(mn);
    }

    let method_count = r.u30()?;
    let mut methods = Vec::with_capacity(method_count.min(16_384));
    for _ in 0..method_count {
        let param_count = r.u30()?;
        let return_type = r.u30()?;
        let mut param_types = Vec::with_capacity(param_count.min(256));
        for _ in 0..param_count {
            param_types.push(r.u30()?);
        }
        let _name_idx = r.u30()?;
        let flags = r.u8()?;
        let mut optional_values = Vec::new();
        if (flags & 0x08) != 0 {
            let opt_count = r.u30()?;
            for _ in 0..opt_count {
                let val = r.u30()?;
                let kind = r.u8()?;
                optional_values.push((val, kind));
            }
        }
        let mut param_names = Vec::new();
        if (flags & 0x80) != 0 {
            for _ in 0..param_count {
                let pn = r.u30()?;
                param_names.push(strings.get(pn).cloned().unwrap_or_default());
            }
        }
        methods.push(AbcMethod {
            param_types,
            return_type,
            flags,
            optional_values,
            param_names,
        });
    }

    let metadata_count = r.u30()?;
    for _ in 0..metadata_count {
        let _name = r.u30()?;
        let item_count = r.u30()?;
        for _ in 0..item_count {
            let _key = r.u30()?;
            let _val = r.u30()?;
        }
    }

    let class_count = r.u30()?;
    let mut instances = Vec::with_capacity(class_count.min(4096));
    for _ in 0..class_count {
        let name = r.u30()?;
        let super_name = r.u30()?;
        let flags = r.u8()?;
        if (flags & 0x08) != 0 {
            let _protected_ns = r.u30()?;
        }
        let intrf_count = r.u30()?;
        let mut interfaces = Vec::with_capacity(intrf_count.min(64));
        for _ in 0..intrf_count {
            interfaces.push(r.u30()?);
        }
        let iinit = r.u30()?;
        let traits = parse_traits(&mut r)?;
        instances.push(AbcInstance {
            name,
            super_name,
            flags,
            interfaces,
            iinit,
            traits,
        });
    }

    let mut classes = Vec::with_capacity(class_count.min(4096));
    for _ in 0..class_count {
        let cinit = r.u30()?;
        let traits = parse_traits(&mut r)?;
        classes.push(AbcClass { cinit, traits });
    }

    let script_count = r.u30()?;
    let mut scripts = Vec::with_capacity(script_count.min(1024));
    for _ in 0..script_count {
        let init = r.u30()?;
        let traits = parse_traits(&mut r)?;
        scripts.push((init, traits));
    }

    let body_count = r.u30()?;
    let mut bodies_by_method = HashMap::with_capacity(body_count.min(16_384));
    for _ in 0..body_count {
        let method = r.u30()?;
        let max_stack = r.u30()?;
        let local_count = r.u30()?;
        let _init_scope = r.u30()?;
        let _max_scope = r.u30()?;
        let code_len = r.u30()?;
        let code = r.take(code_len)?.to_vec();
        let exc_count = r.u30()?;
        let mut exceptions = Vec::with_capacity(exc_count.min(256));
        for _ in 0..exc_count {
            exceptions.push(AbcException {
                from: r.u30()?,
                to: r.u30()?,
                target: r.u30()?,
                exc_type: r.u30()?,
                var_name: r.u30()?,
            });
        }
        let traits = parse_traits(&mut r)?;
        bodies_by_method.insert(
            method,
            AbcMethodBody {
                max_stack,
                local_count,
                code,
                exceptions,
                traits,
            },
        );
    }

    Some(AbcFile {
        ints,
        uints,
        doubles,
        strings,
        namespaces,
        ns_sets,
        multinames,
        methods,
        instances,
        classes,
        scripts,
        bodies_by_method,
    })
}

fn parse_traits(r: &mut AbcReader<'_>) -> Option<Vec<AbcTrait>> {
    let count = r.u30()?;
    let mut out = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let name = r.u30()?;
        let kind_attr = r.u8()?;
        let kind = kind_attr & 0x0F;
        let attr = kind_attr >> 4;
        let data = match kind {
            0 | 6 => {
                let slot_id = r.u30()?;
                let type_name = r.u30()?;
                let vindex = r.u30()?;
                let vkind = if vindex != 0 { r.u8()? } else { 0 };
                AbcTraitData::Slot {
                    slot_id,
                    type_name,
                    vindex,
                    vkind,
                    is_const: kind == 6,
                }
            }
            1 => {
                let _disp_id = r.u30()?;
                AbcTraitData::Method { method: r.u30()? }
            }
            2 => {
                let _disp_id = r.u30()?;
                AbcTraitData::Getter { method: r.u30()? }
            }
            3 => {
                let _disp_id = r.u30()?;
                AbcTraitData::Setter { method: r.u30()? }
            }
            4 => {
                let _slot_id = r.u30()?;
                let class_idx = r.u30()?;
                AbcTraitData::Class { class_idx }
            }
            5 => {
                let _slot_id = r.u30()?;
                AbcTraitData::Function { method: r.u30()? }
            }
            _ => return None,
        };
        if (attr & 0x04) != 0 {
            let mcount = r.u30()?;
            for _ in 0..mcount {
                let _m = r.u30()?;
            }
        }
        out.push(AbcTrait { name, attr, data });
    }
    Some(out)
}

fn sanitize_source_segment(raw: &str) -> String {
    let mut s = String::with_capacity(raw.len());
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '$') {
            s.push(c);
        } else {
            s.push('_');
        }
    }
    let trimmed = s.trim_matches('_');
    if trimmed.is_empty() || matches!(trimmed, "." | "..") {
        String::new()
    } else if trimmed.len() > 80 {
        trimmed[..80].to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn reconstruct_classes(abc: &AbcFile) -> DecompiledAbcBundle {
    let mut bundle = DecompiledAbcBundle::default();

    for (idx, inst) in abc.instances.iter().enumerate() {
        let cls = &abc.classes[idx];
        let (pkg, class_name) = abc.split_qname(inst.name);
        if class_name.is_empty() || class_name == "*" {
            continue;
        }
        let qualified_name = if pkg.is_empty() {
            class_name.clone()
        } else {
            format!("{pkg}.{class_name}")
        };
        let mut rel_parts = Vec::new();
        rel_parts.push("src".to_owned());
        if !pkg.is_empty() {
            for seg in pkg.split(['.', '/', '\\', ':']) {
                let clean = sanitize_source_segment(seg);
                if !clean.is_empty() && rel_parts.len() < 10 {
                    rel_parts.push(clean);
                }
            }
        }
        let clean_class = sanitize_source_segment(&class_name);
        let file_stem = if clean_class.is_empty() {
            format!("Class_{idx}")
        } else {
            clean_class
        };
        rel_parts.push(format!("{file_stem}.as"));
        let source_path = rel_parts.join("/");

        let mut imports = BTreeSet::new();
        let is_interface = (inst.flags & 0x04) != 0;
        let is_dynamic = (inst.flags & 0x01) == 0 && !is_interface;
        let is_final = (inst.flags & 0x02) != 0 && !is_interface;

        let super_short = if inst.super_name != 0 {
            let s = abc.short_type_name(inst.super_name, &mut imports);
            if s == "Object" || s == "*" {
                None
            } else {
                Some(s)
            }
        } else {
            None
        };

        let mut iface_names = Vec::new();
        for &if_mn in &inst.interfaces {
            iface_names.push(abc.short_type_name(if_mn, &mut imports));
        }

        // Pre-decompile all instance methods so we can extract `addFrameScript(...)` targets!
        let mut method_bodies_text: HashMap<String, Vec<String>> = HashMap::new();
        let mut member_blocks: Vec<String> = Vec::new();

        // Static slots/constants
        for tr in &cls.traits {
            if let Some(line) = format_slot_trait(abc, tr, true, &mut imports) {
                member_blocks.push(line);
            }
        }
        // Static initializer (`cinit`) if non-trivial
        if let Some(body) = abc.bodies_by_method.get(&cls.cinit) {
            let mut cinit_calls = Vec::new();
            let cinit_lines = decompile_method_body(
                abc,
                cls.cinit,
                body,
                &class_name,
                false,
                &mut imports,
                &mut cinit_calls,
                &mut bundle.unrecognized_opcodes,
                0,
            );
            if !cinit_lines.is_empty() {
                let mut block = String::from("        {\n");
                for l in cinit_lines {
                    block.push_str("            ");
                    block.push_str(&l);
                    block.push('\n');
                }
                block.push_str("        }");
                member_blocks.push(block);
            }
        }

        // Instance slots/constants
        for tr in &inst.traits {
            if let Some(line) = format_slot_trait(abc, tr, false, &mut imports) {
                member_blocks.push(line);
            }
        }

        // Constructor (`iinit`)
        let mut frame_script_bindings: Vec<(u32, String)> = Vec::new();
        let mut ctor_lines = Vec::new();
        if !is_interface {
            if let Some(body) = abc.bodies_by_method.get(&inst.iinit) {
                ctor_lines = decompile_method_body(
                    abc,
                    inst.iinit,
                    body,
                    &class_name,
                    true,
                    &mut imports,
                    &mut frame_script_bindings,
                    &mut bundle.unrecognized_opcodes,
                    0,
                );
            }
            let sig = format_method_signature(
                abc,
                inst.iinit,
                &class_name,
                "public ",
                "",
                true,
                is_interface,
                &mut imports,
            );
            let mut block = format!("        {sig} {{\n");
            for l in &ctor_lines {
                block.push_str("            ");
                block.push_str(l);
                block.push('\n');
            }
            block.push_str("        }");
            member_blocks.push(block);
        }

        // Instance & static methods/getters/setters
        for (is_static, tr_list) in [(false, &inst.traits), (true, &cls.traits)] {
            for tr in tr_list {
                let (_, mname) = abc.split_qname(tr.name);
                if mname.is_empty() {
                    continue;
                }
                let (accessor, midx) = match tr.data {
                    AbcTraitData::Method { method } | AbcTraitData::Function { method } => {
                        ("", method)
                    }
                    AbcTraitData::Getter { method } => ("get ", method),
                    AbcTraitData::Setter { method } => ("set ", method),
                    _ => continue,
                };
                let mut vis = String::new();
                if (tr.attr & 0x02) != 0 && !is_interface {
                    vis.push_str("override ");
                }
                if (tr.attr & 0x01) != 0 && !is_interface {
                    vis.push_str("final ");
                }
                vis.push_str(abc.visibility_keyword(tr.name, is_interface));
                if is_static {
                    vis.push_str("static ");
                }
                let sig = format_method_signature(
                    abc,
                    midx,
                    &mname,
                    &vis,
                    accessor,
                    false,
                    is_interface,
                    &mut imports,
                );
                if is_interface {
                    member_blocks.push(format!("        {sig};"));
                    continue;
                }
                let mlines = if let Some(body) = abc.bodies_by_method.get(&midx) {
                    let mut dummy = Vec::new();
                    decompile_method_body(
                        abc,
                        midx,
                        body,
                        &class_name,
                        false,
                        &mut imports,
                        &mut dummy,
                        &mut bundle.unrecognized_opcodes,
                        0,
                    )
                } else {
                    Vec::new()
                };
                if !is_static && accessor.is_empty() {
                    method_bodies_text.insert(mname.clone(), mlines.clone());
                }
                let mut block = format!("        {sig} {{\n");
                for l in mlines {
                    block.push_str("            ");
                    block.push_str(&l);
                    block.push('\n');
                }
                block.push_str("        }");
                member_blocks.push(block);
            }
        }

        // Resolve `(frame_index, method_name)` bindings into `(frame_index, frame_script_body)`!
        let mut bound_method_names = BTreeSet::new();
        let mut timeline_frame_scripts = Vec::new();
        for (frame_idx, fn_name) in &frame_script_bindings {
            bound_method_names.insert(fn_name.clone());
            if let Some(mlines) = method_bodies_text.get(fn_name) {
                let body_src = mlines.join("\n");
                if !body_src.trim().is_empty() {
                    timeline_frame_scripts.push((*frame_idx, body_src));
                }
            }
        }

        // Determine if this class is an auto-generated Flash IDE timeline class (`*_fla.MainTimeline`, etc.):
        // 1. Has at least one `addFrameScript(...)` binding (or lives in `*_fla` package),
        // 2. Constructor lines contain ONLY `super();` and `this.addFrameScript(...);` / `addFrameScript(...);`,
        // 3. Every instance method is one of the `addFrameScript` callbacks, and there are no static traits.
        let ctor_only_framescript = ctor_lines.iter().all(|l| {
            let t = l.trim();
            t == "super();"
                || t.starts_with("this.addFrameScript(")
                || t.starts_with("addFrameScript(")
        });
        let all_methods_are_framescripts = !method_bodies_text.is_empty()
            && method_bodies_text
                .keys()
                .all(|k| bound_method_names.contains(k));
        let is_pure_timeline_class = !timeline_frame_scripts.is_empty()
            && ctor_only_framescript
            && all_methods_are_framescripts
            && cls.traits.is_empty()
            && (pkg.ends_with("_fla")
                || class_name == "MainTimeline"
                || class_name.starts_with("Timeline_"));

        // Assemble `.as` file text
        let pkg_header = if pkg.is_empty() {
            "package {".to_owned()
        } else {
            format!("package {pkg} {{")
        };
        let mut out = String::new();
        out.push_str(&pkg_header);
        out.push('\n');
        let own_qname = qualified_name.as_str();
        for imp in &imports {
            if imp != own_qname {
                out.push_str("    import ");
                out.push_str(imp);
                out.push_str(";\n");
            }
        }
        if !imports.is_empty() {
            out.push('\n');
        }
        out.push_str("    public ");
        if is_dynamic {
            out.push_str("dynamic ");
        }
        if is_final {
            out.push_str("final ");
        }
        if is_interface {
            out.push_str("interface ");
        } else {
            out.push_str("class ");
        }
        out.push_str(&class_name);
        if let Some(sup) = super_short {
            out.push_str(&format!(" extends {sup}"));
        }
        if !iface_names.is_empty() {
            let kw = if is_interface {
                "extends"
            } else {
                "implements"
            };
            out.push_str(&format!(" {kw} {}", iface_names.join(", ")));
        }
        out.push_str(" {\n");
        for (bi, block) in member_blocks.iter().enumerate() {
            if bi > 0 {
                out.push('\n');
            }
            out.push_str(block);
            out.push('\n');
        }
        out.push_str("    }\n}\n");

        bundle.classes.push(RecoveredAs3Class {
            qualified_name,
            source_path,
            source: out,
            timeline_frame_scripts,
            is_pure_timeline_class,
        });
    }

    bundle
}

fn format_slot_trait(
    abc: &AbcFile,
    tr: &AbcTrait,
    is_static: bool,
    imports: &mut BTreeSet<String>,
) -> Option<String> {
    let AbcTraitData::Slot {
        type_name,
        vindex,
        vkind,
        is_const,
        ..
    } = tr.data
    else {
        return None;
    };
    let (_, name) = abc.split_qname(tr.name);
    if name.is_empty() {
        return None;
    }
    let vis = abc.visibility_keyword(tr.name, false);
    let static_kw = if is_static { "static " } else { "" };
    let decl_kw = if is_const { "const" } else { "var" };
    let ty = abc.short_type_name(type_name, imports);
    let init = if vindex != 0 {
        format!(" = {}", abc.constant_value(vindex, vkind))
    } else {
        String::new()
    };
    Some(format!(
        "        {vis}{static_kw}{decl_kw} {name}:{ty}{init};"
    ))
}

#[allow(clippy::too_many_arguments)]
fn format_method_signature(
    abc: &AbcFile,
    method_idx: usize,
    name: &str,
    vis_prefix: &str,
    accessor: &str,
    is_ctor: bool,
    is_interface: bool,
    imports: &mut BTreeSet<String>,
) -> String {
    let Some(m) = abc.methods.get(method_idx) else {
        return format!("{vis_prefix}function {accessor}{name}()");
    };
    let param_count = m.param_types.len();
    let opt_start = param_count.saturating_sub(m.optional_values.len());
    let mut params = Vec::with_capacity(param_count + 1);
    for (i, &pty) in m.param_types.iter().enumerate() {
        let pname = m
            .param_names
            .get(i)
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("arg{}", i + 1));
        let tname = abc.short_type_name(pty, imports);
        if i >= opt_start
            && let Some(&(vindex, vkind)) = m.optional_values.get(i - opt_start)
        {
            params.push(format!(
                "{pname}:{tname} = {}",
                abc.constant_value(vindex, vkind)
            ));
        } else {
            params.push(format!("{pname}:{tname}"));
        }
    }
    if (m.flags & 0x04) != 0 {
        params.push("...rest".into());
    }
    let prefix = if is_interface { "" } else { vis_prefix };
    if is_ctor {
        format!("{prefix}function {name}({})", params.join(", "))
    } else {
        let ret = if m.return_type == 0 {
            if accessor == "set " {
                "void".into()
            } else {
                "*".into()
            }
        } else {
            abc.short_type_name(m.return_type, imports)
        };
        format!(
            "{prefix}function {accessor}{name}({}):{ret}",
            params.join(", ")
        )
    }
}

#[derive(Clone, Debug)]
struct AbcInsn {
    offset: usize,
    next_offset: usize,
    op: u8,
    arg1: usize,
    arg2: usize,
    iarg: i32,
}

fn decode_abc_instructions(code: &[u8], unrecognized: &mut usize) -> Vec<AbcInsn> {
    let mut r = AbcReader::new(code);
    let mut out = Vec::new();
    while r.pos < code.len() && out.len() < MAX_ABC_INSNS {
        let offset = r.pos;
        let Some(op) = r.u8() else { break };
        let mut arg1 = 0usize;
        let mut arg2 = 0usize;
        let mut iarg = 0i32;

        let ok: Option<()> = (|| {
            match op {
                // 0 operands
                0x01..=0x03
                | 0x07
                | 0x09
                | 0x1C..=0x21
                | 0x23
                | 0x26..=0x2B
                | 0x30
                | 0x35..=0x3E
                | 0x47
                | 0x48
                | 0x50..=0x52
                | 0x57
                | 0x64
                | 0x70..=0x7B
                | 0x82
                | 0x85
                | 0x87
                | 0x89
                | 0x90
                | 0x91
                | 0x93
                | 0x95..=0x97
                | 0xA0..=0xB1
                | 0xB3
                | 0xB4
                | 0xC0
                | 0xC1
                | 0xC4..=0xC7
                | 0xD0..=0xD7
                | 0xF3 => {}

                // s24 branch offset
                0x0C..=0x1A => {
                    iarg = r.s24()?;
                }

                // lookupswitch: s24 default + u30 max_index + (max_index + 1) * s24
                0x1B => {
                    iarg = r.s24()?;
                    let max_idx = r.u30()?;
                    for _ in 0..=max_idx.min(4096) {
                        let _ = r.s24()?;
                    }
                }

                // 1 byte immediate
                0x24 => {
                    iarg = i32::from(r.i8()?);
                }
                0x65 => {
                    arg1 = usize::from(r.u8()?);
                }

                // pushshort (u30 truncated to i16)
                0x25 => {
                    iarg = i32::from(r.u32_var()? as i16);
                }

                // 1 u30 operand
                0x04..=0x06
                | 0x08
                | 0x2C..=0x2F
                | 0x31
                | 0x40..=0x42
                | 0x49
                | 0x53
                | 0x55
                | 0x56
                | 0x58..=0x5A
                | 0x5D..=0x63
                | 0x66..=0x68
                | 0x6A
                | 0x6C..=0x6F
                | 0x80
                | 0x86
                | 0x92
                | 0x94
                | 0xB2
                | 0xC2
                | 0xC3
                | 0xF0..=0xF2 => {
                    arg1 = r.u30()?;
                }

                // 2 u30 operands
                0x32 | 0x43..=0x46 | 0x4A | 0x4C | 0x4E | 0x4F => {
                    arg1 = r.u30()?;
                    arg2 = r.u30()?;
                }

                // OP_debug: u8 debug_type, u30 name_str, u8 reg, u30 extra
                0xEF => {
                    let dtype = r.u8()?;
                    let name_idx = r.u30()?;
                    let reg = r.u8()?;
                    let _extra = r.u30()?;
                    if dtype == 1 {
                        arg1 = usize::from(reg) + 1;
                        arg2 = name_idx;
                    }
                }

                _ => {
                    *unrecognized += 1;
                    return None;
                }
            }
            Some(())
        })();

        if ok.is_none() {
            break;
        }
        out.push(AbcInsn {
            offset,
            next_offset: r.pos,
            op,
            arg1,
            arg2,
            iarg,
        });
    }
    out
}

#[derive(Clone, Debug)]
struct AbcVal {
    code: String,
    is_this_or_find: bool,
    int_lit: Option<i64>,
    method_ref: Option<String>,
}

impl AbcVal {
    fn expr(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            is_this_or_find: false,
            int_lit: None,
            method_ref: None,
        }
    }

    fn int(v: i64) -> Self {
        Self {
            code: v.to_string(),
            is_this_or_find: false,
            int_lit: Some(v),
            method_ref: None,
        }
    }

    fn find_target() -> Self {
        Self {
            code: "this".into(),
            is_this_or_find: true,
            int_lit: None,
            method_ref: None,
        }
    }
}

struct BodyContext<'a> {
    abc: &'a AbcFile,
    class_name: &'a str,
    is_ctor: bool,
    local_names: HashMap<usize, String>,
    declared_locals: BTreeSet<usize>,
    param_count: usize,
    activation_slots: HashMap<usize, String>,
    imports: &'a mut BTreeSet<String>,
    frame_scripts: &'a mut Vec<(u32, String)>,
    unrecognized: &'a mut usize,
    depth: usize,
}

impl BodyContext<'_> {
    fn local_name(&mut self, reg: usize) -> String {
        if reg == 0 {
            return "this".into();
        }
        self.local_names
            .entry(reg)
            .or_insert_with(|| format!("loc{reg}"))
            .clone()
    }
}

#[allow(clippy::too_many_arguments)]
fn decompile_method_body(
    abc: &AbcFile,
    method_idx: usize,
    body: &AbcMethodBody,
    class_name: &str,
    is_ctor: bool,
    imports: &mut BTreeSet<String>,
    frame_scripts: &mut Vec<(u32, String)>,
    unrecognized: &mut usize,
    depth: usize,
) -> Vec<String> {
    if depth > MAX_ABC_DEPTH {
        return Vec::new();
    }
    let insns = decode_abc_instructions(&body.code, unrecognized);
    if insns.is_empty() {
        return Vec::new();
    }

    let mut local_names = HashMap::new();
    local_names.insert(0, "this".into());
    let mut param_count = 0usize;
    if let Some(m) = abc.methods.get(method_idx) {
        param_count = m.param_types.len();
        for i in 0..param_count {
            let pname = m
                .param_names
                .get(i)
                .filter(|s| !s.is_empty())
                .cloned()
                .unwrap_or_else(|| format!("arg{}", i + 1));
            local_names.insert(i + 1, pname);
        }
        if (m.flags & 0x04) != 0 {
            local_names.insert(param_count + 1, "rest".into());
            param_count += 1;
        } else if (m.flags & 0x01) != 0 {
            local_names.insert(param_count + 1, "arguments".into());
            param_count += 1;
        }
    }
    // Harvest OP_debug local variable names when present.
    for insn in &insns {
        if insn.op == 0xEF && insn.arg1 > 0 && insn.arg2 > 0 {
            let dname = abc.string(insn.arg2);
            if !dname.is_empty() {
                local_names
                    .entry(insn.arg1)
                    .or_insert_with(|| dname.to_owned());
            }
        }
    }
    // Harvest activation trait slot names (`slot_id` -> variable name) for closure captures.
    let mut activation_slots = HashMap::new();
    for (ti, tr) in body.traits.iter().enumerate() {
        if let AbcTraitData::Slot { slot_id, .. } = tr.data {
            let (_, sname) = abc.split_qname(tr.name);
            let resolved_slot = if slot_id == 0 { ti + 1 } else { slot_id };
            if !sname.is_empty() {
                activation_slots.insert(resolved_slot, sname);
            }
        }
    }

    let mut bctx = BodyContext {
        abc,
        class_name,
        is_ctor,
        local_names,
        declared_locals: BTreeSet::new(),
        param_count,
        activation_slots,
        imports,
        frame_scripts,
        unrecognized,
        depth,
    };

    let mut stack = Vec::new();
    let mut scope_stack = Vec::new();
    decompile_abc_range(
        &insns,
        0,
        insns.len(),
        &body.exceptions,
        &mut bctx,
        &mut stack,
        &mut scope_stack,
        0,
    )
}

fn find_abc_insn(insns: &[AbcInsn], start: usize, end: usize, byte_off: isize) -> Option<usize> {
    if byte_off < 0 {
        return None;
    }
    let target = byte_off as usize;
    if let Some(last) = insns.get(end.saturating_sub(1))
        && target == last.next_offset
    {
        return Some(end);
    }
    (start..end).find(|&i| insns[i].offset == target)
}

#[allow(clippy::too_many_arguments)]
fn decompile_abc_range(
    insns: &[AbcInsn],
    start: usize,
    end: usize,
    exceptions: &[AbcException],
    ctx: &mut BodyContext<'_>,
    stack: &mut Vec<AbcVal>,
    scope_stack: &mut Vec<String>,
    depth: usize,
) -> Vec<String> {
    let mut lines = Vec::new();
    if depth > MAX_ABC_DEPTH || start >= end {
        return lines;
    }

    // Detect AVM2 loops where a forward `jump(cond_off)` jumps directly to the loop condition at the bottom,
    // which ends with a backward conditional branch (`iftrue`/`iflt`/etc.) back to `body_start = pc + 1`!
    let mut pc = start;
    while pc < end {
        let insn = &insns[pc];

        // Check if an exception `try` block starts right at `insn.offset` (only at outer level or non-nested boundary).
        if depth < 8
            && let Some(exc) = exceptions.iter().find(|e| e.from == insn.offset)
            && let Some(try_end_idx) = find_abc_insn(insns, pc + 1, end, exc.to as isize)
            && let Some(catch_start_idx) =
                find_abc_insn(insns, try_end_idx, end, exc.target as isize)
            && try_end_idx > pc
        {
            // Determine catch end: if `try_end_idx - 1` is a forward `jump` past the catch block, use its target!
            let catch_end_idx = if try_end_idx > pc
                && insns[try_end_idx - 1].op == 0x10
                && insns[try_end_idx - 1].iarg > 0
                && let Some(after_catch) = find_abc_insn(
                    insns,
                    catch_start_idx + 1,
                    end,
                    insns[try_end_idx - 1].next_offset as isize
                        + insns[try_end_idx - 1].iarg as isize,
                ) {
                after_catch
            } else {
                end
            };
            if catch_end_idx > catch_start_idx {
                let try_inner_end = if insns[try_end_idx - 1].op == 0x10 {
                    try_end_idx - 1
                } else {
                    try_end_idx
                };
                let mut t_stack = Vec::new();
                let mut t_scope = scope_stack.clone();
                let t_lines = decompile_abc_range(
                    insns,
                    pc,
                    try_inner_end,
                    &[],
                    ctx,
                    &mut t_stack,
                    &mut t_scope,
                    depth + 1,
                );
                let err_var = if exc.var_name != 0 {
                    let (_, vn) = ctx.abc.split_qname(exc.var_name);
                    if vn.is_empty() || vn == "*" {
                        "err".to_owned()
                    } else {
                        vn
                    }
                } else {
                    "err".to_owned()
                };
                let err_ty = ctx.abc.short_type_name(exc.exc_type, ctx.imports);
                let mut c_stack = vec![AbcVal::expr(err_var.clone())];
                let mut c_scope = scope_stack.clone();
                let c_lines = decompile_abc_range(
                    insns,
                    catch_start_idx,
                    catch_end_idx,
                    &[],
                    ctx,
                    &mut c_stack,
                    &mut c_scope,
                    depth + 1,
                );
                lines.push("try {".into());
                for tl in t_lines {
                    lines.push(format!("    {tl}"));
                }
                lines.push(format!("}} catch ({err_var}:{err_ty}) {{"));
                for cl in c_lines {
                    lines.push(format!("    {cl}"));
                }
                lines.push("}".into());
                pc = catch_end_idx;
                continue;
            }
        }

        // Standard ASC `while (cond) { body }`: `jump +cond_off`, `L_body: ...`, `L_cond: ... if_op L_body`
        if insn.op == 0x10 && insn.iarg > 0 {
            let cond_byte = insn.next_offset as isize + insn.iarg as isize;
            if let Some(cond_idx) = find_abc_insn(insns, pc + 1, end, cond_byte)
                && let Some((back_br_idx, loop_cond_str)) =
                    match_backward_loop_condition(insns, pc + 1, cond_idx, end, ctx)
            {
                let mut body_stack = Vec::new();
                let mut body_scope = scope_stack.clone();
                let body_lines = decompile_abc_range(
                    insns,
                    pc + 1,
                    cond_idx,
                    exceptions,
                    ctx,
                    &mut body_stack,
                    &mut body_scope,
                    depth + 1,
                );
                lines.push(format!("while ({loop_cond_str}) {{"));
                for bl in body_lines {
                    lines.push(format!("    {bl}"));
                }
                lines.push("}".into());
                pc = back_br_idx + 1;
                continue;
            }
            if let Some(target_idx) = find_abc_insn(insns, pc + 1, end, cond_byte) {
                pc = target_idx;
                continue;
            }
        }

        // Short-circuit `&&` / `||` (`dup` + `iffalse`/`iftrue` + `pop` + `rhs`)
        if pc > start
            && insns[pc - 1].op == 0x2A
            && matches!(insn.op, 0x11 | 0x12)
            && insn.iarg > 0
            && insns.get(pc + 1).is_some_and(|n| n.op == 0x29)
        {
            let target_byte = insn.next_offset as isize + insn.iarg as isize;
            if let Some(target_idx) = find_abc_insn(insns, pc + 2, end, target_byte)
                && stack.len() >= 2
            {
                let mut rhs_stack = stack.clone();
                let _dup_lhs = pop_val(&mut rhs_stack);
                let lhs = pop_val(&mut rhs_stack);
                let mut rhs_scope = scope_stack.clone();
                let rhs_lines = decompile_abc_range(
                    insns,
                    pc + 2,
                    target_idx,
                    exceptions,
                    ctx,
                    &mut rhs_stack,
                    &mut rhs_scope,
                    depth + 1,
                );
                if rhs_lines.is_empty() && rhs_stack.len() == stack.len() - 1 {
                    let rhs = pop_val(&mut rhs_stack);
                    let _ = pop_val(stack);
                    let _ = pop_val(stack);
                    let op_str = if insn.op == 0x12 { "&&" } else { "||" };
                    stack.push(AbcVal::expr(format!(
                        "({} {op_str} {})",
                        lhs.code, rhs.code
                    )));
                    pc = target_idx;
                    continue;
                }
            }
        }

        // Forward conditional branch (`if*` `0x0C..=0x0F` and `0x11..=0x1A` except `0x10` `jump`)
        if matches!(insn.op, 0x0C..=0x0F | 0x11..=0x1A) && insn.iarg > 0 {
            let skip_cond = eval_branch_condition(insn.op, stack);
            let target_byte = insn.next_offset as isize + insn.iarg as isize;
            if let Some(target_idx) = find_abc_insn(insns, pc + 1, end, target_byte)
                && target_idx > pc + 1
            {
                let enter_cond = invert_as3_cond(&skip_cond);
                // Check for `else` branch (`jump +else_len` at `target_idx - 1`)
                if target_idx >= pc + 2
                    && insns[target_idx - 1].op == 0x10
                    && insns[target_idx - 1].iarg > 0
                    && let Some(else_end) = find_abc_insn(
                        insns,
                        target_idx,
                        end,
                        insns[target_idx - 1].next_offset as isize
                            + insns[target_idx - 1].iarg as isize,
                    )
                    && else_end > target_idx
                {
                    let mut then_stack = stack.clone();
                    let mut then_scope = scope_stack.clone();
                    let then_lines = decompile_abc_range(
                        insns,
                        pc + 1,
                        target_idx - 1,
                        exceptions,
                        ctx,
                        &mut then_stack,
                        &mut then_scope,
                        depth + 1,
                    );
                    let mut else_stack = stack.clone();
                    let mut else_scope = scope_stack.clone();
                    let else_lines = decompile_abc_range(
                        insns,
                        target_idx,
                        else_end,
                        exceptions,
                        ctx,
                        &mut else_stack,
                        &mut else_scope,
                        depth + 1,
                    );
                    if then_lines.is_empty()
                        && else_lines.is_empty()
                        && then_stack.len() == stack.len() + 1
                        && else_stack.len() == stack.len() + 1
                    {
                        let tv = pop_val(&mut then_stack);
                        let ev = pop_val(&mut else_stack);
                        stack.push(AbcVal::expr(format!(
                            "({enter_cond} ? {} : {})",
                            tv.code, ev.code
                        )));
                    } else {
                        lines.push(format!("if ({enter_cond}) {{"));
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
                let mut then_scope = scope_stack.clone();
                let then_lines = decompile_abc_range(
                    insns,
                    pc + 1,
                    target_idx,
                    exceptions,
                    ctx,
                    &mut then_stack,
                    &mut then_scope,
                    depth + 1,
                );
                lines.push(format!("if ({enter_cond}) {{"));
                for tl in then_lines {
                    lines.push(format!("    {tl}"));
                }
                lines.push("}".into());
                pc = target_idx;
                continue;
            }
        }

        step_abc_insn(insn, ctx, stack, scope_stack, &mut lines);
        pc += 1;
    }

    // Trim trailing implicit `return;` on void methods / constructors for clean idiomatic AS3.
    if lines.last().is_some_and(|l| l == "return;") {
        lines.pop();
    }
    lines
}

fn match_backward_loop_condition(
    insns: &[AbcInsn],
    body_start_idx: usize,
    cond_start_idx: usize,
    end: usize,
    ctx: &mut BodyContext<'_>,
) -> Option<(usize, String)> {
    let body_byte = insns.get(body_start_idx)?.offset as isize;
    let max_scan = (cond_start_idx + 24).min(end);
    for br_idx in cond_start_idx..max_scan {
        let br = &insns[br_idx];
        if matches!(br.op, 0x0C..=0x0F | 0x11..=0x1A) && br.iarg < 0 {
            let target = br.next_offset as isize + br.iarg as isize;
            if target == body_byte {
                let mut cstack = Vec::new();
                let mut cscope = Vec::new();
                let _ = decompile_abc_range(
                    insns,
                    cond_start_idx,
                    br_idx,
                    &[],
                    ctx,
                    &mut cstack,
                    &mut cscope,
                    ctx.depth + 1,
                );
                let cond = eval_branch_condition(br.op, &mut cstack);
                return Some((br_idx, cond));
            }
        }
    }
    None
}

fn eval_branch_condition(op: u8, stack: &mut Vec<AbcVal>) -> String {
    match op {
        0x11 => pop_val(stack).code,
        0x12 => invert_as3_cond(&pop_val(stack).code),
        0x13 => {
            let b = pop_val(stack);
            let a = pop_val(stack);
            format!("({} == {})", a.code, b.code)
        }
        0x14 => {
            let b = pop_val(stack);
            let a = pop_val(stack);
            format!("({} != {})", a.code, b.code)
        }
        0x15 | 0x0F => {
            let b = pop_val(stack);
            let a = pop_val(stack);
            format!("({} < {})", a.code, b.code)
        }
        0x16 | 0x0E => {
            let b = pop_val(stack);
            let a = pop_val(stack);
            format!("({} <= {})", a.code, b.code)
        }
        0x17 | 0x0D => {
            let b = pop_val(stack);
            let a = pop_val(stack);
            format!("({} > {})", a.code, b.code)
        }
        0x18 | 0x0C => {
            let b = pop_val(stack);
            let a = pop_val(stack);
            format!("({} >= {})", a.code, b.code)
        }
        0x19 => {
            let b = pop_val(stack);
            let a = pop_val(stack);
            format!("({} === {})", a.code, b.code)
        }
        0x1A => {
            let b = pop_val(stack);
            let a = pop_val(stack);
            format!("({} !== {})", a.code, b.code)
        }
        _ => "true".into(),
    }
}

fn invert_as3_cond(c: &str) -> String {
    if let Some(rest) = c.strip_prefix("!(").and_then(|s| s.strip_suffix(')')) {
        return rest.to_owned();
    }
    for (op, inv) in [
        (" === ", " !== "),
        (" !== ", " === "),
        (" == ", " != "),
        (" != ", " == "),
        (" <= ", " > "),
        (" >= ", " < "),
        (" < ", " >= "),
        (" > ", " <= "),
    ] {
        if let Some(inner) = c.strip_prefix('(').and_then(|s| s.strip_suffix(')'))
            && let Some((lhs, rhs)) = inner.split_once(op)
            && !lhs.contains('(')
            && !rhs.contains(')')
        {
            return format!("({lhs}{inv}{rhs})");
        }
    }
    format!("!({c})")
}

fn pop_val(stack: &mut Vec<AbcVal>) -> AbcVal {
    stack.pop().unwrap_or_else(|| AbcVal::expr("undefined"))
}

fn pop_n_args(stack: &mut Vec<AbcVal>, n: usize) -> Vec<AbcVal> {
    let count = n.min(256);
    let mut args = Vec::with_capacity(count);
    for _ in 0..count {
        args.push(pop_val(stack));
    }
    args.reverse();
    args
}

fn step_abc_insn(
    insn: &AbcInsn,
    ctx: &mut BodyContext<'_>,
    stack: &mut Vec<AbcVal>,
    scope_stack: &mut Vec<String>,
    lines: &mut Vec<String>,
) {
    match insn.op {
        0x03 => {
            let err = pop_val(stack);
            lines.push(format!("throw {};", err.code));
        }
        0x04 => {
            let (_, prop) = resolve_prop_name(ctx, insn.arg1, stack);
            let obj = pop_val(stack);
            let _ = obj;
            stack.push(AbcVal::expr(format!("super.{prop}")));
        }
        0x05 => {
            let val = pop_val(stack);
            let (_, prop) = resolve_prop_name(ctx, insn.arg1, stack);
            let _obj = pop_val(stack);
            lines.push(format!("super.{prop} = {};", val.code));
        }
        0x08 => {
            // kill local register — no source line needed
        }
        0x1C | 0x30 => {
            let sc = pop_val(stack);
            scope_stack.push(sc.code);
        }
        0x1D => {
            let _ = scope_stack.pop();
        }
        0x1E | 0x23 => {
            let idx = pop_val(stack);
            let obj = pop_val(stack);
            stack.push(AbcVal::expr(format!("{}[{}]", obj.code, idx.code)));
        }
        0x1F => {
            let cur = pop_val(stack);
            let obj = pop_val(stack);
            stack.push(AbcVal::expr(format!(
                "/* hasnext({}, {}) */",
                obj.code, cur.code
            )));
        }
        0x20 => stack.push(AbcVal::expr("null")),
        0x21 => stack.push(AbcVal::expr("undefined")),
        0x24 | 0x25 => stack.push(AbcVal::int(i64::from(insn.iarg))),
        0x26 => stack.push(AbcVal::expr("true")),
        0x27 => stack.push(AbcVal::expr("false")),
        0x28 => stack.push(AbcVal::expr("NaN")),
        0x29 => {
            if let Some(v) = stack.pop()
                && !v.is_this_or_find
                && (v.code.ends_with(')')
                    || v.code.starts_with("delete ")
                    || v.code.starts_with("new "))
            {
                lines.push(format!("{};", v.code));
            }
        }
        0x2A => {
            if let Some(top) = stack.last().cloned() {
                stack.push(top);
            }
        }
        0x2B => {
            let len = stack.len();
            if len >= 2 {
                stack.swap(len - 1, len - 2);
            }
        }
        0x2C => stack.push(AbcVal::expr(quote_as_string(ctx.abc.string(insn.arg1)))),
        0x2D => {
            let v = ctx.abc.ints.get(insn.arg1).copied().unwrap_or(0);
            stack.push(AbcVal::int(i64::from(v)));
        }
        0x2E => {
            let v = ctx.abc.uints.get(insn.arg1).copied().unwrap_or(0);
            stack.push(AbcVal::int(i64::from(v)));
        }
        0x2F => {
            let v = ctx.abc.doubles.get(insn.arg1).copied().unwrap_or(f64::NAN);
            stack.push(AbcVal::expr(format_number(v)));
        }
        0x32 => {
            // hasnext2(obj_reg, idx_reg)
            let obj_r = ctx.local_name(insn.arg1);
            let idx_r = ctx.local_name(insn.arg2);
            stack.push(AbcVal::expr(format!("({idx_r} < {obj_r}.length)")));
        }
        0x40 => {
            // newfunction(method_idx): inline anonymous closure expression!
            let midx = insn.arg1;
            let sig = format_method_signature(ctx.abc, midx, "", "", "", false, false, ctx.imports)
                .replacen("function ()", "function()", 1)
                .replacen("function (", "function(", 1);
            let closure_lines = if let Some(cbody) = ctx.abc.bodies_by_method.get(&midx) {
                let mut dummy = Vec::new();
                decompile_method_body(
                    ctx.abc,
                    midx,
                    cbody,
                    ctx.class_name,
                    false,
                    ctx.imports,
                    &mut dummy,
                    ctx.unrecognized,
                    ctx.depth + 1,
                )
            } else {
                Vec::new()
            };
            let mut expr = format!("{sig} {{");
            if closure_lines.is_empty() {
                expr.push('}');
            } else {
                expr.push('\n');
                for cl in closure_lines {
                    expr.push_str("    ");
                    expr.push_str(&cl);
                    expr.push('\n');
                }
                expr.push('}');
            }
            stack.push(AbcVal::expr(expr));
        }
        0x41 => {
            let args = pop_n_args(stack, insn.arg1);
            let _recv = pop_val(stack);
            let func = pop_val(stack);
            let arg_strs: Vec<&str> = args.iter().map(|a| a.code.as_str()).collect();
            stack.push(AbcVal::expr(format!(
                "{}({})",
                func.code,
                arg_strs.join(", ")
            )));
        }
        0x42 => {
            let args = pop_n_args(stack, insn.arg1);
            let ctor = pop_val(stack);
            let arg_strs: Vec<&str> = args.iter().map(|a| a.code.as_str()).collect();
            stack.push(AbcVal::expr(format!(
                "new {}({})",
                ctor.code,
                arg_strs.join(", ")
            )));
        }
        0x45 | 0x4E => {
            let args = pop_n_args(stack, insn.arg2);
            let (_, prop) = resolve_prop_name(ctx, insn.arg1, stack);
            let _recv = pop_val(stack);
            let arg_strs: Vec<&str> = args.iter().map(|a| a.code.as_str()).collect();
            let call = format!("super.{prop}({})", arg_strs.join(", "));
            if insn.op == 0x4E {
                lines.push(format!("{call};"));
            } else {
                stack.push(AbcVal::expr(call));
            }
        }
        0x46 | 0x4C | 0x4F => {
            let args = pop_n_args(stack, insn.arg2);
            let (is_bracket, prop) = resolve_prop_name(ctx, insn.arg1, stack);
            let recv = pop_val(stack);
            if ctx.is_ctor && prop == "addFrameScript" {
                record_add_frame_script_args(&args, ctx.frame_scripts);
            }
            let arg_strs: Vec<&str> = args.iter().map(|a| a.code.as_str()).collect();
            let callee = format_receiver_member(&recv, &prop, is_bracket);
            let call = format!("{callee}({})", arg_strs.join(", "));
            if insn.op == 0x4F {
                lines.push(format!("{call};"));
            } else {
                stack.push(AbcVal::expr(call));
            }
        }
        0x47 => lines.push("return;".into()),
        0x48 => {
            let v = pop_val(stack);
            lines.push(format!("return {};", v.code));
        }
        0x49 => {
            let args = pop_n_args(stack, insn.arg1);
            let _recv = pop_val(stack);
            let arg_strs: Vec<&str> = args.iter().map(|a| a.code.as_str()).collect();
            lines.push(format!("super({});", arg_strs.join(", ")));
        }
        0x4A => {
            let args = pop_n_args(stack, insn.arg2);
            let (_, prop) = resolve_prop_name(ctx, insn.arg1, stack);
            let recv = pop_val(stack);
            let arg_strs: Vec<&str> = args.iter().map(|a| a.code.as_str()).collect();
            let target = if recv.is_this_or_find {
                prop
            } else {
                format!("{}.{prop}", recv.code)
            };
            stack.push(AbcVal::expr(format!(
                "new {target}({})",
                arg_strs.join(", ")
            )));
        }
        0x53 => {
            let targs = pop_n_args(stack, insn.arg1);
            let base = pop_val(stack);
            let tstrs: Vec<&str> = targs.iter().map(|a| a.code.as_str()).collect();
            stack.push(AbcVal::expr(format!(
                "{}.<{}>",
                base.code,
                tstrs.join(", ")
            )));
        }
        0x55 => {
            let count = insn.arg1.min(256);
            let mut pairs = Vec::with_capacity(count);
            for _ in 0..count {
                let val = pop_val(stack);
                let key = pop_val(stack);
                pairs.push(format!("{}: {}", key.code, val.code));
            }
            pairs.reverse();
            stack.push(AbcVal::expr(format!("{{{}}}", pairs.join(", "))));
        }
        0x56 => {
            let elems = pop_n_args(stack, insn.arg1);
            let estrs: Vec<&str> = elems.iter().map(|a| a.code.as_str()).collect();
            stack.push(AbcVal::expr(format!("[{}]", estrs.join(", "))));
        }
        0x57 => {
            // newactivation: push activation marker so activation slot accesses resolve directly to variable names.
            stack.push(AbcVal::find_target());
        }
        0x5D..=0x5F => {
            let (has_ns, has_name) = ctx.abc.runtime_multiname_parts(insn.arg1);
            if has_name {
                let _ = pop_val(stack);
            }
            if has_ns {
                let _ = pop_val(stack);
            }
            // Also register import if this multiname references an imported package type (e.g. `flash.display.MovieClip`).
            let _ = ctx.abc.short_type_name(insn.arg1, ctx.imports);
            stack.push(AbcVal::find_target());
        }
        0x60 => {
            let short = ctx.abc.short_type_name(insn.arg1, ctx.imports);
            let mut val = AbcVal::expr(short.clone());
            val.method_ref = Some(short);
            stack.push(val);
        }
        0x61 | 0x68 => {
            let val = pop_val(stack);
            let (is_bracket, prop) = resolve_prop_name(ctx, insn.arg1, stack);
            let obj = pop_val(stack);
            let lhs = format_receiver_member(&obj, &prop, is_bracket);
            lines.push(format!("{lhs} = {};", val.code));
        }
        0x62 => {
            let name = ctx.local_name(insn.arg1);
            let mut v = AbcVal::expr(name);
            if insn.arg1 == 0 {
                v.is_this_or_find = true;
            }
            stack.push(v);
        }
        0x63 => {
            store_local(ctx, insn.arg1, stack, lines);
        }
        0x64 => stack.push(AbcVal::find_target()),
        0x65 | 0x67 => {
            let sc = scope_stack
                .get(insn.arg1)
                .cloned()
                .unwrap_or_else(|| "this".into());
            let mut v = AbcVal::expr(sc);
            v.is_this_or_find = true;
            stack.push(v);
        }
        0x66 => {
            let (is_bracket, prop) = resolve_prop_name(ctx, insn.arg1, stack);
            let obj = pop_val(stack);
            let access = format_receiver_member(&obj, &prop, is_bracket);
            let mut v = AbcVal::expr(access);
            if obj.code == "this" && !is_bracket {
                v.method_ref = Some(prop);
            }
            stack.push(v);
        }
        0x6A => {
            let (is_bracket, prop) = resolve_prop_name(ctx, insn.arg1, stack);
            let obj = pop_val(stack);
            let access = format_receiver_member(&obj, &prop, is_bracket);
            stack.push(AbcVal::expr(format!("delete {access}")));
        }
        0x6C => {
            let _obj = pop_val(stack);
            let sname = ctx
                .activation_slots
                .get(&insn.arg1)
                .cloned()
                .unwrap_or_else(|| format!("_slot{}", insn.arg1));
            stack.push(AbcVal::expr(sname));
        }
        0x6D => {
            let val = pop_val(stack);
            let _obj = pop_val(stack);
            if val.code == "undefined" {
                return;
            }
            let sname = ctx
                .activation_slots
                .get(&insn.arg1)
                .cloned()
                .unwrap_or_else(|| format!("_slot{}", insn.arg1));
            if ctx.activation_slots.contains_key(&insn.arg1)
                && ctx.declared_locals.insert(10_000 + insn.arg1)
            {
                lines.push(format!("var {sname}:* = {};", val.code));
            } else {
                lines.push(format!("{sname} = {};", val.code));
            }
        }
        0x70 | 0x85 => {
            // convert_s / coerce_s: keep clean string expression unless non-obvious
        }
        0x73 | 0x74 | 0x75 | 0x76 | 0x77 | 0x82 | 0x89 => {
            // implicit numeric/boolean/any coercion produced by ASC around operators/locals
        }
        0x80 => {
            let ty = ctx.abc.short_type_name(insn.arg1, ctx.imports);
            let v = pop_val(stack);
            if ty == "*" || ty == "Object" || v.code == "null" {
                stack.push(v);
            } else {
                stack.push(AbcVal::expr(v.code));
            }
        }
        0x86 => {
            let ty = ctx.abc.short_type_name(insn.arg1, ctx.imports);
            let v = pop_val(stack);
            stack.push(AbcVal::expr(format!("({} as {ty})", v.code)));
        }
        0x87 => {
            let ty = pop_val(stack);
            let v = pop_val(stack);
            stack.push(AbcVal::expr(format!("({} as {})", v.code, ty.code)));
        }
        0x90 | 0xC4 => {
            let a = pop_val(stack);
            stack.push(AbcVal::expr(format!("-({})", a.code)));
        }
        0x91 | 0xC0 => {
            let a = pop_val(stack);
            stack.push(AbcVal::expr(format!("({} + 1)", a.code)));
        }
        0x92 | 0xC2 => {
            let r = ctx.local_name(insn.arg1);
            lines.push(format!("{r}++;"));
        }
        0x93 | 0xC1 => {
            let a = pop_val(stack);
            stack.push(AbcVal::expr(format!("({} - 1)", a.code)));
        }
        0x94 | 0xC3 => {
            let r = ctx.local_name(insn.arg1);
            lines.push(format!("{r}--;"));
        }
        0x95 => {
            let a = pop_val(stack);
            stack.push(AbcVal::expr(format!("typeof({})", a.code)));
        }
        0x96 => {
            let a = pop_val(stack);
            stack.push(AbcVal::expr(invert_as3_cond(&a.code)));
        }
        0x97 => {
            let a = pop_val(stack);
            stack.push(AbcVal::expr(format!("~({})", a.code)));
        }
        0xA0 | 0xC5 => abc_binop(stack, "+"),
        0xA1 | 0xC6 => abc_binop(stack, "-"),
        0xA2 | 0xC7 => abc_binop(stack, "*"),
        0xA3 => abc_binop(stack, "/"),
        0xA4 => abc_binop(stack, "%"),
        0xA5 => abc_binop(stack, "<<"),
        0xA6 => abc_binop(stack, ">>"),
        0xA7 => abc_binop(stack, ">>>"),
        0xA8 => abc_binop(stack, "&"),
        0xA9 => abc_binop(stack, "|"),
        0xAA => abc_binop(stack, "^"),
        0xAB => abc_binop(stack, "=="),
        0xAC => abc_binop(stack, "==="),
        0xAD => abc_binop(stack, "<"),
        0xAE => abc_binop(stack, "<="),
        0xAF => abc_binop(stack, ">"),
        0xB0 => abc_binop(stack, ">="),
        0xB1 => abc_binop(stack, "instanceof"),
        0xB2 => {
            let ty = ctx.abc.short_type_name(insn.arg1, ctx.imports);
            let v = pop_val(stack);
            stack.push(AbcVal::expr(format!("({} is {ty})", v.code)));
        }
        0xB3 => {
            let ty = pop_val(stack);
            let v = pop_val(stack);
            stack.push(AbcVal::expr(format!("({} is {})", v.code, ty.code)));
        }
        0xB4 => abc_binop(stack, "in"),
        0xD0..=0xD3 => {
            let reg = usize::from(insn.op - 0xD0);
            let name = ctx.local_name(reg);
            let mut v = AbcVal::expr(name);
            if reg == 0 {
                v.is_this_or_find = true;
            }
            stack.push(v);
        }
        0xD4..=0xD7 => {
            let reg = usize::from(insn.op - 0xD4);
            store_local(ctx, reg, stack, lines);
        }
        _ => {}
    }
}

fn store_local(
    ctx: &mut BodyContext<'_>,
    reg: usize,
    stack: &mut Vec<AbcVal>,
    lines: &mut Vec<String>,
) {
    let val = pop_val(stack);
    if reg == 0 {
        return;
    }
    let name = ctx.local_name(reg);
    if reg > ctx.param_count && ctx.declared_locals.insert(reg) {
        lines.push(format!("var {name}:* = {};", val.code));
    } else if name != val.code {
        lines.push(format!("{name} = {};", val.code));
    }
}

fn abc_binop(stack: &mut Vec<AbcVal>, op: &str) {
    let b = pop_val(stack);
    let a = pop_val(stack);
    stack.push(AbcVal::expr(format!("({} {op} {})", a.code, b.code)));
}

fn resolve_prop_name(
    ctx: &mut BodyContext<'_>,
    mn_idx: usize,
    stack: &mut Vec<AbcVal>,
) -> (bool, String) {
    let (has_ns, has_name) = ctx.abc.runtime_multiname_parts(mn_idx);
    let rt_name = if has_name {
        Some(pop_val(stack).code)
    } else {
        None
    };
    if has_ns {
        let _ = pop_val(stack);
    }
    if let Some(rn) = rt_name {
        return (true, rn);
    }
    let short = ctx.abc.short_type_name(mn_idx, ctx.imports);
    (false, short)
}

fn format_receiver_member(recv: &AbcVal, prop: &str, is_bracket: bool) -> String {
    if is_bracket {
        return format!("{}[{prop}]", recv.code);
    }
    if recv.is_this_or_find && recv.code == "this" {
        // Distinguish explicit `this.foo` (`getlocal_0`) vs lexical `foo` (`findpropstrict` / `getglobalscope`):
        // Using `prop` for lexical builtins like `trace`, `stop`, `MovieClip` and `this.prop` when unambiguous:
        if matches!(
            prop,
            "trace"
                | "stop"
                | "play"
                | "gotoAndStop"
                | "gotoAndPlay"
                | "nextFrame"
                | "prevFrame"
                | "int"
                | "uint"
                | "Number"
                | "String"
                | "Boolean"
                | "Array"
                | "Object"
                | "getTimer"
        ) || prop.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        {
            return prop.to_owned();
        }
        return format!("this.{prop}");
    }
    format!("{}.{prop}", recv.code)
}

fn record_add_frame_script_args(args: &[AbcVal], out: &mut Vec<(u32, String)>) {
    let mut pairs = BTreeMap::new();
    for pair in args.as_chunks::<2>().0 {
        if let Some(frame_idx) = pair[0].int_lit
            && (0..=100_000).contains(&frame_idx)
        {
            let fn_name = pair[1]
                .method_ref
                .clone()
                .or_else(|| pair[1].code.strip_prefix("this.").map(ToOwned::to_owned));
            if let Some(name) = fn_name
                && name != "null"
            {
                pairs.insert(frame_idx as u32, name);
            }
        }
    }
    for (f, name) in pairs {
        out.push((f, name));
    }
}
