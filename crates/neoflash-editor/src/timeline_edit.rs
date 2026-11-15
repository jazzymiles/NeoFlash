//! Transient frame selection and pointer gestures. Authored edits live in the core.
use crate::app::{Editor, ScriptTarget};
use crate::timeline_paint::{frame_at_x, timeline_cell_at};
use eframe::egui::{self, Pos2, Rect, Vec2};
use neoflash_core::{
    ElementKind, FrameClipboard, FrameRange, Keyframe, Layer, LayerProperties, LayerRole, Symbol,
    Timeline, TimelineEdit, TweenKind, copy_frames, edit_frames,
};

#[derive(Default)]
pub(crate) struct TimelineSelection {
    pub focused: bool,
    pub include_scripts: bool,
    /// Edit multiple frames: the stage shows every keyframe in the onion range and
    /// moves apply to the selected objects on all of them.
    pub edit_multiple: bool,
    /// Control ▸ Play all scenes: editor playback continues into the next scene.
    pub play_all_scenes: bool,
    pub range: Option<FrameRange>,
    symbol: String,
    anchor: Option<(String, u32)>,
    clipboard: Option<FrameClipboard>,
    clipboard_marker: Option<String>,
    gesture: Option<Gesture>,
    viewport: Option<egui::ViewportId>,
}

#[derive(Clone)]
enum Gesture {
    Select {
        anchor_row: usize,
        anchor_frame: u32,
        press_row: usize,
        press_frame: u32,
        moved: bool,
    },
    Move {
        range: FrameRange,
        row: usize,
        frame: u32,
        target_row: usize,
        target_frame: u32,
    },
    Key {
        layer: usize,
        frame: u32,
    },
    End {
        layer: String,
        end: u32,
    },
}

/// Frame-range commands shared by Edit ▸ Timeline, the frame context menu and their shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameCommand {
    Cut,
    Copy,
    Paste,
    Clear,
    Remove,
    SelectAll,
}

impl TimelineSelection {
    pub fn dragging(&self) -> bool {
        self.gesture.is_some()
    }

    pub(crate) fn cancel_gesture(&mut self) {
        self.gesture = None;
    }
}

#[derive(Clone)]
pub(crate) struct SharedFrameClipboard {
    pub(crate) clip: FrameClipboard,
    pub(crate) marker: Option<String>,
    pub(crate) source_session: String,
    pub(crate) donor_symbols: Vec<Symbol>,
    pub(crate) donor_scene_ids: std::collections::HashSet<String>,
    pub(crate) donor_resources: Vec<neoflash_core::ResourceUnit>,
    pub(crate) donor_sources: Vec<neoflash_core::SourceUnit>,
    pub(crate) layer_groups: Vec<Vec<neoflash_core::AuthoringGroup>>,
}

impl Editor {
    pub(crate) fn selected_frame_range(&self) -> Option<(u32, u32, usize)> {
        if !self.timeline_edit.focused {
            return None;
        }
        let range = self.frame_range()?;
        Some((range.start, range.end, range.layers.len()))
    }

    pub(crate) fn frame_range(&self) -> Option<FrameRange> {
        (self.timeline_edit.symbol == self.symbol_id()).then_some(())?;
        let mut range = self.timeline_edit.range.clone()?;
        range
            .layers
            .retain(|id| self.symbol().timeline.layers.iter().any(|l| &l.id == id));
        if range.layers.is_empty() {
            return None;
        }
        let max_frame = if self.symbol().kind == neoflash_core::SymbolKind::Button {
            self.symbol().timeline.frame_count.saturating_sub(1)
        } else {
            999_999
        };
        range.end = range.end.min(max_frame);
        range.start = range.start.min(range.end);
        Some(range)
    }

    pub(crate) fn select_frame_range(&mut self, range: FrameRange) {
        self.timeline_edit.symbol = self.symbol_id().to_owned();
        self.timeline_edit.range = Some(range);
        self.timeline_edit.focused = true;
    }

    /// Drop the transient frame target when the user explicitly activates another layer row.
    /// Keeping the old range here is dangerous: frame commands prefer it over the playhead and
    /// would otherwise edit the former layer even though the new layer is visibly active.
    pub(crate) fn clear_timeline_frame_selection(&mut self) {
        self.timeline_edit.focused = false;
        self.timeline_edit.range = None;
        self.timeline_edit.anchor = None;
        self.timeline_edit.gesture = None;
        self.timeline_edit.viewport = None;
        self.timeline_key_drag = None;
    }

    fn frame_rectangle(
        &self,
        rows: &[usize],
        a: usize,
        b: usize,
        start: u32,
        end: u32,
    ) -> FrameRange {
        FrameRange {
            layers: rows[a.min(b)..=a.max(b)]
                .iter()
                .filter_map(|&i| {
                    let layer = &self.symbol().timeline.layers[i];
                    (layer.properties.role != LayerRole::Folder).then(|| layer.id.clone())
                })
                .collect(),
            start: start.min(end),
            end: start.max(end),
        }
    }

