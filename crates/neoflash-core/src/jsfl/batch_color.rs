use super::*;

/// Target scope for built-in Smart Batch Rename (`batch_rename_library_and_instances`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchRenameScope {
    LibraryItemsOnly,
    StageInstancesOnly,
    #[default]
    LibraryAndInstances,
}

/// Options for built-in Smart Batch Rename of Library items and matching Stage instances.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchRenameOptions {
    pub scope: BatchRenameScope,
    /// Optional explicit symbol IDs to target. When `None`, all non-scene symbols/instances
    /// matching `name_filter` are targeted.
    pub target_symbol_ids: Option<Vec<Id>>,
    /// Optional filter applied to current names (supports `*` and `?` wildcards or substring match).
    pub name_filter: Option<String>,
    /// Optional substring or wildcard pattern (`*`, `?`) to replace with `replace`.
    pub find: Option<String>,
    pub replace: String,
    pub prefix: String,
    pub suffix: String,
    /// Optional starting number for sequential numbering (`Some(1)` -> `01`, `02`, ...).
    pub sequence_start: Option<u32>,
    pub sequence_step: u32,
    pub sequence_digits: usize,
    /// When true and renaming Library symbols in `LibraryAndInstances` scope, also updates the
    /// instance names of placed instances of those symbols across every timeline/layer/keyframe.
    pub sync_instance_names_to_symbol: bool,
}

impl Default for BatchRenameOptions {
    fn default() -> Self {
        Self {
            scope: BatchRenameScope::LibraryAndInstances,
            target_symbol_ids: None,
            name_filter: None,
            find: None,
            replace: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            sequence_start: None,
            sequence_step: 1,
            sequence_digits: 2,
            sync_instance_names_to_symbol: true,
        }
    }
}

/// Summary of items and instances renamed by `batch_rename_library_and_instances`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchRenameReport {
    /// `(symbol_id, old_name, new_name)`
    pub renamed_symbols: Vec<(Id, String, String)>,
    /// `(symbol_id, layer_id, keyframe_frame, element_id, old_name, new_name)`
    pub renamed_instances: Vec<(Id, Id, u32, Id, String, String)>,
    pub undo_label: String,
}

pub(super) fn wildcard_matches(pattern: &str, text: &str) -> bool {
    if !pattern.contains('*') && !pattern.contains('?') {
        return text.contains(pattern);
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let mut dp = vec![vec![false; t.len() + 1]; p.len() + 1];
    dp[0][0] = true;
    for i in 1..=p.len() {
        if p[i - 1] == '*' {
            dp[i][0] = dp[i - 1][0];
        }
    }
    for i in 1..=p.len() {
        for j in 1..=t.len() {
            if p[i - 1] == '*' {
                dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
            } else if p[i - 1] == '?' || p[i - 1] == t[j - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            }
        }
    }
    dp[p.len()][t.len()]
}

pub(super) fn format_renamed(
    original: &str,
    options: &BatchRenameOptions,
    seq_index: u32,
) -> Result<String, JsflDiagnostic> {
    let mut base = if let Some(find) = &options.find {
        if find.is_empty() {
            original.to_owned()
        } else if find.contains('*') || find.contains('?') {
            if wildcard_matches(find, original) {
                options.replace.clone()
            } else {
                original.to_owned()
            }
        } else {
            original.replace(find.as_str(), &options.replace)
        }
    } else if !options.replace.is_empty() {
        options.replace.clone()
    } else {
        original.to_owned()
    };

    let seq_str = options.sequence_start.map(|start| {
        let step = options.sequence_step.max(1);
        let num = start.saturating_add(seq_index.saturating_mul(step));
        let width = options.sequence_digits.clamp(1, 8);
        format!("{num:0width$}")
    });

    if let Some(seq) = &seq_str {
        if base.contains("{n}") {
            base = base.replace("{n}", seq);
        } else if base.contains('#') {
            base = base.replace('#', seq);
        } else {
            base = format!("{}{}{}{}", options.prefix, base, options.suffix, seq);
            return validate_renamed(&base);
        }
    }

    let result = format!("{}{}{}", options.prefix, base, options.suffix);
    validate_renamed(&result)
}

pub(super) fn validate_renamed(name: &str) -> Result<String, JsflDiagnostic> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 256 || trimmed.chars().any(char::is_control) {
        return Err(JsflDiagnostic::new(
            "<batch_rename>",
            1,
            1,
            format!("Invalid target name '{name}': must be 1..=256 non-control characters."),
        ));
    }
    Ok(trimmed.to_owned())
}

