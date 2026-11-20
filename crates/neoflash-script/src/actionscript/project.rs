//! Project source units and a deliberately bounded AS2/AS3 class linker.
//! No filesystem/network access: every dependency comes from the Project snapshot.
use super::*;
use neoflash_core::Project;
use std::collections::{BTreeMap, BTreeSet};

#[path = "project_builtins.rs"]
mod project_builtins;
#[path = "project_classes.rs"]
mod project_classes;
#[path = "project_contracts.rs"]
mod project_contracts;

use self::project_builtins::{
    AS2_BUILTINS, BUILTIN_FUNCTIONS, BUILTINS, builtin_namespace_uri, is_display_method,
};
pub(super) use self::project_builtins::{is_graphics_constants, is_host_method};
use self::project_classes::{
    context, frontend, header, imports, parse_class, prefix, visible_names,
};
use self::project_contracts::{
    MethodContract, Signature, builtin_interface_contracts, check_accessor_pairs, emit_class,
    method_contract, native_member, reflection_metadata,
};

/// `code` is a function body taking __nfBuiltins and returning the program API.
pub struct ProjectProgram {
    includes: BTreeMap<String, neoflash_core::ResourceUnit>,
    pub code: String,
    pub source_maps: BTreeMap<String, crate::SourceMap>,
    pub class_names: Vec<String>,
    names: BTreeSet<String>,
    languages: BTreeMap<String, ScriptLanguage>,
    interfaces: BTreeSet<String>,
    public_types: BTreeSet<String>,
    namespace_uris: BTreeMap<String, String>,
    as2_private: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Access {
    Public,
    Private,
    Protected,
    Internal,
}
impl Access {
    fn name(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
            Self::Protected => "protected",
            Self::Internal => "internal",
        }
    }
    fn inherited(self, owner: &str, package: &str, as2: bool) -> bool {
        as2 || match self {
            Self::Public | Self::Protected => true,
            Self::Private => false,
            Self::Internal => type_package(owner) == package,
        }
    }
}
fn type_package(name: &str) -> &str {
    name.rsplit_once('.').map_or("", |(package, _)| package)
}
#[derive(Clone)]
struct MemberRecord {
    name: String,
    ty: Option<String>,
    access: Access,
    namespace: Option<String>,
    owner: String,
    is_static: bool,
}

