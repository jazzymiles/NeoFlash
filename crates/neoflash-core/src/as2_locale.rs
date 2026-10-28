//! Nonvisual AS2 Locale tables and XLIFF ingestion. Browser IO is a host concern.
//! Contract: Adobe AS2 Locale; Flash CS3 User Guide, exported XML sample p.287.
use crate::as2_xml::parse_as2_xml;
use crate::xml::{XmlNode, XmlNodeKind, XmlParseTree};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const MAX_LANGUAGES: usize = 128;
const MAX_STRINGS: usize = 10_000;
const MAX_TABLE_BYTES: usize = 4 * 1024 * 1024;

pub struct ScriptAS2Locale {
    tables: BTreeMap<String, BTreeMap<String, String>>,
    paths: BTreeMap<String, String>,
    languages: Vec<String>,
    ids: Vec<String>,
    default_language: String,
    current_language: String,
    loaded: bool,
    token: u32,
    pending: Option<String>,
}
impl Default for ScriptAS2Locale {
    fn default() -> Self {
        Self {
            tables: BTreeMap::new(),
            paths: BTreeMap::new(),
            languages: Vec::new(),
            ids: Vec::new(),
            default_language: "en".into(),
            current_language: "en".into(),
            loaded: false,
            token: 0,
            pending: None,
        }
    }
}
impl ScriptAS2Locale {
    pub fn new() -> Self {
        Self::default()
    }
    fn add_language(&mut self, language: &str) -> Result<(), String> {
        if !self.languages.iter().any(|v| v == language) {
            if self.languages.len() >= MAX_LANGUAGES {
                return Err("Locale supports at most 128 languages".into());
            }
            self.languages.push(language.into());
        }
        Ok(())
    }
    fn merge(
        &mut self,
        language: &str,
        rows: BTreeMap<String, String>,
        order: Vec<String>,
    ) -> Result<(), String> {
        let mut table = self.tables.get(language).cloned().unwrap_or_default();
        table.extend(rows);
        let mut ids = self.ids.clone();
        for id in order {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        if ids.len() > MAX_STRINGS {
            return Err("Locale supports at most 10000 string IDs".into());
        }
        let bytes: usize = self
            .tables
            .iter()
            .filter(|(name, _)| name.as_str() != language)
            .flat_map(|(_, t)| t.iter())
            .chain(table.iter())
            .map(|(id, value)| id.len() + value.len())
            .sum();
        if bytes > MAX_TABLE_BYTES {
            return Err("Locale translation tables exceed 4 MiB".into());
        }
        self.add_language(language)?;
        self.ids = ids;
        self.tables.insert(language.into(), table);
        Ok(())
    }
    pub fn execute(&mut self, input: &str) -> Result<String, String> {
        if input.len() > 7 * 1024 * 1024 {
            return Err("Locale request exceeds 7 MiB".into());
        }
        let request: Value =
            serde_json::from_str(input).map_err(|e| format!("Invalid Locale request: {e}"))?;
        let result = match string(&request, "op", 64)? {
            "state" => {
                json!({"default":self.default_language,"current":self.current_language,"loaded":self.loaded,"languages":self.languages,"ids":self.ids})
            }
            "default" => {
                let language = language(&request, "language")?;
                self.default_language = language.clone();
                if !self.loaded && self.pending.is_none() {
                    self.current_language = language;
                }
                Value::Null
            }
            "path" => {
                let language = language(&request, "language")?;
                let path = string(&request, "path", 8192)?.to_owned();
                self.add_language(&language)?;
                self.paths.insert(language, path);
                Value::Null
            }
            "set" => {
                let language = language(&request, "language")?;
                let id = string(&request, "id", 1024)?.to_owned();
                let value = request["value"]
                    .as_str()
                    .ok_or("Locale string value must be a string")?;
                if value.len() > 1024 * 1024 {
                    return Err("Locale string exceeds 1 MiB".into());
                }
                self.merge(
                    &language,
                    BTreeMap::from([(id.clone(), value.into())]),
                    vec![id],
                )?;
                Value::Null
            }
            "get" => {
                let language = if request["language"].is_null() {
                    self.current_language.clone()
                } else {
                    language(&request, "language")?
                };
                let id = string(&request, "id", 1024)?;
                self.tables
                    .get(&language)
                    .and_then(|table| table.get(id))
                    .map(|s| json!(s))
                    .unwrap_or(Value::Null)
            }
            "select" => {
                let requested = request["language"]
                    .as_str()
                    .unwrap_or("")
                    .replace('_', "-")
                    .to_ascii_lowercase();
                let exact = self
                    .paths
                    .keys()
                    .find(|key| key.eq_ignore_ascii_case(&requested));
                let base = requested.split('-').next().unwrap_or("");
                let matched =
                    exact.or_else(|| self.paths.keys().find(|key| key.eq_ignore_ascii_case(base)));
                json!(matched.unwrap_or(&self.default_language))
            }
            "begin" => {
                let language = language(&request, "language")?;
                self.token = self
                    .token
                    .checked_add(1)
                    .ok_or("Locale load sequence exhausted")?;
                self.loaded = false;
                self.pending = Some(language.clone());
                json!({"token":self.token,"path":self.paths.get(&language),"language":language})
            }
            "complete" => {
                let token = request["token"]
                    .as_u64()
                    .ok_or("Locale load token required")?;
                if token != u64::from(self.token) || self.pending.is_none() {
                    json!({"accepted":false,"success":false})
                } else {
                    let language = self.pending.as_ref().unwrap().clone();
                    let source = request["xml"]
                        .as_str()
                        .ok_or("Locale XML source must be a string")?;
                    let (rows, order) = parse_xliff(source, &language)?;
                    self.merge(&language, rows, order)?;
                    self.current_language = language;
                    self.loaded = true;
                    self.pending = None;
                    json!({"accepted":true,"success":true})
                }
            }
            "failed" => {
                let accepted = request["token"].as_u64() == Some(u64::from(self.token))
                    && self.pending.is_some();
                if accepted {
                    self.pending = None;
                    self.loaded = false;
                }
                json!(accepted)
            }
            _ => return Err("Unknown AS2 Locale operation".into()),
        };
        Ok(result.to_string())
    }
}
fn string<'a>(value: &'a Value, name: &str, max: usize) -> Result<&'a str, String> {
    let result = value[name]
        .as_str()
        .ok_or_else(|| format!("Locale {name} requires a string"))?;
    if result.is_empty() || result.len() > max {
        return Err(format!("Locale {name} requires 1–{max} UTF-8 bytes"));
    }
    Ok(result)
}
fn language(value: &Value, name: &str) -> Result<String, String> {
    let result = string(value, name, 64)?;
    if !result
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("Locale language codes require letters, digits, hyphens or underscores".into());
    }
    Ok(result.replace('_', "-").to_ascii_lowercase())
}
fn named(node: &XmlNode, name: &str) -> bool {
    node.kind == XmlNodeKind::Element
        && node.name.as_ref().is_some_and(|n| {
            n.local == name
                && (n.uri.is_empty() || n.uri.starts_with("urn:oasis:names:tc:xliff:document:"))
        })
}
fn attr<'a>(node: &'a XmlNode, name: &str) -> Option<&'a str> {
    node.attributes
        .iter()
        .find(|a| a.name.qualified == name)
        .map(|a| a.value.as_str())
}
fn children<'a>(
    tree: &'a XmlParseTree,
    node: &'a XmlNode,
    name: &'a str,
) -> impl Iterator<Item = &'a XmlNode> {
    node.children
        .iter()
        .map(|i| &tree.nodes[*i])
        .filter(move |n| named(n, name))
}
fn parse_xliff(
    source: &str,
    language: &str,
) -> Result<(BTreeMap<String, String>, Vec<String>), String> {
    // AS2 parser retains but never resolves the conventional XLIFF external DTD.
    let tree = parse_as2_xml(source)
        .map_err(|e| format!("Locale XLIFF: {e}"))?
        .tree;
    let roots: Vec<_> = tree
        .roots
        .iter()
        .map(|i| &tree.nodes[*i])
        .filter(|n| n.kind == XmlNodeKind::Element)
        .collect();
    if roots.len() != 1 || !named(roots[0], "xliff") {
        return Err("Locale requires one XLIFF document root".into());
    }
    let root = roots[0];
    if attr(root, "version").is_some_and(|v| !matches!(v, "1.0" | "1.1" | "1.2")) {
        return Err("Locale supports the Flash XLIFF 1.x plain-text profile".into());
    }
    if attr(root, "xml:lang").is_some_and(|v| v.replace('_', "-").to_ascii_lowercase() != language)
    {
        return Err("Locale XLIFF language differs from the requested language".into());
    }
    let mut rows = BTreeMap::new();
    let mut order = Vec::new();
    let mut files = 0;
    for file in children(&tree, root, "file") {
        files += 1;
        if attr(file, "datatype").is_some_and(|v| v != "plaintext") {
            return Err("Locale currently supports XLIFF plaintext files".into());
        }
        for body in children(&tree, file, "body") {
            for unit in body.children.iter().map(|i| &tree.nodes[*i]) {
                if unit.kind != XmlNodeKind::Element {
                    continue;
                }
                if !named(unit, "trans-unit") {
                    return Err("Locale XLIFF body requires direct trans-unit elements; grouped units are unavailable".into());
                }
                let id = attr(unit, "resname").ok_or("Flash XLIFF trans-unit requires resname")?;
                if id.is_empty() || id.len() > 1024 {
                    return Err("Locale XLIFF resname requires 1–1024 UTF-8 bytes".into());
                }
                if rows.contains_key(id) {
                    return Err(format!("Locale XLIFF contains duplicate resname '{id}'"));
                }
                if children(&tree, unit, "target").next().is_some() {
                    return Err("Locale loads Flash Strings-panel source values; XLIFF target selection is unavailable".into());
                }
                let sources: Vec<_> = children(&tree, unit, "source").collect();
                if sources.len() != 1 {
                    return Err("Locale trans-unit requires one source element".into());
                }
                let mut text = String::new();
                for node in sources[0].children.iter().map(|i| &tree.nodes[*i]) {
                    match node.kind {
                        XmlNodeKind::Text => text.push_str(&node.value),
                        XmlNodeKind::Element => return Err(
                            "Locale inline XLIFF markup is unavailable; use plain text or CDATA"
                                .into(),
                        ),
                        _ => {}
                    }
                }
                rows.insert(id.into(), text);
                order.push(id.into());
                if rows.len() > MAX_STRINGS {
                    return Err("Locale XLIFF exceeds 10000 strings".into());
                }
            }
        }
    }
    if files == 0 {
        return Err("Locale XLIFF requires a file element".into());
    }
    Ok((rows, order))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn call(locale: &mut ScriptAS2Locale, value: Value) -> Value {
        serde_json::from_str(&locale.execute(&value.to_string()).unwrap()).unwrap()
    }
    #[test]
    fn as2_locale_family() {
        let mut locale = ScriptAS2Locale::new();
        call(
            &mut locale,
            json!({"op":"set","language":"en","id":"GREETING","value":"Hello"}),
        );
        call(
            &mut locale,
            json!({"op":"set","language":"fr","id":"GREETING","value":"Salut"}),
        );
        assert_eq!(
            call(
                &mut locale,
                json!({"op":"get","id":"GREETING","language":"fr"})
            ),
            "Salut"
        );
        call(&mut locale, json!({"op":"default","language":"fr"}));
        assert_eq!(
            call(&mut locale, json!({"op":"get","id":"GREETING"})),
            "Salut"
        );
        call(
            &mut locale,
            json!({"op":"path","language":"en","path":"en.xml"}),
        );
        call(
            &mut locale,
            json!({"op":"path","language":"fr","path":"fr.xml"}),
        );
        assert_eq!(
            call(&mut locale, json!({"op":"select","language":"fr-CA"})),
            "fr"
        );
        assert_eq!(
            call(&mut locale, json!({"op":"select","language":"xx"})),
            "fr"
        );
        let first = call(&mut locale, json!({"op":"begin","language":"en"}));
        let next = call(&mut locale, json!({"op":"begin","language":"fr"}));
        let xml = "<?xml version='1.0'?><!DOCTYPE xliff PUBLIC '-//XLIFF//DTD XLIFF//EN' 'https://example.invalid/xliff.dtd'><xliff version='1.0' xml:lang='fr'><file datatype='plaintext'><body><trans-unit id='1' resname='GREETING'><source>Bonjour &amp; bienvenue!</source></trans-unit><trans-unit id='2' resname='EMPTY'><source/></trans-unit><trans-unit id='3' resname='TEXT'><source><![CDATA[<b>Texte</b>]]></source></trans-unit></body></file></xliff>";
        assert_eq!(
            call(
                &mut locale,
                json!({"op":"complete","token":first["token"],"xml":xml})
            )["accepted"],
            false
        );
        assert!(
            locale
                .execute(
                    &json!({"op":"complete","token":next["token"],"xml":"<xliff>"}).to_string()
                )
                .is_err()
        );
        assert_eq!(
            call(&mut locale, json!({"op":"get","id":"GREETING"})),
            "Salut"
        );
        assert_eq!(
            call(
                &mut locale,
                json!({"op":"complete","token":next["token"],"xml":xml})
            )["success"],
            true
        );
        assert_eq!(
            call(&mut locale, json!({"op":"get","id":"GREETING"})),
            "Bonjour & bienvenue!"
        );
        assert_eq!(call(&mut locale, json!({"op":"get","id":"EMPTY"})), "");
        assert_eq!(
            call(&mut locale, json!({"op":"get","id":"TEXT"})),
            "<b>Texte</b>"
        );
        let state = call(&mut locale, json!({"op":"state"}));
        assert_eq!(state["languages"], json!(["en", "fr"]));
        assert_eq!(state["ids"], json!(["GREETING", "EMPTY", "TEXT"]));
        assert_eq!(state["loaded"], true);
        assert!(
            parse_xliff(
                &xml.replace("<source/>", "<source><b>unsupported</b></source>"),
                "fr"
            )
            .is_err()
        );
        let bad = call(&mut locale, json!({"op":"begin","language":"missing"}));
        assert!(bad["path"].is_null());
        call(&mut locale, json!({"op":"failed","token":bad["token"]}));
        assert_eq!(call(&mut locale, json!({"op":"state"}))["loaded"], false);
    }
}