/// Built-in native Smart Batch Rename across Library symbols and matching Stage instances
/// across every symbol, layer, and keyframe in `project`.
///
/// Executes atomically on a cloned candidate project and validates with `crate::validate`
/// before committing.
pub fn batch_rename_library_and_instances(
    project: &mut Project,
    options: &BatchRenameOptions,
) -> Result<BatchRenameReport, JsflDiagnostic> {
    let mut candidate = project.clone();
    let mut report = BatchRenameReport {
        renamed_symbols: Vec::new(),
        renamed_instances: Vec::new(),
        undo_label: "Batch Rename Library & Instances".to_owned(),
    };

    let target_set: Option<HashSet<Id>> = options
        .target_symbol_ids
        .as_ref()
        .map(|ids| ids.iter().cloned().collect());

    let mut symbol_old_to_new: HashMap<Id, (String, String)> = HashMap::new();

    if matches!(
        options.scope,
        BatchRenameScope::LibraryItemsOnly | BatchRenameScope::LibraryAndInstances
    ) {
        let mut seq_idx = 0u32;
        let scene_ids: HashSet<Id> = candidate.scene_ids().into_iter().collect();
        for sym in &mut candidate.symbols {
            if scene_ids.contains(&sym.id) {
                continue;
            }
            if let Some(targets) = &target_set
                && !targets.contains(&sym.id)
            {
                continue;
            }
            if let Some(filter) = &options.name_filter
                && !filter.is_empty()
                && !wildcard_matches(filter, &sym.name)
            {
                continue;
            }
            let new_name = format_renamed(&sym.name, options, seq_idx)?;
            seq_idx += 1;
            if new_name != sym.name {
                let old_name = sym.name.clone();
                sym.name = new_name.clone();
                symbol_old_to_new.insert(sym.id.clone(), (old_name.clone(), new_name.clone()));
                report
                    .renamed_symbols
                    .push((sym.id.clone(), old_name, new_name));
            }
        }
    }

    if matches!(
        options.scope,
        BatchRenameScope::StageInstancesOnly | BatchRenameScope::LibraryAndInstances
    ) {
        let mut inst_seq_idx = 0u32;
        for sym in &mut candidate.symbols {
            let holder_sym_id = sym.id.clone();
            for layer in &mut sym.timeline.layers {
                let layer_id = layer.id.clone();
                for key in &mut layer.keyframes {
                    let kf_frame = key.frame;
                    let mut element_renames: Vec<(String, String)> = Vec::new();
                    for el in &mut key.elements {
                        let ElementKind::Instance { symbol_id, .. } = &el.kind else {
                            continue;
                        };
                        if let Some(targets) = &target_set
                            && !targets.contains(symbol_id)
                        {
                            continue;
                        }
                        let source_name = if el.name.is_empty() {
                            symbol_old_to_new
                                .get(symbol_id)
                                .map(|(old, _)| old.as_str())
                                .unwrap_or("")
                        } else {
                            el.name.as_str()
                        };
                        if let Some(filter) = &options.name_filter
                            && !filter.is_empty()
                        {
                            let matches_el = wildcard_matches(filter, &el.name);
                            let matches_sym = symbol_old_to_new.contains_key(symbol_id)
                                || (!source_name.is_empty()
                                    && wildcard_matches(filter, source_name));
                            if !matches_el && !matches_sym {
                                continue;
                            }
                        }
                        let new_inst_name = if options.scope
                            == BatchRenameScope::LibraryAndInstances
                            && options.sync_instance_names_to_symbol
                            && let Some((old_sym_name, new_sym_name)) =
                                symbol_old_to_new.get(symbol_id)
                        {
                            if el.name.is_empty() || el.name == *old_sym_name {
                                format!("{new_sym_name}_{:02}", inst_seq_idx + 1)
                            } else if el.name.contains(old_sym_name.as_str()) {
                                el.name.replace(old_sym_name.as_str(), new_sym_name)
                            } else {
                                format_renamed(&el.name, options, inst_seq_idx)?
                            }
                        } else {
                            let base_for_inst = if source_name.is_empty() {
                                "instance"
                            } else {
                                source_name
                            };
                            format_renamed(base_for_inst, options, inst_seq_idx)?
                        };
                        inst_seq_idx += 1;
                        if new_inst_name != el.name {
                            let old_name = el.name.clone();
                            el.name = new_inst_name.clone();
                            if !old_name.is_empty() {
                                element_renames.push((old_name.clone(), new_inst_name.clone()));
                            }
                            report.renamed_instances.push((
                                holder_sym_id.clone(),
                                layer_id.clone(),
                                kf_frame,
                                el.id.clone(),
                                old_name,
                                new_inst_name,
                            ));
                        }
                    }
                    for (old_target, new_target) in element_renames {
                        for ov in &mut key.instance_overrides {
                            if ov.target_name == old_target {
                                ov.target_name = new_target.clone();
                            }
                        }
                    }
                }
            }
        }
    }

    candidate.upgrade_format_for_features();
    let issues = validate(&candidate);
    if let Some(first) = issues.first() {
        return Err(JsflDiagnostic::new(
            "<batch_rename>",
            1,
            1,
            format!(
                "Project validation failed after batch rename ({}): {}",
                first.path, first.message
            ),
        ));
    }

    *project = candidate;
    Ok(report)
}

