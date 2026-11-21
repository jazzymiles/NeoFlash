use super::*;
use crate::code_tools::CodeCommand;
use crate::panels::help_manual::{
    HELP_CHAPTERS, LanguageScope, fit_text_to_width, plain_chapter_label, text_width,
};
use crate::theme::ScrollTheme as _;
use eframe::egui::{self, Rect, Stroke, StrokeKind, Ui, vec2};
use neoflash_core::ScriptLanguage;
use neoflash_ui::{
    TextStyle,
    components::{self as c, Badge, BadgeType, Button},
    icons::{self, i16},
    tokens::{radius, space},
    typography::{font, rich},
};
use std::collections::BTreeSet;
use std::sync::Arc;

/// Selection and view options of the API tab for one reference language.
#[derive(Clone, Debug, Default)]
pub(crate) struct ApiSelection {
    pub class: Option<String>,
    pub member: Option<String>,
    /// Open packages.
    pub open: BTreeSet<String>,
    /// Keyboard row in the tree column.
    pub cursor: usize,
}

/// State of the API tab, kept with the window state.
#[derive(Clone, Debug, Default)]
pub(crate) struct ApiBrowserState {
    as2: ApiSelection,
    as3: ApiSelection,
    js_ts: ApiSelection,
    pub filter: MemberFilter,
    pub hide_missing: bool,
    /// ↑↓ move in the member list instead of the tree.
    pub focus_members: bool,
    /// Bring the selected rows into view on the next frame.
    pub reveal_tree: bool,
    pub reveal_member: bool,
    hits: Option<(LanguageScope, String, Arc<Vec<ApiHit>>)>,
}

impl ApiBrowserState {
    pub(crate) fn selection(&self, scope: LanguageScope) -> &ApiSelection {
        match scope {
            LanguageScope::As1As2 => &self.as2,
            LanguageScope::As3 => &self.as3,
            LanguageScope::JsTs | LanguageScope::All => &self.js_ts,
        }
    }
    pub(crate) fn selection_mut(&mut self, scope: LanguageScope) -> &mut ApiSelection {
        match scope {
            LanguageScope::As1As2 => &mut self.as2,
            LanguageScope::As3 => &mut self.as3,
            LanguageScope::JsTs | LanguageScope::All => &mut self.js_ts,
        }
    }
    /// Search results for `query`, computed once per language and query.
    pub(crate) fn hits(&mut self, scope: LanguageScope, query: &str) -> Arc<Vec<ApiHit>> {
        let query = query.trim();
        if let Some((cached_scope, cached_query, hits)) = &self.hits
            && *cached_scope == scope
            && cached_query == query
        {
            return Arc::clone(hits);
        }
        let hits = Arc::new(api_search(scope, query));
        self.hits = Some((scope, query.to_owned(), Arc::clone(&hits)));
        self.selection_mut(scope).cursor = 0;
        hits
    }
    /// Opens `class_key` (and `member_key`) and reveals it in the tree.
    pub(crate) fn select(
        &mut self,
        scope: LanguageScope,
        class_key: &str,
        member_key: Option<&str>,
    ) {
        let index = api_index(scope);
        if index.class(class_key).is_none() {
            return;
        }
        let package = index.package_of(class_key).map(str::to_owned);
        let selection = self.selection_mut(scope);
        selection.class = Some(class_key.to_owned());
        selection.member = member_key.map(str::to_owned);
        if let Some(package) = package {
            selection.open.insert(package);
        }
        self.reveal_tree = true;
        self.reveal_member = member_key.is_some();
        self.focus_members = member_key.is_some();
    }
    /// First use of a language: open its default class so the window never starts empty.
    pub(crate) fn ensure_selection(&mut self, scope: LanguageScope) {
        if self.selection(scope).class.is_some() {
            return;
        }
        if let Some(key) = api_index(scope).default_class() {
            let key = key.to_owned();
            self.select(scope, &key, None);
            self.focus_members = false;
        }
    }
    pub(crate) fn selected_class(&self, scope: LanguageScope) -> Option<&'static Arc<ApiClass>> {
        api_index(scope).class(self.selection(scope).class.as_deref()?)
    }
    pub(crate) fn selected_member(&self, scope: LanguageScope) -> Option<&'static ApiMember> {
        let class = self.selected_class(scope)?;
        class.member(self.selection(scope).member.as_deref()?)
    }
    /// Members of the selected class that pass the filter row, in list order.
    pub(crate) fn visible_members(&self, scope: LanguageScope) -> Vec<&'static ApiMember> {
        self.selected_class(scope)
            .map(|class| {
                class
                    .members
                    .iter()
                    .filter(|m| m.group.passes(self.filter))
                    .filter(|m| !(self.hide_missing && m.support == Some(Support::Missing)))
                    .collect()
            })
            .unwrap_or_default()
    }
}

// ── Scripting Reference: rows, blocks and the three API columns ──

/// Tooltip of a disabled Insert at cursor.
pub(crate) const NO_SCRIPT_EDITOR: &str =
    "Open a script in the Actions panel to insert at its cursor";

