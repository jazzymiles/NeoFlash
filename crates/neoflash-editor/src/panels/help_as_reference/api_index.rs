use super::{JS_TS_API_ENTRIES, JS_TS_CATEGORIES, JsTsApiEntry, JsTsEntryKind, api_examples};
use crate::code_tools::CodeCommand;
use crate::panels::help_manual::LanguageScope;
use crate::script_api::{self, Member};
use eframe::egui;
use neoflash_core::ScriptLanguage;
use neoflash_ui::Palette;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};

// ── Scripting Reference: API browser model ──
//
// One view model for the three reference languages. ActionScript 2 and 3 come from the generated
// declaration index (`script_api`), JavaScript / TypeScript from `JS_TS_API_ENTRIES`.

/// Pseudo-class of the top-level functions, properties and constants.
pub(crate) const API_GLOBAL_KEY: &str = "<global>";
/// Pseudo-class of operators, statements and directives.
pub(crate) const API_LANGUAGE_KEY: &str = "<language>";

/// Support status of a declaration in NeoFlash (`docs/progress/actionscript-catalog.json`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Support {
    Working,
    Partial,
    Missing,
}

impl Support {
    pub(super) fn of(status: &str) -> Option<Self> {
        match status {
            "subset" => Some(Self::Working),
            "partial" => Some(Self::Partial),
            "missing" => Some(Self::Missing),
            _ => None,
        }
    }
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::Partial => "Partial",
            Self::Missing => "Missing",
        }
    }
    pub(super) fn color(self, p: &Palette) -> egui::Color32 {
        match self {
            Self::Working => p.success_default,
            Self::Partial => p.warning_default,
            Self::Missing => p.text_disabled,
        }
    }
}

/// Member filter of the class column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MemberFilter {
    #[default]
    All,
    Properties,
    Methods,
    Events,
}

/// Group a member is listed under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MemberGroup {
    Properties,
    Methods,
    Events,
    Language,
}

impl MemberGroup {
    pub(super) const ALL: [Self; 4] = [
        Self::Properties,
        Self::Methods,
        Self::Events,
        Self::Language,
    ];
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Properties => "Properties",
            Self::Methods => "Methods",
            Self::Events => "Events",
            Self::Language => "Language elements",
        }
    }
    pub(super) fn passes(self, filter: MemberFilter) -> bool {
        match filter {
            MemberFilter::All => true,
            MemberFilter::Properties => self == Self::Properties,
            MemberFilter::Methods => self == Self::Methods,
            MemberFilter::Events => self == Self::Events,
        }
    }
}

/// Link from a member to another declaration of the same language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ApiLink {
    pub label: String,
    pub class_key: String,
    pub member_key: Option<String>,
}

/// One member of a class (or one JavaScript / TypeScript entry).
#[derive(Clone, Debug)]
pub(crate) struct ApiMember {
    /// Unique inside its class.
    pub key: String,
    pub name: String,
    /// Name as written in code: `play()`, `currentFrame`.
    pub label: String,
    /// `method`, `static constant`, `NeoFlash binding`, …
    pub kind: String,
    pub group: MemberGroup,
    /// Text after the name in the member list.
    pub row_signature: String,
    /// Concise call or access expression shown at the top of the detail column.
    pub usage: String,
    /// Declaration shown in the detail code box, wrapped by parameters.
    pub declaration: String,
    /// `(name, type and default)`.
    pub params: Vec<(String, String)>,
    pub returns: String,
    /// `None` when the catalogue has no status for the declaration.
    pub support: Option<Support>,
    /// Catalogue note (partial) or availability (JavaScript / TypeScript).
    pub note: String,
    pub description: String,
    pub example: String,
    pub insertion: CodeCommand,
    pub see_also: Vec<ApiLink>,
}

/// One class, interface, pseudo-class or JavaScript / TypeScript category.
#[derive(Clone, Debug)]
pub(crate) struct ApiClass {
    pub key: String,
    pub name: String,
    /// `flash.display`, `Top level`, `JavaScript / TypeScript`.
    pub package: String,
    /// Superclass chain, nearest first, with the key of every indexed parent.
    pub chain: Vec<(String, Option<String>)>,
    /// `subset`, `partial`, `members`, or empty when the catalogue has no class status.
    pub status: String,
    pub description: String,
    /// Runnable class-level example shown when no member is selected.
    pub example: String,
    /// What Insert at cursor writes when no member is selected.
    pub insertion: Option<CodeCommand>,
    pub members: Vec<ApiMember>,
}

