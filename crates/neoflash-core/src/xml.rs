//! Bounded XML 1.0 parsing for the runtime facades. This is a flat transfer tree,
//! not an authoring model or a second mutable XML object store. Fragment roots
//! are allowed; AS2/AS3 cardinality and whitespace policies belong to the host.
//!
//! Grammar/namespace checks follow XML 1.0 fifth edition and Namespaces 1.0:
//! https://www.w3.org/TR/2008/REC-xml-20081126/
//! https://www.w3.org/TR/2009/REC-xml-names-20091208/
//! DTDs and custom entities are deliberately unsupported. No external IO occurs.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub const MAX_XML_BYTES: usize = 1024 * 1024;
pub const MAX_XML_ITEMS: usize = 50_000;
pub const MAX_XML_DEPTH: usize = 128;
pub const MAX_XML_ATTRIBUTES: usize = 256;
pub const MAX_XML_NAME_BYTES: usize = 1024;
pub const MAX_XML_STRING_BYTES: usize = 4 * 1024 * 1024;
const XML_URI: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_URI: &str = "http://www.w3.org/2000/xmlns/";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct XmlName {
    pub qualified: String,
    pub local: String,
    pub prefix: String,
    pub uri: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct XmlAttribute {
    pub name: XmlName,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct XmlDeclaration {
    pub prefix: String,
    pub uri: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum XmlNodeKind {
    Element,
    Text,
    Comment,
    Pi,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct XmlNode {
    pub kind: XmlNodeKind,
    pub name: Option<XmlName>,
    pub value: String,
    pub attributes: Vec<XmlAttribute>,
    pub declarations: Vec<XmlDeclaration>,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct XmlParseTree {
    pub roots: Vec<usize>,
    pub nodes: Vec<XmlNode>,
    /// Original XML declaration, including <?xml and ?>, excluding a UTF-8 BOM.
    pub declaration: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum XmlErrorKind {
    Limit,
    Syntax,
    Namespace,
    Entity,
    Doctype,
    Encoding,
    Character,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{message} (UTF-8 byte {offset})")]
pub struct XmlParseError {
    pub kind: XmlErrorKind,
    pub offset: usize,
    pub message: String,
}

fn error(kind: XmlErrorKind, offset: usize, message: impl AsRef<str>) -> XmlParseError {
    XmlParseError {
        kind,
        offset,
        message: message.as_ref().chars().take(512).collect(),
    }
}

fn reader_error(value: quick_xml::Error, offset: usize) -> XmlParseError {
    let kind = match &value {
        quick_xml::Error::Encoding(_) => XmlErrorKind::Encoding,
        quick_xml::Error::Escape(_) => XmlErrorKind::Entity,
        quick_xml::Error::Namespace(_) => XmlErrorKind::Namespace,
        _ => XmlErrorKind::Syntax,
    };
    error(kind, offset, value.to_string())
}

fn utf8(bytes: &[u8], offset: usize) -> Result<&str, XmlParseError> {
    std::str::from_utf8(bytes)
        .map_err(|_| error(XmlErrorKind::Encoding, offset, "XML input must be UTF-8."))
}

fn xml_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

fn xml_char(c: char) -> bool {
    matches!(c as u32, 0x9 | 0xa | 0xd | 0x20..=0xd7ff | 0xe000..=0xfffd | 0x10000..=0x10ffff)
}

fn name_start(c: char) -> bool {
    matches!(c, '_' | 'A'..='Z' | 'a'..='z')
        || matches!(c as u32, 0xc0..=0xd6 | 0xd8..=0xf6 | 0xf8..=0x2ff | 0x370..=0x37d
            | 0x37f..=0x1fff | 0x200c..=0x200d | 0x2070..=0x218f | 0x2c00..=0x2fef
            | 0x3001..=0xd7ff | 0xf900..=0xfdcf | 0xfdf0..=0xfffd | 0x10000..=0xeffff)
}

fn name_rest(c: char) -> bool {
    name_start(c)
        || matches!(c, '-' | '.' | '0'..='9' | '\u{b7}')
        || matches!(c as u32, 0x300..=0x36f | 0x203f..=0x2040)
}

fn valid_ncname(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(name_start) && chars.all(name_rest)
}

fn qname(value: &str, offset: usize) -> Result<(&str, &str), XmlParseError> {
    if value.len() > MAX_XML_NAME_BYTES {
        return Err(error(
            XmlErrorKind::Limit,
            offset,
            "XML name exceeds 1024 UTF-8 bytes.",
        ));
    }
    let (prefix, local) = value.split_once(':').unwrap_or(("", value));
    if !valid_ncname(local)
        || (!prefix.is_empty() && !valid_ncname(prefix))
        || value.starts_with(':')
    {
        return Err(error(
            XmlErrorKind::Syntax,
            offset,
            "Invalid XML qualified name.",
        ));
    }
    Ok((prefix, local))
}

fn normalize_lines(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn entity(value: &str, offset: usize) -> Result<char, XmlParseError> {
    let resolved = match value {
        "lt" => '<',
        "gt" => '>',
        "amp" => '&',
        "apos" => '\'',
        "quot" => '"',
        _ => {
            let (digits, radix) = if let Some(hex) = value.strip_prefix("#x") {
                (hex, 16)
            } else if let Some(decimal) = value.strip_prefix('#') {
                (decimal, 10)
            } else {
                return Err(error(
                    XmlErrorKind::Entity,
                    offset,
                    "Custom XML entities are unsupported; use predefined or numeric references.",
                ));
            };
            if digits.is_empty()
                || !digits.bytes().all(|c| {
                    if radix == 16 {
                        c.is_ascii_hexdigit()
                    } else {
                        c.is_ascii_digit()
                    }
                })
            {
                return Err(error(
                    XmlErrorKind::Entity,
                    offset,
                    "Invalid XML character reference.",
                ));
            }
            u32::from_str_radix(digits, radix)
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| {
                    error(
                        XmlErrorKind::Character,
                        offset,
                        "XML character reference is outside the Unicode scalar range.",
                    )
                })?
        }
    };
    if !xml_char(resolved) {
        return Err(error(
            XmlErrorKind::Character,
            offset,
            "XML 1.0 forbids this character reference.",
        ));
    }
    Ok(resolved)
}

fn attribute_value(raw: &str, offset: usize) -> Result<String, XmlParseError> {
    // Normalize literal whitespace before expanding references: &#xA; must stay LF.
    let mut output = String::with_capacity(raw.len());
    let mut chars = raw.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        match c {
            '<' => {
                return Err(error(
                    XmlErrorKind::Syntax,
                    offset + index,
                    "Literal '<' is forbidden in an XML attribute value.",
                ));
            }
            '&' => {
                let start = index + 1;
                let end = raw[start..].find(';').map(|n| start + n).ok_or_else(|| {
                    error(
                        XmlErrorKind::Entity,
                        offset + index,
                        "Unterminated XML entity reference.",
                    )
                })?;
                output.push(entity(&raw[start..end], offset + index)?);
                while chars.peek().is_some_and(|(position, _)| *position <= end) {
                    chars.next();
                }
            }
            '\r' => {
                if chars.peek().is_some_and(|(_, next)| *next == '\n') {
                    chars.next();
                }
                output.push(' ');
            }
            '\t' | '\n' => output.push(' '),
            _ => output.push(c),
        }
    }
    Ok(output)
}

fn attribute_separators(raw: &[u8], offset: usize) -> Result<(), XmlParseError> {
    // quick-xml's attribute iterator intentionally also accepts adjacent quoted
    // attributes. XML requires whitespace between them, so enforce that here.
    let mut quote = None;
    if raw
        .first()
        .is_some_and(|c| !matches!(c, b' ' | b'\t' | b'\r' | b'\n'))
    {
        return Err(error(
            XmlErrorKind::Syntax,
            offset,
            "XML attributes require separating whitespace.",
        ));
    }
    for (index, byte) in raw.iter().copied().enumerate() {
        if quote == Some(byte) {
            quote = None;
            if raw
                .get(index + 1)
                .is_some_and(|c| !matches!(c, b' ' | b'\t' | b'\r' | b'\n'))
            {
                return Err(error(
                    XmlErrorKind::Syntax,
                    offset + index + 1,
                    "XML attributes require separating whitespace.",
                ));
            }
        } else if quote.is_none() && matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
        }
    }
    Ok(())
}

#[derive(Default)]
struct Parser {
    tree: XmlParseTree,
    stack: Vec<usize>,
    // Index declarations rather than cloning URI strings or scanning all
    // ancestors for every name in a deeply nested document.
    bindings: HashMap<String, Vec<(usize, usize)>>,
    items: usize,
    strings: usize,
}

impl Parser {
    fn charge_items(&mut self, count: usize, offset: usize) -> Result<(), XmlParseError> {
        if count > MAX_XML_ITEMS - self.items {
            return Err(error(
                XmlErrorKind::Limit,
                offset,
                "XML exceeds 50000 nodes, attributes and namespace declarations.",
            ));
        }
        self.items += count;
        Ok(())
    }

    fn charge_strings(&mut self, count: usize, offset: usize) -> Result<(), XmlParseError> {
        if count > MAX_XML_STRING_BYTES - self.strings {
            return Err(error(
                XmlErrorKind::Limit,
                offset,
                "Expanded XML strings exceed 4 MiB.",
            ));
        }
        self.strings += count;
        Ok(())
    }

    fn push_node(&mut self, mut node: XmlNode, offset: usize) -> Result<usize, XmlParseError> {
        self.charge_items(1, offset)?;
        node.parent = self.stack.last().copied();
        let index = self.tree.nodes.len();
        if let Some(parent) = node.parent {
            self.tree.nodes[parent].children.push(index);
        } else {
            self.tree.roots.push(index);
        }
        self.tree.nodes.push(node);
        Ok(index)
    }

    fn value_node(
        &mut self,
        kind: XmlNodeKind,
        value: String,
        name: Option<XmlName>,
        offset: usize,
    ) -> Result<(), XmlParseError> {
        self.charge_strings(value.len(), offset)?;
        // XML references and CDATA can split one text run into several events.
        // Coalesce only adjacent text under the same parent, never across markup.
        let last = self
            .stack
            .last()
            .and_then(|p| self.tree.nodes[*p].children.last())
            .or_else(|| {
                if self.stack.is_empty() {
                    self.tree.roots.last()
                } else {
                    None
                }
            })
            .copied();
        if kind == XmlNodeKind::Text
            && let Some(index) = last
            && self.tree.nodes[index].kind == XmlNodeKind::Text
        {
            self.tree.nodes[index].value.push_str(&value);
            return Ok(());
        }
        self.push_node(
            XmlNode {
                kind,
                name,
                value,
                attributes: Vec::new(),
                declarations: Vec::new(),
                parent: None,
                children: Vec::new(),
            },
            offset,
        )?;
        Ok(())
    }

    fn lookup<'a>(&'a self, prefix: &str, local: &'a [XmlDeclaration]) -> Option<&'a str> {
        if prefix == "xml" {
            return Some(XML_URI);
        }
        local
            .iter()
            .find(|d| d.prefix == prefix)
            .map(|d| d.uri.as_str())
            .or_else(|| {
                self.bindings
                    .get(prefix)
                    .and_then(|entries| entries.last())
                    .map(|(node, declaration)| {
                        self.tree.nodes[*node].declarations[*declaration]
                            .uri
                            .as_str()
                    })
            })
    }

    fn expanded_name(
        &mut self,
        qualified: &str,
        declarations: &[XmlDeclaration],
        attribute: bool,
        offset: usize,
    ) -> Result<XmlName, XmlParseError> {
        let (prefix, local) = qname(qualified, offset)?;
        if prefix == "xmlns" {
            return Err(error(
                XmlErrorKind::Namespace,
                offset,
                "The xmlns prefix is reserved for namespace declarations.",
            ));
        }
        let uri = if prefix.is_empty() && attribute {
            ""
        } else {
            match self.lookup(prefix, declarations) {
                Some(uri) => uri,
                None if prefix.is_empty() => "",
                None => {
                    return Err(error(
                        XmlErrorKind::Namespace,
                        offset,
                        "XML name uses an undeclared namespace prefix.",
                    ));
                }
            }
        };
        let bytes = qualified.len() + prefix.len() + local.len() + uri.len();
        // Charge before copying a possibly large inherited namespace URI.
        if bytes > MAX_XML_STRING_BYTES - self.strings {
            return Err(error(
                XmlErrorKind::Limit,
                offset,
                "Expanded XML strings exceed 4 MiB.",
            ));
        }
        let name = XmlName {
            qualified: qualified.into(),
            local: local.into(),
            prefix: prefix.into(),
            uri: uri.into(),
        };
        self.strings += bytes;
        Ok(name)
    }

    fn element(
        &mut self,
        event: &BytesStart<'_>,
        empty: bool,
        offset: usize,
    ) -> Result<(), XmlParseError> {
        if self.stack.len() >= MAX_XML_DEPTH {
            return Err(error(
                XmlErrorKind::Limit,
                offset,
                "XML element nesting exceeds 128 levels.",
            ));
        }
        let binding = event.name();
        let qualified = utf8(binding.as_ref(), offset)?;
        qname(qualified, offset)?;
        attribute_separators(event.attributes_raw(), offset + 1 + qualified.len())?;
        let mut declarations = Vec::new();
        let mut attributes = Vec::new();
        let mut seen = HashSet::new();
        for (count, attribute) in event.attributes().with_checks(false).enumerate() {
            if count >= MAX_XML_ATTRIBUTES {
                return Err(error(
                    XmlErrorKind::Limit,
                    offset,
                    "An XML element exceeds 256 attributes and namespace declarations.",
                ));
            }
            self.charge_items(1, offset)?;
            let attribute =
                attribute.map_err(|e| error(XmlErrorKind::Syntax, offset, e.to_string()))?;
            let key = utf8(attribute.key.as_ref(), offset)?;
            qname(key, offset)?;
            if !seen.insert(key.to_owned()) {
                return Err(error(
                    XmlErrorKind::Syntax,
                    offset,
                    "Duplicate XML attribute name.",
                ));
            }
            let value = attribute_value(utf8(&attribute.value, offset)?, offset)?;
            self.charge_strings(value.len(), offset)?;
            if key == "xmlns" || key.starts_with("xmlns:") {
                let prefix = key.strip_prefix("xmlns:").unwrap_or("");
                if prefix == "xmlns"
                    || value == XMLNS_URI
                    || (prefix == "xml") != (value == XML_URI)
                    || (!prefix.is_empty() && value.is_empty())
                {
                    return Err(error(
                        XmlErrorKind::Namespace,
                        offset,
                        "Invalid reserved namespace binding or empty prefixed namespace.",
                    ));
                }
                self.charge_strings(prefix.len(), offset)?;
                declarations.push(XmlDeclaration {
                    prefix: prefix.into(),
                    uri: value,
                });
            } else {
                attributes.push((key.to_owned(), value));
            }
        }
        let name = self.expanded_name(qualified, &declarations, false, offset)?;
        let attributes: Vec<XmlAttribute> = attributes
            .into_iter()
            .map(|(key, value)| {
                Ok(XmlAttribute {
                    name: self.expanded_name(&key, &declarations, true, offset)?,
                    value,
                })
            })
            .collect::<Result<_, XmlParseError>>()?;
        let mut expanded = HashSet::new();
        for attribute in &attributes {
            if !expanded.insert((&attribute.name.uri, &attribute.name.local)) {
                return Err(error(
                    XmlErrorKind::Namespace,
                    offset,
                    "Duplicate XML attributes have the same expanded name.",
                ));
            }
        }
        let index = self.push_node(
            XmlNode {
                kind: XmlNodeKind::Element,
                name: Some(name),
                value: String::new(),
                attributes,
                declarations,
                parent: None,
                children: Vec::new(),
            },
            offset,
        )?;
        if !empty {
            for (declaration, value) in self.tree.nodes[index].declarations.iter().enumerate() {
                self.bindings
                    .entry(value.prefix.clone())
                    .or_default()
                    .push((index, declaration));
            }
            self.stack.push(index);
        }
        Ok(())
    }
}

fn declaration(raw: &str, offset: usize) -> Result<String, XmlParseError> {
    // BytesDecl accessors intentionally tolerate extra/duplicate attributes.
    // Validate the whole ordered declaration through the strict attribute path.
    let event = BytesStart::from_content(raw, 3);
    attribute_separators(event.attributes_raw(), offset + 5)?;
    let mut step = 0;
    for attribute in event.attributes() {
        let attribute =
            attribute.map_err(|e| error(XmlErrorKind::Syntax, offset, e.to_string()))?;
        let key = utf8(attribute.key.as_ref(), offset)?;
        let value = utf8(&attribute.value, offset)?;
        match (key, step) {
            ("version", 0) if value == "1.0" => step = 1,
            ("encoding", 1) => {
                if !value.eq_ignore_ascii_case("UTF-8") {
                    return Err(error(
                        XmlErrorKind::Encoding,
                        offset,
                        "Only UTF-8 XML declarations are supported.",
                    ));
                }
                step = 2;
            }
            ("standalone", 1 | 2) if matches!(value, "yes" | "no") => step = 3,
            _ => {
                return Err(error(
                    XmlErrorKind::Syntax,
                    offset,
                    "Expected XML 1.0 declaration with ordered version, optional UTF-8 encoding and optional standalone=yes/no.",
                ));
            }
        }
    }
    if step == 0 {
        return Err(error(
            XmlErrorKind::Syntax,
            offset,
            "XML declaration requires version='1.0'.",
        ));
    }
    Ok(format!("<?{raw}?>"))
}

/// Parse XML 1.0 fragments without applying AS2/AS3 ignore or cardinality rules.
/// Errors never expose a partial tree. Offsets are UTF-8 bytes in the input.
pub fn parse_xml(source: &str) -> Result<XmlParseTree, XmlParseError> {
    if source.len() > MAX_XML_BYTES {
        return Err(error(
            XmlErrorKind::Limit,
            0,
            "XML source exceeds 1 MiB UTF-8.",
        ));
    }
    if let Some((offset, _)) = source.char_indices().find(|(_, c)| !xml_char(*c)) {
        return Err(error(
            XmlErrorKind::Character,
            offset,
            "XML 1.0 forbids this input character.",
        ));
    }
    let body = source.strip_prefix('\u{feff}').unwrap_or(source);
    let base = source.len() - body.len();
    let mut reader = Reader::from_str(body);
    reader.config_mut().enable_all_checks(true);
    let mut parser = Parser::default();
    let mut first = true;
    loop {
        let offset = base + reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|e| {
            reader_error(e, base + (reader.error_position() as usize).min(body.len()))
        })?;
        match event {
            Event::Start(event) => parser.element(&event, false, offset)?,
            Event::Empty(event) => parser.element(&event, true, offset)?,
            Event::End(_) => {
                let index = parser.stack.pop().ok_or_else(|| {
                    error(XmlErrorKind::Syntax, offset, "Unexpected XML closing tag.")
                })?;
                for declaration in &parser.tree.nodes[index].declarations {
                    let entries = parser
                        .bindings
                        .get_mut(&declaration.prefix)
                        .expect("open element namespace binding");
                    entries.pop();
                    if entries.is_empty() {
                        parser.bindings.remove(&declaration.prefix);
                    }
                }
            }
            Event::Text(event) => {
                let value = utf8(&event, offset)?;
                if value.contains("]]>") {
                    return Err(error(
                        XmlErrorKind::Syntax,
                        offset,
                        "Literal ']]>' is forbidden outside CDATA.",
                    ));
                }
                parser.value_node(XmlNodeKind::Text, normalize_lines(value), None, offset)?;
            }
            Event::CData(event) => parser.value_node(
                XmlNodeKind::Text,
                normalize_lines(utf8(&event, offset)?),
                None,
                offset,
            )?,
            Event::GeneralRef(event) => {
                let value = entity(utf8(&event, offset)?, offset)?.to_string();
                parser.value_node(XmlNodeKind::Text, value, None, offset)?;
            }
            Event::Comment(event) => parser.value_node(
                XmlNodeKind::Comment,
                normalize_lines(utf8(&event, offset)?),
                None,
                offset,
            )?,
            Event::PI(event) => {
                let target = utf8(event.target(), offset)?;
                if target.len() > MAX_XML_NAME_BYTES {
                    return Err(error(
                        XmlErrorKind::Limit,
                        offset,
                        "XML PI target exceeds 1024 UTF-8 bytes.",
                    ));
                }
                let mut chars = target.chars();
                if !chars.next().is_some_and(|c| name_start(c) || c == ':')
                    || !chars.all(|c| name_rest(c) || c == ':')
                    || target.eq_ignore_ascii_case("xml")
                {
                    return Err(error(
                        XmlErrorKind::Syntax,
                        offset,
                        "Invalid or reserved XML processing-instruction target.",
                    ));
                }
                parser.charge_strings(target.len() * 2, offset)?;
                let name = XmlName {
                    qualified: target.into(),
                    local: target.into(),
                    prefix: String::new(),
                    uri: String::new(),
                };
                let value =
                    normalize_lines(utf8(event.content(), offset)?.trim_start_matches(xml_space));
                parser.value_node(XmlNodeKind::Pi, value, Some(name), offset)?;
            }
            Event::Decl(event) => {
                if !first {
                    return Err(error(
                        XmlErrorKind::Syntax,
                        offset,
                        "The XML declaration must occur once at the beginning.",
                    ));
                }
                let value = declaration(utf8(&event, offset)?, offset)?;
                parser.charge_strings(value.len(), offset)?;
                parser.tree.declaration = Some(value);
            }
            Event::DocType(_) => {
                return Err(error(
                    XmlErrorKind::Doctype,
                    offset,
                    "DOCTYPE declarations, DTDs and external entities are unsupported.",
                ));
            }
            Event::Eof => {
                if !parser.stack.is_empty() {
                    return Err(error(
                        XmlErrorKind::Syntax,
                        offset,
                        "XML ended before all elements were closed.",
                    ));
                }
                return Ok(parser.tree);
            }
        }
        first = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_xml_fragment_family() {
        let source = "\u{feff}<?xml version='1.0' encoding='utf-8' standalone='yes'?><catalog xmlns='urn:catalog' xmlns:p='urn:item' xml:lang='en' id='a\r\nb&#xA;c'><!--note--><?show mode='card'?><p:item p:id='1' plain='yes'>Tea &amp; <![CDATA[coffee]]>&#x1F375;</p:item><empty xmlns=''/></catalog><tail/>";
        let tree = parse_xml(source).unwrap();
        assert_eq!(tree.roots, vec![0, 6]);
        assert_eq!(tree.nodes[0].name.as_ref().unwrap().uri, "urn:catalog");
        assert_eq!(tree.nodes[0].attributes[0].name.uri, XML_URI);
        assert_eq!(tree.nodes[0].attributes[1].name.uri, "");
        assert_eq!(tree.nodes[0].attributes[1].value, "a b\nc");
        assert_eq!(tree.nodes[1].kind, XmlNodeKind::Comment);
        assert_eq!(tree.nodes[2].value, "mode='card'");
        assert_eq!(tree.nodes[3].name.as_ref().unwrap().qualified, "p:item");
        assert_eq!(tree.nodes[3].attributes[0].name.uri, "urn:item");
        assert_eq!(tree.nodes[4].value, "Tea & coffee🍵");
        assert_eq!(tree.nodes[5].name.as_ref().unwrap().uri, "");
        assert!(tree.declaration.as_ref().unwrap().starts_with("<?xml"));
        for (index, node) in tree.nodes.iter().enumerate() {
            for child in &node.children {
                assert!(*child > index);
                assert_eq!(tree.nodes[*child].parent, Some(index));
            }
        }
        assert_eq!(
            parse_xml("plain &#65;\r\ntext").unwrap().nodes[0].value,
            "plain A\ntext"
        );
        assert!(parse_xml("").unwrap().nodes.is_empty());
        assert_eq!(
            parse_xml("<é:名 xmlns:é='urn:unicode'/>").unwrap().nodes[0]
                .name
                .as_ref()
                .unwrap()
                .local,
            "名"
        );

        for (source, kind) in [
            ("<a><b></a>", XmlErrorKind::Syntax),
            ("<a>", XmlErrorKind::Syntax),
            ("<a x='1'x='2'/>", XmlErrorKind::Syntax),
            ("<a x='1' x='2'/>", XmlErrorKind::Syntax),
            ("<a x='<bad'/>", XmlErrorKind::Syntax),
            ("<1bad/>", XmlErrorKind::Syntax),
            ("<a:b:c/>", XmlErrorKind::Syntax),
            ("<a>]]></a>", XmlErrorKind::Syntax),
            ("<!--bad--comment-->", XmlErrorKind::Syntax),
            (" <?xml version='1.0'?>", XmlErrorKind::Syntax),
            ("<?xml version='1.0' extra='x'?>", XmlErrorKind::Syntax),
            (
                "<a xmlns:p='u' xmlns:q='u' p:x='1' q:x='2'/>",
                XmlErrorKind::Namespace,
            ),
            ("<p:a/>", XmlErrorKind::Namespace),
            ("<a xmlns:p='u'/><p:b/>", XmlErrorKind::Namespace),
            ("<a xmlns:xml='bad'/>", XmlErrorKind::Namespace),
            ("<a xmlns:p=''/>", XmlErrorKind::Namespace),
            ("<a>&custom;</a>", XmlErrorKind::Entity),
            ("<a x='&custom;'/>", XmlErrorKind::Entity),
            ("<a>&#x0;</a>", XmlErrorKind::Character),
            ("<a x='&#x1;'/>", XmlErrorKind::Character),
            ("<a>\u{0}</a>", XmlErrorKind::Character),
            ("<!DOCTYPE a><a/>", XmlErrorKind::Doctype),
            (
                "<?xml version='1.0' encoding='UTF-16'?><a/>",
                XmlErrorKind::Encoding,
            ),
        ] {
            let failure = parse_xml(source).unwrap_err();
            assert_eq!(failure.kind, kind, "{source}: {failure}");
            assert!(failure.offset <= source.len());
        }
        let nested = format!(
            "{}{}",
            "<a>".repeat(MAX_XML_DEPTH),
            "</a>".repeat(MAX_XML_DEPTH)
        );
        assert!(parse_xml(&nested).is_ok());
        let deep = format!("<a>{nested}</a>");
        let attrs = (0..=MAX_XML_ATTRIBUTES)
            .map(|n| format!(" a{n}='x'"))
            .collect::<String>();
        let expanded = format!(
            "<a xmlns='{}'>{}</a>",
            "u".repeat(64 * 1024),
            "<b/>".repeat(65)
        );
        for source in [
            "x".repeat(MAX_XML_BYTES + 1),
            "<a/>".repeat(MAX_XML_ITEMS + 1),
            deep,
            format!("<a{attrs}/>"),
            expanded,
        ] {
            assert_eq!(parse_xml(&source).unwrap_err().kind, XmlErrorKind::Limit);
        }
        // Error serialization contains no partially accepted tree.
        let encoded = serde_json::to_string(&parse_xml("<a>").unwrap_err()).unwrap();
        assert!(encoded.contains("\"kind\":\"syntax\""));
        assert!(!encoded.contains("nodes"));
    }
}