/// Summary of a unique colour used across a `Project` (`collect_project_colors`, `T-197`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectColorEntry {
    /// Unmultiplied `[R, G, B, A]` colour.
    pub color: Color,
    /// Formatted hex string (`#rrggbb` when opaque, `#rrggbbaa` when `alpha < 255`).
    pub hex: String,
    /// Number of solid fill slots using this colour.
    pub fill_count: usize,
    /// Number of solid stroke slots using this colour.
    pub stroke_count: usize,
    /// Number of gradient stops (linear, radial, freeform, gradient mesh) using this colour.
    pub gradient_count: usize,
    /// Number of text elements / text format runs using this colour.
    pub text_count: usize,
    /// Whether this colour is also stored in the document's custom or global swatches.
    pub in_custom_swatches: bool,
}

impl ProjectColorEntry {
    pub fn total_uses(&self) -> usize {
        self.fill_count
            + self.stroke_count
            + self.gradient_count
            + self.text_count
            + usize::from(self.in_custom_swatches)
    }

    pub fn roles_summary(&self) -> String {
        let mut roles = Vec::new();
        if self.fill_count > 0 {
            roles.push("fill");
        }
        if self.stroke_count > 0 {
            roles.push("stroke");
        }
        if self.gradient_count > 0 {
            roles.push("gradient");
        }
        if self.text_count > 0 {
            roles.push("text");
        }
        if self.in_custom_swatches && roles.is_empty() {
            roles.push("swatch");
        }
        roles.join(", ")
    }
}

/// Options for project-wide or scoped colour replacement (`change_project_color`, `T-197`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeColorOptions {
    pub from: Color,
    pub to: Color,
    /// When `true`, matches `[R, G, B]` regardless of the slot's alpha channel.
    pub match_rgb_only: bool,
    /// When `true`, preserves each matched slot's original alpha channel instead of overwriting with `to[3]`.
    pub preserve_slot_alpha: bool,
    /// Optional filter restricting replacement to specific symbol IDs (`None` = all symbols).
    pub target_symbol_ids: Option<Vec<Id>>,
    /// Optional filter restricting replacement to specific `(symbol_id, layer_id, frame, element_id)` placements.
    pub target_elements: Option<Vec<(Id, Id, u32, Id)>>,
    /// Whether to also update matching entries in `project.custom_swatches()` and `project.global_swatches()`.
    pub include_custom_swatches: bool,
}

