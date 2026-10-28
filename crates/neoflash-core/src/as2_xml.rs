//! AS2 document metadata over the shared bounded XML parser.
//!
//! Adobe's XML contract and Ruffle's MIT/Apache-2.0 AVM1 XML implementation
//! (07d3e430631333f80f754df01a968bdcd80d98c5) informed the declaration/idMap
//! behavior. No Ruffle implementation is copied. DTD text is retained, never
//! interpreted or fetched; the shared parser still rejects custom entities.

use crate::xml::{MAX_XML_BYTES, XmlErrorKind, XmlParseError, XmlParseTree, parse_xml};
use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize)]
pub struct As2XmlTree {
    #[serde(flatten)]
    pub tree: XmlParseTree,
    pub doctype: Option<String>,
    /// Parsed node indices: the last element with a duplicate id wins.
    pub id_map: BTreeMap<String, usize>,
}

fn failure(kind: XmlErrorKind, offset: usize, message: &str) -> XmlParseError {
    XmlParseError {
        kind,
        offset,
        message: message.chars().take(512).collect(),
    }
}

pub fn parse_as2_xml(source: &str) -> Result<As2XmlTree, XmlParseError> {
    if source.len() > MAX_XML_BYTES {
        return Err(failure(
            XmlErrorKind::Limit,
            0,
            "XML source exceeds 1 MiB UTF-8.",
        ));
    }
    let mut reader = Reader::from_str(source);
    reader.config_mut().enable_all_checks(true);
    let mut declaration_range = None;
    let mut content_started = false;
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|e| {
            failure(
                XmlErrorKind::Syntax,
                reader.error_position() as usize,
                &e.to_string(),
            )
        })?;
        match event {
            Event::DocType(_) => {
                if content_started || declaration_range.is_some() {
                    return Err(failure(
                        XmlErrorKind::Doctype,
                        start,
                        "AS2 XML accepts one DOCTYPE before the document elements.",
                    ));
                }
                declaration_range = Some(start..reader.buffer_position() as usize);
            }
            Event::Decl(_) if declaration_range.is_some() => {
                return Err(failure(
                    XmlErrorKind::Syntax,
                    start,
                    "The XML declaration must precede the DOCTYPE.",
                ));
            }
            Event::Start(_) | Event::Empty(_) | Event::CData(_) | Event::GeneralRef(_) => {
                content_started = true
            }
            Event::Text(text) if text.iter().any(|b| !b.is_ascii_whitespace()) => {
                content_started = true
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let mut stripped = String::new();
    let (input, doctype) = if let Some(range) = &declaration_range {
        stripped.push_str(&source[..range.start]);
        stripped.push_str(&source[range.end..]);
        (stripped.as_str(), Some(source[range.clone()].to_owned()))
    } else {
        (source, None)
    };
    let tree = parse_xml(input).map_err(|mut error| {
        if let Some(range) = &declaration_range
            && error.offset >= range.start
        {
            error.offset += range.len();
        }
        error
    })?;
    let mut id_map = BTreeMap::new();
    for (index, node) in tree.nodes.iter().enumerate() {
        if let Some(attribute) = node.attributes.iter().find(|a| a.name.qualified == "id") {
            id_map.insert(attribute.value.clone(), index);
        }
    }
    Ok(As2XmlTree {
        tree,
        doctype,
        id_map,
    })
}

#[derive(Deserialize)]
struct Binding {
    prefix: String,
    uri: String,
}

/// The host marshals ancestry nearest-first; Rust owns prefix shadowing and
/// reverse lookup. Moving an XMLNode therefore uses its current parent chain.
pub fn namespace_lookup_json(
    ancestors_json: &str,
    value: &str,
    reverse: bool,
) -> Result<String, String> {
    if ancestors_json.len() > MAX_XML_BYTES || value.len() > MAX_XML_BYTES {
        return Err("AS2 namespace lookup input exceeds 1 MiB.".into());
    }
    let ancestry: Vec<Vec<Binding>> =
        serde_json::from_str(ancestors_json).map_err(|e| e.to_string())?;
    if ancestry.len() > 129 || ancestry.iter().any(|scope| scope.len() > 256) {
        return Err("AS2 namespace lookup exceeds the XML depth/declaration budget.".into());
    }
    let mut seen = BTreeSet::new();
    let mut result = None;
    for scope in ancestry {
        for binding in scope {
            if seen.insert(binding.prefix.clone())
                && if reverse {
                    binding.uri == value
                } else {
                    binding.prefix == value
                }
            {
                result = Some(if reverse { binding.prefix } else { binding.uri });
                break;
            }
        }
        if result.is_some() {
            break;
        }
    }
    if result.is_none() && !seen.contains("xml") {
        let xml_uri = "http://www.w3.org/XML/1998/namespace";
        if (reverse && value == xml_uri) || (!reverse && value == "xml") {
            result = Some(if reverse {
                "xml".to_owned()
            } else {
                xml_uri.to_owned()
            });
        }
    }
    serde_json::to_string(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as2_xml_document_metadata_batch() {
        let source = "<?xml version='1.0'?><!DOCTYPE menu SYSTEM 'never-fetch.dtd'><menu id='root'><item id='same'/><item id='same'>A &amp; B</item></menu>";
        let parsed = parse_as2_xml(source).unwrap();
        assert_eq!(
            parsed.doctype.as_deref(),
            Some("<!DOCTYPE menu SYSTEM 'never-fetch.dtd'>")
        );
        assert_eq!(
            parsed.tree.declaration.as_deref(),
            Some("<?xml version='1.0'?>")
        );
        let last = &parsed.tree.nodes[parsed.id_map["same"]];
        assert_eq!(parsed.tree.nodes[last.children[0]].value, "A & B");
        assert_eq!(parsed.id_map.len(), 2);
        assert!(parse_as2_xml("<!DOCTYPE a [<!ELEMENT a ANY>]><a/>").is_ok());
        assert_eq!(
            parse_as2_xml("<!DOCTYPE a [<!ENTITY x 'value'>]><a>&x;</a>")
                .unwrap_err()
                .kind,
            XmlErrorKind::Entity
        );
        assert_eq!(
            parse_as2_xml("<a/><!DOCTYPE a>").unwrap_err().kind,
            XmlErrorKind::Doctype
        );
        assert_eq!(
            parse_as2_xml("<!DOCTYPE a><!DOCTYPE a><a/>")
                .unwrap_err()
                .kind,
            XmlErrorKind::Doctype
        );
        assert_eq!(
            parse_xml(source).unwrap_err().kind,
            XmlErrorKind::Doctype,
            "AS3 remains on the original parser profile"
        );
        let scopes = r#"[[{"prefix":"p","uri":"urn:inner"}],[{"prefix":"p","uri":"urn:outer"},{"prefix":"q","uri":"urn:other"}]]"#;
        assert_eq!(
            namespace_lookup_json(scopes, "p", false).unwrap(),
            "\"urn:inner\""
        );
        assert_eq!(
            namespace_lookup_json(scopes, "urn:outer", true).unwrap(),
            "null"
        );
        assert_eq!(
            namespace_lookup_json(scopes, "urn:other", true).unwrap(),
            "\"q\""
        );
        assert_eq!(
            namespace_lookup_json("[]", "xml", false).unwrap(),
            "\"http://www.w3.org/XML/1998/namespace\""
        );
    }
}