impl ApiClass {
    pub(crate) fn member(&self, key: &str) -> Option<&ApiMember> {
        self.members.iter().find(|m| m.key == key)
    }
    /// `(working, partial, missing)` among the members that carry a status.
    pub(crate) fn support_counts(&self) -> (usize, usize, usize) {
        let count = |s| self.members.iter().filter(|m| m.support == Some(s)).count();
        (
            count(Support::Working),
            count(Support::Partial),
            count(Support::Missing),
        )
    }
}

pub(crate) struct ApiPackage {
    pub label: String,
    /// `(class key, name)`.
    pub classes: Vec<(String, String)>,
}

pub(crate) struct ApiIndex {
    /// Packages in tree order. `flat` trees (JavaScript / TypeScript) have one unnamed package.
    pub packages: Vec<ApiPackage>,
    pub flat: bool,
    pub classes: HashMap<String, Arc<ApiClass>>,
    pub class_count: usize,
    pub member_count: usize,
    /// Members with a catalogue status, for the coverage report.
    #[cfg_attr(not(test), allow(dead_code))]
    pub status_count: usize,
}

impl ApiIndex {
    pub(crate) fn class(&self, key: &str) -> Option<&Arc<ApiClass>> {
        self.classes.get(key)
    }
    /// Class the browser opens on: `MovieClip` where the language has one.
    pub(crate) fn default_class(&self) -> Option<&str> {
        ["flash.display.MovieClip", "MovieClip"]
            .into_iter()
            .find(|key| self.classes.contains_key(*key))
            .or_else(|| {
                self.packages
                    .iter()
                    .flat_map(|p| &p.classes)
                    .map(|(key, _)| key.as_str())
                    .next()
            })
    }
    pub(crate) fn package_of(&self, class_key: &str) -> Option<&str> {
        self.packages
            .iter()
            .find(|p| p.classes.iter().any(|(key, _)| key == class_key))
            .map(|p| p.label.as_str())
    }
}

/// The language whose declaration index backs `scope`; `None` for JavaScript / TypeScript.
pub(crate) fn scope_script_language(scope: LanguageScope) -> Option<ScriptLanguage> {
    match scope {
        LanguageScope::As1As2 => Some(ScriptLanguage::ActionScript2),
        LanguageScope::As3 => Some(ScriptLanguage::ActionScript3),
        LanguageScope::JsTs | LanguageScope::All => None,
    }
}

/// API index of a reference language. `All` is not a reference language and reads as JavaScript.
pub(crate) fn api_index(scope: LanguageScope) -> &'static ApiIndex {
    static AS2: OnceLock<ApiIndex> = OnceLock::new();
    static AS3: OnceLock<ApiIndex> = OnceLock::new();
    static JS_TS: OnceLock<ApiIndex> = OnceLock::new();
    match scope_script_language(scope) {
        Some(language) => {
            let cell = if language == ScriptLanguage::ActionScript3 {
                &AS3
            } else {
                &AS2
            };
            cell.get_or_init(|| {
                script_api::api(language)
                    .map(|api| build_as_index(api, language))
                    .unwrap_or_else(build_js_ts_index)
            })
        }
        None => JS_TS.get_or_init(build_js_ts_index),
    }
}

/// `flash.filters.flash.filters.BlurFilter` (an ActionScript 2 reference quirk) → `flash.filters.BlurFilter`.
fn canonical_class_key(raw: &str) -> String {
    let package = script_api::package_of(raw);
    let parts: Vec<&str> = package.split('.').collect();
    if parts.len() >= 2 && parts.len().is_multiple_of(2) {
        let half = parts.len() / 2;
        if parts[..half] == parts[half..] {
            return format!(
                "{}.{}",
                parts[..half].join("."),
                script_api::short_name(raw)
            );
        }
    }
    raw.to_owned()
}

fn is_language_kind(kind: &str) -> bool {
    matches!(
        kind,
        "operator" | "statement" | "directive" | "keyword" | "special_type" | "compiler_directive"
    )
}

fn member_group(kind: &str) -> MemberGroup {
    match kind {
        "property" | "constant" => MemberGroup::Properties,
        "method" | "function" | "constructor" => MemberGroup::Methods,
        "event" | "callback" => MemberGroup::Events,
        _ => MemberGroup::Language,
    }
}

/// Splits a documented parameter list at its top-level commas.
pub(super) fn split_params(params: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0_i32;
    let mut quote: Option<char> = None;
    let mut prev: Option<char> = None;
    let mut current = String::new();
    for ch in params.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(ch),
            (None, '(' | '[' | '<' | '{') => depth += 1,
            (None, '>') if prev == Some('=') => {}
            (None, ')' | ']' | '>' | '}') => depth -= 1,
            (None, ',') if depth <= 0 => {
                parts.push(current.trim().to_owned());
                current.clear();
                prev = Some(ch);
                continue;
            }
            _ => {}
        }
        prev = Some(ch);
        current.push(ch);
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_owned());
    }
    parts
}