/// What the columns ask the window to do after the frame.
#[derive(Default)]
pub(crate) struct RefActions {
    pub insert: Option<CodeCommand>,
    pub open_in_browser: bool,
}

/// List keys of the frame, taken by the window before any field sees them.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RefKeys {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub enter: bool,
}

/// `1234567` → `1 234 567`.
pub(crate) fn group_thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

/// Left text of the footer status row on the API tab.
pub(crate) fn api_footer_text(scope: LanguageScope) -> String {
    let index = api_index(scope);
    if index.flat {
        let mut working = 0usize;
        let mut partial = 0usize;
        for class in index.classes.values() {
            let (w, p, _) = class.support_counts();
            working += w;
            partial += p;
        }
        return format!(
            "{} · {} categories · {} entries · {} working · {} partial",
            scope.reference_label(),
            group_thousands(index.class_count),
            group_thousands(index.member_count),
            group_thousands(working),
            group_thousands(partial),
        );
    }
    format!(
        "{} · {} classes · {} members",
        scope.reference_label(),
        group_thousands(index.class_count),
        group_thousands(index.member_count)
    )
}

/// One list row of the Reference (tree row, search hit, member, outline entry): `row()` high.
pub(crate) struct RefRow<'a> {
    pub label: &'a str,
    pub style: TextStyle,
    pub depth: usize,
    /// Disclosure chevron of a package: `Some(open)`.
    pub chevron: Option<bool>,
    pub icon: Option<neoflash_ui::Icon>,
    /// Status dot column: `Some(None)` keeps the column empty.
    pub dot: Option<Option<Support>>,
    /// Signature after the label, `Mono Small`, shortened with an ellipsis.
    pub trailing: &'a str,
    /// Right-aligned secondary text.
    pub meta: &'a str,
    pub selected: bool,
    /// Keyboard row that is not the selection.
    pub cursor: bool,
    pub dimmed: bool,
}

impl<'a> RefRow<'a> {
    pub(crate) fn new(label: &'a str) -> Self {
        Self {
            label,
            style: TextStyle::Body,
            depth: 0,
            chevron: None,
            icon: None,
            dot: None,
            trailing: "",
            meta: "",
            selected: false,
            cursor: false,
            dimmed: false,
        }
    }

    pub(crate) fn paint(&self, ui: &Ui, rect: Rect, response: &egui::Response) {
        if !ui.is_rect_visible(rect) {
            return;
        }
        let p = neoflash_ui::palette(ui.ctx());
        let corner = egui::CornerRadius::same(radius::sm() as u8);
        if self.selected {
            ui.painter().rect_filled(rect, corner, p.bg_selected);
        } else if response.hovered() || self.cursor {
            ui.painter().rect_filled(rect, corner, p.bg_hover);
        }
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        let cy = rect.center().y;
        let mut x =
            rect.left() + space::s3() + self.depth as f32 * neoflash_ui::metrics::LAYER_INDENT;
        if let Some(open) = self.chevron {
            icons::paint(
                &painter,
                ui.ctx(),
                Rect::from_center_size(egui::pos2(x + 6.0, cy), vec2(12.0, 12.0)),
                if open { i16::CHEVDOWN } else { i16::CHEVRIGHT },
                p.text_secondary,
            );
            x += 12.0 + space::s2();
        }
        if let Some(icon) = self.icon {
            icons::paint(
                &painter,
                ui.ctx(),
                Rect::from_center_size(egui::pos2(x + 8.0, cy), vec2(16.0, 16.0)),
                icon,
                if self.selected {
                    p.accent_default
                } else {
                    p.text_secondary
                },
            );
            x += 16.0 + space::s2();
        }
        if let Some(dot) = self.dot {
            if let Some(support) = dot {
                painter.circle_filled(egui::pos2(x + 3.0, cy), 3.0, support.color(p));
            }
            x += 6.0 + space::s2();
        }
        let mut right = rect.right() - space::s3();
        if !self.meta.is_empty() {
            let galley = painter.layout_no_wrap(
                self.meta.to_owned(),
                font(TextStyle::Small),
                p.text_tertiary,
            );
            right -= galley.size().x;
            painter.galley(
                egui::pos2(right, cy - galley.size().y / 2.0),
                galley,
                p.text_tertiary,
            );
            right -= space::s2();
        }
        let color = if self.dimmed {
            p.text_tertiary
        } else if self.selected || self.style == TextStyle::Mono {
            p.text_primary
        } else {
            p.text_secondary
        };
        let label = fit_text_to_width(ui, self.label, self.style, (right - x).max(0.0));
        let galley = painter.layout_no_wrap(label, font(self.style), color);
        let label_w = galley.size().x;
        painter.galley(egui::pos2(x, cy - galley.size().y / 2.0), galley, color);
        x += label_w + space::s2();
        if !self.trailing.is_empty() && right - x > 24.0 {
            let text = fit_text_to_width(ui, self.trailing, TextStyle::MonoSmall, right - x);
            let galley = painter.layout_no_wrap(text, font(TextStyle::MonoSmall), p.text_tertiary);
            painter.galley(
                egui::pos2(x, cy - galley.size().y / 2.0),
                galley,
                p.text_tertiary,
            );
        }
        let (label, selected) = (self.label.to_owned(), self.selected);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &label)
        });
    }

    /// Adds the row at the cursor, as wide as the column.
    pub(crate) fn show(&self, ui: &mut Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width(), neoflash_ui::metrics::row()),
            egui::Sense::click(),
        );
        self.paint(ui, rect, &response);
        response
    }
}

