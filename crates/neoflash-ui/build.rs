//! Generates Rust code from the exported design system in `interface/design-system`:
//! colour palettes (Dark / Light), spacing, radii, typography, icons and cursors.
//! The Figma export is the source of truth; never edit the generated files.
use serde_json::Value;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../interface/design-system")
        .canonicalize()
        .expect("interface/design-system exists");
    let tokens_path = root.join("tokens/tokens.json");
    let dump_path = root.join("docs/data/figma-dump.json");
    for path in [
        &tokens_path,
        &dump_path,
        &root.join("exports/icons/16"),
        &root.join("exports/icons/20"),
        &root.join("exports/cursors"),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let tokens: Value = serde_json::from_str(&fs::read_to_string(&tokens_path).unwrap()).unwrap();
    let dump: Value = serde_json::from_str(&fs::read_to_string(&dump_path).unwrap()).unwrap();

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out.join("tokens.rs"), tokens_rs(&tokens)).unwrap();
    fs::write(out.join("icons.rs"), icons_rs(&root)).unwrap();
    fs::write(out.join("cursors.rs"), cursors_rs(&root, &dump)).unwrap();
}

/// `color/bg/app` → `bg_app`, `timeline/classic-tween` → `timeline_classic_tween`.
fn ident(path: &[&str]) -> String {
    path.join("_").replace(['-', ' ', '/'], "_").to_lowercase()
}

fn flatten<'a>(prefix: Vec<&'a str>, value: &'a Value, out: &mut Vec<(Vec<&'a str>, &'a Value)>) {
    if let Some(v) = value.get("$value") {
        out.push((prefix, v));
        return;
    }
    if let Value::Object(map) = value {
        for (k, v) in map {
            let mut p = prefix.clone();
            p.push(k);
            flatten(p, v, out);
        }
    }
}

fn premultiplied(hex: &str) -> String {
    let h = hex.trim_start_matches('#');
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap() as u32;
    let a = if h.len() == 8 { b(6) } else { 255 };
    let pm = |c: u32| (c * a + 127) / 255;
    format!(
        "Color32::from_rgba_premultiplied({}, {}, {}, {a})",
        pm(b(0)),
        pm(b(2)),
        pm(b(4))
    )
}

fn px(v: &Value) -> f32 {
    v.as_str().unwrap().trim_end_matches("px").parse().unwrap()
}

fn tokens_rs(tokens: &Value) -> String {
    let mut s = String::new();
    let mut fields = Vec::new();
    flatten(vec![], &tokens["dark"]["color"], &mut fields);
    // Palette struct.
    writeln!(
        s,
        "/// Every NF2 colour token. Field = Figma variable `color/<group>/<name>`."
    )
    .unwrap();
    writeln!(
        s,
        "#[derive(Clone, Copy, Debug, PartialEq)]\npub struct Palette {{"
    )
    .unwrap();
    for (path, _) in &fields {
        writeln!(
            s,
            "    /// `color/{}`\n    pub {}: Color32,",
            path.join("/"),
            ident(path)
        )
        .unwrap();
    }
    writeln!(s, "}}\n").unwrap();
    for theme in ["dark", "light"] {
        let mut theme_fields = Vec::new();
        flatten(vec![], &tokens[theme]["color"], &mut theme_fields);
        writeln!(
            s,
            "pub const {}: Palette = Palette {{",
            theme.to_uppercase()
        )
        .unwrap();
        for (path, v) in theme_fields {
            writeln!(
                s,
                "    {}: {},",
                ident(&path),
                premultiplied(v.as_str().unwrap())
            )
            .unwrap();
        }
        writeln!(s, "}};\n").unwrap();
    }
    // Spacing and radius (same in both modes).
    writeln!(
        s,
        "/// `space/1..6`: 4 · 8 · 12 · 16 · 24 · 32.\npub mod space {{"
    )
    .unwrap();
    for (k, v) in tokens["dark"]["space"].as_object().unwrap() {
        writeln!(s, "    pub const S{k}: f32 = {:.1};", px(&v["$value"])).unwrap();
    }
    writeln!(s, "}}\n/// `radius/sm..full`.\npub mod radius {{").unwrap();
    for (k, v) in tokens["dark"]["radius"].as_object().unwrap() {
        writeln!(
            s,
            "    pub const {}: f32 = {:.1};",
            k.to_uppercase(),
            px(&v["$value"])
        )
        .unwrap();
    }
    writeln!(s, "}}\n").unwrap();
    // Typography.
    writeln!(s, "/// One entry per `NF2/*` text style.\n#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\npub enum TextStyle {{").unwrap();
    let styles = tokens["typography"].as_object().unwrap();
    for name in styles.keys() {
        writeln!(s, "    /// `NF2/{name}`\n    {},", name.replace(' ', "")).unwrap();
    }
    writeln!(
        s,
        "}}\n\nimpl TextStyle {{\n    pub const ALL: [Self; {}] = [",
        styles.len()
    )
    .unwrap();
    for name in styles.keys() {
        writeln!(s, "        Self::{},", name.replace(' ', "")).unwrap();
    }
    writeln!(
        s,
        "    ];\n    pub const fn spec(self) -> TextSpec {{\n        match self {{"
    )
    .unwrap();
    for (name, v) in styles {
        let v = &v["$value"];
        let family = if v["fontFamily"].as_str().unwrap().contains("Mono") {
            "Family::Mono"
        } else {
            "Family::Inter"
        };
        let tracking: f32 = v["letterSpacing"]
            .as_str()
            .map_or(0.0, |t| t.trim_end_matches("em").parse().unwrap_or(0.0));
        let upper = v["textCase"].as_str() == Some("UPPER");
        writeln!(
            s,
            "            Self::{} => TextSpec {{ name: \"NF2/{name}\", family: {family}, weight: {}, size: {:.1}, line_height: {:.1}, tracking_em: {tracking:.3}, uppercase: {upper} }},",
            name.replace(' ', ""),
            v["fontWeight"],
            px(&v["fontSize"]),
            px(&v["lineHeight"]),
        )
        .unwrap();
    }
    writeln!(s, "        }}\n    }}\n}}").unwrap();
    s
}