/// `(name, description)` of one documented parameter: `scene:String = null` →
/// `("scene", "String, optional (default null)")`.
pub(super) fn describe_param(raw: &str) -> (String, String) {
    let mut text = raw.trim();
    let bracketed = text.starts_with('[') && text.ends_with(']');
    if bracketed {
        text = text[1..text.len() - 1].trim();
    }
    if let Some(rest) = text.strip_prefix("...") {
        let name = rest.trim().split(':').next().unwrap_or("").trim();
        let name = if name.is_empty() { "rest" } else { name };
        return (
            format!("...{name}"),
            "Any number of further arguments".into(),
        );
    }
    let (declared, default) = match text.split_once('=') {
        Some((declared, default)) => (declared.trim(), Some(default.trim())),
        None => (text, None),
    };
    let (name, ty) = match declared.split_once(':') {
        Some((name, ty)) => (name.trim(), ty.trim()),
        None => (declared, ""),
    };
    let mut description = if ty.is_empty() {
        String::new()
    } else {
        ty.to_owned()
    };
    match default {
        Some(default) => {
            if !description.is_empty() {
                description.push_str(", ");
            }
            let _ = write!(description, "optional (default {default})");
        }
        None if bracketed => {
            if !description.is_empty() {
                description.push_str(", ");
            }
            description.push_str("optional");
        }
        None => {}
    }
    (name.to_owned(), description)
}

fn lower_first(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let upper_run = chars.iter().take_while(|c| c.is_ascii_uppercase()).count();
    let lowered: String = if upper_run == chars.len() {
        chars.iter().map(|c| c.to_ascii_lowercase()).collect()
    } else if upper_run > 1 {
        chars[..upper_run - 1]
            .iter()
            .map(|c| c.to_ascii_lowercase())
            .chain(chars[upper_run - 1..].iter().copied())
            .collect()
    } else {
        chars[0]
            .to_lowercase()
            .chain(chars[1..].iter().copied())
            .collect()
    };
    match lowered.as_str() {
        "function" => "fn".into(),
        "class" => "cls".into(),
        "namespace" => "ns".into(),
        "package" => "pkg".into(),
        "default" => "defaultValue".into(),
        "int" => "intVal".into(),
        "uint" => "uintVal".into(),
        "void" => "voidVal".into(),
        _ => lowered,
    }
}

/// Leading word of a camel-case name: `gotoAndPlay` → `goto`, `_xscale` → `_xscale`.
fn leading_word(name: &str) -> &str {
    let end = name
        .char_indices()
        .skip(1)
        .find(|(_, c)| c.is_uppercase())
        .map_or(name.len(), |(i, _)| i);
    &name[..end]
}

struct AsClassDraft {
    key: String,
    name: String,
    package: String,
    kind: String,
    pseudo: bool,
    status: String,
    parents: Vec<String>,
    implements: Vec<String>,
    members: Vec<Member>,
}