/// Overline caption of a block in the class and detail columns.
pub(crate) fn group_caption(ui: &mut Ui, text: &str) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.add_space(space::s3());
    ui.label(rich(text, TextStyle::Overline).color(p.text_tertiary));
    ui.add_space(space::s1());
}

/// Code box: `bg/field`, 1 px `border/subtle`, syntax-coloured `Mono`, wrapped to the column.
pub(crate) fn code_box(ui: &mut Ui, code: &str) -> egui::Response {
    let p = neoflash_ui::palette(ui.ctx());
    let pad = vec2(space::s3(), space::s2());
    let width = ui.available_width().max(2.0 * pad.x + 8.0);
    let mut job = c::code::highlight(ui.ctx(), code);
    job.wrap.max_width = width - 2.0 * pad.x;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let (rect, response) = ui.allocate_exact_size(
        vec2(width, galley.size().y + 2.0 * pad.y),
        egui::Sense::hover(),
    );
    if ui.is_rect_visible(rect) {
        let corner = egui::CornerRadius::same(radius::md() as u8);
        ui.painter().rect_filled(rect, corner, p.bg_field);
        ui.painter().rect_stroke(
            rect,
            corner,
            Stroke::new(1.0_f32, p.border_subtle),
            StrokeKind::Inside,
        );
        ui.painter()
            .with_clip_rect(rect.intersect(ui.clip_rect()))
            .galley(rect.min + pad, galley, p.text_primary);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, code));
    response
}

/// Whether Insert at cursor has a script editor to write to (set by the window every frame).
fn insert_target_id() -> egui::Id {
    egui::Id::new("neoflash-reference-insert-target")
}
fn article_insert_id() -> egui::Id {
    egui::Id::new("neoflash-reference-article-insert")
}
pub(crate) fn set_insert_available(ctx: &egui::Context, available: bool) {
    ctx.data_mut(|d| d.insert_temp(insert_target_id(), available));
}
fn insert_available(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(insert_target_id()))
        .unwrap_or(false)
}
/// Code a guide sample asked to insert during this frame.
pub(crate) fn take_article_insert(ctx: &egui::Context) -> Option<String> {
    ctx.data_mut(|d| d.remove_temp::<String>(article_insert_id()))
}

/// `Insert at cursor`, disabled with a tooltip while no script editor is open.
fn insert_button(ui: &mut Ui, primary: bool) -> bool {
    let available = insert_available(ui.ctx());
    let button = if primary {
        Button::primary("Insert at cursor")
    } else {
        Button::new("Insert at cursor")
    };
    let response = ui.add_enabled(available, button);
    if available {
        return response.clicked();
    }
    // egui does not hover disabled widgets, so the reason is shown from `contains_pointer`.
    let ctx = ui.ctx();
    if response.contains_pointer() && !ctx.input(|i| i.pointer.any_down()) {
        let delay = f64::from(ctx.style().interaction.tooltip_delay);
        let still = f64::from(ctx.input(|i| i.pointer.time_since_last_movement()));
        if still < delay {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay - still + 0.01));
        } else {
            c::paint_tooltip_bubble(ctx, response.rect, NO_SCRIPT_EDITOR, None);
        }
    }
    false
}

/// `Copy` and `Insert at cursor` under a guide code sample.
pub(super) fn code_sample_actions(ui: &mut Ui, code: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::s2();
        if ui.add(Button::new("Copy").icon(i16::COPY)).clicked() {
            ui.ctx().copy_text(code.to_owned());
        }
        if insert_button(ui, false) {
            ui.ctx()
                .data_mut(|d| d.insert_temp(article_insert_id(), code.to_owned()));
        }
    });
}

/// Status pill of a class or member.
fn support_badge(ui: &mut Ui, support: Support) {
    let tone = match support {
        Support::Working => BadgeType::Success,
        Support::Partial => BadgeType::Warning,
        Support::Missing => BadgeType::Neutral,
    };
    ui.add(Badge::new(support.label(), tone));
}

/// "In NeoFlash" line: the catalogue status in words, with its note when there is one.
fn support_message(ui: &mut Ui, support: Support, note: &str) {
    let text = match support {
        Support::Working if note.is_empty() => {
            "Working. The documented behaviour runs in Test Movie and HTML5 publish; only browser or sandbox limits may remain.".to_owned()
        }
        Support::Working => format!("Working. {note}"),
        Support::Partial if note.is_empty() => {
            "Partial. A runtime path exists; part of the documented behaviour is missing.".to_owned()
        }
        Support::Partial => format!("Partial. {note}"),
        Support::Missing => {
            "Missing. NeoFlash has no runtime path for it yet; the compiler reports its use.".to_owned()
        }
    };
    ui.add(match support {
        Support::Working => c::InlineMessage::success(&text),
        Support::Partial => c::InlineMessage::warning(&text),
        Support::Missing => c::InlineMessage::info(&text),
    });
}