impl ChangeColorOptions {
    pub fn new(from: Color, to: Color) -> Self {
        Self {
            from,
            to,
            match_rgb_only: false,
            preserve_slot_alpha: false,
            target_symbol_ids: None,
            target_elements: None,
            include_custom_swatches: true,
        }
    }
}

/// Summary of colour slots replaced by `change_project_color` or `doc.changeColor(...)` (`T-197`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeColorReport {
    pub replaced_fills: usize,
    pub replaced_strokes: usize,
    pub replaced_gradient_stops: usize,
    pub replaced_texts: usize,
    pub replaced_swatches: usize,
    /// `(symbol_id, layer_id, keyframe_frame, element_id)`
    pub affected_elements: Vec<(Id, Id, u32, Id)>,
    pub undo_label: String,
}

impl ChangeColorReport {
    pub fn total_replaced(&self) -> usize {
        self.replaced_fills
            + self.replaced_strokes
            + self.replaced_gradient_stops
            + self.replaced_texts
            + self.replaced_swatches
    }
}

pub(super) fn format_rgba_hex(c: Color) -> String {
    if c[3] == 255 {
        format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
    }
}

/// Canonical JSFL `change_color.jsfl` script (`T-197`).
///
/// Traverses `fl.getDocumentDOM()`, all scene timelines (`doc.timelines`) and all Library symbol
/// timelines (`doc.library.items`), inspecting every keyframe's elements and replacing `fromColor`
/// with `toColor` across solid fills, linear/radial gradient `colorArray` stops, strokes, and text.
pub const CHANGE_COLOR_JSFL_SCRIPT: &str = r##"
function normalizeHex(hex) {
    var s = String(hex).trim().toLowerCase();
    if (s.indexOf("#") !== 0) {
        s = "#" + s;
    }
    return s;
}

function changeColorInTimeline(tl, targetHex, replacementHex) {
    var replaced = 0;
    var layers = tl.layers;
    for (var l = 0; l < layers.length; l++) {
        var layer = layers[l];
        var frames = layer.frames;
        for (var f = 0; f < frames.length; f++) {
            var frame = frames[f];
            if (frame.startFrame !== f) {
                continue;
            }
            var elements = frame.elements;
            for (var e = 0; e < elements.length; e++) {
                var el = elements[e];
                var et = el.elementType;
                if (et === "shape" || et === "shapeObj") {
                    var fill = el.getCustomFill();
                    if (fill.style === "solid" && normalizeHex(fill.color) === targetHex) {
                        fill.color = replacementHex;
                        el.setCustomFill(fill);
                        replaced++;
                    } else if (fill.colorArray && fill.colorArray.length > 0) {
                        var fillChanged = false;
                        for (var k = 0; k < fill.colorArray.length; k++) {
                            if (normalizeHex(fill.colorArray[k]) === targetHex) {
                                fill.colorArray[k] = replacementHex;
                                fillChanged = true;
                                replaced++;
                            }
                        }
                        if (fillChanged) {
                            el.setCustomFill(fill);
                        }
                    }
                    var stroke = el.getCustomStroke();
                    if (stroke.style === "solid" && normalizeHex(stroke.color) === targetHex) {
                        stroke.color = replacementHex;
                        el.setCustomStroke(stroke);
                        replaced++;
                    } else if (stroke.colorArray && stroke.colorArray.length > 0) {
                        var strokeChanged = false;
                        for (var sk = 0; sk < stroke.colorArray.length; sk++) {
                            if (normalizeHex(stroke.colorArray[sk]) === targetHex) {
                                stroke.colorArray[sk] = replacementHex;
                                strokeChanged = true;
                                replaced++;
                            }
                        }
                        if (strokeChanged) {
                            el.setCustomStroke(stroke);
                        }
                    }
                } else if (et === "text") {
                    var tc = el.getTextAttr("fillColor");
                    if (tc && normalizeHex(tc) === targetHex) {
                        el.setTextAttr("fillColor", replacementHex);
                        replaced++;
                    }
                }
            }
        }
    }
    return replaced;
}

