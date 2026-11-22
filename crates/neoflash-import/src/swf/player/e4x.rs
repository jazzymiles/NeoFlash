use super::*;
use std::cell::RefCell;
use std::rc::Rc;

const MAX_E4X_NODES: usize = 16_384;

#[derive(Clone, Debug)]
pub(super) struct E4xSettings {
    pub(super) ignore_comments: bool,
    pub(super) ignore_processing_instructions: bool,
    pub(super) ignore_whitespace: bool,
    pub(super) pretty_printing: bool,
    pub(super) pretty_indent: usize,
}

impl Default for E4xSettings {
    fn default() -> Self {
        Self {
            ignore_comments: true,
            ignore_processing_instructions: true,
            ignore_whitespace: true,
            pretty_printing: true,
            pretty_indent: 2,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum E4xNodeKind {
    Element {
        prefix: String,
        uri: String,
        local_name: String,
        attrs: Vec<usize>,
        children: Vec<usize>,
        in_scope_ns: Vec<(String, String)>,
    },
    Attribute {
        prefix: String,
        uri: String,
        local_name: String,
        value: String,
    },
    Text(String),
    Comment(String),
    ProcessingInstruction {
        target: String,
        content: String,
    },
}

#[derive(Clone, Debug)]
pub(super) struct E4xNode {
    pub(super) kind: E4xNodeKind,
    pub(super) parent: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct E4xArena {
    pub(super) nodes: Vec<E4xNode>,
    pub(super) default_ns: String,
    pub(super) settings: E4xSettings,
}

#[derive(Clone, Debug)]
pub(super) enum Avm2XmlKind {
    Xml(usize),
    XmlList {
        items: Vec<usize>,
        target_node: Option<usize>,
        target_prop: Option<String>,
    },
}

#[derive(Clone, Debug)]
pub(super) struct Avm2XmlData {
    pub(super) arena: Rc<RefCell<E4xArena>>,
    pub(super) kind: Avm2XmlKind,
}

impl Avm2XmlData {
    pub(super) fn format_string(&self) -> String {
        let arena = self.arena.borrow();
        match &self.kind {
            Avm2XmlKind::Xml(nid) => arena.node_to_string(*nid),
            Avm2XmlKind::XmlList { items, .. } => arena.list_to_string(items),
        }
    }
}

pub(super) fn esc_xml_elem(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
    out
}

pub(super) fn esc_xml_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            '\x0A' => out.push_str("&#xA;"),
            '\x0D' => out.push_str("&#xD;"),
            '\x09' => out.push_str("&#x9;"),
            other => out.push(other),
        }
    }
    out
}

fn unescape_xml_entities(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after_amp = &rest[amp + 1..];
        if let Some(semi) = after_amp.find(';') {
            let ent = &after_amp[..semi];
            let decoded = match ent {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                    u32::from_str_radix(&ent[2..], 16)
                        .ok()
                        .and_then(char::from_u32)
                }
                _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            };
            if let Some(ch) = decoded {
                out.push(ch);
                rest = &after_amp[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = after_amp;
    }
    out.push_str(rest);
    out
}

fn is_xml_whitespace(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
}

fn split_xml_qname(raw: &str) -> (&str, &str) {
    match raw.split_once(':') {
        Some((p, l)) => (p, l),
        None => ("", raw),
    }
}

impl E4xArena {
    pub(super) fn alloc_node(&mut self, kind: E4xNodeKind, parent: Option<usize>) -> usize {
        if self.nodes.len() >= MAX_E4X_NODES {
            return self.nodes.len().saturating_sub(1);
        }
        let id = self.nodes.len();
        self.nodes.push(E4xNode { kind, parent });
        id
    }

    pub(super) fn deep_copy(&mut self, nid: usize, new_parent: Option<usize>) -> usize {
        let Some(node) = self.nodes.get(nid).cloned() else {
            return self.alloc_node(E4xNodeKind::Text(String::new()), new_parent);
        };
        match node.kind {
            E4xNodeKind::Element {
                prefix,
                uri,
                local_name,
                attrs,
                children,
                in_scope_ns,
            } => {
                let copy_id = self.alloc_node(
                    E4xNodeKind::Element {
                        prefix,
                        uri,
                        local_name,
                        attrs: Vec::new(),
                        children: Vec::new(),
                        in_scope_ns,
                    },
                    new_parent,
                );
                let mut copied_attrs = Vec::with_capacity(attrs.len());
                for a in attrs {
                    copied_attrs.push(self.deep_copy(a, Some(copy_id)));
                }
                let mut copied_children = Vec::with_capacity(children.len());
                for c in children {
                    copied_children.push(self.deep_copy(c, Some(copy_id)));
                }
                if let Some(E4xNode {
                    kind:
                        E4xNodeKind::Element {
                            attrs: dst_a,
                            children: dst_c,
                            ..
                        },
                    ..
                }) = self.nodes.get_mut(copy_id)
                {
                    *dst_a = copied_attrs;
                    *dst_c = copied_children;
                }
                copy_id
            }
            other => self.alloc_node(other, new_parent),
        }
    }

    pub(super) fn node_has_simple_content(&self, nid: usize) -> bool {
        let Some(node) = self.nodes.get(nid) else {
            return true;
        };
        match &node.kind {
            E4xNodeKind::Comment(_) | E4xNodeKind::ProcessingInstruction { .. } => false,
            E4xNodeKind::Text(_) | E4xNodeKind::Attribute { .. } => true,
            E4xNodeKind::Element { children, .. } => children.iter().all(|&cid| {
                !matches!(
                    self.nodes.get(cid).map(|n| &n.kind),
                    Some(E4xNodeKind::Element { .. })
                )
            }),
        }
    }

    pub(super) fn node_has_complex_content(&self, nid: usize) -> bool {
        let Some(node) = self.nodes.get(nid) else {
            return false;
        };
        match &node.kind {
            E4xNodeKind::Element { children, .. } => children.iter().any(|&cid| {
                matches!(
                    self.nodes.get(cid).map(|n| &n.kind),
                    Some(E4xNodeKind::Element { .. })
                )
            }),
            _ => false,
        }
    }

    pub(super) fn list_has_simple_content(&self, items: &[usize]) -> bool {
        match items.len() {
            0 => true,
            1 => self.node_has_simple_content(items[0]),
            _ => items.iter().all(|&id| {
                !matches!(
                    self.nodes.get(id).map(|n| &n.kind),
                    Some(E4xNodeKind::Element { .. })
                )
            }),
        }
    }

    pub(super) fn list_has_complex_content(&self, items: &[usize]) -> bool {
        match items.len() {
            0 => false,
            1 => self.node_has_complex_content(items[0]),
            _ => items.iter().any(|&id| {
                matches!(
                    self.nodes.get(id).map(|n| &n.kind),
                    Some(E4xNodeKind::Element { .. })
                )
            }),
        }
    }

    pub(super) fn node_to_string(&self, nid: usize) -> String {
        let Some(node) = self.nodes.get(nid) else {
            return String::new();
        };
        match &node.kind {
            E4xNodeKind::Text(s) => s.clone(),
            E4xNodeKind::Attribute { value, .. } => value.clone(),
            E4xNodeKind::Comment(_) | E4xNodeKind::ProcessingInstruction { .. } => {
                self.node_to_xml_string(nid, 0)
            }
            E4xNodeKind::Element { children, .. } => {
                if self.node_has_simple_content(nid) {
                    let mut out = String::new();
                    for &cid in children {
                        if let Some(E4xNode {
                            kind: E4xNodeKind::Text(t),
                            ..
                        }) = self.nodes.get(cid)
                        {
                            out.push_str(t);
                        }
                    }
                    out
                } else {
                    self.node_to_xml_string(nid, 0)
                }
            }
        }
    }

    pub(super) fn list_to_string(&self, items: &[usize]) -> String {
        if items.is_empty() {
            return String::new();
        }
        if self.list_has_simple_content(items) {
            let mut out = String::new();
            for &id in items {
                if !matches!(
                    self.nodes.get(id).map(|n| &n.kind),
                    Some(E4xNodeKind::Comment(_) | E4xNodeKind::ProcessingInstruction { .. })
                ) {
                    out.push_str(&self.node_to_string(id));
                }
            }
            out
        } else {
            self.list_to_xml_string(items)
        }
    }

    pub(super) fn list_to_xml_string(&self, items: &[usize]) -> String {
        items
            .iter()
            .map(|&id| self.node_to_xml_string(id, 0))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(super) fn node_to_xml_string(&self, nid: usize, indent_level: usize) -> String {
        if indent_level > 64 {
            return String::new();
        }
        let Some(node) = self.nodes.get(nid) else {
            return String::new();
        };
        let pad = if self.settings.pretty_printing && indent_level > 0 {
            " ".repeat(indent_level * self.settings.pretty_indent)
        } else {
            String::new()
        };
        match &node.kind {
            E4xNodeKind::Text(s) => {
                let text = if self.settings.pretty_printing {
                    s.trim()
                } else {
                    s.as_str()
                };
                format!("{pad}{}", esc_xml_elem(text))
            }
            E4xNodeKind::Attribute { value, .. } => esc_xml_attr(value),
            E4xNodeKind::Comment(c) => format!("{pad}<!--{c}-->"),
            E4xNodeKind::ProcessingInstruction { target, content } => {
                if content.is_empty() {
                    format!("{pad}<?{target}?>")
                } else {
                    format!("{pad}<?{target} {content}?>")
                }
            }
            E4xNodeKind::Element {
                prefix,
                local_name,
                attrs,
                children,
                in_scope_ns,
                ..
            } => {
                let tag_name = if prefix.is_empty() {
                    local_name.clone()
                } else {
                    format!("{prefix}:{local_name}")
                };
                let mut header = format!("{pad}<{tag_name}");
                for (ns_pfx, ns_uri) in in_scope_ns {
                    if ns_pfx.is_empty() {
                        if !ns_uri.is_empty() {
                            header.push_str(&format!(" xmlns=\"{}\"", esc_xml_attr(ns_uri)));
                        }
                    } else {
                        header.push_str(&format!(" xmlns:{ns_pfx}=\"{}\"", esc_xml_attr(ns_uri)));
                    }
                }
                for &aid in attrs {
                    if let Some(E4xNode {
                        kind:
                            E4xNodeKind::Attribute {
                                prefix: ap,
                                local_name: al,
                                value: av,
                                ..
                            },
                        ..
                    }) = self.nodes.get(aid)
                    {
                        let aname = if ap.is_empty() {
                            al.clone()
                        } else {
                            format!("{ap}:{al}")
                        };
                        header.push_str(&format!(" {aname}=\"{}\"", esc_xml_attr(av)));
                    }
                }
                if children.is_empty() {
                    header.push_str("/>");
                    return header;
                }
                if children.len() == 1
                    && let Some(E4xNode {
                        kind: E4xNodeKind::Text(t),
                        ..
                    }) = self.nodes.get(children[0])
                {
                    header.push('>');
                    header.push_str(&esc_xml_elem(t));
                    header.push_str(&format!("</{tag_name}>"));
                    return header;
                }
                header.push('>');
                if self.settings.pretty_printing {
                    let mut out = header;
                    for &cid in children {
                        out.push('\n');
                        out.push_str(&self.node_to_xml_string(cid, indent_level + 1));
                    }
                    out.push('\n');
                    out.push_str(&format!("{pad}</{tag_name}>"));
                    out
                } else {
                    let mut out = header;
                    for &cid in children {
                        out.push_str(&self.node_to_xml_string(cid, 0));
                    }
                    out.push_str(&format!("</{tag_name}>"));
                    out
                }
            }
        }
    }

    pub(super) fn parse_fragments(&mut self, input: &str) -> Result<Vec<usize>, (i32, String)> {
        let trimmed = if self.settings.ignore_whitespace {
            input.trim()
        } else {
            input
        };
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        if !trimmed.contains('<') {
            let nid = self.alloc_node(E4xNodeKind::Text(unescape_xml_entities(input)), None);
            return Ok(vec![nid]);
        }

        let mut pos = 0usize;
        let bytes = trimmed.as_bytes();
        let inherited_ns = if self.default_ns.is_empty() {
            Vec::new()
        } else {
            vec![(String::new(), self.default_ns.clone())]
        };
        let mut roots = Vec::new();
        while pos < bytes.len() {
            if bytes[pos] == b'<' {
                if trimmed[pos..].starts_with("<!--") {
                    let Some(end_rel) = trimmed[pos + 4..].find("-->") else {
                        return Err((1090, "XML parser failure: Unterminated comment.".to_owned()));
                    };
                    let comment_body = &trimmed[pos + 4..pos + 4 + end_rel];
                    pos += 4 + end_rel + 3;
                    if !self.settings.ignore_comments {
                        roots.push(
                            self.alloc_node(E4xNodeKind::Comment(comment_body.to_owned()), None),
                        );
                    }
                } else if trimmed[pos..].starts_with("<?") {
                    let Some(end_rel) = trimmed[pos + 2..].find("?>") else {
                        return Err((
                            1091,
                            "XML parser failure: Unterminated processing instruction.".to_owned(),
                        ));
                    };
                    let raw_inner = &trimmed[pos + 2..pos + 2 + end_rel];
                    let inner_trimmed = raw_inner.trim();
                    pos += 2 + end_rel + 2;
                    if !inner_trimmed.eq_ignore_ascii_case("xml")
                        && !inner_trimmed.to_ascii_lowercase().starts_with("xml ")
                        && !self.settings.ignore_processing_instructions
                    {
                        let (target, content) = match raw_inner.find(char::is_whitespace) {
                            Some(ws) => (&raw_inner[..ws], raw_inner[ws + 1..].trim_start()),
                            None => (raw_inner, ""),
                        };
                        roots.push(self.alloc_node(
                            E4xNodeKind::ProcessingInstruction {
                                target: target.to_owned(),
                                content: content.to_owned(),
                            },
                            None,
                        ));
                    }
                } else if trimmed[pos..].starts_with("<![CDATA[") {
                    let Some(end_rel) = trimmed[pos + 9..].find("]]>") else {
                        return Err((
                            1090,
                            "XML parser failure: Unterminated CDATA section.".to_owned(),
                        ));
                    };
                    let cdata = &trimmed[pos + 9..pos + 9 + end_rel];
                    pos += 9 + end_rel + 3;
                    roots.push(self.alloc_node(E4xNodeKind::Text(cdata.to_owned()), None));
                } else if trimmed[pos..].starts_with("<!DOCTYPE") {
                    let Some(end_rel) = trimmed[pos + 9..].find('>') else {
                        return Err((1090, "XML parser failure: Unterminated DOCTYPE.".to_owned()));
                    };
                    pos += 9 + end_rel + 1;
                } else {
                    let elem_id = self.parse_element(trimmed, &mut pos, None, &inherited_ns, 0)?;
                    roots.push(elem_id);
                }
            } else {
                let next_lt = trimmed[pos..].find('<').map_or(trimmed.len(), |r| pos + r);
                let raw_text = &trimmed[pos..next_lt];
                pos = next_lt;
                if !(self.settings.ignore_whitespace && is_xml_whitespace(raw_text)) {
                    let text_val = if self.settings.ignore_whitespace {
                        unescape_xml_entities(raw_text.trim())
                    } else {
                        unescape_xml_entities(raw_text)
                    };
                    if !text_val.is_empty() || !self.settings.ignore_whitespace {
                        roots.push(self.alloc_node(E4xNodeKind::Text(text_val), None));
                    }
                }
            }
        }
        Ok(roots)
    }

    fn parse_element(
        &mut self,
        src: &str,
        pos: &mut usize,
        parent: Option<usize>,
        active_ns: &[(String, String)],
        depth: usize,
    ) -> Result<usize, (i32, String)> {
        if depth > 64 {
            return Err((1090, "XML nesting limit exceeded.".to_owned()));
        }
        let bytes = src.as_bytes();
        if *pos >= bytes.len() || bytes[*pos] != b'<' {
            return Err((1090, "Expected '<'.".to_owned()));
        }
        *pos += 1;
        if *pos < bytes.len() && bytes[*pos] == b'/' {
            return Err((1085, "Unexpected closing tag.".to_owned()));
        }
        let name_start = *pos;
        while *pos < bytes.len()
            && !bytes[*pos].is_ascii_whitespace()
            && bytes[*pos] != b'>'
            && bytes[*pos] != b'/'
        {
            *pos += 1;
        }
        let full_tag = &src[name_start..*pos];
        if full_tag.is_empty() {
            return Err((1090, "Empty element tag name.".to_owned()));
        }
        let (tag_prefix, tag_local) = split_xml_qname(full_tag);

        let mut local_ns_decls: Vec<(String, String)> = Vec::new();
        let mut raw_attrs: Vec<(String, String)> = Vec::new();
        let mut self_closing = false;

        loop {
            while *pos < bytes.len() && bytes[*pos].is_ascii_whitespace() {
                *pos += 1;
            }
            if *pos >= bytes.len() {
                return Err((1090, "Unterminated start tag.".to_owned()));
            }
            if bytes[*pos] == b'>' {
                *pos += 1;
                break;
            }
            if src[*pos..].starts_with("/>") {
                *pos += 2;
                self_closing = true;
                break;
            }
            let a_start = *pos;
            while *pos < bytes.len()
                && !bytes[*pos].is_ascii_whitespace()
                && bytes[*pos] != b'='
                && bytes[*pos] != b'>'
                && bytes[*pos] != b'/'
            {
                *pos += 1;
            }
            let attr_name = &src[a_start..*pos];
            if attr_name.is_empty() {
                return Err((1090, "Malformed attribute in tag.".to_owned()));
            }
            while *pos < bytes.len() && bytes[*pos].is_ascii_whitespace() {
                *pos += 1;
            }
            if *pos >= bytes.len() || bytes[*pos] != b'=' {
                return Err((1090, "Missing '=' in XML attribute.".to_owned()));
            }
            *pos += 1;
            while *pos < bytes.len() && bytes[*pos].is_ascii_whitespace() {
                *pos += 1;
            }
            if *pos >= bytes.len() || (bytes[*pos] != b'"' && bytes[*pos] != b'\'') {
                return Err((1095, "XML attribute value must be quoted.".to_owned()));
            }
            let quote = bytes[*pos] as char;
            *pos += 1;
            let Some(val_end_rel) = src[*pos..].find(quote) else {
                return Err((1095, "Unterminated XML attribute value.".to_owned()));
            };
            let attr_val = unescape_xml_entities(&src[*pos..*pos + val_end_rel]);
            *pos += val_end_rel + 1;

            if attr_name == "xmlns" {
                local_ns_decls.push((String::new(), attr_val));
            } else if let Some(ns_pfx) = attr_name.strip_prefix("xmlns:") {
                local_ns_decls.push((ns_pfx.to_owned(), attr_val));
            } else {
                if raw_attrs.iter().any(|(existing, _)| existing == attr_name) {
                    return Err((1093, format!("Duplicate attribute {attr_name} specified.")));
                }
                raw_attrs.push((attr_name.to_owned(), attr_val));
            }
        }

        let mut merged_ns = active_ns.to_vec();
        for (p, u) in &local_ns_decls {
            if let Some(existing) = merged_ns.iter_mut().find(|(ep, _)| ep == p) {
                existing.1 = u.clone();
            } else {
                merged_ns.push((p.clone(), u.clone()));
            }
        }
        let elem_uri = merged_ns
            .iter()
            .rev()
            .find(|(p, _)| p == tag_prefix)
            .map(|(_, u)| u.clone())
            .unwrap_or_default();

        let elem_id = self.alloc_node(
            E4xNodeKind::Element {
                prefix: tag_prefix.to_owned(),
                uri: elem_uri,
                local_name: tag_local.to_owned(),
                attrs: Vec::new(),
                children: Vec::new(),
                in_scope_ns: local_ns_decls,
            },
            parent,
        );

        let mut attr_ids = Vec::with_capacity(raw_attrs.len());
        for (aname, aval) in raw_attrs {
            let (apfx, aloc) = split_xml_qname(&aname);
            let auri = if apfx.is_empty() {
                String::new()
            } else {
                merged_ns
                    .iter()
                    .rev()
                    .find(|(p, _)| p == apfx)
                    .map(|(_, u)| u.clone())
                    .unwrap_or_default()
            };
            let aid = self.alloc_node(
                E4xNodeKind::Attribute {
                    prefix: apfx.to_owned(),
                    uri: auri,
                    local_name: aloc.to_owned(),
                    value: aval,
                },
                Some(elem_id),
            );
            attr_ids.push(aid);
        }

        let mut child_ids = Vec::new();
        if !self_closing {
            loop {
                if *pos >= bytes.len() {
                    return Err((
                        1085,
                        format!(
                            "The element type \"{full_tag}\" must be terminated by the matching end-tag \"</{full_tag}>\"."
                        ),
                    ));
                }
                if src[*pos..].starts_with("</") {
                    *pos += 2;
                    let c_start = *pos;
                    while *pos < bytes.len()
                        && !bytes[*pos].is_ascii_whitespace()
                        && bytes[*pos] != b'>'
                    {
                        *pos += 1;
                    }
                    let close_tag = &src[c_start..*pos];
                    while *pos < bytes.len() && bytes[*pos].is_ascii_whitespace() {
                        *pos += 1;
                    }
                    if *pos >= bytes.len() || bytes[*pos] != b'>' || close_tag != full_tag {
                        return Err((
                            1085,
                            format!(
                                "The element type \"{full_tag}\" must be terminated by the matching end-tag \"</{full_tag}>\"."
                            ),
                        ));
                    }
                    *pos += 1;
                    break;
                } else if src[*pos..].starts_with("<!--") {
                    let Some(end_rel) = src[*pos + 4..].find("-->") else {
                        return Err((1090, "Unterminated comment.".to_owned()));
                    };
                    let body = &src[*pos + 4..*pos + 4 + end_rel];
                    *pos += 4 + end_rel + 3;
                    if !self.settings.ignore_comments {
                        child_ids.push(
                            self.alloc_node(E4xNodeKind::Comment(body.to_owned()), Some(elem_id)),
                        );
                    }
                } else if src[*pos..].starts_with("<?") {
                    let Some(end_rel) = src[*pos + 2..].find("?>") else {
                        return Err((1091, "Unterminated processing instruction.".to_owned()));
                    };
                    let raw_inner = &src[*pos + 2..*pos + 2 + end_rel];
                    *pos += 2 + end_rel + 2;
                    if !self.settings.ignore_processing_instructions {
                        let (target, content) = match raw_inner.find(char::is_whitespace) {
                            Some(ws) => (&raw_inner[..ws], raw_inner[ws + 1..].trim_start()),
                            None => (raw_inner, ""),
                        };
                        child_ids.push(self.alloc_node(
                            E4xNodeKind::ProcessingInstruction {
                                target: target.to_owned(),
                                content: content.to_owned(),
                            },
                            Some(elem_id),
                        ));
                    }
                } else if src[*pos..].starts_with("<![CDATA[") {
                    let Some(end_rel) = src[*pos + 9..].find("]]>") else {
                        return Err((1090, "Unterminated CDATA.".to_owned()));
                    };
                    let cdata = &src[*pos + 9..*pos + 9 + end_rel];
                    *pos += 9 + end_rel + 3;
                    child_ids
                        .push(self.alloc_node(E4xNodeKind::Text(cdata.to_owned()), Some(elem_id)));
                } else if bytes[*pos] == b'<' {
                    let cid = self.parse_element(src, pos, Some(elem_id), &merged_ns, depth + 1)?;
                    child_ids.push(cid);
                } else {
                    let next_lt = src[*pos..].find('<').map_or(src.len(), |r| *pos + r);
                    let raw_text = &src[*pos..next_lt];
                    *pos = next_lt;
                    if !(self.settings.ignore_whitespace && is_xml_whitespace(raw_text)) {
                        let text_val = if self.settings.ignore_whitespace {
                            unescape_xml_entities(raw_text.trim())
                        } else {
                            unescape_xml_entities(raw_text)
                        };
                        child_ids.push(self.alloc_node(E4xNodeKind::Text(text_val), Some(elem_id)));
                    }
                }
            }
        }

        if let Some(E4xNode {
            kind: E4xNodeKind::Element {
                attrs, children, ..
            },
            ..
        }) = self.nodes.get_mut(elem_id)
        {
            *attrs = attr_ids;
            *children = child_ids;
        }
        Ok(elem_id)
    }

    pub(super) fn collect_descendants(
        &self,
        nid: usize,
        prop: &str,
        out: &mut Vec<usize>,
        depth: usize,
    ) {
        if depth > 64 {
            return;
        }
        let Some(node) = self.nodes.get(nid) else {
            return;
        };
        let E4xNodeKind::Element {
            attrs, children, ..
        } = &node.kind
        else {
            return;
        };
        if let Some(attr_name) = prop.strip_prefix('@') {
            for &aid in attrs {
                if let Some(E4xNode {
                    kind: E4xNodeKind::Attribute { local_name, .. },
                    ..
                }) = self.nodes.get(aid)
                    && (attr_name == "*" || local_name == attr_name)
                {
                    out.push(aid);
                }
            }
            for &cid in children {
                self.collect_descendants(cid, prop, out, depth + 1);
            }
        } else {
            for &cid in children {
                if let Some(cnode) = self.nodes.get(cid) {
                    match &cnode.kind {
                        E4xNodeKind::Element { local_name, .. } => {
                            if prop == "*" || local_name == prop {
                                out.push(cid);
                            }
                            self.collect_descendants(cid, prop, out, depth + 1);
                        }
                        E4xNodeKind::Text(_)
                        | E4xNodeKind::Comment(_)
                        | E4xNodeKind::ProcessingInstruction { .. }
                            if prop == "*" =>
                        {
                            out.push(cid);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub(super) fn nodes_equal(&self, a: usize, b: usize) -> bool {
        if a == b {
            return true;
        }
        let (Some(na), Some(nb)) = (self.nodes.get(a), self.nodes.get(b)) else {
            return false;
        };
        match (&na.kind, &nb.kind) {
            (E4xNodeKind::Text(sa), E4xNodeKind::Text(sb)) => sa == sb,
            (E4xNodeKind::Comment(sa), E4xNodeKind::Comment(sb)) => sa == sb,
            (
                E4xNodeKind::ProcessingInstruction {
                    target: ta,
                    content: ca,
                },
                E4xNodeKind::ProcessingInstruction {
                    target: tb,
                    content: cb,
                },
            ) => ta == tb && ca == cb,
            (
                E4xNodeKind::Attribute {
                    uri: ua,
                    local_name: la,
                    value: va,
                    ..
                },
                E4xNodeKind::Attribute {
                    uri: ub,
                    local_name: lb,
                    value: vb,
                    ..
                },
            ) => ua == ub && la == lb && va == vb,
            (
                E4xNodeKind::Element {
                    uri: ua,
                    local_name: la,
                    attrs: aa,
                    children: ca,
                    ..
                },
                E4xNodeKind::Element {
                    uri: ub,
                    local_name: lb,
                    attrs: ab,
                    children: cb,
                    ..
                },
            ) => {
                ua == ub
                    && la == lb
                    && aa.len() == ab.len()
                    && ca.len() == cb.len()
                    && aa
                        .iter()
                        .all(|&aid| ab.iter().any(|&bid| self.nodes_equal(aid, bid)))
                    && ca
                        .iter()
                        .zip(cb.iter())
                        .all(|(&cid_a, &cid_b)| self.nodes_equal(cid_a, cid_b))
            }
            _ => false,
        }
    }
}

impl SwfBytecodePlayer {
    pub(super) fn alloc_avm2_xml(&mut self, nid: usize) -> Avm1Value {
        let oid = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object {
            avm2_xml: Some(Avm2XmlData {
                arena: Rc::clone(&self.e4x_arena),
                kind: Avm2XmlKind::Xml(nid),
            }),
            ..Default::default()
        });
        Avm1Value::Object(oid)
    }

    pub(super) fn alloc_avm2_xml_list(
        &mut self,
        items: Vec<usize>,
        target_node: Option<usize>,
        target_prop: Option<String>,
    ) -> Avm1Value {
        let oid = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object {
            avm2_xml: Some(Avm2XmlData {
                arena: Rc::clone(&self.e4x_arena),
                kind: Avm2XmlKind::XmlList {
                    items,
                    target_node,
                    target_prop,
                },
            }),
            ..Default::default()
        });
        Avm1Value::Object(oid)
    }

    pub(super) fn construct_avm2_xml(
        &mut self,
        args: &[Avm1Value],
        is_class_call: bool,
        abc_idx: usize,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        let Some(first) = args.first() else {
            let nid = self
                .e4x_arena
                .borrow_mut()
                .alloc_node(E4xNodeKind::Text(String::new()), None);
            return self.alloc_avm2_xml(nid);
        };
        if matches!(first, Avm1Value::Null | Avm1Value::Undefined) {
            let nid = self
                .e4x_arena
                .borrow_mut()
                .alloc_node(E4xNodeKind::Text(String::new()), None);
            return self.alloc_avm2_xml(nid);
        }
        if let Avm1Value::Object(oid) = first
            && let Some(xd) = self.avm1_objects.get(*oid).and_then(|o| o.avm2_xml.clone())
        {
            match xd.kind {
                Avm2XmlKind::Xml(nid) => {
                    if is_class_call {
                        return first.clone();
                    }
                    let copied = self.e4x_arena.borrow_mut().deep_copy(nid, None);
                    return self.alloc_avm2_xml(copied);
                }
                Avm2XmlKind::XmlList { items, .. } => {
                    if items.len() == 1 {
                        if is_class_call {
                            return self.alloc_avm2_xml(items[0]);
                        }
                        let copied = self.e4x_arena.borrow_mut().deep_copy(items[0], None);
                        return self.alloc_avm2_xml(copied);
                    }
                    self.throw_avm2_builtin_error(
                        "TypeError",
                        1088,
                        "The markup in the document following the root element must be well-formed.",
                    );
                    return Avm1Value::Undefined;
                }
            }
        }
        let raw = self.avm2_coerce_to_string_eval(first, abc_idx, steps, depth);
        if self.avm2_thrown.is_some() {
            return Avm1Value::Undefined;
        }
        let parsed = self.e4x_arena.borrow_mut().parse_fragments(&raw);
        match parsed {
            Ok(mut roots) => {
                if roots.is_empty() {
                    let nid = self
                        .e4x_arena
                        .borrow_mut()
                        .alloc_node(E4xNodeKind::Text(String::new()), None);
                    self.alloc_avm2_xml(nid)
                } else if roots.len() == 1 {
                    self.alloc_avm2_xml(roots.remove(0))
                } else {
                    self.throw_avm2_builtin_error(
                        "TypeError",
                        1088,
                        "The markup in the document following the root element must be well-formed.",
                    );
                    Avm1Value::Undefined
                }
            }
            Err((code, msg)) => {
                self.throw_avm2_builtin_error("TypeError", code, &msg);
                Avm1Value::Undefined
            }
        }
    }

    pub(super) fn construct_avm2_xml_list(
        &mut self,
        args: &[Avm1Value],
        is_class_call: bool,
        abc_idx: usize,
        steps: &mut usize,
        depth: usize,
    ) -> Avm1Value {
        let Some(first) = args.first() else {
            return self.alloc_avm2_xml_list(Vec::new(), None, None);
        };
        if matches!(first, Avm1Value::Null | Avm1Value::Undefined) {
            return self.alloc_avm2_xml_list(Vec::new(), None, None);
        }
        if let Avm1Value::Object(oid) = first
            && let Some(xd) = self.avm1_objects.get(*oid).and_then(|o| o.avm2_xml.clone())
        {
            match xd.kind {
                Avm2XmlKind::XmlList { .. } if is_class_call => return first.clone(),
                Avm2XmlKind::XmlList { items, .. } => {
                    return self.alloc_avm2_xml_list(items, None, None);
                }
                Avm2XmlKind::Xml(nid) => {
                    return self.alloc_avm2_xml_list(vec![nid], None, None);
                }
            }
        }
        let raw = self.avm2_coerce_to_string_eval(first, abc_idx, steps, depth);
        if self.avm2_thrown.is_some() {
            return Avm1Value::Undefined;
        }
        let parsed = self.e4x_arena.borrow_mut().parse_fragments(&raw);
        match parsed {
            Ok(roots) => self.alloc_avm2_xml_list(roots, None, None),
            Err((code, msg)) => {
                self.throw_avm2_builtin_error("TypeError", code, &msg);
                Avm1Value::Undefined
            }
        }
    }

    pub(super) fn xml_items_of_oid(&self, oid: usize) -> Option<(Vec<usize>, Option<usize>)> {
        let xd = self.avm1_objects.get(oid)?.avm2_xml.as_ref()?;
        match &xd.kind {
            Avm2XmlKind::Xml(nid) => Some((vec![*nid], Some(*nid))),
            Avm2XmlKind::XmlList { items, .. } => Some((items.clone(), None)),
        }
    }

    pub(super) fn get_avm2_xml_property(&mut self, oid: usize, prop: &str) -> Option<Avm1Value> {
        let (items, single_target) = self.xml_items_of_oid(oid)?;
        if let Ok(idx) = prop.parse::<usize>() {
            if single_target.is_some() {
                return Some(if idx == 0 {
                    Avm1Value::Object(oid)
                } else {
                    Avm1Value::Undefined
                });
            }
            return Some(if let Some(&nid) = items.get(idx) {
                self.alloc_avm2_xml(nid)
            } else {
                Avm1Value::Undefined
            });
        }
        let mut matched = Vec::new();
        {
            let arena = self.e4x_arena.borrow();
            if let Some(attr_name) = prop.strip_prefix('@') {
                for &nid in &items {
                    if let Some(E4xNode {
                        kind: E4xNodeKind::Element { attrs, .. },
                        ..
                    }) = arena.nodes.get(nid)
                    {
                        for &aid in attrs {
                            if let Some(E4xNode {
                                kind: E4xNodeKind::Attribute { local_name, .. },
                                ..
                            }) = arena.nodes.get(aid)
                                && (attr_name == "*" || local_name == attr_name)
                            {
                                matched.push(aid);
                            }
                        }
                    }
                }
            } else {
                for &nid in &items {
                    if let Some(E4xNode {
                        kind: E4xNodeKind::Element { children, .. },
                        ..
                    }) = arena.nodes.get(nid)
                    {
                        for &cid in children {
                            if prop == "*" {
                                matched.push(cid);
                            } else if let Some(E4xNode {
                                kind: E4xNodeKind::Element { local_name, .. },
                                ..
                            }) = arena.nodes.get(cid)
                                && local_name == prop
                            {
                                matched.push(cid);
                            }
                        }
                    }
                }
            }
        }
        let target_node = single_target.or_else(|| (items.len() == 1).then(|| items[0]));
        Some(self.alloc_avm2_xml_list(matched, target_node, Some(prop.to_owned())))
    }

    pub(super) fn avm2_xml_has_property(&self, oid: usize, prop: &str) -> bool {
        let Some((items, single_target)) = self.xml_items_of_oid(oid) else {
            return false;
        };
        if let Ok(idx) = prop.parse::<usize>() {
            return if single_target.is_some() {
                idx == 0
            } else {
                idx < items.len()
            };
        }
        let arena = self.e4x_arena.borrow();
        if let Some(attr_name) = prop.strip_prefix('@') {
            items.iter().any(|&nid| {
                if let Some(E4xNode {
                    kind: E4xNodeKind::Element { attrs, .. },
                    ..
                }) = arena.nodes.get(nid)
                {
                    attrs.iter().any(|&aid| {
                        matches!(
                            arena.nodes.get(aid).map(|n| &n.kind),
                            Some(E4xNodeKind::Attribute { local_name, .. })
                                if attr_name == "*" || local_name == attr_name
                        )
                    })
                } else {
                    false
                }
            })
        } else {
            items.iter().any(|&nid| {
                if let Some(E4xNode {
                    kind: E4xNodeKind::Element { children, .. },
                    ..
                }) = arena.nodes.get(nid)
                {
                    children.iter().any(|&cid| {
                        if prop == "*" {
                            true
                        } else {
                            matches!(
                                arena.nodes.get(cid).map(|n| &n.kind),
                                Some(E4xNodeKind::Element { local_name, .. }) if local_name == prop
                            )
                        }
                    })
                } else {
                    false
                }
            })
        }
    }

    pub(super) fn set_avm2_xml_property(
        &mut self,
        oid: usize,
        prop: &str,
        val: Avm1Value,
        abc_idx: usize,
        steps: &mut usize,
        depth: usize,
    ) -> bool {
        let Some(xd) = self.avm1_objects.get(oid).and_then(|o| o.avm2_xml.clone()) else {
            return false;
        };
        // Setting an integer index on an XMLList (used heavily by E4X `.(predicate)` filter loops: `res[idx++] = item`)
        if let Avm2XmlKind::XmlList { mut items, .. } = xd.kind.clone()
            && let Ok(idx) = prop.parse::<usize>()
        {
            let new_nids = self.value_to_e4x_nodes(&val, abc_idx, steps, depth);
            if self.avm2_thrown.is_some() {
                return true;
            }
            if idx >= items.len() {
                items.extend(new_nids);
            } else if new_nids.len() == 1 {
                items[idx] = new_nids[0];
            } else {
                items.splice(idx..=idx, new_nids);
            }
            if let Some(dst) = self
                .avm1_objects
                .get_mut(oid)
                .and_then(|o| o.avm2_xml.as_mut())
                && let Avm2XmlKind::XmlList { items: dst_it, .. } = &mut dst.kind
            {
                *dst_it = items;
            }
            return true;
        }

        let target_nid = match xd.kind {
            Avm2XmlKind::Xml(nid) => Some(nid),
            Avm2XmlKind::XmlList { items, .. } if items.len() == 1 => Some(items[0]),
            Avm2XmlKind::XmlList {
                items,
                target_node: Some(tn),
                target_prop: Some(tp),
            } if items.is_empty() && tp != "*" && !tp.starts_with('@') => {
                let created = {
                    let mut arena = self.e4x_arena.borrow_mut();
                    let default_ns = arena.default_ns.clone();
                    let new_elem = arena.alloc_node(
                        E4xNodeKind::Element {
                            prefix: String::new(),
                            uri: default_ns,
                            local_name: tp,
                            attrs: Vec::new(),
                            children: Vec::new(),
                            in_scope_ns: Vec::new(),
                        },
                        Some(tn),
                    );
                    if let Some(E4xNode {
                        kind: E4xNodeKind::Element { children, .. },
                        ..
                    }) = arena.nodes.get_mut(tn)
                    {
                        children.push(new_elem);
                    }
                    new_elem
                };
                if let Some(dst) = self
                    .avm1_objects
                    .get_mut(oid)
                    .and_then(|o| o.avm2_xml.as_mut())
                    && let Avm2XmlKind::XmlList { items: dst_it, .. } = &mut dst.kind
                {
                    dst_it.push(created);
                }
                Some(created)
            }
            Avm2XmlKind::XmlList { .. } => {
                self.throw_avm2_builtin_error(
                    "TypeError",
                    1089,
                    "Assignment to lists with more than one item is not supported.",
                );
                return true;
            }
        };
        let Some(nid) = target_nid else {
            return true;
        };

        if let Some(attr_name) = prop.strip_prefix('@') {
            let sval = self.avm2_coerce_to_string_eval(&val, abc_idx, steps, depth);
            if self.avm2_thrown.is_some() {
                return true;
            }
            let mut arena = self.e4x_arena.borrow_mut();
            let Some(E4xNode {
                kind: E4xNodeKind::Element { attrs, .. },
                ..
            }) = arena.nodes.get(nid).cloned()
            else {
                return true;
            };
            let mut matching: Vec<usize> = attrs
                .iter()
                .copied()
                .filter(|&aid| {
                    matches!(
                        arena.nodes.get(aid).map(|n| &n.kind),
                        Some(E4xNodeKind::Attribute { local_name, .. })
                            if attr_name == "*" || local_name == attr_name
                    )
                })
                .collect();
            if let Some(first_aid) = matching.first().copied() {
                if let Some(E4xNode {
                    kind: E4xNodeKind::Attribute { value, .. },
                    ..
                }) = arena.nodes.get_mut(first_aid)
                {
                    *value = sval;
                }
                for extra in matching.drain(1..) {
                    if let Some(en) = arena.nodes.get_mut(extra) {
                        en.parent = None;
                    }
                    if let Some(E4xNode {
                        kind: E4xNodeKind::Element { attrs: a_mut, .. },
                        ..
                    }) = arena.nodes.get_mut(nid)
                    {
                        a_mut.retain(|&x| x != extra);
                    }
                }
            } else if attr_name != "*" {
                let (apfx, aloc) = split_xml_qname(attr_name);
                let aid = arena.alloc_node(
                    E4xNodeKind::Attribute {
                        prefix: apfx.to_owned(),
                        uri: String::new(),
                        local_name: aloc.to_owned(),
                        value: sval,
                    },
                    Some(nid),
                );
                if let Some(E4xNode {
                    kind: E4xNodeKind::Element { attrs: a_mut, .. },
                    ..
                }) = arena.nodes.get_mut(nid)
                {
                    a_mut.push(aid);
                }
            }
            return true;
        }

        // Element child property assignment (`xml.child = ...`)
        let replacement_nodes =
            self.value_to_named_child_nodes(prop, &val, nid, abc_idx, steps, depth);
        if self.avm2_thrown.is_some() {
            return true;
        }
        let mut arena = self.e4x_arena.borrow_mut();
        let Some(E4xNode {
            kind: E4xNodeKind::Element { mut children, .. },
            ..
        }) = arena.nodes.get(nid).cloned()
        else {
            return true;
        };
        if prop == "*" {
            for &old in &children {
                if let Some(n) = arena.nodes.get_mut(old) {
                    n.parent = None;
                }
            }
            children = replacement_nodes;
        } else {
            let mut first_pos = None;
            let mut kept = Vec::with_capacity(children.len());
            for cid in children {
                let is_match = matches!(
                    arena.nodes.get(cid).map(|n| &n.kind),
                    Some(E4xNodeKind::Element { local_name, .. }) if local_name == prop
                );
                if is_match {
                    if let Some(n) = arena.nodes.get_mut(cid) {
                        n.parent = None;
                    }
                    if first_pos.is_none() {
                        first_pos = Some(kept.len());
                    }
                } else {
                    kept.push(cid);
                }
            }
            let ins_pos = first_pos.unwrap_or(kept.len());
            kept.splice(ins_pos..ins_pos, replacement_nodes);
            children = kept;
        }
        if let Some(E4xNode {
            kind: E4xNodeKind::Element {
                children: c_mut, ..
            },
            ..
        }) = arena.nodes.get_mut(nid)
        {
            *c_mut = children;
        }
        true
    }

    pub(super) fn delete_avm2_xml_property(&mut self, oid: usize, prop: &str) -> Option<bool> {
        let xd = self.avm1_objects.get(oid)?.avm2_xml.clone()?;
        let mut arena = self.e4x_arena.borrow_mut();
        match xd.kind {
            Avm2XmlKind::Xml(nid) => {
                if prop.parse::<usize>().is_ok() {
                    return Some(true);
                }
                Self::delete_on_e4x_node(&mut arena, nid, prop);
                Some(true)
            }
            Avm2XmlKind::XmlList { mut items, .. } => {
                if let Ok(idx) = prop.parse::<usize>() {
                    if idx < items.len() {
                        let target_id = items.remove(idx);
                        if let Some(parent_id) = arena.nodes.get(target_id).and_then(|n| n.parent)
                            && let Some(E4xNode {
                                kind:
                                    E4xNodeKind::Element {
                                        attrs, children, ..
                                    },
                                ..
                            }) = arena.nodes.get_mut(parent_id)
                        {
                            attrs.retain(|&x| x != target_id);
                            children.retain(|&x| x != target_id);
                        }
                        if let Some(tn) = arena.nodes.get_mut(target_id) {
                            tn.parent = None;
                        }
                        drop(arena);
                        if let Some(dst) = self
                            .avm1_objects
                            .get_mut(oid)
                            .and_then(|o| o.avm2_xml.as_mut())
                            && let Avm2XmlKind::XmlList { items: dst_it, .. } = &mut dst.kind
                        {
                            *dst_it = items;
                        }
                    }
                    return Some(true);
                }
                for nid in items {
                    Self::delete_on_e4x_node(&mut arena, nid, prop);
                }
                Some(true)
            }
        }
    }

    fn delete_on_e4x_node(arena: &mut E4xArena, nid: usize, prop: &str) {
        let Some(E4xNode {
            kind:
                E4xNodeKind::Element {
                    mut attrs,
                    mut children,
                    ..
                },
            ..
        }) = arena.nodes.get(nid).cloned()
        else {
            return;
        };
        if let Some(attr_name) = prop.strip_prefix('@') {
            attrs.retain(|&aid| {
                let is_match = matches!(
                    arena.nodes.get(aid).map(|n| &n.kind),
                    Some(E4xNodeKind::Attribute { local_name, .. })
                        if attr_name == "*" || local_name == attr_name
                );
                if is_match && let Some(an) = arena.nodes.get_mut(aid) {
                    an.parent = None;
                }
                !is_match
            });
        } else {
            children.retain(|&cid| {
                let is_match = if prop == "*" {
                    true
                } else {
                    matches!(
                        arena.nodes.get(cid).map(|n| &n.kind),
                        Some(E4xNodeKind::Element { local_name, .. }) if local_name == prop
                    )
                };
                if is_match && let Some(cn) = arena.nodes.get_mut(cid) {
                    cn.parent = None;
                }
                !is_match
            });
        }
        if let Some(E4xNode {
            kind:
                E4xNodeKind::Element {
                    attrs: a_mut,
                    children: c_mut,
                    ..
                },
            ..
        }) = arena.nodes.get_mut(nid)
        {
            *a_mut = attrs;
            *c_mut = children;
        }
    }

    fn value_to_e4x_nodes(
        &mut self,
        val: &Avm1Value,
        abc_idx: usize,
        steps: &mut usize,
        depth: usize,
    ) -> Vec<usize> {
        if let Avm1Value::Object(oid) = val
            && let Some((items, _)) = self.xml_items_of_oid(*oid)
        {
            return items;
        }
        let s = self.avm2_coerce_to_string_eval(val, abc_idx, steps, depth);
        if self.avm2_thrown.is_some() {
            return Vec::new();
        }
        let mut arena = self.e4x_arena.borrow_mut();
        match arena.parse_fragments(&s) {
            Ok(nodes) if !nodes.is_empty() => nodes,
            _ => vec![arena.alloc_node(E4xNodeKind::Text(s), None)],
        }
    }

    fn value_to_named_child_nodes(
        &mut self,
        prop: &str,
        val: &Avm1Value,
        parent_nid: usize,
        abc_idx: usize,
        steps: &mut usize,
        depth: usize,
    ) -> Vec<usize> {
        if let Avm1Value::Object(oid) = val
            && let Some((items, _)) = self.xml_items_of_oid(*oid)
        {
            let mut arena = self.e4x_arena.borrow_mut();
            return items
                .into_iter()
                .map(|id| arena.deep_copy(id, Some(parent_nid)))
                .collect();
        }
        let s = self.avm2_coerce_to_string_eval(val, abc_idx, steps, depth);
        if self.avm2_thrown.is_some() {
            return Vec::new();
        }
        let mut arena = self.e4x_arena.borrow_mut();
        if prop == "*" {
            let txt = arena.alloc_node(E4xNodeKind::Text(s), Some(parent_nid));
            return vec![txt];
        }
        let default_ns = arena.default_ns.clone();
        let elem = arena.alloc_node(
            E4xNodeKind::Element {
                prefix: String::new(),
                uri: default_ns,
                local_name: prop.to_owned(),
                attrs: Vec::new(),
                children: Vec::new(),
                in_scope_ns: Vec::new(),
            },
            Some(parent_nid),
        );
        if !s.is_empty() {
            let txt = arena.alloc_node(E4xNodeKind::Text(s), Some(elem));
            if let Some(E4xNode {
                kind: E4xNodeKind::Element { children, .. },
                ..
            }) = arena.nodes.get_mut(elem)
            {
                children.push(txt);
            }
        }
        vec![elem]
    }

    pub(super) fn get_avm2_descendants(&mut self, obj: &Avm1Value, prop: &str) -> Avm1Value {
        let Avm1Value::Object(oid) = obj else {
            self.throw_avm2_builtin_error(
                "TypeError",
                1016,
                "Descendants operator (..) not supported on non-XML type.",
            );
            return Avm1Value::Undefined;
        };
        let Some((items, _)) = self.xml_items_of_oid(*oid) else {
            self.throw_avm2_builtin_error(
                "TypeError",
                1016,
                "Descendants operator (..) not supported on non-XML type.",
            );
            return Avm1Value::Undefined;
        };
        let mut out = Vec::new();
        {
            let arena = self.e4x_arena.borrow();
            for nid in items {
                arena.collect_descendants(nid, prop, &mut out, 0);
            }
        }
        self.alloc_avm2_xml_list(out, None, None)
    }

    pub(super) fn call_avm2_xml_method(
        &mut self,
        oid: usize,
        prop: &str,
        args: &[Avm1Value],
        abc_idx: usize,
        steps: &mut usize,
        depth: usize,
    ) -> Option<Avm1Value> {
        let (items, single_xml) = self.xml_items_of_oid(oid)?;
        let require_single = |this: &mut Self| -> Option<usize> {
            if let Some(nid) = single_xml {
                Some(nid)
            } else if items.len() == 1 {
                Some(items[0])
            } else {
                this.throw_avm2_builtin_error(
                    "TypeError",
                    1086,
                    &format!("The {prop} method only works on lists containing one item."),
                );
                None
            }
        };

        let qname_arg = |arg: Option<&Avm1Value>, objects: &[Avm1Object]| -> String {
            let Some(v) = arg else {
                return "*".to_owned();
            };
            if let Avm1Value::Object(qoid) = v
                && let Some(qo) = objects.get(*qoid)
                && let Some(Avm1Value::String(ln)) = qo.props.get("localName")
            {
                return ln.clone();
            }
            avm2_convert_string(v, objects)
        };

        match prop {
            "toString" | "valueOf" => {
                let s = {
                    let arena = self.e4x_arena.borrow();
                    if let Some(nid) = single_xml {
                        arena.node_to_string(nid)
                    } else {
                        arena.list_to_string(&items)
                    }
                };
                Some(Avm1Value::String(s))
            }
            "toXMLString" => {
                let s = {
                    let arena = self.e4x_arena.borrow();
                    if let Some(nid) = single_xml {
                        arena.node_to_xml_string(nid, 0)
                    } else {
                        arena.list_to_xml_string(&items)
                    }
                };
                Some(Avm1Value::String(s))
            }
            "length" => {
                let n = if single_xml.is_some() { 1 } else { items.len() };
                Some(Avm1Value::Number(n as f64))
            }
            "hasSimpleContent" => {
                let res = {
                    let arena = self.e4x_arena.borrow();
                    if let Some(nid) = single_xml {
                        arena.node_has_simple_content(nid)
                    } else {
                        arena.list_has_simple_content(&items)
                    }
                };
                Some(Avm1Value::Bool(res))
            }
            "hasComplexContent" => {
                let res = {
                    let arena = self.e4x_arena.borrow();
                    if let Some(nid) = single_xml {
                        arena.node_has_complex_content(nid)
                    } else {
                        arena.list_has_complex_content(&items)
                    }
                };
                Some(Avm1Value::Bool(res))
            }
            "copy" => {
                if let Some(nid) = single_xml {
                    let copied = self.e4x_arena.borrow_mut().deep_copy(nid, None);
                    Some(self.alloc_avm2_xml(copied))
                } else {
                    let copied: Vec<usize> = {
                        let mut arena = self.e4x_arena.borrow_mut();
                        items.iter().map(|&id| arena.deep_copy(id, None)).collect()
                    };
                    Some(self.alloc_avm2_xml_list(copied, None, None))
                }
            }
            "parent" => {
                if items.is_empty() {
                    return Some(Avm1Value::Undefined);
                }
                let parent_opt = {
                    let arena = self.e4x_arena.borrow();
                    let first_p = arena.nodes.get(items[0]).and_then(|n| n.parent);
                    if items
                        .iter()
                        .all(|&id| arena.nodes.get(id).and_then(|n| n.parent) == first_p)
                    {
                        first_p
                    } else {
                        None
                    }
                };
                Some(match parent_opt {
                    Some(pid) => self.alloc_avm2_xml(pid),
                    None => Avm1Value::Undefined,
                })
            }
            "children" => self.get_avm2_xml_property(oid, "*"),
            "child" => {
                let key = qname_arg(args.first(), &self.avm1_objects);
                if let Ok(idx) = key.parse::<usize>() {
                    let mut all_children = Vec::new();
                    {
                        let arena = self.e4x_arena.borrow();
                        for &nid in &items {
                            if let Some(E4xNode {
                                kind: E4xNodeKind::Element { children, .. },
                                ..
                            }) = arena.nodes.get(nid)
                                && let Some(&cid) = children.get(idx)
                            {
                                all_children.push(cid);
                            }
                        }
                    }
                    Some(self.alloc_avm2_xml_list(all_children, None, None))
                } else {
                    self.get_avm2_xml_property(oid, &key)
                }
            }
            "elements" => {
                let filter_name = qname_arg(args.first(), &self.avm1_objects);
                let mut out = Vec::new();
                {
                    let arena = self.e4x_arena.borrow();
                    for &nid in &items {
                        if let Some(E4xNode {
                            kind: E4xNodeKind::Element { children, .. },
                            ..
                        }) = arena.nodes.get(nid)
                        {
                            for &cid in children {
                                if let Some(E4xNode {
                                    kind: E4xNodeKind::Element { local_name, .. },
                                    ..
                                }) = arena.nodes.get(cid)
                                    && (filter_name == "*" || local_name == &filter_name)
                                {
                                    out.push(cid);
                                }
                            }
                        }
                    }
                }
                Some(self.alloc_avm2_xml_list(out, None, None))
            }
            "text" => {
                let mut out = Vec::new();
                {
                    let arena = self.e4x_arena.borrow();
                    for &nid in &items {
                        if let Some(E4xNode {
                            kind: E4xNodeKind::Element { children, .. },
                            ..
                        }) = arena.nodes.get(nid)
                        {
                            for &cid in children {
                                if matches!(
                                    arena.nodes.get(cid).map(|n| &n.kind),
                                    Some(E4xNodeKind::Text(_))
                                ) {
                                    out.push(cid);
                                }
                            }
                        }
                    }
                }
                Some(self.alloc_avm2_xml_list(out, None, None))
            }
            "attributes" => self.get_avm2_xml_property(oid, "@*"),
            "attribute" => {
                let aname = qname_arg(args.first(), &self.avm1_objects);
                let attr_prop = if aname.starts_with('@') {
                    aname
                } else {
                    format!("@{aname}")
                };
                self.get_avm2_xml_property(oid, &attr_prop)
            }
            "descendants" => {
                let dname = qname_arg(args.first(), &self.avm1_objects);
                Some(self.get_avm2_descendants(&Avm1Value::Object(oid), &dname))
            }
            "contains" => {
                let Some(arg0) = args.first() else {
                    return Some(Avm1Value::Bool(false));
                };
                if let Avm1Value::Object(other_oid) = arg0
                    && let Some((other_items, _)) = self.xml_items_of_oid(*other_oid)
                    && other_items.len() == 1
                {
                    let arena = self.e4x_arena.borrow();
                    let found = items
                        .iter()
                        .any(|&id| arena.nodes_equal(id, other_items[0]));
                    return Some(Avm1Value::Bool(found));
                }
                Some(Avm1Value::Bool(false))
            }
            "hasOwnProperty" => {
                let key = args
                    .first()
                    .map_or_else(String::new, |v| avm2_convert_string(v, &self.avm1_objects));
                Some(Avm1Value::Bool(self.avm2_xml_has_property(oid, &key)))
            }
            "nodeKind" => {
                let Some(nid) = require_single(self) else {
                    return Some(Avm1Value::Undefined);
                };
                let kind_str = {
                    let arena = self.e4x_arena.borrow();
                    match arena.nodes.get(nid).map(|n| &n.kind) {
                        Some(E4xNodeKind::Element { .. }) => "element",
                        Some(E4xNodeKind::Attribute { .. }) => "attribute",
                        Some(E4xNodeKind::Text(_)) => "text",
                        Some(E4xNodeKind::Comment(_)) => "comment",
                        Some(E4xNodeKind::ProcessingInstruction { .. }) => "processing-instruction",
                        None => "text",
                    }
                };
                Some(Avm1Value::String(kind_str.to_owned()))
            }
            "localName" => {
                let Some(nid) = require_single(self) else {
                    return Some(Avm1Value::Undefined);
                };
                let ln = {
                    let arena = self.e4x_arena.borrow();
                    match arena.nodes.get(nid).map(|n| &n.kind) {
                        Some(E4xNodeKind::Element { local_name, .. })
                        | Some(E4xNodeKind::Attribute { local_name, .. }) => {
                            Some(local_name.clone())
                        }
                        Some(E4xNodeKind::ProcessingInstruction { target, .. }) => {
                            Some(target.clone())
                        }
                        _ => None,
                    }
                };
                Some(ln.map_or(Avm1Value::Null, Avm1Value::String))
            }
            "name" => {
                let Some(nid) = require_single(self) else {
                    return Some(Avm1Value::Undefined);
                };
                let qinfo = {
                    let arena = self.e4x_arena.borrow();
                    match arena.nodes.get(nid).map(|n| &n.kind) {
                        Some(E4xNodeKind::Element {
                            uri, local_name, ..
                        })
                        | Some(E4xNodeKind::Attribute {
                            uri, local_name, ..
                        }) => Some((uri.clone(), local_name.clone())),
                        Some(E4xNodeKind::ProcessingInstruction { target, .. }) => {
                            Some((String::new(), target.clone()))
                        }
                        _ => None,
                    }
                };
                let Some((uri, local_name)) = qinfo else {
                    return Some(Avm1Value::Null);
                };
                Some(self.alloc_avm2_qname(&uri, &local_name))
            }
            "setName" | "setLocalName" => {
                let Some(nid) = require_single(self) else {
                    return Some(Avm1Value::Undefined);
                };
                let new_name = qname_arg(args.first(), &self.avm1_objects);
                let mut arena = self.e4x_arena.borrow_mut();
                if let Some(node) = arena.nodes.get_mut(nid) {
                    match &mut node.kind {
                        E4xNodeKind::Element { local_name, .. }
                        | E4xNodeKind::Attribute { local_name, .. } => {
                            *local_name = new_name;
                        }
                        E4xNodeKind::ProcessingInstruction { target, .. } => {
                            *target = new_name;
                        }
                        _ => {}
                    }
                }
                Some(Avm1Value::Undefined)
            }
            "childIndex" => {
                let Some(nid) = require_single(self) else {
                    return Some(Avm1Value::Undefined);
                };
                let idx = {
                    let arena = self.e4x_arena.borrow();
                    arena
                        .nodes
                        .get(nid)
                        .and_then(|n| n.parent)
                        .and_then(|pid| match arena.nodes.get(pid).map(|p| &p.kind) {
                            Some(E4xNodeKind::Element { children, .. }) => {
                                children.iter().position(|&c| c == nid).map(|i| i as i32)
                            }
                            _ => None,
                        })
                        .unwrap_or(-1)
                };
                Some(Avm1Value::Number(f64::from(idx)))
            }
            "appendChild" | "prependChild" => {
                let Some(nid) = require_single(self) else {
                    return Some(Avm1Value::Undefined);
                };
                if let Some(arg0) = args.first() {
                    let new_nodes = self.value_to_e4x_nodes(arg0, abc_idx, steps, depth);
                    if self.avm2_thrown.is_some() {
                        return Some(Avm1Value::Undefined);
                    }
                    let mut arena = self.e4x_arena.borrow_mut();
                    let prepared: Vec<usize> = new_nodes
                        .into_iter()
                        .map(|cid| {
                            if arena.nodes.get(cid).and_then(|n| n.parent).is_some() {
                                arena.deep_copy(cid, Some(nid))
                            } else {
                                if let Some(cn) = arena.nodes.get_mut(cid) {
                                    cn.parent = Some(nid);
                                }
                                cid
                            }
                        })
                        .collect();
                    if let Some(E4xNode {
                        kind: E4xNodeKind::Element { children, .. },
                        ..
                    }) = arena.nodes.get_mut(nid)
                    {
                        if prop == "prependChild" {
                            let mut combined = prepared;
                            combined.append(children);
                            *children = combined;
                        } else {
                            children.extend(prepared);
                        }
                    }
                }
                Some(Avm1Value::Object(oid))
            }
            "setChildren" => {
                if require_single(self).is_none() {
                    return Some(Avm1Value::Undefined);
                }
                let val = args.first().cloned().unwrap_or(Avm1Value::Undefined);
                let _ = self.set_avm2_xml_property(oid, "*", val, abc_idx, steps, depth);
                Some(Avm1Value::Object(oid))
            }
            // String.prototype delegation on simple-content XML / XMLList (ECMA-357 §11.2.2.1)
            "split" | "indexOf" | "lastIndexOf" | "slice" | "substring" | "substr"
            | "toLowerCase" | "toUpperCase" | "charAt" | "charCodeAt" | "replace" => {
                let s = {
                    let arena = self.e4x_arena.borrow();
                    if let Some(nid) = single_xml {
                        arena.node_to_string(nid)
                    } else {
                        arena.list_to_string(&items)
                    }
                };
                Some(self.call_avm2_property(
                    abc_idx,
                    &Avm1Value::String(s),
                    prop,
                    args.to_vec(),
                    steps,
                    depth,
                ))
            }
            _ => None,
        }
    }

    pub(super) fn alloc_avm2_qname(&mut self, uri: &str, local_name: &str) -> Avm1Value {
        self.alloc_avm2_qname_opt(Some(uri), local_name)
    }

    pub(super) fn alloc_avm2_qname_opt(
        &mut self,
        uri: Option<&str>,
        local_name: &str,
    ) -> Avm1Value {
        let mut props = HashMap::new();
        let formatted = match uri {
            Some(u) => {
                props.insert("uri".to_owned(), Avm1Value::String(u.to_owned()));
                if u.is_empty() {
                    local_name.to_owned()
                } else {
                    format!("{u}::{local_name}")
                }
            }
            None => {
                props.insert("uri".to_owned(), Avm1Value::Null);
                format!("*::{local_name}")
            }
        };
        props.insert(
            "localName".to_owned(),
            Avm1Value::String(local_name.to_owned()),
        );
        props.insert(
            "__avm2_short_class__".to_owned(),
            Avm1Value::String("QName".to_owned()),
        );
        props.insert("qname_formatted".to_owned(), Avm1Value::String(formatted));
        let oid = self.avm1_objects.len();
        self.avm1_objects.push(Avm1Object {
            props,
            ..Default::default()
        });
        Avm1Value::Object(oid)
    }
}