/// `Mono Small` text that opens another declaration: accent and underlined while hovered.
fn mono_link(ui: &mut Ui, text: &str, color: egui::Color32) -> egui::Response {
    let p = neoflash_ui::palette(ui.ctx());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font(TextStyle::MonoSmall), color);
    let (rect, response) = ui.allocate_exact_size(galley.size(), egui::Sense::click());
    let hovered = response.hovered();
    let tint = if hovered { p.accent_default } else { color };
    ui.painter()
        .galley_with_override_text_color(rect.min, galley, tint);
    if hovered {
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0_f32, p.accent_default),
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let label = text.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, &label));
    response
}

fn member_icon(member: &ApiMember) -> neoflash_ui::Icon {
    match member.group {
        MemberGroup::Properties => i16::VARIABLE,
        MemberGroup::Events => i16::FLAG,
        MemberGroup::Methods | MemberGroup::Language => i16::CODE,
    }
}

/// Tree column of the API tab: packages → classes, or the flat search results.
pub(crate) fn api_tree_column(
    ui: &mut Ui,
    state: &mut ApiBrowserState,
    scope: LanguageScope,
    query: &str,
    keys: RefKeys,
) {
    enum Row<'a> {
        Package(&'a ApiPackage, bool),
        Class {
            key: &'a str,
            name: &'a str,
            depth: usize,
        },
        Hit(&'a ApiHit),
    }
    let p = neoflash_ui::palette(ui.ctx());
    let index = api_index(scope);
    let searching = !query.trim().is_empty();
    let hits = searching.then(|| state.hits(scope, query));
    let selection = state.selection(scope).clone();
    let mut rows: Vec<Row> = Vec::new();
    if let Some(hits) = &hits {
        rows.extend(hits.iter().map(Row::Hit));
    } else {
        for package in &index.packages {
            let open = index.flat || selection.open.contains(&package.label);
            if !index.flat {
                rows.push(Row::Package(package, open));
            }
            if open {
                rows.extend(package.classes.iter().map(|(key, name)| Row::Class {
                    key,
                    name,
                    depth: usize::from(!index.flat),
                }));
            }
        }
    }
    let is_selected = |row: &Row| match row {
        Row::Package(..) => false,
        Row::Class { key, .. } => selection.class.as_deref() == Some(key),
        Row::Hit(hit) => {
            selection.class.as_deref() == Some(hit.class_key.as_str())
                && selection.member == hit.member_key
        }
    };
    let count = rows.len();
    let mut cursor = selection.cursor.min(count.saturating_sub(1));
    let mut reveal = std::mem::take(&mut state.reveal_tree);
    if reveal && let Some(at) = rows.iter().position(&is_selected) {
        cursor = at;
    }
    // What the frame does to the row under the keyboard or the pointer.
    enum Act {
        Select(usize, bool),
        Toggle(usize),
    }
    let mut act: Option<Act> = None;
    if !state.focus_members && count > 0 {
        if keys.down || keys.up {
            cursor = if keys.down {
                (cursor + 1).min(count - 1)
            } else {
                cursor.saturating_sub(1)
            };
            reveal = true;
            if !matches!(rows[cursor], Row::Package(..)) {
                act = Some(Act::Select(cursor, false));
            }
        } else if keys.enter {
            act = Some(match rows[cursor] {
                Row::Package(..) => Act::Toggle(cursor),
                _ => Act::Select(cursor, true),
            });
        } else if keys.right {
            match rows[cursor] {
                Row::Package(_, false) => act = Some(Act::Toggle(cursor)),
                Row::Package(..) => {}
                _ => act = Some(Act::Select(cursor, true)),
            }
        } else if keys.left && matches!(rows[cursor], Row::Package(_, true)) {
            act = Some(Act::Toggle(cursor));
        }
    }

    let row_h = neoflash_ui::metrics::row();
    let pitch = row_h + 1.0;
    let pad = space::s1();
    egui::ScrollArea::vertical()
        .id_salt(("reference-api-tree", scope.html_slug(), searching))
        .auto_shrink([false, false])
        .themed()
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if searching {
                let summary = if count == 0 {
                    format!(
                        "No classes or members of {} match “{}”.",
                        scope.reference_label(),
                        query.trim()
                    )
                } else {
                    format!(
                        "{} {}",
                        group_thousands(count),
                        if count == 1 { "match" } else { "matches" }
                    )
                };
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(
                        (pad + space::s3()) as i8,
                        space::s2() as i8,
                    ))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                rich(&summary, TextStyle::Small).color(p.text_tertiary),
                            )
                            .wrap(),
                        );
                    });
            }
            let width = ui.available_width();
            let (area, _) = ui.allocate_exact_size(
                vec2(width, count as f32 * pitch + 2.0 * pad),
                egui::Sense::hover(),
            );
            let row_rect = |i: usize| {
                Rect::from_min_size(
                    egui::pos2(area.left() + pad, area.top() + pad + i as f32 * pitch),
                    vec2(width - 2.0 * pad, row_h),
                )
            };
            let clip = ui.clip_rect();
            let first =
                (((clip.top() - area.top() - pad) / pitch).floor().max(0.0) as usize).min(count);
            let last =
                (((clip.bottom() - area.top() - pad) / pitch).ceil().max(0.0) as usize).min(count);
            for (i, row) in rows.iter().enumerate().take(last).skip(first) {
                let rect = row_rect(i);
                let selected = is_selected(row);
                let at_cursor = i == cursor && !state.focus_members;
                let (id, spec) = match row {
                    Row::Package(package, open) => (
                        ui.id().with(("package", &package.label)),
                        RefRow {
                            chevron: Some(*open),
                            icon: Some(i16::PACKAGE),
                            ..RefRow::new(&package.label)
                        },
                    ),
                    Row::Class { key, name, depth } => (
                        ui.id().with(("class", key)),
                        RefRow {
                            depth: *depth,
                            icon: Some(if index.flat {
                                i16::PACKAGE
                            } else if key.starts_with('<') || *name == "Package functions" {
                                i16::CODE
                            } else {
                                i16::CLASS
                            }),
                            ..RefRow::new(name)
                        },
                    ),
                    Row::Hit(hit) => (
                        ui.id().with(("hit", &hit.class_key, &hit.member_key)),
                        RefRow {
                            icon: Some(match &hit.member_key {
                                None => i16::CLASS,
                                Some(key) => index
                                    .class(&hit.class_key)
                                    .and_then(|class| class.member(key))
                                    .map_or(i16::CODE, member_icon),
                            }),
                            meta: if hit.member_key.is_none() {
                                &hit.meta
                            } else {
                                ""
                            },
                            ..RefRow::new(&hit.label)
                        },
                    ),
                };
                let response = ui.interact(rect, id, egui::Sense::click());
                RefRow {
                    selected,
                    cursor: at_cursor && !selected,
                    ..spec
                }
                .paint(ui, rect, &response);
                if response.clicked() {
                    cursor = i;
                    state.focus_members = false;
                    act = Some(match row {
                        Row::Package(..) => Act::Toggle(i),
                        Row::Hit(hit) => Act::Select(i, hit.member_key.is_some()),
                        Row::Class { .. } => Act::Select(i, false),
                    });
                }
            }
            if reveal && count > 0 {
                ui.scroll_to_rect(row_rect(cursor).expand(row_h), None);
            }
        });

    // Apply after the rows are painted: the borrow of `rows` ends here.
    let outcome = act.map(|act| match act {
        Act::Toggle(i) => match &rows[i] {
            Row::Package(package, _) => (Some(package.label.clone()), None, false),
            _ => (None, None, false),
        },
        Act::Select(i, to_members) => match &rows[i] {
            Row::Class { key, .. } => (None, Some(((*key).to_owned(), None)), to_members),
            Row::Hit(hit) => (
                None,
                Some((hit.class_key.clone(), hit.member_key.clone())),
                to_members,
            ),
            Row::Package(..) => (None, None, false),
        },
    });
    drop(rows);
    let target = state.selection_mut(scope);
    target.cursor = cursor;
    if let Some((toggle, select, to_members)) = outcome {
        if let Some(package) = toggle
            && !target.open.remove(&package)
        {
            target.open.insert(package);
        }
        if let Some((class, member)) = select {
            let changed = target.class.as_deref() != Some(class.as_str());
            if changed || member.is_some() {
                target.member = member;
            }
            target.class = Some(class);
            state.reveal_member = true;
            if to_members {
                state.focus_members = true;
                if state.selection(scope).member.is_none()
                    && let Some(first) = state.visible_members(scope).first()
                {
                    state.selection_mut(scope).member = Some(first.key.clone());
                }
            }
        }
    }
}