function changeColor(fromColor, toColor) {
    var doc = fl.getDocumentDOM();
    var targetHex = normalizeHex(fromColor);
    var replacementHex = normalizeHex(toColor);
    var total = 0;
    var scenes = doc.timelines;
    for (var s = 0; s < scenes.length; s++) {
        total += changeColorInTimeline(scenes[s], targetHex, replacementHex);
    }
    var items = doc.library.items;
    for (var i = 0; i < items.length; i++) {
        var item = items[i];
        if (item.itemType !== "folder" && item.timeline) {
            total += changeColorInTimeline(item.timeline, targetHex, replacementHex);
        }
    }
    fl.trace("Changed " + total + " colour occurrence(s) from " + targetHex + " to " + replacementHex);
    return total;
}
"##;

/// Execute the built-in `CHANGE_COLOR_JSFL_SCRIPT` (`change_color.jsfl`) in a sandboxed JSFL VM (`T-197`).
pub fn run_change_color_jsfl(
    project: &Project,
    state: &JsflAuthoringState,
    from: Color,
    to: Color,
) -> Result<JsflResult, JsflDiagnostic> {
    let from_hex = format_rgba_hex(from);
    let to_hex = format_rgba_hex(to);
    let mut sandbox = JsflSandbox::new();
    sandbox.register_virtual_script("file:///change_color.jsfl", CHANGE_COLOR_JSFL_SCRIPT);
    let invoke = format!(
        "fl.runScript(\"file:///change_color.jsfl\", \"changeColor\", {:?}, {:?});",
        from_hex, to_hex
    );
    sandbox.execute(project, state, "change_color.jsfl", &invoke)
}

/// Collect all unique visible colours used across `project` (or a scoped subset of symbols/elements)
/// in deterministic authoring order (`T-197`).
pub fn collect_project_colors(project: &Project) -> Vec<ProjectColorEntry> {
    collect_project_colors_scoped(project, None, None)
}