fn build_as_index(api: &'static script_api::LanguageApi, language: ScriptLanguage) -> ApiIndex {
    const TOP_LEVEL: &str = "Top level";
    let mut groups: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for raw in api.classes.keys() {
        groups
            .entry(canonical_class_key(raw))
            .or_default()
            .push(raw.as_str());
    }

    let mut drafts: Vec<AsClassDraft> = Vec::new();
    // Top-level declarations: the language keeps them under an empty owner, `<language>` or
    // the `Top Level` package entry.
    let mut top_level: Vec<Member> = ["", "<language>", "Top Level"]
        .into_iter()
        .flat_map(|owner| script_api::own_members(&[], api, owner))
        .filter(|m| m.kind != "package")
        .collect();
    top_level.sort_by_key(|a| a.name.to_lowercase());
    let (language_elements, globals): (Vec<Member>, Vec<Member>) = top_level
        .into_iter()
        .partition(|m| is_language_kind(&m.kind));
    if !globals.is_empty() {
        drafts.push(AsClassDraft {
            key: API_GLOBAL_KEY.into(),
            name: "Global functions and properties".into(),
            package: TOP_LEVEL.into(),
            kind: "package".into(),
            pseudo: true,
            status: String::new(),
            parents: Vec::new(),
            implements: Vec::new(),
            members: globals,
        });
    }
    if !language_elements.is_empty() {
        drafts.push(AsClassDraft {
            key: API_LANGUAGE_KEY.into(),
            name: "Operators and statements".into(),
            package: TOP_LEVEL.into(),
            kind: "package".into(),
            pseudo: true,
            status: String::new(),
            parents: Vec::new(),
            implements: Vec::new(),
            members: language_elements,
        });
    }
    for (key, raws) in &groups {
        if key == "Top Level" {
            continue;
        }
        let mut members: Vec<Member> = Vec::new();
        for raw in raws {
            members.extend(script_api::own_members(&[], api, raw));
            members.extend(script_api::missing_members(api, raw).cloned());
        }
        // Prefer the audited class entry over a `members` placeholder for the same class.
        let Some(info) = raws
            .iter()
            .filter_map(|raw| api.classes.get(*raw))
            .min_by_key(|info| info.status == "members")
        else {
            continue;
        };
        // A package entry (`flash.utils`) owns its package-level functions.
        let container = info.status == "members"
            && !members.is_empty()
            && members
                .iter()
                .all(|m| matches!(m.kind.as_str(), "function" | "package"));
        members.retain(|m| m.kind != "package");
        if container {
            if members.is_empty() {
                continue;
            }
            drafts.push(AsClassDraft {
                key: key.clone(),
                name: "Package functions".into(),
                package: key.clone(),
                kind: "package".into(),
                pseudo: true,
                status: String::new(),
                parents: Vec::new(),
                implements: Vec::new(),
                members,
            });
            continue;
        }
        let package = script_api::package_of(key);
        drafts.push(AsClassDraft {
            key: key.clone(),
            name: script_api::short_name(key).to_owned(),
            package: if package.is_empty() {
                TOP_LEVEL.into()
            } else {
                package.to_owned()
            },
            kind: info.kind.clone(),
            pseudo: false,
            status: info.status.clone(),
            parents: info.parents.clone(),
            implements: info.implements.clone(),
            members,
        });
    }

    // Short class name → keys, to link types and parents.
    let mut by_short: HashMap<String, Vec<String>> = HashMap::new();
    for draft in drafts.iter().filter(|d| !d.pseudo) {
        by_short
            .entry(draft.name.clone())
            .or_default()
            .push(draft.key.clone());
    }
    let known: std::collections::HashSet<String> = drafts.iter().map(|d| d.key.clone()).collect();
    let resolve = |name: &str, near: &str| -> Option<String> {
        let canonical = canonical_class_key(name);
        if known.contains(&canonical) {
            return Some(canonical);
        }
        let candidates = by_short.get(script_api::short_name(name))?;
        candidates
            .iter()
            .find(|key| script_api::package_of(key) == near)
            .or_else(|| candidates.iter().find(|key| !key.contains('.')))
            .or_else(|| (candidates.len() == 1).then(|| &candidates[0]))
            .cloned()
    };

    let mut packages: BTreeMap<String, Vec<(u8, String, String)>> = BTreeMap::new();
    let mut classes = HashMap::new();
    let (mut member_count, mut status_count) = (0, 0);
    for draft in drafts {
        let near = script_api::package_of(&draft.key).to_owned();
        let ctor_params = draft
            .members
            .iter()
            .find(|m| m.kind == "constructor")
            .and_then(|m| m.params.as_deref());
        let mut used: HashMap<String, usize> = HashMap::new();
        let mut members: Vec<ApiMember> = draft
            .members
            .iter()
            .map(|m| {
                let mut view = as_member_view(m, language, &draft, ctor_params);
                let seen = used.entry(view.key.clone()).or_insert(0);
                *seen += 1;
                if *seen > 1 {
                    let _ = write!(view.key, "|{seen}");
                }
                // See also: the class of the value type (`Vector.<Point>` → `Point`).
                let ty = m.ty.trim_end_matches('>').rsplit('<').next().unwrap_or("");
                if let Some(class_key) = resolve(ty, &near).filter(|key| *key != draft.key) {
                    view.see_also.push(ApiLink {
                        label: script_api::short_name(&class_key).to_owned(),
                        class_key,
                        member_key: None,
                    });
                }
                view
            })
            .collect();
        members.sort_by(|a, b| {
            (a.group, a.name.to_lowercase(), &a.key).cmp(&(b.group, b.name.to_lowercase(), &b.key))
        });
        // See also: up to four members sharing the leading word (`gotoAndPlay` → `gotoAndStop`).
        let names: Vec<(String, String, String)> = members
            .iter()
            .map(|m| (m.key.clone(), m.name.clone(), m.label.clone()))
            .collect();
        for member in &mut members {
            let word = leading_word(&member.name);
            if word.len() < 3 || word.len() == member.name.len() {
                continue;
            }
            let siblings: Vec<ApiLink> = names
                .iter()
                .filter(|(key, name, _)| *key != member.key && leading_word(name) == word)
                .take(4)
                .map(|(key, _, label)| ApiLink {
                    label: label.clone(),
                    class_key: draft.key.clone(),
                    member_key: Some(key.clone()),
                })
                .collect();
            member.see_also.splice(0..0, siblings);
        }
        member_count += members.len();
        status_count += members.iter().filter(|m| m.support.is_some()).count();
        let chain = draft
            .parents
            .iter()
            .map(|parent| {
                (
                    script_api::short_name(parent).to_owned(),
                    resolve(parent, &near),
                )
            })
            .collect();
        let description =
            if draft.pseudo {
                match draft.key.as_str() {
                API_GLOBAL_KEY => {
                    "Functions, properties and constants available in every script without a class."
                        .to_owned()
                }
                API_LANGUAGE_KEY => {
                    "Operators, statements and directives of the language.".to_owned()
                }
                _ => format!("Functions declared directly in the {} package.", draft.package),
            }
            } else {
                script_api::describe_as_class(
                    language,
                    &draft.key,
                    &draft.name,
                    &draft.package,
                    &draft.kind,
                    &draft.parents,
                    &draft.implements,
                )
            };
        let example = api_examples::synthesize_as_class_example(
            language,
            &draft.key,
            &draft.name,
            &draft.package,
            &draft.kind,
            ctor_params,
        );
        let insertion = (!draft.pseudo).then(|| {
            crate::panels::actions_sidebar::toolbox_insertion(
                &Member {
                    owner: draft.key.clone(),
                    name: draft.name.clone(),
                    kind: "class".into(),
                    ..Member::default()
                },
                language,
            )
        });
        let order = match draft.key.as_str() {
            API_GLOBAL_KEY => 0,
            API_LANGUAGE_KEY => 1,
            _ if draft.pseudo => 2,
            _ => 3,
        };
        packages.entry(draft.package.clone()).or_default().push((
            order,
            draft.key.clone(),
            draft.name.clone(),
        ));
        classes.insert(
            draft.key.clone(),
            Arc::new(ApiClass {
                key: draft.key,
                name: draft.name,
                package: draft.package,
                chain,
                status: draft.status,
                description,
                example,
                insertion,
                members,
            }),
        );
    }
    let mut ordered: Vec<ApiPackage> = Vec::new();
    for (label, mut entries) in packages {
        entries.sort_by_key(|a| (a.0, a.2.to_lowercase()));
        let package = ApiPackage {
            label: label.clone(),
            classes: entries
                .into_iter()
                .map(|(_, key, name)| (key, name))
                .collect(),
        };
        if label == TOP_LEVEL {
            ordered.insert(0, package);
        } else {
            ordered.push(package);
        }
    }
    ApiIndex {
        class_count: classes.len(),
        packages: ordered,
        flat: false,
        classes,
        member_count,
        status_count,
    }
}

