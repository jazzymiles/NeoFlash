//! Parser-selected XML literal lexical context (ECMA-357, sections 8.3/11.1.4/5).
//! https://ecma-international.org/wp-content/uploads/ECMA-357_2nd_edition_december_2005.pdf
//!
//! This scanner finds static XML boundaries and interpolation sites. It does not
//! lex ActionScript expressions or validate XML entities/namespaces: the caller
//! parses each expression normally, and the existing Rust XML parser validates
//! the host builder's static skeleton. It is invoked only from primary-expression
//! context, never by guessing whether an ordinary '<' or shift token means XML.

use crate::Diagnostic;
use neoflash_core::{
    MAX_XML_ATTRIBUTES, MAX_XML_BYTES, MAX_XML_DEPTH, MAX_XML_ITEMS, MAX_XML_NAME_BYTES, Script,
};
use serde::Serialize;
use std::ops::Range;

pub(super) const MAX_XML_LITERAL_HOLES: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum HoleContext {
    Child,
    Attribute,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub(super) enum Part {
    Text { text: String },
    Hole { hole: usize, context: HoleContext },
}

/// Serialized builder IR is exactly `{list,parts}`. Source bookkeeping is not
/// sent to the host. Hole expressions are emitted as ordered thunks, so the host
/// can evaluate and convert one value before evaluating the next expression.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct XmlLiteral {
    #[serde(skip)]
    pub end: usize,
    pub list: bool,
    pub parts: Vec<Part>,
    #[serde(skip)]
    pub holes: Vec<Range<usize>>,
}

struct Scanner<'a, F> {
    script: &'a Script,
    parse_hole: F,
    start: usize,
    position: usize,
    static_start: usize,
    list: bool,
    stack: Vec<&'a str>,
    parts: Vec<Part>,
    holes: Vec<Range<usize>>,
    items: usize,
}