/// Middle column of the API tab: the selected class and its members.
pub(crate) fn api_class_column(
    ui: &mut Ui,
    state: &mut ApiBrowserState,
    scope: LanguageScope,
    keys: RefKeys,
    actions: &mut RefActions,
) {
    let p = neoflash_ui::palette(ui.ctx());
    let Some(class) = state.selected_class(scope) else {
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(
                space::s5() as i8,
                space::s4() as i8,
            ))
            .show(ui, |ui| {
                ui.label(
                    rich("Select a class in the tree.", TextStyle::Body).color(p.text_tertiary),
                );
            });
        return;
    };
    let members = state.visible_members(scope);
    // ↑↓ in the member list; ← returns to the tree.
    if state.focus_members {
        let at = state
            .selection(scope)
            .member
            .as_deref()
            .and_then(|key| members.iter().position(|m| m.key == key));
        let next = if keys.down {
            Some(at.map_or(0, |i| (i + 1).min(members.len().saturating_sub(1))))
        } else if keys.up {
            Some(at.map_or(0, |i| i.saturating_sub(1)))
        } else {
            None
        };
        if let Some(member) = next.and_then(|i| members.get(i)) {
            state.selection_mut(scope).member = Some(member.key.clone());
            state.reveal_member = true;
        }
        if keys.left {
            state.focus_members = false;
            state.reveal_tree = true;
        }
    }
    let selected_member = state.selection(scope).member.clone();
    let reveal = std::mem::take(&mut state.reveal_member);
    let mut navigate: Option<String> = None;
    let mut clicked: Option<String> = None;

    egui::ScrollArea::vertical()
        .id_salt(("reference-api-class", scope.html_slug(), &class.key))
        .auto_shrink([false, false])
        .themed()
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(space::s5() as i8, space::s4() as i8))
                .show(ui, |ui| {
                    let width = ui.available_width();
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing = vec2(space::s2(), neoflash_ui::metrics::row_gap());

                    // Name, status, Open in browser. The button shrinks to its icon before the
                    // name is cut.
                    let support = Support::of(&class.status);
                    let badge_text = match class.status.as_str() {
                        "members" => "Members only",
                        _ => support.map_or("", Support::label),
                    };
                    let badge_w = if badge_text.is_empty() {
                        0.0
                    } else {
                        text_width(ui, badge_text, TextStyle::Small) + 12.0 + space::s2()
                    };
                    let button_w =
                        text_width(ui, "Open in browser", TextStyle::Body) + 16.0 + 4.0 * space::s2();
                    let fixed = 16.0 + space::s2() + badge_w + space::s2();
                    let name_w = text_width(ui, &class.name, TextStyle::Heading);
                    let full_button = fixed + name_w + button_w <= width;
                    let name_max = width
                        - fixed
                        - if full_button {
                            button_w
                        } else {
                            neoflash_ui::metrics::control() + space::s2()
                        };
                    ui.horizontal(|ui| {
                        ui.set_min_height(neoflash_ui::metrics::control());
                        icons::show(
                            ui,
                            if class.insertion.is_some() {
                                i16::CLASS
                            } else {
                                i16::PACKAGE
                            },
                            p.accent_default,
                        );
                        let name = fit_text_to_width(ui, &class.name, TextStyle::Heading, name_max);
                        let heading =
                            ui.label(rich(&name, TextStyle::Heading).color(p.text_primary));
                        if name != class.name {
                            c::tooltip(heading, &class.name, None);
                        }
                        match (support, class.status.as_str()) {
                            (Some(support), _) => support_badge(ui, support),
                            (None, "members") => {
                                c::tooltip(
                                    ui.add(Badge::new(badge_text, BadgeType::Neutral)),
                                    "The class declaration has no status of its own; the members listed here do",
                                    None,
                                );
                            }
                            _ => {}
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let open = if full_button {
                                ui.add(Button::ghost("Open in browser").icon(i16::EXTERNAL))
                            } else {
                                ui.add(c::IconButton::new(i16::EXTERNAL, "Open in browser"))
                            };
                            actions.open_in_browser |= open.clicked();
                        });
                    });

                    // Package and superclass chain; indexed parents open on click.
                    // Wrapped rows are as high as the text, not as a control.
                    let control_h = ui.spacing().interact_size.y;
                    ui.spacing_mut().interact_size.y = TextStyle::MonoSmall.spec().line_height;
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(space::s1(), 2.0);
                        let small = |ui: &mut Ui, text: &str| {
                            ui.label(rich(text, TextStyle::MonoSmall).color(p.text_tertiary));
                        };
                        small(ui, &class.package);
                        if !class.chain.is_empty() {
                            small(ui, " · ");
                            small(ui, &class.name);
                        }
                        for (name, key) in &class.chain {
                            small(ui, "→");
                            match key {
                                Some(key) => {
                                    let link = c::tooltip(
                                        mono_link(ui, name, p.text_tertiary),
                                        &format!("Open {key}"),
                                        None,
                                    );
                                    if link.clicked() {
                                        navigate = Some(key.clone());
                                    }
                                }
                                None => small(ui, name),
                            }
                        }
                    });
                    ui.spacing_mut().interact_size.y = control_h;
                    if !class.description.is_empty() {
                        ui.add(
                            egui::Label::new(
                                rich(&class.description, TextStyle::DialogBody)
                                    .color(p.text_secondary),
                            )
                            .wrap(),
                        );
                    }

                    // Filter row; the counts move to their own line when the row is full.
                    ui.add_space(neoflash_ui::metrics::row_gap());
                    let (working, partial, missing) = class.support_counts();
                    let total = class.members.len();
                    let noun = if api_index(scope).flat { "entries" } else { "members" };
                    let counts = if working + partial + missing == 0 {
                        format!("{total} {noun}")
                    } else {
                        format!(
                            "{total} {noun} · {working} working · {partial} partial · {missing} missing"
                        )
                    };
                    let counts_w = text_width(ui, &counts, TextStyle::Small);
                    let mut counts_inline = false;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::s3();
                        c::Segmented::show(
                            ui,
                            &mut state.filter,
                            &[
                                (MemberFilter::All, "All"),
                                (MemberFilter::Properties, "Properties"),
                                (MemberFilter::Methods, "Methods"),
                                (MemberFilter::Events, "Events"),
                            ],
                            false,
                        );
                        if working + partial + missing > 0 {
                            ui.add(c::Checkbox::new(&mut state.hide_missing, "Hide missing"));
                        }
                        if ui.available_width() >= counts_w + space::s3() {
                            counts_inline = true;
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(rich(&counts, TextStyle::Small).color(p.text_tertiary));
                                },
                            );
                        }
                    });
                    if !counts_inline {
                        ui.label(rich(&counts, TextStyle::Small).color(p.text_tertiary));
                    }

                    // One row per member, grouped.
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let mut any = false;
                    for group in MemberGroup::ALL {
                        let rows: Vec<&&ApiMember> =
                            members.iter().filter(|m| m.group == group).collect();
                        if rows.is_empty() {
                            continue;
                        }
                        any = true;
                        group_caption(ui, group.label());
                        for member in rows {
                            let selected = selected_member.as_deref() == Some(member.key.as_str());
                            let response = RefRow {
                                style: TextStyle::Mono,
                                dot: Some(member.support),
                                trailing: &member.row_signature,
                                selected,
                                dimmed: member.support == Some(Support::Missing),
                                ..RefRow::new(&member.label)
                            }
                            .show(ui);
                            if response.clicked() {
                                clicked = Some(member.key.clone());
                            }
                            if selected && reveal {
                                ui.scroll_to_rect(
                                    response.rect.expand(neoflash_ui::metrics::row()),
                                    None,
                                );
                            }
                        }
                    }
                    if !any {
                        ui.add_space(space::s3());
                        ui.label(
                            rich(
                                "No members match the filter.",
                                TextStyle::Body,
                            )
                            .color(p.text_tertiary),
                        );
                    }
                });
        });
    if let Some(key) = clicked {
        state.selection_mut(scope).member = Some(key);
        state.focus_members = true;
    }
    if let Some(key) = navigate {
        state.select(scope, &key, None);
    }
}