fn as_member_key(m: &Member) -> String {
    format!(
        "{}|{}|{}",
        m.name,
        m.kind,
        if m.is_static { "static" } else { "instance" }
    )
}

fn default_member_usage(
    m: &Member,
    language: ScriptLanguage,
    class: &AsClassDraft,
    arg_names: &[String],
) -> String {
    let as3 = language == ScriptLanguage::ActionScript3;
    let kind = m.kind.as_str();
    let callable = m.params.is_some();
    let args = arg_names
        .iter()
        .map(|name| name.strip_prefix("...").unwrap_or(name.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let owner = class.name.as_str();
    let receiver = lower_first(owner);
    if class.pseudo {
        if callable {
            format!("{}({args});", m.name)
        } else if kind == "property" && !m.read_only && !m.name.starts_with('_') {
            format!("{} = value;", m.name)
        } else if kind == "event" && m.name == "on" {
            "on (release) {\n}".to_owned()
        } else if kind == "event" && m.name == "onClipEvent" {
            "onClipEvent (enterFrame) {\n}".to_owned()
        } else {
            m.name.clone()
        }
    } else {
        match (kind, callable) {
            ("constructor", _) if owner == "Vector" && as3 => {
                format!("var {receiver}:Vector.<T> = new Vector.<T>({args});")
            }
            ("constructor", _) => format!("var {receiver}:{owner} = new {owner}({args});"),
            (_, true) if m.is_static => format!("{owner}.{}({args});", m.name),
            (_, true) => format!("{receiver}.{}({args});", m.name),
            ("event" | "callback", false) if as3 => {
                format!("{receiver}.addEventListener(\"{}\", listener);", m.name)
            }
            ("event" | "callback", false) => {
                format!("{receiver}.{} = function():Void {{}};", m.name)
            }
            ("constant", false) => format!("{owner}.{}", m.name),
            ("property", false) if m.read_only => {
                if m.is_static {
                    format!("{owner}.{}", m.name)
                } else {
                    format!("{receiver}.{}", m.name)
                }
            }
            ("property", false) => {
                if m.is_static {
                    format!("{owner}.{} = value;", m.name)
                } else {
                    format!("{receiver}.{} = value;", m.name)
                }
            }
            (_, false) if m.is_static => format!("{owner}.{}", m.name),
            (_, false) => format!("{receiver}.{}", m.name),
        }
    }
}

fn format_js_ts_declaration(signature: &str, kind: JsTsEntryKind) -> String {
    if !matches!(kind, JsTsEntryKind::Function | JsTsEntryKind::Member(_)) {
        return signature.to_owned();
    }
    let mut depth = 0_i32;
    let mut quote: Option<char> = None;
    let mut prev: Option<char> = None;
    let mut open_paren: Option<usize> = None;
    let mut close_paren: Option<usize> = None;
    for (idx, ch) in signature.char_indices() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(ch),
            (None, '|' | ';') if depth == 0 => return signature.to_owned(),
            (None, '(') => {
                if depth == 0 && open_paren.is_none() {
                    open_paren = Some(idx);
                }
                depth += 1;
            }
            (None, '[' | '<' | '{') => depth += 1,
            (None, '>') if prev == Some('=') => {}
            (None, ')') => {
                depth -= 1;
                if depth == 0 && open_paren.is_some() && close_paren.is_none() {
                    close_paren = Some(idx);
                }
            }
            (None, ']' | '>' | '}') => depth -= 1,
            _ => {}
        }
        prev = Some(ch);
    }
    let (Some(open), Some(close)) = (open_paren, close_paren) else {
        return signature.to_owned();
    };
    let inner = &signature[open + 1..close];
    let parts = split_params(inner);
    if parts.len() > 1 {
        let head = &signature[..open];
        let tail = &signature[close + 1..];
        format!("{head}(\n    {}\n){tail}", parts.join(",\n    "))
    } else {
        signature.to_owned()
    }
}