impl<'a, F: FnMut(usize) -> Result<usize, Diagnostic>> Scanner<'a, F> {
    fn rest(&self) -> &'a str {
        &self.script.source[self.position..]
    }

    fn error(&self, offset: usize, message: impl Into<String>) -> Diagnostic {
        Diagnostic::at(self.script, offset, message)
    }

    fn bounds(&self) -> Result<(), Diagnostic> {
        if self.position - self.start > MAX_XML_BYTES {
            return Err(self.error(self.start, "XML literal exceeds 1 MiB of UTF-8 source."));
        }
        Ok(())
    }

    fn advance(&mut self, bytes: usize) -> Result<(), Diagnostic> {
        self.position += bytes;
        self.bounds()
    }

    fn item(&mut self) -> Result<(), Diagnostic> {
        self.items += 1;
        if self.items > MAX_XML_ITEMS {
            return Err(self.error(
                self.position,
                "XML literal exceeds 50000 static nodes/attributes.",
            ));
        }
        Ok(())
    }

    fn whitespace(&mut self) -> Result<bool, Diagnostic> {
        let bytes = self.rest().bytes().take_while(|c| xml_space(*c)).count();
        self.advance(bytes)?;
        Ok(bytes != 0)
    }

    fn static_name(&mut self, description: &str) -> Result<&'a str, Diagnostic> {
        let start = self.position;
        let length = self
            .rest()
            .find(|c: char| {
                matches!(
                    c,
                    ' ' | '\t' | '\r' | '\n' | '/' | '>' | '=' | '{' | '}' | '<' | '\'' | '"'
                )
            })
            .unwrap_or(self.rest().len());
        if length == 0 {
            return Err(self.error(start, format!("Expected a static XML {description}; dynamic tag/attribute names are not supported.")));
        }
        if length > MAX_XML_NAME_BYTES {
            return Err(self.error(start, "XML literal names are limited to 1024 UTF-8 bytes."));
        }
        self.advance(length)?;
        Ok(&self.script.source[start..self.position])
    }

    fn flush(&mut self, end: usize) {
        if end > self.static_start {
            self.parts.push(Part::Text {
                text: self.script.source[self.static_start..end].into(),
            });
        }
    }

    fn hole(&mut self, context: HoleContext) -> Result<(), Diagnostic> {
        let open = self.position;
        if self.holes.len() >= MAX_XML_LITERAL_HOLES {
            return Err(self.error(open, "XML literal exceeds 1024 interpolated expressions."));
        }
        self.flush(open);
        let expression_start = open + 1;
        let end = (self.parse_hole)(expression_start)?;
        if end <= expression_start
            || end > self.script.source.len()
            || !self.script.source.is_char_boundary(end)
            || self.script.source.as_bytes()[end - 1] != b'}'
        {
            return Err(self.error(
                open,
                "XML expression parser did not consume its closing '}'.",
            ));
        }
        let hole = self.holes.len();
        self.holes.push(expression_start..end - 1);
        self.parts.push(Part::Hole { hole, context });
        self.position = end;
        self.static_start = end;
        self.bounds()
    }

    fn open_element(&mut self) -> Result<bool, Diagnostic> {
        self.item()?;
        if self.stack.len() >= MAX_XML_DEPTH {
            return Err(self.error(self.position, "XML literal nesting exceeds 128 elements."));
        }
        self.advance(1)?; // <
        let name = self.static_name("element name")?;
        let mut attributes = 0;
        loop {
            let separated = self.whitespace()?;
            if self.rest().starts_with("/>") {
                self.advance(2)?;
                return Ok(true);
            }
            if self.rest().starts_with('>') {
                self.advance(1)?;
                self.stack.push(name);
                return Ok(false);
            }
            if self.rest().starts_with('{') {
                return Err(self.error(
                    self.position,
                    "Dynamic XML attribute names and attribute fragments are not supported.",
                ));
            }
            if !separated {
                return Err(self.error(
                    self.position,
                    "Expected whitespace between XML attributes, '>' or '/>'.",
                ));
            }
            attributes += 1;
            if attributes > MAX_XML_ATTRIBUTES {
                return Err(self.error(
                    self.position,
                    "An XML literal element exceeds 256 attributes/declarations.",
                ));
            }
            self.item()?;
            let attribute = self.static_name("attribute name")?;
            self.whitespace()?;
            if !self.rest().starts_with('=') {
                return Err(self.error(self.position, "Expected '=' after the XML attribute name."));
            }
            self.advance(1)?;
            self.whitespace()?;
            if self.rest().starts_with('{') {
                if attribute == "xmlns" || attribute.starts_with("xmlns:") {
                    return Err(self.error(self.position, "Interpolated XML namespace declarations are not supported; use a static namespace URI."));
                }
                self.hole(HoleContext::Attribute)?;
            } else if matches!(self.rest().as_bytes().first(), Some(b'\'' | b'"')) {
                let quote = self.rest().as_bytes()[0];
                self.advance(1)?;
                let Some(end) = self.rest().bytes().position(|c| c == quote) else {
                    return Err(
                        self.error(self.position, "Unterminated quoted XML attribute value.")
                    );
                };
                // ECMA XMLAttributeValue is a lexical unit. Braces and backslashes
                // inside it are ordinary XML characters, not AS substitutions.
                self.advance(end + 1)?;
            } else {
                return Err(self.error(
                    self.position,
                    "XML attribute values require quotes or a complete {expression}.",
                ));
            }
        }
    }

    fn close_element(&mut self) -> Result<(), Diagnostic> {
        let start = self.position;
        self.advance(2)?; // </
        let name = self.static_name("closing element name")?;
        self.whitespace()?;
        if !self.rest().starts_with('>') {
            return Err(self.error(
                self.position,
                "Expected '>' after the XML closing element name.",
            ));
        }
        self.advance(1)?;
        if self.stack.pop() != Some(name) {
            return Err(self.error(
                start,
                "XML literal closing tag does not match its static opening tag.",
            ));
        }
        Ok(())
    }

    fn markup(&mut self, open: &str, close: &str, description: &str) -> Result<(), Diagnostic> {
        self.item()?;
        let start = self.position;
        self.advance(open.len())?;
        let Some(end) = self.rest().find(close) else {
            return Err(self.error(start, format!("Unterminated XML {description}.")));
        };
        // These ECMA lexical units do not contain expression sites; braces are
        // preserved verbatim, including in CDATA, comments and PI data.
        self.advance(end + close.len())
    }

    fn complete(mut self, static_end: usize) -> XmlLiteral {
        self.flush(static_end);
        XmlLiteral {
            end: self.position,
            list: self.list,
            parts: self.parts,
            holes: self.holes,
        }
    }

    fn run(mut self) -> Result<XmlLiteral, Diagnostic> {
        loop {
            if self.rest().is_empty() {
                return Err(self.error(self.start, "Unterminated XML literal."));
            }
            if self.rest().starts_with("</>") {
                if !self.list || !self.stack.is_empty() {
                    return Err(self.error(
                        self.position,
                        "XMLList closing marker does not close an XML element.",
                    ));
                }
                let end = self.position;
                self.advance(3)?;
                return Ok(self.complete(end));
            }
            if self.rest().starts_with("</") {
                self.close_element()?;
                if self.stack.is_empty() && !self.list {
                    let end = self.position;
                    return Ok(self.complete(end));
                }
            } else if self.rest().starts_with("<!--") {
                self.markup("<!--", "-->", "comment")?;
                if self.stack.is_empty() && !self.list {
                    let end = self.position;
                    return Ok(self.complete(end));
                }
            } else if self.rest().starts_with("<![CDATA[") {
                self.markup("<![CDATA[", "]]>", "CDATA section")?;
                if self.stack.is_empty() && !self.list {
                    let end = self.position;
                    return Ok(self.complete(end));
                }
            } else if self.rest().starts_with("<?") {
                self.markup("<?", "?>", "processing instruction")?;
                if self.stack.is_empty() && !self.list {
                    let end = self.position;
                    return Ok(self.complete(end));
                }
            } else if self.rest().starts_with("<!") {
                return Err(self.error(
                    self.position,
                    "DTD/DOCTYPE and other XML declarations are unsupported in XML literals.",
                ));
            } else if self.rest().starts_with("<>") {
                return Err(self.error(
                    self.position,
                    "Nested XMLList literals require an interpolated {expression}.",
                ));
            } else if self.rest().starts_with('<') {
                let empty = self.open_element()?;
                if empty && self.stack.is_empty() && !self.list {
                    let end = self.position;
                    return Ok(self.complete(end));
                }
            } else if self.rest().starts_with('{') {
                self.hole(HoleContext::Child)?;
            } else {
                let length = self.rest().find(['<', '{']).unwrap_or(self.rest().len());
                self.advance(length)?;
            }
        }
    }
}