    fn apply_timeline_edit_to_symbol(
        &self,
        symbol_id: &str,
        candidate_symbol: &mut Symbol,
        edit: TimelineEdit,
    ) -> Result<(), String> {
        let mut referenced_resources = std::collections::HashSet::<String>::new();
        let mut collect_key_resources = |key: &Keyframe| {
            for bf in &key.bitmap_fills {
                referenced_resources.insert(bf.resource_id.clone());
            }
            if let Some(sound) = &key.sound {
                referenced_resources.insert(sound.resource.clone());
            }
            if let Some(video) = &key.video
                && !video.resource.is_empty()
            {
                referenced_resources.insert(video.resource.clone());
            }
            for el in &key.elements {
                if let ElementKind::Bitmap { resource_id, .. } = &el.kind {
                    referenced_resources.insert(resource_id.clone());
                }
            }
        };
        for layer in &candidate_symbol.timeline.layers {
            for key in &layer.keyframes {
                collect_key_resources(key);
            }
        }
        let mut clip_element_ids = std::collections::HashSet::<String>::new();
        if let TimelineEdit::Paste { clip, .. } = &edit {
            for layer in &clip.layers {
                for key in &layer.keyframes {
                    collect_key_resources(key);
                    for el in &key.elements {
                        clip_element_ids.insert(el.id.clone());
                    }
                }
            }
        }
        let has_mask_layer = candidate_symbol
            .timeline
            .layers
            .iter()
            .any(|l| l.properties.role == LayerRole::Mask);
        let mut carrier = neoflash_core::Project::blank();
        carrier.format_version = self.project.format_version.max(12);
        carrier.width = self.project.width;
        carrier.height = self.project.height;
        carrier.fps = self.project.fps;
        carrier.root = symbol_id.to_owned();
        if !referenced_resources.is_empty() {
            for s in &self.project.symbols {
                if s.id == symbol_id {
                    continue;
                }
                for layer in &s.timeline.layers {
                    for key in &layer.keyframes {
                        if let Some(video) = &key.video
                            && referenced_resources.contains(video.resource.as_str())
                        {
                            referenced_resources.insert(video.resource.clone());
                        }
                    }
                }
            }
            carrier.resources = self
                .project
                .resources
                .iter()
                .filter(|r| referenced_resources.contains(r.id.as_str()))
                .cloned()
                .collect();
        }
        let mut symbols = Vec::with_capacity(self.project.symbols.len());
        let empty_timeline = || Timeline {
            frame_count: 1,
            layers: Vec::new(),
            scripts: Vec::new(),
            guides: Vec::new(),
        };
        symbols.push(Symbol {
            id: candidate_symbol.id.clone(),
            name: candidate_symbol.name.clone(),
            kind: candidate_symbol.kind,
            folder: String::new(),
            linkage_id: None,
            class_name: None,
            timeline: std::mem::replace(&mut candidate_symbol.timeline, empty_timeline()),
        });
        for s in &self.project.symbols {
            if s.id == symbol_id {
                continue;
            }
            let mut stub_layers = Vec::new();
            if has_mask_layer {
                stub_layers = s.timeline.layers.clone();
                for l in &mut stub_layers {
                    for k in &mut l.keyframes {
                        for el in &mut k.elements {
                            if let ElementKind::Bitmap { resource_id, .. } = &el.kind
                                && !carrier.resources.iter().any(|r| &r.id == resource_id)
                                && let Some(res) =
                                    self.project.resources.iter().find(|r| &r.id == resource_id)
                            {
                                carrier.resources.push(res.clone());
                            }
                        }
                    }
                }
            } else if !clip_element_ids.is_empty() {
                let mut colliding_elements = Vec::new();
                for l in &s.timeline.layers {
                    for k in &l.keyframes {
                        for el in &k.elements {
                            if clip_element_ids.contains(el.id.as_str())
                                || matches!(&el.kind, ElementKind::Instance { .. })
                            {
                                colliding_elements.push(neoflash_core::Element {
                                    id: el.id.clone(),
                                    kind: match &el.kind {
                                        ElementKind::Instance {
                                            symbol_id,
                                            playback,
                                        } => ElementKind::Instance {
                                            symbol_id: symbol_id.clone(),
                                            playback: *playback,
                                        },
                                        _ => ElementKind::Rectangle {
                                            width: 1.0,
                                            height: 1.0,
                                            fill: [0, 0, 0, 255],
                                            stroke: [0, 0, 0, 0],
                                            stroke_width: 0.0,
                                        },
                                    },
                                    ..neoflash_core::Element::default()
                                });
                            }
                        }
                    }
                }
                if !colliding_elements.is_empty() {
                    stub_layers.push(Layer {
                        id: format!("stub-{}", s.id),
                        name: "Stub".into(),
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: colliding_elements,
                            ..Keyframe::default()
                        }],
                        properties: LayerProperties::default(),
                    });
                }
            }
            symbols.push(Symbol {
                id: s.id.clone(),
                name: s.name.clone(),
                kind: s.kind,
                folder: String::new(),
                linkage_id: None,
                class_name: None,
                timeline: Timeline {
                    frame_count: s.timeline.frame_count.max(1),
                    layers: stub_layers,
                    scripts: Vec::new(),
                    guides: Vec::new(),
                },
            });
        }
        carrier.symbols = symbols;
        edit_frames(&mut carrier, symbol_id, edit)?;
        if let Some(edited) = carrier.symbol_mut(symbol_id) {
            candidate_symbol.timeline = std::mem::replace(&mut edited.timeline, empty_timeline());
        }
        for layer in &mut candidate_symbol.timeline.layers {
            layer.normalize_motion_keys();
        }
        validate_symbol_timeline_invariants(candidate_symbol)
    }

    pub(crate) fn apply_frame_edit(&mut self, edit: TimelineEdit) -> bool {
        self.apply_frame_edit_named("Edit", edit)
    }

    pub(crate) fn apply_frame_edit_named(
        &mut self,
        label: &'static str,
        edit: TimelineEdit,
    ) -> bool {
        if !self.has_document() {
            return false;
        }
        self.finish_edits();
        let old_count = self.symbol().timeline.frame_count;
        let symbol_id = self.symbol_id().to_owned();
        let mut candidate_symbol = self.symbol().clone();
        match self.apply_timeline_edit_to_symbol(&symbol_id, &mut candidate_symbol, edit) {
            Ok(()) => {
                self.transact_named(label, |editor| {
                    if let Some(sym) = editor.project.symbol_mut(&symbol_id) {
                        *sym = candidate_symbol;
                    }
                    editor.project.format_version = editor.project.format_version.max(5);
                    editor.project.upgrade_format_for_features();
                    editor.project.prune_authoring_groups();
                });
                if self.symbol().timeline.frame_count < old_count {
                    let last = self.symbol().timeline.frame_count.saturating_sub(1);
                    self.frame = self.frame.min(last);
                    if let Some(range) = &mut self.timeline_edit.range {
                        range.end = range.end.min(last);
                        range.start = range.start.min(range.end);
                    }
                }
                self.reset_authored_preview();
                self.normalize_selection();
                self.clear_selection();
                true
            }
            Err(error) => {
                self.message(error);
                false
            }
        }
    }

    pub(crate) fn copy_frame_range(&mut self, cut: bool) -> bool {
        if !self.has_document() {
            return false;
        }
        let Some(range) = self.frame_range() else {
            return false;
        };
        let include_scripts = self.timeline_edit.include_scripts;
        match copy_frames(&self.project, self.symbol_id(), &range, include_scripts) {
            Ok(clip) => {
                let shared = build_shared_frame_clipboard(
                    &self.project,
                    self.symbol_id(),
                    &clip,
                    &self.clipboard.session,
                );
                if !cut
                    || self.apply_frame_edit_named(
                        "Cut frames",
                        TimelineEdit::Clear {
                            range,
                            include_scripts,
                        },
                    )
                {
                    self.message(format!(
                        "{} {} frames on {} layers",
                        if cut { "Cut" } else { "Copied" },
                        clip.frame_count,
                        clip.layers.len()
                    ));
                    self.timeline_edit.clipboard = Some(clip);
                    self.timeline_edit.clipboard_marker = None;
                    self.shared_frame_clipboard = Some(shared);
                    true
                } else {
                    false
                }
            }
            Err(error) => {
                self.message(error);
                false
            }
        }
    }

    pub(crate) fn copy_frames_to_system_clipboard(&mut self, ctx: &egui::Context, cut: bool) {
        if self.copy_frame_range(cut) {
            // Native Paste emits a payload only when the system clipboard has
            // text. SharedFrameClipboard keeps the frame data live across open tabs.
            let marker = format!(
                "NeoFlash Timeline frames: {}:{}",
                self.clipboard.session,
                neoflash_core::new_id("frames")
            );
            self.timeline_edit.clipboard_marker = Some(marker.clone());
            if let Some(shared) = &mut self.shared_frame_clipboard {
                shared.marker = Some(marker.clone());
            }
            ctx.copy_text(marker);
        }
    }

    pub(crate) fn paste_frames_from_system_clipboard(&mut self, payload: Option<String>) {
        if !self.has_document() {
            return;
        }
        let text = match payload.map(Ok).unwrap_or_else(|| {
            arboard::Clipboard::new()
                .and_then(|mut clipboard| clipboard.get_text())
                .map_err(|error| error.to_string())
        }) {
            Ok(text) => text,
            Err(error) => {
                self.message(format!("Cannot read clipboard: {error}"));
                return;
            }
        };
        let local_matches = self.timeline_edit.clipboard_marker.as_deref() == Some(text.as_str());
        let shared_matches = self
            .shared_frame_clipboard
            .as_ref()
            .and_then(|s| s.marker.as_deref())
            == Some(text.as_str());
        if !local_matches && !shared_matches {
            self.timeline_edit.clipboard = None;
            self.timeline_edit.clipboard_marker = None;
            self.shared_frame_clipboard = None;
            self.message("The clipboard no longer contains Timeline frames. Copy frames in the Timeline again.");
            return;
        }
        self.paste_frame_range();
    }

    pub(crate) fn paste_frame_range(&mut self) {
        if !self.has_document() {
            return;
        }
        let (mut clip, shared_opt) = if let Some(local_clip) = self.timeline_edit.clipboard.clone()
        {
            let matching_shared = self
                .shared_frame_clipboard
                .clone()
                .filter(|s| s.source_session == self.clipboard.session);
            (local_clip, matching_shared)
        } else if let Some(shared) = self.shared_frame_clipboard.clone() {
            (shared.clip.clone(), Some(shared))
        } else {
            self.message("Copy frames in the Timeline before pasting them.");
            return;
        };
        let Some(range) = self.frame_range() else {
            return;
        };
        let timeline = &self.symbol().timeline;
        let rows = crate::layer_controls::visible_layer_indices(timeline);
        let Some(first) = rows
            .iter()
            .position(|&index| range.layers.first() == Some(&timeline.layers[index].id))
        else {
            return;
        };
        let mut layers: Vec<_> = rows[first..]
            .iter()
            .map(|&index| &timeline.layers[index])
            .filter(|l| l.properties.role != LayerRole::Folder)
            .take(clip.layers.len())
            .map(|l| l.id.clone())
            .collect();
        let missing = clip.layers.len().saturating_sub(layers.len());
        let end = range
            .start
            .saturating_add(clip.frame_count.saturating_sub(1));
        let symbol_id = self.symbol_id().to_owned();
        if self.project.symbol(&symbol_id).is_none() {
            return;
        }

        let cross_document = shared_opt
            .as_ref()
            .is_some_and(|s| s.source_session != self.clipboard.session);
        let include_scripts = self.timeline_edit.include_scripts;
        let mut candidate_project = self.project.clone();

        if missing > 0 {
            let mut occupied_layer_ids: std::collections::HashSet<String> = candidate_project
                .symbols
                .iter()
                .flat_map(|s| {
                    std::iter::once(s.id.clone())
                        .chain(s.timeline.layers.iter().map(|l| l.id.clone()))
                })
                .collect();
            if let Some(cand_sym) = candidate_project.symbol_mut(&symbol_id) {
                let timeline = &mut cand_sym.timeline;
                let parent = range
                    .layers
                    .first()
                    .and_then(|first_id| timeline.layers.iter().find(|l| &l.id == first_id))
                    .and_then(|l| l.properties.parent.clone());
                for _ in 0..missing {
                    let mut idx = timeline.layers.len() + 1;
                    let id = loop {
                        let cand_id = format!("layer-{idx}");
                        if occupied_layer_ids.insert(cand_id.clone()) {
                            break cand_id;
                        }
                        idx += 1;
                    };
                    let name = format!("Layer {idx}");
                    let new_layer = Layer {
                        id: id.clone(),
                        name,
                        visible: true,
                        locked: false,
                        keyframes: vec![Keyframe::default()],
                        properties: LayerProperties {
                            parent: parent.clone(),
                            ..Default::default()
                        },
                    };
                    // Display order goes top-to-bottom (`layers.iter().enumerate().rev()`), so
                    // inserting at index 0 places the new destination layer below existing rows.
                    timeline.layers.insert(0, new_layer);
                    layers.push(id);
                }
            }
        }

        let imported_maps = if cross_document {
            let shared = shared_opt
                .as_ref()
                .expect("cross-document shared clipboard");
            let mut seeds = crate::clipboard::SeedAssetRefs::default();
            if include_scripts && !clip.scripts.is_empty() {
                seeds.needs_sources = true;
            }
            for layer_clip in &clip.layers {
                for kf in &layer_clip.keyframes {
                    seeds.record_keyframe(kf);
                }
            }
            let donor_scenes: std::collections::HashSet<&str> =
                shared.donor_scene_ids.iter().map(String::as_str).collect();
            match crate::clipboard::import_cross_document_assets(
                &mut candidate_project,
                &shared.donor_symbols,
                &donor_scenes,
                &shared.donor_resources,
                &shared.donor_sources,
                seeds,
            ) {
                Ok(maps) => Some(maps),
                Err(err) => {
                    self.message(format!("Timeline edit failed: {err}"));
                    return;
                }
            }
        } else {
            None
        };

        // Pre-remap colliding (or cross-tab) element IDs and IK armature IDs across all keyframe
        // side-tables (`bitmap_fills`, `appearances`, `asset_warps`, `instance_overrides`,
        // `element_parents`, `controller_elements`, `ik_armatures`, `extended_accessibility`,
        // `vector_effects`, `motion_tracks`, `motion_keys`) so `edit_frames` never renames bare
        // `elements` without their side-tables.
        let mut occupied_ids = imported_maps
            .as_ref()
            .map(|m| m.occupied_ids.clone())
            .unwrap_or_else(|| crate::clipboard::collect_occupied_project_ids(&candidate_project));
        let mut elem_owners = std::collections::HashMap::<String, (String, String)>::new();
        let mut ik_owners = std::collections::HashMap::<String, (String, String)>::new();
        for s in &candidate_project.symbols {
            for l in &s.timeline.layers {
                for kf in &l.keyframes {
                    for arm in &kf.ik_armatures {
                        ik_owners.insert(arm.id.clone(), (s.id.clone(), l.id.clone()));
                    }
                    for el in &kf.elements {
                        elem_owners.insert(el.id.clone(), (s.id.clone(), l.id.clone()));
                    }
                }
            }
        }

        let mut pasted_layer_groups: Vec<(String, Vec<(usize, neoflash_core::AuthoringGroup)>)> =
            Vec::new();
        for (row_idx, (source_clip, dest_layer_id)) in
            clip.layers.iter_mut().zip(&layers).enumerate()
        {
            let mut elem_map = std::collections::HashMap::<String, String>::new();
            for kf in &source_clip.keyframes {
                for el in &kf.elements {
                    if elem_map.contains_key(&el.id) {
                        continue;
                    }
                    let collides = cross_document
                        || elem_owners
                            .get(&el.id)
                            .is_some_and(|(os, ol)| os != &symbol_id || ol != dest_layer_id);
                    if collides {
                        let fresh = crate::clipboard::alloc_unique_id("object", &mut occupied_ids);
                        elem_owners
                            .insert(fresh.clone(), (symbol_id.clone(), dest_layer_id.clone()));
                        elem_map.insert(el.id.clone(), fresh);
                    } else {
                        elem_owners
                            .insert(el.id.clone(), (symbol_id.clone(), dest_layer_id.clone()));
                    }
                }
            }
            for kf in &mut source_clip.keyframes {
                kf.remap_all_ids(
                    &elem_map,
                    imported_maps.as_ref().map(|m| &m.sym_map),
                    imported_maps.as_ref().map(|m| &m.res_map),
                );
                for fx in &mut kf.vector_effects {
                    if let Some(blend) = &mut fx.vector_blend
                        && let Some(tid) = elem_map.get(&blend.target_element_id)
                    {
                        blend.target_element_id = tid.clone();
                    }
                }
                for arm in &mut kf.ik_armatures {
                    let ik_collides = cross_document
                        || ik_owners
                            .get(&arm.id)
                            .is_some_and(|(os, ol)| os != &symbol_id || ol != dest_layer_id);
                    if ik_collides {
                        let fresh_ik = crate::clipboard::alloc_unique_id("ik", &mut occupied_ids);
                        ik_owners
                            .insert(fresh_ik.clone(), (symbol_id.clone(), dest_layer_id.clone()));
                        arm.id = fresh_ik;
                    } else {
                        ik_owners
                            .insert(arm.id.clone(), (symbol_id.clone(), dest_layer_id.clone()));
                    }
                }
                if let Some(maps) = &imported_maps {
                    for app in &mut kf.appearances {
                        crate::clipboard::remap_appearance_refs(
                            app,
                            &maps.swatch_map,
                            &maps.style_map,
                        );
                    }
                }
            }
            if let Some(shared) = &shared_opt
                && let Some(src_groups) = shared.layer_groups.get(row_idx)
                && !src_groups.is_empty()
            {
                let present_ids: std::collections::HashSet<&str> = source_clip
                    .keyframes
                    .iter()
                    .flat_map(|kf| kf.elements.iter().map(|e| e.id.as_str()))
                    .collect();
                let mut remapped_groups = Vec::new();
                for grp in src_groups {
                    let orig_len = grp.members.len();
                    let mut seen_members = std::collections::HashSet::new();
                    let members: Vec<String> = grp
                        .members
                        .iter()
                        .map(|m| elem_map.get(m).cloned().unwrap_or_else(|| m.clone()))
                        .filter(|m| {
                            present_ids.contains(m.as_str()) && seen_members.insert(m.clone())
                        })
                        .collect();
                    if !members.is_empty() && !(orig_len > 1 && members.len() == 1) {
                        remapped_groups.push((
                            orig_len,
                            neoflash_core::AuthoringGroup {
                                id: crate::clipboard::alloc_unique_id("group", &mut occupied_ids),
                                name: grp.name.clone(),
                                members,
                            },
                        ));
                    }
                }
                if !remapped_groups.is_empty() {
                    pasted_layer_groups.push((dest_layer_id.clone(), remapped_groups));
                }
            }
        }

        if let Err(err) = edit_frames(
            &mut candidate_project,
            &symbol_id,
            TimelineEdit::Paste {
                clip,
                layers: layers.clone(),
                at: range.start,
                include_scripts,
            },
        ) {
            self.message(format!("Timeline edit failed: {err}"));
            return;
        }

        // Prune any existing layer groups invalidated by overwritten frames before merging pasted groups.
        candidate_project.prune_authoring_groups();
        if let Some(cand_sym) = candidate_project.symbol_mut(&symbol_id) {
            for (dest_lid, groups) in pasted_layer_groups {
                if let Some(dest_layer) = cand_sym
                    .timeline
                    .layers
                    .iter_mut()
                    .find(|l| l.id == dest_lid)
                {
                    let present_on_layer: std::collections::HashSet<String> = dest_layer
                        .keyframes
                        .iter()
                        .flat_map(|k| k.elements.iter().map(|e| e.id.clone()))
                        .collect();
                    let mut already_grouped: std::collections::HashSet<String> = dest_layer
                        .properties
                        .groups
                        .iter()
                        .flat_map(|g| g.members.iter().cloned())
                        .collect();
                    for (orig_len, mut grp) in groups {
                        let filtered: Vec<String> = grp
                            .members
                            .into_iter()
                            .filter(|m| {
                                present_on_layer.contains(m) && !already_grouped.contains(m)
                            })
                            .collect();
                        if filtered.is_empty() || (orig_len > 1 && filtered.len() == 1) {
                            continue;
                        }
                        for m in &filtered {
                            already_grouped.insert(m.clone());
                        }
                        grp.members = filtered;
                        dest_layer.properties.groups.push(grp);
                    }
                }
            }
            for layer in &mut cand_sym.timeline.layers {
                layer.normalize_motion_keys();
            }
            if let Err(err) = validate_symbol_timeline_invariants(cand_sym) {
                self.message(format!("Timeline edit failed: {err}"));
                return;
            }
        }
        candidate_project.prune_authoring_groups();
        crate::timeline_commands::upgrade_all_format_versions(&mut candidate_project);
        if let Err(err) = candidate_project.validate() {
            self.message(format!("Timeline edit failed: {err}"));
            return;
        }

        self.finish_edits();
        self.set_playing(false);
        let selected_layers = layers.clone();
        self.transact_named("Paste frames", |editor| {
            editor.project = candidate_project;
            editor.layer = editor
                .layer
                .min(editor.symbol().timeline.layers.len().saturating_sub(1));
            editor.frame = editor
                .frame
                .min(editor.symbol().timeline.frame_count.saturating_sub(1));
            editor.normalize_selection();
        });
        self.reset_authored_preview();
        self.clear_selection();
        self.select_frame_range(FrameRange {
            layers: selected_layers,
            start: range.start,
            end,
        });
    }

    pub(crate) fn expand_with_linked_layers(&self, layers: &[String]) -> Vec<String> {
        let timeline = &self.symbol().timeline;
        let mut out: Vec<String> = Vec::with_capacity(layers.len() + 2);
        for id in layers {
            if !out.contains(id) {
                out.push(id.clone());
            }
        }
        let mut idx = 0;
        while idx < out.len() {
            let current = out[idx].clone();
            if let Some(layer) = timeline.layers.iter().find(|l| l.id == current)
                && let Some(partner_id) = &layer.properties.linked_layer
                && !out.contains(partner_id)
                && timeline.layers.iter().any(|p| {
                    &p.id == partner_id
                        && p.properties.role != LayerRole::Folder
                        && !neoflash_core::layer_effectively_locked(timeline, p)
                        && neoflash_core::layer_effectively_visible(timeline, p)
                })
            {
                out.push(partner_id.clone());
            }
            for partner in &timeline.layers {
                if partner.properties.linked_layer.as_deref() == Some(current.as_str())
                    && !out.contains(&partner.id)
                    && partner.properties.role != LayerRole::Folder
                    && !neoflash_core::layer_effectively_locked(timeline, partner)
                    && neoflash_core::layer_effectively_visible(timeline, partner)
                {
                    out.push(partner.id.clone());
                }
            }
            idx += 1;
        }
        out
    }

    pub fn split_clip_at(&mut self, layers: Vec<String>, at: u32) -> bool {
        self.finish_edits();
        let timeline = &self.symbol().timeline;
        let expanded: Vec<String> = self
            .expand_with_linked_layers(&layers)
            .into_iter()
            .filter(|id| {
                timeline.layers.iter().any(|l| {
                    &l.id == id
                        && l.properties.role != LayerRole::Folder
                        && !neoflash_core::layer_effectively_locked(timeline, l)
                        && neoflash_core::layer_effectively_visible(timeline, l)
                })
            })
            .collect();
        if expanded.is_empty() {
            self.message("Select an unlocked, visible layer before splitting a clip.");
            return false;
        }
        if self.apply_frame_edit(TimelineEdit::SplitClip {
            layers: expanded.clone(),
            at,
        }) {
            self.frame = at;
            self.select_frame_range(FrameRange {
                layers: expanded,
                start: at,
                end: at,
            });
            true
        } else {
            false
        }
    }

    pub fn split_clip_at_playhead(&mut self) -> bool {
        let layers = if self.timeline_edit.focused {
            self.frame_range().map(|r| r.layers)
        } else {
            None
        }
        .unwrap_or_else(|| vec![self.symbol().timeline.layers[self.layer].id.clone()]);
        self.split_clip_at(layers, self.frame)
    }

    fn resolve_av_link_pair(&self) -> Option<(String, String)> {
        let timeline = &self.symbol().timeline;
        if let Some(range) = self.frame_range() {
            let non_folder: Vec<String> = range
                .layers
                .into_iter()
                .filter(|id| {
                    timeline
                        .layers
                        .iter()
                        .any(|l| &l.id == id && l.properties.role != LayerRole::Folder && !l.locked)
                })
                .collect();
            if non_folder.len() >= 2 {
                return Some((non_folder[0].clone(), non_folder[1].clone()));
            }
        }
        let active = timeline.layers.get(self.layer)?;
        if active.properties.role == LayerRole::Folder || active.locked {
            return None;
        }
        let active_has_video = active.keyframes.iter().any(|k| k.video.is_some());
        let active_has_sound = active.keyframes.iter().any(|k| k.sound.is_some());
        let candidates: Vec<&Layer> = timeline
            .layers
            .iter()
            .filter(|l| {
                l.id != active.id
                    && l.properties.role != LayerRole::Folder
                    && !l.locked
                    && l.properties.linked_layer.as_deref() != Some(active.id.as_str())
            })
            .collect();
        if active_has_video
            && let Some(audio) = candidates
                .iter()
                .find(|l| l.keyframes.iter().any(|k| k.sound.is_some()))
        {
            return Some((active.id.clone(), audio.id.clone()));
        }
        if active_has_sound
            && let Some(video) = candidates
                .iter()
                .find(|l| l.keyframes.iter().any(|k| k.video.is_some()))
        {
            return Some((active.id.clone(), video.id.clone()));
        }
        candidates
            .into_iter()
            .find(|l| l.properties.linked_layer.is_none())
            .map(|partner| (active.id.clone(), partner.id.clone()))
    }

    pub fn can_link_av_layers(&self) -> bool {
        self.resolve_av_link_pair().is_some_and(|(a, b)| {
            let timeline = &self.symbol().timeline;
            timeline
                .layers
                .iter()
                .find(|l| l.id == a)
                .and_then(|l| l.properties.linked_layer.as_deref())
                != Some(b.as_str())
        })
    }

    pub fn link_layers(&mut self, a: &str, b: &str) -> bool {
        if a == b {
            return false;
        }
        self.finish_edits();
        let symbol_id = self.symbol_id().to_owned();
        let Some(orig_symbol) = self.project.symbol(&symbol_id) else {
            return false;
        };
        let mut candidate_symbol = orig_symbol.clone();
        let has_a = candidate_symbol.timeline.layers.iter().any(|l| {
            l.id == a
                && l.properties.role != LayerRole::Folder
                && !neoflash_core::layer_effectively_locked(&candidate_symbol.timeline, l)
        });
        let has_b = candidate_symbol.timeline.layers.iter().any(|l| {
            l.id == b
                && l.properties.role != LayerRole::Folder
                && !neoflash_core::layer_effectively_locked(&candidate_symbol.timeline, l)
        });
        if !has_a || !has_b {
            return false;
        }
        for layer in &mut candidate_symbol.timeline.layers {
            if layer.id != a
                && layer.id != b
                && matches!(layer.properties.linked_layer.as_deref(), Some(target) if target == a || target == b)
            {
                layer.properties.linked_layer = None;
            }
        }
        for layer in &mut candidate_symbol.timeline.layers {
            if layer.id == a {
                layer.properties.linked_layer = Some(b.to_owned());
            } else if layer.id == b {
                layer.properties.linked_layer = Some(a.to_owned());
            }
        }
        for layer in &mut candidate_symbol.timeline.layers {
            layer.normalize_motion_keys();
        }
        if let Err(err) = validate_symbol_timeline_invariants(&candidate_symbol) {
            self.message(format!("Cannot link layers: {err}"));
            return false;
        }
        if &candidate_symbol == orig_symbol {
            return false;
        }
        self.transact_named("Link video & audio", |editor| {
            if let Some(sym) = editor.project.symbol_mut(&symbol_id) {
                *sym = candidate_symbol;
            }
            editor.project.upgrade_format_for_features();
        });
        true
    }

    pub fn link_selected_or_adjacent_av_layers(&mut self) -> bool {
        let Some((a, b)) = self.resolve_av_link_pair() else {
            self.message("Select two unlocked layers (or a video and audio track) to link.");
            return false;
        };
        self.link_layers(&a, &b)
    }

    pub fn can_unlink_av_layers(&self) -> bool {
        let timeline = &self.symbol().timeline;
        let targets: Vec<String> = self.frame_range().map(|r| r.layers).unwrap_or_else(|| {
            timeline
                .layers
                .get(self.layer)
                .map(|l| vec![l.id.clone()])
                .unwrap_or_default()
        });
        let unlocked = |id: &str| {
            timeline
                .layers
                .iter()
                .find(|l| l.id == id)
                .is_none_or(|l| !neoflash_core::layer_effectively_locked(timeline, l))
        };
        timeline.layers.iter().any(|l| {
            !neoflash_core::layer_effectively_locked(timeline, l)
                && ((targets.contains(&l.id)
                    && l.properties.linked_layer.as_deref().is_some_and(unlocked))
                    || l.properties
                        .linked_layer
                        .as_ref()
                        .is_some_and(|p| targets.contains(p) && unlocked(p)))
        })
    }

    #[allow(dead_code)]
    pub fn unlink_layer(&mut self, layer_id: &str) -> bool {
        self.unlink_layers_by_ids(&[layer_id.to_owned()])
    }

    pub fn unlink_active_layers(&mut self) -> bool {
        let targets = self.frame_range().map(|r| r.layers).unwrap_or_else(|| {
            self.symbol()
                .timeline
                .layers
                .get(self.layer)
                .map(|l| vec![l.id.clone()])
                .unwrap_or_default()
        });
        self.unlink_layers_by_ids(&targets)
    }

    fn unlink_layers_by_ids(&mut self, targets: &[String]) -> bool {
        if targets.is_empty() {
            return false;
        }
        self.finish_edits();
        let symbol_id = self.symbol_id().to_owned();
        let Some(orig_symbol) = self.project.symbol(&symbol_id) else {
            return false;
        };
        let mut candidate_symbol = orig_symbol.clone();
        let locked_ids: std::collections::HashSet<String> = candidate_symbol
            .timeline
            .layers
            .iter()
            .filter(|l| neoflash_core::layer_effectively_locked(&candidate_symbol.timeline, l))
            .map(|l| l.id.clone())
            .collect();
        let mut changed = false;
        for layer in &mut candidate_symbol.timeline.layers {
            if locked_ids.contains(&layer.id) {
                continue;
            }
            if let Some(partner) = layer.properties.linked_layer.as_deref()
                && !locked_ids.contains(partner)
                && (targets.contains(&layer.id) || targets.iter().any(|t| t == partner))
            {
                layer.properties.linked_layer = None;
                changed = true;
            }
        }
        if !changed {
            return false;
        }
        for layer in &mut candidate_symbol.timeline.layers {
            layer.normalize_motion_keys();
        }
        if let Err(err) = validate_symbol_timeline_invariants(&candidate_symbol) {
            self.message(format!("Cannot unlink layers: {err}"));
            return false;
        }
        self.transact_named("Unlink video & audio", |editor| {
            if let Some(sym) = editor.project.symbol_mut(&symbol_id) {
                *sym = candidate_symbol;
            }
            editor.project.upgrade_format_for_features();
        });
        true
    }

    pub(crate) fn clear_frame_range(&mut self, remove: bool) {
        if let Some(mut range) = self.frame_range() {
            let include_scripts = self.timeline_edit.include_scripts;
            if remove {
                range.layers = self.expand_with_linked_layers(&range.layers);
            }
            self.apply_frame_edit(if remove {
                TimelineEdit::Delete {
                    range,
                    include_scripts,
                }
            } else {
                TimelineEdit::Clear {
                    range,
                    include_scripts,
                }
            });
        }
    }

    pub(crate) fn insert_frame_range(&mut self) {
        if let Some(mut range) = self.frame_range() {
            range.layers = self.expand_with_linked_layers(&range.layers);
            let count = self.symbol().timeline.frame_count;
            let extends_beyond_end = range.layers.iter().all(|id| {
                self.symbol()
                    .timeline
                    .layers
                    .iter()
                    .find(|l| &l.id == id)
                    .is_some_and(|l| {
                        let layer_end = l.properties.end_frame.unwrap_or(count).min(count);
                        range.end >= layer_end
                    })
            });
            if extends_beyond_end {
                let new_end = range.end.saturating_add(1);
                if self.apply_frame_edit(TimelineEdit::SetEnd {
                    layers: range.layers.clone(),
                    end_frame: new_end,
                }) {
                    self.frame = range.end;
                    self.select_frame_range(FrameRange {
                        layers: range.layers,
                        start: range.end,
                        end: range.end,
                    });
                }
                return;
            }
            self.apply_frame_edit(TimelineEdit::Insert {
                layers: range.layers,
                at: range.start,
                count: range.end - range.start + 1,
                include_scripts: self.timeline_edit.include_scripts,
            });
        }
    }

    pub(crate) fn set_layer_duration(&mut self, end_frame: u32) -> bool {
        let base_layers = if self.timeline_edit.focused {
            self.frame_range().map(|r| r.layers)
        } else {
            None
        }
        .unwrap_or_else(|| vec![self.symbol().timeline.layers[self.layer].id.clone()]);
        let layers = self.expand_with_linked_layers(&base_layers);
        self.apply_frame_edit(TimelineEdit::SetEnd { layers, end_frame })
    }

    pub(crate) fn timeline_shortcuts(&mut self, ctx: &egui::Context) {
        use crate::commands::CommandId;
        if !self.timeline_edit.focused
            || self.frame_range().is_none()
            || self.blocks_unmodified_shortcuts(ctx)
            || self.confirm.is_some()
            || self
                .timeline_edit
                .viewport
                .is_some_and(|id| id != ctx.viewport_id())
        {
            return;
        }
        let mut copy = false;
        let mut cut = false;
        let mut paste = false;
        let mut paste_payload = None;
        ctx.input_mut(|input| {
            input.events.retain(|event| match event {
                egui::Event::Copy => {
                    copy = true;
                    false
                }
                egui::Event::Cut => {
                    cut = true;
                    false
                }
                egui::Event::Paste(text) => {
                    paste = true;
                    paste_payload = Some(text.clone());
                    false
                }
                _ => true,
            })
        });
        copy |= self.take_command_shortcut(ctx, CommandId::EditCopy);
        cut |= self.take_command_shortcut(ctx, CommandId::EditCut);
        paste |= self.take_command_shortcut(ctx, CommandId::EditPaste);
        if cut {
            self.copy_frames_to_system_clipboard(ctx, true);
        } else if copy {
            self.copy_frames_to_system_clipboard(ctx, false);
        }
        if paste {
            self.paste_frames_from_system_clipboard(paste_payload);
        }
        if self.take_command_shortcut(ctx, CommandId::EditClear) {
            self.clear_frame_range(false);
        }
        if self.take_command_shortcut(ctx, CommandId::InsertFrame) {
            self.insert_frame_range();
        }
        if self.take_command_shortcut(ctx, CommandId::EditSelectAll) {
            self.select_all_frames();
        }
    }

    pub(crate) fn frame_command_enabled(&self, command: FrameCommand) -> bool {
        command == FrameCommand::SelectAll
            || (self.frame_range().is_some()
                && (command != FrameCommand::Paste || self.timeline_edit.clipboard.is_some()))
    }

    pub(crate) fn run_frame_command(&mut self, ctx: &egui::Context, command: FrameCommand) {
        if !self.frame_command_enabled(command) {
            return;
        }
        match command {
            FrameCommand::Cut => self.copy_frames_to_system_clipboard(ctx, true),
            FrameCommand::Copy => self.copy_frames_to_system_clipboard(ctx, false),
            FrameCommand::Paste => self.paste_frames_from_system_clipboard(None),
            FrameCommand::Clear => self.clear_frame_range(false),
            FrameCommand::Remove => self.clear_frame_range(true),
            FrameCommand::SelectAll => self.select_all_frames(),
        }
    }

    /// Every frame of every non-folder layer in the edited timeline.
    pub(crate) fn select_all_frames(&mut self) {
        let layers = self
            .symbol()
            .timeline
            .layers
            .iter()
            .filter(|l| l.properties.role != LayerRole::Folder)
            .map(|l| l.id.clone())
            .collect();
        self.select_frame_range(FrameRange {
            layers,
            start: 0,
            end: self.symbol().timeline.frame_count.saturating_sub(1),
        });
    }

    /// Inclusive `(start, end)` frame span containing `frame` on `layer_index`.
    /// ◆ property keys inside a motion tween belong to the enclosing motion span; frames outside
    /// the layer's exposure or before its first keyframe select only `(frame, frame)`.
    pub(crate) fn layer_span_range(
        &self,
        layer_index: usize,
        frame: u32,
        count: u32,
    ) -> (u32, u32) {
        let Some(layer) = self.symbol().timeline.layers.get(layer_index) else {
            return (frame, frame);
        };
        let layer_end = layer.properties.end_frame.unwrap_or(count).min(count);
        if frame >= layer_end || layer.properties.role == LayerRole::Folder {
            return (frame, frame);
        }
        if let Some((start, _, end)) = neoflash_core::motion_span(layer, frame) {
            let exclusive_end = end.unwrap_or(layer_end).min(layer_end);
            return if exclusive_end > start.frame {
                (start.frame, exclusive_end - 1)
            } else {
                (frame, frame)
            };
        }
        let Some(start) = layer
            .keyframes
            .iter()
            .filter(|k| k.frame <= frame && k.keyed_properties.is_none())
            .map(|k| k.frame)
            .max()
        else {
            return (frame, frame);
        };
        let next = layer
            .keyframes
            .iter()
            .filter(|k| k.frame > start && k.keyed_properties.is_none())
            .map(|k| k.frame)
            .min();
        let exclusive_end = next.unwrap_or(layer_end).min(layer_end);
        if exclusive_end > start {
            (start, exclusive_end - 1)
        } else {
            (frame, frame)
        }
    }

    // Shared viewport geometry is passed directly by the timeline painter.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn timeline_grid_interact(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
        clip: Rect,
        cell: f32,
        grid_count: u32,
        rows: &[usize],
        blocked: bool,
    ) {
        let count = self.symbol().timeline.frame_count;
        let row_height = self.workspace.timeline.row_height();
        if rows.is_empty() {
            return;
        }
        if self.timeline_edit.symbol != self.symbol_id() {
            self.timeline_edit.range = None;
            self.timeline_edit.anchor = None;
            self.timeline_edit.gesture = None;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape) || !i.focused) {
            self.timeline_edit.gesture = None;
            self.timeline_key_drag = None;
            return;
        }
        let (pressed, released) = ui.input(|input| {
            let mut down = None;
            let mut up = None;
            for event in &input.events {
                if let egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers,
                } = event
                {
                    if *pressed {
                        down = Some((*pos, *modifiers));
                        up = None;
                    } else {
                        up = Some((*pos, *modifiers));
                    }
                }
            }
            (down, up)
        });
        if self.tool == crate::app::Tool::Razor && !blocked && ui.is_enabled() && response.hovered()
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        if let Some((pos, modifiers)) = pressed.filter(|(pos, _)| {
            !blocked
                && ui.is_enabled()
                && clip.contains(*pos)
                && !response.context_menu_opened()
                && ui.ctx().layer_id_at(*pos) == Some(ui.layer_id())
        }) {
            let hit = timeline_cell_at(pos, rect.min, cell, grid_count, rows.len(), row_height);
            let end_hit = rows
                .iter()
                .enumerate()
                .find(|(row, index)| {
                    let layer = &self.symbol().timeline.layers[**index];
                    layer.properties.role != LayerRole::Folder
                        && end_handle(
                            rect,
                            *row,
                            cell,
                            layer.properties.end_frame.unwrap_or(count),
                            row_height,
                        )
                        .contains(pos)
                })
                .map(|(_, &i)| {
                    (
                        self.symbol().timeline.layers[i].id.clone(),
                        self.symbol().timeline.layers[i]
                            .properties
                            .end_frame
                            .unwrap_or(count),
                    )
                });
            if hit.is_some() || end_hit.is_some() {
                self.finish_edits();
                self.set_playing(false);
                self.reset_authored_preview();
                self.clear_selection();
                self.timeline_edit.focused = true;
                self.timeline_edit.symbol = self.symbol_id().to_owned();
                self.timeline_edit.viewport = Some(ui.ctx().viewport_id());
                self.script_target = ScriptTarget::Frame;
            }
            if let Some((layer, end)) = end_hit {
                self.timeline_edit.gesture = Some(Gesture::End { layer, end });
            } else if let Some((frame, row)) = hit {
                let layer_id = self.symbol().timeline.layers[rows[row]].id.clone();
                if self.tool == crate::app::Tool::Razor && !modifiers.shift && !modifiers.command {
                    self.frame = frame;
                    self.layer = rows[row];
                    self.split_clip_at(vec![layer_id], frame);
                    return;
                }
                let span_mode = self.workspace.timeline_prefs.span_selection && !modifiers.command;
                let existing = (!modifiers.command)
                    .then(|| {
                        self.frame_range().filter(|r| {
                            (r.layers.len() > 1 || r.start < r.end)
                                && r.start <= frame
                                && frame <= r.end
                                && r.layers.contains(&layer_id)
                        })
                    })
                    .flatten();
                self.frame = frame;
                self.layer = rows[row];
                if modifiers.shift {
                    let (anchor_row, anchor_frame) = self
                        .timeline_edit
                        .anchor
                        .as_ref()
                        .and_then(|(id, frame)| {
                            rows.iter()
                                .position(|i| &self.symbol().timeline.layers[*i].id == id)
                                .map(|row| (row, *frame))
                        })
                        .unwrap_or((row, frame));
                    let (sel_start, sel_end) = if span_mode {
                        let (a0, a1) = self.layer_span_range(rows[anchor_row], anchor_frame, count);
                        let (b0, b1) = self.layer_span_range(rows[row], frame, count);
                        (a0.min(b0), a1.max(b1))
                    } else {
                        (anchor_frame, frame)
                    };
                    self.select_frame_range(
                        self.frame_rectangle(rows, anchor_row, row, sel_start, sel_end),
                    );
                    self.timeline_edit.gesture = Some(Gesture::Select {
                        anchor_row,
                        anchor_frame,
                        press_row: row,
                        press_frame: frame,
                        moved: false,
                    });
                } else if let Some(range) = existing {
                    self.timeline_edit.gesture = Some(Gesture::Move {
                        range,
                        row,
                        frame,
                        target_row: row,
                        target_frame: frame,
                    });
                } else {
                    let layer_end = self.symbol().timeline.layers[rows[row]]
                        .properties
                        .end_frame
                        .unwrap_or(count)
                        .min(count);
                    let key = frame < layer_end
                        && self.symbol().timeline.layers[rows[row]]
                            .keyframes
                            .iter()
                            .any(|key| key.frame == frame);
                    let (sel_start, sel_end) = if span_mode {
                        self.layer_span_range(rows[row], frame, count)
                    } else {
                        (frame, frame)
                    };
                    self.select_frame_range(
                        self.frame_rectangle(rows, row, row, sel_start, sel_end),
                    );
                    self.timeline_edit.anchor = Some((layer_id.clone(), frame));
                    self.timeline_edit.gesture = Some(if key {
                        self.timeline_key_drag =
                            Some((self.symbol_id().to_owned(), layer_id, frame));
                        Gesture::Key {
                            layer: rows[row],
                            frame,
                        }
                    } else {
                        Gesture::Select {
                            anchor_row: row,
                            anchor_frame: frame,
                            press_row: row,
                            press_frame: frame,
                            moved: false,
                        }
                    });
                }
            }
        } else if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
            && let Some((frame, row)) =
                timeline_cell_at(pos, rect.min, cell, grid_count, rows.len(), row_height)
        {
            self.finish_edits();
            self.set_playing(false);
            self.reset_authored_preview();
            self.clear_selection();
            let layer_id = &self.symbol().timeline.layers[rows[row]].id;
            if !self
                .frame_range()
                .is_some_and(|r| r.start <= frame && frame <= r.end && r.layers.contains(layer_id))
            {
                let (sel_start, sel_end) = if self.workspace.timeline_prefs.span_selection {
                    self.layer_span_range(rows[row], frame, count)
                } else {
                    (frame, frame)
                };
                self.select_frame_range(self.frame_rectangle(rows, row, row, sel_start, sel_end));
            }
            self.timeline_edit.focused = true;
            self.timeline_edit.viewport = Some(ui.ctx().viewport_id());
            self.frame = frame;
            self.layer = rows[row];
            self.script_target = ScriptTarget::Frame;
        }
        let position = released
            .map(|p| p.0)
            .or_else(|| ui.input(|i| i.pointer.latest_pos()));
        if let Some(pos) = position
            && let Some(gesture) = self.timeline_edit.gesture.clone()
        {
            let frame = frame_at_x(pos.x, rect.left(), cell, grid_count);
            let key_frame = frame_at_x(pos.x, rect.left(), cell, grid_count);
            let row = (((pos.y - rect.top()) / row_height).max(0.) as usize)
                .min(rows.len().saturating_sub(1));
            let dragging = ui.input(|i| i.pointer.is_decidedly_dragging()) || response.dragged();
            if dragging || released.is_some() {
                match gesture {
                    Gesture::Select {
                        anchor_row,
                        anchor_frame,
                        press_row,
                        press_frame,
                        moved,
                    } => {
                        let moved = moved || row != press_row || frame != press_frame;
                        self.timeline_edit.gesture = Some(Gesture::Select {
                            anchor_row,
                            anchor_frame,
                            press_row,
                            press_frame,
                            moved,
                        });
                        if moved {
                            self.select_frame_range(self.frame_rectangle(
                                rows,
                                anchor_row,
                                row,
                                anchor_frame,
                                frame,
                            ));
                        }
                        self.frame = frame;
                        self.layer = rows[row];
                    }
                    Gesture::Move {
                        range,
                        row: start_row,
                        frame: start,
                        ..
                    } => {
                        self.timeline_edit.gesture = Some(Gesture::Move {
                            range,
                            row: start_row,
                            frame: start,
                            target_row: row,
                            target_frame: frame,
                        })
                    }
                    Gesture::Key { layer, .. } => {
                        self.frame = key_frame;
                        self.layer = layer;
                    }
                    Gesture::End { layer, .. } => {
                        let end =
                            (((pos.x - rect.left()) / cell).round().max(0.) as u32).min(1_000_000);
                        self.timeline_edit.gesture = Some(Gesture::End { layer, end });
                    }
                }
            }
        }
        if let Some((pos, modifiers)) = released {
            let gesture = self.timeline_edit.gesture.take();
            self.timeline_key_drag = None;
            if pos.y < clip.top()
                || pos.y
                    >= clip
                        .bottom()
                        .min(rect.top() + rows.len() as f32 * row_height)
            {
                return;
            }
            match gesture {
                Some(Gesture::Key { layer, frame }) if frame != self.frame => {
                    self.move_key(layer, frame, self.frame, modifiers.alt);
                    self.select_frame_range(FrameRange {
                        layers: vec![self.symbol().timeline.layers[layer].id.clone()],
                        start: self.frame,
                        end: self.frame,
                    });
                }
                Some(Gesture::Move {
                    range,
                    row,
                    frame,
                    target_row,
                    target_frame,
                }) => {
                    if row != target_row || frame != target_frame {
                        let delta = target_frame as i64 - frame as i64;
                        let start = (range.start as i64 + delta).max(0) as u32;
                        let (move_range, layers) = if row == target_row {
                            let expanded = self.expand_with_linked_layers(&range.layers);
                            let mut expanded_range = range;
                            expanded_range.layers = expanded.clone();
                            (expanded_range, expanded)
                        } else {
                            let first = rows
                                .iter()
                                .position(|i| {
                                    Some(&self.symbol().timeline.layers[*i].id)
                                        == range.layers.first()
                                })
                                .unwrap_or(row);
                            let destination = (first as isize + target_row as isize - row as isize)
                                .max(0) as usize;
                            let dst_layers: Vec<_> = rows
                                .iter()
                                .skip(destination)
                                .map(|&i| &self.symbol().timeline.layers[i])
                                .filter(|l| l.properties.role != LayerRole::Folder)
                                .take(range.layers.len())
                                .map(|l| l.id.clone())
                                .collect();
                            (range, dst_layers)
                        };
                        let end = start + move_range.end - move_range.start;
                        if self.apply_frame_edit(TimelineEdit::Move {
                            range: move_range,
                            layers: layers.clone(),
                            at: start,
                            copy: modifiers.alt,
                            include_scripts: self.timeline_edit.include_scripts,
                        }) {
                            self.select_frame_range(FrameRange { layers, start, end });
                            self.frame = start;
                        }
                    } else if let Some(&layer_index) = rows.get(row) {
                        let layer_id = self.symbol().timeline.layers[layer_index].id.clone();
                        let (sel_start, sel_end) =
                            if self.workspace.timeline_prefs.span_selection && !modifiers.command {
                                self.layer_span_range(layer_index, frame, count)
                            } else {
                                (frame, frame)
                            };
                        self.select_frame_range(
                            self.frame_rectangle(rows, row, row, sel_start, sel_end),
                        );
                        self.timeline_edit.anchor = Some((layer_id, frame));
                    }
                }
                Some(Gesture::End { layer, end }) => {
                    let layers = self.expand_with_linked_layers(&[layer]);
                    self.apply_frame_edit(TimelineEdit::SetEnd {
                        layers,
                        end_frame: end,
                    });
                }
                _ => {}
            }
        }
    }

    pub(crate) fn paint_frame_selection(
        &self,
        ui: &egui::Ui,
        rect: Rect,
        cell: f32,
        rows: &[usize],
    ) {
        let row_height = self.workspace.timeline.row_height();
        // Selected frames are painted per row as `NF2/TL Span` Type=Selected (panels/timeline.rs)
        // so markers on top can switch to Tone=On Selection. Here: cursor feedback only.
        let palette = crate::theme::UiPalette::current(ui.ctx());
        if let Some(range) = self.frame_range() {
            for (row, &i) in rows.iter().enumerate() {
                if range.layers.contains(&self.symbol().timeline.layers[i].id) {
                    let selected = Rect::from_min_size(
                        Pos2::new(
                            rect.left() + range.start as f32 * cell,
                            rect.top() + row as f32 * row_height,
                        ),
                        Vec2::new((range.end - range.start + 1) as f32 * cell, row_height),
                    );
                    if ui.rect_contains_pointer(selected) {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    }
                }
            }
        }
        if let Some(Gesture::Move {
            range,
            row,
            frame,
            target_row,
            target_frame,
        }) = &self.timeline_edit.gesture
        {
            let first = rows
                .iter()
                .position(|i| Some(&self.symbol().timeline.layers[*i].id) == range.layers.first())
                .unwrap_or(*row);
            let destination =
                (first as isize + *target_row as isize - *row as isize).max(0) as usize;
            let start = (range.start as i64 + *target_frame as i64 - *frame as i64).max(0) as u32;
            for (row, _) in rows
                .iter()
                .enumerate()
                .skip(destination)
                .filter(|(_, i)| {
                    self.symbol().timeline.layers[**i].properties.role != LayerRole::Folder
                })
                .take(range.layers.len())
            {
                let preview = Rect::from_min_size(
                    Pos2::new(
                        rect.left() + start as f32 * cell,
                        rect.top() + row as f32 * row_height,
                    ),
                    Vec2::new((range.end - range.start + 1) as f32 * cell, row_height),
                );
                ui.painter().rect_stroke(
                    preview,
                    0,
                    egui::Stroke::new(2.0_f32, palette.accent),
                    egui::StrokeKind::Inside,
                );
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        for (row, &i) in rows.iter().enumerate() {
            let layer = &self.symbol().timeline.layers[i];
            if layer.properties.role == LayerRole::Folder {
                continue;
            }
            let end = match &self.timeline_edit.gesture {
                Some(Gesture::End { layer: id, end }) if id == &layer.id => *end,
                _ => layer
                    .properties
                    .end_frame
                    .unwrap_or(self.symbol().timeline.frame_count),
            };
            let handle = end_handle(rect, row, cell, end, row_height);
            // The span end ▯ marks the layer end; the drag handle shows as an accent edge
            // while hovered or dragged (drop-indicator role).
            let dragging = matches!(&self.timeline_edit.gesture, Some(Gesture::End { layer: id, .. }) if id == &layer.id);
            if ui.rect_contains_pointer(handle) || dragging {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                let x = rect.left() + end as f32 * cell;
                let top = rect.top() + row as f32 * row_height;
                ui.painter().vline(
                    x,
                    top..=top + row_height,
                    egui::Stroke::new(2.0_f32, palette.accent),
                );
            }
            let handle_resp = ui.interact(
                handle,
                ui.id().with(("exposure-end", &layer.id)),
                egui::Sense::hover(),
            );
            neoflash_ui::components::tooltip(
                handle_resp,
                &format!(
                    "Layer ends after frame {end} · drag to change duration. Shortening removes later keys"
                ),
                None,
            );
        }
    }
}

fn end_handle(rect: Rect, row: usize, cell: f32, end: u32, row_height: f32) -> Rect {
    Rect::from_center_size(
        Pos2::new(
            rect.left() + end as f32 * cell,
            rect.top() + (row + 1) as f32 * row_height - 4.,
        ),
        Vec2::new(8., 7.),
    )
}

fn validate_symbol_timeline_invariants(symbol: &Symbol) -> Result<(), String> {
    let timeline = &symbol.timeline;
    if timeline.frame_count == 0 || timeline.frame_count > 1_000_000 {
        return Err("frame count must be in 1..=1000000".into());
    }
    for script in &timeline.scripts {
        if script.frame >= timeline.frame_count {
            return Err("script frame is outside the timeline".into());
        }
    }
    let layers = &timeline.layers;
    let mut layer_ids = std::collections::HashSet::with_capacity(layers.len());
    for layer in layers {
        if layer.id.is_empty() || !layer_ids.insert(layer.id.as_str()) {
            return Err(format!("invalid or duplicate layer id '{}'", layer.id));
        }
        let effective_end = layer.properties.end_frame.unwrap_or(timeline.frame_count);
        if let Some(end) = layer.properties.end_frame
            && end > timeline.frame_count
        {
            return Err("layer exposure ends outside the timeline".into());
        }
        if layer.properties.role == LayerRole::Folder {
            if layer.keyframes.iter().any(|k| !k.elements.is_empty()) {
                return Err(format!(
                    "Folder '{}' must not contain artwork; move artwork into a child layer",
                    layer.name
                ));
            }
        } else if effective_end > 0 && layer.keyframes.is_empty() {
            return Err(format!(
                "Layer '{}' requires at least one keyframe when exposed",
                layer.name
            ));
        }
        if let Some(linked_id) = &layer.properties.linked_layer {
            if layer.properties.role == LayerRole::Folder {
                return Err("folder layers cannot be linked as montage tracks".into());
            }
            if linked_id == &layer.id {
                return Err("a layer cannot be linked to itself".into());
            }
            match layers.iter().find(|other| &other.id == linked_id) {
                Some(other) if other.properties.role != LayerRole::Folder => {}
                Some(_) => return Err("linked_layer cannot reference a folder layer".into()),
                None => {
                    return Err(format!(
                        "linked_layer '{linked_id}' does not exist in timeline"
                    ));
                }
            }
        }
        if let Some(id) = &layer.properties.mask
            && (id == &layer.id
                || layer.properties.role != LayerRole::Normal
                || !layers
                    .iter()
                    .any(|l| &l.id == id && l.properties.role == LayerRole::Mask))
        {
            return Err(format!(
                "Layer '{}' must reference another mask layer from a normal layer",
                layer.name
            ));
        }
        if let Some(id) = &layer.properties.motion_guide
            && (id == &layer.id
                || layer.properties.role != LayerRole::Normal
                || !layers
                    .iter()
                    .any(|l| &l.id == id && l.properties.role == LayerRole::Guide))
        {
            return Err(format!(
                "Layer '{}' must reference another guide layer as its motion guide from a normal layer",
                layer.name
            ));
        }
        if let Some(id) = &layer.properties.parent {
            let mut seen = std::collections::HashSet::from([layer.id.as_str()]);
            let mut next = Some(id.as_str());
            while let Some(cur) = next {
                if !seen.insert(cur) {
                    return Err(format!("Layer '{}' has cyclic folder ancestry", layer.name));
                }
                let Some(parent) = layers
                    .iter()
                    .find(|l| l.id == cur && l.properties.role == LayerRole::Folder)
                else {
                    return Err(format!(
                        "Layer '{}' references a missing folder",
                        layer.name
                    ));
                };
                next = parent.properties.parent.as_deref();
            }
        }
        let mut seen_frames = std::collections::HashSet::with_capacity(layer.keyframes.len());
        let mut keys: Vec<_> = layer.keyframes.iter().collect();
        keys.sort_by_key(|k| k.frame);
        let mut in_motion = false;
        for key in keys {
            if key.frame >= timeline.frame_count || key.frame >= effective_end {
                return Err("keyframe is outside the timeline or layer exposure".into());
            }
            if !seen_frames.insert(key.frame) {
                return Err("duplicate frame in a layer".into());
            }
            if key.keyed_properties.is_some() {
                if !in_motion {
                    return Err(format!(
                        "Layer '{}' has a property keyframe at frame {} outside a motion tween",
                        layer.name,
                        key.frame + 1
                    ));
                }
            } else {
                in_motion = key.tween.is_some() && key.tween_kind == TweenKind::Motion;
            }
        }
    }
    Ok(())
}

fn build_shared_frame_clipboard(
    project: &neoflash_core::Project,
    symbol_id: &str,
    clip: &FrameClipboard,
    source_session: &str,
) -> SharedFrameClipboard {
    let mut seeds = crate::clipboard::SeedAssetRefs::default();
    if !clip.scripts.is_empty() {
        seeds.needs_sources = true;
    }
    for layer_clip in &clip.layers {
        for kf in &layer_clip.keyframes {
            seeds.record_keyframe(kf);
        }
    }

    let mut reachable_syms = std::collections::HashSet::<String>::new();
    let mut stack = seeds.symbol_ids.clone();
    while let Some(sid) = stack.pop() {
        if project.is_scene(&sid) || !reachable_syms.insert(sid.clone()) {
            continue;
        }
        if let Some(sym) = project.symbol(&sid) {
            if !sym.timeline.scripts.is_empty()
                || sym.class_name.is_some()
                || sym.linkage_id.is_some()
            {
                seeds.needs_sources = true;
            }
            for layer in &sym.timeline.layers {
                for kf in &layer.keyframes {
                    let prev = seeds.symbol_ids.len();
                    seeds.record_keyframe(kf);
                    for new_sid in &seeds.symbol_ids[prev..] {
                        stack.push(new_sid.clone());
                    }
                }
            }
        }
    }

    let needed_res: std::collections::HashSet<&str> =
        seeds.resource_ids.iter().map(String::as_str).collect();
    let donor_resources: Vec<neoflash_core::ResourceUnit> = project
        .resources
        .iter()
        .filter(|r| {
            needed_res.contains(r.id.as_str())
                || (!seeds.font_families.is_empty()
                    && (neoflash_core::is_font_resource_unit(r)
                        || r.path.eq_ignore_ascii_case("fonts/embedding.json")))
                || (!seeds.swatch_ids.is_empty()
                    && r.path.eq_ignore_ascii_case(
                        neoflash_core::Project::GLOBAL_SWATCHES_RESOURCE_PATH,
                    ))
                || (!seeds.style_ids.is_empty()
                    && r.path
                        .eq_ignore_ascii_case(neoflash_core::Project::VECTOR_STYLES_RESOURCE_PATH))
                || (!reachable_syms.is_empty()
                    && r.path.eq_ignore_ascii_case(
                        neoflash_core::Project::SHARED_LIBRARIES_RESOURCE_PATH,
                    ))
        })
        .cloned()
        .collect();

    let mut donor_scene_ids = std::collections::HashSet::<String>::new();
    donor_scene_ids.insert(project.root.clone());
    donor_scene_ids.extend(project.scenes.iter().cloned());

    let donor_symbols: Vec<Symbol> = project
        .symbols
        .iter()
        .filter(|s| reachable_syms.contains(&s.id))
        .cloned()
        .collect();

    let layer_groups: Vec<Vec<neoflash_core::AuthoringGroup>> =
        if let Some(sym) = project.symbol(symbol_id) {
            clip.layers
                .iter()
                .map(|lc| {
                    sym.timeline
                        .layers
                        .iter()
                        .find(|l| l.id == lc.source_layer)
                        .map(|l| l.properties.groups.clone())
                        .unwrap_or_default()
                })
                .collect()
        } else {
            vec![Vec::new(); clip.layers.len()]
        };

    SharedFrameClipboard {
        clip: clip.clone(),
        marker: None,
        source_session: source_session.to_owned(),
        donor_symbols,
        donor_scene_ids,
        donor_resources,
        donor_sources: if seeds.needs_sources {
            project.sources.clone()
        } else {
            Vec::new()
        },
        layer_groups,
    }
}