/// Detail column of the API tab: the selected member, or a summary of the class.
pub(crate) fn api_detail_column(
    ui: &mut Ui,
    state: &mut ApiBrowserState,
    scope: LanguageScope,
    actions: &mut RefActions,
) -> Option<egui::scroll_area::ScrollAreaOutput<()>> {
    let p = neoflash_ui::palette(ui.ctx());
    let class = state.selected_class(scope)?;
    let member = state.selected_member(scope);
    let mut navigate: Option<ApiLink> = None;
    let output = egui::ScrollArea::vertical()
        .id_salt((
            "reference-api-detail",
            scope.html_slug(),
            &class.key,
            member.map(|m| &m.key),
        ))
        .auto_shrink([false, false])
        .themed()
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::same(space::s4() as i8))
                .show(ui, |ui| {
                    let width = ui.available_width();
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing =
                        vec2(space::s2(), neoflash_ui::metrics::row_gap());
                    let Some(member) = member else {
                        class_summary(ui, class, actions);
                        return;
                    };
                    // Name, status, kind.
                    ui.horizontal_wrapped(|ui| {
                        ui.add(
                            egui::Label::new(
                                rich(&member.label, TextStyle::Title).color(p.text_primary),
                            )
                            .wrap(),
                        );
                        if let Some(support) = member.support {
                            support_badge(ui, support);
                        }
                        ui.label(rich(&member.kind, TextStyle::Small).color(p.text_tertiary));
                    });
                    code_box(ui, &member.declaration);
                    if !member.description.is_empty() {
                        ui.add(
                            egui::Label::new(
                                rich(&member.description, TextStyle::DialogBody)
                                    .color(p.text_secondary),
                            )
                            .wrap(),
                        );
                    }
                    if !member.params.is_empty() {
                        group_caption(ui, "Parameters");
                        let name_w = neoflash_ui::metrics::field_s().min(width * 0.4);
                        for (name, text) in &member.params {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = space::s2();
                                let shown = fit_text_to_width(ui, name, TextStyle::Mono, name_w);
                                let (rect, _) = ui.allocate_exact_size(
                                    vec2(name_w, TextStyle::Body.spec().line_height),
                                    egui::Sense::hover(),
                                );
                                ui.painter().text(
                                    rect.left_center(),
                                    egui::Align2::LEFT_CENTER,
                                    shown,
                                    font(TextStyle::Mono),
                                    p.text_primary,
                                );
                                let rest = ui.available_width().max(24.0);
                                ui.vertical(|ui| {
                                    ui.set_max_width(rest);
                                    ui.add(
                                        egui::Label::new(
                                            rich(text, TextStyle::Body).color(p.text_secondary),
                                        )
                                        .wrap(),
                                    );
                                });
                            });
                        }
                    }
                    if !member.returns.is_empty() {
                        group_caption(ui, "Returns");
                        ui.label(rich(&member.returns, TextStyle::Mono).color(p.text_primary));
                    }
                    group_caption(ui, "In NeoFlash");
                    match member.support {
                        Some(support) => support_message(ui, support, &member.note),
                        None if !member.note.is_empty() => {
                            ui.add(c::InlineMessage::info(&member.note));
                        }
                        None => {
                            ui.add(c::InlineMessage::info(
                                "The progress catalogue has no status for this declaration.",
                            ));
                        }
                    }
                    group_caption(ui, "Example");
                    code_box(ui, &member.example);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = space::s2();
                        if ui.add(Button::new("Copy").icon(i16::COPY)).clicked() {
                            ui.ctx().copy_text(member.example.clone());
                        }
                        if insert_button(ui, true) {
                            actions.insert = Some(member.insertion.clone());
                        }
                    });
                    if !member.see_also.is_empty() {
                        group_caption(ui, "See also");
                        ui.spacing_mut().interact_size.y = TextStyle::MonoSmall.spec().line_height;
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(space::s2(), space::s1());
                            for (i, link) in member.see_also.iter().enumerate() {
                                if i > 0 {
                                    ui.label(
                                        rich("·", TextStyle::MonoSmall).color(p.text_tertiary),
                                    );
                                }
                                if mono_link(ui, &link.label, p.accent_default).clicked() {
                                    navigate = Some(link.clone());
                                }
                            }
                        });
                    }
                });
        });
    if let Some(link) = navigate {
        state.select(scope, &link.class_key, link.member_key.as_deref());
    }
    Some(output)
}