/// Collect unique visible colours restricted to `target_symbol_ids` and/or `target_elements` (`T-197`).
pub fn collect_project_colors_scoped(
    project: &Project,
    target_symbol_ids: Option<&HashSet<Id>>,
    target_elements: Option<&HashSet<(Id, Id, u32, Id)>>,
) -> Vec<ProjectColorEntry> {
    let mut entries: Vec<ProjectColorEntry> = Vec::new();
    let mut index_by_color: HashMap<Color, usize> = HashMap::new();

    #[derive(Clone, Copy)]
    enum Role {
        Fill,
        Stroke,
        Gradient,
        Text,
        Swatch,
    }

    let mut record = |c: Color, role: Role| {
        if c[3] == 0 {
            return;
        }
        let idx = *index_by_color.entry(c).or_insert_with(|| {
            let i = entries.len();
            entries.push(ProjectColorEntry {
                color: c,
                hex: format_rgba_hex(c),
                ..Default::default()
            });
            i
        });
        let entry = &mut entries[idx];
        match role {
            Role::Fill => entry.fill_count += 1,
            Role::Stroke => entry.stroke_count += 1,
            Role::Gradient => entry.gradient_count += 1,
            Role::Text => entry.text_count += 1,
            Role::Swatch => entry.in_custom_swatches = true,
        }
    };

    for sym in &project.symbols {
        if let Some(syms) = target_symbol_ids
            && !syms.contains(&sym.id)
        {
            continue;
        }
        for layer in &sym.timeline.layers {
            for key in &layer.keyframes {
                for el in &key.elements {
                    if let Some(els) = target_elements
                        && !els.contains(&(
                            sym.id.clone(),
                            layer.id.clone(),
                            key.frame,
                            el.id.clone(),
                        ))
                    {
                        continue;
                    }
                    match &el.kind {
                        ElementKind::Rectangle {
                            fill,
                            stroke,
                            stroke_width,
                            ..
                        }
                        | ElementKind::Ellipse {
                            fill,
                            stroke,
                            stroke_width,
                            ..
                        }
                        | ElementKind::Path {
                            fill,
                            stroke,
                            stroke_width,
                            ..
                        } => {
                            record(*fill, Role::Fill);
                            if *stroke_width > 0.0 {
                                record(*stroke, Role::Stroke);
                            }
                        }
                        ElementKind::PaintedPath {
                            fill,
                            stroke,
                            fill_gradient,
                            stroke_gradient,
                            stroke_width,
                            ..
                        } => {
                            if let Some(grad) = fill_gradient {
                                for stop in &grad.stops {
                                    record(stop.color, Role::Gradient);
                                }
                            } else {
                                record(*fill, Role::Fill);
                            }
                            if let Some(grad) = stroke_gradient {
                                for stop in &grad.stops {
                                    record(stop.color, Role::Gradient);
                                }
                            } else if *stroke_width > 0.0 {
                                record(*stroke, Role::Stroke);
                            }
                        }
                        ElementKind::Text { color, .. } => {
                            record(*color, Role::Text);
                        }
                        ElementKind::TextBox { format, .. } => {
                            let c = [
                                ((format.color >> 16) & 0xFF) as u8,
                                ((format.color >> 8) & 0xFF) as u8,
                                (format.color & 0xFF) as u8,
                                255,
                            ];
                            record(c, Role::Text);
                        }
                        _ => {}
                    }
                }
                for app in &key.appearances {
                    if let Some(els) = target_elements
                        && !els.contains(&(
                            sym.id.clone(),
                            layer.id.clone(),
                            key.frame,
                            app.element_id.clone(),
                        ))
                    {
                        continue;
                    }
                    for entry in &app.entries {
                        match entry {
                            crate::AppearancePaintEntry::Fill {
                                color, gradient, ..
                            } => {
                                if let Some(grad) = gradient {
                                    for s in &grad.stops {
                                        record(s.color, Role::Gradient);
                                    }
                                } else {
                                    record(*color, Role::Fill);
                                }
                            }
                            crate::AppearancePaintEntry::Stroke {
                                color,
                                gradient,
                                width,
                                ..
                            } => {
                                if let Some(grad) = gradient {
                                    for s in &grad.stops {
                                        record(s.color, Role::Gradient);
                                    }
                                } else if *width > 0.0 {
                                    record(*color, Role::Stroke);
                                }
                            }
                        }
                    }
                }
                for fx in &key.vector_effects {
                    if let Some(els) = target_elements
                        && !els.contains(&(
                            sym.id.clone(),
                            layer.id.clone(),
                            key.frame,
                            fx.element_id.clone(),
                        ))
                    {
                        continue;
                    }
                    if let Some(fg) = &fx.freeform_gradient {
                        for pt in &fg.points {
                            record(pt.color, Role::Gradient);
                        }
                        for line in &fg.lines {
                            record(line.start_color, Role::Gradient);
                            record(line.end_color, Role::Gradient);
                        }
                    }
                    if let Some(mesh) = &fx.gradient_mesh {
                        for v in &mesh.vertices {
                            record(v.color, Role::Gradient);
                        }
                    }
                }
            }
        }
    }

    if target_symbol_ids.is_none() && target_elements.is_none() {
        for sw in project.custom_swatches() {
            record(sw, Role::Swatch);
        }
        for gsw in project.global_swatches() {
            record(gsw.color, Role::Swatch);
        }
    }

    entries
}

