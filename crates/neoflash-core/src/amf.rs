//! Bounded AMF value codec, independently implemented from Adobe's AMF0 (June 2006)
//! and AMF3 (January 2013) specifications. No packet envelope or class loading.
//! Graph references are separate from AMF0/AMF3 wire reference tables.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const MAX_AMF_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_AMF_JSON_BYTES: usize = 64 * 1024 * 1024;
const MAX_ITEMS: usize = 100_000;
const MAX_DEPTH: usize = 128;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AmfValue {
    Undefined,
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Ref(usize),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AmfNode {
    Object {
        entries: Vec<(String, AmfValue)>,
    },
    Array {
        dense: Vec<AmfValue>,
        entries: Vec<(String, AmfValue)>,
    },
    Date {
        milliseconds: String,
    },
    Bytes {
        base64: String,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmfGraph {
    pub root: AmfValue,
    pub nodes: Vec<AmfNode>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmfDecoded {
    pub graph: AmfGraph,
    pub consumed: usize,
}
#[derive(Clone, Debug, thiserror::Error, PartialEq)]
#[error("AMF at byte {offset}: {message}")]
pub struct AmfError {
    pub offset: usize,
    pub message: String,
}
fn fail(message: &str) -> AmfError {
    AmfError {
        offset: 0,
        message: message.into(),
    }
}
fn number(s: &str) -> Result<f64, AmfError> {
    match s {
        "NaN" => Ok(f64::NAN),
        "Infinity" => Ok(f64::INFINITY),
        "-Infinity" => Ok(f64::NEG_INFINITY),
        _ => s
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && !s.is_empty() && s.trim() == s)
            .ok_or_else(|| fail("invalid tagged number")),
    }
}
fn number_text(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v == f64::INFINITY {
        "Infinity".into()
    } else if v == f64::NEG_INFINITY {
        "-Infinity".into()
    } else if v == 0. && v.is_sign_negative() {
        "-0".into()
    } else {
        v.to_string()
    }
}
fn index(name: &str) -> Option<usize> {
    let n = name.parse::<usize>().ok()?;
    (n.to_string() == name && n < u32::MAX as usize).then_some(n)
}
#[derive(Default)]
struct Budget {
    items: usize,
    bytes: usize,
}
impl Budget {
    fn items(&mut self, n: usize) -> Result<(), AmfError> {
        self.items = self.items.saturating_add(n);
        if self.items > MAX_ITEMS {
            Err(fail("graph exceeds 100000 values/nodes/properties"))
        } else {
            Ok(())
        }
    }
    fn bytes(&mut self, n: usize) -> Result<(), AmfError> {
        self.bytes = self.bytes.saturating_add(n);
        if self.bytes > MAX_AMF_BYTES {
            Err(fail("graph strings/bytes exceed 16 MiB"))
        } else {
            Ok(())
        }
    }
}
impl AmfGraph {
    fn validate(&self) -> Result<(), AmfError> {
        let mut b = Budget::default();
        b.items(self.nodes.len())?;
        let val = |v: &AmfValue, b: &mut Budget| -> Result<(), AmfError> {
            b.items(1)?;
            match v {
                AmfValue::Number(s) => {
                    number(s)?;
                }
                AmfValue::String(s) => b.bytes(s.len())?,
                AmfValue::Ref(i) if *i >= self.nodes.len() => {
                    return Err(fail("graph reference is out of bounds"));
                }
                _ => {}
            }
            Ok(())
        };
        val(&self.root, &mut b)?;
        for node in &self.nodes {
            let entries = match node {
                AmfNode::Object { entries } => entries,
                AmfNode::Array { dense, entries } => {
                    if dense.len() > MAX_ITEMS {
                        return Err(fail("array length exceeds budget"));
                    }
                    for v in dense {
                        val(v, &mut b)?;
                    }
                    for (key, _) in entries {
                        if key == "length"
                            || index(key).is_some_and(|i| i < dense.len() || i >= MAX_ITEMS)
                        {
                            return Err(fail(
                                "array associative index conflicts with dense data or exceeds budget",
                            ));
                        }
                    }
                    entries
                }
                AmfNode::Date { milliseconds } => {
                    number(milliseconds)?;
                    continue;
                }
                AmfNode::Bytes { base64 } => {
                    if base64.len() > MAX_AMF_BYTES.div_ceil(3) * 4 {
                        return Err(fail("ByteArray exceeds budget"));
                    }
                    let bytes = STANDARD
                        .decode(base64)
                        .map_err(|_| fail("invalid canonical base64"))?;
                    if STANDARD.encode(&bytes) != *base64 {
                        return Err(fail("invalid canonical base64"));
                    }
                    b.bytes(bytes.len())?;
                    continue;
                }
            };
            let mut names = HashSet::new();
            for (name, v) in entries {
                b.bytes(name.len())?;
                b.items(1)?;
                if !names.insert(name) {
                    return Err(fail("duplicate property name"));
                }
                val(v, &mut b)?;
            }
        }
        Ok(())
    }
}
pub fn amf_encode(format: u8, graph: &AmfGraph) -> Result<Vec<u8>, AmfError> {
    if !matches!(format, 0 | 3) {
        return Err(fail("objectEncoding must be 0 or 3"));
    }
    graph.validate()?;
    let mut w = Writer {
        out: vec![],
        graph,
        refs0: HashMap::new(),
        refs3: HashMap::new(),
        strings: HashMap::new(),
        trait_written: false,
    };
    w.value(&graph.root, format, 0)?;
    Ok(w.out)
}
struct Writer<'a> {
    out: Vec<u8>,
    graph: &'a AmfGraph,
    refs0: HashMap<usize, usize>,
    refs3: HashMap<usize, usize>,
    strings: HashMap<String, usize>,
    trait_written: bool,
}
impl Writer<'_> {
    fn put(&mut self, v: &[u8]) -> Result<(), AmfError> {
        if self.out.len().saturating_add(v.len()) > MAX_AMF_BYTES {
            return Err(fail("encoded value exceeds 16 MiB"));
        }
        self.out.extend_from_slice(v);
        Ok(())
    }
    fn byte(&mut self, v: u8) -> Result<(), AmfError> {
        self.put(&[v])
    }
    fn u29(&mut self, n: u32) -> Result<(), AmfError> {
        if n < 0x80 {
            self.byte(n as u8)
        } else if n < 0x4000 {
            self.put(&[((n >> 7) as u8) | 0x80, (n & 0x7f) as u8])
        } else if n < 0x20_0000 {
            self.put(&[
                ((n >> 14) as u8) | 0x80,
                ((n >> 7) as u8) | 0x80,
                (n & 0x7f) as u8,
            ])
        } else {
            self.put(&[
                ((n >> 22) as u8) | 0x80,
                ((n >> 15) as u8) | 0x80,
                ((n >> 8) as u8) | 0x80,
                n as u8,
            ])
        }
    }
    fn string3(&mut self, s: &str) -> Result<(), AmfError> {
        if s.is_empty() {
            return self.byte(1);
        }
        if let Some(i) = self.strings.get(s) {
            return self.u29((*i as u32) << 1);
        }
        self.u29(((s.len() as u32) << 1) | 1)?;
        self.put(s.as_bytes())?;
        self.strings.insert(s.into(), self.strings.len());
        Ok(())
    }
    fn name0(&mut self, s: &str) -> Result<(), AmfError> {
        let n = u16::try_from(s.len())
            .map_err(|_| fail("AMF0 property names are limited to 65535 UTF-8 bytes"))?;
        self.put(&n.to_be_bytes())?;
        self.put(s.as_bytes())
    }
    fn value(&mut self, v: &AmfValue, format: u8, depth: usize) -> Result<(), AmfError> {
        if depth > MAX_DEPTH {
            return Err(fail("value nesting exceeds 128"));
        }
        match v {
            AmfValue::Undefined => self.byte(if format == 0 { 6 } else { 0 }),
            AmfValue::Null => self.byte(if format == 0 { 5 } else { 1 }),
            AmfValue::Bool(v) => {
                if format == 0 {
                    self.put(&[1, u8::from(*v)])
                } else {
                    self.byte(if *v { 3 } else { 2 })
                }
            }
            AmfValue::Number(s) => {
                let n = number(s)?;
                if format == 3 && n.fract() == 0. && !n.is_sign_negative() && n <= 268_435_455.
                    || format == 3 && n.fract() == 0. && (-268_435_456. ..0.).contains(&n)
                {
                    self.byte(4)?;
                    self.u29((n as i32 as u32) & 0x1fff_ffff)
                } else {
                    self.byte(if format == 0 { 0 } else { 5 })?;
                    self.put(&n.to_be_bytes())
                }
            }
            AmfValue::String(s) => {
                if format == 3 {
                    self.byte(6)?;
                    self.string3(s)
                } else {
                    if s.len() <= u16::MAX as usize {
                        self.byte(2)?;
                        self.name0(s)
                    } else {
                        self.byte(12)?;
                        self.put(&(s.len() as u32).to_be_bytes())?;
                        self.put(s.as_bytes())
                    }
                }
            }
            AmfValue::Ref(id) => self.node(*id, format, depth),
        }
    }
    fn node(&mut self, id: usize, format: u8, depth: usize) -> Result<(), AmfError> {
        let node = &self.graph.nodes[id];
        if format == 0 {
            if matches!(node, AmfNode::Bytes { .. }) {
                self.byte(17)?;
                return self.node(id, 3, depth);
            }
            if let AmfNode::Date { milliseconds } = node {
                self.byte(11)?;
                self.put(&number(milliseconds)?.to_be_bytes())?;
                return self.put(&[0, 0]);
            }
            if let Some(i) = self.refs0.get(&id) {
                let i = u16::try_from(*i)
                    .map_err(|_| fail("AMF0 reference table exceeds 65536 objects"))?;
                self.byte(7)?;
                return self.put(&i.to_be_bytes());
            }
            if self.refs0.len() > u16::MAX as usize {
                return Err(fail("AMF0 reference table exceeds 65536 objects"));
            }
            self.refs0.insert(id, self.refs0.len());
            match node {
                AmfNode::Object { entries } => {
                    self.byte(3)?;
                    self.entries0(entries, depth)?;
                    self.put(&[0, 0, 9])
                }
                AmfNode::Array { dense, entries } => {
                    self.byte(if entries.is_empty() { 10 } else { 8 })?;
                    self.put(&((dense.len() + entries.len()) as u32).to_be_bytes())?;
                    for (i, v) in dense.iter().enumerate() {
                        if !entries.is_empty() {
                            self.name0(&i.to_string())?;
                        }
                        self.value(v, 0, depth + 1)?;
                    }
                    if !entries.is_empty() {
                        self.entries0(entries, depth)?;
                        self.put(&[0, 0, 9])?;
                    }
                    Ok(())
                }
                _ => unreachable!(),
            }
        } else {
            let marker = match node {
                AmfNode::Object { .. } => 10,
                AmfNode::Array { .. } => 9,
                AmfNode::Date { .. } => 8,
                AmfNode::Bytes { .. } => 12,
            };
            self.byte(marker)?;
            if let Some(i) = self.refs3.get(&id) {
                return self.u29((*i as u32) << 1);
            }
            self.refs3.insert(id, self.refs3.len());
            match node {
                AmfNode::Object { entries } => {
                    if self.trait_written {
                        self.u29(1)?;
                    } else {
                        self.u29(11)?;
                        self.string3("")?;
                        self.trait_written = true;
                    }
                    self.entries3(entries, depth)?;
                    self.string3("")
                }
                AmfNode::Array { dense, entries } => {
                    self.u29(((dense.len() as u32) << 1) | 1)?;
                    self.entries3(entries, depth)?;
                    self.string3("")?;
                    for v in dense {
                        self.value(v, 3, depth + 1)?;
                    }
                    Ok(())
                }
                AmfNode::Date { milliseconds } => {
                    self.byte(1)?;
                    self.put(&number(milliseconds)?.to_be_bytes())
                }
                AmfNode::Bytes { base64 } => {
                    let bytes = STANDARD
                        .decode(base64)
                        .map_err(|_| fail("invalid base64"))?;
                    self.u29(((bytes.len() as u32) << 1) | 1)?;
                    self.put(&bytes)
                }
            }
        }
    }
    fn entries0(&mut self, entries: &[(String, AmfValue)], depth: usize) -> Result<(), AmfError> {
        for (name, v) in entries {
            self.name0(name)?;
            self.value(v, 0, depth + 1)?;
        }
        Ok(())
    }
    fn entries3(&mut self, entries: &[(String, AmfValue)], depth: usize) -> Result<(), AmfError> {
        for (name, v) in entries {
            if name.is_empty() {
                return Err(fail(
                    "empty AMF3 dynamic member names are not representable",
                ));
            }
            self.string3(name)?;
            self.value(v, 3, depth + 1)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
struct Trait {
    dynamic: bool,
    names: Vec<String>,
}
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    nodes: Vec<AmfNode>,
    refs0: Vec<usize>,
    refs3: Vec<usize>,
    strings: Vec<String>,
    traits: Vec<Trait>,
    budget: Budget,
}
pub fn amf_decode(format: u8, bytes: &[u8]) -> Result<AmfDecoded, AmfError> {
    if !matches!(format, 0 | 3) {
        return Err(fail("objectEncoding must be 0 or 3"));
    }
    let mut r = Reader {
        data: bytes,
        pos: 0,
        nodes: vec![],
        refs0: vec![],
        refs3: vec![],
        strings: vec![],
        traits: vec![],
        budget: Budget::default(),
    };
    let root = r.value(format, 0).map_err(|mut error| {
        error.offset = r.pos;
        error
    })?;
    Ok(AmfDecoded {
        graph: AmfGraph {
            root,
            nodes: r.nodes,
        },
        consumed: r.pos,
    })
}
impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], AmfError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| fail("byte length overflow"))?;
        if end > MAX_AMF_BYTES {
            return Err(fail("encoded value exceeds 16 MiB"));
        }
        let result = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| fail("truncated AMF value"))?;
        self.pos = end;
        Ok(result)
    }
    fn byte(&mut self) -> Result<u8, AmfError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, AmfError> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, AmfError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn double(&mut self) -> Result<String, AmfError> {
        Ok(number_text(f64::from_be_bytes(
            self.take(8)?.try_into().unwrap(),
        )))
    }
    fn u29(&mut self) -> Result<u32, AmfError> {
        let mut n = 0;
        for _ in 0..3 {
            let b = self.byte()?;
            n = (n << 7) | u32::from(b & 127);
            if b < 128 {
                return Ok(n);
            }
        }
        Ok((n << 8) | u32::from(self.byte()?))
    }
    fn text(&mut self, n: usize) -> Result<String, AmfError> {
        self.budget.bytes(n)?;
        let s = std::str::from_utf8(self.take(n)?).map_err(|_| fail("invalid UTF-8"))?;
        Ok(s.into())
    }
    fn string3(&mut self) -> Result<String, AmfError> {
        let h = self.u29()?;
        if h & 1 == 0 {
            let s = self
                .strings
                .get((h >> 1) as usize)
                .ok_or_else(|| fail("invalid AMF3 string reference"))?;
            self.budget.bytes(s.len())?;
            Ok(s.clone())
        } else {
            let s = self.text((h >> 1) as usize)?;
            if !s.is_empty() {
                self.budget.items(1)?;
                self.strings.push(s.clone());
            }
            Ok(s)
        }
    }
    fn node(&mut self, format: u8) -> Result<usize, AmfError> {
        self.budget.items(1)?;
        let id = self.nodes.len();
        self.nodes.push(AmfNode::Object { entries: vec![] });
        if format == 0 {
            self.refs0.push(id);
        } else {
            self.refs3.push(id);
        }
        Ok(id)
    }
    fn count(&self, n: usize) -> Result<(), AmfError> {
        if n > MAX_ITEMS.saturating_sub(self.budget.items) {
            Err(fail("declared collection count exceeds budget"))
        } else {
            Ok(())
        }
    }
    fn value(&mut self, format: u8, depth: usize) -> Result<AmfValue, AmfError> {
        if depth > MAX_DEPTH {
            return Err(fail("value nesting exceeds 128"));
        }
        self.budget.items(1)?;
        let marker = self.byte()?;
        if format == 0 {
            match marker {
                0 => Ok(AmfValue::Number(self.double()?)),
                1 => Ok(AmfValue::Bool(self.byte()? != 0)),
                2 => {
                    let n = self.u16()? as usize;
                    Ok(AmfValue::String(self.text(n)?))
                }
                5 => Ok(AmfValue::Null),
                6 => Ok(AmfValue::Undefined),
                7 => {
                    let i = self.u16()? as usize;
                    Ok(AmfValue::Ref(
                        *self
                            .refs0
                            .get(i)
                            .ok_or_else(|| fail("invalid AMF0 object reference"))?,
                    ))
                }
                3 | 8 | 10 => {
                    let id = self.node(0)?;
                    let node = if marker == 10 {
                        let count = self.u32()? as usize;
                        self.count(count)?;
                        let mut dense = Vec::with_capacity(count);
                        for _ in 0..count {
                            dense.push(self.value(0, depth + 1)?);
                        }
                        AmfNode::Array {
                            dense,
                            entries: vec![],
                        }
                    } else {
                        if marker == 8 {
                            self.u32()?;
                        }
                        let entries = self.entries0(depth)?;
                        if marker == 8 {
                            self.array(vec![], entries)?
                        } else {
                            AmfNode::Object { entries }
                        }
                    };
                    self.nodes[id] = node;
                    Ok(AmfValue::Ref(id))
                }
                11 => {
                    let milliseconds = self.double()?;
                    self.take(2)?;
                    self.budget.items(1)?;
                    let id = self.nodes.len();
                    self.nodes.push(AmfNode::Date { milliseconds });
                    Ok(AmfValue::Ref(id))
                }
                12 => {
                    let n = self.u32()? as usize;
                    Ok(AmfValue::String(self.text(n)?))
                }
                17 => self.value(3, depth + 1),
                _ => Err(fail(
                    "unsupported AMF0 marker (typed objects/XML/legacy extensions are pending)",
                )),
            }
        } else {
            match marker {
                0 => Ok(AmfValue::Undefined),
                1 => Ok(AmfValue::Null),
                2 => Ok(AmfValue::Bool(false)),
                3 => Ok(AmfValue::Bool(true)),
                4 => {
                    let n = self.u29()?;
                    let signed = if n & 0x1000_0000 != 0 {
                        (n | 0xe000_0000) as i32
                    } else {
                        n as i32
                    };
                    Ok(AmfValue::Number(signed.to_string()))
                }
                5 => Ok(AmfValue::Number(self.double()?)),
                6 => Ok(AmfValue::String(self.string3()?)),
                8 | 9 | 10 | 12 => self.complex3(marker, depth),
                _ => Err(fail(
                    "unsupported AMF3 marker (XML/Vector/Dictionary are pending)",
                )),
            }
        }
    }
    fn entries0(&mut self, depth: usize) -> Result<Vec<(String, AmfValue)>, AmfError> {
        let mut result = vec![];
        let mut names = HashSet::new();
        loop {
            let n = self.u16()? as usize;
            if n == 0 && self.data.get(self.pos) == Some(&9) {
                self.byte()?;
                break;
            }
            self.budget.items(1)?;
            let name = self.text(n)?;
            if !names.insert(name.clone()) {
                return Err(fail("duplicate member name"));
            }
            let v = self.value(0, depth + 1)?;
            result.push((name, v));
        }
        Ok(result)
    }
    fn entries3(
        &mut self,
        depth: usize,
        names: &mut HashSet<String>,
    ) -> Result<Vec<(String, AmfValue)>, AmfError> {
        let mut result = vec![];
        loop {
            let name = self.string3()?;
            if name.is_empty() {
                break;
            }
            self.budget.items(1)?;
            if !names.insert(name.clone()) {
                return Err(fail("duplicate member name"));
            }
            let v = self.value(3, depth + 1)?;
            result.push((name, v));
        }
        Ok(result)
    }
    fn array(
        &mut self,
        mut dense: Vec<AmfValue>,
        entries: Vec<(String, AmfValue)>,
    ) -> Result<AmfNode, AmfError> {
        let original = dense.len();
        let mut named = vec![];
        for (name, v) in entries {
            if name == "length" {
                return Err(fail("associative length member is not supported"));
            }
            if let Some(i) = index(&name) {
                if i < original || i >= MAX_ITEMS {
                    return Err(fail(
                        "associative index conflicts with dense data or exceeds budget",
                    ));
                }
                if i >= dense.len() {
                    let added = i + 1 - dense.len();
                    self.budget.items(added)?;
                    dense.resize(i + 1, AmfValue::Undefined);
                }
                dense[i] = v;
            } else {
                named.push((name, v));
            }
        }
        Ok(AmfNode::Array {
            dense,
            entries: named,
        })
    }
    fn complex3(&mut self, marker: u8, depth: usize) -> Result<AmfValue, AmfError> {
        let h = self.u29()?;
        if h & 1 == 0 {
            let id = *self
                .refs3
                .get((h >> 1) as usize)
                .ok_or_else(|| fail("invalid AMF3 object reference"))?;
            let valid = matches!(
                (&self.nodes[id], marker),
                (AmfNode::Date { .. }, 8)
                    | (AmfNode::Array { .. }, 9)
                    | (AmfNode::Object { .. }, 10)
                    | (AmfNode::Bytes { .. }, 12)
            );
            if !valid {
                return Err(fail("AMF3 reference marker/type mismatch"));
            }
            return Ok(AmfValue::Ref(id));
        }
        let id = self.node(3)?;
        let node = match marker {
            8 => AmfNode::Date {
                milliseconds: self.double()?,
            },
            12 => {
                let n = (h >> 1) as usize;
                self.budget.bytes(n)?;
                AmfNode::Bytes {
                    base64: STANDARD.encode(self.take(n)?),
                }
            }
            9 => {
                let n = (h >> 1) as usize;
                self.count(n)?;
                self.nodes[id] = AmfNode::Array {
                    dense: vec![],
                    entries: vec![],
                };
                let entries = self.entries3(depth, &mut HashSet::new())?;
                self.count(n)?;
                let mut dense = Vec::with_capacity(n);
                for _ in 0..n {
                    dense.push(self.value(3, depth + 1)?);
                }
                self.array(dense, entries)?
            }
            10 => {
                let traits = if h & 2 == 0 {
                    self.traits
                        .get((h >> 2) as usize)
                        .ok_or_else(|| fail("invalid AMF3 trait reference"))?
                        .clone()
                } else {
                    if h & 4 != 0 {
                        return Err(fail("externalizable objects are not supported"));
                    }
                    let count = (h >> 4) as usize;
                    self.count(count)?;
                    let class = self.string3()?;
                    if !class.is_empty() {
                        return Err(fail("registered class aliases are not supported"));
                    }
                    let mut names = Vec::with_capacity(count);
                    let mut seen = HashSet::new();
                    for _ in 0..count {
                        self.budget.items(1)?;
                        let name = self.string3()?;
                        if !seen.insert(name.clone()) {
                            return Err(fail("duplicate sealed member name"));
                        }
                        names.push(name);
                    }
                    let t = Trait {
                        dynamic: h & 8 != 0,
                        names,
                    };
                    self.traits.push(t.clone());
                    t
                };
                let mut entries = vec![];
                let mut seen = HashSet::new();
                for name in traits.names {
                    self.budget.items(1)?;
                    self.budget.bytes(name.len())?;
                    seen.insert(name.clone());
                    entries.push((name, self.value(3, depth + 1)?));
                }
                if traits.dynamic {
                    entries.extend(self.entries3(depth, &mut seen)?);
                }
                AmfNode::Object { entries }
            }
            _ => unreachable!(),
        };
        self.nodes[id] = node;
        Ok(AmfValue::Ref(id))
    }
}