fn svg_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "svg"))
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), p))
        .collect();
    files.sort();
    files
}

fn const_name(name: &str) -> String {
    let n = name.replace(['-', ' '], "_").to_uppercase();
    if n.starts_with(|c: char| c.is_ascii_digit()) {
        format!("N{n}")
    } else {
        n
    }
}

fn icons_rs(root: &Path) -> String {
    let mut s = String::new();
    for size in ["16", "20"] {
        writeln!(
            s,
            "/// `NF2 Icon/{size}/*` ({size} px grid).\npub mod i{size} {{\n    use super::Icon;"
        )
        .unwrap();
        let files = svg_files(&root.join("exports/icons").join(size));
        for (name, path) in &files {
            writeln!(
                s,
                "    /// `NF2 Icon/{size}/{name}`\n    pub const {}: Icon = Icon {{ size: {size}, name: \"{name}\", svg: include_str!({:?}) }};",
                const_name(name),
                path.display().to_string()
            )
            .unwrap();
        }
        writeln!(s, "    pub const ALL: [Icon; {}] = [", files.len()).unwrap();
        for (name, _) in &files {
            writeln!(s, "        {},", const_name(name)).unwrap();
        }
        writeln!(s, "    ];\n}}\n").unwrap();
    }
    s
}

fn cursors_rs(root: &Path, dump: &Value) -> String {
    let mut hotspots = std::collections::HashMap::new();
    for c in dump["components"]["v2 · System & Components"]
        .as_array()
        .unwrap()
    {
        let name = c["name"].as_str().unwrap_or_default();
        if let Some(n) = name.strip_prefix("NF2 Cursor/") {
            let d = c["description"].as_str().unwrap_or_default();
            if let Some(rest) = d.split("Hotspot at x ").nth(1) {
                let nums: Vec<f32> = rest
                    .split(|ch: char| !ch.is_ascii_digit())
                    .filter(|x| !x.is_empty())
                    .take(2)
                    .map(|x| x.parse().unwrap())
                    .collect();
                hotspots.insert(n.to_string(), (nums[0], nums[1]));
            }
        }
    }
    let mut s = String::new();
    let files = svg_files(&root.join("exports/cursors"));
    for (name, path) in &files {
        let (x, y) = hotspots
            .get(name)
            .copied()
            .unwrap_or_else(|| panic!("cursor {name} has no hotspot in figma-dump.json"));
        writeln!(
            s,
            "/// `NF2 Cursor/{name}`, hotspot ({x}, {y}).\npub const {}: Cursor = Cursor {{ name: \"{name}\", hotspot: [{x:.1}, {y:.1}], svg: include_str!({:?}) }};",
            const_name(name),
            path.display().to_string()
        )
        .unwrap();
    }
    writeln!(s, "pub const ALL: [Cursor; {}] = [", files.len()).unwrap();
    for (name, _) in &files {
        writeln!(s, "    {},", const_name(name)).unwrap();
    }
    writeln!(s, "];").unwrap();
    s
}
