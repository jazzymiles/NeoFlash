//! AS2 SOAP 1.1/WSDL 1.1 literal services. Pure parsing/encoding policy; no IO.
//! Reuses the bounded quick-xml-backed XML tree. Contracts: W3C NOTE-wsdl-20010315,
//! NOTE-SOAP-20000508 and Adobe Flash CS3 AS2 Components Reference pp.1415–1506.
use crate::xml::{XmlNodeKind, XmlParseTree, parse_xml};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
const WSDL: &str = "http://schemas.xmlsoap.org/wsdl/";
const BIND: &str = "http://schemas.xmlsoap.org/wsdl/soap/";
const SOAP: &str = "http://schemas.xmlsoap.org/soap/envelope/";
const XSD: &str = "http://www.w3.org/2001/XMLSchema";
const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Field {
    name: String,
    kind: String,
    uri: String,
    optional: bool,
    nullable: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Operation {
    name: String,
    action: String,
    uri: String,
    request: String,
    response: String,
    response_uri: String,
    input: Vec<Field>,
    output: Vec<Field>,
}
fn attr<'a>(t: &'a XmlParseTree, n: usize, name: &str) -> &'a str {
    t.nodes[n]
        .attributes
        .iter()
        .find(|a| a.name.local == name && a.name.uri.is_empty())
        .map(|a| a.value.as_str())
        .unwrap_or("")
}
fn is(t: &XmlParseTree, n: usize, uri: &str, name: &str) -> bool {
    t.nodes[n]
        .name
        .as_ref()
        .is_some_and(|v| v.uri == uri && v.local == name)
}
fn elements(t: &XmlParseTree, n: usize) -> Vec<usize> {
    t.nodes[n]
        .children
        .iter()
        .copied()
        .filter(|&i| t.nodes[i].kind == XmlNodeKind::Element)
        .collect()
}
fn child(t: &XmlParseTree, n: usize, uri: &str, name: &str) -> Result<usize, String> {
    let found: Vec<_> = elements(t, n)
        .into_iter()
        .filter(|&i| is(t, i, uri, name))
        .collect();
    if found.len() != 1 {
        return Err(format!("Expected one {name} element"));
    }
    Ok(found[0])
}
fn named(t: &XmlParseTree, n: usize, uri: &str, kind: &str, name: &str) -> Result<usize, String> {
    elements(t, n)
        .into_iter()
        .find(|&i| is(t, i, uri, kind) && attr(t, i, "name") == name)
        .ok_or_else(|| format!("Missing inline {kind} '{name}'"))
}
fn namespaces(t: &XmlParseTree, mut n: usize) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    loop {
        for d in &t.nodes[n].declarations {
            out.entry(d.prefix.clone()).or_insert_with(|| d.uri.clone());
        }
        if let Some(p) = t.nodes[n].parent {
            n = p
        } else {
            break;
        }
    }
    out
}
fn qname(t: &XmlParseTree, n: usize, v: &str) -> Result<(String, String), String> {
    let (p, local) = v.split_once(':').unwrap_or(("", v));
    if local.is_empty() {
        return Err("Missing QName".into());
    }
    let ns = namespaces(t, n);
    let uri = ns.get(p).cloned().unwrap_or_default();
    if !p.is_empty() && uri.is_empty() {
        return Err(format!("Unbound QName prefix {p}"));
    }
    Ok((uri, local.into()))
}
fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.chars().enumerate().all(|(i, c)| {
            c == '_'
                || c.is_ascii_alphabetic()
                || (i > 0 && (c.is_ascii_digit() || c == '-' || c == '.'))
        })
}
fn simple_type(t: &XmlParseTree, n: usize, value: &str) -> Result<String, String> {
    let (uri, kind) = qname(t, n, value)?;
    if uri != XSD
        || !matches!(
            kind.as_str(),
            "string"
                | "boolean"
                | "int"
                | "integer"
                | "long"
                | "short"
                | "byte"
                | "unsignedInt"
                | "unsignedShort"
                | "unsignedByte"
                | "float"
                | "double"
                | "decimal"
        )
    {
        return Err(format!(
            "Unsupported SOAP schema type {value}; use inline scalar XSD types"
        ));
    }
    Ok(kind)
}
fn fields(
    t: &XmlParseTree,
    root: usize,
    message: &str,
    tns: &str,
) -> Result<(String, String, Vec<Field>), String> {
    let msg = named(t, root, WSDL, "message", message)?;
    let parts: Vec<_> = elements(t, msg)
        .into_iter()
        .filter(|&n| is(t, n, WSDL, "part"))
        .collect();
    if parts.is_empty() || parts.len() > 128 {
        return Err("A SOAP message needs 1–128 parts".into());
    }
    if parts.len() == 1 && !attr(t, parts[0], "element").is_empty() {
        let (uri, wrapper) = qname(t, parts[0], attr(t, parts[0], "element"))?;
        let types = child(t, root, WSDL, "types")?;
        let schema = elements(t, types)
            .into_iter()
            .find(|&n| is(t, n, XSD, "schema") && attr(t, n, "targetNamespace") == uri)
            .ok_or("Wrapper requires an inline XSD schema")?;
        let elem = named(t, schema, XSD, "element", &wrapper)?;
        let complex = child(t, elem, XSD, "complexType")?;
        let sequence = child(t, complex, XSD, "sequence")?;
        if elements(t, complex).len() != 1 {
            return Err("Only an inline sequence is supported in a SOAP wrapper".into());
        }
        let mut result = Vec::new();
        for n in elements(t, sequence) {
            if !is(t, n, XSD, "element")
                || !elements(t, n).is_empty()
                || !attr(t, n, "ref").is_empty()
            {
                return Err("SOAP wrappers support scalar element declarations only".into());
            }
            let name = attr(t, n, "name");
            if !valid_name(name) || result.iter().any(|f: &Field| f.name == name) {
                return Err("Invalid or duplicate SOAP parameter name".into());
            }
            if !matches!(attr(t, n, "maxOccurs"), "" | "1")
                || !matches!(attr(t, n, "minOccurs"), "" | "0" | "1")
            {
                return Err(
                    "Repeated SOAP schema elements are outside the literal scalar profile".into(),
                );
            }
            let qualified = attr(t, n, "form") == "qualified"
                || (attr(t, n, "form").is_empty()
                    && attr(t, schema, "elementFormDefault") == "qualified");
            result.push(Field {
                name: name.into(),
                kind: simple_type(t, n, attr(t, n, "type"))?,
                uri: if qualified {
                    uri.clone()
                } else {
                    String::new()
                },
                optional: attr(t, n, "minOccurs") == "0",
                nullable: matches!(attr(t, n, "nillable"), "true" | "1"),
            });
        }
        if result.len() > 128 {
            return Err("SOAP wrapper exceeds 128 parameters".into());
        }
        Ok((uri, wrapper, result))
    } else {
        let mut result = Vec::new();
        for n in parts {
            let name = attr(t, n, "name");
            if !valid_name(name) || !attr(t, n, "element").is_empty() {
                return Err("RPC/literal requires named scalar type parts".into());
            }
            result.push(Field {
                name: name.into(),
                kind: simple_type(t, n, attr(t, n, "type"))?,
                uri: String::new(),
                optional: false,
                nullable: false,
            });
        }
        Ok((tns.into(), String::new(), result))
    }
}
fn wsdl(source: &str) -> Result<Value, String> {
    let t = parse_xml(source).map_err(|e| e.to_string())?;
    let roots: Vec<_> = t
        .roots
        .iter()
        .copied()
        .filter(|&n| t.nodes[n].kind == XmlNodeKind::Element)
        .collect();
    if roots.len() != 1 || !is(&t, roots[0], WSDL, "definitions") {
        return Err("Expected WSDL 1.1 definitions".into());
    }
    let root = roots[0];
    let tns = attr(&t, root, "targetNamespace");
    for (n, node) in t.nodes.iter().enumerate() {
        if node.kind == XmlNodeKind::Element
            && (is(&t, n, WSDL, "import")
                || is(&t, n, XSD, "import")
                || is(&t, n, XSD, "include")
                || is(&t, n, XSD, "redefine"))
        {
            return Err(
                "External WSDL/XSD imports are unsupported; provide one inline WSDL document"
                    .into(),
            );
        }
    }
    let service = child(&t, root, WSDL, "service")?;
    let ports: Vec<_> = elements(&t, service)
        .into_iter()
        .filter(|&n| {
            is(&t, n, WSDL, "port")
                && elements(&t, n)
                    .into_iter()
                    .any(|a| is(&t, a, BIND, "address"))
        })
        .collect();
    if ports.len() != 1 {
        return Err("Provide exactly one SOAP 1.1 service port".into());
    }
    let port = ports[0];
    let endpoint = attr(&t, child(&t, port, BIND, "address")?, "location");
    if endpoint.is_empty() {
        return Err("SOAP address is empty".into());
    }
    let (bns, bname) = qname(&t, port, attr(&t, port, "binding"))?;
    if bns != tns {
        return Err("External WSDL binding is unsupported".into());
    }
    let binding = named(&t, root, WSDL, "binding", &bname)?;
    let sb = child(&t, binding, BIND, "binding")?;
    let default_style = attr(&t, sb, "style");
    if !matches!(default_style, "" | "document" | "rpc")
        || attr(&t, sb, "transport") != "http://schemas.xmlsoap.org/soap/http"
    {
        return Err("Only document/rpc literal SOAP over HTTP is supported".into());
    }
    let (pns, pname) = qname(&t, binding, attr(&t, binding, "type"))?;
    if pns != tns {
        return Err("External WSDL portType unsupported".into());
    }
    let pt = named(&t, root, WSDL, "portType", &pname)?;
    let mut ops = Vec::new();
    for bo in elements(&t, binding)
        .into_iter()
        .filter(|&n| is(&t, n, WSDL, "operation"))
    {
        let name = attr(&t, bo, "name");
        if !valid_name(name) || ops.iter().any(|o: &Operation| o.name == name) {
            return Err("Invalid/duplicate WSDL operation name".into());
        }
        let so = child(&t, bo, BIND, "operation")?;
        let style = if attr(&t, so, "style").is_empty() {
            default_style
        } else {
            attr(&t, so, "style")
        };
        if !matches!(style, "" | "document" | "rpc") {
            return Err("Unsupported SOAP operation style".into());
        }
        let op = named(&t, pt, WSDL, "operation", name)?;
        let mut descriptors = Vec::new();
        for direction in ["input", "output"] {
            let bio = child(&t, bo, WSDL, direction)?;
            let body = child(&t, bio, BIND, "body")?;
            if attr(&t, body, "use") != "literal"
                || !attr(&t, body, "parts").is_empty()
                || elements(&t, bio).len() != 1
            {
                return Err(
                    "SOAP encoded bodies, selected parts and headers are unsupported".into(),
                );
            }
            let io = child(&t, op, WSDL, direction)?;
            let (ns, msg) = qname(&t, io, attr(&t, io, "message"))?;
            if ns != tns {
                return Err("External WSDL message unsupported".into());
            }
            let (mut uri, mut wrapper, params) = fields(&t, root, &msg, tns)?;
            if style == "rpc" {
                if !wrapper.is_empty() {
                    return Err("RPC/literal messages require type parts".into());
                }
                uri = attr(&t, body, "namespace").into();
                if uri.is_empty() {
                    uri = tns.into()
                }
                wrapper = if direction == "input" {
                    name.into()
                } else {
                    format!("{name}Response")
                };
            } else if wrapper.is_empty() {
                return Err("Document/literal requires one inline wrapped element part".into());
            }
            descriptors.push((uri, wrapper, params));
        }
        let (out_uri, response, output) = descriptors.pop().unwrap();
        let (uri, request, input) = descriptors.pop().unwrap();
        ops.push(Operation {
            name: name.into(),
            action: attr(&t, so, "soapAction").into(),
            uri,
            request,
            response,
            response_uri: out_uri,
            input,
            output,
        });
        if ops.len() > 128 {
            return Err("WSDL supports at most 128 operations".into());
        }
    }
    if ops.is_empty() {
        return Err("WSDL has no supported operations".into());
    }
    Ok(json!({"endpoint":endpoint,"operations":ops}))
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn scalar(value: &Value, field: &Field) -> Result<String, String> {
    match field.kind.as_str() {
        "string" => value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("Parameter {} requires String", field.name)),
        "boolean" => value
            .as_bool()
            .map(|v| v.to_string())
            .ok_or_else(|| format!("Parameter {} requires Boolean", field.name)),
        _ => {
            let n = value
                .as_f64()
                .ok_or_else(|| format!("Parameter {} requires a finite Number", field.name))?;
            if !n.is_finite() {
                return Err("SOAP Number must be finite".into());
            }
            if !matches!(field.kind.as_str(), "float" | "double" | "decimal")
                && (n.fract() != 0.0 || n.abs() > 9007199254740991.0)
            {
                return Err("SOAP integers require exact safe integer values".into());
            }
            let (min, max) = match field.kind.as_str() {
                "byte" => (-128., 127.),
                "short" => (-32768., 32767.),
                "int" => (-2147483648., 2147483647.),
                "unsignedByte" => (0., 255.),
                "unsignedShort" => (0., 65535.),
                "unsignedInt" => (0., 4294967295.),
                _ => (f64::NEG_INFINITY, f64::INFINITY),
            };
            if n < min || n > max {
                return Err(format!("SOAP {} is outside its numeric range", field.kind));
            }
            Ok(value.to_string())
        }
    }
}
fn request(op: &Operation, params: &Value) -> Result<Value, String> {
    let mut xml = format!(
        "<soap:Envelope xmlns:soap=\"{SOAP}\" xmlns:xsi=\"{XSI}\"><soap:Body><m:{} xmlns:m=\"{}\">",
        op.request,
        escape(&op.uri)
    );
    let ar = params.as_array();
    let ob = params.as_object();
    if ar.is_none() && ob.is_none() {
        return Err("SOAP parameters require an Array or named Object".into());
    }
    if ar.is_some_and(|a| a.len() > op.input.len())
        || ob.is_some_and(|o| o.keys().any(|k| !op.input.iter().any(|f| &f.name == k)))
    {
        return Err("Unexpected SOAP parameter".into());
    }
    for (i, f) in op.input.iter().enumerate() {
        let v = ar
            .and_then(|a| a.get(i))
            .or_else(|| ob.and_then(|o| o.get(&f.name)));
        if v.is_none() && f.optional {
            continue;
        }
        let v = v.ok_or_else(|| format!("Missing SOAP parameter {}", f.name))?;
        let ns = if f.uri.is_empty() {
            String::new()
        } else {
            format!(" xmlns=\"{}\"", escape(&f.uri))
        };
        if v.is_null() {
            if !f.nullable {
                return Err(format!("SOAP parameter {} is not nillable", f.name));
            }
            xml += &format!("<{}{ns} xsi:nil=\"true\"/>", f.name);
        } else {
            xml += &format!("<{}{ns}>{}</{}>", f.name, escape(&scalar(v, f)?), f.name);
        }
    }
    xml += &format!("</m:{}></soap:Body></soap:Envelope>", op.request);
    parse_xml(&xml).map_err(|e| e.to_string())?;
    Ok(json!({"xml":xml,"action":op.action}))
}
fn node_text(t: &XmlParseTree, n: usize) -> String {
    t.nodes[n]
        .children
        .iter()
        .filter(|&&i| t.nodes[i].kind == XmlNodeKind::Text)
        .map(|&i| t.nodes[i].value.as_str())
        .collect()
}
fn serialize(t: &XmlParseTree, n: usize) -> String {
    fn emit(t: &XmlParseTree, n: usize, root: bool, out: &mut String) {
        let node = &t.nodes[n];
        match node.kind {
            XmlNodeKind::Element => {
                let name = &node.name.as_ref().unwrap().qualified;
                out.push('<');
                out.push_str(name);
                let ns = if root {
                    namespaces(t, n)
                } else {
                    node.declarations
                        .iter()
                        .map(|d| (d.prefix.clone(), d.uri.clone()))
                        .collect()
                };
                for (p, u) in ns {
                    out.push_str(&format!(
                        " xmlns{}=\"{}\"",
                        if p.is_empty() {
                            String::new()
                        } else {
                            format!(":{p}")
                        },
                        escape(&u)
                    ));
                }
                for a in &node.attributes {
                    out.push_str(&format!(" {}=\"{}\"", a.name.qualified, escape(&a.value)));
                }
                out.push('>');
                for &c in &node.children {
                    emit(t, c, false, out)
                }
                out.push_str(&format!("</{name}>"));
            }
            XmlNodeKind::Text => out.push_str(&escape(&node.value)),
            _ => {}
        }
    }
    let mut out = String::new();
    emit(t, n, true, &mut out);
    out
}
fn response(op: &Operation, source: &str, status: u16) -> Result<Value, String> {
    let t = parse_xml(source).map_err(|e| e.to_string())?;
    let root: Vec<_> = t
        .roots
        .iter()
        .copied()
        .filter(|&n| t.nodes[n].kind == XmlNodeKind::Element)
        .collect();
    if root.len() != 1 || !is(&t, root[0], SOAP, "Envelope") {
        return Err("Expected SOAP 1.1 Envelope".into());
    }
    for n in elements(&t, root[0]) {
        if is(&t, n, SOAP, "Header") && !elements(&t, n).is_empty() {
            return Err("SOAP response headers are outside this profile".into());
        }
    }
    let body = child(&t, root[0], SOAP, "Body")?;
    let content = elements(&t, body);
    if content.len() != 1 {
        return Err("SOAP body requires one response or Fault".into());
    }
    let payload = content[0];
    if is(&t, payload, SOAP, "Fault") {
        let mut fault = serde_json::Map::new();
        for name in ["faultcode", "faultstring", "faultactor", "detail"] {
            let value = elements(&t, payload)
                .into_iter()
                .find(|&i| t.nodes[i].name.as_ref().is_some_and(|n| n.local == name));
            fault.insert(
                name.into(),
                Value::String(
                    value
                        .map(|i| {
                            if name == "detail" {
                                serialize(&t, i)
                            } else {
                                node_text(&t, i)
                            }
                        })
                        .unwrap_or_default(),
                ),
            );
        }
        fault.insert("element".into(), Value::String(serialize(&t, payload)));
        if fault["faultcode"].as_str() == Some("") || fault["faultstring"].as_str() == Some("") {
            return Err("Malformed SOAP Fault".into());
        }
        return Ok(json!({"fault":fault}));
    }
    if status >= 400 {
        return Err(format!("HTTP {status} response is not a SOAP Fault"));
    }
    if !is(&t, payload, &op.response_uri, &op.response) {
        return Err("SOAP response wrapper does not match the WSDL operation".into());
    }
    let children = elements(&t, payload);
    let mut output = Vec::new();
    let mut used = Vec::new();
    for field in &op.output {
        let matching: Vec<_> = children
            .iter()
            .copied()
            .filter(|&n| is(&t, n, &field.uri, &field.name))
            .collect();
        if matching.is_empty() && field.optional {
            continue;
        }
        if matching.len() != 1 {
            return Err(format!("Expected one response parameter {}", field.name));
        }
        let n = matching[0];
        used.push(n);
        if !elements(&t, n).is_empty() {
            return Err("Nested SOAP result values require a richer schema profile".into());
        }
        let nil = t.nodes[n].attributes.iter().any(|a| {
            a.name.uri == XSI && a.name.local == "nil" && matches!(a.value.as_str(), "true" | "1")
        });
        let value = if nil {
            if !field.nullable {
                return Err("Unexpected nil response parameter".into());
            }
            Value::Null
        } else {
            let text = node_text(&t, n);
            match field.kind.as_str() {
                "string" => Value::String(text),
                "boolean" => match text.trim() {
                    "true" | "1" => Value::Bool(true),
                    "false" | "0" => Value::Bool(false),
                    _ => return Err("Invalid SOAP Boolean response".into()),
                },
                _ => {
                    let number = text
                        .trim()
                        .parse::<f64>()
                        .map_err(|_| "Invalid SOAP Number response")?;
                    let value = json!(number);
                    scalar(&value, field)?;
                    value
                }
            }
        };
        output.push(json!({"name":field.name,"value":value,"xml":serialize(&t,n)}));
    }
    if used.len() != children.len() {
        return Err("Unexpected SOAP response parameter".into());
    }
    Ok(json!({"output":output}))
}
pub fn command(input: &str) -> Result<Value, String> {
    if input.len() > 4 * 1024 * 1024 {
        return Err("AS2 service command exceeds 4 MiB".into());
    }
    let v: Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
    let text = |key: &str| {
        v[key]
            .as_str()
            .ok_or_else(|| format!("Missing service string {key}"))
    };
    match text("op")? {
        "wsdl" => wsdl(text("xml")?),
        "request" => request(
            &serde_json::from_value(v["operation"].clone()).map_err(|e| e.to_string())?,
            &v["params"],
        ),
        "response" => response(
            &serde_json::from_value(v["operation"].clone()).map_err(|e| e.to_string())?,
            text("xml")?,
            v["status"].as_u64().unwrap_or(200) as u16,
        ),
        "validateXml" => {
            let t = parse_xml(text("xml")?).map_err(|e| e.to_string())?;
            if t.roots
                .iter()
                .filter(|&&n| t.nodes[n].kind == XmlNodeKind::Element)
                .count()
                != 1
            {
                return Err("Connector XML needs exactly one document element".into());
            }
            Ok(Value::Null)
        }
        "log" => {
            let level = v["level"].as_u64().ok_or("Invalid Log level")?;
            if level > 3 {
                return Err("Log level must be NONE, BRIEF, VERBOSE or DEBUG".into());
            }
            let debug = v["debug"].as_bool().unwrap_or(false);
            if level == 0 || (debug && level < 3) {
                return Ok(Value::Null);
            }
            let parts = v["date"].as_array().ok_or("Missing local clock fields")?;
            if parts.len() != 5 {
                return Err("Expected month/day/hour/minute/second".into());
            }
            let n: Vec<u64> = parts.iter().map(|x| x.as_u64().unwrap_or(0)).collect();
            let date = format!("{}/{} {:02}:{:02}:{:02}", n[0], n[1], n[2], n[3], n[4]);
            let name = text("name")?;
            let msg = text("message")?;
            if name.len() > 1024 || msg.len() > 65536 {
                return Err("Log message/name exceeds service limits".into());
            }
            Ok(
                json!({"date":date,"message":format!("{} [{}] {}: {}",date,if debug{"DEBUG"}else{"INFO"},name,msg)}),
            )
        }
        _ => Err("Unknown AS2 service command".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn as2_services_family() {
        let fixture = include_str!("../../../examples/as2-services.wsdl");
        let service = wsdl(fixture).unwrap();
        let op: Operation = serde_json::from_value(service["operations"][0].clone()).unwrap();
        assert_eq!(service["endpoint"], "/__as2_soap");
        assert_eq!(op.input.len(), 2);
        let body = request(&op, &json!(["Rust & XML <safe>", 7])).unwrap();
        assert!(
            body["xml"]
                .as_str()
                .unwrap()
                .contains("Rust &amp; XML &lt;safe&gt;")
        );
        assert!(request(&op, &json!(["x", 1.5])).is_err());
        let reply = format!(
            r#"<s:Envelope xmlns:s="{SOAP}"><s:Body><EchoResponse xmlns="urn:neoflash:as2-services"><result>Rust &amp; XML</result><accepted>true</accepted><count>7</count></EchoResponse></s:Body></s:Envelope>"#
        );
        let values = response(&op, &reply, 200).unwrap();
        assert_eq!(values["output"][0]["value"], "Rust & XML");
        assert_eq!(values["output"][1]["value"], true);
        assert_eq!(values["output"][2]["value"], 7.0);
        parse_xml(values["output"][0]["xml"].as_str().unwrap()).unwrap();
        let failure = format!(
            r#"<s:Envelope xmlns:s="{SOAP}"><s:Body><s:Fault><faultcode>s:Server</faultcode><faultstring>Deliberate failure</faultstring><detail><reason>bad input</reason></detail></s:Fault></s:Body></s:Envelope>"#
        );
        assert_eq!(
            response(&op, &failure, 500).unwrap()["fault"]["faultcode"],
            "s:Server"
        );
        assert!(response(&op, &reply, 500).is_err());
        assert!(
            response(
                &op,
                &reply.replace("<count>7</count>", "<count>bad</count>"),
                200
            )
            .is_err()
        );
        assert!(wsdl(&fixture.replace("use=\"literal\"", "use=\"encoded\"")).is_err());
        assert!(
            wsdl(&fixture.replace(
                "<wsdl:types>",
                "<wsdl:import namespace=\"urn:x\" location=\"remote.wsdl\"/><wsdl:types>"
            ))
            .is_err()
        );
        assert!(command(r#"{"op":"validateXml","xml":"<!DOCTYPE x SYSTEM 'https://invalid.example/dtd'><x/>"}"#).is_err());
        assert_eq!(command(r#"{"op":"log","level":0,"debug":false,"date":[10,28,12,3,4],"name":"demo","message":"suppressed"}"#).unwrap(),Value::Null);
        assert_eq!(command(r#"{"op":"log","level":3,"debug":true,"date":[10,28,12,3,4],"name":"demo","message":"visible"}"#).unwrap()["message"],"10/28 12:03:04 [DEBUG] demo: visible");
    }
}
