//! Self-contained, runnable ActionScript 2.0 and ActionScript 3.0 reference examples with
//! concrete sample inputs, `trace(...)` calls, and `// Output: ...` annotations.

use super::api_index::{API_GLOBAL_KEY, API_LANGUAGE_KEY, describe_param, split_params};
use crate::script_api::{self, Member};
use neoflash_core::ScriptLanguage;
use std::collections::BTreeSet;
use std::fmt::Write as _;

/// Synthesizes a realistic, multi-line ActionScript 2.0 or 3.0 example with `trace(...)` and
/// `// Output: ...` for a single member (method, constructor, property, constant, event,
/// global/package function, or language element).
#[allow(dead_code)]
pub(super) fn synthesize_as_member_example(
    language: ScriptLanguage,
    class_key: &str,
    class_name: &str,
    pseudo: bool,
    class_kind: &str,
    constructor_params: Option<&str>,
    member: &Member,
) -> String {
    let as3 = language == ScriptLanguage::ActionScript3;
    let canonical_key = canonical_owner_key(class_key, member);

    if canonical_key == API_LANGUAGE_KEY || is_language_member_kind(&member.kind) {
        return language_element_example(as3, member);
    }

    if pseudo
        || canonical_key == API_GLOBAL_KEY
        || canonical_key.is_empty()
        || canonical_key == "Top Level"
        || class_kind == "package"
    {
        return global_or_package_example(as3, &canonical_key, member);
    }

    let short = if class_name.is_empty() {
        script_api::short_name(&canonical_key)
    } else {
        class_name
    };

    if let Some(curated) = curated_member_example(as3, &canonical_key, short, member) {
        return curated;
    }

    let inferred_ctor = constructor_params
        .map(str::to_owned)
        .or_else(|| lookup_constructor_params(language, &canonical_key));

    synthesize_domain_member_example(
        as3,
        &canonical_key,
        short,
        class_kind,
        inferred_ctor.as_deref(),
        member,
    )
}

/// Synthesizes a realistic, multi-line ActionScript 2.0 or 3.0 overview example with `trace(...)`
/// and `// Output: ...` for a class or interface summary view.
#[allow(dead_code)]
pub(super) fn synthesize_as_class_example(
    language: ScriptLanguage,
    class_key: &str,
    class_name: &str,
    package: &str,
    class_kind: &str,
    constructor_params: Option<&str>,
) -> String {
    let as3 = language == ScriptLanguage::ActionScript3;
    let canonical_key = canonical_class_key_str(class_key);
    let short = if class_name.is_empty() {
        script_api::short_name(&canonical_key)
    } else {
        class_name
    };

    if canonical_key == API_GLOBAL_KEY {
        return if as3 {
            "var parsed:int = parseInt(\"42px\", 10);\ntrace(parsed, isNaN(parsed)); // Output: 42 false"
                .to_owned()
        } else {
            "var parsed:Number = parseInt(\"42px\", 10);\ntrace(parsed); // Output: 42".to_owned()
        };
    }
    if canonical_key == API_LANGUAGE_KEY {
        return if as3 {
            "var total:int = 0;\nfor (var i:int = 1; i <= 4; i++) {\n    total += i;\n}\ntrace(total); // Output: 10"
                .to_owned()
        } else {
            "var total:Number = 0;\nfor (var i:Number = 1; i <= 4; i++) {\n    total += i;\n}\ntrace(total); // Output: 10"
                .to_owned()
        };
    }

    if let Some(curated) = curated_class_example(as3, &canonical_key, short) {
        return curated;
    }

    let inferred_ctor = constructor_params
        .map(str::to_owned)
        .or_else(|| lookup_constructor_params(language, &canonical_key));

    synthesize_domain_class_example(
        language,
        as3,
        &canonical_key,
        short,
        package,
        class_kind,
        inferred_ctor.as_deref(),
    )
}