fn as_member_view(
    m: &Member,
    language: ScriptLanguage,
    class: &AsClassDraft,
    ctor_params: Option<&str>,
) -> ApiMember {
    let as3 = language == ScriptLanguage::ActionScript3;
    let kind = m.kind.as_str();
    let callable = m.params.is_some();
    let constructor = kind == "constructor";
    let returns = if callable && !constructor {
        m.ty.clone()
    } else {
        String::new()
    };
    let param_parts = m.params.as_deref().map(split_params).unwrap_or_default();
    let ty_suffix = |separator: &str| {
        if m.ty.is_empty() {
            String::new()
        } else {
            format!("{separator}{}", m.ty)
        }
    };
    let row_signature = match &m.params {
        Some(params) => format!(
            "({}){}",
            params.trim(),
            if returns.is_empty() {
                String::new()
            } else {
                format!(":{returns}")
            }
        ),
        None => {
            let flag = match kind {
                "constant" => "constant",
                "event" | "callback" => "event",
                "property" => match (m.is_static, m.read_only) {
                    (true, true) => "static · read-only",
                    (false, true) => "read-only",
                    (true, false) => "static",
                    (false, false) => "",
                },
                _ if m.is_static => "static",
                _ => "",
            };
            let ty = ty_suffix(": ");
            match (ty.is_empty(), flag.is_empty()) {
                (false, false) => format!("{ty}  ·  {flag}"),
                (false, true) => ty,
                (true, _) => flag.to_owned(),
            }
        }
    };
    let access = if class.pseudo { "" } else { "public " };
    let stat = if m.is_static { "static " } else { "" };
    let val_suffix = m
        .value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| format!(" = {v}"))
        .unwrap_or_default();
    let declaration = match (&m.params, kind) {
        (Some(params), _) => {
            let head = if constructor {
                format!("{access}function {}", m.name)
            } else {
                format!("{access}{stat}function {}", m.name)
            };
            let ret = if returns.is_empty() {
                String::new()
            } else {
                format!(":{returns}")
            };
            if param_parts.len() > 1 {
                format!("{head}(\n    {}\n){ret}", param_parts.join(",\n    "))
            } else {
                format!("{head}({}){ret}", params.trim())
            }
        }
        (None, "constant") if class.pseudo => {
            format!("const {}{}{val_suffix}", m.name, ty_suffix(":"))
        }
        (None, "constant") => {
            format!(
                "public static const {}{}{val_suffix}",
                m.name,
                ty_suffix(":")
            )
        }
        (None, "property") if m.read_only && as3 => {
            format!("{access}{stat}function get {}(){}", m.name, ty_suffix(":"))
        }
        (None, "property") if m.read_only => {
            format!(
                "{access}{stat}var {}{} // read-only",
                m.name,
                ty_suffix(":")
            )
        }
        (None, "property") => format!("{access}{stat}var {}{}", m.name, ty_suffix(":")),
        (None, "event" | "callback") if as3 && !class.pseudo => {
            let event_ty = if m.ty.trim().is_empty() {
                "flash.events.Event"
            } else {
                m.ty.trim()
            };
            format!("[Event(name=\"{}\", type=\"{event_ty}\")]", m.name)
        }
        (None, "event" | "callback") if !class.pseudo => {
            format!("public {} = function() {{}}", m.name)
        }
        _ => m.name.clone(),
    };
    let arg_names: Vec<String> = param_parts
        .iter()
        .map(|part| describe_param(part).0)
        .filter(|name| !name.is_empty())
        .collect();
    let usage = default_member_usage(m, language, class, &arg_names);
    let example = api_examples::synthesize_as_member_example(
        language,
        &class.key,
        &class.name,
        class.pseudo,
        &class.kind,
        ctor_params,
        m,
    );
    let description =
        script_api::describe_as_member(language, &class.key, &class.name, class.pseudo, m);
    let kind_label = match (m.is_static, kind) {
        (_, "special_type") => "special type".to_owned(),
        (true, "constant") => "constant".to_owned(),
        (true, _) => format!("static {kind}"),
        (false, _) => kind.to_owned(),
    };
    ApiMember {
        key: as_member_key(m),
        name: m.name.clone(),
        label: if is_language_kind(kind) {
            m.name.clone()
        } else {
            m.label()
        },
        kind: kind_label,
        group: member_group(kind),
        row_signature,
        usage,
        declaration,
        params: param_parts
            .iter()
            .map(|part| describe_param(part))
            .collect(),
        returns,
        support: Support::of(&m.status),
        note: m.limitation.clone().unwrap_or_default(),
        description,
        example,
        insertion: crate::panels::actions_sidebar::toolbox_insertion(m, language),
        see_also: Vec::new(),
    }
}