struct Header {
    script: Script,
    path: String,
    package: String,
    name: String,
    base: Option<String>,
    interfaces: Vec<String>,
    is_interface: bool,
    is_namespace: bool,
    namespace_uri: Option<String>,
    local_namespaces: BTreeMap<String, String>,
    open_namespaces: Vec<String>,
    imports: Vec<String>,
    tokens: TokenCursor,
    body: usize,
    packaged: bool,
    public: bool,
    dynamic: bool,
    final_: bool,
    intrinsic: bool,
}
struct Field {
    offset: usize,
    name: String,
    ty: Option<String>,
    value: Option<Expr>,
    constant: bool,
    is_static: bool,
    access: Access,
    namespace: Option<String>,
}
struct Method {
    function: Function,
    is_static: bool,
    override_: bool,
    access: Access,
    namespace: Option<String>,
    open_namespaces: Vec<String>,
    final_: bool,
    kind: MethodKind,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MethodKind {
    Method,
    Get,
    Set,
}
struct Class {
    header: Header,
    context: CompileContext,
    base: Option<String>,
    interfaces: Vec<String>,
    fields: Vec<Field>,
    methods: Vec<Method>,
}

pub fn compile_project(project: &Project) -> Result<ProjectProgram, Vec<Diagnostic>> {
    let includes: BTreeMap<_, _> = project
        .resources
        .iter()
        .filter(|r| r.path.to_ascii_lowercase().ends_with(".as"))
        .map(|r| (r.path.clone(), r.clone()))
        .collect();
    let mut headers = vec![];
    let mut errors = vec![];
    let mut names: BTreeSet<String> = BUILTINS
        .iter()
        .chain(BUILTIN_FUNCTIONS)
        .map(|s| (*s).into())
        .collect();
    for source in &project.sources {
        match header(source, &includes) {
            Ok(h) => {
                if !names.insert(h.name.clone()) {
                    errors.push(Diagnostic::at(
                        &h.script,
                        0,
                        format!("{}: duplicate class {}", h.path, h.name),
                    ));
                }
                headers.push(h);
            }
            Err(e) => errors.push(prefix(e, &source.path)),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let languages: BTreeMap<_, _> = headers
        .iter()
        .map(|h| (h.name.clone(), h.script.language))
        .collect();
    let public_types: BTreeSet<_> = headers
        .iter()
        .filter(|h| h.public)
        .map(|h| h.name.clone())
        .collect();
    let namespace_uris: BTreeMap<_, _> = headers
        .iter()
        .filter(|h| h.is_namespace)
        .map(|h| (h.name.clone(), h.namespace_uri.clone().unwrap_or_default()))
        .collect();
    let builtin_interfaces = builtin_interface_contracts();
    let interfaces: BTreeSet<_> = headers
        .iter()
        .filter(|h| h.is_interface)
        .map(|h| h.name.clone())
        .chain(builtin_interfaces.keys().cloned())
        .collect();
    let mut classes = BTreeMap::new();
    for h in headers {
        if h.is_namespace {
            continue;
        }
        let path = h.path.clone();
        let visible = visible_names(
            &names,
            &languages,
            h.script.language,
            &public_types,
            &h.package,
        );
        match parse_class(h, &visible, &interfaces, &namespace_uris) {
            Ok(c) => {
                classes.insert(c.header.name.clone(), c);
            }
            Err(e) => errors.push(prefix(e, &path)),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut order = vec![];
    let mut pending: BTreeSet<_> = classes.keys().cloned().collect();
    while !pending.is_empty() {
        let before = pending.len();
        for name in pending.clone() {
            if classes[&name]
                .base
                .iter()
                .chain(&classes[&name].interfaces)
                .all(|dependency| !pending.contains(dependency))
            {
                pending.remove(&name);
                order.push(name);
            }
        }
        if pending.len() == before {
            let c = &classes[pending.first().unwrap()];
            return Err(vec![Diagnostic::at(
                &c.header.script,
                0,
                format!("{}: inheritance cycle", c.header.path),
            )]);
        }
    }
    // Retain ancestor traits with their declaring owner; a foreign-package
    // intermediate subclass must not erase an ancestor's internal namespace.
    let final_classes: BTreeSet<_> = classes
        .values()
        .filter(|c| c.header.final_)
        .map(|c| c.header.name.clone())
        .collect();
    let mut ancestors = BTreeMap::<String, BTreeSet<String>>::new();
    let mut private_owners = BTreeMap::<String, Vec<(String, String)>>::new();
    for name in &order {
        let c = &classes[name];
        let mut chain = c
            .base
            .as_ref()
            .and_then(|b| ancestors.get(b))
            .cloned()
            .unwrap_or_default();
        chain.insert(name.clone());
        ancestors.insert(name.clone(), chain);
        let mut private = c
            .base
            .as_ref()
            .and_then(|b| private_owners.get(b))
            .cloned()
            .unwrap_or_default();
        if c.header.script.language == ScriptLanguage::ActionScript2 {
            private.extend(
                c.fields
                    .iter()
                    .filter(|f| f.access == Access::Private)
                    .map(|f| (f.name.clone(), name.clone())),
            );
            private.extend(
                c.methods
                    .iter()
                    .filter(|m| m.access == Access::Private)
                    .map(|m| (m.function.name.clone().unwrap(), name.clone())),
            );
        }
        private_owners.insert(name.clone(), private);
    }
    let as2_private: BTreeMap<_, _> = private_owners
        .iter()
        .filter(|(_, members)| !members.is_empty())
        .map(|(name, members)| {
            (
                name.clone(),
                members.iter().map(|(name, _)| name.clone()).collect(),
            )
        })
        .collect();
    let mut resolved = BTreeMap::<String, Vec<MemberRecord>>::new();
    let mut class_contracts =
        BTreeMap::<String, Vec<((String, MethodKind), MethodContract)>>::new();
    {
        let acc_owner = "flash.accessibility.AccessibilityImplementation";
        let acc_method = |name: &str, params: &[(&str, bool)], returns: &str| {
            (
                (name.to_owned(), MethodKind::Method),
                MethodContract {
                    signature: Signature {
                        params: params.iter().map(|(ty, _)| (*ty).to_owned()).collect(),
                        optional: params.iter().map(|(_, opt)| *opt).collect(),
                        rest: false,
                        returns: returns.to_owned(),
                    },
                    access: Access::Public,
                    owner: acc_owner.to_owned(),
                    final_: false,
                },
            )
        };
        let acc_contracts = vec![
            acc_method("accDoDefaultAction", &[("uint", false)], "void"),
            acc_method("accLocation", &[("uint", false)], "*"),
            acc_method("accSelect", &[("uint", false), ("uint", false)], "void"),
            acc_method("getChildIDArray", &[], "Array"),
            acc_method("get_accDefaultAction", &[("uint", false)], "String"),
            acc_method("get_accFocus", &[], "uint"),
            acc_method("get_accName", &[("uint", false)], "String"),
            acc_method("get_accRole", &[("uint", false)], "uint"),
            acc_method("get_accSelection", &[], "Array"),
            acc_method("get_accState", &[("uint", false)], "uint"),
            acc_method("get_accValue", &[("uint", false)], "String"),
            acc_method("isLabeledBy", &[("flash.geom.Rectangle", false)], "Boolean"),
            acc_method("get_selectionActiveIndex", &[], "*"),
            acc_method("get_selectionAnchorIndex", &[], "*"),
        ];
        let mut acc_members = vec![
            MemberRecord {
                name: "errno".into(),
                namespace: None,
                ty: Some("uint".into()),
                access: Access::Public,
                owner: acc_owner.into(),
                is_static: false,
            },
            MemberRecord {
                name: "stub".into(),
                namespace: None,
                ty: Some("Boolean".into()),
                access: Access::Public,
                owner: acc_owner.into(),
                is_static: false,
            },
        ];
        for ((mname, _), _) in &acc_contracts {
            acc_members.push(MemberRecord {
                name: mname.clone(),
                namespace: None,
                ty: Some("Function".into()),
                access: Access::Public,
                owner: acc_owner.into(),
                is_static: false,
            });
        }
        resolved.insert(acc_owner.into(), acc_members);
        class_contracts.insert(acc_owner.into(), acc_contracts);
    }
    let html_host_contract = |method_name: &str, kind: MethodKind, params: &[&str], ret: &str| {
        (
            (method_name.to_owned(), kind),
            MethodContract {
                signature: Signature {
                    params: params.iter().map(|s| (*s).to_owned()).collect(),
                    optional: vec![false; params.len()],
                    rest: false,
                    returns: ret.to_owned(),
                },
                access: Access::Public,
                owner: "flash.html.HTMLHost".to_owned(),
                final_: false,
            },
        )
    };
    class_contracts.insert(
        "flash.html.HTMLHost".to_owned(),
        vec![
            html_host_contract(
                "createWindow",
                MethodKind::Method,
                &["flash.html.HTMLWindowCreateOptions"],
                "flash.html.HTMLLoader",
            ),
            html_host_contract("updateLocation", MethodKind::Method, &["String"], "void"),
            html_host_contract("updateStatus", MethodKind::Method, &["String"], "void"),
            html_host_contract("updateTitle", MethodKind::Method, &["String"], "void"),
            html_host_contract("windowBlur", MethodKind::Method, &[], "void"),
            html_host_contract("windowClose", MethodKind::Method, &[], "void"),
            html_host_contract("windowFocus", MethodKind::Method, &[], "void"),
            html_host_contract("windowRect", MethodKind::Get, &[], "flash.geom.Rectangle"),
            html_host_contract(
                "windowRect",
                MethodKind::Set,
                &["flash.geom.Rectangle"],
                "void",
            ),
        ],
    );
    let mut native_bases = BTreeMap::<String, String>::new();
    let mut contracts = builtin_interfaces.clone();
    let mut interface_closures: BTreeMap<String, BTreeSet<String>> = builtin_interfaces
        .keys()
        .map(|name| (name.clone(), BTreeSet::from([name.clone()])))
        .collect();
    let mut reflection_types = BUILTINS
        .iter()
        .filter(|name| {
            !name.starts_with("mx.transitions.")
                && (!AS2_BUILTINS.contains(name)
                    || matches!(
                        **name,
                        "flash.net.FileReference" | "flash.net.FileReferenceList"
                    ))
        })
        .copied()
        .collect::<Vec<_>>();
    reflection_types.extend([
        "Date",
        "RegExp",
        "JSON",
        "XML",
        "XMLList",
        "Namespace",
        "QName",
    ]);
    let mapping_prefix = crate::sourcemap::marker_prefix(
        &classes
            .values()
            .map(|class| class.header.script.source.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let mut registrations = format!("__nfProgram.setNativeTypes({reflection_types:?});\n");
    for (ns_name, ns_uri) in &namespace_uris {
        registrations.push_str(&format!(
            "if(typeof __nfProgram.defineNamespace==='function')__nfProgram.defineNamespace({ns_name:?},{ns_uri:?});\n"
        ));
    }
    for name in &order {
        let c = classes.get_mut(name).unwrap();
        let as2 = c.header.script.language == ScriptLanguage::ActionScript2;
        c.context.as2_private = private_owners
            .iter()
            .filter_map(|(receiver, members)| {
                let inaccessible: BTreeSet<_> = members
                    .iter()
                    .filter(|(_, owner)| !ancestors[name].contains(owner))
                    .map(|(member, _)| member.clone())
                    .collect();
                (!inaccessible.is_empty()).then(|| (receiver.clone(), inaccessible))
            })
            .collect();
        if c.base
            .as_ref()
            .is_some_and(|base| final_classes.contains(base))
        {
            errors.push(Diagnostic::at(
                &c.header.script,
                0,
                format!(
                    "{}: cannot extend final class '{}'",
                    c.header.path,
                    c.base.as_ref().unwrap()
                ),
            ));
        }
        if c.header.is_interface {
            let mut inherited = BTreeMap::new();
            let mut closure = BTreeSet::from([name.clone()]);
            for parent in &c.interfaces {
                closure.extend(interface_closures[parent].iter().cloned());
                for (key, contract) in &contracts[parent] {
                    if inherited
                        .get(key)
                        .is_some_and(|old: &MethodContract| old.signature != contract.signature)
                    {
                        errors.push(Diagnostic::at(
                            &c.header.script,
                            0,
                            format!(
                                "{}: conflicting inherited interface signature '{}'",
                                c.header.path, key.0
                            ),
                        ));
                    }
                    inherited.insert(key.clone(), contract.clone());
                }
            }
            for method in &c.methods {
                let key = (method.function.name.clone().unwrap(), method.kind);
                let contract = method_contract(c, method);
                if inherited
                    .get(&key)
                    .is_some_and(|old| old.signature != contract.signature)
                {
                    errors.push(Diagnostic::at(
                        &c.header.script,
                        method.function.offset,
                        format!(
                            "{}: incompatible interface signature '{}'",
                            c.header.path, key.0
                        ),
                    ));
                }
                inherited.insert(key, contract);
            }
            check_accessor_pairs(c, &inherited, &mut errors);
            contracts.insert(name.clone(), inherited);
            interface_closures.insert(name.clone(), closure);
            registrations.push_str(&format!(
                "__nfProgram.defineInterface({name:?},{});\n",
                reflection_metadata(c)
            ));
            continue;
        }
        let inherited_history = c
            .base
            .as_ref()
            .and_then(|base| class_contracts.get(base))
            .cloned()
            .unwrap_or_default();
        let mut contract: BTreeMap<_, _> = inherited_history
            .iter()
            .filter(|(_, m)| m.access.inherited(&m.owner, &c.header.package, as2))
            .cloned()
            .collect();
        let inherited_contract = contract.clone();
        let native_base = c
            .base
            .as_ref()
            .map(|base| native_bases.get(base).unwrap_or(base).clone());
        if let Some(base) = &native_base {
            native_bases.insert(name.clone(), base.clone());
            if base == "flash.events.EventDispatcher" || base.starts_with("flash.display.") {
                for (key, item) in &builtin_interfaces["flash.events.IEventDispatcher"] {
                    contract.entry(key.clone()).or_insert_with(|| item.clone());
                }
            }
            for member in c
                .fields
                .iter()
                .filter(|f| !f.is_static && f.namespace.is_none())
                .map(|f| f.name.as_str())
                .chain(
                    c.methods
                        .iter()
                        .filter(|m| !m.is_static && m.namespace.is_none())
                        .filter_map(|m| m.function.name.as_deref()),
                )
            {
                if !c.header.intrinsic && native_member(base, member) {
                    errors.push(Diagnostic::at(&c.header.script, 0, format!("{}: redeclaring native member '{member}' is not supported by this class profile", c.header.path)));
                }
            }
        }
        let mut member_history = c
            .base
            .as_ref()
            .and_then(|base| resolved.get(base))
            .cloned()
            .unwrap_or_default();
        let inherited_records: BTreeMap<_, _> = member_history
            .iter()
            .filter(|m| {
                (m.namespace.is_none() && m.access.inherited(&m.owner, &c.header.package, as2))
                    || m.namespace
                        .as_ref()
                        .is_some_and(|ns| c.context.open_namespaces.contains(ns))
            })
            .map(|m| (m.name.clone(), m.clone()))
            .collect();
        let mut members: BTreeMap<_, _> = inherited_records
            .iter()
            .map(|(name, m)| (name.clone(), m.ty.clone()))
            .collect();
        let inherited_members = members.clone();
        for f in &c.fields {
            if f.namespace.is_none()
                && inherited_records
                    .get(&f.name)
                    .is_some_and(|m| !m.is_static && !f.is_static)
            {
                errors.push(Diagnostic::at(
                    &c.header.script,
                    0,
                    format!(
                        "{}: field '{}' redeclares an accessible inherited member",
                        c.header.path, f.name
                    ),
                ));
            }
            if let Some(ns) = &f.namespace {
                c.context
                    .namespace_members
                    .insert((ns.clone(), f.name.clone()));
            }
            if f.namespace.is_none()
                || c.context
                    .open_namespaces
                    .iter()
                    .any(|u| Some(u) == f.namespace.as_ref())
            {
                members
                    .entry(f.name.clone())
                    .or_insert_with(|| f.ty.clone());
            }
            member_history.push(MemberRecord {
                name: f.name.clone(),
                ty: f.ty.clone(),
                access: f.access,
                namespace: f.namespace.clone(),
                owner: name.clone(),
                is_static: f.is_static,
            });
        }
        let short = c.header.name.rsplit('.').next().unwrap();
        let is_proxy_subclass = matches!(
            native_base.as_deref(),
            Some("flash.utils.Proxy" | "mx.utils.ObjectProxy")
        );
        if is_proxy_subclass {
            const PROXY_NS_URIS: &[&str] = &[
                "http://www.adobe.com/2006/actionscript/flash/proxy",
                "http://www.adobe.com/2006/flex/mx/internal",
            ];
            const PROXY_HOOK_NAMES: &[&str] = &[
                "getProperty",
                "setProperty",
                "callProperty",
                "hasProperty",
                "deleteProperty",
                "nextNameIndex",
                "nextName",
                "nextValue",
                "getDescendants",
                "isAttribute",
            ];
            let has_open_proxy_ns = c
                .context
                .open_namespaces
                .iter()
                .any(|u| PROXY_NS_URIS.contains(&u.as_str()));
            for &hook in PROXY_HOOK_NAMES {
                for &uri in PROXY_NS_URIS {
                    c.context
                        .namespace_members
                        .insert((uri.to_owned(), hook.to_owned()));
                }
                if hook == "isAttribute" || has_open_proxy_ns {
                    members
                        .entry(hook.to_owned())
                        .or_insert_with(|| Some("Function".into()));
                }
            }
            if native_bases.get(name).map(String::as_str) == Some("mx.utils.ObjectProxy") {
                for &prop in &[
                    "object",
                    "uid",
                    "type",
                    "propertyList",
                    "notifiers",
                    "dispatcher",
                ] {
                    c.context.namespace_members.insert((
                        "http://www.adobe.com/2006/flex/mx/internal".to_owned(),
                        prop.to_owned(),
                    ));
                    if c.context
                        .open_namespaces
                        .iter()
                        .any(|u| u == "http://www.adobe.com/2006/flex/mx/internal")
                    {
                        members.entry(prop.to_owned()).or_insert(None);
                    }
                }
            }
        }
        for m in &c.methods {
            let n = m.function.name.as_ref().unwrap();
            if n != short || m.kind != MethodKind::Method {
                let trait_key = if let Some(ns) = &m.namespace {
                    format!("{ns}::{n}")
                } else {
                    n.clone()
                };
                let key = (trait_key.clone(), m.kind);
                let own_contract = method_contract(c, m);
                let inherited = if m.is_static {
                    None
                } else {
                    inherited_contract.get(&key)
                };
                let is_native_proxy_hook = is_proxy_subclass
                    && matches!(
                        m.namespace.as_deref(),
                        Some(
                            "http://www.adobe.com/2006/actionscript/flash/proxy"
                                | "http://www.adobe.com/2006/flex/mx/internal"
                        )
                    )
                    && matches!(
                        n.as_str(),
                        "getProperty"
                            | "setProperty"
                            | "callProperty"
                            | "hasProperty"
                            | "deleteProperty"
                            | "nextNameIndex"
                            | "nextName"
                            | "nextValue"
                            | "getDescendants"
                            | "isAttribute"
                    );
                let same_name = inherited_contract.keys().any(|(name, kind)| {
                    name == &trait_key
                        && (*kind == MethodKind::Method || m.kind == MethodKind::Method)
                });
                if m.override_ && inherited.is_none() && !is_native_proxy_hook {
                    errors.push(Diagnostic::at(
                        &c.header.script,
                        m.function.offset,
                        format!(
                            "{}: override target '{n}' not found in a project base",
                            c.header.path
                        ),
                    ));
                }
                if c.header.script.language == ScriptLanguage::ActionScript3 {
                    if let Some(base) = inherited {
                        if !m.override_
                            || base.signature != own_contract.signature
                            || base.access != own_contract.access
                            || base.final_
                        {
                            errors.push(Diagnostic::at(&c.header.script, m.function.offset, format!("{}: inherited member '{n}' requires a non-final base, override and an identical access/signature", c.header.path)));
                        }
                    } else if !m.is_static
                        && m.namespace.is_none()
                        && (same_name
                            || inherited_members.contains_key(n)
                                && !inherited_contract.keys().any(|(name, _)| name == n))
                    {
                        errors.push(Diagnostic::at(
                            &c.header.script,
                            m.function.offset,
                            format!(
                                "{}: member '{n}' conflicts with an inherited field or method kind",
                                c.header.path
                            ),
                        ));
                    }
                }
                if !m.is_static {
                    contract.insert(key, own_contract);
                }
                // Accessor invocation owns coercion. A second emitter coercion
                // on a bare property would repeat observable conversions.
                member_history.push(MemberRecord {
                    name: n.clone(),
                    ty: if m.kind == MethodKind::Method {
                        Some("Function".into())
                    } else {
                        None
                    },
                    access: m.access,
                    namespace: m.namespace.clone(),
                    owner: name.clone(),
                    is_static: m.is_static,
                });
                if let Some(ns) = &m.namespace {
                    c.context.namespace_members.insert((ns.clone(), n.clone()));
                }
                if m.namespace.is_none()
                    || c.context
                        .open_namespaces
                        .iter()
                        .any(|u| Some(u) == m.namespace.as_ref())
                {
                    members.entry(n.clone()).or_insert_with(|| {
                        if m.kind == MethodKind::Method {
                            Some("Function".into())
                        } else {
                            None
                        }
                    });
                }
            }
        }
        check_accessor_pairs(c, &contract, &mut errors);
        let statics_contract = c
            .methods
            .iter()
            .filter(|m| m.is_static)
            .map(|m| {
                let n = m.function.name.as_ref().unwrap();
                let trait_key = if let Some(ns) = &m.namespace {
                    format!("{ns}::{n}")
                } else {
                    n.clone()
                };
                ((trait_key, m.kind), method_contract(c, m))
            })
            .collect();
        check_accessor_pairs(c, &statics_contract, &mut errors);
        for getter in c.methods.iter().filter(|m| m.kind == MethodKind::Get) {
            if c.methods.iter().any(|m| {
                m.kind == MethodKind::Set
                    && m.function.name == getter.function.name
                    && m.namespace == getter.namespace
                    && m.is_static != getter.is_static
            }) {
                errors.push(Diagnostic::at(
                    &c.header.script,
                    getter.function.offset,
                    format!("{}: getter and setter must agree on static", c.header.path),
                ));
            }
        }
        let mut implemented = BTreeSet::new();
        for interface in &c.interfaces {
            implemented.extend(interface_closures[interface].iter().cloned());
            for (key, expected) in &contracts[interface] {
                if !contract.get(key).is_some_and(|actual| {
                    actual.access == Access::Public && actual.signature == expected.signature
                }) {
                    errors.push(Diagnostic::at(&c.header.script, 0, format!("{}: interface '{interface}' requires public member '{}' with the matching method/accessor signature", c.header.path, key.0)));
                }
            }
        }
        c.interfaces = implemented.into_iter().collect();
        contracts.insert(name.clone(), contract);
        let mut history = inherited_history;
        history.extend(
            c.methods
                .iter()
                .filter(|m| {
                    !m.is_static
                        && (m.function.name.as_deref() != Some(short)
                            || m.kind != MethodKind::Method)
                })
                .map(|m| {
                    let n = m.function.name.as_ref().unwrap();
                    let trait_key = if let Some(ns) = &m.namespace {
                        format!("{ns}::{n}")
                    } else {
                        n.clone()
                    };
                    ((trait_key, m.kind), method_contract(c, m))
                }),
        );
        class_contracts.insert(name.clone(), history);
        c.context.static_members = member_history
            .iter()
            .filter(|m| {
                m.is_static
                    && (m.namespace.is_none()
                        || c.context
                            .open_namespaces
                            .iter()
                            .any(|u| Some(u) == m.namespace.as_ref()))
                    && (m.owner == *name || m.access.inherited(&m.owner, &c.header.package, as2))
            })
            .map(|m| (m.name.clone(), m.owner.clone()))
            .collect();
        c.context.members = members;
        resolved.insert(name.clone(), member_history);
        c.context.mapping_prefix = Some(format!("{mapping_prefix}class{}:", resolved.len()));
        match emit_class(c) {
            Ok(code) => registrations.push_str(&code),
            Err(e) => errors.push(prefix(e, &c.header.path)),
        }
    }
    for linked in project
        .document_class
        .iter()
        .chain(project.symbols.iter().filter_map(|s| s.class_name.as_ref()))
    {
        if !classes
            .get(linked)
            .is_some_and(|class| !class.header.is_interface)
        {
            errors.push(Diagnostic::new(
                ScriptLanguage::ActionScript3,
                1,
                1,
                format!("Unknown linked project class '{linked}'."),
            ));
        }
    }
    for symbol in project
        .symbols
        .iter()
        .filter(|symbol| symbol.linkage_id.is_some())
    {
        let required_base = if symbol.kind == neoflash_core::SymbolKind::Button {
            "flash.display.SimpleButton"
        } else {
            "flash.display.MovieClip"
        };
        if let Some(class) = &symbol.class_name
            && native_bases.get(class).map(String::as_str) != Some(required_base)
        {
            errors.push(Diagnostic::new(classes.get(class).map_or(ScriptLanguage::ActionScript3, |class| class.header.script.language), 1, 1, format!("Exported symbol '{}' requires class '{class}' to ultimately extend {required_base}.",symbol.name)));
        }
    }
    let mut init_clips = Vec::new();
    for (symbol_idx, symbol) in project.symbols.iter().enumerate() {
        if symbol.kind != neoflash_core::SymbolKind::MovieClip {
            continue;
        }
        for (script_idx, entry) in symbol.timeline.scripts.iter().enumerate() {
            if entry.frame != 0
                || !matches!(
                    entry.script.language,
                    ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2
                )
                || (!entry.script.source.contains("#initclip")
                    && !has_include_directive(&entry.script))
            {
                continue;
            }
            if entry.script.source.len() > 1_048_576 {
                errors.push(Diagnostic::at(
                    &entry.script,
                    0,
                    "A single script is limited to 1 MiB.",
                ));
                continue;
            }
            let is_as1 = entry.script.language == ScriptLanguage::ActionScript1;
            let expanded = if has_include_directive(&entry.script) {
                match super::includes::expand(&entry.script, &includes) {
                    Ok(exp) => Some(exp),
                    Err(e) => {
                        errors.push(e);
                        continue;
                    }
                }
            } else {
                None
            };
            let source_script = expanded.as_ref().map_or(&entry.script, |exp| &exp.script);
            if !source_script.source.contains("#initclip") {
                continue;
            }
            let map_err = |mut err: Diagnostic| {
                if is_as1 {
                    err.language = ScriptLanguage::ActionScript1;
                }
                expanded
                    .as_ref()
                    .map_or(err.clone(), |exp| exp.diagnostic(err))
            };
            let (imported, end) = if is_as1 {
                (Vec::new(), 0)
            } else {
                let mut p = frontend(
                    source_script,
                    TokenCursor::default(),
                    CompileContext::default(),
                );
                let imported = match imports(&mut p) {
                    Ok(v) => v,
                    Err(e) => {
                        errors.push(map_err(e));
                        continue;
                    }
                };
                (imported, p.token().offset)
            };
            let mut c = match context(
                &visible_names(
                    &names,
                    &languages,
                    ScriptLanguage::ActionScript2,
                    &public_types,
                    "",
                ),
                "",
                &imported,
                ScriptLanguage::ActionScript2,
                &interfaces,
                &namespace_uris,
            ) {
                Ok(c) => c,
                Err(m) => {
                    errors.push(map_err(Diagnostic::at(source_script, 0, m)));
                    continue;
                }
            };
            c.legacy_case = is_as1;
            c.as2_private = as2_private.clone();
            let mut replaced = source_script.clone();
            replaced.language = ScriptLanguage::ActionScript2;
            replaced.source = source_script.source[..end]
                .bytes()
                .map(|ch| {
                    if ch == b'\n' || ch == b'\r' {
                        ch as char
                    } else {
                        ' '
                    }
                })
                .collect::<String>()
                + &source_script.source[end..];
            let mut body_parser = frontend(&replaced, TokenCursor::default(), c.clone());
            let statements = match body_parser.statements(false) {
                Ok(s) => s,
                Err(e) => {
                    errors.push(map_err(e));
                    continue;
                }
            };
            let analysis = match scope::Analysis::with_context(&replaced, &statements, c) {
                Ok(a) => a,
                Err(e) => {
                    errors.push(map_err(e));
                    continue;
                }
            };
            for (stmt_idx, stmt) in statements.iter().enumerate() {
                if let Stmt::InitClip(order_num, body, id) = stmt.unlocated() {
                    let key = format!("{:016x}:{id}", super::fnv1a64(&replaced.source));
                    let mut body_code = Emitter {
                        as3: false,
                        analysis: &analysis,
                        scope: 0,
                        with_targets: Vec::new(),
                        tell_target: None,
                    }
                    .body(body);
                    if is_as1 {
                        body_code = format!("const __nfLegacyCase = true;\n{body_code}");
                    }
                    init_clips.push((
                        (
                            order_num.unwrap_or(0),
                            order_num.is_some(),
                            symbol_idx,
                            script_idx,
                            stmt_idx,
                        ),
                        key,
                        body_code,
                    ));
                }
            }
        }
    }
    init_clips.sort_by_key(|a| a.0);
    for (_, key, body_code) in init_clips {
        registrations.push_str(&format!(
            "__nfProgram.registerInitClip({key:?}, function(__nfInitRoot){{ const _root=__nfInitRoot, _parent=null, trace=__nfTrace, play=__nfInitRoot.play, stop=__nfInitRoot.stop, gotoAndPlay=__nfInitRoot.gotoAndPlay, gotoAndStop=__nfInitRoot.gotoAndStop, nextFrame=__nfInitRoot.nextFrame, prevFrame=__nfInitRoot.prevFrame, __nfFrame=__nfInitRoot;\n(function(){{\n{body_code}\n}}).call(__nfInitRoot);\n}});\n"
        ));
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let code = format!(
        "{}\n{}\n{}\nif(typeof __nfBuiltins.__nfGeometryNumberVector==='function')__nfBuiltins.__nfGeometryNumberVector(values=>__nfVectorType(__nfPrimitiveDescriptor('Number'))(values));\nif(typeof __nfBuiltins.__nfGeometryVector3DVector==='function')__nfBuiltins.__nfGeometryVector3DVector(values=>__nfVectorType(__nfProgram.elementDescriptor('flash.geom.Vector3D'))(values));\nif(typeof __nfBuiltins.__nfDateStringVector==='function')__nfBuiltins.__nfDateStringVector(values=>__nfVectorType(__nfPrimitiveDescriptor('String'))(values));\nif(typeof __nfBuiltins.__nfBitmapUintVector==='function')__nfBuiltins.__nfBitmapUintVector(values=>__nfVectorType(__nfPrimitiveDescriptor('uint'))(values));\nif(typeof __nfBuiltins.__nfBitmapHistogramVector==='function')__nfBuiltins.__nfBitmapHistogramVector(channels=>__nfVectorType(__nfVectorDescriptor(__nfPrimitiveDescriptor('Number')))(channels.map(ch=>__nfVectorType(__nfPrimitiveDescriptor('Number'))(ch))));\nif(typeof __nfBuiltins.__nfGraphicsVectors==='function')__nfBuiltins.__nfGraphicsVectors({{intVector:values=>__nfVectorType(__nfPrimitiveDescriptor('int'))(values),numberVector:values=>__nfVectorType(__nfPrimitiveDescriptor('Number'))(values),dataVector:values=>__nfVectorType(__nfProgram.elementDescriptor('flash.display.IGraphicsData'))(values)}});\nif(typeof __nfBuiltins.__nfMouseCursorVector==='function')__nfBuiltins.__nfMouseCursorVector(values=>__nfVectorType(__nfProgram.elementDescriptor('flash.display.BitmapData'))(values));\nif(typeof __nfBuiltins.__nfAirVectorFactory==='function')__nfBuiltins.__nfAirVectorFactory((typeName,values)=>__nfVectorType(['Number','String','Boolean','int','uint','Object','*'].includes(typeName)?__nfPrimitiveDescriptor(typeName):__nfProgram.elementDescriptor(typeName))(values));\nif(typeof __nfBuiltins.__nfAs3NetSysVectorFactory==='function')__nfBuiltins.__nfAs3NetSysVectorFactory((typeName,values)=>__nfVectorType(['Number','String','Boolean','int','uint','Object','*'].includes(typeName)?__nfPrimitiveDescriptor(typeName):__nfProgram.elementDescriptor(typeName))(values));\nif(typeof __nfBuiltins.__nfFte3dVectorFactory==='function')__nfBuiltins.__nfFte3dVectorFactory((typeName,values)=>__nfVectorType(['Number','String','Boolean','int','uint','Object','*'].includes(typeName)?__nfPrimitiveDescriptor(typeName):__nfProgram.elementDescriptor(typeName))(values));\nif(typeof __nfBuiltins.__nfMediaEventsVectorFactory==='function')__nfBuiltins.__nfMediaEventsVectorFactory((typeName,values)=>__nfVectorType(['Number','String','Boolean','int','uint','Object','*'].includes(typeName)?__nfPrimitiveDescriptor(typeName):__nfProgram.elementDescriptor(typeName))(values));\n__nfProgram.initializeStatics();\nreturn __nfProgram;",
        embedded_helpers(),
        include_str!("project-runtime.js"),
        registrations
    );
    // Common intrinsic captures precede helper declarations.
    let code = format!(
        "const __nfNumber=Number,__nfString=String,__nfBoolean=Boolean,__nfTypeError=TypeError;\nconst __nfAs2Method=__nfBuiltins.__nfAs2Method,__nfAs2Value=__nfBuiltins.__nfAs2Value,__nfDisplay=__nfBuiltins.__nfDisplay,__nfLegacy=__nfBuiltins.__nfLegacy,__nfXML=__nfBuiltins.__nfXML;\nconst __nfTrace=(...args)=>{{if(typeof __nfBuiltins.__nfTrace !== 'function')throw new Error('Project trace host binding is missing');return __nfBuiltins.__nfTrace(...args);}};\n{code}"
    );
    let mut source_maps = BTreeMap::new();
    for c in classes.values() {
        let Some(marker) = &c.context.mapping_prefix else {
            continue;
        };
        let mut map = crate::sourcemap::map_marked_source(
            &c.header.path,
            &c.header.script.source,
            &code,
            marker,
        );
        if let Some(unit) = project
            .sources
            .iter()
            .find(|unit| unit.path == c.header.path)
        {
            let raw = Script {
                language: unit.language,
                source: unit.source.clone(),
            };
            if let Ok(expanded) = super::includes::expand_at(&raw, &unit.path, &includes) {
                map = remap_includes(map, &expanded, &unit.path, &raw.source);
            }
        }
        source_maps.insert(c.header.path.clone(), map);
    }
    Ok(ProjectProgram {
        includes,
        source_maps,
        code,
        class_names: order
            .into_iter()
            .filter(|name| {
                !interfaces.contains(name)
                    && !classes
                        .get(name)
                        .is_some_and(|class| class.header.intrinsic)
            })
            .collect(),
        names,
        languages,
        interfaces,
        public_types,
        namespace_uris,
        as2_private,
    })
}

fn has_include_directive(script: &Script) -> bool {
    match script.language {
        ScriptLanguage::ActionScript1 | ScriptLanguage::ActionScript2 => {
            script.source.contains("#include")
        }
        ScriptLanguage::ActionScript3 => {
            script.source.contains("include ")
                || script.source.contains("include\"")
                || script.source.contains("include'")
                || script.source.contains("#include")
        }
        _ => false,
    }
}

pub fn compile_with_program(
    script: &Script,
    program: &ProjectProgram,
) -> Result<String, Vec<Diagnostic>> {
    if script.source.len() > 1_048_576 {
        return Err(vec![Diagnostic::at(
            script,
            0,
            "A single script is limited to 1 MiB.",
        )]);
    }
    if has_include_directive(script) {
        let expanded = super::includes::expand(script, &program.includes).map_err(|e| vec![e])?;
        compile_prepared(&expanded.script, program, None)
            .map_err(|errors| errors.into_iter().map(|e| expanded.diagnostic(e)).collect())
    } else {
        compile_prepared(script, program, None)
    }
}
pub(crate) fn compile_with_program_mapped(
    script: &Script,
    program: &ProjectProgram,
    source_name: &str,
) -> Result<(String, crate::SourceMap), Vec<Diagnostic>> {
    // Mapping must not normalize untouched source: #initclip identity is shared with
    // the program bootstrap and includes the authored source bytes.
    if !has_include_directive(script) {
        let prefix = crate::sourcemap::marker_prefix(&script.source);
        let code = compile_prepared(script, program, Some(prefix.clone()))?;
        let map = crate::sourcemap::map_marked_source(source_name, &script.source, &code, &prefix);
        return Ok((code, map));
    }

    let expanded = super::includes::expand(script, &program.includes).map_err(|e| vec![e])?;
    let prefix = crate::sourcemap::marker_prefix(&expanded.script.source);
    let code =
        compile_prepared(&expanded.script, program, Some(prefix.clone())).map_err(|errors| {
            errors
                .into_iter()
                .map(|e| expanded.diagnostic(e))
                .collect::<Vec<_>>()
        })?;
    let map =
        crate::sourcemap::map_marked_source(source_name, &expanded.script.source, &code, &prefix);
    Ok((
        code,
        remap_includes(map, &expanded, source_name, &script.source),
    ))
}

fn remap_includes(
    mut map: crate::SourceMap,
    expanded: &super::includes::Expanded,
    source_name: &str,
    source: &str,
) -> crate::SourceMap {
    let mut segments = map.decode_segments();
    map.sources = vec![source_name.to_owned()];
    map.sources_content = vec![source.to_owned()];
    for segment in &mut segments {
        if let Some((path, line)) = expanded.origins.get(segment.original_line) {
            let name = if path.is_empty() { source_name } else { path };
            let index = map
                .sources
                .iter()
                .position(|s| s == name)
                .unwrap_or_else(|| {
                    map.sources.push(name.to_owned());
                    map.sources_content
                        .push(expanded.sources.get(path).cloned().unwrap_or_default());
                    map.sources.len() - 1
                });
            segment.source_index = index;
            segment.original_line = line.saturating_sub(1);
        }
    }
    crate::SourceMap::from_segments(
        map.file,
        map.sources,
        map.sources_content,
        map.names,
        segments,
    )
}

fn apply_as3_timeline_auto_imports(
    c: &mut CompileContext,
    visible: &BTreeSet<String>,
    languages: &BTreeMap<String, ScriptLanguage>,
) {
    // Match Adobe Flash CS3+ timeline frame scripts, which implicitly wildcard-import
    // standard flash.* packages while letting top-level project classes and explicit
    // or wildcard user imports take precedence.
    let mut counts = BTreeMap::<&str, usize>::new();
    for name in visible {
        if !languages.contains_key(name) && name.starts_with("flash.") {
            let short = name.rsplit('.').next().unwrap_or(name);
            *counts.entry(short).or_insert(0) += 1;
        }
    }
    for name in visible {
        if !languages.contains_key(name) && name.starts_with("flash.") {
            let short = name.rsplit('.').next().unwrap_or(name);
            if counts.get(short) == Some(&1)
                && !c.types.contains_key(short)
                && !c.functions.contains_key(short)
            {
                if let Some(uri) = builtin_namespace_uri(name) {
                    c.namespaces
                        .entry(short.into())
                        .or_insert_with(|| uri.into());
                } else if BUILTIN_FUNCTIONS.contains(&name.as_str()) {
                    c.functions.insert(short.into(), name.clone());
                } else {
                    c.types.insert(short.into(), name.clone());
                }
            }
        }
    }
}

fn compile_prepared(
    script: &Script,
    program: &ProjectProgram,
    mapping_prefix: Option<String>,
) -> Result<String, Vec<Diagnostic>> {
    if !matches!(
        script.language,
        ScriptLanguage::ActionScript1
            | ScriptLanguage::ActionScript2
            | ScriptLanguage::ActionScript3
    ) {
        return crate::compile(script);
    }
    let is_as1 = script.language == ScriptLanguage::ActionScript1;
    let dialect = if is_as1 {
        ScriptLanguage::ActionScript2
    } else {
        script.language
    };
    let (imported, end) = if is_as1 {
        (Vec::new(), 0)
    } else {
        let tokens = TokenCursor::default();
        let mut p = frontend(script, tokens, CompileContext::default());
        let imported = imports(&mut p).map_err(|e| vec![e])?;
        (imported, p.token().offset)
    };
    let visible = visible_names(
        &program.names,
        &program.languages,
        dialect,
        &program.public_types,
        "",
    );
    let mut c = context(
        &visible,
        "",
        &imported,
        dialect,
        &program.interfaces,
        &program.namespace_uris,
    )
    .map_err(|m| vec![Diagnostic::at(script, 0, m)])?;
    if dialect == ScriptLanguage::ActionScript3 {
        apply_as3_timeline_auto_imports(&mut c, &visible, &program.languages);
    }
    c.mapping_prefix = mapping_prefix;
    c.legacy_case = is_as1;
    c.as2_private = program.as2_private.clone();
    // Keep original byte positions/line numbers when removing import statements.
    let mut replaced = script.clone();
    replaced.language = dialect;
    replaced.source = script.source[..end]
        .bytes()
        .map(|c| {
            if c == b'\n' || c == b'\r' {
                c as char
            } else {
                ' '
            }
        })
        .collect::<String>()
        + &script.source[end..];
    super::compile_context(&replaced, c).map_err(|error| {
        super::diagnostics::collect_errors(&replaced, error)
            .into_iter()
            .map(|mut error| {
                if is_as1 {
                    error.language = ScriptLanguage::ActionScript1;
                }
                error
            })
            .collect()
    })
}