/// Detail column while no member is selected.
fn class_summary(ui: &mut Ui, class: &ApiClass, actions: &mut RefActions) {
    let p = neoflash_ui::palette(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.add(egui::Label::new(rich(&class.name, TextStyle::Title).color(p.text_primary)).wrap());
        if let Some(support) = Support::of(&class.status) {
            support_badge(ui, support);
        }
    });
    if !class.description.is_empty() {
        ui.add(
            egui::Label::new(
                rich(&class.description, TextStyle::DialogBody).color(p.text_secondary),
            )
            .wrap(),
        );
    }
    ui.add(
        egui::Label::new(
            rich(
                "Select a member to see its signature, parameters, support in NeoFlash and an example.",
                TextStyle::Body,
            )
            .color(p.text_tertiary),
        )
        .wrap(),
    );
    if let Some(support) = Support::of(&class.status) {
        group_caption(ui, "In NeoFlash");
        support_message(ui, support, "");
    }
    if let Some(insertion) = &class.insertion {
        ui.add_space(space::s3());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::s2();
            if ui.add(Button::new("Copy").icon(i16::COPY)).clicked() {
                ui.ctx().copy_text(class.name.clone());
            }
            if insert_button(ui, true) {
                actions.insert = Some(insertion.clone());
            }
        });
    }
}