/// Replace `options.from` with `options.to` across `project` (solid fills, strokes, linear/radial/
/// freeform/mesh gradients, text elements, appearance stacks, and custom/global swatches) and
/// validate atomically before committing (`T-197`).
pub fn change_project_color(
    project: &mut Project,
    options: &ChangeColorOptions,
) -> Result<ChangeColorReport, JsflDiagnostic> {
    let mut candidate = project.clone();
    let mut report = ChangeColorReport {
        undo_label: format!(
            "Change colour {} → {}",
            format_rgba_hex(options.from),
            format_rgba_hex(options.to)
        ),
        ..Default::default()
    };

    let target_syms: Option<HashSet<Id>> = options
        .target_symbol_ids
        .as_ref()
        .map(|v| v.iter().cloned().collect());
    let target_els: Option<HashSet<(Id, Id, u32, Id)>> = options
        .target_elements
        .as_ref()
        .map(|v| v.iter().cloned().collect());

    let matches_color = |c: Color| -> bool {
        if c[3] == 0 && options.from[3] > 0 {
            return false;
        }
        if options.match_rgb_only {
            c[0] == options.from[0] && c[1] == options.from[1] && c[2] == options.from[2]
        } else {
            c == options.from
        }
    };

    let apply_slot = |dst: &mut Color| -> bool {
        if !matches_color(*dst) {
            return false;
        }
        let mut next = options.to;
        if options.preserve_slot_alpha {
            next[3] = dst[3];
        }
        if *dst != next {
            *dst = next;
            true
        } else {
            false
        }
    };

    let mut push_affected = |loc: (Id, Id, u32, Id)| {
        if !report.affected_elements.contains(&loc) {
            report.affected_elements.push(loc);
        }
    };

    for sym in &mut candidate.symbols {
        if let Some(syms) = &target_syms
            && !syms.contains(&sym.id)
        {
            continue;
        }
        let sym_id = sym.id.clone();
        for layer in &mut sym.timeline.layers {
            let layer_id = layer.id.clone();
            for key in &mut layer.keyframes {
                let kf_frame = key.frame;
                for el in &mut key.elements {
                    let loc = (sym_id.clone(), layer_id.clone(), kf_frame, el.id.clone());
                    if let Some(els) = &target_els
                        && !els.contains(&loc)
                    {
                        continue;
                    }
                    let mut el_changed = false;
                    match &mut el.kind {
                        ElementKind::Rectangle {
                            fill,
                            stroke,
                            stroke_width,
                            ..
                        }
                        | ElementKind::Ellipse {
                            fill,
                            stroke,
                            stroke_width,
                            ..
                        }
                        | ElementKind::Path {
                            fill,
                            stroke,
                            stroke_width,
                            ..
                        } => {
                            if apply_slot(fill) {
                                report.replaced_fills += 1;
                                el_changed = true;
                            }
                            if *stroke_width > 0.0 && apply_slot(stroke) {
                                report.replaced_strokes += 1;
                                el_changed = true;
                            }
                        }
                        ElementKind::PaintedPath {
                            fill,
                            stroke,
                            fill_gradient,
                            stroke_gradient,
                            stroke_width,
                            ..
                        } => {
                            if let Some(grad) = fill_gradient {
                                let mut grad_changed = false;
                                for stop in &mut grad.stops {
                                    if apply_slot(&mut stop.color) {
                                        report.replaced_gradient_stops += 1;
                                        grad_changed = true;
                                        el_changed = true;
                                    }
                                }
                                if grad_changed && let Some(first) = grad.stops.first() {
                                    *fill = first.color;
                                }
                            } else if apply_slot(fill) {
                                report.replaced_fills += 1;
                                el_changed = true;
                            }
                            if let Some(grad) = stroke_gradient {
                                let mut grad_changed = false;
                                for stop in &mut grad.stops {
                                    if apply_slot(&mut stop.color) {
                                        report.replaced_gradient_stops += 1;
                                        grad_changed = true;
                                        el_changed = true;
                                    }
                                }
                                if grad_changed && let Some(first) = grad.stops.first() {
                                    *stroke = first.color;
                                }
                            } else if *stroke_width > 0.0 && apply_slot(stroke) {
                                report.replaced_strokes += 1;
                                el_changed = true;
                            }
                        }
                        ElementKind::Text { color, .. } => {
                            if apply_slot(color) {
                                report.replaced_texts += 1;
                                el_changed = true;
                            }
                        }
                        ElementKind::TextBox { format, .. } => {
                            let mut c = [
                                ((format.color >> 16) & 0xFF) as u8,
                                ((format.color >> 8) & 0xFF) as u8,
                                (format.color & 0xFF) as u8,
                                255,
                            ];
                            if apply_slot(&mut c) {
                                format.color = (u32::from(c[0]) << 16)
                                    | (u32::from(c[1]) << 8)
                                    | u32::from(c[2]);
                                report.replaced_texts += 1;
                                el_changed = true;
                            }
                        }
                        _ => {}
                    }
                    if el_changed {
                        push_affected(loc);
                    }
                }
                for app in &mut key.appearances {
                    let loc = (
                        sym_id.clone(),
                        layer_id.clone(),
                        kf_frame,
                        app.element_id.clone(),
                    );
                    if let Some(els) = &target_els
                        && !els.contains(&loc)
                    {
                        continue;
                    }
                    let mut app_changed = false;
                    for entry in &mut app.entries {
                        match entry {
                            crate::AppearancePaintEntry::Fill {
                                color, gradient, ..
                            } => {
                                if let Some(grad) = gradient {
                                    for s in &mut grad.stops {
                                        if apply_slot(&mut s.color) {
                                            report.replaced_gradient_stops += 1;
                                            app_changed = true;
                                        }
                                    }
                                } else if apply_slot(color) {
                                    report.replaced_fills += 1;
                                    app_changed = true;
                                }
                            }
                            crate::AppearancePaintEntry::Stroke {
                                color,
                                gradient,
                                width,
                                ..
                            } => {
                                if let Some(grad) = gradient {
                                    for s in &mut grad.stops {
                                        if apply_slot(&mut s.color) {
                                            report.replaced_gradient_stops += 1;
                                            app_changed = true;
                                        }
                                    }
                                } else if *width > 0.0 && apply_slot(color) {
                                    report.replaced_strokes += 1;
                                    app_changed = true;
                                }
                            }
                        }
                    }
                    if app_changed {
                        push_affected(loc);
                    }
                }
                for fx in &mut key.vector_effects {
                    let loc = (
                        sym_id.clone(),
                        layer_id.clone(),
                        kf_frame,
                        fx.element_id.clone(),
                    );
                    if let Some(els) = &target_els
                        && !els.contains(&loc)
                    {
                        continue;
                    }
                    let mut fx_changed = false;
                    if let Some(fg) = &mut fx.freeform_gradient {
                        for pt in &mut fg.points {
                            if apply_slot(&mut pt.color) {
                                report.replaced_gradient_stops += 1;
                                fx_changed = true;
                            }
                        }
                        for line in &mut fg.lines {
                            if apply_slot(&mut line.start_color) {
                                report.replaced_gradient_stops += 1;
                                fx_changed = true;
                            }
                            if apply_slot(&mut line.end_color) {
                                report.replaced_gradient_stops += 1;
                                fx_changed = true;
                            }
                        }
                    }
                    if let Some(mesh) = &mut fx.gradient_mesh {
                        for v in &mut mesh.vertices {
                            if apply_slot(&mut v.color) {
                                report.replaced_gradient_stops += 1;
                                fx_changed = true;
                            }
                        }
                    }
                    if fx_changed {
                        push_affected(loc);
                    }
                }
            }
        }
    }

    if options.include_custom_swatches && target_syms.is_none() && target_els.is_none() {
        let mut custom = candidate.custom_swatches();
        let mut custom_changed = false;
        for sw in &mut custom {
            if apply_slot(sw) {
                report.replaced_swatches += 1;
                custom_changed = true;
            }
        }
        if custom_changed {
            let mut deduped = Vec::with_capacity(custom.len());
            for sw in custom {
                if !deduped.contains(&sw) {
                    deduped.push(sw);
                }
            }
            candidate.set_custom_swatches(&deduped);
        }

        let mut global = candidate.global_swatches();
        let mut global_changed = false;
        for gsw in &mut global {
            if apply_slot(&mut gsw.color) {
                report.replaced_swatches += 1;
                global_changed = true;
            }
        }
        if global_changed {
            candidate.set_global_swatches(&global);
        }
    }

    candidate.upgrade_format_for_features();
    let issues = validate(&candidate);
    if let Some(first) = issues.first() {
        return Err(JsflDiagnostic::new(
            "<change_color>",
            1,
            1,
            format!(
                "Project validation failed after colour replacement ({}): {}",
                first.path, first.message
            ),
        ));
    }

    *project = candidate;
    Ok(report)
}