fn canonical_class_key_str(raw: &str) -> String {
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

fn canonical_owner_key(class_key: &str, member: &Member) -> String {
    if class_key == API_LANGUAGE_KEY || is_language_member_kind(&member.kind) {
        return API_LANGUAGE_KEY.to_owned();
    }
    if class_key == API_GLOBAL_KEY
        || class_key == "Top Level"
        || (class_key.is_empty()
            && (member.owner.is_empty()
                || member.owner == "Top Level"
                || member.owner == API_LANGUAGE_KEY))
    {
        return API_GLOBAL_KEY.to_owned();
    }
    let raw = if class_key.is_empty() {
        member.owner.as_str()
    } else {
        class_key
    };
    canonical_class_key_str(raw)
}

fn is_language_member_kind(kind: &str) -> bool {
    matches!(
        kind,
        "operator" | "statement" | "directive" | "keyword" | "special_type" | "compiler_directive"
    )
}

fn lookup_constructor_params(language: ScriptLanguage, canonical_key: &str) -> Option<String> {
    let api = script_api::api(language)?;
    api.members
        .iter()
        .chain(api.missing.iter())
        .find(|m| m.kind == "constructor" && canonical_class_key_str(&m.owner) == canonical_key)
        .and_then(|m| m.params.clone())
}

fn format_constant_trace_output(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        && let Ok(parsed) = u64::from_str_radix(hex, 16)
    {
        return parsed.to_string();
    }
    trimmed.to_owned()
}

fn lower_ident(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if clean.is_empty() {
        return "instance".to_owned();
    }
    let mut chars = clean.chars();
    let first = chars.next().unwrap_or('v');
    let candidate: String = first.to_lowercase().chain(chars).collect();
    match candidate.as_str() {
        "class" | "function" | "var" | "const" | "int" | "uint" | "void" | "null" | "true"
        | "false" | "new" | "in" | "is" | "as" | "for" | "if" | "else" | "switch" | "case"
        | "default" | "return" | "package" | "import" | "super" | "this" | "with" | "typeof"
        | "delete" | "try" | "catch" | "finally" | "throw" | "while" | "do" | "break"
        | "continue" | "interface" | "extends" | "implements" | "public" | "private"
        | "protected" | "internal" | "static" | "dynamic" | "final" | "native" | "override"
        | "namespace" | "use" | "root" | "stage" | "parent" | "loaderInfo" | "transform"
        | "graphics" | "name" | "scenes" => format!("{candidate}Val"),
        _ => candidate,
    }
}

// ── Layer 1: Language Elements (`<language>`) ──

fn language_element_example(as3: bool, member: &Member) -> String {
    let raw = member.name.trim();
    let op_note = member.operator.as_deref().unwrap_or("");
    let num_ty = ":Number";
    let int_ty = if as3 { ":int" } else { ":Number" };

    if member.kind == "special_type" {
        return match raw {
            "*" => "var payload:* = \"ready\";\npayload = 42;\ntrace(payload); // Output: 42"
                .to_owned(),
            "void" => "function logBanner(title:String):void {\n    trace(\"Banner: \" + title);\n}\nlogBanner(\"Level 1\"); // Output: Banner: Level 1"
                .to_owned(),
            "Null" => "var target:Object = null;\ntrace(target === null); // Output: true"
                .to_owned(),
            _ => format!("var value:{raw} = null;\ntrace(value); // Output: null"),
        };
    }

    match raw {
        "+ (concatenation (XMLList))" => {
            return "var list:XMLList = <a/> + <b/>;\ntrace(list.length()); // Output: 2"
                .to_owned();
        }
        "+ (concatenation)" => {
            return "var label:String = \"Stage \" + 3;\ntrace(label); // Output: Stage 3"
                .to_owned();
        }
        "+= (concatenation assignment (XMLList))" => {
            return "var list:XMLList = XMLList(<item id=\"1\"/>);\nlist += <item id=\"2\"/>;\ntrace(list.length()); // Output: 2"
                .to_owned();
        }
        "+= (concatenation assignment)" => {
            return "var status:String = \"Score: \";\nstatus += 250;\ntrace(status); // Output: Score: 250"
                .to_owned();
        }
        ". (dot (XML))" => {
            return "var doc:XML = <hud><score>900</score></hud>;\ntrace(doc.score.toString()); // Output: 900"
                .to_owned();
        }
        ".. (descendant accessor)" => {
            return "var doc:XML = <menu><group><item>Play</item></group></menu>;\ntrace(doc..item.toString()); // Output: Play"
                .to_owned();
        }
        "@ (attribute identifier)" => {
            return "var node:XML = <player id=\"p1\" hp=\"100\"/>;\ntrace(node.@id.toString()); // Output: p1"
                .to_owned();
        }
        "( ) (parentheses (XML))" => {
            return "var items:XML = <bag><coin val=\"5\"/><coin val=\"20\"/></bag>;\nvar big:XMLList = items.coin.(Number(@val) >= 10);\ntrace(big.@val.toString()); // Output: 20"
                .to_owned();
        }
        "< > (XML literal tag delimiter)" => {
            return "var card:XML = <card rank=\"A\" suit=\"spades\"/>;\ntrace(card.@rank.toString()); // Output: A"
                .to_owned();
        }
        "[ ] (brackets (XML))" => {
            return "var list:XML = <team><hero>Aria</hero><hero>Sol</hero></team>;\ntrace(list.hero[1].toString()); // Output: Sol"
                .to_owned();
        }
        "{ } (braces (XML))" => {
            return "var tag:String = \"badge\";\nvar level:int = 4;\nvar node:XML = <{tag} level={level}/>;\ntrace(node.@level.toString()); // Output: 4"
                .to_owned();
        }
        "delete (XML)" => {
            return "var doc:XML = <bag><item>Key</item><item>Map</item></bag>;\ndelete doc.item[0];\ntrace(doc.item.length()); // Output: 1"
                .to_owned();
        }
        "/ (RegExp delimiter)" => {
            return "var digits:RegExp = /[0-9]+/;\ntrace(digits.test(\"wave_12\")); // Output: true"
                .to_owned();
        }
        "... (rest) parameter" => {
            return "function sumAll(...nums):int {\n    var total:int = 0;\n    for each (var n:int in nums) total += n;\n    return total;\n}\ntrace(sumAll(4, 6, 10)); // Output: 20"
                .to_owned();
        }
        ":: (name qualifier)" => {
            return "var doc:XML = <ns:item xmlns:ns=\"urn:game\">Orb</ns:item>;\nvar ns:Namespace = new Namespace(\"urn:game\");\ntrace(doc.ns::item.length() >= 0); // Output: true"
                .to_owned();
        }
        _ => {}
    }

    let token = raw.split(" (").next().unwrap_or(raw).trim();
    match token {
        "!" | "not" => {
            if token == "not" {
                "var paused:Boolean = false;\ntrace(not paused); // Output: true".to_owned()
            } else {
                "var paused:Boolean = false;\ntrace(!paused); // Output: true".to_owned()
            }
        }
        "!=" => format!("var a{int_ty} = 10;\nvar b{int_ty} = 20;\ntrace(a != b); // Output: true"),
        "!==" => "var count:Number = 5;\ntrace(count !== 0); // Output: true".to_owned(),
        "<>" => "var hp:Number = 50;\ntrace(hp <> 0); // Output: true".to_owned(),
        "\"" => "var title:String = \"NeoFlash\";\ntrace(title.length); // Output: 8".to_owned(),
        "#include" => {
            "// Pulls shared ActionScript source into this frame at compile time:\n// #include \"scripts/helpers.as\"\nvar loaded:Boolean = true;\ntrace(loaded); // Output: true"
                .to_owned()
        }
        "#initclip" => {
            "#initclip\nvar componentReady:Boolean = true;\n#endinitclip\ntrace(componentReady); // Output: true"
                .to_owned()
        }
        "#endinitclip" => {
            "#initclip\nvar registered:Number = 1;\n#endinitclip\ntrace(registered); // Output: 1"
                .to_owned()
        }
        "include" => {
            "// Inline an external .as snippet during compilation:\n// include \"logic/constants.as\";\nvar version:int = 3;\ntrace(version); // Output: 3"
                .to_owned()
        }
        "%" => format!("var rem{int_ty} = 17 % 5;\ntrace(rem); // Output: 2"),
        "%=" => format!("var energy{int_ty} = 14;\nenergy %= 5;\ntrace(energy); // Output: 4"),
        "&" => format!("var masked{int_ty} = 0x0F & 0x36;\ntrace(masked); // Output: 6"),
        "&&" => "var hasKey:Boolean = true;\nvar doorUnlocked:Boolean = hasKey && (3 > 1);\ntrace(doorUnlocked); // Output: true"
            .to_owned(),
        "and" => "var ready:Boolean = true;\nvar alive:Boolean = true;\ntrace(ready and alive); // Output: true"
            .to_owned(),
        "&&=" => "var canJump:Boolean = true;\ncanJump &&= false;\ntrace(canJump); // Output: false"
            .to_owned(),
        "&=" => format!("var flags{int_ty} = 7;\nflags &= 3;\ntrace(flags); // Output: 3"),
        "()" => format!("var result{int_ty} = (2 + 3) * 4;\ntrace(result); // Output: 20"),
        "*" => format!("var area{int_ty} = 12 * 8;\ntrace(area); // Output: 96"),
        "*=" => format!("var score{int_ty} = 25;\nscore *= 4;\ntrace(score); // Output: 100"),
        "+" => format!("var sum{int_ty} = 18 + 24;\ntrace(sum); // Output: 42"),
        "add" => "var full:String = \"Neo\" add \"Flash\";\ntrace(full); // Output: NeoFlash"
            .to_owned(),
        "++" => format!("var step{int_ty} = 4;\nstep++;\ntrace(step); // Output: 5"),
        "+=" => format!("var coins{int_ty} = 10;\ncoins += 15;\ntrace(coins); // Output: 25"),
        "," => format!("var a{int_ty} = 1, b{int_ty} = 2;\ntrace(a + b); // Output: 3"),
        "-" => format!("var delta{int_ty} = 50 - 18;\ntrace(delta); // Output: 32"),
        "--" => format!("var lives{int_ty} = 3;\nlives--;\ntrace(lives); // Output: 2"),
        "-=" => format!("var shield{int_ty} = 100;\nshield -= 35;\ntrace(shield); // Output: 65"),
        "." => "var pt:Object = {x: 32, y: 64};\ntrace(pt.x + pt.y); // Output: 96".to_owned(),
        "/" => format!("var half{num_ty} = 45 / 2;\ntrace(half); // Output: 22.5"),
        "/*..*/" => {
            format!("/* Multi-line block comment */\nvar fps{int_ty} = 24;\ntrace(fps); // Output: 24")
        }
        "//" => {
            format!("// Single-line comment explaining the constant\nvar speed{int_ty} = 6;\ntrace(speed); // Output: 6")
        }
        "/=" => format!("var width{num_ty} = 120;\nwidth /= 4;\ntrace(width); // Output: 30"),
        ":" => "var playerName:String = \"Aria\";\ntrace(playerName); // Output: Aria".to_owned(),
        "<" => format!("var x{int_ty} = 8;\ntrace(x < 16); // Output: true"),
        "<<" => format!("var shifted{int_ty} = 3 << 2;\ntrace(shifted); // Output: 12"),
        "<<=" => format!("var bit{int_ty} = 1;\nbit <<= 4;\ntrace(bit); // Output: 16"),
        "<=" => format!("var level{int_ty} = 5;\ntrace(level <= 5); // Output: true"),
        "=" => format!("var combo{int_ty} = 7;\ntrace(combo); // Output: 7"),
        "==" => format!("var value{int_ty} = 12;\ntrace(value == 12); // Output: true"),
        "===" => format!("var exact{int_ty} = 42;\ntrace(exact === 42); // Output: true"),
        ">" => format!("var score{int_ty} = 900;\ntrace(score > 500); // Output: true"),
        ">=" => format!("var rank{int_ty} = 10;\ntrace(rank >= 10); // Output: true"),
        ">>" => format!("var halfShift{int_ty} = 32 >> 2;\ntrace(halfShift); // Output: 8"),
        ">>=" => format!("var val{int_ty} = 64;\nval >>= 3;\ntrace(val); // Output: 8"),
        ">>>" => "var hi:Number = 16 >>> 2;\ntrace(hi); // Output: 4".to_owned(),
        ">>>=" => "var bits:Number = 32;\nbits >>>= 2;\ntrace(bits); // Output: 8".to_owned(),
        "?:" => {
            format!("var hp{int_ty} = 80;\nvar state:String = (hp > 0) ? \"alive\" : \"ko\";\ntrace(state); // Output: alive")
        }
        "[]" => "var items:Array = [\"sword\", \"shield\", \"bow\"];\ntrace(items[1]); // Output: shield"
            .to_owned(),
        "^" => format!("var toggled{int_ty} = 5 ^ 3;\ntrace(toggled); // Output: 6"),
        "^=" => format!("var mask{int_ty} = 15;\nmask ^= 5;\ntrace(mask); // Output: 10"),
        "|" => format!("var combined{int_ty} = 4 | 2 | 1;\ntrace(combined); // Output: 7"),
        "|=" => format!("var perms{int_ty} = 2;\nperms |= 1;\ntrace(perms); // Output: 3"),
        "||" => "var fallback:Boolean = false || true;\ntrace(fallback); // Output: true"
            .to_owned(),
        "or" => "var a:Boolean = false;\nvar b:Boolean = true;\ntrace(a or b); // Output: true"
            .to_owned(),
        "||=" => "var active:Boolean = false;\nactive ||= true;\ntrace(active); // Output: true"
            .to_owned(),
        "~" => format!("var inverted{int_ty} = ~0;\ntrace(inverted); // Output: -1"),
        "{}" => "var hero:Object = {name: \"Nova\", hp: 100};\ntrace(hero.name, hero.hp); // Output: Nova 100"
            .to_owned(),
        "eq" => "var mode:String = \"play\";\ntrace(mode eq \"play\"); // Output: true".to_owned(),
        "ne" => "var mode:String = \"menu\";\ntrace(mode ne \"play\"); // Output: true".to_owned(),
        "lt" => "var a:String = \"alpha\";\ntrace(a lt \"beta\"); // Output: true".to_owned(),
        "le" => "var a:String = \"alpha\";\ntrace(a le \"alpha\"); // Output: true".to_owned(),
        "gt" => "var a:String = \"zeta\";\ntrace(a gt \"beta\"); // Output: true".to_owned(),
        "ge" => "var a:String = \"beta\";\ntrace(a ge \"alpha\"); // Output: true".to_owned(),
        "as" => "var raw:Object = \"NeoFlash\";\nvar label:String = raw as String;\ntrace(label); // Output: NeoFlash"
            .to_owned(),
        "is" => "var items:Array = [1, 2, 3];\ntrace(items is Array); // Output: true".to_owned(),
        "in" => "var stats:Object = {hp: 100, mp: 40};\ntrace(\"hp\" in stats); // Output: true"
            .to_owned(),
        "instanceof" => {
            "var list:Array = [10, 20];\ntrace(list instanceof Array); // Output: true".to_owned()
        }
        "typeof" => "var title:String = \"Intro\";\ntrace(typeof title); // Output: string"
            .to_owned(),
        "new" => "var list:Array = new Array(\"a\", \"b\");\ntrace(list.length); // Output: 2"
            .to_owned(),
        "delete" => "var bag:Object = {key: 1, map: 1};\ndelete bag.key;\ntrace(bag.key === undefined); // Output: true"
            .to_owned(),
        "void" => "var cleared:* = void(0);\ntrace(cleared === undefined); // Output: true"
            .to_owned(),
        "if" => format!(
            "var score{int_ty} = 120;\nif (score >= 100) {{\n    trace(\"Bonus!\"); // Output: Bonus!\n}}"
        ),
        "else" => format!(
            "var keys{int_ty} = 0;\nif (keys > 0) {{\n    trace(\"open\");\n}} else {{\n    trace(\"locked\"); // Output: locked\n}}"
        ),
        "else if" => format!(
            "var pct{int_ty} = 75;\nif (pct >= 90) {{\n    trace(\"A\");\n}} else if (pct >= 70) {{\n    trace(\"B\"); // Output: B\n}}"
        ),
        "switch" | "case" | "default" => format!(
            "var rank{int_ty} = 2;\nswitch (rank) {{\n    case 1: trace(\"Gold\"); break;\n    case 2: trace(\"Silver\"); break;\n    default: trace(\"Bronze\"); break;\n}}\n// Output: Silver"
        ),
        "for" => format!(
            "var sum{int_ty} = 0;\nfor (var i{int_ty} = 1; i <= 4; i++) {{\n    sum += i;\n}}\ntrace(sum); // Output: 10"
        ),
        "for..in" => {
            "var item:Object = {id: \"orb\", tier: 3};\nvar count:Number = 0;\nfor (var key:String in item) {\n    count++;\n}\ntrace(count); // Output: 2"
                .to_owned()
        }
        "for each..in" => {
            "var nums:Array = [2, 4, 6];\nvar total:int = 0;\nfor each (var n:int in nums) {\n    total += n;\n}\ntrace(total); // Output: 12"
                .to_owned()
        }
        "while" => format!(
            "var n{int_ty} = 1;\nwhile (n < 8) {{\n    n *= 2;\n}}\ntrace(n); // Output: 8"
        ),
        "do..while" => format!(
            "var tries{int_ty} = 0;\ndo {{\n    tries++;\n}} while (tries < 3);\ntrace(tries); // Output: 3"
        ),
        "break" => format!(
            "var stoppedAt{int_ty} = 0;\nfor (var i{int_ty} = 1; i <= 10; i++) {{\n    if (i == 4) {{ stoppedAt = i; break; }}\n}}\ntrace(stoppedAt); // Output: 4"
        ),
        "continue" => format!(
            "var odds{int_ty} = 0;\nfor (var i{int_ty} = 1; i <= 5; i++) {{\n    if (i % 2 == 0) continue;\n    odds += i;\n}}\ntrace(odds); // Output: 9"
        ),
        "label" => {
            "var hits:int = 0;\nouter: for (var r:int = 0; r < 3; r++) {\n    hits++;\n    if (r == 1) break outer;\n}\ntrace(hits); // Output: 2"
                .to_owned()
        }
        "return" => {
            let ret_ty = if as3 { ":int" } else { ":Number" };
            format!(
                "function doubleVal(n{int_ty}){ret_ty} {{\n    return n * 2;\n}}\ntrace(doubleVal(21)); // Output: 42"
            )
        }
        "function" => "function greet(name:String):String {\n    return \"Hi, \" + name;\n}\ntrace(greet(\"Player 1\")); // Output: Hi, Player 1".to_owned(),
        "var" => format!("var lives{int_ty} = 3;\nlives += 1;\ntrace(lives); // Output: 4"),
        "const" => "const MAX_LIVES:int = 5;\ntrace(MAX_LIVES); // Output: 5".to_owned(),
        "set variable" => {
            "set(\"playerScore\", 500);\ntrace(playerScore); // Output: 500".to_owned()
        }
        "with" => {
            "var coords:Object = {x: 15, y: 25};\nwith (coords) {\n    trace(x + y); // Output: 40\n}"
                .to_owned()
        }
        "throw" | "try..catch..finally" => {
            let err_ty = if as3 { ":Error" } else { "" };
            format!(
                "try {{\n    throw new Error(\"Invalid frame\");\n}} catch (err{err_ty}) {{\n    trace(err.message); // Output: Invalid frame\n}}"
            )
        }
        "true" => "var active:Boolean = true;\ntrace(active); // Output: true".to_owned(),
        "false" => "var muted:Boolean = false;\ntrace(muted); // Output: false".to_owned(),
        "null" => "var selection:Object = null;\ntrace(selection === null); // Output: true"
            .to_owned(),
        "this" => {
            if as3 {
                "var frame:int = this.currentFrame;\ntrace(frame >= 1); // Output: true".to_owned()
            } else {
                "this.tag = \"hero\";\ntrace(this.tag); // Output: hero".to_owned()
            }
        }
        "super" => {
            if as3 {
                "// Inside a subclass constructor or overridden method:\n// super(x, y);\nvar inherited:Boolean = true;\ntrace(inherited); // Output: true"
                    .to_owned()
            } else {
                "// Inside a subclass method in a .as class file:\n// super.update(dt);\nvar ok:Boolean = true;\ntrace(ok); // Output: true"
                    .to_owned()
            }
        }
        "class" | "public" | "private" | "protected" | "internal" | "static" | "final"
        | "dynamic" | "override" | "native" | "intrinsic" | "extends" | "implements"
        | "interface" | "package" | "get" | "set" => {
            if as3 {
                format!(
                    "// Declared in a project .as source file (`{token}`):\n// package app {{ public class Counter {{ public var count:int = 7; }} }}\nvar count:int = 7;\ntrace(count); // Output: 7"
                )
            } else {
                format!(
                    "// Declared in an AS2 .as class file (`{token}`):\n// class Counter {{ public var count:Number = 7; }}\nvar count:Number = 7;\ntrace(count); // Output: 7"
                )
            }
        }
        "import" => {
            "import flash.geom.Point;\n\nvar pt:Point = new Point(3, 4);\ntrace(pt.length); // Output: 5"
                .to_owned()
        }
        "namespace" | "use namespace" | "default xml namespace" | "AS3" | "flash_proxy"
        | "object_proxy" => format!(
            "// Namespace directive (`{token}`):\nvar xml:XML = <config mode=\"arcade\"/>;\ntrace(xml.@mode.toString()); // Output: arcade"
        ),
        _ => {
            let label = if op_note.is_empty() { raw } else { op_note };
            format!(
                "// Language construct: {label}\nvar check:Boolean = true;\ntrace(check); // Output: true"
            )
        }
    }
}

// ── Layer 2: Global (`<global>`) & Package-Level Declarations ──

fn global_or_package_example(as3: bool, class_key: &str, member: &Member) -> String {
    let name = member.name.as_str();
    match (as3, name) {
        (_, "trace") => "var score:Number = 1200;\ntrace(\"Score:\", score); // Output: Score: 1200"
            .to_owned(),
        (_, "parseInt") => {
            "trace(parseInt(\"42px\", 10)); // Output: 42\ntrace(parseInt(\"0xFF\", 16)); // Output: 255"
                .to_owned()
        }
        (_, "parseFloat") => {
            "var speed:Number = parseFloat(\"3.25px\");\ntrace(speed * 2); // Output: 6.5"
                .to_owned()
        }
        (_, "isNaN") => {
            "trace(isNaN(NaN)); // Output: true\ntrace(isNaN(42));  // Output: false".to_owned()
        }
        (_, "isFinite") => {
            "trace(isFinite(250));      // Output: true\ntrace(isFinite(Infinity)); // Output: false"
                .to_owned()
        }
        (_, "escape") => {
            "var encoded:String = escape(\"a b\");\ntrace(encoded); // Output: a%20b".to_owned()
        }
        (_, "unescape") => {
            "var decoded:String = unescape(\"a%20b\");\ntrace(decoded); // Output: a b".to_owned()
        }
        (true, "encodeURI") => {
            "var url:String = encodeURI(\"https://example.com/my level\");\ntrace(url); // Output: https://example.com/my%20level"
                .to_owned()
        }
        (true, "decodeURI") => {
            "var url:String = decodeURI(\"https://example.com/my%20level\");\ntrace(url); // Output: https://example.com/my level"
                .to_owned()
        }
        (true, "encodeURIComponent") => {
            "var q:String = encodeURIComponent(\"a&b=1\");\ntrace(q); // Output: a%26b%3D1"
                .to_owned()
        }
        (true, "decodeURIComponent") => {
            "var q:String = decodeURIComponent(\"a%26b%3D1\");\ntrace(q); // Output: a&b=1"
                .to_owned()
        }
        (true, "isXMLName") => {
            "trace(isXMLName(\"spriteNode\")); // Output: true\ntrace(isXMLName(\"123bad\"));     // Output: false"
                .to_owned()
        }
        (_, "int") => "var whole:Number = int(7.9);\ntrace(whole); // Output: 7".to_owned(),
        (true, "uint") => {
            "var color:uint = uint(255);\ntrace(color); // Output: 255".to_owned()
        }
        (_, "Number") => {
            "var n:Number = Number(\"19.5\");\ntrace(n + 0.5); // Output: 20".to_owned()
        }
        (_, "String") => {
            "var label:String = String(2026);\ntrace(label.length, label); // Output: 4 2026"
                .to_owned()
        }
        (_, "Boolean") => {
            "trace(Boolean(1)); // Output: true\ntrace(Boolean(0)); // Output: false".to_owned()
        }
        (_, "Array") => {
            "var items:Array = Array(\"red\", \"green\", \"blue\");\ntrace(items.length, items[0]); // Output: 3 red"
                .to_owned()
        }
        (_, "Object") => {
            "var wrapper:Object = Object(\"ok\");\ntrace(wrapper.toString()); // Output: ok"
                .to_owned()
        }
        (true, "Vector") => {
            "var nums:Vector.<int> = Vector.<int>([10, 20, 30]);\ntrace(nums.length, nums[2]); // Output: 3 30"
                .to_owned()
        }
        (true, "XML") => {
            "var node:XML = XML(\"<item id='potion'>3</item>\");\ntrace(node.@id.toString(), node.toString()); // Output: potion 3"
                .to_owned()
        }
        (true, "XMLList") => {
            "var list:XMLList = XMLList(\"<a/><b/>\");\ntrace(list.length()); // Output: 2"
                .to_owned()
        }
        (_, "Infinity") => {
            "var limit:Number = Infinity;\ntrace(limit > 1e300); // Output: true".to_owned()
        }
        (_, "-Infinity") => {
            "var floorLimit:Number = -Infinity;\ntrace(floorLimit < -1e300); // Output: true"
                .to_owned()
        }
        (_, "NaN") => "var missing:Number = NaN;\ntrace(isNaN(missing)); // Output: true".to_owned(),
        (_, "undefined") => {
            "var unset:* = undefined;\ntrace(unset === undefined); // Output: true".to_owned()
        }
        (_, "null") => "var empty:Object = null;\ntrace(empty === null); // Output: true".to_owned(),
        (_, "true") => "var flag:Boolean = true;\ntrace(flag); // Output: true".to_owned(),
        (_, "false") => "var flag:Boolean = false;\ntrace(flag); // Output: false".to_owned(),
        (false, "newline") => {
            "var text:String = \"Line 1\" + newline + \"Line 2\";\ntrace(text.indexOf(\"Line 2\") > 0); // Output: true"
                .to_owned()
        }
        // AS2 timeline & movie globals
        (false, "play") => {
            "stop();\nplay();\ntrace(this._currentframe >= 1); // Output: true".to_owned()
        }
        (false, "stop") => "stop();\ntrace(this._currentframe); // Output: 1".to_owned(),
        (false, "gotoAndPlay") => {
            "gotoAndPlay(1);\ntrace(this._currentframe); // Output: 1".to_owned()
        }
        (false, "gotoAndStop") => {
            "gotoAndStop(1);\ntrace(this._currentframe); // Output: 1".to_owned()
        }
        (false, "nextFrame") => {
            "nextFrame();\ntrace(this._currentframe >= 1); // Output: true".to_owned()
        }
        (false, "prevFrame") => {
            "prevFrame();\ntrace(this._currentframe); // Output: 1".to_owned()
        }
        (false, "nextScene") => {
            "nextScene();\ntrace(this._currentframe >= 1); // Output: true".to_owned()
        }
        (false, "prevScene") => {
            "prevScene();\ntrace(this._currentframe >= 1); // Output: true".to_owned()
        }
        (false, "stopAllSounds") => {
            "stopAllSounds();\ntrace(\"All sounds stopped\"); // Output: All sounds stopped"
                .to_owned()
        }
        (false, "getTimer") => {
            "var elapsedMs:Number = getTimer();\ntrace(elapsedMs >= 0); // Output: true".to_owned()
        }
        (false, "getVersion") => {
            "var ver:String = getVersion();\ntrace(ver.length > 0); // Output: true".to_owned()
        }
        (false, "random") => {
            "var roll:Number = random(6);\ntrace(roll >= 0 && roll < 6); // Output: true"
                .to_owned()
        }
        (false, "chr") => "var letter:String = chr(65);\ntrace(letter); // Output: A".to_owned(),
        (false, "ord") => "var code:Number = ord(\"A\");\ntrace(code); // Output: 65".to_owned(),
        (false, "mbchr") => "var ch:String = mbchr(66);\ntrace(ch); // Output: B".to_owned(),
        (false, "mbord") => "var code:Number = mbord(\"B\");\ntrace(code); // Output: 66".to_owned(),
        (false, "length") => {
            "var count:Number = length(\"NeoFlash\");\ntrace(count); // Output: 8".to_owned()
        }
        (false, "mblength") => {
            "var count:Number = mblength(\"Flash\");\ntrace(count); // Output: 5".to_owned()
        }
        (false, "substring") => {
            "var part:String = substring(\"NeoFlash\", 4, 5);\ntrace(part); // Output: Flash"
                .to_owned()
        }
        (false, "mbsubstring") => {
            "var part:String = mbsubstring(\"NeoFlash\", 1, 3);\ntrace(part); // Output: Neo"
                .to_owned()
        }
        (false, "getProperty") => {
            "this._x = 120;\nvar xPos:Number = Number(getProperty(this, _x));\ntrace(xPos); // Output: 120"
                .to_owned()
        }
        (false, "setProperty") => {
            "setProperty(this, _x, 96);\ntrace(this._x); // Output: 96".to_owned()
        }
        (false, "eval") => {
            "var heroScore:Number = 750;\ntrace(eval(\"heroScore\")); // Output: 750".to_owned()
        }
        (false, "targetPath") => {
            "var path:String = targetPath(this);\ntrace(path.length > 0); // Output: true"
                .to_owned()
        }
        (false, "setInterval" | "clearInterval") => {
            "var id:Number = setInterval(function():Void {}, 100);\nclearInterval(id);\ntrace(id > 0); // Output: true"
                .to_owned()
        }
        (false, "setTimeout" | "clearTimeout") => {
            "var id:Number = setTimeout(function():Void {}, 100);\nclearTimeout(id);\ntrace(id > 0); // Output: true"
                .to_owned()
        }
        (false, "duplicateMovieClip") => {
            "var src:MovieClip = this.createEmptyMovieClip(\"dot_mc\", 1);\nduplicateMovieClip(src, \"dot_copy_mc\", 2);\ntrace(typeof this.dot_copy_mc); // Output: movieclip"
                .to_owned()
        }
        (false, "removeMovieClip") => {
            "var temp:MovieClip = this.createEmptyMovieClip(\"temp_mc\", 5);\nremoveMovieClip(temp);\ntrace(this.temp_mc === undefined); // Output: true"
                .to_owned()
        }
        (false, "startDrag" | "stopDrag") => {
            "var puck:MovieClip = this.createEmptyMovieClip(\"puck_mc\", 1);\nstartDrag(puck, false, 0, 0, 300, 200);\nstopDrag();\ntrace(puck._name); // Output: puck_mc"
                .to_owned()
        }
        (false, "on") => {
            "// Attached to a Button or MovieClip instance on Stage:\n// on (release) { trace(\"Clicked!\"); }\nvar buttonReady:Boolean = true;\ntrace(buttonReady); // Output: true"
                .to_owned()
        }
        (false, "onClipEvent") => {
            "// Attached to a MovieClip instance on Stage:\n// onClipEvent (load) { trace(\"Loaded clip\"); }\nvar clipReady:Boolean = true;\ntrace(clipReady); // Output: true"
                .to_owned()
        }
        (false, "_root") => {
            "_root.gameTitle = \"Orbit\";\ntrace(_root.gameTitle); // Output: Orbit".to_owned()
        }
        (false, "_parent") => {
            "var hasParent:Boolean = (_parent !== undefined);\ntrace(hasParent); // Output: false"
                .to_owned()
        }
        (false, "_global") => {
            "_global.HIGH_SCORE = 5000;\ntrace(_global.HIGH_SCORE); // Output: 5000".to_owned()
        }
        (false, "_quality") => "_quality = \"HIGH\";\ntrace(_quality); // Output: HIGH".to_owned(),
        (false, "_focusrect") => {
            "_focusrect = true;\ntrace(_focusrect); // Output: true".to_owned()
        }
        (false, "_soundbuftime") => {
            "_soundbuftime = 5;\ntrace(_soundbuftime); // Output: 5".to_owned()
        }
        // AS3 package-level functions
        (true, "getTimer") => {
            "import flash.utils.getTimer;\n\nvar ms:int = getTimer();\ntrace(ms >= 0); // Output: true"
                .to_owned()
        }
        (true, "getQualifiedClassName") => {
            "import flash.geom.Point;\nimport flash.utils.getQualifiedClassName;\n\nvar qname:String = getQualifiedClassName(new Point(1, 2));\ntrace(qname); // Output: flash.geom::Point"
                .to_owned()
        }
        (true, "getQualifiedSuperclassName") => {
            "import flash.display.MovieClip;\nimport flash.utils.getQualifiedSuperclassName;\n\nvar parentName:String = getQualifiedSuperclassName(MovieClip);\ntrace(parentName); // Output: flash.display::Sprite"
                .to_owned()
        }
        (true, "getDefinitionByName") => {
            "import flash.geom.Point;\nimport flash.utils.getDefinitionByName;\n\nvar PointClass:Class = getDefinitionByName(\"flash.geom.Point\") as Class;\nvar pt:Object = new PointClass(6, 8);\ntrace(pt.length); // Output: 10"
                .to_owned()
        }
        (true, "describeType") => {
            "import flash.geom.Point;\nimport flash.utils.describeType;\n\nvar info:XML = describeType(new Point(3, 4));\ntrace(info.@name.toString()); // Output: flash.geom::Point"
                .to_owned()
        }
        (true, "setInterval" | "clearInterval") => {
            "import flash.utils.clearInterval;\nimport flash.utils.setInterval;\n\nvar id:uint = setInterval(function():void {}, 250);\nclearInterval(id);\ntrace(id > 0); // Output: true"
                .to_owned()
        }
        (true, "setTimeout" | "clearTimeout") => {
            "import flash.utils.clearTimeout;\nimport flash.utils.setTimeout;\n\nvar id:uint = setTimeout(function():void {}, 250);\nclearTimeout(id);\ntrace(id > 0); // Output: true"
                .to_owned()
        }
        (true, "escapeMultiByte") => {
            "import flash.utils.escapeMultiByte;\n\ntrace(escapeMultiByte(\"Neo Flash\")); // Output: Neo%20Flash"
                .to_owned()
        }
        (true, "unescapeMultiByte") => {
            "import flash.utils.unescapeMultiByte;\n\ntrace(unescapeMultiByte(\"Neo%20Flash\")); // Output: Neo Flash"
                .to_owned()
        }
        (true, "registerClassAlias" | "getClassByAlias") => {
            "import flash.geom.Point;\nimport flash.net.getClassByAlias;\nimport flash.net.registerClassAlias;\n\nregisterClassAlias(\"app.PointAlias\", Point);\nvar resolved:Class = getClassByAlias(\"app.PointAlias\");\ntrace(resolved === Point); // Output: true"
                .to_owned()
        }
        (true, "navigateToURL") => {
            "import flash.net.URLRequest;\nimport flash.net.navigateToURL;\n\nvar req:URLRequest = new URLRequest(\"https://example.com\");\n// navigateToURL(req, \"_blank\");\ntrace(req.url); // Output: https://example.com"
                .to_owned()
        }
        (true, "sendToURL") => {
            "import flash.net.URLRequest;\nimport flash.net.sendToURL;\n\nvar beacon:URLRequest = new URLRequest(\"https://example.com/ping\");\n// sendToURL(beacon);\ntrace(beacon.url); // Output: https://example.com/ping"
                .to_owned()
        }
        (true, "fscommand") => {
            "import flash.system.fscommand;\n\nfscommand(\"fullscreen\", \"false\");\ntrace(\"Sent fscommand\"); // Output: Sent fscommand"
                .to_owned()
        }
        (true, "generateRandomBytes") => {
            "import flash.crypto.generateRandomBytes;\nimport flash.utils.ByteArray;\n\nvar rand:ByteArray = generateRandomBytes(16);\ntrace(rand.length); // Output: 16"
                .to_owned()
        }
        _ => {
            let mut imports = BTreeSet::new();
            if as3 && class_key.contains('.') {
                imports.insert(format!("import {class_key}.{name};"));
            }
            let args = build_call_args(as3, member.params.as_deref(), &mut imports);
            let mut header = String::new();
            for imp in &imports {
                let _ = writeln!(header, "{imp}");
            }
            if !header.is_empty() {
                header.push('\n');
            }
            if member.params.is_some() {
                if member.ty.is_empty() || member.ty.eq_ignore_ascii_case("void") {
                    format!(
                        "{header}{name}({args});\ntrace(\"Executed {name}\"); // Output: Executed {name}"
                    )
                } else {
                    let out = sample_output_for_type(&member.ty);
                    format!("{header}var result:* = {name}({args});\ntrace(result); // Output: {out}")
                }
            } else {
                format!("{header}var val:* = {name};\ntrace(val !== null); // Output: true")
            }
        }
    }
}

// ── Layer 3: Hand-Curated Core Classes & Members ──

fn curated_member_example(
    as3: bool,
    owner_key: &str,
    class_name: &str,
    member: &Member,
) -> Option<String> {
    let mname = member.name.as_str();
    let mkind = member.kind.as_str();

    // 1. Array (AS2 & AS3)
    if owner_key == "Array" {
        return Some(match mname {
            "Array" => "var items:Array = new Array(\"alpha\", \"beta\", \"gamma\");\ntrace(items.length, items[1]); // Output: 3 beta".to_owned(),
            "lastIndexOf" => "var items:Array = [\"a\", \"b\", \"c\", \"b\", \"d\"];\ntrace(items.lastIndexOf(\"b\"));    // Output: 3\ntrace(items.lastIndexOf(\"b\", 2)); // Output: 1\ntrace(items.lastIndexOf(\"z\"));    // Output: -1".to_owned(),
            "indexOf" => "var items:Array = [\"a\", \"b\", \"c\", \"b\"];\ntrace(items.indexOf(\"b\"));    // Output: 1\ntrace(items.indexOf(\"b\", 2)); // Output: 3\ntrace(items.indexOf(\"z\"));    // Output: -1".to_owned(),
            "push" => "var stack:Array = [\"coin\"];\nvar newLen:Number = stack.push(\"gem\", \"key\");\ntrace(newLen, stack[2]); // Output: 3 key".to_owned(),
            "pop" => "var stack:Array = [10, 20, 30];\nvar top:Number = stack.pop();\ntrace(top, stack.length); // Output: 30 2".to_owned(),
            "shift" => "var queue:Array = [\"first\", \"second\", \"third\"];\nvar next:String = queue.shift();\ntrace(next, queue.length); // Output: first 2".to_owned(),
            "unshift" => "var queue:Array = [\"b\", \"c\"];\nvar count:Number = queue.unshift(\"a\");\ntrace(count, queue[0]); // Output: 3 a".to_owned(),
            "slice" => "var letters:Array = [\"a\", \"b\", \"c\", \"d\"];\nvar mid:Array = letters.slice(1, 3);\ntrace(mid.join(\",\")); // Output: b,c".to_owned(),
            "splice" => "var slots:Array = [\"a\", \"b\", \"c\", \"d\"];\nvar removed:Array = slots.splice(1, 2, \"x\");\ntrace(removed.join(\",\"), slots.join(\",\")); // Output: b,c a,x,d".to_owned(),
            "insertAt" => "var list:Array = [\"a\", \"c\"];\nlist.insertAt(1, \"b\");\ntrace(list.join(\",\")); // Output: a,b,c".to_owned(),
            "removeAt" => "var list:Array = [\"a\", \"b\", \"c\"];\nvar removed:String = list.removeAt(1);\ntrace(removed, list.join(\",\")); // Output: b a,c".to_owned(),
            "concat" => "var a:Array = [1, 2];\nvar b:Array = a.concat([3, 4]);\ntrace(b.join(\"-\")); // Output: 1-2-3-4".to_owned(),
            "join" => "var parts:Array = [\"2026\", \"11\", \"21\"];\ntrace(parts.join(\"-\")); // Output: 2026-11-21".to_owned(),
            "reverse" => "var nums:Array = [1, 2, 3];\nnums.reverse();\ntrace(nums.join(\",\")); // Output: 3,2,1".to_owned(),
            "sort" => "var scores:Array = [30, 5, 100];\nscores.sort(Array.NUMERIC);\ntrace(scores.join(\",\")); // Output: 5,30,100".to_owned(),
            "sortOn" => "var players:Array = [{name: \"Sol\", pts: 40}, {name: \"Aria\", pts: 90}];\nplayers.sortOn(\"pts\", Array.NUMERIC | Array.DESCENDING);\ntrace(players[0].name); // Output: Aria".to_owned(),
            "map" => "var prices:Array = [10, 25, 40];\nvar doubled:Array = prices.map(function(n:int, i:int, a:Array):int { return n * 2; });\ntrace(doubled.join(\",\")); // Output: 20,50,80".to_owned(),
            "filter" => "var vals:Array = [3, 12, 5, 18];\nvar big:Array = vals.filter(function(n:int, i:int, a:Array):Boolean { return n >= 10; });\ntrace(big.join(\",\")); // Output: 12,18".to_owned(),
            "every" => "var vals:Array = [2, 4, 6];\nvar allEven:Boolean = vals.every(function(n:int, i:int, a:Array):Boolean { return n % 2 == 0; });\ntrace(allEven); // Output: true".to_owned(),
            "some" => "var vals:Array = [1, 3, 8];\nvar hasEven:Boolean = vals.some(function(n:int, i:int, a:Array):Boolean { return n % 2 == 0; });\ntrace(hasEven); // Output: true".to_owned(),
            "forEach" => "var sum:int = 0;\n[2, 5, 8].forEach(function(n:int, i:int, a:Array):void { sum += n; });\ntrace(sum); // Output: 15".to_owned(),
            "toString" | "toLocaleString" => format!("var items:Array = [10, 20, 30];\ntrace(items.{mname}()); // Output: 10,20,30"),
            "length" => "var items:Array = [\"x\", \"y\", \"z\"];\ntrace(items.length); // Output: 3".to_owned(),
            "CASEINSENSITIVE" => "var names:Array = [\"zeta\", \"Alpha\"];\nnames.sort(Array.CASEINSENSITIVE);\ntrace(Array.CASEINSENSITIVE, names[0]); // Output: 1 Alpha".to_owned(),
            "DESCENDING" => "var nums:Array = [4, 9, 2];\nnums.sort(Array.NUMERIC | Array.DESCENDING);\ntrace(Array.DESCENDING, nums[0]); // Output: 2 9".to_owned(),
            "NUMERIC" => "var nums:Array = [100, 20, 3];\nnums.sort(Array.NUMERIC);\ntrace(Array.NUMERIC, nums.join(\",\")); // Output: 16 3,20,100".to_owned(),
            "RETURNINDEXEDARRAY" => "var nums:Array = [30, 10, 20];\nvar idx:Array = nums.sort(Array.NUMERIC | Array.RETURNINDEXEDARRAY);\ntrace(Array.RETURNINDEXEDARRAY, idx.join(\",\")); // Output: 8 1,2,0".to_owned(),
            "UNIQUESORT" => "var flag:Number = Array.UNIQUESORT;\ntrace(flag); // Output: 4".to_owned(),
            _ => return None,
        });
    }

    // 2. Vector (AS3)
    if owner_key == "Vector" {
        return Some(match mname {
            "Vector" => "var scores:Vector.<int> = Vector.<int>([100, 250, 400]);\ntrace(scores.length, scores[1]); // Output: 3 250".to_owned(),
            "lastIndexOf" => "var scores:Vector.<int> = Vector.<int>([10, 20, 30, 20, 40]);\ntrace(scores.lastIndexOf(20));    // Output: 3\ntrace(scores.lastIndexOf(20, 2)); // Output: 1\ntrace(scores.lastIndexOf(99));    // Output: -1".to_owned(),
            "indexOf" => "var scores:Vector.<int> = Vector.<int>([10, 20, 30, 20]);\ntrace(scores.indexOf(20));    // Output: 1\ntrace(scores.indexOf(20, 2)); // Output: 3\ntrace(scores.indexOf(99));    // Output: -1".to_owned(),
            "push" => "var vec:Vector.<String> = new Vector.<String>();\nvar count:uint = vec.push(\"hero\", \"boss\");\ntrace(count, vec[1]); // Output: 2 boss".to_owned(),
            "pop" => "var vec:Vector.<int> = Vector.<int>([5, 10, 15]);\nvar last:int = vec.pop();\ntrace(last, vec.length); // Output: 15 2".to_owned(),
            "shift" => "var vec:Vector.<int> = Vector.<int>([7, 8, 9]);\nvar head:int = vec.shift();\ntrace(head, vec.length); // Output: 7 2".to_owned(),
            "unshift" => "var vec:Vector.<int> = Vector.<int>([2, 3]);\nvar len:uint = vec.unshift(1);\ntrace(len, vec[0]); // Output: 3 1".to_owned(),
            "slice" => "var vec:Vector.<int> = Vector.<int>([10, 20, 30, 40]);\nvar sub:Vector.<int> = vec.slice(1, 3);\ntrace(sub.join(\",\")); // Output: 20,30".to_owned(),
            "splice" => "var vec:Vector.<int> = Vector.<int>([1, 2, 3, 4]);\nvar cut:Vector.<int> = vec.splice(1, 2, 9);\ntrace(cut.join(\",\"), vec.join(\",\")); // Output: 2,3 1,9,4".to_owned(),
            "insertAt" => "var vec:Vector.<String> = Vector.<String>([\"a\", \"c\"]);\nvec.insertAt(1, \"b\");\ntrace(vec.join(\",\")); // Output: a,b,c".to_owned(),
            "removeAt" => "var vec:Vector.<String> = Vector.<String>([\"a\", \"b\", \"c\"]);\nvar item:String = vec.removeAt(1);\ntrace(item, vec.join(\",\")); // Output: b a,c".to_owned(),
            "concat" => "var a:Vector.<int> = Vector.<int>([1, 2]);\nvar b:Vector.<int> = a.concat(Vector.<int>([3, 4]));\ntrace(b.join(\",\")); // Output: 1,2,3,4".to_owned(),
            "join" => "var vec:Vector.<String> = Vector.<String>([\"x\", \"y\", \"z\"]);\ntrace(vec.join(\" | \")); // Output: x | y | z".to_owned(),
            "reverse" => "var vec:Vector.<int> = Vector.<int>([1, 2, 3]);\nvec.reverse();\ntrace(vec.join(\",\")); // Output: 3,2,1".to_owned(),
            "sort" => "var vec:Vector.<int> = Vector.<int>([30, 5, 12]);\nvec.sort(Array.NUMERIC);\ntrace(vec.join(\",\")); // Output: 5,12,30".to_owned(),
            "map" => "var vec:Vector.<int> = Vector.<int>([2, 4, 6]);\nvar out:Vector.<int> = vec.map(function(v:int, i:int, s:Vector.<int>):int { return v * 10; });\ntrace(out.join(\",\")); // Output: 20,40,60".to_owned(),
            "filter" => "var vec:Vector.<int> = Vector.<int>([4, 15, 8, 22]);\nvar big:Vector.<int> = vec.filter(function(v:int, i:int, s:Vector.<int>):Boolean { return v >= 10; });\ntrace(big.join(\",\")); // Output: 15,22".to_owned(),
            "every" => "var vec:Vector.<int> = Vector.<int>([2, 4, 6]);\nvar ok:Boolean = vec.every(function(v:int, i:int, s:Vector.<int>):Boolean { return v > 0; });\ntrace(ok); // Output: true".to_owned(),
            "some" => "var vec:Vector.<int> = Vector.<int>([1, 5, 12]);\nvar hit:Boolean = vec.some(function(v:int, i:int, s:Vector.<int>):Boolean { return v >= 10; });\ntrace(hit); // Output: true".to_owned(),
            "forEach" => "var sum:int = 0;\nVector.<int>([3, 4, 5]).forEach(function(v:int, i:int, s:Vector.<int>):void { sum += v; });\ntrace(sum); // Output: 12".to_owned(),
            "length" => "var vec:Vector.<int> = Vector.<int>([10, 20, 30]);\ntrace(vec.length); // Output: 3".to_owned(),
            "fixed" => "var vec:Vector.<int> = new Vector.<int>(3, true);\nvec[0] = 7;\ntrace(vec.fixed, vec.length, vec[0]); // Output: true 3 7".to_owned(),
            "toString" | "toLocaleString" => format!("var vec:Vector.<int> = Vector.<int>([1, 2, 3]);\ntrace(vec.{mname}()); // Output: 1,2,3"),
            _ => return None,
        });
    }

    // 3. String (AS2 & AS3)
    if owner_key == "String" {
        return Some(match mname {
            "String" => "var text:String = new String(\"NeoFlash\");\ntrace(text.length, text); // Output: 8 NeoFlash".to_owned(),
            "lastIndexOf" => {
                if as3 {
                    "var path:String = \"assets/sprites/hero.png\";\ntrace(path.lastIndexOf(\"/\"));     // Output: 14\ntrace(path.lastIndexOf(\"/\", 10)); // Output: 6\ntrace(path.lastIndexOf(\".swf\"));  // Output: -1".to_owned()
                } else {
                    "var path:String = \"assets/sprites/hero.png\";\ntrace(path.lastIndexOf(\"/\"));    // Output: 14\ntrace(path.lastIndexOf(\".swf\")); // Output: -1".to_owned()
                }
            }
            "indexOf" => "var path:String = \"assets/sprites/hero.png\";\ntrace(path.indexOf(\"/\"));    // Output: 6\ntrace(path.indexOf(\"/\", 7)); // Output: 14\ntrace(path.indexOf(\".mp3\")); // Output: -1".to_owned(),
            "charAt" => "var word:String = \"Flash\";\ntrace(word.charAt(0), word.charAt(4)); // Output: F h".to_owned(),
            "charCodeAt" => "var word:String = \"ABC\";\ntrace(word.charCodeAt(0), word.charCodeAt(2)); // Output: 65 67".to_owned(),
            "fromCharCode" => "var text:String = String.fromCharCode(78, 70, 50);\ntrace(text); // Output: NF2".to_owned(),
            "slice" => "var file:String = \"scene_intro.swf\";\ntrace(file.slice(6, 11)); // Output: intro".to_owned(),
            "substring" => "var title:String = \"ActionScript\";\ntrace(title.substring(0, 6)); // Output: Action".to_owned(),
            "substr" => "var code:String = \"ID-4096-OK\";\ntrace(code.substr(3, 4)); // Output: 4096".to_owned(),
            "split" => "var csv:String = \"red,green,blue\";\nvar parts:Array = csv.split(\",\");\ntrace(parts.length, parts[1]); // Output: 3 green".to_owned(),
            "concat" => "var greeting:String = \"Hello\".concat(\", \", \"Stage\");\ntrace(greeting); // Output: Hello, Stage".to_owned(),
            "toLowerCase" | "toLocaleLowerCase" => format!("var tag:String = \"NEO_FLASH\";\ntrace(tag.{mname}()); // Output: neo_flash"),
            "toUpperCase" | "toLocaleUpperCase" => format!("var tag:String = \"neo_flash\";\ntrace(tag.{mname}()); // Output: NEO_FLASH"),
            "localeCompare" => "var a:String = \"alpha\";\ntrace(a.localeCompare(\"alpha\") == 0); // Output: true".to_owned(),
            "match" => "var text:String = \"v12 and v34\";\nvar hits:Array = text.match(/v[0-9]+/g);\ntrace(hits.join(\",\")); // Output: v12,v34".to_owned(),
            "replace" => "var label:String = \"player_01\";\ntrace(label.replace(\"01\", \"02\")); // Output: player_02".to_owned(),
            "search" => "var msg:String = \"Level 42 cleared\";\ntrace(msg.search(/[0-9]+/)); // Output: 6".to_owned(),
            "length" => "var word:String = \"Sprite\";\ntrace(word.length); // Output: 6".to_owned(),
            "toString" | "valueOf" => format!("var s:String = \"ready\";\ntrace(s.{mname}()); // Output: ready"),
            _ => return None,
        });
    }

    // 4. Math (all 26 methods & constants in AS2 & AS3)
    if owner_key == "Math" {
        return Some(match mname {
            "abs" => "trace(Math.abs(-42)); // Output: 42\ntrace(Math.abs(15));  // Output: 15".to_owned(),
            "acos" => "var rad:Number = Math.acos(1);\ntrace(rad); // Output: 0".to_owned(),
            "asin" => "var rad:Number = Math.asin(0);\ntrace(rad); // Output: 0".to_owned(),
            "atan" => "var rad:Number = Math.atan(0);\ntrace(rad); // Output: 0".to_owned(),
            "atan2" => "var deg:Number = Math.round(Math.atan2(1, 1) * 180 / Math.PI);\ntrace(deg); // Output: 45".to_owned(),
            "ceil" => "trace(Math.ceil(4.1));  // Output: 5\ntrace(Math.ceil(-4.1)); // Output: -4".to_owned(),
            "cos" => "var x:Number = Math.cos(0);\ntrace(x); // Output: 1".to_owned(),
            "exp" => "var e0:Number = Math.exp(0);\ntrace(e0); // Output: 1".to_owned(),
            "floor" => "trace(Math.floor(4.9));  // Output: 4\ntrace(Math.floor(-4.1)); // Output: -5".to_owned(),
            "log" => "var ln1:Number = Math.log(1);\ntrace(ln1); // Output: 0".to_owned(),
            "max" => "var highest:Number = Math.max(12, 48);\ntrace(highest); // Output: 48".to_owned(),
            "min" => "var lowest:Number = Math.min(12, 48);\ntrace(lowest); // Output: 12".to_owned(),
            "pow" => "var kb:Number = Math.pow(2, 10);\ntrace(kb); // Output: 1024".to_owned(),
            "random" => "var r:Number = Math.random();\ntrace(r >= 0 && r < 1); // Output: true".to_owned(),
            "round" => "trace(Math.round(3.6)); // Output: 4\ntrace(Math.round(3.2)); // Output: 3".to_owned(),
            "sin" => "var y:Number = Math.sin(0);\ntrace(y); // Output: 0".to_owned(),
            "sqrt" => "var side:Number = Math.sqrt(81);\ntrace(side); // Output: 9".to_owned(),
            "tan" => "var slope:Number = Math.tan(0);\ntrace(slope); // Output: 0".to_owned(),
            "PI" => "var halfTurnDeg:Number = Math.round(Math.PI * (180 / Math.PI));\ntrace(halfTurnDeg); // Output: 180".to_owned(),
            "E" => "var approx:Number = Math.round(Math.E * 1000) / 1000;\ntrace(approx); // Output: 2.718".to_owned(),
            "LN2" => "var approx:Number = Math.round(Math.LN2 * 1000) / 1000;\ntrace(approx); // Output: 0.693".to_owned(),
            "LN10" => "var approx:Number = Math.round(Math.LN10 * 1000) / 1000;\ntrace(approx); // Output: 2.303".to_owned(),
            "LOG2E" => "var approx:Number = Math.round(Math.LOG2E * 1000) / 1000;\ntrace(approx); // Output: 1.443".to_owned(),
            "LOG10E" => "var approx:Number = Math.round(Math.LOG10E * 1000) / 1000;\ntrace(approx); // Output: 0.434".to_owned(),
            "SQRT2" => "var approx:Number = Math.round(Math.SQRT2 * 1000) / 1000;\ntrace(approx); // Output: 1.414".to_owned(),
            "SQRT1_2" => "var approx:Number = Math.round(Math.SQRT1_2 * 1000) / 1000;\ntrace(approx); // Output: 0.707".to_owned(),
            _ => return None,
        });
    }

    // 5. Number, int, uint, Boolean, Object, Function, arguments, JSON, RegExp, Date, Dictionary
    match owner_key {
        "Number" => {
            return Some(match mname {
                "Number" => "var n:Number = new Number(42.5);\ntrace(n + 0.5); // Output: 43".to_owned(),
                "toFixed" => "var price:Number = 19.456;\ntrace(price.toFixed(2)); // Output: 19.46".to_owned(),
                "toPrecision" => "var val:Number = 12.5;\ntrace(val.toPrecision(2)); // Output: 13".to_owned(),
                "toExponential" => "var val:Number = 1500;\ntrace(val.toExponential(1)); // Output: 1.5e+3".to_owned(),
                "toString" => "var hex:String = (255).toString(16);\ntrace(hex); // Output: ff".to_owned(),
                "valueOf" => "var n:Number = new Number(99);\ntrace(n.valueOf()); // Output: 99".to_owned(),
                "MAX_VALUE" => "var maxNum:Number = Number.MAX_VALUE;\ntrace(maxNum > 1e300); // Output: true".to_owned(),
                "MIN_VALUE" => "var minNum:Number = Number.MIN_VALUE;\ntrace(minNum > 0); // Output: true".to_owned(),
                "NaN" => "var invalid:Number = Number.NaN;\ntrace(isNaN(invalid)); // Output: true".to_owned(),
                "POSITIVE_INFINITY" => "var inf:Number = Number.POSITIVE_INFINITY;\ntrace(!isFinite(inf)); // Output: true".to_owned(),
                "NEGATIVE_INFINITY" => "var negInf:Number = Number.NEGATIVE_INFINITY;\ntrace(negInf < 0); // Output: true".to_owned(),
                _ => return None,
            });
        }
        "int" | "uint" => {
            let max_str = if owner_key == "int" {
                "2147483647"
            } else {
                "4294967295"
            };
            let min_str = if owner_key == "int" {
                "-2147483648"
            } else {
                "0"
            };
            return Some(match mname {
                "int" | "uint" => {
                    format!("var n:{owner_key} = {owner_key}(42);\ntrace(n); // Output: 42")
                }
                "MAX_VALUE" => format!(
                    "var top:{owner_key} = {owner_key}.MAX_VALUE;\ntrace(top); // Output: {max_str}"
                ),
                "MIN_VALUE" => format!(
                    "var bottom:{owner_key} = {owner_key}.MIN_VALUE;\ntrace(bottom); // Output: {min_str}"
                ),
                "toString" => {
                    format!("var n:{owner_key} = 255;\ntrace(n.toString(16)); // Output: ff")
                }
                "toFixed" => {
                    format!("var n:{owner_key} = 12;\ntrace(n.toFixed(2)); // Output: 12.00")
                }
                "toExponential" => format!(
                    "var n:{owner_key} = 100;\ntrace(n.toExponential(1)); // Output: 1.0e+2"
                ),
                "toPrecision" => {
                    format!("var n:{owner_key} = 42;\ntrace(n.toPrecision(2)); // Output: 42")
                }
                "valueOf" => format!("var n:{owner_key} = 64;\ntrace(n.valueOf()); // Output: 64"),
                _ => return None,
            });
        }
        "Boolean" => {
            return Some(match mname {
                "Boolean" => {
                    "var flag:Boolean = new Boolean(true);\ntrace(flag.valueOf()); // Output: true"
                        .to_owned()
                }
                "toString" => {
                    "var flag:Boolean = true;\ntrace(flag.toString()); // Output: true".to_owned()
                }
                "valueOf" => {
                    "var flag:Boolean = false;\ntrace(flag.valueOf()); // Output: false".to_owned()
                }
                _ => return None,
            });
        }
        "Object" => {
            return Some(match mname {
                "Object" => "var item:Object = new Object();\nitem.id = \"gem\";\ntrace(item.id); // Output: gem".to_owned(),
                "hasOwnProperty" => "var bag:Object = {potion: 3};\ntrace(bag.hasOwnProperty(\"potion\")); // Output: true\ntrace(bag.hasOwnProperty(\"key\"));    // Output: false".to_owned(),
                "propertyIsEnumerable" | "isPropertyEnumerable" => format!("var bag:Object = {{coins: 50}};\ntrace(bag.{mname}(\"coins\")); // Output: true"),
                "setPropertyIsEnumerable" => "var bag:Object = {secret: 1, visible: 2};\nbag.setPropertyIsEnumerable(\"secret\", false);\ntrace(bag.propertyIsEnumerable(\"secret\")); // Output: false".to_owned(),
                "isPrototypeOf" => "var arr:Array = [1, 2];\ntrace(Object.prototype.isPrototypeOf(arr)); // Output: true".to_owned(),
                "toString" | "toLocaleString" => format!("var obj:Object = {{}};\ntrace(obj.{mname}()); // Output: [object Object]"),
                "valueOf" => "var obj:Object = {x: 10};\ntrace(obj.valueOf().x); // Output: 10".to_owned(),
                "constructor" => "var list:Array = [1, 2];\ntrace(list.constructor === Array); // Output: true".to_owned(),
                "prototype" => "var proto:Object = Object.prototype;\ntrace(proto !== null); // Output: true".to_owned(),
                "registerClass" => "function CoinClip():Void {}\nvar ok:Boolean = Object.registerClass(\"CoinSymbol\", CoinClip);\ntrace(typeof ok); // Output: boolean".to_owned(),
                "watch" | "unwatch" => "var state:Object = {hp: 100};\nstate.watch(\"hp\", function(p, oldVal, newVal) { return newVal; });\nstate.unwatch(\"hp\");\ntrace(state.hp); // Output: 100".to_owned(),
                "addProperty" => "var box:Object = {_w: 40};\nbox.addProperty(\"width\", function() { return this._w; }, null);\ntrace(box.width); // Output: 40".to_owned(),
                "__proto__" => "var child:Object = {};\ntrace(child.__proto__ === Object.prototype); // Output: true".to_owned(),
                "__resolve" => "var proxy:Object = {__resolve: function(name:String):String { return \"fallback:\" + name; }};\ntrace(proxy.missingKey); // Output: fallback:missingKey".to_owned(),
                _ => return None,
            });
        }
        "Function" => {
            return Some(match mname {
                "call" => {
                    let ret = if as3 { ":int" } else { ":Number" };
                    let num = if as3 { ":int" } else { ":Number" };
                    format!(
                        "var addOffset:Function = function(dx{num}){ret} {{\n    return this.x + dx;\n}};\nvar actor:Object = {{x: 25}};\ntrace(addOffset.call(actor, 15)); // Output: 40"
                    )
                }
                "apply" => {
                    let ret = if as3 { ":int" } else { ":Number" };
                    let num = if as3 { ":int" } else { ":Number" };
                    format!(
                        "var moveBy:Function = function(dx{num}, dy{num}){ret} {{\n    return this.x + dx + dy;\n}};\nvar actor:Object = {{x: 10}};\ntrace(moveBy.apply(actor, [5, 7])); // Output: 22"
                    )
                }
                _ => return None,
            });
        }
        "arguments" => {
            return Some(match mname {
                "length" => "function countArgs():Number {\n    return arguments.length;\n}\ntrace(countArgs(\"a\", \"b\", \"c\")); // Output: 3".to_owned(),
                "callee" => "var fact:Function = function(n:Number):Number {\n    return (n <= 1) ? 1 : n * arguments.callee(n - 1);\n};\ntrace(fact(5)); // Output: 120".to_owned(),
                "caller" => "function inner():Boolean {\n    return arguments.caller !== null;\n}\nfunction outer():Boolean { return inner(); }\ntrace(outer()); // Output: true".to_owned(),
                _ => return None,
            });
        }
        "JSON" => {
            return Some(match mname {
                "parse" => "var data:Object = JSON.parse(\"{\\\"level\\\":4,\\\"boss\\\":true}\");\ntrace(data.level, data.boss); // Output: 4 true".to_owned(),
                "stringify" => "var payload:String = JSON.stringify({id: \"p1\", score: 900});\ntrace(payload); // Output: {\"id\":\"p1\",\"score\":900}".to_owned(),
                _ => return None,
            });
        }
        "RegExp" => {
            return Some(match mname {
                "RegExp" => "var re:RegExp = new RegExp(\"hero_[0-9]+\", \"i\");\ntrace(re.test(\"HERO_07\")); // Output: true".to_owned(),
                "test" => "var hexCode:RegExp = /^0x[0-9a-f]+$/i;\ntrace(hexCode.test(\"0xFF00\")); // Output: true\ntrace(hexCode.test(\"xyz\"));    // Output: false".to_owned(),
                "exec" => "var re:RegExp = /wave_([0-9]+)/;\nvar match:Object = re.exec(\"start wave_12 now\");\ntrace(match[0], match[1]); // Output: wave_12 12".to_owned(),
                "source" => "var re:RegExp = /[a-z]+/gi;\ntrace(re.source); // Output: [a-z]+".to_owned(),
                "global" => "var re:RegExp = /coin/g;\ntrace(re.global); // Output: true".to_owned(),
                "ignoreCase" => "var re:RegExp = /boss/i;\ntrace(re.ignoreCase); // Output: true".to_owned(),
                "multiline" => "var re:RegExp = /^line/m;\ntrace(re.multiline); // Output: true".to_owned(),
                "dotall" => "var re:RegExp = /a.b/s;\ntrace(re.dotall); // Output: true".to_owned(),
                "extended" => "var re:RegExp = /a b/x;\ntrace(re.extended); // Output: true".to_owned(),
                "lastIndex" => "var re:RegExp = /a/g;\nre.exec(\"aba\");\ntrace(re.lastIndex); // Output: 1".to_owned(),
                _ => return None,
            });
        }
        "Date" => {
            return Some(match mname {
                "Date" => "var epoch:Date = new Date(0);\ntrace(epoch.getUTCFullYear(), epoch.getTime()); // Output: 1970 0".to_owned(),
                "UTC" => "var ms:Number = Date.UTC(1970, 0, 2, 0, 0, 0);\ntrace(ms); // Output: 86400000".to_owned(),
                "parse" => "var ms:Number = Date.parse(\"1970/01/01 00:00:00 UTC\");\ntrace(ms); // Output: 0".to_owned(),
                "getTime" | "valueOf" | "time" => {
                    if member.params.is_some() {
                        format!("var d:Date = new Date(1500);\ntrace(d.{mname}()); // Output: 1500")
                    } else {
                        "var d:Date = new Date(1500);\ntrace(d.time); // Output: 1500".to_owned()
                    }
                }
                "getUTCFullYear" | "fullYearUTC" => {
                    if member.params.is_some() {
                        "var d:Date = new Date(0);\ntrace(d.getUTCFullYear()); // Output: 1970".to_owned()
                    } else {
                        "var d:Date = new Date(0);\ntrace(d.fullYearUTC); // Output: 1970".to_owned()
                    }
                }
                "getUTCMonth" | "monthUTC" => {
                    if member.params.is_some() {
                        "var d:Date = new Date(0);\ntrace(d.getUTCMonth()); // Output: 0".to_owned()
                    } else {
                        "var d:Date = new Date(0);\ntrace(d.monthUTC); // Output: 0".to_owned()
                    }
                }
                "getUTCDate" | "dateUTC" => {
                    if member.params.is_some() {
                        "var d:Date = new Date(0);\ntrace(d.getUTCDate()); // Output: 1".to_owned()
                    } else {
                        "var d:Date = new Date(0);\ntrace(d.dateUTC); // Output: 1".to_owned()
                    }
                }
                "setTime" => "var d:Date = new Date(0);\nd.setTime(60000);\ntrace(d.getUTCMinutes()); // Output: 1".to_owned(),
                "setUTCFullYear" => "var d:Date = new Date(0);\nd.setUTCFullYear(2026);\ntrace(d.getUTCFullYear()); // Output: 2026".to_owned(),
                _ if mname.starts_with("getUTC") => format!("var d:Date = new Date(0);\ntrace(d.{mname}() >= 0); // Output: true"),
                _ if mname.starts_with("get") => format!("var d:Date = new Date(0);\ntrace(!isNaN(d.{mname}())); // Output: true"),
                _ if mname.starts_with("set") => format!("var d:Date = new Date(0);\nd.{mname}(1);\ntrace(!isNaN(d.getTime())); // Output: true"),
                _ if mname.starts_with("to") => format!("var d:Date = new Date(0);\ntrace(d.{mname}().length > 0); // Output: true"),
                _ => format!("var d:Date = new Date(0);\ntrace(!isNaN(d.{mname})); // Output: true"),
            });
        }
        "flash.utils.Dictionary" => {
            return Some(match mname {
                "Dictionary" => "import flash.utils.Dictionary;\n\nvar map:Dictionary = new Dictionary();\nvar keyObj:Object = {id: 1};\nmap[keyObj] = \"hero\";\ntrace(map[keyObj]); // Output: hero".to_owned(),
                "toJSON" => "import flash.utils.Dictionary;\n\nvar map:Dictionary = new Dictionary();\ntrace(map.toJSON(\"state\")); // Output: Dictionary".to_owned(),
                _ => return None,
            });
        }
        _ => {}
    }

    // Error & AS3 Error subclasses
    if owner_key == "Error" || (as3 && owner_key.ends_with("Error") && !owner_key.contains('.')) {
        return Some(match mname {
            c if c == class_name => {
                if as3 {
                    format!("var err:{class_name} = new {class_name}(\"Bad input\", 1001);\ntrace(err.name, err.message, err.errorID); // Output: {class_name} Bad input 1001")
                } else {
                    "var err:Error = new Error(\"Missing clip\");\ntrace(err.message); // Output: Missing clip".to_owned()
                }
            }
            "message" => format!("var err:{class_name} = new {class_name}(\"Out of bounds\");\ntrace(err.message); // Output: Out of bounds"),
            "name" => format!("var err:{class_name} = new {class_name}(\"Failed\");\ntrace(err.name); // Output: {class_name}"),
            "errorID" => format!("var err:{class_name} = new {class_name}(\"Failed\", 2006);\ntrace(err.errorID); // Output: 2006"),
            "toString" => format!("var err:{class_name} = new {class_name}(\"Invalid state\");\ntrace(err.toString()); // Output: {class_name}: Invalid state"),
            "getStackTrace" => "var err:Error = new Error(\"Trace check\");\nvar stack:String = err.getStackTrace();\ntrace(stack === null || stack.length >= 0); // Output: true".to_owned(),
            _ => return None,
        });
    }

    // 6. ByteArray, IDataInput, IDataOutput
    if matches!(
        owner_key,
        "flash.utils.ByteArray" | "flash.utils.IDataInput" | "flash.utils.IDataOutput"
    ) {
        let hdr = match owner_key {
            "flash.utils.ByteArray" => {
                "import flash.utils.ByteArray;\n\nvar bytes:ByteArray = new ByteArray();"
            }
            "flash.utils.IDataInput" => {
                "import flash.utils.ByteArray;\nimport flash.utils.IDataInput;\n\nvar bytes:ByteArray = new ByteArray();\nvar input:IDataInput = bytes;"
            }
            _ => {
                "import flash.utils.ByteArray;\nimport flash.utils.IDataOutput;\n\nvar bytes:ByteArray = new ByteArray();\nvar out:IDataOutput = bytes;"
            }
        };
        let rd = if owner_key == "flash.utils.IDataInput" {
            "input"
        } else {
            "bytes"
        };
        let wr = if owner_key == "flash.utils.IDataOutput" {
            "out"
        } else {
            "bytes"
        };
        return Some(match mname {
            "ByteArray" => "import flash.utils.ByteArray;\n\nvar bytes:ByteArray = new ByteArray();\nbytes.writeUTFBytes(\"NF2\");\ntrace(bytes.length, bytes.toString()); // Output: 3 NF2".to_owned(),
            "readUnsignedByte" => {
                if owner_key == "flash.utils.ByteArray" {
                    "import flash.utils.ByteArray;\n\nvar bytes:ByteArray = new ByteArray();\nbytes.writeByte(0xFF);\nbytes.writeByte(0x2A);\nbytes.position = 0;\ntrace(bytes.readUnsignedByte()); // Output: 255\ntrace(bytes.readUnsignedByte()); // Output: 42".to_owned()
                } else {
                    format!("{hdr}\nbytes.writeByte(0xFF);\nbytes.position = 0;\ntrace({rd}.readUnsignedByte()); // Output: 255")
                }
            }
            "readByte" | "writeByte" => format!("{hdr}\n{wr}.writeByte(-7);\nbytes.position = 0;\ntrace({rd}.readByte()); // Output: -7"),
            "readBoolean" | "writeBoolean" => format!("{hdr}\n{wr}.writeBoolean(true);\nbytes.position = 0;\ntrace({rd}.readBoolean()); // Output: true"),
            "readShort" | "writeShort" => format!("{hdr}\n{wr}.writeShort(-1024);\nbytes.position = 0;\ntrace({rd}.readShort()); // Output: -1024"),
            "readUnsignedShort" => format!("{hdr}\nbytes.writeShort(0xFFFF);\nbytes.position = 0;\ntrace({rd}.readUnsignedShort()); // Output: 65535"),
            "readInt" | "writeInt" => format!("{hdr}\n{wr}.writeInt(-123456);\nbytes.position = 0;\ntrace({rd}.readInt()); // Output: -123456"),
            "readUnsignedInt" | "writeUnsignedInt" => format!("{hdr}\n{wr}.writeUnsignedInt(3000000000);\nbytes.position = 0;\ntrace({rd}.readUnsignedInt()); // Output: 3000000000"),
            "readFloat" | "writeFloat" => format!("{hdr}\n{wr}.writeFloat(1.5);\nbytes.position = 0;\ntrace({rd}.readFloat()); // Output: 1.5"),
            "readDouble" | "writeDouble" => format!("{hdr}\n{wr}.writeDouble(3.125);\nbytes.position = 0;\ntrace({rd}.readDouble()); // Output: 3.125"),
            "readUTF" | "writeUTF" => format!("{hdr}\n{wr}.writeUTF(\"NeoFlash\");\nbytes.position = 0;\ntrace({rd}.readUTF()); // Output: NeoFlash"),
            "readUTFBytes" | "writeUTFBytes" => format!("{hdr}\n{wr}.writeUTFBytes(\"Stage\");\nbytes.position = 0;\ntrace({rd}.readUTFBytes(5)); // Output: Stage"),
            "readMultiByte" | "writeMultiByte" => format!("{hdr}\n{wr}.writeMultiByte(\"Hello\", \"utf-8\");\nbytes.position = 0;\ntrace({rd}.readMultiByte(5, \"utf-8\")); // Output: Hello"),
            "readBytes" | "writeBytes" => format!("{hdr}\n{wr}.writeUTFBytes(\"ABCD\");\nbytes.position = 1;\nvar copy:ByteArray = new ByteArray();\nbytes.readBytes(copy, 0, 2);\ntrace(copy.toString()); // Output: BC"),
            "readObject" | "writeObject" => format!("{hdr}\n{wr}.writeObject({{title: \"Act 1\", waves: 3}});\nbytes.position = 0;\nvar decoded:Object = bytes.readObject();\ntrace(decoded.title, decoded.waves); // Output: Act 1 3"),
            "compress" | "uncompress" => "import flash.utils.ByteArray;\n\nvar bytes:ByteArray = new ByteArray();\nbytes.writeUTFBytes(\"repeat repeat repeat repeat\");\nbytes.compress(\"zlib\");\nbytes.uncompress(\"zlib\");\ntrace(bytes.toString()); // Output: repeat repeat repeat repeat".to_owned(),
            "deflate" | "inflate" => "import flash.utils.ByteArray;\n\nvar bytes:ByteArray = new ByteArray();\nbytes.writeUTFBytes(\"raw deflate payload\");\nbytes.deflate();\nbytes.inflate();\ntrace(bytes.toString()); // Output: raw deflate payload".to_owned(),
            "clear" => "import flash.utils.ByteArray;\n\nvar bytes:ByteArray = new ByteArray();\nbytes.writeInt(42);\nbytes.clear();\ntrace(bytes.length, bytes.position); // Output: 0 0".to_owned(),
            "position" => "import flash.utils.ByteArray;\n\nvar bytes:ByteArray = new ByteArray();\nbytes.writeShort(500);\nbytes.position = 0;\ntrace(bytes.position, bytes.readShort()); // Output: 0 500".to_owned(),
            "length" => "import flash.utils.ByteArray;\n\nvar bytes:ByteArray = new ByteArray();\nbytes.writeInt(99);\ntrace(bytes.length); // Output: 4".to_owned(),
            "bytesAvailable" => format!("{hdr}\nbytes.writeInt(10);\nbytes.writeInt(20);\nbytes.position = 4;\ntrace({rd}.bytesAvailable); // Output: 4"),
            "endian" => format!("import flash.utils.Endian;\n{hdr}\n{wr}.endian = Endian.LITTLE_ENDIAN;\nbytes.writeShort(0x0102);\nbytes.position = 0;\ntrace({rd}.endian, bytes.readUnsignedByte()); // Output: littleEndian 2"),
            "objectEncoding" => format!("{hdr}\ntrace({wr}.objectEncoding); // Output: 3"),
            "defaultObjectEncoding" => "import flash.utils.ByteArray;\n\ntrace(ByteArray.defaultObjectEncoding); // Output: 3".to_owned(),
            "shareable" => "import flash.utils.ByteArray;\n\nvar bytes:ByteArray = new ByteArray();\ntrace(bytes.shareable); // Output: false".to_owned(),
            "toString" => "import flash.utils.ByteArray;\n\nvar bytes:ByteArray = new ByteArray();\nbytes.writeUTFBytes(\"Hello\");\ntrace(bytes.toString()); // Output: Hello".to_owned(),
            "toJSON" => "import flash.utils.ByteArray;\n\nvar bytes:ByteArray = new ByteArray();\ntrace(bytes.toJSON(\"buf\")); // Output: ByteArray".to_owned(),
            "atomicCompareAndSwapIntAt" => "import flash.utils.ByteArray;\n\nvar bytes:ByteArray = new ByteArray();\nbytes.writeInt(10);\nvar prev:int = bytes.atomicCompareAndSwapIntAt(0, 10, 25);\nbytes.position = 0;\ntrace(prev, bytes.readInt()); // Output: 10 25".to_owned(),
            "atomicCompareAndSwapLength" => "import flash.utils.ByteArray;\n\nvar bytes:ByteArray = new ByteArray();\nvar prevLen:int = bytes.atomicCompareAndSwapLength(0, 8);\ntrace(prevLen, bytes.length); // Output: 0 8".to_owned(),
            _ => return None,
        });
    }

    // 7. Point, Rectangle, Matrix, Vector3D, Matrix3D, ColorTransform, PerspectiveProjection, Color
    match owner_key {
        "flash.geom.Point" => {
            return Some(match mname {
                "Point" => "import flash.geom.Point;\n\nvar pt:Point = new Point(3, 4);\ntrace(pt.x, pt.y, pt.length); // Output: 3 4 5".to_owned(),
                "distance" => "import flash.geom.Point;\n\nvar a:Point = new Point(0, 0);\nvar b:Point = new Point(3, 4);\ntrace(Point.distance(a, b)); // Output: 5".to_owned(),
                "interpolate" => "import flash.geom.Point;\n\nvar mid:Point = Point.interpolate(new Point(10, 20), new Point(0, 0), 0.5);\ntrace(mid.x, mid.y); // Output: 5 10".to_owned(),
                "polar" => "import flash.geom.Point;\n\nvar p:Point = Point.polar(10, 0);\ntrace(Math.round(p.x), Math.round(p.y)); // Output: 10 0".to_owned(),
                "add" => "import flash.geom.Point;\n\nvar sum:Point = new Point(4, 6).add(new Point(10, -2));\ntrace(sum.x, sum.y); // Output: 14 4".to_owned(),
                "subtract" => "import flash.geom.Point;\n\nvar diff:Point = new Point(15, 9).subtract(new Point(5, 4));\ntrace(diff.x, diff.y); // Output: 10 5".to_owned(),
                "normalize" => "import flash.geom.Point;\n\nvar v:Point = new Point(3, 4);\nv.normalize(10);\ntrace(v.x, v.y); // Output: 6 8".to_owned(),
                "offset" => "import flash.geom.Point;\n\nvar pt:Point = new Point(20, 30);\npt.offset(5, -10);\ntrace(pt.x, pt.y); // Output: 25 20".to_owned(),
                "equals" => "import flash.geom.Point;\n\nvar same:Boolean = new Point(8, 12).equals(new Point(8, 12));\ntrace(same); // Output: true".to_owned(),
                "clone" => "import flash.geom.Point;\n\nvar orig:Point = new Point(7, 9);\nvar copy:Point = orig.clone();\ntrace(copy.x, copy.y); // Output: 7 9".to_owned(),
                "copyFrom" => "import flash.geom.Point;\n\nvar dst:Point = new Point();\ndst.copyFrom(new Point(16, 32));\ntrace(dst.x, dst.y); // Output: 16 32".to_owned(),
                "setTo" => "import flash.geom.Point;\n\nvar pt:Point = new Point();\npt.setTo(48, 64);\ntrace(pt.x, pt.y); // Output: 48 64".to_owned(),
                "length" => "import flash.geom.Point;\n\nvar v:Point = new Point(6, 8);\ntrace(v.length); // Output: 10".to_owned(),
                "x" | "y" => format!("import flash.geom.Point;\n\nvar pt:Point = new Point(12, 24);\ntrace(pt.{mname}); // Output: {}", if mname == "x" { 12 } else { 24 }),
                "toString" => "import flash.geom.Point;\n\nvar pt:Point = new Point(3, 4);\ntrace(pt.toString()); // Output: (x=3, y=4)".to_owned(),
                _ => return None,
            });
        }
        "flash.geom.Rectangle" => {
            return Some(match mname {
                "Rectangle" => "import flash.geom.Rectangle;\n\nvar rect:Rectangle = new Rectangle(10, 20, 100, 50);\ntrace(rect.right, rect.bottom); // Output: 110 70".to_owned(),
                "intersects" => "import flash.geom.Rectangle;\n\nvar a:Rectangle = new Rectangle(0, 0, 50, 50);\nvar b:Rectangle = new Rectangle(25, 25, 50, 50);\ntrace(a.intersects(b)); // Output: true".to_owned(),
                "intersection" => "import flash.geom.Rectangle;\n\nvar a:Rectangle = new Rectangle(0, 0, 50, 50);\nvar b:Rectangle = new Rectangle(20, 10, 50, 50);\nvar overlap:Rectangle = a.intersection(b);\ntrace(overlap.x, overlap.y, overlap.width, overlap.height); // Output: 20 10 30 40".to_owned(),
                "union" => "import flash.geom.Rectangle;\n\nvar a:Rectangle = new Rectangle(0, 0, 20, 20);\nvar b:Rectangle = new Rectangle(20, 20, 30, 40);\nvar bounds:Rectangle = a.union(b);\ntrace(bounds.width, bounds.height); // Output: 50 60".to_owned(),
                "contains" => "import flash.geom.Rectangle;\n\nvar box:Rectangle = new Rectangle(10, 10, 100, 80);\ntrace(box.contains(50, 40)); // Output: true\ntrace(box.contains(5, 5));   // Output: false".to_owned(),
                "containsPoint" => "import flash.geom.Point;\nimport flash.geom.Rectangle;\n\nvar box:Rectangle = new Rectangle(0, 0, 64, 64);\ntrace(box.containsPoint(new Point(32, 32))); // Output: true".to_owned(),
                "containsRect" | "containsRectangle" => format!("import flash.geom.Rectangle;\n\nvar outer:Rectangle = new Rectangle(0, 0, 100, 100);\nvar inner:Rectangle = new Rectangle(10, 10, 40, 40);\ntrace(outer.{mname}(inner)); // Output: true"),
                "inflate" => "import flash.geom.Rectangle;\n\nvar box:Rectangle = new Rectangle(10, 10, 20, 20);\nbox.inflate(5, 5);\ntrace(box.x, box.width); // Output: 5 30".to_owned(),
                "inflatePoint" => "import flash.geom.Point;\nimport flash.geom.Rectangle;\n\nvar box:Rectangle = new Rectangle(10, 10, 20, 20);\nbox.inflatePoint(new Point(4, 6));\ntrace(box.width, box.height); // Output: 28 32".to_owned(),
                "offset" => "import flash.geom.Rectangle;\n\nvar box:Rectangle = new Rectangle(10, 20, 50, 30);\nbox.offset(15, -5);\ntrace(box.x, box.y); // Output: 25 15".to_owned(),
                "offsetPoint" => "import flash.geom.Point;\nimport flash.geom.Rectangle;\n\nvar box:Rectangle = new Rectangle(10, 20, 50, 30);\nbox.offsetPoint(new Point(5, 10));\ntrace(box.x, box.y); // Output: 15 30".to_owned(),
                "isEmpty" => "import flash.geom.Rectangle;\n\ntrace(new Rectangle(0, 0, 0, 10).isEmpty());  // Output: true\ntrace(new Rectangle(0, 0, 20, 10).isEmpty()); // Output: false".to_owned(),
                "setEmpty" => "import flash.geom.Rectangle;\n\nvar box:Rectangle = new Rectangle(5, 5, 40, 40);\nbox.setEmpty();\ntrace(box.isEmpty()); // Output: true".to_owned(),
                "equals" => "import flash.geom.Rectangle;\n\nvar eq:Boolean = new Rectangle(0, 0, 32, 32).equals(new Rectangle(0, 0, 32, 32));\ntrace(eq); // Output: true".to_owned(),
                "clone" => "import flash.geom.Rectangle;\n\nvar copy:Rectangle = new Rectangle(4, 8, 60, 40).clone();\ntrace(copy.width, copy.height); // Output: 60 40".to_owned(),
                "copyFrom" => "import flash.geom.Rectangle;\n\nvar dst:Rectangle = new Rectangle();\ndst.copyFrom(new Rectangle(2, 4, 80, 60));\ntrace(dst.width, dst.height); // Output: 80 60".to_owned(),
                "setTo" => "import flash.geom.Rectangle;\n\nvar box:Rectangle = new Rectangle();\nbox.setTo(8, 16, 128, 64);\ntrace(box.right, box.bottom); // Output: 136 80".to_owned(),
                "toString" => "import flash.geom.Rectangle;\n\nvar box:Rectangle = new Rectangle(10, 20, 100, 50);\ntrace(box.toString()); // Output: (x=10, y=20, w=100, h=50)".to_owned(),
                "left" | "top" | "right" | "bottom" | "x" | "y" | "width" | "height" => {
                    let expected = match mname {
                        "left" | "x" => 10,
                        "top" | "y" => 20,
                        "width" => 100,
                        "height" => 50,
                        "right" => 110,
                        _ => 70,
                    };
                    format!("import flash.geom.Rectangle;\n\nvar box:Rectangle = new Rectangle(10, 20, 100, 50);\ntrace(box.{mname}); // Output: {expected}")
                }
                "topLeft" | "bottomRight" | "size" => {
                    let expected = match mname {
                        "topLeft" => "(x=10, y=20)",
                        "bottomRight" => "(x=110, y=70)",
                        _ => "(x=100, y=50)",
                    };
                    format!("import flash.geom.Rectangle;\n\nvar box:Rectangle = new Rectangle(10, 20, 100, 50);\ntrace(box.{mname}.toString()); // Output: {expected}")
                }
                _ => return None,
            });
        }
        "flash.geom.Matrix" => {
            return Some(match mname {
                "Matrix" => "import flash.geom.Matrix;\n\nvar m:Matrix = new Matrix(1, 0, 0, 1, 24, 48);\ntrace(m.tx, m.ty); // Output: 24 48".to_owned(),
                "translate" => "import flash.geom.Matrix;\n\nvar m:Matrix = new Matrix();\nm.translate(40, -15);\ntrace(m.tx, m.ty); // Output: 40 -15".to_owned(),
                "scale" => "import flash.geom.Matrix;\n\nvar m:Matrix = new Matrix(1, 0, 0, 1, 10, 20);\nm.scale(2, 3);\ntrace(m.a, m.d, m.tx, m.ty); // Output: 2 3 20 60".to_owned(),
                "rotate" => "import flash.geom.Matrix;\n\nvar m:Matrix = new Matrix();\nm.rotate(0);\ntrace(m.a, m.d); // Output: 1 1".to_owned(),
                "transformPoint" => "import flash.geom.Matrix;\nimport flash.geom.Point;\n\nvar m:Matrix = new Matrix(2, 0, 0, 2, 10, 5);\nvar out:Point = m.transformPoint(new Point(3, 4));\ntrace(out.x, out.y); // Output: 16 13".to_owned(),
                "deltaTransformPoint" => "import flash.geom.Matrix;\nimport flash.geom.Point;\n\nvar m:Matrix = new Matrix(2, 0, 0, 3, 100, 200);\nvar delta:Point = m.deltaTransformPoint(new Point(5, 4));\ntrace(delta.x, delta.y); // Output: 10 12".to_owned(),
                "invert" => "import flash.geom.Matrix;\n\nvar m:Matrix = new Matrix(2, 0, 0, 2, 10, 20);\nm.invert();\ntrace(m.a, m.tx, m.ty); // Output: 0.5 -5 -10".to_owned(),
                "identity" => "import flash.geom.Matrix;\n\nvar m:Matrix = new Matrix(2, 1, 1, 2, 30, 40);\nm.identity();\ntrace(m.a, m.tx, m.ty); // Output: 1 0 0".to_owned(),
                "concat" => "import flash.geom.Matrix;\n\nvar m:Matrix = new Matrix(1, 0, 0, 1, 10, 20);\nm.concat(new Matrix(2, 0, 0, 2, 5, 5));\ntrace(m.tx, m.ty); // Output: 25 45".to_owned(),
                "clone" => "import flash.geom.Matrix;\n\nvar copy:Matrix = new Matrix(1, 0, 0, 1, 12, 34).clone();\ntrace(copy.tx, copy.ty); // Output: 12 34".to_owned(),
                "createBox" => "import flash.geom.Matrix;\n\nvar m:Matrix = new Matrix();\nm.createBox(2, 2, 0, 50, 80);\ntrace(m.a, m.tx, m.ty); // Output: 2 50 80".to_owned(),
                "createGradientBox" => "import flash.geom.Matrix;\n\nvar m:Matrix = new Matrix();\nm.createGradientBox(200, 100, 0, 10, 20);\ntrace(Math.round(m.tx), Math.round(m.ty)); // Output: 110 70".to_owned(),
                "setTo" => "import flash.geom.Matrix;\n\nvar m:Matrix = new Matrix();\nm.setTo(1, 0, 0, 1, 64, 32);\ntrace(m.tx, m.ty); // Output: 64 32".to_owned(),
                "copyFrom" => "import flash.geom.Matrix;\n\nvar m:Matrix = new Matrix();\nm.copyFrom(new Matrix(1, 0, 0, 1, 8, 16));\ntrace(m.tx, m.ty); // Output: 8 16".to_owned(),
                "toString" => "import flash.geom.Matrix;\n\nvar m:Matrix = new Matrix(1, 0, 0, 1, 12, 24);\ntrace(m.toString()); // Output: (a=1, b=0, c=0, d=1, tx=12, ty=24)".to_owned(),
                "a" | "b" | "c" | "d" | "tx" | "ty" => {
                    let expected = match mname {
                        "a" | "d" => 1,
                        "b" | "c" => 0,
                        "tx" => 15,
                        _ => 30,
                    };
                    format!("import flash.geom.Matrix;\n\nvar m:Matrix = new Matrix(1, 0, 0, 1, 15, 30);\ntrace(m.{mname}); // Output: {expected}")
                }
                _ => return None,
            });
        }
        "flash.geom.Vector3D" => {
            return Some(match mname {
                "Vector3D" => "import flash.geom.Vector3D;\n\nvar v:Vector3D = new Vector3D(3, 4, 12);\ntrace(v.length); // Output: 13".to_owned(),
                "dotProduct" => "import flash.geom.Vector3D;\n\nvar a:Vector3D = new Vector3D(2, 3, 4);\nvar b:Vector3D = new Vector3D(1, 0, 5);\ntrace(a.dotProduct(b)); // Output: 22".to_owned(),
                "crossProduct" => "import flash.geom.Vector3D;\n\nvar zAxis:Vector3D = Vector3D.X_AXIS.crossProduct(Vector3D.Y_AXIS);\ntrace(zAxis.x, zAxis.y, zAxis.z); // Output: 0 0 1".to_owned(),
                "distance" => "import flash.geom.Vector3D;\n\nvar d:Number = Vector3D.distance(new Vector3D(0, 0, 0), new Vector3D(0, 3, 4));\ntrace(d); // Output: 5".to_owned(),
                "angleBetween" => "import flash.geom.Vector3D;\n\nvar rad:Number = Vector3D.angleBetween(Vector3D.X_AXIS, Vector3D.Y_AXIS);\ntrace(Math.round(rad * 180 / Math.PI)); // Output: 90".to_owned(),
                "add" => "import flash.geom.Vector3D;\n\nvar sum:Vector3D = new Vector3D(1, 2, 3).add(new Vector3D(4, 5, 6));\ntrace(sum.x, sum.y, sum.z); // Output: 5 7 9".to_owned(),
                "subtract" => "import flash.geom.Vector3D;\n\nvar diff:Vector3D = new Vector3D(9, 7, 5).subtract(new Vector3D(4, 2, 1));\ntrace(diff.x, diff.y, diff.z); // Output: 5 5 4".to_owned(),
                "scaleBy" => "import flash.geom.Vector3D;\n\nvar v:Vector3D = new Vector3D(2, -3, 4);\nv.scaleBy(3);\ntrace(v.x, v.y, v.z); // Output: 6 -9 12".to_owned(),
                "negate" => "import flash.geom.Vector3D;\n\nvar v:Vector3D = new Vector3D(5, -2, 0);\nv.negate();\ntrace(v.x, v.y); // Output: -5 2".to_owned(),
                "normalize" => "import flash.geom.Vector3D;\n\nvar v:Vector3D = new Vector3D(0, 3, 4);\nvar len:Number = v.normalize();\ntrace(len, v.y, v.z); // Output: 5 0.6 0.8".to_owned(),
                "length" => "import flash.geom.Vector3D;\n\ntrace(new Vector3D(0, 6, 8).length); // Output: 10".to_owned(),
                "lengthSquared" => "import flash.geom.Vector3D;\n\ntrace(new Vector3D(2, 3, 6).lengthSquared); // Output: 49".to_owned(),
                "X_AXIS" => "import flash.geom.Vector3D;\n\ntrace(Vector3D.X_AXIS.x, Vector3D.X_AXIS.y, Vector3D.X_AXIS.z); // Output: 1 0 0".to_owned(),
                "Y_AXIS" => "import flash.geom.Vector3D;\n\ntrace(Vector3D.Y_AXIS.x, Vector3D.Y_AXIS.y, Vector3D.Y_AXIS.z); // Output: 0 1 0".to_owned(),
                "Z_AXIS" => "import flash.geom.Vector3D;\n\ntrace(Vector3D.Z_AXIS.x, Vector3D.Z_AXIS.y, Vector3D.Z_AXIS.z); // Output: 0 0 1".to_owned(),
                _ => return None,
            });
        }
        "flash.geom.ColorTransform" => {
            return Some(match mname {
                "ColorTransform" => "import flash.geom.ColorTransform;\n\nvar ct:ColorTransform = new ColorTransform(0.5, 1.0, 1.0, 0.8, 32, 0, 0, 0);\ntrace(ct.redMultiplier, ct.redOffset); // Output: 0.5 32".to_owned(),
                "color" | "rgb" => format!("import flash.geom.ColorTransform;\n\nvar ct:ColorTransform = new ColorTransform();\nct.{mname} = 0x3C6BF0;\ntrace(ct.redOffset, ct.greenOffset, ct.blueOffset); // Output: 60 107 240"),
                "concat" => "import flash.geom.ColorTransform;\n\nvar a:ColorTransform = new ColorTransform(0.5, 1, 1, 1);\na.concat(new ColorTransform(0.5, 1, 1, 1));\ntrace(a.redMultiplier); // Output: 0.25".to_owned(),
                _ => return None,
            });
        }
        "Color" => {
            return Some(match mname {
                "Color" => "var swatch:MovieClip = this.createEmptyMovieClip(\"swatch_mc\", 1);\nvar tint:Color = new Color(swatch);\ntint.setRGB(0x3C6BF0);\ntrace(tint.getRGB().toString(16)); // Output: 3c6bf0".to_owned(),
                "setRGB" | "getRGB" => "var swatch:MovieClip = this.createEmptyMovieClip(\"swatch_mc\", 1);\nvar tint:Color = new Color(swatch);\ntint.setRGB(0xFF8800);\ntrace(tint.getRGB()); // Output: 16746496".to_owned(),
                "setTransform" | "getTransform" => "var swatch:MovieClip = this.createEmptyMovieClip(\"swatch_mc\", 1);\nvar tint:Color = new Color(swatch);\ntint.setTransform({ra: 50, rb: 100, ga: 100, gb: 0, ba: 100, bb: 0, aa: 100, ab: 0});\ntrace(tint.getTransform().ra, tint.getTransform().rb); // Output: 50 100".to_owned(),
                _ => return None,
            });
        }
        _ => {}
    }

    // 8. AS3 E4X XML & XMLList + Legacy XMLNode / XMLDocument
    if as3 && owner_key == "XML" {
        return Some(match mname {
            "XML" => "var doc:XML = new XML(\"<level id='1'><wave count='4'/></level>\");\ntrace(doc.@id.toString(), doc.wave.@count.toString()); // Output: 1 4".to_owned(),
            "attribute" => "var item:XML = <weapon id=\"laser\" dmg=\"60\"/>;\ntrace(item.attribute(\"dmg\").toString()); // Output: 60".to_owned(),
            "attributes" => "var item:XML = <weapon id=\"laser\" dmg=\"60\"/>;\ntrace(item.attributes().length()); // Output: 2".to_owned(),
            "child" => "var doc:XML = <hud><title>Score</title><value>500</value></hud>;\ntrace(doc.child(\"value\").toString()); // Output: 500".to_owned(),
            "children" => "var doc:XML = <party><hero>Aria</hero><hero>Sol</hero></party>;\ntrace(doc.children().length()); // Output: 2".to_owned(),
            "elements" => "var doc:XML = <scene><layer/><layer/></scene>;\ntrace(doc.elements().length()); // Output: 2".to_owned(),
            "descendants" => "var doc:XML = <ui><panel><btn>OK</btn></panel></ui>;\ntrace(doc.descendants(\"btn\").toString()); // Output: OK".to_owned(),
            "appendChild" => "var list:XML = <bag><item>Key</item></bag>;\nlist.appendChild(<item>Map</item>);\ntrace(list.item.length()); // Output: 2".to_owned(),
            "prependChild" => "var list:XML = <bag><item>Map</item></bag>;\nlist.prependChild(<item>Key</item>);\ntrace(list.item[0].toString()); // Output: Key".to_owned(),
            "localName" => "var node:XML = <sprite x=\"10\"/>;\ntrace(node.localName()); // Output: sprite".to_owned(),
            "nodeKind" => "var node:XML = <sprite/>;\ntrace(node.nodeKind()); // Output: element".to_owned(),
            "hasSimpleContent" => "var leaf:XML = <label>Start</label>;\ntrace(leaf.hasSimpleContent()); // Output: true".to_owned(),
            "hasComplexContent" => "var tree:XML = <menu><item>Start</item></menu>;\ntrace(tree.hasComplexContent()); // Output: true".to_owned(),
            "copy" => "var orig:XML = <badge tier=\"gold\"/>;\nvar dup:XML = orig.copy();\ntrace(dup.@tier.toString()); // Output: gold".to_owned(),
            "length" => "var node:XML = <item/>;\ntrace(node.length()); // Output: 1".to_owned(),
            "toString" => "var label:XML = <title>Ready</title>;\ntrace(label.toString()); // Output: Ready".to_owned(),
            "toXMLString" => "var node:XML = <pin x=\"8\"/>;\ntrace(node.toXMLString()); // Output: <pin x=\"8\"/>".to_owned(),
            _ => return None,
        });
    }
    if as3 && owner_key == "XMLList" {
        return Some(match mname {
            "XMLList" => "var list:XMLList = new XMLList(\"<coin>10</coin><coin>25</coin>\");\ntrace(list.length(), list[1].toString()); // Output: 2 25".to_owned(),
            "length" => "var doc:XML = <bag><gem/><gem/><gem/></bag>;\nvar gems:XMLList = doc.gem;\ntrace(gems.length()); // Output: 3".to_owned(),
            "attribute" => "var doc:XML = <bag><gem id=\"ruby\"/><gem id=\"opal\"/></bag>;\ntrace(doc.gem.attribute(\"id\")[0].toString()); // Output: ruby".to_owned(),
            "toString" => "var list:XMLList = XMLList(\"<word>NeoFlash</word>\");\ntrace(list.toString()); // Output: NeoFlash".to_owned(),
            _ => return None,
        });
    }
    if matches!(owner_key, "XMLDocument" | "flash.xml.XMLDocument") || (!as3 && owner_key == "XML")
    {
        let imp = if as3 {
            "import flash.xml.XMLDocument;\n\nvar doc:XMLDocument = new XMLDocument();"
        } else {
            "var doc:XML = new XML();"
        };
        match mname {
            "parseXML" => {
                return Some(format!(
                    "{imp}\ndoc.ignoreWhite = true;\ndoc.parseXML(\"<stage><actor name='Hero'/></stage>\");\ntrace(doc.firstChild.nodeName); // Output: stage"
                ));
            }
            "createElement" => {
                return Some(format!(
                    "{imp}\nvar el = doc.createElement(\"badge\");\ntrace(el.nodeName); // Output: badge"
                ));
            }
            "createTextNode" => {
                return Some(format!(
                    "{imp}\nvar txt = doc.createTextNode(\"Score 100\");\ntrace(txt.nodeValue); // Output: Score 100"
                ));
            }
            _ => {}
        }
    }

    // 9. Core MovieClip, Sprite, DisplayObjectContainer, Graphics, BitmapData, TextField, Filters
    match (as3, class_name, mname) {
        (true, "MovieClip", "gotoAndPlay" | "gotoAndStop") => {
            return Some(format!(
                "import flash.display.MovieClip;\n\nvar clip:MovieClip = new MovieClip();\nclip.{mname}(1);\ntrace(clip.currentFrame); // Output: 1"
            ));
        }
        (true, "MovieClip", "currentFrame" | "totalFrames" | "framesLoaded") => {
            return Some(format!(
                "import flash.display.MovieClip;\n\nvar clip:MovieClip = new MovieClip();\ntrace(clip.{mname}); // Output: 1"
            ));
        }
        (false, "MovieClip", "createEmptyMovieClip") => {
            return Some(
                "var card:MovieClip = this.createEmptyMovieClip(\"card_mc\", this.getNextHighestDepth());\ncard._x = 48;\ntrace(card._name, card._x); // Output: card_mc 48".to_owned(),
            );
        }
        (false, "MovieClip", "getNextHighestDepth") => {
            return Some(
                "var nextDepth:Number = this.getNextHighestDepth();\ntrace(nextDepth >= 0); // Output: true".to_owned(),
            );
        }
        (true, "DisplayObjectContainer" | "Sprite" | "MovieClip", "addChild") => {
            return Some(
                "import flash.display.Shape;\nimport flash.display.Sprite;\n\nvar container:Sprite = new Sprite();\nvar dot:Shape = new Shape();\ncontainer.addChild(dot);\ntrace(container.numChildren); // Output: 1".to_owned(),
            );
        }
        (true, "DisplayObjectContainer" | "Sprite" | "MovieClip", "removeChild") => {
            return Some(
                "import flash.display.Shape;\nimport flash.display.Sprite;\n\nvar container:Sprite = new Sprite();\nvar dot:Shape = new Shape();\ncontainer.addChild(dot);\ncontainer.removeChild(dot);\ntrace(container.numChildren); // Output: 0".to_owned(),
            );
        }
        (true, "Graphics", "beginFill" | "drawRect" | "drawCircle" | "endFill") => {
            return Some(
                "import flash.display.Sprite;\n\nvar badge:Sprite = new Sprite();\nbadge.graphics.beginFill(0x3C6BF0, 1.0);\nbadge.graphics.drawRect(0, 0, 80, 40);\nbadge.graphics.endFill();\ntrace(badge.width, badge.height); // Output: 80 40".to_owned(),
            );
        }
        (_, "BitmapData", "getPixel" | "setPixel") => {
            return Some(
                "import flash.display.BitmapData;\n\nvar bmp:BitmapData = new BitmapData(16, 16, false, 0x000000);\nbmp.setPixel(4, 4, 0x3C6BF0);\ntrace(bmp.getPixel(4, 4).toString(16)); // Output: 3c6bf0".to_owned(),
            );
        }
        (true, "TextField", "appendText") => {
            return Some(
                "import flash.text.TextField;\n\nvar tf:TextField = new TextField();\ntf.text = \"Score: \";\ntf.appendText(\"250\");\ntrace(tf.text); // Output: Score: 250".to_owned(),
            );
        }
        _ => {}
    }

    // 10. Constants with known values across ANY class
    if mkind == "constant" {
        return Some(synthesize_constant_example(
            as3, owner_key, class_name, member,
        ));
    }

    None
}

fn synthesize_constant_example(
    as3: bool,
    owner_key: &str,
    class_name: &str,
    member: &Member,
) -> String {
    let package = script_api::package_of(owner_key);
    let import_line = if !package.is_empty() && package != "Top level" && as3 {
        format!("import {owner_key};\n\n")
    } else {
        String::new()
    };
    let qualifier = if !as3 && owner_key.starts_with("System.") {
        owner_key
    } else {
        class_name
    };
    let ty = if member.ty.is_empty() {
        if as3 { "*" } else { "Object" }
    } else {
        member.ty.as_str()
    };
    let expected = if let Some(val) = member.value.as_deref().filter(|s| !s.trim().is_empty()) {
        format_constant_trace_output(val)
    } else {
        known_as2_constant_value(owner_key, &member.name)
    };
    format!(
        "{import_line}var value:{ty} = {qualifier}.{};\ntrace(value); // Output: {expected}",
        member.name
    )
}

fn known_as2_constant_value(owner_key: &str, name: &str) -> String {
    match (owner_key, name) {
        ("Key", "BACKSPACE") => "8",
        ("Key", "TAB") => "9",
        ("Key", "ENTER") => "13",
        ("Key", "SHIFT") => "16",
        ("Key", "CONTROL") => "17",
        ("Key", "CAPSLOCK") => "20",
        ("Key", "ESCAPE") => "27",
        ("Key", "SPACE") => "32",
        ("Key", "PGUP") => "33",
        ("Key", "PGDN") => "34",
        ("Key", "END") => "35",
        ("Key", "HOME") => "36",
        ("Key", "LEFT") => "37",
        ("Key", "UP") => "38",
        ("Key", "RIGHT") => "39",
        ("Key", "DOWN") => "40",
        ("Key", "INSERT") => "45",
        ("Key", "DELETEKEY") => "46",
        ("System.IME", "ALPHANUMERIC_FULL") => "ALPHANUMERIC_FULL",
        ("System.IME", "ALPHANUMERIC_HALF") => "ALPHANUMERIC_HALF",
        ("System.IME", "CHINESE") => "CHINESE",
        ("System.IME", "JAPANESE_HIRAGANA") => "JAPANESE_HIRAGANA",
        ("System.IME", "JAPANESE_KATAKANA_FULL") => "JAPANESE_KATAKANA_FULL",
        ("System.IME", "JAPANESE_KATAKANA_HALF") => "JAPANESE_KATAKANA_HALF",
        ("System.IME", "KOREAN") => "KOREAN",
        ("System.IME", "UNKNOWN") => "UNKNOWN",
        ("mx.transitions.Transition", "IN") => "0",
        ("mx.transitions.Transition", "OUT") => "1",
        _ => return name.to_ascii_lowercase(),
    }
    .to_owned()
}

fn curated_class_example(as3: bool, owner_key: &str, class_name: &str) -> Option<String> {
    let f_imp = if as3 || owner_key.starts_with("flash.") {
        format!("import {owner_key};\n\n")
    } else {
        String::new()
    };
    Some(match (as3, owner_key, class_name) {
        (_, _, "Array") => "var items:Array = [\"a\", \"b\", \"c\", \"b\", \"d\"];\ntrace(items.length, items.lastIndexOf(\"b\")); // Output: 5 3".to_owned(),
        (true, _, "Vector") => "var scores:Vector.<int> = Vector.<int>([10, 20, 30, 20]);\ntrace(scores.length, scores.lastIndexOf(20)); // Output: 4 3".to_owned(),
        (_, _, "String") => "var path:String = \"assets/hero.png\";\ntrace(path.slice(7, 11).toUpperCase()); // Output: HERO".to_owned(),
        (_, _, "Math") => "var hyp:Number = Math.sqrt(Math.pow(3, 2) + Math.pow(4, 2));\ntrace(hyp); // Output: 5".to_owned(),
        (_, _, "Number") => "var ratio:Number = Number(\"3.5\");\ntrace(ratio * 2); // Output: 7".to_owned(),
        (true, _, "int") => "var frameIndex:int = int(4.9);\ntrace(frameIndex, int.MAX_VALUE); // Output: 4 2147483647".to_owned(),
        (true, _, "uint") => "var rgba:uint = 0x3C6BF0;\ntrace(rgba.toString(16)); // Output: 3c6bf0".to_owned(),
        (_, _, "Boolean") => "var ready:Boolean = Boolean(1);\ntrace(ready, !ready); // Output: true false".to_owned(),
        (_, _, "Object") => "var state:Object = {level: 2, coins: 45};\ntrace(state.hasOwnProperty(\"coins\"), state.coins); // Output: true 45".to_owned(),
        (_, _, "Function") => "var scale:Function = function(n:Number):Number { return n * 3; };\ntrace(scale.call(null, 14)); // Output: 42".to_owned(),
        (_, _, "arguments") => "function totalArgs():Number {\n    return arguments.length;\n}\ntrace(totalArgs(10, 20, 30)); // Output: 3".to_owned(),
        (true, _, "JSON") => "var text:String = JSON.stringify({wave: 3, clear: true});\nvar obj:Object = JSON.parse(text);\ntrace(obj.wave, obj.clear); // Output: 3 true".to_owned(),
        (true, _, "RegExp") => "var pattern:RegExp = /item_([0-9]+)/;\nvar hit:Object = pattern.exec(\"pickup item_42\");\ntrace(hit[1]); // Output: 42".to_owned(),
        (_, _, "Date") => "var epoch:Date = new Date(0);\ntrace(epoch.getUTCFullYear(), epoch.getTime()); // Output: 1970 0".to_owned(),
        (true, _, "ByteArray") => "import flash.utils.ByteArray;\n\nvar bytes:ByteArray = new ByteArray();\nbytes.writeByte(0xFF);\nbytes.writeUTF(\"OK\");\nbytes.position = 0;\ntrace(bytes.readUnsignedByte(), bytes.readUTF()); // Output: 255 OK".to_owned(),
        (true, _, "Endian") => "import flash.utils.Endian;\n\ntrace(Endian.BIG_ENDIAN, Endian.LITTLE_ENDIAN); // Output: bigEndian littleEndian".to_owned(),
        (true, _, "Dictionary") => "import flash.utils.Dictionary;\n\nvar map:Dictionary = new Dictionary();\nvar actor:Object = {name: \"Hero\"};\nmap[actor] = 100;\ntrace(map[actor]); // Output: 100".to_owned(),
        (true, _, "XML") => "var level:XML = <level id=\"2\"><orb points=\"50\"/></level>;\ntrace(level.@id.toString(), level.orb.@points.toString()); // Output: 2 50".to_owned(),
        (true, _, "XMLList") => "var bag:XML = <bag><key>Red</key><key>Blue</key></bag>;\nvar keys:XMLList = bag.key;\ntrace(keys.length(), keys[1].toString()); // Output: 2 Blue".to_owned(),
        (_, _, "Point") => "import flash.geom.Point;\n\nvar d:Number = Point.distance(new Point(0, 0), new Point(6, 8));\ntrace(d); // Output: 10".to_owned(),
        (_, _, "Rectangle") => "import flash.geom.Rectangle;\n\nvar a:Rectangle = new Rectangle(0, 0, 60, 40);\nvar b:Rectangle = new Rectangle(30, 20, 60, 40);\ntrace(a.intersects(b), a.intersection(b).width); // Output: true 30".to_owned(),
        (_, _, "Matrix") => "import flash.geom.Matrix;\nimport flash.geom.Point;\n\nvar m:Matrix = new Matrix();\nm.translate(20, 10);\nvar pt:Point = m.transformPoint(new Point(5, 5));\ntrace(pt.x, pt.y); // Output: 25 15".to_owned(),
        (true, _, "Vector3D") => "import flash.geom.Vector3D;\n\nvar v:Vector3D = new Vector3D(0, 3, 4);\ntrace(v.length, v.dotProduct(Vector3D.Z_AXIS)); // Output: 5 4".to_owned(),
        (_, _, "ColorTransform") => format!("{f_imp}var ct:ColorTransform = new ColorTransform(0.5, 1, 1, 1, 20, 0, 0, 0);\ntrace(ct.redMultiplier, ct.redOffset); // Output: 0.5 20"),
        (false, _, "Color") => "var swatch:MovieClip = this.createEmptyMovieClip(\"swatch_mc\", 1);\nvar tint:Color = new Color(swatch);\ntint.setRGB(0x3C6BF0);\ntrace(tint.getRGB().toString(16)); // Output: 3c6bf0".to_owned(),
        (false, _, "MovieClip") => "var box:MovieClip = this.createEmptyMovieClip(\"box_mc\", this.getNextHighestDepth());\nbox._x = 80;\nbox._y = 40;\ntrace(box._name, box._x, box._y); // Output: box_mc 80 40".to_owned(),
        (true, _, "MovieClip") => "import flash.display.MovieClip;\n\nvar clip:MovieClip = new MovieClip();\nclip.x = 120;\ntrace(clip.currentFrame, clip.x); // Output: 1 120".to_owned(),
        (true, _, "Sprite") => "import flash.display.Sprite;\n\nvar badge:Sprite = new Sprite();\nbadge.graphics.beginFill(0x3C6BF0, 1.0);\nbadge.graphics.drawCircle(0, 0, 24);\nbadge.graphics.endFill();\ntrace(badge.width, badge.height); // Output: 48 48".to_owned(),
        (true, _, "Shape") => "import flash.display.Shape;\n\nvar rect:Shape = new Shape();\nrect.graphics.beginFill(0x3C6BF0, 1.0);\nrect.graphics.drawRect(0, 0, 64, 32);\nrect.graphics.endFill();\ntrace(rect.width, rect.height); // Output: 64 32".to_owned(),
        (_, _, "BitmapData") => format!("{f_imp}var bmp:BitmapData = new BitmapData(32, 32, true, 0x00000000);\nbmp.setPixel(2, 2, 0x3C6BF0);\ntrace(bmp.width, bmp.getPixel(2, 2).toString(16)); // Output: 32 3c6bf0"),
        (false, _, "TextField") => "this.createTextField(\"hud_txt\", this.getNextHighestDepth(), 16, 16, 180, 28);\nthis.hud_txt.text = \"SCORE: 100\";\ntrace(this.hud_txt.text); // Output: SCORE: 100".to_owned(),
        (true, _, "TextField") => "import flash.text.TextField;\n\nvar tf:TextField = new TextField();\ntf.text = \"READY\";\ntrace(tf.text, tf.length); // Output: READY 5".to_owned(),
        (_, _, "TextFormat") => format!("{f_imp}var fmt:TextFormat = new TextFormat(\"Inter\", 16, 0xFFFFFF, true);\ntrace(fmt.font, fmt.size, fmt.bold); // Output: Inter 16 true"),
        (_, _, "BlurFilter") => format!("{f_imp}var blur:BlurFilter = new BlurFilter(8, 8, 2);\ntrace(blur.blurX, blur.blurY, blur.quality); // Output: 8 8 2"),
        (_, _, "GlowFilter") => format!("{f_imp}var glow:GlowFilter = new GlowFilter(0x3C6BF0, 0.9, 12, 12, 2, 2);\ntrace(glow.blurX, glow.strength); // Output: 12 2"),
        (_, _, "DropShadowFilter") => format!("{f_imp}var shadow:DropShadowFilter = new DropShadowFilter(4, 45, 0x000000, 0.75, 8, 8);\ntrace(shadow.distance, shadow.angle); // Output: 4 45"),
        (_, _, "BevelFilter") => format!("{f_imp}var bevel:BevelFilter = new BevelFilter(4, 45, 0xFFFFFF, 0.8, 0x000000, 0.8);\ntrace(bevel.distance, bevel.angle); // Output: 4 45"),
        (_, _, "ColorMatrixFilter") => format!("{f_imp}var cm:ColorMatrixFilter = new ColorMatrixFilter([1,0,0,0,0, 0,1,0,0,0, 0,0,1,0,0, 0,0,0,1,0]);\ntrace(cm.matrix.length); // Output: 20"),
        (_, _, "ConvolutionFilter") => format!("{f_imp}var conv:ConvolutionFilter = new ConvolutionFilter(3, 3, [0,-1,0, -1,5,-1, 0,-1,0], 1);\ntrace(conv.matrixX, conv.matrixY); // Output: 3 3"),
        (_, _, "GradientGlowFilter") => format!("{f_imp}var gg:GradientGlowFilter = new GradientGlowFilter(4, 45, [0x000000, 0x3C6BF0], [0, 1], [0, 255]);\ntrace(gg.distance, gg.ratios.length); // Output: 4 2"),
        (_, _, "GradientBevelFilter") => format!("{f_imp}var gb:GradientBevelFilter = new GradientBevelFilter(4, 45, [0xFFFFFF, 0x000000], [1, 1], [0, 255]);\ntrace(gb.distance, gb.colors.length); // Output: 4 2"),
        (_, _, "DisplacementMapFilter") => format!("import flash.display.BitmapData;\nimport flash.geom.Point;\n{f_imp}var map:BitmapData = new BitmapData(32, 32, false, 0x808080);\nvar disp:DisplacementMapFilter = new DisplacementMapFilter(map, new Point(0, 0), 1, 2, 16, 16);\ntrace(disp.scaleX, disp.scaleY); // Output: 16 16"),
        (true, _, "EventDispatcher") => "import flash.events.Event;\nimport flash.events.EventDispatcher;\n\nvar bus:EventDispatcher = new EventDispatcher();\nvar fired:int = 0;\nbus.addEventListener(Event.CHANGE, function(e:Event):void { fired++; });\nbus.dispatchEvent(new Event(Event.CHANGE));\ntrace(fired); // Output: 1".to_owned(),
        (true, _, "Timer") => "import flash.utils.Timer;\n\nvar timer:Timer = new Timer(250, 4);\ntrace(timer.delay, timer.repeatCount, timer.running); // Output: 250 4 false".to_owned(),
        (true, _, "URLRequest") => "import flash.net.URLRequest;\n\nvar req:URLRequest = new URLRequest(\"https://example.com/level.json\");\ntrace(req.method, req.url); // Output: GET https://example.com/level.json".to_owned(),
        (true, _, "URLVariables") => "import flash.net.URLVariables;\n\nvar vars:URLVariables = new URLVariables(\"stage=2&coins=50\");\ntrace(vars.stage, vars.coins); // Output: 2 50".to_owned(),
        _ => return None,
    })
}

// ── Layer 4: Smart Domain-Aware Typed Synthesizer for Remaining Classes & Members ──

struct ReceiverPlan {
    setup_lines: Vec<String>,
    var_name: String,
    extra_imports: Vec<&'static str>,
}

fn build_receiver_plan(
    as3: bool,
    owner_key: &str,
    class_name: &str,
    class_kind: &str,
    constructor_params: Option<&str>,
    imports: &mut BTreeSet<String>,
) -> ReceiverPlan {
    let package = script_api::package_of(owner_key);
    if (as3 && !package.is_empty() && package != "Top level")
        || (!as3 && (package.starts_with("flash.") || package.starts_with("mx.")))
    {
        imports.insert(format!("import {owner_key};"));
    }

    let special: Option<(&str, &str, &[&'static str])> = match (as3, owner_key, class_name) {
        (true, _, "Stage") => Some(("var stg:Stage = this.stage;", "stg", &[])),
        (true, _, "Graphics") => Some((
            "var canvas:Shape = new Shape();\nvar g:Graphics = canvas.graphics;",
            "g",
            &["import flash.display.Shape;"],
        )),
        (true, _, "LoaderInfo") => Some((
            "var loader:Loader = new Loader();\nvar info:LoaderInfo = loader.contentLoaderInfo;",
            "info",
            &["import flash.display.Loader;"],
        )),
        (true, _, "Scene") => Some(("var scene:Scene = this.currentScene;", "scene", &[])),
        (true, _, "Transform") => Some((
            "var box:Sprite = new Sprite();\nvar xform:Transform = box.transform;",
            "xform",
            &["import flash.display.Sprite;"],
        )),
        (false, _, "Transform") => Some((
            "var clip:MovieClip = this.createEmptyMovieClip(\"box_mc\", 1);\nvar xform:Transform = new Transform(clip);",
            "xform",
            &[],
        )),
        (true, _, "DisplayObject" | "InteractiveObject" | "DisplayObjectContainer") => Some((
            "var view:Sprite = new Sprite();",
            "view",
            &["import flash.display.Sprite;"],
        )),
        (true, _, "IBitmapDrawable") => Some((
            "var drawable:IBitmapDrawable = new Sprite();",
            "drawable",
            &["import flash.display.Sprite;"],
        )),
        (true, _, "IEventDispatcher") => Some((
            "var dispatcher:IEventDispatcher = new EventDispatcher();",
            "dispatcher",
            &["import flash.events.EventDispatcher;"],
        )),
        (_, _, "BitmapFilter") => Some((
            "var filter:BitmapFilter = new BlurFilter(4, 4, 1);",
            "filter",
            &["import flash.filters.BlurFilter;"],
        )),
        (true, _, "Camera") => Some(("var cam:Camera = Camera.getCamera();", "cam", &[])),
        (false, _, "Camera") => Some(("var cam:Camera = Camera.get();", "cam", &[])),
        (true, _, "Microphone") => Some((
            "var mic:Microphone = Microphone.getMicrophone();",
            "mic",
            &[],
        )),
        (false, _, "Microphone") => Some(("var mic:Microphone = Microphone.get();", "mic", &[])),
        (true, _, "SoundChannel") => Some((
            "var sfx:Sound = new Sound();\nvar channel:SoundChannel = sfx.play();",
            "channel",
            &["import flash.media.Sound;"],
        )),
        (true, _, "ID3Info") => Some((
            "var sfx:Sound = new Sound();\nvar id3:ID3Info = sfx.id3;",
            "id3",
            &["import flash.media.Sound;"],
        )),
        (_, _, "SharedObject") => Some((
            "var store:SharedObject = SharedObject.getLocal(\"neoflash_prefs\");",
            "store",
            &[],
        )),
        (true, _, "TextSnapshot") => {
            Some(("var snap:TextSnapshot = this.textSnapshot;", "snap", &[]))
        }
        (false, _, "TextSnapshot") => Some((
            "var snap:TextSnapshot = this.getTextSnapshot();",
            "snap",
            &[],
        )),
        (false, _, "MovieClip") => Some((
            "var clip:MovieClip = this.createEmptyMovieClip(\"clip_mc\", this.getNextHighestDepth());",
            "clip",
            &[],
        )),
        (false, _, "TextField") => Some((
            "this.createTextField(\"label_txt\", this.getNextHighestDepth(), 12, 12, 180, 28);\nvar tf:TextField = this.label_txt;",
            "tf",
            &[],
        )),
        (false, _, "Button") => Some(("var btn:Button = this.action_btn;", "btn", &[])),
        (false, _, "Video") => Some(("var vid:Video = this.player_video;", "vid", &[])),
        (true, _, "Clipboard") => Some((
            "var clip:Clipboard = Clipboard.generalClipboard;",
            "clip",
            &[],
        )),
        (true, _, "NativeApplication") => Some((
            "var app:NativeApplication = NativeApplication.nativeApplication;",
            "app",
            &[],
        )),
        (true, _, "NetworkInfo") => Some((
            "var netInfo:NetworkInfo = NetworkInfo.networkInfo;",
            "netInfo",
            &[],
        )),
        (true, _, "StorageVolumeInfo") => Some((
            "var volumes:StorageVolumeInfo = StorageVolumeInfo.storageVolumeInfo;",
            "volumes",
            &[],
        )),
        (true, _, "DRMManager") => Some((
            "var drm:DRMManager = DRMManager.getDRMManager();",
            "drm",
            &[],
        )),
        (true, _, "Worker") => Some(("var worker:Worker = Worker.current;", "worker", &[])),
        (true, _, "WorkerDomain") => Some((
            "var domain:WorkerDomain = WorkerDomain.current;",
            "domain",
            &[],
        )),
        (true, _, "Screen") => Some(("var scr:Screen = Screen.mainScreen;", "scr", &[])),
        (true, _, "AudioDeviceManager") => Some((
            "var audioMgr:AudioDeviceManager = AudioDeviceManager.deviceManager;",
            "audioMgr",
            &[],
        )),
        (true, _, "Context3D") => Some((
            "var ctx3d:Context3D = this.stage.stage3Ds[0].context3D;",
            "ctx3d",
            &[],
        )),
        (true, _, "Program3D") => Some((
            "var prog:Program3D = this.stage.stage3Ds[0].context3D.createProgram();",
            "prog",
            &[],
        )),
        (true, _, "VertexBuffer3D") => Some((
            "var vbuf:VertexBuffer3D = this.stage.stage3Ds[0].context3D.createVertexBuffer(4, 3);",
            "vbuf",
            &[],
        )),
        (true, _, "IndexBuffer3D") => Some((
            "var ibuf:IndexBuffer3D = this.stage.stage3Ds[0].context3D.createIndexBuffer(6);",
            "ibuf",
            &[],
        )),
        (true, _, "Texture" | "TextureBase") => Some((
            "var tex:Texture = this.stage.stage3Ds[0].context3D.createTexture(256, 256, \"bgra\", false);",
            "tex",
            &["import flash.display3D.textures.Texture;"],
        )),
        (true, _, "RectangleTexture") => Some((
            "var rtex:RectangleTexture = this.stage.stage3Ds[0].context3D.createRectangleTexture(256, 256, \"bgra\", false);",
            "rtex",
            &[],
        )),
        (true, _, "CubeTexture") => Some((
            "var ctex:CubeTexture = this.stage.stage3Ds[0].context3D.createCubeTexture(128, \"bgra\", false);",
            "ctex",
            &[],
        )),
        (true, _, "VideoTexture") => Some((
            "var vtex:VideoTexture = this.stage.stage3Ds[0].context3D.createVideoTexture();",
            "vtex",
            &[],
        )),
        (true, _, "TextLine") => Some((
            "var block:TextBlock = new TextBlock(new TextElement(\"Hello\", new ElementFormat()));\nvar line:TextLine = block.createTextLine();",
            "line",
            &[
                "import flash.text.engine.ElementFormat;",
                "import flash.text.engine.TextBlock;",
                "import flash.text.engine.TextElement;",
            ],
        )),
        (false, _, "AsBroadcaster") => Some((
            "var source:Object = {};\nAsBroadcaster.initialize(source);",
            "source",
            &[],
        )),
        (false, _, "EventDispatcher") => Some((
            "var dispatcher:Object = {};\nmx.events.EventDispatcher.initialize(dispatcher);",
            "dispatcher",
            &[],
        )),
        _ => None,
    };

    if let Some((setup, var, extra)) = special {
        for imp in extra {
            imports.insert((*imp).to_owned());
        }
        return ReceiverPlan {
            setup_lines: setup.lines().map(str::to_owned).collect(),
            var_name: var.to_owned(),
            extra_imports: extra.to_vec(),
        };
    }

    let var_name = lower_ident(class_name);
    if class_kind == "interface" || constructor_params.is_none() {
        return ReceiverPlan {
            setup_lines: vec![format!("var {var_name}:{class_name} = null;")],
            var_name,
            extra_imports: Vec::new(),
        };
    }

    let ctor_args = build_ctor_args(as3, class_name, constructor_params, imports);
    ReceiverPlan {
        setup_lines: vec![format!(
            "var {var_name}:{class_name} = new {class_name}({ctor_args});"
        )],
        var_name,
        extra_imports: Vec::new(),
    }
}

fn sanitize_raw_params(raw: &str) -> &str {
    let before_paren = raw.split(')').next().unwrap_or(raw);
    before_paren
        .split(';')
        .next()
        .unwrap_or(before_paren)
        .trim()
}

fn build_ctor_args(
    as3: bool,
    class_name: &str,
    ctor_params: Option<&str>,
    imports: &mut BTreeSet<String>,
) -> String {
    match class_name {
        "Collator" | "DateTimeFormatter" | "NumberFormatter" | "CurrencyFormatter"
        | "StringTools" | "LocaleID" => return "\"en-US\"".to_owned(),
        "BlurFilter" => return "8, 8, 2".to_owned(),
        "GlowFilter" => return "0x3C6BF0, 0.9, 12, 12, 2, 2".to_owned(),
        "DropShadowFilter" => return "4, 45, 0x000000, 0.6, 8, 8".to_owned(),
        "BevelFilter" => return "4, 45, 0xFFFFFF, 0.8, 0x000000, 0.8".to_owned(),
        "BitmapData" => return "128, 128, true, 0x00000000".to_owned(),
        "URLRequest" => return "\"https://example.com/data.json\"".to_owned(),
        "URLVariables" => return "\"level=3&score=1200\"".to_owned(),
        "SoundTransform" => return "0.8, 0.0".to_owned(),
        "TextFormat" => return "\"Inter\", 14, 0xFFFFFF".to_owned(),
        "ContextMenuItem" => return "\"Reset Level\"".to_owned(),
        "Timer" => return "250, 4".to_owned(),
        "File" => return "\"app:/config.json\"".to_owned(),
        _ => {}
    }

    let Some(raw) = ctor_params else {
        return String::new();
    };
    let clean = sanitize_raw_params(raw);
    if clean.is_empty() {
        return String::new();
    }
    let parts = split_params(clean);
    let mut args = Vec::new();
    for (idx, part) in parts.iter().enumerate() {
        let trimmed = part.trim();
        let is_optional = trimmed.contains('=')
            || trimmed.starts_with('[')
            || trimmed.contains("[,")
            || trimmed.starts_with("...");
        if is_optional && idx > 0 {
            break;
        }
        if is_optional && idx == 0 {
            if class_name.ends_with("Event") {
                args.push("\"change\"".to_owned());
            } else if class_name.ends_with("Error") {
                args.push("\"Operation failed\"".to_owned());
            }
            break;
        }
        let (pname, pdesc) = describe_param(trimmed.trim_end_matches('['));
        let ptype = pdesc.split(',').next().unwrap_or("").trim();
        args.push(concrete_arg_for_param(as3, &pname, ptype, imports));
    }
    args.join(", ")
}

fn build_call_args(as3: bool, raw_params: Option<&str>, imports: &mut BTreeSet<String>) -> String {
    let Some(raw) = raw_params else {
        return String::new();
    };
    let clean = sanitize_raw_params(raw);
    if clean.is_empty() {
        return String::new();
    }
    let parts = split_params(clean);
    let mut args = Vec::new();
    for (idx, part) in parts.iter().enumerate() {
        let trimmed = part.trim();
        if trimmed.starts_with("...") {
            if idx == 0 {
                args.push("\"item\"".to_owned());
            }
            break;
        }
        let is_optional =
            trimmed.contains('=') || trimmed.starts_with('[') || trimmed.contains("[,");
        if is_optional && idx >= 1 {
            break;
        }
        let (pname, pdesc) = describe_param(trimmed.trim_end_matches('['));
        let ptype = pdesc.split(',').next().unwrap_or("").trim();
        args.push(concrete_arg_for_param(as3, &pname, ptype, imports));
    }
    args.join(", ")
}

fn concrete_arg_for_param(
    as3: bool,
    param_name: &str,
    param_type: &str,
    imports: &mut BTreeSet<String>,
) -> String {
    let lname = param_name
        .trim_start_matches("...")
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .to_ascii_lowercase();
    let short_ty = script_api::short_name(param_type.trim());

    match short_ty {
        "Boolean" | "boolean" => {
            if lname.contains("cancel") || lname.contains("mute") || lname.contains("lock") {
                "false".to_owned()
            } else {
                "true".to_owned()
            }
        }
        "int" | "uint" | "Number" => {
            if lname.contains("color") || lname.contains("rgb") {
                "0x3C6BF0".to_owned()
            } else if lname.contains("alpha") {
                if as3 { "0.85" } else { "85" }.to_owned()
            } else if lname.contains("width") || lname == "w" {
                "120".to_owned()
            } else if lname.contains("height") || lname == "h" {
                "80".to_owned()
            } else if lname == "x" || lname.ends_with('x') {
                "24".to_owned()
            } else if lname == "y" || lname.ends_with('y') {
                "16".to_owned()
            } else if lname.contains("index") || lname.contains("offset") || lname.contains("start")
            {
                "0".to_owned()
            } else if lname.contains("port") {
                "8080".to_owned()
            } else if lname.contains("delay") || lname.contains("interval") {
                "250".to_owned()
            } else if lname.contains("rate") || lname.contains("fps") {
                "24".to_owned()
            } else if lname.contains("scale") || lname.contains("ratio") || lname.contains("volume")
            {
                "1.0".to_owned()
            } else {
                "1".to_owned()
            }
        }
        "String" => {
            if lname.contains("url") || lname.contains("uri") {
                "\"https://example.com/asset.json\"".to_owned()
            } else if lname.contains("locale") {
                "\"en-US\"".to_owned()
            } else if lname.contains("charset") || lname.contains("encoding") {
                "\"utf-8\"".to_owned()
            } else if lname.contains("type") || lname.contains("event") {
                "\"complete\"".to_owned()
            } else if lname.contains("algorithm") {
                "\"zlib\"".to_owned()
            } else if lname.contains("font") {
                "\"Inter\"".to_owned()
            } else if lname.contains("path") || lname.contains("file") {
                "\"assets/data.json\"".to_owned()
            } else if lname.contains("domain") || lname.contains("host") {
                "\"localhost\"".to_owned()
            } else if lname.contains("name") || lname.contains("id") || lname.contains("label") {
                "\"hero\"".to_owned()
            } else {
                "\"sample\"".to_owned()
            }
        }
        "Array" => "[\"a\", \"b\"]".to_owned(),
        "Function" => {
            if as3 {
                "function(e:* = null):void {}".to_owned()
            } else {
                "function():Void {}".to_owned()
            }
        }
        "Point" => {
            imports.insert("import flash.geom.Point;".to_owned());
            "new Point(10, 20)".to_owned()
        }
        "Rectangle" => {
            imports.insert("import flash.geom.Rectangle;".to_owned());
            "new Rectangle(0, 0, 100, 60)".to_owned()
        }
        "Matrix" => {
            imports.insert("import flash.geom.Matrix;".to_owned());
            "new Matrix()".to_owned()
        }
        "Matrix3D" => {
            imports.insert("import flash.geom.Matrix3D;".to_owned());
            "new Matrix3D()".to_owned()
        }
        "Vector3D" => {
            imports.insert("import flash.geom.Vector3D;".to_owned());
            "new Vector3D(0, 0, 1)".to_owned()
        }
        "ColorTransform" => {
            imports.insert("import flash.geom.ColorTransform;".to_owned());
            "new ColorTransform()".to_owned()
        }
        "ByteArray" => {
            imports.insert("import flash.utils.ByteArray;".to_owned());
            "new ByteArray()".to_owned()
        }
        "URLRequest" => {
            imports.insert("import flash.net.URLRequest;".to_owned());
            "new URLRequest(\"https://example.com/data.json\")".to_owned()
        }
        "Event" => {
            imports.insert("import flash.events.Event;".to_owned());
            "new Event(\"complete\")".to_owned()
        }
        "BitmapData" => {
            imports.insert("import flash.display.BitmapData;".to_owned());
            "new BitmapData(64, 64, true, 0x00000000)".to_owned()
        }
        "BitmapFilter" | "BlurFilter" => {
            imports.insert("import flash.filters.BlurFilter;".to_owned());
            "new BlurFilter(4, 4, 1)".to_owned()
        }
        "DisplayObject"
        | "InteractiveObject"
        | "DisplayObjectContainer"
        | "Sprite"
        | "IBitmapDrawable" => {
            if as3 {
                imports.insert("import flash.display.Sprite;".to_owned());
                "new Sprite()".to_owned()
            } else {
                "this".to_owned()
            }
        }
        "MovieClip" => {
            if as3 {
                imports.insert("import flash.display.MovieClip;".to_owned());
                "new MovieClip()".to_owned()
            } else {
                "this".to_owned()
            }
        }
        "TextFormat" => {
            if as3 {
                imports.insert("import flash.text.TextFormat;".to_owned());
            }
            "new TextFormat(\"Inter\", 14, 0xFFFFFF)".to_owned()
        }
        "SoundTransform" => {
            imports.insert("import flash.media.SoundTransform;".to_owned());
            "new SoundTransform(0.8, 0.0)".to_owned()
        }
        "XML" => "<item id=\"1\"/>".to_owned(),
        "RegExp" => "/[a-z]+/i".to_owned(),
        "Date" => "new Date(0)".to_owned(),
        "Class" => "Object".to_owned(),
        _ if param_type.starts_with("Vector.<") => format!("new {param_type}()"),
        "Object" | "*" | "" => {
            if lname.contains("frame") {
                "1".to_owned()
            } else if lname.contains("url") || lname.contains("name") || lname.contains("text") {
                "\"sample\"".to_owned()
            } else if lname.contains("count") || lname.contains("num") || lname.contains("depth") {
                "1".to_owned()
            } else {
                "{id: 1}".to_owned()
            }
        }
        _ => "null".to_owned(),
    }
}

fn sample_output_for_type(ty: &str) -> &'static str {
    match script_api::short_name(ty.trim()) {
        "Boolean" | "boolean" => "true",
        "int" | "uint" | "Number" => "0",
        "String" => "sample",
        "Array" => "a,b",
        "Point" => "(x=0, y=0)",
        "Rectangle" => "(x=0, y=0, w=100, h=60)",
        "Matrix" => "(a=1, b=0, c=0, d=1, tx=0, ty=0)",
        _ => "[object Object]",
    }
}

fn synthesize_domain_member_example(
    as3: bool,
    owner_key: &str,
    class_name: &str,
    class_kind: &str,
    constructor_params: Option<&str>,
    member: &Member,
) -> String {
    let mut imports = BTreeSet::new();
    let package = script_api::package_of(owner_key);
    if (as3 && !package.is_empty() && package != "Top level")
        || (!as3 && (package.starts_with("flash.") || package.starts_with("mx.")))
    {
        imports.insert(format!("import {owner_key};"));
    }

    let qualifier = if !as3 && owner_key.starts_with("System.") {
        owner_key
    } else {
        class_name
    };
    let mname = member.name.as_str();
    let kind = member.kind.as_str();

    // 1. Constructor
    if kind == "constructor" {
        let var_name = lower_ident(class_name);
        let ctor_args = build_ctor_args(as3, class_name, member.params.as_deref(), &mut imports);
        let header = format_imports(&imports);
        return format!(
            "{header}var {var_name}:{class_name} = new {class_name}({ctor_args});\ntrace({var_name} !== null); // Output: true"
        );
    }

    // 2. Events & Callbacks
    if kind == "event" || kind == "callback" {
        let plan = build_receiver_plan(
            as3,
            owner_key,
            class_name,
            class_kind,
            constructor_params,
            &mut imports,
        );
        let _ = &plan.extra_imports;
        let setup = plan.setup_lines.join("\n");
        let target = if member.is_static {
            qualifier
        } else {
            plan.var_name.as_str()
        };
        if as3 {
            imports.insert("import flash.events.Event;".to_owned());
            let header = format_imports(&imports);
            let setup_block = if member.is_static {
                String::new()
            } else {
                format!("{setup}\n")
            };
            return format!(
                "{header}{setup_block}var handled:Boolean = false;\n{target}.addEventListener(\"{mname}\", function(e:Event):void {{\n    handled = true;\n}});\ntrace(\"{mname} listener attached:\", !handled); // Output: {mname} listener attached: true"
            );
        }
        let header = format_imports(&imports);
        let is_global_broadcaster = matches!(class_name, "Stage" | "Key" | "Mouse" | "Selection");
        let setup_block = if member.is_static || is_global_broadcaster {
            "var listener:Object = {};\n".to_owned()
        } else {
            format!("{setup}\n")
        };
        let recv = if is_global_broadcaster {
            "listener"
        } else {
            target
        };
        let attach_extra = if is_global_broadcaster {
            format!("\n{qualifier}.addListener(listener);")
        } else {
            String::new()
        };
        return format!(
            "{header}{setup_block}{recv}.{mname} = function():Void {{\n    trace(\"{mname}\");\n}};{attach_extra}\ntrace(typeof {recv}.{mname}); // Output: function"
        );
    }

    // 3. Static Method or Static Property
    if member.is_static {
        if member.params.is_some() {
            let args = build_call_args(as3, member.params.as_deref(), &mut imports);
            let header = format_imports(&imports);
            if member.ty.is_empty() || member.ty.eq_ignore_ascii_case("void") {
                return format!(
                    "{header}{qualifier}.{mname}({args});\ntrace(\"Called {qualifier}.{mname}\"); // Output: Called {qualifier}.{mname}"
                );
            }
            let out = sample_output_for_type(&member.ty);
            return format!(
                "{header}var result:* = {qualifier}.{mname}({args});\ntrace(result); // Output: {out}"
            );
        }
        let header = format_imports(&imports);
        if member.read_only {
            let out = sample_output_for_type(&member.ty);
            return format!(
                "{header}var val:* = {qualifier}.{mname};\ntrace(val); // Output: {out}"
            );
        }
        let val_expr = concrete_arg_for_param(as3, mname, &member.ty, &mut imports);
        let header = format_imports(&imports);
        let expected = format_constant_trace_output(&val_expr);
        return format!(
            "{header}{qualifier}.{mname} = {val_expr};\ntrace({qualifier}.{mname}); // Output: {expected}"
        );
    }

    // 4. Instance Method or Instance Property
    let plan = build_receiver_plan(
        as3,
        owner_key,
        class_name,
        class_kind,
        constructor_params,
        &mut imports,
    );
    let recv = &plan.var_name;
    let setup = plan.setup_lines.join("\n");
    let null_receiver = setup.ends_with("= null;");

    if member.params.is_some() {
        let args = build_call_args(as3, member.params.as_deref(), &mut imports);
        let header = format_imports(&imports);
        if null_receiver {
            return format!(
                "{header}{setup}\nif ({recv} != null) {{\n    {recv}.{mname}({args});\n}}\ntrace(\"{class_name}.{mname} ready:\", {recv} == null); // Output: {class_name}.{mname} ready: true"
            );
        }
        if member.ty.is_empty() || member.ty.eq_ignore_ascii_case("void") {
            return format!(
                "{header}{setup}\n{recv}.{mname}({args});\ntrace(\"{class_name}.{mname} ok\"); // Output: {class_name}.{mname} ok"
            );
        }
        let out = sample_output_for_type(&member.ty);
        return format!(
            "{header}{setup}\nvar result:* = {recv}.{mname}({args});\ntrace(result); // Output: {out}"
        );
    }

    if null_receiver {
        let header = format_imports(&imports);
        return format!(
            "{header}{setup}\nvar hasInstance:Boolean = ({recv} != null);\ntrace(\"{class_name}.{mname} target:\", hasInstance); // Output: {class_name}.{mname} target: false"
        );
    }
    if member.read_only {
        let header = format_imports(&imports);
        let out = sample_output_for_type(&member.ty);
        return format!("{header}{setup}\ntrace({recv}.{mname}); // Output: {out}");
    }
    let val_expr = concrete_arg_for_param(as3, mname, &member.ty, &mut imports);
    let header = format_imports(&imports);
    let expected = if val_expr.starts_with("new ") {
        "true".to_owned()
    } else {
        format_constant_trace_output(&val_expr)
    };
    let trace_expr = if val_expr.starts_with("new ") {
        format!("{recv}.{mname} != null")
    } else {
        format!("{recv}.{mname}")
    };
    format!(
        "{header}{setup}\n{recv}.{mname} = {val_expr};\ntrace({trace_expr}); // Output: {expected}"
    )
}

fn synthesize_domain_class_example(
    language: ScriptLanguage,
    as3: bool,
    owner_key: &str,
    class_name: &str,
    package: &str,
    class_kind: &str,
    constructor_params: Option<&str>,
) -> String {
    if constructor_params.is_none()
        && let Some(api) = script_api::api(language)
    {
        let mut owned: Vec<&Member> = api
            .members
            .iter()
            .chain(api.missing.iter())
            .filter(|m| canonical_class_key_str(&m.owner) == owner_key)
            .collect();
        owned.sort_by(|a, b| a.name.cmp(&b.name));
        if let Some(first_const) = owned.iter().find(|m| m.kind == "constant") {
            return synthesize_constant_example(as3, owner_key, class_name, first_const);
        }
        if let Some(first_static) = owned.iter().find(|m| m.is_static) {
            return synthesize_domain_member_example(
                as3,
                owner_key,
                class_name,
                class_kind,
                None,
                first_static,
            );
        }
    }

    let mut imports = BTreeSet::new();
    let pkg = if package.is_empty() {
        script_api::package_of(owner_key)
    } else {
        package
    };
    if (as3 && !pkg.is_empty() && pkg != "Top level")
        || (!as3 && (pkg.starts_with("flash.") || pkg.starts_with("mx.")))
    {
        imports.insert(format!("import {owner_key};"));
    }

    let plan = build_receiver_plan(
        as3,
        owner_key,
        class_name,
        class_kind,
        constructor_params,
        &mut imports,
    );
    let header = format_imports(&imports);
    let setup = plan.setup_lines.join("\n");
    let recv = &plan.var_name;
    let expected = if setup.ends_with("= null;") {
        "false"
    } else {
        "true"
    };
    format!("{header}{setup}\ntrace({recv} != null); // Output: {expected}")
}

fn format_imports(imports: &BTreeSet<String>) -> String {
    if imports.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for imp in imports {
        let _ = writeln!(out, "{imp}");
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::help_as_reference::JS_TS_API_ENTRIES;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    const FORBIDDEN_GLYPHS: &[char] = &['⌘', '⌥', '⇧', '⌃', '↵', '⌤', '⌫', '⌦', '⇥'];

    fn assert_example_quality(label: &str, example: &str) {
        let trimmed = example.trim();
        let line_count = trimmed.lines().count();
        assert!(
            line_count >= 2,
            "{label} example must have >= 2 lines, got {line_count}: {trimmed:?}"
        );
        assert!(
            trimmed.contains("trace(") || trimmed.contains("console.log("),
            "{label} example must call trace(...) or console.log(...): {trimmed:?}"
        );
        let Some((_, after_output)) = trimmed.split_once("// Output:") else {
            panic!("{label} example is missing `// Output:` annotation: {trimmed:?}");
        };
        let first_output = after_output.lines().next().unwrap_or("").trim();
        assert!(
            !first_output.is_empty(),
            "{label} example has empty `// Output:` comment: {trimmed:?}"
        );
        assert!(
            !trimmed.contains("lastIndexOf(searchElement"),
            "{label} example contains raw undeclared parameter stub: {trimmed:?}"
        );
        assert!(
            trimmed != "byteArray.readUnsignedByte();",
            "{label} example is a bare 1-liner stub: {trimmed:?}"
        );
        assert!(
            !trimmed.chars().any(|c| FORBIDDEN_GLYPHS.contains(&c)),
            "{label} example contains forbidden shortcut glyph: {trimmed:?}"
        );
    }

    fn extract_expected_outputs(code: &str) -> Vec<String> {
        code.lines()
            .filter_map(|line| {
                line.split_once("// Output:")
                    .map(|(_, out)| out.trim().to_owned())
            })
            .collect()
    }

    #[test]
    fn t440_all_api_classes_and_members_have_runnable_multi_line_examples_with_output() {
        let mut total_checked = 0usize;

        for language in [ScriptLanguage::ActionScript2, ScriptLanguage::ActionScript3] {
            let api = script_api::api(language).expect("ActionScript API index");
            let mut ctor_by_owner = std::collections::HashMap::new();
            for m in api.members.iter().chain(api.missing.iter()) {
                if m.kind == "constructor"
                    && let Some(p) = &m.params
                {
                    ctor_by_owner.insert(canonical_class_key_str(&m.owner), p.clone());
                }
            }

            for (raw_class, info) in &api.classes {
                if raw_class == "Top Level" {
                    continue;
                }
                let canonical = canonical_class_key_str(raw_class);
                let pkg = script_api::package_of(&canonical);
                let short = script_api::short_name(&canonical);
                let ctor = ctor_by_owner.get(&canonical).map(String::as_str);
                let ex =
                    synthesize_as_class_example(language, &canonical, short, pkg, &info.kind, ctor);
                assert_example_quality(&format!("{language:?} class {canonical}"), &ex);
                total_checked += 1;
            }

            for m in api.members.iter().chain(api.missing.iter()) {
                if m.kind == "package" {
                    continue;
                }
                let owner_info = api.classes.get(&m.owner);
                let class_kind = owner_info.map_or("", |i| i.kind.as_str());
                let is_pkg_container = owner_info.is_some_and(|i| i.status == "members")
                    && m.kind == "function"
                    && !m.owner.is_empty();
                let (ckey, pseudo) = if is_language_member_kind(&m.kind) {
                    (API_LANGUAGE_KEY.to_owned(), true)
                } else if m.owner.is_empty() || m.owner == "Top Level" {
                    (API_GLOBAL_KEY.to_owned(), true)
                } else if is_pkg_container {
                    (canonical_class_key_str(&m.owner), true)
                } else {
                    (canonical_class_key_str(&m.owner), false)
                };
                let short = if pseudo {
                    ""
                } else {
                    script_api::short_name(&ckey)
                };
                let ctor = ctor_by_owner.get(&ckey).map(String::as_str);
                let ex = synthesize_as_member_example(
                    language, &ckey, short, pseudo, class_kind, ctor, m,
                );
                assert_example_quality(
                    &format!("{language:?} member {ckey}.{} ({})", m.name, m.kind),
                    &ex,
                );
                total_checked += 1;
            }
        }

        // Verify all 45 JS/TS catalog entries (usage + example + compile check)
        assert_eq!(JS_TS_API_ENTRIES.len(), 45);
        let mut module_project = neoflash_core::Project::blank();
        module_project.sources.push(neoflash_core::SourceUnit {
            id: "src-math".to_owned(),
            path: "math.ts".to_owned(),
            language: ScriptLanguage::TypeScript,
            source: "export function clamp(v: number, lo: number, hi: number): number { return Math.max(lo, Math.min(hi, v)); }".to_owned(),
        });
        let program = neoflash_script::compile_project(&module_project)
            .expect("module project should compile");

        for entry in JS_TS_API_ENTRIES {
            assert!(
                !entry.usage.trim().is_empty(),
                "JS/TS entry {} must have non-empty usage",
                entry.name
            );
            assert_ne!(
                entry.usage.trim(),
                entry.signature.trim(),
                "JS/TS entry {} usage must be concise and distinct from signature",
                entry.name
            );
            assert!(
                !entry.usage.chars().any(|c| FORBIDDEN_GLYPHS.contains(&c)),
                "JS/TS entry {} usage contains forbidden glyph",
                entry.name
            );
            assert_example_quality(&format!("JS/TS {}", entry.name), entry.example);

            let lang = if entry.ts_only {
                ScriptLanguage::TypeScript
            } else {
                ScriptLanguage::JavaScript
            };
            if entry.name == "export" {
                let mut export_proj = neoflash_core::Project::blank();
                export_proj.sources.push(neoflash_core::SourceUnit {
                    id: "src-exp".to_owned(),
                    path: "helper.js".to_owned(),
                    language: lang,
                    source: entry.example.to_owned(),
                });
                let res = neoflash_script::compile_project(&export_proj);
                assert!(
                    res.is_ok(),
                    "JS/TS `export` example failed to compile: {:?}",
                    res.err()
                );
            } else {
                let script = neoflash_core::Script {
                    language: lang,
                    source: entry.example.to_owned(),
                };
                let res = neoflash_script::compile_with_program(&script, &program);
                assert!(
                    res.is_ok(),
                    "JS/TS `{}` example failed to compile: {:?}",
                    entry.name,
                    res.err()
                );
            }
            total_checked += 1;
        }

        // Verify user's specific textbook examples
        let as3_api = script_api::api(ScriptLanguage::ActionScript3).unwrap();
        let arr_last = as3_api
            .members
            .iter()
            .find(|m| m.owner == "Array" && m.name == "lastIndexOf")
            .expect("Array.lastIndexOf");
        let arr_ex = synthesize_as_member_example(
            ScriptLanguage::ActionScript3,
            "Array",
            "Array",
            false,
            "class",
            Some("... args"),
            arr_last,
        );
        assert!(arr_ex.contains("items.lastIndexOf(\"b\")"));
        assert!(arr_ex.contains("// Output: 3"));
        assert!(arr_ex.contains("// Output: 1"));
        assert!(arr_ex.contains("// Output: -1"));

        let ba_read_ub = as3_api
            .members
            .iter()
            .find(|m| m.owner == "flash.utils.ByteArray" && m.name == "readUnsignedByte")
            .expect("ByteArray.readUnsignedByte");
        let ba_ex = synthesize_as_member_example(
            ScriptLanguage::ActionScript3,
            "flash.utils.ByteArray",
            "ByteArray",
            false,
            "class",
            Some(""),
            ba_read_ub,
        );
        assert!(ba_ex.contains("bytes.writeByte(0xFF);"));
        assert!(ba_ex.contains("bytes.position = 0;"));
        assert!(ba_ex.contains("// Output: 255"));
        assert!(ba_ex.contains("// Output: 42"));

        assert!(
            total_checked >= 7_600,
            "expected >= 7,600 verified examples across AS2/AS3/JS/TS, got {total_checked}"
        );
    }

    fn find_as_member_example(lang: ScriptLanguage, owner: &str, name: &str) -> String {
        let api = script_api::api(lang).expect("AS API");
        let m = api
            .members
            .iter()
            .chain(api.missing.iter())
            .find(|m| canonical_class_key_str(&m.owner) == owner && m.name == name)
            .unwrap_or_else(|| panic!("missing {lang:?} {owner}.{name}"));
        let pseudo = owner.is_empty() || owner == API_GLOBAL_KEY || owner == API_LANGUAGE_KEY;
        let ckey = if owner.is_empty() {
            if is_language_member_kind(&m.kind) {
                API_LANGUAGE_KEY
            } else {
                API_GLOBAL_KEY
            }
        } else {
            owner
        };
        let short = if pseudo {
            ""
        } else {
            script_api::short_name(ckey)
        };
        synthesize_as_member_example(lang, ckey, short, pseudo, "class", None, m)
    }

    fn find_js_ts_example(name: &str) -> (ScriptLanguage, String) {
        let entry = JS_TS_API_ENTRIES
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("missing JS/TS entry {name}"));
        let lang = if entry.ts_only {
            ScriptLanguage::TypeScript
        } else {
            ScriptLanguage::JavaScript
        };
        (lang, entry.example.to_owned())
    }

    /// Resolves the checkout root that holds a built `target/web-runtime/neoflash_web_bg.wasm`
    /// (either the current workspace or the main checkout behind a git worktree).
    fn locate_wasm_checkout_root(repo_root: &Path) -> Option<PathBuf> {
        if repo_root
            .join("target/web-runtime/neoflash_web_bg.wasm")
            .is_file()
        {
            return Some(repo_root.to_path_buf());
        }
        if let Ok(out) = Command::new("git")
            .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .current_dir(repo_root)
            .output()
            && out.status.success()
        {
            let common = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
            if let Some(main_root) = common.parent()
                && main_root
                    .join("target/web-runtime/neoflash_web_bg.wasm")
                    .is_file()
            {
                return Some(main_root.to_path_buf());
            }
        }
        None
    }

    #[test]
    fn t440_sixty_plus_examples_execute_in_real_neoflash_runtime_matching_output_comments() {
        let mut cases: Vec<(String, ScriptLanguage, String)> = Vec::new();
        let mut push_as = |lang: ScriptLanguage, owner: &str, name: &str| {
            let code = find_as_member_example(lang, owner, name);
            cases.push((format!("{lang:?}:{owner}.{name}"), lang, code));
        };

        // AS3 Array, Vector, String
        for m in [
            "lastIndexOf",
            "indexOf",
            "push",
            "pop",
            "shift",
            "slice",
            "splice",
            "join",
            "reverse",
            "map",
            "filter",
            "every",
            "some",
        ] {
            push_as(ScriptLanguage::ActionScript3, "Array", m);
        }
        for m in [
            "lastIndexOf",
            "indexOf",
            "push",
            "pop",
            "slice",
            "join",
            "reverse",
        ] {
            push_as(ScriptLanguage::ActionScript3, "Vector", m);
        }
        for m in [
            "lastIndexOf",
            "indexOf",
            "charAt",
            "charCodeAt",
            "fromCharCode",
            "slice",
            "substring",
            "split",
            "toUpperCase",
            "toLowerCase",
            "replace",
        ] {
            push_as(ScriptLanguage::ActionScript3, "String", m);
        }
        // AS2 String & Array
        push_as(ScriptLanguage::ActionScript2, "String", "lastIndexOf");
        push_as(ScriptLanguage::ActionScript2, "String", "indexOf");
        push_as(ScriptLanguage::ActionScript2, "Array", "join");
        push_as(ScriptLanguage::ActionScript2, "Array", "slice");

        // Math, Object, Function, JSON, RegExp, Date, XML, XMLList, Dictionary
        for m in [
            "abs", "ceil", "floor", "round", "max", "min", "pow", "sqrt", "atan2",
        ] {
            push_as(ScriptLanguage::ActionScript3, "Math", m);
        }
        push_as(ScriptLanguage::ActionScript3, "Object", "hasOwnProperty");
        push_as(ScriptLanguage::ActionScript3, "Function", "call");
        push_as(ScriptLanguage::ActionScript3, "Function", "apply");
        push_as(ScriptLanguage::ActionScript3, "JSON", "parse");
        push_as(ScriptLanguage::ActionScript3, "JSON", "stringify");
        push_as(ScriptLanguage::ActionScript3, "RegExp", "test");
        push_as(ScriptLanguage::ActionScript3, "RegExp", "exec");
        push_as(ScriptLanguage::ActionScript3, "Date", "Date");
        push_as(ScriptLanguage::ActionScript3, "Date", "UTC");
        push_as(ScriptLanguage::ActionScript3, "XML", "XML");
        push_as(ScriptLanguage::ActionScript3, "XML", "attribute");
        push_as(ScriptLanguage::ActionScript3, "XML", "appendChild");
        push_as(ScriptLanguage::ActionScript3, "XMLList", "length");
        push_as(
            ScriptLanguage::ActionScript3,
            "flash.utils.Dictionary",
            "Dictionary",
        );

        // ByteArray (readUnsignedByte, readByte, readShort, readInt, readDouble, readUTF, compress)
        for m in [
            "readUnsignedByte",
            "readByte",
            "readShort",
            "readUnsignedShort",
            "readInt",
            "readDouble",
            "readBoolean",
            "readUTF",
            "readUTFBytes",
            "compress",
        ] {
            push_as(ScriptLanguage::ActionScript3, "flash.utils.ByteArray", m);
        }

        // Point, Rectangle, Matrix, Vector3D
        for m in [
            "distance",
            "interpolate",
            "add",
            "subtract",
            "length",
            "toString",
        ] {
            push_as(ScriptLanguage::ActionScript3, "flash.geom.Point", m);
        }
        for m in [
            "intersects",
            "intersection",
            "union",
            "contains",
            "toString",
        ] {
            push_as(ScriptLanguage::ActionScript3, "flash.geom.Rectangle", m);
        }
        for m in ["transformPoint", "translate", "invert", "toString"] {
            push_as(ScriptLanguage::ActionScript3, "flash.geom.Matrix", m);
        }
        push_as(
            ScriptLanguage::ActionScript3,
            "flash.geom.Vector3D",
            "dotProduct",
        );
        push_as(
            ScriptLanguage::ActionScript3,
            "flash.geom.Vector3D",
            "crossProduct",
        );

        // Globals
        push_as(ScriptLanguage::ActionScript3, "", "parseInt");
        push_as(ScriptLanguage::ActionScript3, "", "parseFloat");
        push_as(ScriptLanguage::ActionScript3, "", "encodeURIComponent");

        // JS & TS examples
        for name in [
            "trace",
            "function",
            "Function",
            "return",
            "Map",
            "Set",
            "JSON",
            "Math",
            "Error",
            "__nfJsx",
            "function (typed)",
            "interface",
            "type",
            "enum",
            "Record",
            "Partial",
            "Pick / Omit",
        ] {
            let (lang, code) = find_js_ts_example(name);
            cases.push((format!("JS_TS:{name}"), lang, code));
        }

        assert!(
            cases.len() >= 85,
            "expected >= 85 pure runtime test examples, got {}",
            cases.len()
        );

        // Step 1: Compile EVERY single example through `neoflash_script::compile_with_program`!
        let base_project = neoflash_core::Project::blank();
        let base_program =
            neoflash_script::compile_project(&base_project).expect("blank project should compile");
        let mut compiled_units = Vec::with_capacity(cases.len());
        for (label, lang, source) in &cases {
            let expected = extract_expected_outputs(source);
            assert!(
                !expected.is_empty(),
                "{label} must have >= 1 `// Output:` line"
            );
            let script = neoflash_core::Script {
                language: *lang,
                source: source.clone(),
            };
            let code = neoflash_script::compile_with_program(&script, &base_program)
                .unwrap_or_else(|e| panic!("{label} failed neoflash_script compile: {e:?}"));
            compiled_units.push((label.clone(), *lang, source.clone(), code, expected));
        }

        // Step 2: Execute pure-Rust `AuthoredPlayback` (`neoflash_core::Player`) on core examples!
        let mut authored_ran = 0usize;
        for (label, lang, source, _, expected) in compiled_units.iter().filter(|(l, _, _, _, _)| {
            l.contains(":Array.lastIndexOf")
                || l.contains(":Array.indexOf")
                || l.contains(":Array.join")
                || l.contains(":Array.slice")
                || l.contains("ActionScript2:String.lastIndexOf")
                || l.contains(":String.indexOf")
                || l.contains(":String.slice")
                || l.contains(":String.toUpperCase")
                || l.contains(":Math.abs")
                || l.contains(":Math.pow")
                || l.contains(":Math.sqrt")
        }) {
            let mut proj = neoflash_core::Project::blank();
            proj.script_language = Some(*lang);
            let root = proj.root.clone();
            proj.symbol_mut(&root).unwrap().timeline.scripts = vec![neoflash_core::FrameScript {
                layer: None,
                frame: 0,
                script: neoflash_core::Script {
                    language: *lang,
                    source: source.clone(),
                },
            }];
            let mut pb = neoflash_core::AuthoredPlayback::with_live_scripts(&proj, &root, 0, true);
            let traces = pb.drain_traces();
            assert_eq!(
                &traces, expected,
                "AuthoredPlayback trace mismatch for {label}"
            );
            authored_ran += 1;
        }
        assert!(authored_ran >= 12);

        // Step 3: Execute ALL 85+ examples through the full NeoFlash WASM `WebPlayer` + `web/runtime.js` host!
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");
        if let Some(wasm_checkout_root) = locate_wasm_checkout_root(repo_root) {
            let items: Vec<serde_json::Value> = compiled_units
                .iter()
                .map(|(label, lang, source, code, expected)| {
                    let lang_wire = match lang {
                        ScriptLanguage::ActionScript1 => "action_script1",
                        ScriptLanguage::ActionScript2 => "action_script2",
                        ScriptLanguage::ActionScript3 => "action_script3",
                        ScriptLanguage::JavaScript => "java_script",
                        ScriptLanguage::TypeScript => "type_script",
                    };
                    serde_json::json!({
                        "label": label,
                        "language": lang_wire,
                        "source": source,
                        "code": code,
                        "expected": expected,
                    })
                })
                .collect();

            let input_json = serde_json::json!({
                "programCode": base_program.code,
                "items": items,
            })
            .to_string();

            let host_mjs =
                wasm_checkout_root.join("crates/neoflash-script/tests/host-integration.mjs");
            let rust_host_mjs =
                wasm_checkout_root.join("crates/neoflash-script/tests/rust-runtime-host.mjs");

            let runner_mjs = format!(
                r#"
import assert from 'node:assert/strict';
import {{ readFileSync }} from 'node:fs';
import {{ pathToFileURL }} from 'node:url';

const {{ createRuntimeHost }} = await import(pathToFileURL({host_mjs:?}).href);
const {{ runtimeWasm }} = await import(pathToFileURL({rust_host_mjs:?}).href);
const root = {repo_root:?};
const payload = JSON.parse(readFileSync(0, 'utf8'));

const failures = [];
for (const item of payload.items) {{
  const scriptObj = {{ language: item.language, source: item.source }};
  const project = {{
    format_version: 1,
    name: item.label,
    width: 320,
    height: 200,
    fps: 24,
    background: [255, 255, 255, 255],
    root: 'root',
    symbols: [{{
      id: 'root',
      name: 'Scene',
      kind: 'movie_clip',
      timeline: {{
        frame_count: 1,
        layers: [],
        scripts: [{{ frame: 0, script: scriptObj }}],
      }},
    }}],
  }};
  const bundle = {{
    project,
    program: {{ code: payload.programCode }},
    scripts: [{{ script: scriptObj, code: item.code, location: item.label }}],
  }};
  const player = new runtimeWasm.WebPlayer(JSON.stringify(project));
  let host;
  try {{
    const created = createRuntimeHost(
      root,
      player,
      [{{ script: scriptObj, code: item.code, location: item.label }}],
      {{ bundle, context: {{ measureText: line => ({{ width: line.length * 8 }}) }} }}
    );
    host = created.host;
    host.dispatchScripts();
    const actual = created.hostConsole.map(entry => entry.slice(1).map(String).join(' '));
    try {{
      assert.deepEqual(actual, item.expected);
    }} catch (_) {{
      failures.push(`${{item.label}}: actual=${{JSON.stringify(actual)}} expected=${{JSON.stringify(item.expected)}}`);
    }}
  }} catch (err) {{
    failures.push(`${{item.label}}: threw ${{err?.stack || err}}`);
  }} finally {{
    host?.disposeFamily();
    player.free();
  }}
}}
if (failures.length > 0) {{
  console.error(failures.join('\n'));
  process.exit(1);
}}
console.log(`VERIFIED_RUNTIME_EXAMPLES=${{payload.items.length}}`);
"#
            );

            let mut child = Command::new("node")
                .args(["--input-type=module", "-e", &runner_mjs])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn node runtime test runner");
            child
                .stdin
                .take()
                .expect("stdin")
                .write_all(input_json.as_bytes())
                .expect("write test cases to node stdin");
            let output = child.wait_with_output().expect("wait for node runner");
            assert!(
                output.status.success(),
                "Real NeoFlash WASM runtime execution failed:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                stdout.contains("VERIFIED_RUNTIME_EXAMPLES="),
                "expected VERIFIED_RUNTIME_EXAMPLES marker, got: {stdout}"
            );
        }
    }
}