/// What ⌘↵ inserts on the API tab: the selected member, else the selected class.
pub(crate) fn api_selected_insertion(
    state: &ApiBrowserState,
    scope: LanguageScope,
) -> Option<CodeCommand> {
    match state.selected_member(scope) {
        Some(member) => Some(member.insertion.clone()),
        None => state.selected_class(scope)?.insertion.clone(),
    }
}

/// Topics that match the search field, listed above the article: title, signature, summary and
/// the sample of each curated topic and `<ESC>` chord of the selected language.
pub(crate) fn render_topic_hits(
    ui: &mut Ui,
    query: &str,
    scope: Option<LanguageScope>,
    project_lang: ScriptLanguage,
    nav_target: &mut Option<usize>,
) {
    let p = neoflash_ui::palette(ui.ctx());
    // Help (`None`) lists only its own authoring topics, never a scripting language.
    let (mut hits, _) =
        search_reference_hits(query, scope.unwrap_or(LanguageScope::All), project_lang);
    if scope.is_none() {
        hits.retain(|hit| hit.dialect_badge == "[Authoring]");
    }
    let heading = format!("Topics matching “{}”", query.trim());
    ui.add(egui::Label::new(rich(&heading, TextStyle::Title).color(p.text_primary)).wrap());
    if hits.is_empty() {
        ui.add(
            egui::Label::new(
                rich(
                    "No topic matches. The chapter list on the left shows every chapter that mentions it.",
                    TextStyle::Body,
                )
                .color(p.text_tertiary),
            )
            .wrap(),
        );
    }
    for hit in hits.iter().take(12) {
        ui.add_space(space::s3());
        ui.add(
            egui::Label::new(rich(&hit.title, TextStyle::BodyStrong).color(p.text_primary)).wrap(),
        );
        ui.add(
            egui::Label::new(rich(&hit.signature, TextStyle::MonoSmall).color(p.text_tertiary))
                .wrap(),
        );
        ui.add(
            egui::Label::new(rich(&hit.summary, TextStyle::Body).color(p.text_secondary)).wrap(),
        );
        if let Some(snippet) = hit.code_preview {
            let preview: String = snippet.lines().take(8).collect::<Vec<_>>().join("\n");
            code_box(ui, &preview);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::s2();
            if let Some(snippet) = hit.code_preview {
                if ui.add(Button::new("Copy").icon(i16::COPY)).clicked() {
                    ui.ctx().copy_text(snippet.to_owned());
                }
                if insert_button(ui, false) {
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(article_insert_id(), snippet.to_owned()));
                }
            }
            if let Some(chapter) = HELP_CHAPTERS.get(hit.chapter_idx) {
                let label = format!("Open “{}”", plain_chapter_label(chapter.sidebar_label));
                if ui.add(Button::ghost(&label)).clicked() {
                    *nav_target = Some(hit.chapter_idx);
                }
            }
        });
    }
    ui.add_space(space::s4());
    c::divider(ui, false);
    ui.add_space(space::s4());
}