fn build_js_ts_index() -> ApiIndex {
    let mut classes = HashMap::new();
    let mut package = ApiPackage {
        label: String::new(),
        classes: Vec::new(),
    };
    let mut member_count = 0;
    let mut status_count = 0;
    for &(category, description, _) in JS_TS_CATEGORIES {
        let entries: Vec<&'static JsTsApiEntry> = JS_TS_API_ENTRIES
            .iter()
            .filter(|e| e.category == category)
            .collect();
        if entries.is_empty() {
            continue;
        }
        let key_of = |e: &JsTsApiEntry| format!("{}|{}", e.category, e.name);
        let mut members: Vec<ApiMember> = entries
            .iter()
            .map(|entry| {
                let (group, kind) = match entry.kind {
                    JsTsEntryKind::Binding => (MemberGroup::Properties, "binding".to_owned()),
                    JsTsEntryKind::Function => (MemberGroup::Methods, "function".to_owned()),
                    JsTsEntryKind::Member(owner) => {
                        (MemberGroup::Methods, format!("member of {owner}"))
                    }
                    JsTsEntryKind::Keyword => (MemberGroup::Language, "keyword".to_owned()),
                    JsTsEntryKind::TypeConstruct => (MemberGroup::Language, "type".to_owned()),
                    JsTsEntryKind::Class => (MemberGroup::Language, "class".to_owned()),
                };
                // Related entries: a class and its members.
                let related = |other: &&&JsTsApiEntry| match (entry.kind, other.kind) {
                    (JsTsEntryKind::Class, JsTsEntryKind::Member(owner)) => owner == entry.name,
                    (JsTsEntryKind::Member(owner), JsTsEntryKind::Class) => owner == other.name,
                    (JsTsEntryKind::Member(a), JsTsEntryKind::Member(b)) => {
                        a == b && other.name != entry.name
                    }
                    _ => false,
                };
                let see_also = entries
                    .iter()
                    .filter(related)
                    .take(6)
                    .map(|other| ApiLink {
                        label: other.name.to_owned(),
                        class_key: category.to_owned(),
                        member_key: Some(key_of(other)),
                    })
                    .collect();
                let row_signature = entry
                    .signature
                    .strip_prefix(entry.name)
                    .filter(|rest| rest.starts_with(['(', ':', '<']))
                    .unwrap_or(entry.signature)
                    .to_owned();
                let note = if entry.support_note.is_empty() {
                    format!(
                        "{}. Available in: {}.",
                        entry.provenance, entry.availability
                    )
                } else {
                    format!(
                        "{}. {} Available in: {}.",
                        entry.provenance, entry.support_note, entry.availability
                    )
                };
                let usage = entry.usage.to_owned();
                let declaration = format_js_ts_declaration(entry.signature, entry.kind);
                let example = entry.example.to_owned();
                ApiMember {
                    key: key_of(entry),
                    name: entry.name.to_owned(),
                    label: entry.name.to_owned(),
                    kind: if entry.ts_only {
                        format!("{kind}  ·  TypeScript only")
                    } else {
                        kind
                    },
                    group,
                    row_signature,
                    usage,
                    declaration,
                    params: Vec::new(),
                    returns: String::new(),
                    support: Some(entry.support),
                    note,
                    description: entry.summary.to_owned(),
                    example,
                    insertion: crate::panels::actions::js_ts_toolbox_insertion(entry),
                    see_also,
                }
            })
            .collect();
        members.sort_by_key(|m| m.group);
        member_count += members.len();
        status_count += members.iter().filter(|m| m.support.is_some()).count();
        let class_status = if members.iter().all(|m| m.support == Some(Support::Working)) {
            "subset"
        } else {
            "partial"
        };
        let class_example = entries
            .first()
            .map(|e| e.example.to_owned())
            .unwrap_or_default();
        package
            .classes
            .push((category.to_owned(), category.to_owned()));
        classes.insert(
            category.to_owned(),
            Arc::new(ApiClass {
                key: category.to_owned(),
                name: category.to_owned(),
                package: "JavaScript / TypeScript".into(),
                chain: Vec::new(),
                status: class_status.into(),
                description: format!("{description}."),
                example: class_example,
                insertion: None,
                members,
            }),
        );
    }
    ApiIndex {
        class_count: classes.len(),
        packages: vec![package],
        flat: true,
        classes,
        member_count,
        status_count,
    }
}

