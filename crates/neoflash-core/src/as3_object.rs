//! Bounded AS3 Object property policy and source-class reflection.
//! Original Rust implementation from Adobe's public Object and flash.utils contracts:
//! https://help.adobe.com/en_US/FlashPlatform/reference/actionscript/3/Object.html
//! https://help.adobe.com/en_US/FlashPlatform/reference/actionscript/3/flash/utils/package.html
//! Default labels were checked against Adobe avmplus 858d034a3bd3a54d9b70909386435cf4aec81d21,
//! core/ScriptObject.cpp::implToString and core/ClassClosure.cpp::implToString.
//! No native/bytecode trait catalogue is inferred. Unsupported reflection diagnoses.
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_OBJECT_JSON_BYTES: usize = 8 * 1024 * 1024;
const MAX_CLASSES: usize = 4096;
const MAX_MEMBERS: usize = 16384;

#[derive(Clone, Deserialize)]
struct Parameter {
    #[serde(rename = "type")]
    ty: String,
    optional: bool,
}
#[derive(Clone, Deserialize)]
struct Member {
    name: String,
    kind: String,
    access: String,
    #[serde(rename = "static")]
    is_static: bool,
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    parameters: Vec<Parameter>,
    #[serde(default)]
    rest: bool,
}
#[derive(Clone, Deserialize)]
struct Class {
    name: String,
    base: Option<String>,
    public: bool,
    dynamic: bool,
    #[serde(rename = "final")]
    is_final: bool,
    #[serde(default)]
    interface: bool,
    #[serde(default)]
    interfaces: Vec<String>,
    members: Vec<Member>,
    #[serde(default)]
    constructor: Vec<Parameter>,
    #[serde(default)]
    constructor_rest: bool,
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Object metadata requires string {key}"))
}
fn boolean(value: &Value, key: &str) -> Result<bool, String> {
    value
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("Object metadata requires Boolean {key}"))
}
fn qualified(name: &str) -> String {
    if let Some(element) = name
        .strip_prefix("Vector.<")
        .and_then(|name| name.strip_suffix('>'))
    {
        format!("__AS3__.vec::Vector.<{}>", qualified(element))
    } else if name.contains("::") {
        name.into()
    } else if let Some((package, local)) = name.rsplit_once('.') {
        format!("{package}::{local}")
    } else {
        name.into()
    }
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn attr(name: &str, value: &str) -> String {
    format!(" {name}=\"{}\"", escape(value))
}
fn property(request: &Value) -> Result<Value, String> {
    let method = string(request, "method")?;
    if string(request, "name")?.len() > 1_000_000 {
        return Err("Object property name exceeds 1000000 bytes".into());
    }
    if !boolean(request, "supported")? {
        return Err("Object property queries support plain records and ordinary AS3 project classes; native and AS2 trait reflection is unsupported".into());
    }
    let own = boolean(request, "own")?;
    let fixed = boolean(request, "fixed")?;
    match method {
        "hasOwnProperty" => Ok(json!({"value": own || fixed, "change_enumerable": null})),
        "propertyIsEnumerable" => Ok(
            json!({"value": own && !fixed && boolean(request, "enumerable")?, "change_enumerable": null}),
        ),
        "setPropertyIsEnumerable" => {
            let desired = boolean(request, "is_enum")?;
            let change = if own && !fixed {
                if !boolean(request, "configurable")? {
                    return Err(
                        "Cannot change enumerability of this nonconfigurable host property".into(),
                    );
                }
                Some(desired)
            } else {
                None
            };
            Ok(json!({"value": null, "change_enumerable": change}))
        }
        _ => Err(format!("Unsupported Object property operation {method}")),
    }
}
fn classes(request: &Value) -> Result<BTreeMap<String, Class>, String> {
    let values: Vec<Class> =
        serde_json::from_value(request.get("classes").cloned().unwrap_or_else(|| json!([])))
            .map_err(|e| format!("Invalid source reflection metadata: {e}"))?;
    if values.len() > MAX_CLASSES
        || values.iter().map(|c| c.members.len()).sum::<usize>() > MAX_MEMBERS
    {
        return Err("Source reflection exceeds 4096 classes or 16384 traits".into());
    }
    let mut result = BTreeMap::new();
    for class in values {
        if result.insert(class.name.clone(), class).is_some() {
            return Err("Duplicate source reflection class".into());
        }
    }
    Ok(result)
}
fn value_name(request: &Value) -> Result<String, String> {
    let value = request
        .get("value")
        .ok_or("Missing reflection value metadata")?;
    Ok(match string(value, "kind")? {
        "project" | "class" => string(value, "name")?.into(),
        "undefined" => "void".into(), "null" => "null".into(),
        "boolean" => "Boolean".into(), "string" => "String".into(),
        "array" => "Array".into(), "date" => "Date".into(), "regexp" => "RegExp".into(),
        "function" => "Function".into(), "object" => "Object".into(),
        "number" => return Err("Numeric value reflection cannot recover erased Number/int/uint identity; pass the numeric class object explicitly".into()),
        _ => return Err("Reflection of this native or foreign value is unsupported; use an ordinary AS3 project class".into()),
    })
}
fn base_name<'a>(
    name: &str,
    classes: &'a BTreeMap<String, Class>,
) -> Result<Option<&'a str>, String> {
    if let Some(class) = classes.get(name) {
        return Ok(if class.interface {
            None
        } else {
            Some(class.base.as_deref().unwrap_or("Object"))
        });
    }
    Ok(match name {
        "Object" | "null" | "void" => None,
        "Number" | "int" | "uint" | "Boolean" | "String" | "Array" | "Date" | "RegExp"
        | "Function" | "Class" => Some("Object"),
        _ => {
            return Err(format!(
                "Native superclass metadata for {name} is not implemented"
            ));
        }
    })
}
fn lineage<'a>(name: &str, classes: &'a BTreeMap<String, Class>) -> Result<Vec<&'a Class>, String> {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = name;
    while current != "Object" {
        if !seen.insert(current) {
            return Err("Cyclic reflection inheritance".into());
        }
        let class = classes.get(current).ok_or_else(|| format!("describeType cannot describe native base {current}; complete native trait metadata is not implemented"))?;
        if class.interface {
            return Err("describeType interface metadata is not implemented; use the qualified-name functions".into());
        }
        result.push(class);
        current = class.base.as_deref().unwrap_or("Object");
    }
    Ok(result)
}
fn parameters(out: &mut String, values: &[Parameter]) {
    for (index, parameter) in values.iter().enumerate() {
        out.push_str(&format!(
            "<parameter{}{}{}/>",
            attr("index", &(index + 1).to_string()),
            attr("type", &qualified(&parameter.ty)),
            attr(
                "optional",
                if parameter.optional { "true" } else { "false" }
            )
        ));
    }
}
fn traits(out: &mut String, chain: &[&Class], is_static: bool) -> Result<(), String> {
    let mut members = BTreeMap::<String, (&Member, &str, bool, bool)>::new();
    let definitions: Vec<_> = if is_static {
        chain.first().copied().into_iter().collect()
    } else {
        chain.iter().copied().rev().collect()
    };
    for class in definitions {
        for member in &class.members {
            if member.access != "public" || member.is_static != is_static {
                continue;
            }
            if member.rest {
                return Err("describeType rest-parameter metadata is not yet supported".into());
            }
            let previous = members.get(&member.name);
            let read = member.kind == "get" || previous.is_some_and(|p| p.2);
            let write = member.kind == "set" || previous.is_some_and(|p| p.3);
            members.insert(member.name.clone(), (member, &class.name, read, write));
        }
    }
    for (member, owner, read, write) in members.values() {
        let kind = match member.kind.as_str() {
            "get" | "set" => "accessor",
            "method" => "method",
            "constant" => "constant",
            "variable" => "variable",
            _ => return Err("Unknown reflection trait kind".into()),
        };
        out.push('<');
        out.push_str(kind);
        out.push_str(&attr("name", &member.name));
        out.push_str(&attr(
            if kind == "method" {
                "returnType"
            } else {
                "type"
            },
            &qualified(&member.ty),
        ));
        if matches!(kind, "method" | "accessor") {
            out.push_str(&attr("declaredBy", &qualified(owner)));
        }
        if kind == "accessor" {
            out.push_str(&attr(
                "access",
                match (*read, *write) {
                    (true, true) => "readwrite",
                    (true, false) => "readonly",
                    _ => "writeonly",
                },
            ));
        }
        if kind == "method" {
            out.push('>');
            parameters(out, &member.parameters);
            out.push_str("</method>");
        } else {
            out.push_str("/>");
        }
    }
    Ok(())
}
fn instance_body(
    out: &mut String,
    chain: &[&Class],
    include_constructor: bool,
) -> Result<(), String> {
    for class in chain.iter().skip(1) {
        out.push_str(&format!(
            "<extendsClass{}/>",
            attr("type", &qualified(&class.name))
        ));
    }
    out.push_str("<extendsClass type=\"Object\"/>");
    let interfaces: BTreeSet<_> = chain
        .iter()
        .flat_map(|class| class.interfaces.iter())
        .collect();
    for interface in interfaces {
        out.push_str(&format!(
            "<implementsInterface{}/>",
            attr("type", &qualified(interface))
        ));
    }
    if include_constructor {
        let class = chain[0];
        if class.constructor_rest {
            return Err("describeType rest-constructor metadata is not yet supported".into());
        }
        out.push_str("<constructor>");
        parameters(out, &class.constructor);
        out.push_str("</constructor>");
    }
    traits(out, chain, false)
}
fn describe(
    request: &Value,
    name: &str,
    classes: &BTreeMap<String, Class>,
) -> Result<String, String> {
    let chain = lineage(name, classes)?;
    let class = chain
        .first()
        .ok_or("describeType currently requires a declared AS3 project class")?;
    let is_static = string(&request["value"], "kind")? == "class";
    let base = if is_static {
        "Class".into()
    } else {
        class
            .base
            .as_deref()
            .map(qualified)
            .unwrap_or_else(|| "Object".into())
    };
    let mut out = format!(
        "<type{}{}{}{}{}>",
        attr("name", &qualified(name)),
        attr("base", &base),
        attr(
            "isDynamic",
            if is_static || class.dynamic {
                "true"
            } else {
                "false"
            }
        ),
        attr(
            "isFinal",
            if is_static || class.is_final {
                "true"
            } else {
                "false"
            }
        ),
        attr("isStatic", if is_static { "true" } else { "false" })
    );
    if is_static {
        out.push_str("<extendsClass type=\"Class\"/><extendsClass type=\"Object\"/>");
        traits(&mut out, &chain, true)?;
        out.push_str(&format!("<factory{}>", attr("type", &qualified(name))));
        instance_body(&mut out, &chain, true)?;
        out.push_str("</factory>");
    } else {
        instance_body(&mut out, &chain, false)?;
    }
    out.push_str("</type>");
    Ok(out)
}
pub fn execute(request: Value) -> Result<Value, String> {
    match string(&request, "op")? {
        "property" => property(&request),
        "default_string" => {
            let name = string(&request, "name")?;
            if name.is_empty() || name.len() > 4096 {
                return Err("Object class label requires 1 through 4096 bytes".into());
            }
            let short = name
                .rsplit("::")
                .next()
                .unwrap()
                .rsplit('.')
                .next()
                .unwrap();
            Ok(json!(format!(
                "[{} {short}]",
                if boolean(&request, "class_object")? {
                    "class"
                } else {
                    "object"
                }
            )))
        }
        "getDefinitionByName" => {
            let name = string(&request, "name")?;
            if name.len() > 4096 {
                return Err("Definition name exceeds 4096 bytes".into());
            }
            let canonical = name.replace("::", ".");
            let classes = classes(&request)?;
            let names = request["definitions"]
                .as_array()
                .ok_or("Missing public definition list")?;
            if names.len() > MAX_CLASSES {
                return Err("Definition lookup exceeds 4096 names".into());
            }
            if classes.get(&canonical).is_some_and(|class| class.public)
                || names.iter().any(|value| value.as_str() == Some(&canonical))
            {
                Ok(json!(canonical))
            } else {
                Err(format!("No public definition exists with the name {name}"))
            }
        }
        "getQualifiedClassName" => Ok(json!(qualified(&value_name(&request)?))),
        "getQualifiedSuperclassName" => Ok(base_name(&value_name(&request)?, &classes(&request)?)?
            .map(qualified)
            .map(Value::String)
            .unwrap_or(Value::Null)),
        "describeType" => Ok(json!(describe(
            &request,
            &value_name(&request)?,
            &classes(&request)?
        )?)),
        operation => Err(format!("Unsupported AS3 Object operation {operation}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_traits_and_dynamic_flags_are_distinct() {
        let mut request = json!({"op":"property","method":"setPropertyIsEnumerable","name":"count","own":true,"fixed":true,"enumerable":true,"configurable":true,"supported":true,"is_enum":true});
        assert_eq!(
            execute(request.clone()).unwrap()["change_enumerable"],
            Value::Null
        );
        request["method"] = json!("propertyIsEnumerable");
        assert_eq!(execute(request.clone()).unwrap()["value"], false);
        request["fixed"] = json!(false);
        request["method"] = json!("setPropertyIsEnumerable");
        request["is_enum"] = json!(false);
        assert_eq!(execute(request).unwrap()["change_enumerable"], false);
    }
    #[test]
    fn default_labels_use_local_trait_names() {
        assert_eq!(
            execute(json!({"op":"default_string","name":"app.Item","class_object":false})).unwrap(),
            "[object Item]"
        );
        assert_eq!(
            execute(json!({"op":"default_string","name":"app::Item","class_object":true})).unwrap(),
            "[class Item]"
        );
    }
    #[test]
    fn reflection_uses_public_traits_without_reading_values() {
        let classes = json!([
            {"name":"model.Base","base":null,"public":true,"dynamic":false,"final":false,"members":[{"name":"secret","kind":"variable","access":"private","static":false,"type":"String"},{"name":"score","kind":"get","access":"public","static":false,"type":"int"}]},
            {"name":"model.Item","base":"model.Base","public":true,"dynamic":true,"final":false,"members":[{"name":"score","kind":"set","access":"public","static":false,"type":"int"},{"name":"make","kind":"method","access":"public","static":true,"type":"model.Item","parameters":[{"type":"int","optional":true}]}]}
        ]);
        let output = execute(json!({"op":"describeType","value":{"kind":"class","name":"model.Item"},"classes":classes})).unwrap();
        let xml = output.as_str().unwrap();
        assert!(xml.contains("name=\"model::Item\""));
        assert!(xml.contains("<factory type=\"model::Item\""));
        assert!(xml.contains("access=\"readwrite\""));
        assert!(xml.contains("optional=\"true\""));
        assert!(!xml.contains("secret"));
    }
}