fn xml_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}

/// `start` points at '<' in a primary expression. For each hole the caller is
/// given the first byte after '{'; it parses with its ordinary expression parser
/// and returns the byte after the consumed closing '}'. This permits nested XML
/// literals and preserves source diagnostics/function scope without a brace lexer.
pub(super) fn scan(
    script: &Script,
    start: usize,
    parse_hole: impl FnMut(usize) -> Result<usize, Diagnostic>,
) -> Result<XmlLiteral, Diagnostic> {
    let Some(rest) = script.source.get(start..) else {
        return Err(Diagnostic::at(
            script,
            start,
            "Invalid XML literal source offset.",
        ));
    };
    if !rest.starts_with('<') {
        return Err(Diagnostic::at(
            script,
            start,
            "Expected '<' at the XML literal source offset.",
        ));
    }
    let list = rest.starts_with("<>");
    let position = start + if list { 2 } else { 0 };
    Scanner {
        script,
        parse_hole,
        start,
        position,
        static_start: position,
        list,
        stack: Vec::new(),
        parts: Vec::new(),
        holes: Vec::new(),
        items: 0,
    }
    .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{ScriptLanguage, parse_xml};

    #[test]
    fn literal_scanner_preserves_context_and_delegates_expressions() {
        let first = "label + \":\" + next()";
        let second = "({a: \"}\"}).a + /* } */ nested(<leaf/>)";
        let source = format!(
            "var x = <catalog xmlns:p=\"urn:item\" fixed=\"{{static}}\"><p:item title={{{first}}}>{{{second}}}<![CDATA[{{raw}}<>&]]><!--{{literal}}--><?mode {{literal}}?></p:item></catalog>; tail();"
        );
        let script = Script {
            language: ScriptLanguage::ActionScript3,
            source,
        };
        let mut called = Vec::new();
        let literal = scan(&script, 8, |start| {
            called.push(start);
            let expression = if called.len() == 1 { first } else { second };
            assert!(script.source[start..].starts_with(expression));
            Ok(start + expression.len() + 1)
        })
        .unwrap();
        assert_eq!(called.len(), 2);
        assert!(!literal.list);
        assert_eq!(&script.source[literal.end..], "; tail();");
        assert_eq!(&script.source[literal.holes[0].clone()], first);
        assert_eq!(&script.source[literal.holes[1].clone()], second);
        let contexts: Vec<_> = literal
            .parts
            .iter()
            .filter_map(|part| {
                if let Part::Hole { context, .. } = part {
                    Some(*context)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(contexts, [HoleContext::Attribute, HoleContext::Child]);
        let skeleton: String = literal
            .parts
            .iter()
            .map(|part| match part {
                Part::Text { text } => text.as_str(),
                Part::Hole {
                    context: HoleContext::Attribute,
                    ..
                } => "\"placeholder\"",
                Part::Hole {
                    context: HoleContext::Child,
                    ..
                } => "<placeholder/>",
            })
            .collect();
        assert!(parse_xml(&skeleton).is_ok());
        assert!(skeleton.contains("<![CDATA[{raw}<>&]]><!--{literal}--><?mode {literal}?>"));
        let ir = serde_json::to_value(&literal).unwrap();
        assert_eq!(ir.as_object().unwrap().len(), 2);
        assert_eq!(ir["parts"][1]["context"], "attribute");
        assert_eq!(ir["parts"][3]["hole"], 1);

        for source in [
            "<><one/><two/></>;",
            "<></>;",
            "<!--{plain}-->;",
            "<?mode {plain}?>;",
            "<![CDATA[{plain}]]>;",
            "<é:名 xmlns:é='urn:test'/>;",
        ] {
            let script = Script {
                language: ScriptLanguage::ActionScript3,
                source: source.into(),
            };
            let literal =
                scan(&script, 0, |_| panic!("static markup is not an expression")).unwrap();
            assert_eq!(&source[literal.end..], ";");
            if literal.list {
                assert!(!literal.parts.iter().any(
                    |p| matches!(p,Part::Text {text} if text.contains("<>") || text.contains("</>"))
                ));
            }
        }
        for source in [
            "<{name}/>",
            "<a {name}='x'/>",
            "<a xmlns={uri}/>",
            "<a xmlns:p={uri}/>",
            "<a x=word/>",
            "<a x='unfinished>",
            "<a></b>",
            "<a>",
            "<><a/></a>",
            "<a><></></a>",
            "<!DOCTYPE a><a/>",
            "<!--unfinished",
            "<?unfinished",
            "<![CDATA[unfinished",
        ] {
            let script = Script {
                language: ScriptLanguage::ActionScript3,
                source: source.into(),
            };
            assert!(
                scan(&script, 0, |_| panic!(
                    "unsupported dynamic position must diagnose first"
                ))
                .is_err(),
                "{source}"
            );
        }
        let nested = format!(
            "{}{}",
            "<a>".repeat(MAX_XML_DEPTH + 1),
            "</a>".repeat(MAX_XML_DEPTH + 1)
        );
        let holes = format!("<a>{}</a>", "{x}".repeat(MAX_XML_LITERAL_HOLES + 1));
        for source in [
            nested,
            holes,
            format!("<a>{}</a>", "x".repeat(MAX_XML_BYTES)),
        ] {
            let script = Script {
                language: ScriptLanguage::ActionScript3,
                source,
            };
            assert!(scan(&script, 0, |start| Ok(start + 2)).is_err());
        }
    }
}