/// One search result of the API tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ApiHit {
    pub class_key: String,
    pub member_key: Option<String>,
    /// `MovieClip`, `MovieClip.gotoAndPlay()`.
    pub label: String,
    /// Package of a class hit, kind of a member hit.
    pub meta: String,
    pub support: Option<Support>,
    /// 0 exact name, 1 prefix, 2 substring, 3 description match.
    pub rank: u8,
}

/// Classes and members of `scope` whose name or description matches `query`, ordered exact name,
/// prefix, substring, then description; classes before members inside a rank. `Class.member`
/// narrows members to a class.
pub(crate) fn api_search(scope: LanguageScope, query: &str) -> Vec<ApiHit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let index = api_index(scope);
    let rank = |name: &str, needle: &str| -> Option<u8> {
        let name = name.to_lowercase();
        if name == needle {
            Some(0)
        } else if name.starts_with(needle) {
            Some(1)
        } else if name.contains(needle) {
            Some(2)
        } else {
            None
        }
    };
    let dotted = q
        .rsplit_once('.')
        .filter(|(owner, member)| !owner.is_empty() && !member.is_empty());
    let match_descriptions = dotted.is_none() && q.chars().count() >= 3;
    let mut hits = Vec::new();
    for (class_key, _) in index.packages.iter().flat_map(|p| &p.classes) {
        let Some(class) = index.class(class_key) else {
            continue;
        };
        let pseudo = class.insertion.is_none();
        let class_name = class.name.to_lowercase();
        let owner_matches = dotted
            .is_some_and(|(owner, _)| class_name == owner || class.key.to_lowercase() == owner);
        let mut class_has_member_name_hit = false;
        for member in &class.members {
            let by_owner = dotted
                .filter(|_| owner_matches)
                .and_then(|(_, name)| rank(&member.name, name));
            let name_rank = [by_owner, rank(&member.name, &q)]
                .into_iter()
                .flatten()
                .min();
            if name_rank.is_some() {
                class_has_member_name_hit = true;
            }
            let desc_rank = (match_descriptions
                && (member.description.to_lowercase().contains(&q)
                    || member.note.to_lowercase().contains(&q)))
            .then_some(3);
            let Some(rank) = [name_rank, desc_rank].into_iter().flatten().min() else {
                continue;
            };
            hits.push(ApiHit {
                class_key: class.key.clone(),
                member_key: Some(member.key.clone()),
                label: if pseudo {
                    member.label.clone()
                } else {
                    format!("{}.{}", class.name, member.label)
                },
                meta: member.kind.clone(),
                support: member.support,
                rank,
            });
        }
        if !pseudo || index.flat {
            let class_rank = rank(&class.name, &q)
                .or_else(|| class.key.to_lowercase().contains(&q).then_some(2))
                .or_else(|| {
                    (!class_has_member_name_hit
                        && match_descriptions
                        && class.description.to_lowercase().contains(&q))
                    .then_some(3)
                });
            if let Some(rank) = class_rank {
                hits.push(ApiHit {
                    class_key: class.key.clone(),
                    member_key: None,
                    label: class.name.clone(),
                    meta: class.package.clone(),
                    support: Support::of(&class.status),
                    rank,
                });
            }
        }
    }
    hits.sort_by(|a, b| {
        (
            a.rank,
            a.member_key.is_some(),
            a.label.to_lowercase(),
            &a.class_key,
        )
            .cmp(&(
                b.rank,
                b.member_key.is_some(),
                b.label.to_lowercase(),
                &b.class_key,
            ))
    });
    hits
}
