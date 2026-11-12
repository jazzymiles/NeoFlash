//! SWF character dictionary, display-list timeline reconstruction, button states,
//! multi-scene splitting, and AVM1/AVM2 script & linkage wiring into a validated
//! NeoFlash `Project`.

use super::avm1::{
    DecompiledObjectHandler, decompile_actions, parse_and_decompile_button2_actions,
    parse_and_decompile_clip_actions, script_language_for_swf,
};
use super::avm2::{RecoveredAs3Class, decompile_do_abc_tag};
use super::bits::SwfSlice;
use super::media::{
    SwfSoundAsset, SwfStreamSoundCollector, append_sound_stream_block, decode_define_sound_tag,
    decode_jpeg_bitmap_tag, decode_lossless_bitmap_tag, finalize_stream_sound, parse_sound_info,
    parse_sound_stream_head,
};
use super::shape::{SwfBitmapRef, decode_define_morph_shape, decode_define_shape};
use super::text::{
    SwfFontDef, decode_define_edit_text, decode_define_font, decode_define_font_info,
    decode_define_font_name_or_4, decode_define_font2_or_3, decode_define_text,
};
use super::{SwfContainer, SwfInspection};
use crate::ImportReport;
use neoflash_core::{
    Color, ColorTransform, Easing, Element, ElementKind, FrameScript, FrameSound, Id,
    InstancePlayback, Keyframe, Layer, LayerProperties, LayerRole, MAX_BITMAP_BYTES,
    MAX_RESOURCE_BYTES, MAX_RESOURCE_TOTAL_BYTES, MAX_RESOURCES, ObjectScript, Project,
    ResourceUnit, Script, ScriptLanguage, SourceUnit, Symbol, SymbolKind, Timeline, Transform,
    TweenKind,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

const MAX_AUTHORED_ELEMENTS: usize = 95_000;
const MAX_AUTHORED_BITMAPS: usize = 250;

#[derive(Clone, Debug)]
enum SwfCharacter {
    GraphicSymbol {
        symbol_id: Id,
        is_morph: bool,
        is_pure_vector: bool,
    },
    InlineText {
        element: Box<Element>,
        fallback_symbol_id: Option<Id>,
    },
    Button {
        symbol_id: Id,
        instance_scripts: Vec<DecompiledObjectHandler>,
        is_pure_vector: bool,
    },
    Sprite {
        symbol_id: Id,
        is_pure_vector: bool,
    },
}

#[derive(Clone, Debug)]
struct ButtonRecord {
    states: u8, // bit 0=Up, 1=Over, 2=Down, 3=Hit
    character_id: u16,
    place_depth: u16,
    matrix: [f64; 6],
    color_transform: ColorTransform,
    alpha: f64,
    blend_mode: neoflash_core::BlendMode,
    filters: Vec<neoflash_core::FilterSpec>,
}

struct PendingButton {
    button_id: u16,
    records: Vec<ButtonRecord>,
    handlers: Vec<DecompiledObjectHandler>,
}

#[derive(Clone, Debug)]
struct ActiveSlot {
    span_id: u32,
    character_id: u16,
    matrix: [f64; 6],
    color_transform: ColorTransform,
    alpha: f64,
    ratio: Option<u16>,
    name: String,
    clip_depth: Option<u16>,
    visible: bool,
    cache_as_bitmap: bool,
    blend_mode: neoflash_core::BlendMode,
    filters: Vec<neoflash_core::FilterSpec>,
    handlers: Vec<DecompiledObjectHandler>,
}

impl ActiveSlot {
    fn same_visual_state(&self, other: &Self) -> bool {
        self.span_id == other.span_id
            && self.character_id == other.character_id
            && self.matrix == other.matrix
            && self.color_transform == other.color_transform
            && self.alpha == other.alpha
            && self.ratio == other.ratio
            && self.name == other.name
            && self.clip_depth == other.clip_depth
            && self.visible == other.visible
            && self.cache_as_bitmap == other.cache_as_bitmap
            && self.blend_mode == other.blend_mode
            && self.filters == other.filters
            && self.handlers.len() == other.handlers.len()
    }
}

#[derive(Clone, Debug, Default)]
struct FrameSnapshot {
    slots: BTreeMap<u16, ActiveSlot>,
    label: Option<String>,
    actions: Vec<String>,
    sounds: Vec<FrameSound>,
}

struct SwfBuilder<'a> {
    swf_version: u8,
    location: &'a str,
    report: &'a mut ImportReport,
    jpeg_tables: Option<Vec<u8>>,
    bitmaps: HashMap<u16, SwfBitmapRef>,
    bitmap_rgba_bytes: usize,
    sounds: HashMap<u16, SwfSoundAsset>,
    fonts: HashMap<u16, SwfFontDef>,
    characters: HashMap<u16, SwfCharacter>,
    pending_buttons: HashMap<u16, PendingButton>,
    button_cxforms: HashMap<u16, Vec<(ColorTransform, f64)>>,
    button_sounds: HashMap<u16, Vec<(u32, FrameSound)>>,
    init_actions: HashMap<u16, Vec<String>>,
    exports: Vec<(u16, String)>,
    symbol_classes: Vec<(u16, String)>,
    abc_classes: Vec<RecoveredAs3Class>,
    scenes_meta: Vec<(u32, String)>,
    symbols: Vec<Symbol>,
    resources: Vec<ResourceUnit>,
    resource_bytes_total: usize,
    total_elements: usize,
    text_metrics_warned: bool,
    legacy_cp_warned: bool,
    filter_warned: bool,
    blend_warned: bool,
    clip_event_warned: bool,
    unrecognized_avm1: usize,
    unrecognized_avm2: usize,
}

pub(crate) fn build_project_from_swf(
    container: &SwfContainer,
    inspection: &SwfInspection,
    body: &[u8],
    tags_offset: usize,
    location: &str,
    report: &mut ImportReport,
) -> Option<Project> {
    let bounds = inspection.stage_bounds_twips.unwrap_or([0, 11000, 0, 8000]);
    let width_px = if bounds[1] < bounds[0] {
        550
    } else {
        ((i64::from(bounds[1]) - i64::from(bounds[0])).max(20) as f64 / 20.0)
            .round()
            .clamp(1.0, 65535.0) as u32
    };
    let height_px = if bounds[3] < bounds[2] {
        400
    } else {
        ((i64::from(bounds[3]) - i64::from(bounds[2])).max(20) as f64 / 20.0)
            .round()
            .clamp(1.0, 65535.0) as u32
    };
    let stage_dx = if bounds[1] < bounds[0] {
        0.0
    } else {
        -(f64::from(bounds[0]) / 20.0)
    };
    let stage_dy = if bounds[3] < bounds[2] {
        0.0
    } else {
        -(f64::from(bounds[2]) / 20.0)
    };
    let fps = inspection
        .frame_rate
        .unwrap_or(24.0)
        .round()
        .clamp(1.0, 1000.0) as u32;
    let background = inspection
        .background_color
        .as_deref()
        .and_then(parse_hex_rgb)
        .unwrap_or([255, 255, 255, 255]);

    let project_name = Path::new(location)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Imported SWF")
        .to_owned();

    let mut builder = SwfBuilder {
        swf_version: container.swf_version,
        location,
        report,
        jpeg_tables: None,
        bitmaps: HashMap::new(),
        bitmap_rgba_bytes: 0,
        sounds: HashMap::new(),
        fonts: HashMap::new(),
        characters: HashMap::new(),
        pending_buttons: HashMap::new(),
        button_cxforms: HashMap::new(),
        button_sounds: HashMap::new(),
        init_actions: HashMap::new(),
        exports: Vec::new(),
        symbol_classes: Vec::new(),
        abc_classes: Vec::new(),
        scenes_meta: Vec::new(),
        symbols: Vec::new(),
        resources: Vec::new(),
        resource_bytes_total: 0,
        total_elements: 0,
        text_metrics_warned: false,
        legacy_cp_warned: false,
        filter_warned: false,
        blend_warned: false,
        clip_event_warned: false,
        unrecognized_avm1: 0,
        unrecognized_avm2: 0,
    };

    let tag_bytes = body.get(tags_offset..)?;
    let root_snapshots = builder.process_timeline_tags(tag_bytes, "root", (stage_dx, stage_dy));
    builder.finalize_pending_buttons();

    // Attach `DoInitAction` scripts to target sprites (or root frame 0 if target isn't a sprite).
    let mut root_extra_frame0_actions = Vec::new();
    let init_actions = std::mem::take(&mut builder.init_actions);
    for (sprite_char_id, scripts) in init_actions {
        let joined = scripts.join("\n\n");
        if joined.trim().is_empty() {
            continue;
        }
        let target_sym_id = match builder.characters.get(&sprite_char_id) {
            Some(SwfCharacter::Sprite { symbol_id, .. }) => Some(symbol_id.clone()),
            _ => None,
        };
        if let Some(sym_id) = target_sym_id
            && let Some(sym) = builder.symbols.iter_mut().find(|s| s.id == sym_id)
        {
            prepend_frame_script(
                &mut sym.timeline,
                0,
                script_language_for_swf(builder.swf_version),
                joined,
            );
        } else {
            root_extra_frame0_actions.push(joined);
        }
    }

    // Build root timeline (or split into ordered scene timelines when `DefineSceneAndFrameLabelData` defines >1 scene).
    let root_id: Id = "swf-root".into();
    let mut scenes_ids: Vec<Id> = Vec::new();
    let mut root_snapshots = root_snapshots;
    if root_snapshots.is_empty() {
        root_snapshots.push(FrameSnapshot::default());
    }
    if !root_extra_frame0_actions.is_empty() {
        let mut combined = root_extra_frame0_actions;
        combined.append(&mut root_snapshots[0].actions);
        root_snapshots[0].actions = combined;
    }

    let valid_multi_scenes = normalized_scene_ranges(&builder.scenes_meta, root_snapshots.len());
    if valid_multi_scenes.len() > 1 {
        let mut scene_symbols = Vec::with_capacity(valid_multi_scenes.len());
        for (si, (start, end, sname)) in valid_multi_scenes.into_iter().enumerate() {
            let sid = if si == 0 {
                root_id.clone()
            } else {
                format!("swf-scene-{}", si + 1)
            };
            scenes_ids.push(sid.clone());
            let (timeline, _) =
                builder.build_timeline_from_snapshots(&sid, &root_snapshots[start..end], false);
            scene_symbols.push(Symbol {
                id: sid,
                name: sname,
                folder: String::new(),
                kind: SymbolKind::MovieClip,
                timeline,
                class_name: None,
                linkage_id: None,
            });
        }
        // Place scene symbols first, followed by library symbols.
        let mut all_symbols = scene_symbols;
        all_symbols.append(&mut builder.symbols);
        builder.symbols = all_symbols;
    } else {
        let root_name = valid_multi_scenes
            .first()
            .map(|(_, _, n)| n.clone())
            .unwrap_or_else(|| "Scene 1".into());
        let (root_timeline, _) =
            builder.build_timeline_from_snapshots(&root_id, &root_snapshots, false);
        let root_sym = Symbol {
            id: root_id.clone(),
            name: root_name,
            folder: String::new(),
            kind: SymbolKind::MovieClip,
            timeline: root_timeline,
            class_name: None,
            linkage_id: None,
        };
        builder.symbols.insert(0, root_sym);
    }

    if builder.legacy_cp_warned {
        builder.report.warning(
            "swf_legacy_codepage",
            location,
            "SWF <= 5 non-UTF-8 strings were decoded using legacy Windows-1251/1252 heuristic.",
        );
    }
    if builder.unrecognized_avm1 > 0 {
        builder.report.warning(
            "swf_avm1_partial",
            location,
            format!(
                "{} unrecognized AVM1 action opcode(s) were preserved as diagnostic comments.",
                builder.unrecognized_avm1
            ),
        );
    }
    if builder.unrecognized_avm2 > 0 {
        builder.report.warning(
            "swf_avm2_partial",
            location,
            format!(
                "{} unrecognized AVM2 bytecode instruction(s) stopped method body decoding early.",
                builder.unrecognized_avm2
            ),
        );
    }

    // Collect library folders actually used by symbols.
    let mut folder_set = std::collections::BTreeSet::new();
    for s in &builder.symbols {
        if !s.folder.is_empty() && s.id != root_id {
            folder_set.insert(s.folder.clone());
        }
    }

    let mut project = Project {
        format_version: 1,
        name: project_name,
        width: width_px,
        height: height_px,
        fps,
        background,
        root: root_id.clone(),
        symbols: builder.symbols,
        library_folders: folder_set.into_iter().collect(),
        sources: Vec::new(),
        resources: builder.resources,
        document_class: None,
        scenes: scenes_ids,
    };

    // Wire `ExportAssets`, `SymbolClass`, and decompiled AVM2 AS3 classes + `addFrameScript` timeline scripts!
    apply_linkage_and_abc(
        &mut project,
        &builder.characters,
        &builder.exports,
        &builder.symbol_classes,
        builder.abc_classes,
        location,
        builder.report,
    );

    // Embed provenance summary + original SWF bytes when within the 64 MiB resource / 128 MiB JSON budget.
    embed_swf_provenance_and_source(
        &mut project,
        container,
        inspection,
        builder.resource_bytes_total,
        location,
        builder.report,
    );

    finalize_project_format_version(&mut project);

    // Final validation & 128 MiB JSON budget check.
    match project.to_json() {
        Ok(_) => {
            populate_project_statistics(&project, builder.report);
            Some(project)
        }
        Err(first_err) => {
            // If pretty-printed JSON exceeded 128 MiB solely because `legacy/source.swf` pushed it over,
            // shed `legacy/source.swf` and re-check once!
            let before_len = project.resources.len();
            project
                .resources
                .retain(|r| !r.path.starts_with("legacy/source.swf"));
            if project.resources.len() < before_len {
                builder.report.warning(
                    "swf_source_not_embedded",
                    location,
                    "Original SWF bytes were omitted so the converted project fits within the 128 MiB document limit.",
                );
                finalize_project_format_version(&mut project);
                if project.to_json().is_ok() {
                    populate_project_statistics(&project, builder.report);
                    return Some(project);
                }
            }
            builder.report.error(
                "swf_invalid_project",
                location,
                format!("Converted SWF project failed validation: {first_err}"),
            );
            None
        }
    }
}

impl SwfBuilder<'_> {
    fn register_character_symbol(
        &mut self,
        char_id: u16,
        name: String,
        folder: &str,
        kind: SymbolKind,
        timeline: Timeline,
    ) -> Id {
        let symbol_id = format!("swf-sym-{char_id}");
        self.symbols.push(Symbol {
            id: symbol_id.clone(),
            name,
            folder: folder.to_owned(),
            kind,
            timeline,
            class_name: None,
            linkage_id: None,
        });
        symbol_id
    }

    fn process_timeline_tags(
        &mut self,
        bytes: &[u8],
        scope_uid: &str,
        root_offset: (f64, f64),
    ) -> Vec<FrameSnapshot> {
        let mut snapshots: Vec<FrameSnapshot> = Vec::new();
        let mut active_slots: BTreeMap<u16, ActiveSlot> = BTreeMap::new();
        let mut next_span_id: u32 = 1;
        let mut pending_label: Option<String> = None;
        let mut pending_actions: Vec<String> = Vec::new();
        let mut pending_sounds: Vec<FrameSound> = Vec::new();
        let mut stream_collector: Option<SwfStreamSoundCollector> = None;
        let mut stream_seq: usize = 0;
        let mut saw_pending_frame_mutation = false;

        let mut pos = 0usize;
        while pos + 2 <= bytes.len() {
            let raw_hdr = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
            pos += 2;
            let tag_code = raw_hdr >> 6;
            let short_len = usize::from(raw_hdr & 0x3f);
            let tag_len = if short_len == 0x3f {
                if pos + 4 > bytes.len() {
                    break;
                }
                let l = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4;
                l
            } else {
                short_len
            };
            let end = pos.saturating_add(tag_len).min(bytes.len());
            let payload = &bytes[pos..end];
            pos = end;

            match tag_code {
                0 => break,
                // ShowFrame
                1 => {
                    snapshots.push(FrameSnapshot {
                        slots: active_slots.clone(),
                        label: pending_label.take(),
                        actions: std::mem::take(&mut pending_actions),
                        sounds: std::mem::take(&mut pending_sounds),
                    });
                    saw_pending_frame_mutation = false;
                }
                // DefineShape 1/2/3/4
                2 | 22 | 32 | 83 if scope_uid == "root" => {
                    if let Some((shape_id, decoded)) = decode_define_shape(
                        tag_code,
                        payload,
                        &self.bitmaps,
                        self.location,
                        self.report,
                    ) {
                        let is_pure_vector = decoded.elements.iter().all(element_is_pure_vector);
                        let timeline =
                            single_frame_timeline(&format!("swf-sym-{shape_id}"), decoded.elements);
                        let sym_id = self.register_character_symbol(
                            shape_id,
                            format!("Shape {shape_id}"),
                            "Shapes",
                            SymbolKind::Graphic,
                            timeline,
                        );
                        self.characters.insert(
                            shape_id,
                            SwfCharacter::GraphicSymbol {
                                symbol_id: sym_id,
                                is_morph: false,
                                is_pure_vector,
                            },
                        );
                    }
                }
                // DefineMorphShape 1/2
                46 | 84 if scope_uid == "root" => {
                    if let Some((morph_id, decoded)) = decode_define_morph_shape(
                        tag_code,
                        payload,
                        &self.bitmaps,
                        self.location,
                        self.report,
                    ) {
                        let is_pure_vector =
                            decoded.start_elements.iter().all(element_is_pure_vector)
                                && decoded.end_elements.iter().all(element_is_pure_vector);
                        let sym_id_preview = format!("swf-sym-{morph_id}");
                        let timeline = morph_shape_timeline(
                            &sym_id_preview,
                            decoded.start_elements,
                            decoded.end_elements,
                        );
                        let sym_id = self.register_character_symbol(
                            morph_id,
                            format!("MorphShape {morph_id}"),
                            "Shapes",
                            SymbolKind::Graphic,
                            timeline,
                        );
                        self.characters.insert(
                            morph_id,
                            SwfCharacter::GraphicSymbol {
                                symbol_id: sym_id,
                                is_morph: true,
                                is_pure_vector,
                            },
                        );
                    }
                }
                // JPEGTables
                8 if scope_uid == "root" => {
                    self.jpeg_tables = Some(payload.to_vec());
                }
                // DefineBits, DefineBitsJPEG2/3/4
                6 | 21 | 35 | 90 if scope_uid == "root" => {
                    if self.bitmaps.len() < MAX_AUTHORED_BITMAPS
                        && self.bitmap_rgba_bytes < MAX_BITMAP_BYTES
                    {
                        if let Some((bid, bref)) = decode_jpeg_bitmap_tag(
                            tag_code,
                            payload,
                            self.jpeg_tables.as_deref(),
                            &mut self.resources,
                            &mut self.resource_bytes_total,
                            self.location,
                            self.report,
                        ) {
                            let px_bytes = (bref.width as usize)
                                .saturating_mul(bref.height as usize)
                                .saturating_mul(4);
                            if self.bitmap_rgba_bytes + px_bytes <= MAX_BITMAP_BYTES {
                                self.bitmap_rgba_bytes += px_bytes;
                                self.register_bitmap_character(bid, &bref);
                                self.bitmaps.insert(bid, bref);
                            }
                        }
                    } else {
                        self.report.warning(
                            "swf_media_budget",
                            self.location,
                            "Bitmap skipped because document reached the 250-bitmap / 128 MiB decoded pixel budget.",
                        );
                    }
                }
                // DefineBitsLossless 1/2
                20 | 36 if scope_uid == "root" => {
                    if self.bitmaps.len() < MAX_AUTHORED_BITMAPS
                        && self.bitmap_rgba_bytes < MAX_BITMAP_BYTES
                    {
                        if let Some((bid, bref)) = decode_lossless_bitmap_tag(
                            tag_code,
                            payload,
                            &mut self.resources,
                            &mut self.resource_bytes_total,
                            self.location,
                            self.report,
                        ) {
                            let px_bytes = (bref.width as usize)
                                .saturating_mul(bref.height as usize)
                                .saturating_mul(4);
                            if self.bitmap_rgba_bytes + px_bytes <= MAX_BITMAP_BYTES {
                                self.bitmap_rgba_bytes += px_bytes;
                                self.register_bitmap_character(bid, &bref);
                                self.bitmaps.insert(bid, bref);
                            }
                        }
                    } else {
                        self.report.warning(
                            "swf_media_budget",
                            self.location,
                            "Lossless bitmap skipped because document reached the 250-bitmap / 128 MiB decoded pixel budget.",
                        );
                    }
                }
                // Fonts
                10 if scope_uid == "root" => decode_define_font(payload, &mut self.fonts),
                13 | 62 if scope_uid == "root" => decode_define_font_info(
                    tag_code,
                    payload,
                    self.swf_version,
                    &mut self.fonts,
                    &mut self.legacy_cp_warned,
                ),
                48 | 75 if scope_uid == "root" => decode_define_font2_or_3(
                    tag_code,
                    payload,
                    self.swf_version,
                    &mut self.fonts,
                    &mut self.legacy_cp_warned,
                ),
                88 | 91 if scope_uid == "root" => decode_define_font_name_or_4(
                    tag_code,
                    payload,
                    self.swf_version,
                    &mut self.fonts,
                ),
                // DefineText / DefineText2
                11 | 33 if scope_uid == "root" => {
                    if let Some((tid, decoded)) = decode_define_text(
                        tag_code,
                        payload,
                        &self.fonts,
                        self.location,
                        self.report,
                        &mut self.text_metrics_warned,
                    ) {
                        self.register_text_character(tid, decoded.elements);
                    }
                }
                // DefineEditText
                37 if scope_uid == "root" => {
                    if let Some((tid, decoded)) = decode_define_edit_text(
                        payload,
                        self.swf_version,
                        &self.fonts,
                        self.location,
                        self.report,
                        &mut self.text_metrics_warned,
                        &mut self.legacy_cp_warned,
                    ) {
                        self.register_text_character(tid, decoded.elements);
                    }
                }
                // DefineSound
                14 if scope_uid == "root" => {
                    if let Some((sid, asset)) = decode_define_sound_tag(
                        payload,
                        &mut self.resources,
                        &mut self.resource_bytes_total,
                        self.location,
                        self.report,
                    ) {
                        self.sounds.insert(sid, asset);
                    }
                }
                // StartSound
                15 => {
                    let mut r = SwfSlice::new(payload);
                    if let Some(sid) = r.u16()
                        && let Some(asset) = self.sounds.get(&sid)
                        && let Some(fs) =
                            parse_sound_info(&mut r, asset, self.location, self.report)
                    {
                        pending_sounds.push(fs);
                        saw_pending_frame_mutation = true;
                    }
                }
                // SoundStreamHead / SoundStreamHead2
                18 | 45 => {
                    if let Some(prev) = stream_collector.take() {
                        stream_seq += 1;
                        if let Some((sf, fs)) = finalize_stream_sound(
                            &format!("{scope_uid}-{stream_seq}"),
                            prev,
                            &mut self.resources,
                            &mut self.resource_bytes_total,
                            self.location,
                            self.report,
                        ) {
                            attach_stream_sound(&mut snapshots, &mut pending_sounds, sf, fs);
                        }
                    }
                    stream_collector = parse_sound_stream_head(payload);
                }
                // SoundStreamBlock
                19 => {
                    if let Some(col) = stream_collector.as_mut() {
                        append_sound_stream_block(col, snapshots.len() as u32, payload);
                    }
                }
                // DefineButton / DefineButton2
                7 | 34 if scope_uid == "root" => {
                    self.decode_define_button(tag_code, payload);
                }
                // DefineButtonCxform
                23 if scope_uid == "root" => {
                    self.decode_button_cxform(payload);
                }
                // DefineButtonSound
                17 if scope_uid == "root" => {
                    self.decode_button_sound(payload);
                }
                // DefineSprite
                39 if scope_uid == "root" && payload.len() >= 4 => {
                    let sprite_id = u16::from_le_bytes([payload[0], payload[1]]);
                    let sym_uid = format!("swf-sym-{sprite_id}");
                    let child_snaps =
                        self.process_timeline_tags(&payload[4..], &sym_uid, (0.0, 0.0));
                    let snaps = if child_snaps.is_empty() {
                        vec![FrameSnapshot::default()]
                    } else {
                        child_snaps
                    };
                    let (timeline, is_pure_vector) =
                        self.build_timeline_from_snapshots(&sym_uid, &snaps, false);
                    let sym_id = self.register_character_symbol(
                        sprite_id,
                        format!("Sprite {sprite_id}"),
                        "Sprites",
                        SymbolKind::MovieClip,
                        timeline,
                    );
                    self.characters.insert(
                        sprite_id,
                        SwfCharacter::Sprite {
                            symbol_id: sym_id,
                            is_pure_vector,
                        },
                    );
                }
                // PlaceObject, PlaceObject2, PlaceObject3
                4 | 26 | 70 => {
                    self.apply_place_object(
                        tag_code,
                        payload,
                        root_offset,
                        &mut active_slots,
                        &mut next_span_id,
                    );
                    saw_pending_frame_mutation = true;
                }
                // RemoveObject (5), RemoveObject2 (28)
                5 => {
                    let mut r = SwfSlice::new(payload);
                    let _cid = r.u16();
                    if let Some(depth) = r.u16() {
                        active_slots.remove(&depth);
                        saw_pending_frame_mutation = true;
                    }
                }
                28 => {
                    let mut r = SwfSlice::new(payload);
                    if let Some(depth) = r.u16() {
                        active_slots.remove(&depth);
                        saw_pending_frame_mutation = true;
                    }
                }
                // FrameLabel
                43 => {
                    let mut r = SwfSlice::new(payload);
                    if let Some((lbl, cp)) = r.c_string(self.swf_version) {
                        self.legacy_cp_warned |= cp;
                        if !lbl.is_empty() {
                            pending_label = Some(lbl);
                            saw_pending_frame_mutation = true;
                        }
                    }
                }
                // DoAction
                12 => {
                    let (src, unrec) =
                        decompile_actions(payload, self.swf_version, &mut self.legacy_cp_warned);
                    self.unrecognized_avm1 += unrec;
                    if !src.trim().is_empty() {
                        pending_actions.push(src);
                        saw_pending_frame_mutation = true;
                    }
                }
                // DoInitAction
                59 if scope_uid == "root" && payload.len() >= 2 => {
                    let sprite_id = u16::from_le_bytes([payload[0], payload[1]]);
                    let (src, unrec) = decompile_actions(
                        &payload[2..],
                        self.swf_version,
                        &mut self.legacy_cp_warned,
                    );
                    self.unrecognized_avm1 += unrec;
                    if !src.trim().is_empty() {
                        self.init_actions.entry(sprite_id).or_default().push(src);
                    }
                }
                // ExportAssets
                56 if scope_uid == "root" => {
                    let mut r = SwfSlice::new(payload);
                    let count = usize::from(r.u16().unwrap_or(0));
                    for _ in 0..count {
                        let Some(cid) = r.u16() else { break };
                        let Some((name, cp)) = r.c_string(self.swf_version) else {
                            break;
                        };
                        self.legacy_cp_warned |= cp;
                        if !name.is_empty() {
                            self.exports.push((cid, name));
                        }
                    }
                }
                // SymbolClass
                76 if scope_uid == "root" => {
                    let mut r = SwfSlice::new(payload);
                    let count = usize::from(r.u16().unwrap_or(0));
                    for _ in 0..count {
                        let Some(cid) = r.u16() else { break };
                        let Some((cname, _)) = r.c_string(9) else {
                            break;
                        };
                        if !cname.is_empty() {
                            self.symbol_classes.push((cid, cname));
                        }
                    }
                }
                // DoABC / DoABC2
                72 | 82 if scope_uid == "root" => {
                    if let Some(bundle) = decompile_do_abc_tag(tag_code, payload) {
                        self.unrecognized_avm2 += bundle.unrecognized_opcodes;
                        self.abc_classes.extend(bundle.classes);
                    } else {
                        self.report.warning(
                            "swf_avm2_partial",
                            self.location,
                            "An AVM2 DoABC bytecode block could not be parsed cleanly.",
                        );
                    }
                }
                // DefineSceneAndFrameLabelData
                86 if scope_uid == "root" => {
                    let mut r = SwfSlice::new(payload);
                    if let Some(scene_count) = r.encoded_u32() {
                        for _ in 0..scene_count.min(256) {
                            let Some(offset) = r.encoded_u32() else { break };
                            let Some((sname, _)) = r.c_string(9) else {
                                break;
                            };
                            self.scenes_meta.push((offset, sname));
                        }
                    }
                }
                // DefineVideoStream (60) / VideoFrame (61)
                60 if scope_uid == "root" && payload.len() >= 8 => {
                    let vid = u16::from_le_bytes([payload[0], payload[1]]);
                    let vw = f64::from(u16::from_le_bytes([payload[4], payload[5]])).max(16.0);
                    let vh = f64::from(u16::from_le_bytes([payload[6], payload[7]])).max(16.0);
                    let el = Element {
                        id: format!("swf-video-{vid}-el"),
                        name: String::new(),
                        transform: Transform::default(),
                        kind: ElementKind::Video {
                            width: vw,
                            height: vh,
                        },
                        ..Element::default()
                    };
                    let tl = single_frame_timeline(&format!("swf-sym-{vid}"), vec![el]);
                    let sym_id = self.register_character_symbol(
                        vid,
                        format!("Video {vid}"),
                        "Media",
                        SymbolKind::MovieClip,
                        tl,
                    );
                    self.characters.insert(
                        vid,
                        SwfCharacter::Sprite {
                            symbol_id: sym_id,
                            is_pure_vector: false,
                        },
                    );
                    self.report.warning(
                        "swf_video_viewport",
                        format!("{}#video:{vid}", self.location),
                        "Embedded SWF video stream imported as a Video viewport placeholder; raw codec frames remain in the source SWF.",
                    );
                }
                _ => {}
            }
        }

        if snapshots.is_empty() && saw_pending_frame_mutation {
            snapshots.push(FrameSnapshot {
                slots: active_slots,
                label: pending_label,
                actions: pending_actions,
                sounds: pending_sounds,
            });
        } else if !snapshots.is_empty()
            && (!pending_actions.is_empty()
                || !pending_sounds.is_empty()
                || pending_label.is_some())
            && let Some(last) = snapshots.last_mut()
        {
            if last.label.is_none() {
                last.label = pending_label;
            }
            last.actions.extend(pending_actions);
            last.sounds.extend(pending_sounds);
        }

        if let Some(col) = stream_collector {
            stream_seq += 1;
            if let Some((sf, fs)) = finalize_stream_sound(
                &format!("{scope_uid}-{stream_seq}"),
                col,
                &mut self.resources,
                &mut self.resource_bytes_total,
                self.location,
                self.report,
            ) {
                let mut extra = Vec::new();
                attach_stream_sound(&mut snapshots, &mut extra, sf, fs);
                if !extra.is_empty()
                    && let Some(first) = snapshots.first_mut()
                {
                    first.sounds.extend(extra);
                }
            }
        }

        snapshots
    }

    fn register_bitmap_character(&mut self, bitmap_id: u16, bref: &SwfBitmapRef) {
        // SWF occasionally places a bitmap character ID directly via `PlaceObject*`.
        let el = Element {
            id: format!("swf-bmpchar-{bitmap_id}-el"),
            name: String::new(),
            transform: Transform::default(),
            kind: ElementKind::Bitmap {
                resource_id: bref.resource_id.clone(),
                width: bref.width,
                height: bref.height,
                smoothing: true,
            },
            ..Element::default()
        };
        let tl = single_frame_timeline(&format!("swf-sym-{bitmap_id}"), vec![el]);
        let sym_id = self.register_character_symbol(
            bitmap_id,
            format!("Bitmap {bitmap_id}"),
            "Bitmaps",
            SymbolKind::Graphic,
            tl,
        );
        self.characters.insert(
            bitmap_id,
            SwfCharacter::GraphicSymbol {
                symbol_id: sym_id,
                is_morph: false,
                is_pure_vector: false,
            },
        );
    }

    fn register_text_character(&mut self, text_id: u16, elements: Vec<Element>) {
        if elements.len() == 1 && matches!(elements[0].kind, ElementKind::TextBox { .. }) {
            let elem = elements.into_iter().next().unwrap();
            let tl = single_frame_timeline(&format!("swf-sym-{text_id}"), vec![elem.clone()]);
            let fallback_id = self.register_character_symbol(
                text_id,
                format!("Text {text_id}"),
                "Text",
                SymbolKind::Graphic,
                tl,
            );
            self.characters.insert(
                text_id,
                SwfCharacter::InlineText {
                    element: Box::new(elem),
                    fallback_symbol_id: Some(fallback_id),
                },
            );
        } else {
            let is_pure_vector = elements.iter().all(element_is_pure_vector);
            let tl = single_frame_timeline(&format!("swf-sym-{text_id}"), elements);
            let sym_id = self.register_character_symbol(
                text_id,
                format!("Text {text_id}"),
                "Text",
                SymbolKind::Graphic,
                tl,
            );
            self.characters.insert(
                text_id,
                SwfCharacter::GraphicSymbol {
                    symbol_id: sym_id,
                    is_morph: false,
                    is_pure_vector,
                },
            );
        }
    }

    fn decode_define_button(&mut self, tag_code: u16, payload: &[u8]) {
        let mut r = SwfSlice::new(payload);
        let Some(button_id) = r.u16() else { return };
        let is_button2 = tag_code == 34;
        let action_abs_offset = if is_button2 {
            let _flags = r.u8().unwrap_or(0);
            let off_pos = r.pos();
            let rel = usize::from(r.u16().unwrap_or(0));
            if rel == 0 { None } else { Some(off_pos + rel) }
        } else {
            None
        };

        let mut records = Vec::new();
        for _ in 0..1024 {
            let Some(flags) = r.u8() else { break };
            if flags == 0 {
                break;
            }
            let Some(character_id) = r.u16() else { break };
            let Some(place_depth) = r.u16() else { break };
            let Some(matrix) = r.matrix_px() else { break };
            let (color_transform, alpha) = if is_button2 {
                r.cxform(true)
                    .unwrap_or_else(|| (ColorTransform::default(), 1.0))
            } else {
                (ColorTransform::default(), 1.0)
            };
            let mut filters = Vec::new();
            let mut blend_mode = neoflash_core::BlendMode::Normal;
            if is_button2 && (flags & 0x10) != 0 {
                let (parsed, unmodeled) = parse_swf_filter_list(&mut r);
                filters = parsed;
                if unmodeled {
                    self.warn_filter_once();
                }
            }
            if is_button2 && (flags & 0x20) != 0 {
                let blend = r.u8().unwrap_or(0);
                if let Some(bm) = parse_swf_blend_mode(blend) {
                    blend_mode = bm;
                } else if blend > 1 {
                    self.warn_blend_once();
                }
            }
            records.push(ButtonRecord {
                states: flags & 0x0F,
                character_id,
                place_depth,
                matrix,
                color_transform,
                alpha,
                blend_mode,
                filters,
            });
        }

        let handlers = if is_button2 {
            if let Some(off) = action_abs_offset
                && off < payload.len()
            {
                let hs = parse_and_decompile_button2_actions(
                    &payload[off..],
                    self.swf_version,
                    &mut self.legacy_cp_warned,
                );
                for h in &hs {
                    self.unrecognized_avm1 += h.unrecognized_opcodes;
                }
                hs
            } else {
                Vec::new()
            }
        } else {
            let rest = r.rest();
            if rest.is_empty() {
                Vec::new()
            } else {
                let (src, unrec) =
                    decompile_actions(rest, self.swf_version, &mut self.legacy_cp_warned);
                self.unrecognized_avm1 += unrec;
                if src.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![DecompiledObjectHandler {
                        script: ObjectScript {
                            event: neoflash_core::ObjectEvent::Click,
                            script: Script {
                                language: script_language_for_swf(self.swf_version),
                                source: src,
                            },
                        },
                        approximated_event: false,
                        unrecognized_opcodes: unrec,
                    }]
                }
            }
        };

        self.pending_buttons.insert(
            button_id,
            PendingButton {
                button_id,
                records,
                handlers,
            },
        );
    }

    fn decode_button_cxform(&mut self, payload: &[u8]) {
        let mut r = SwfSlice::new(payload);
        let Some(button_id) = r.u16() else { return };
        let mut list = Vec::new();
        while r.remaining() > 0 {
            let Some(cx) = r.cxform(false) else { break };
            list.push(cx);
        }
        if !list.is_empty() {
            self.button_cxforms.insert(button_id, list);
        }
    }

    fn decode_button_sound(&mut self, payload: &[u8]) {
        let mut r = SwfSlice::new(payload);
        let Some(button_id) = r.u16() else { return };
        let mut attached = Vec::new();
        // 4 button sound slots: 0=OverUpToIdle(rollOut), 1=IdleToOverUp(Over->frame 1),
        // 2=OverUpToOverDown(Down->frame 2), 3=OverDownToOverUp(release->frame 0/2).
        for state_slot in 0..4u32 {
            let Some(sound_id) = r.u16() else { break };
            if sound_id == 0 {
                continue;
            }
            if let Some(asset) = self.sounds.get(&sound_id)
                && let Some(fs) = parse_sound_info(&mut r, asset, self.location, self.report)
            {
                let target_frame = match state_slot {
                    1 => 1,
                    2 => 2,
                    _ => 0,
                };
                attached.push((target_frame, fs));
            } else {
                break;
            }
        }
        if !attached.is_empty() {
            self.button_sounds.insert(button_id, attached);
        }
    }

    fn finalize_pending_buttons(&mut self) {
        let mut buttons: Vec<PendingButton> =
            self.pending_buttons.drain().map(|(_, v)| v).collect();
        buttons.sort_by_key(|b| b.button_id);

        for mut btn in buttons {
            if let Some(cx_list) = self.button_cxforms.get(&btn.button_id) {
                for (rec, &(ct, alpha)) in btn.records.iter_mut().zip(cx_list.iter()) {
                    rec.color_transform = ct;
                    rec.alpha = alpha;
                }
            }
            let sym_uid = format!("swf-sym-{}", btn.button_id);
            let mut snaps = vec![FrameSnapshot::default(); 4];
            let mut next_span = 1u32;
            for rec in &btn.records {
                for (state_idx, snap) in snaps.iter_mut().enumerate().take(4) {
                    if (rec.states & (1 << state_idx)) != 0 {
                        let span_id = next_span;
                        next_span += 1;
                        snap.slots.insert(
                            rec.place_depth,
                            ActiveSlot {
                                span_id,
                                character_id: rec.character_id,
                                matrix: rec.matrix,
                                color_transform: rec.color_transform,
                                alpha: rec.alpha,
                                ratio: None,
                                name: String::new(),
                                clip_depth: None,
                                visible: true,
                                cache_as_bitmap: false,
                                blend_mode: rec.blend_mode,
                                filters: rec.filters.clone(),
                                handlers: Vec::new(),
                            },
                        );
                    }
                }
            }
            if let Some(bsounds) = self.button_sounds.get(&btn.button_id) {
                for (f, fs) in bsounds {
                    if let Some(snap) = snaps.get_mut(*f as usize) {
                        snap.sounds.push(fs.clone());
                    }
                }
            }
            let (timeline, is_pure_vector) =
                self.build_timeline_from_snapshots(&sym_uid, &snaps, true);
            let sym_id = self.register_character_symbol(
                btn.button_id,
                format!("Button {}", btn.button_id),
                "Buttons",
                SymbolKind::Button,
                timeline,
            );
            self.characters.insert(
                btn.button_id,
                SwfCharacter::Button {
                    symbol_id: sym_id,
                    instance_scripts: btn.handlers,
                    is_pure_vector,
                },
            );
        }
    }

    fn apply_place_object(
        &mut self,
        tag_code: u16,
        payload: &[u8],
        root_offset: (f64, f64),
        active_slots: &mut BTreeMap<u16, ActiveSlot>,
        next_span_id: &mut u32,
    ) {
        let mut r = SwfSlice::new(payload);
        if tag_code == 4 {
            let Some(character_id) = r.u16() else { return };
            let Some(depth) = r.u16() else { return };
            let Some(mut matrix) = r.matrix_px() else {
                return;
            };
            matrix[4] += root_offset.0;
            matrix[5] += root_offset.1;
            let (color_transform, alpha) = if r.remaining() > 0 {
                r.cxform(false)
                    .unwrap_or_else(|| (ColorTransform::default(), 1.0))
            } else {
                (ColorTransform::default(), 1.0)
            };
            let span_id = *next_span_id;
            *next_span_id += 1;
            active_slots.insert(
                depth,
                ActiveSlot {
                    span_id,
                    character_id,
                    matrix,
                    color_transform,
                    alpha,
                    ratio: None,
                    name: String::new(),
                    clip_depth: None,
                    visible: true,
                    cache_as_bitmap: false,
                    blend_mode: neoflash_core::BlendMode::Normal,
                    filters: Vec::new(),
                    handlers: Vec::new(),
                },
            );
            return;
        }

        let Some(flags1) = r.u8() else { return };
        let flags2 = if tag_code == 70 {
            r.u8().unwrap_or(0)
        } else {
            0
        };
        let Some(depth) = r.u16() else { return };

        let has_clip_actions = (flags1 & 0x80) != 0;
        let has_clip_depth = (flags1 & 0x40) != 0;
        let has_name = (flags1 & 0x20) != 0;
        let has_ratio = (flags1 & 0x10) != 0;
        let has_cxform = (flags1 & 0x08) != 0;
        let has_matrix = (flags1 & 0x04) != 0;
        let has_character = (flags1 & 0x02) != 0;
        let is_move = (flags1 & 0x01) != 0;

        if tag_code == 70 && ((flags2 & 0x08) != 0 || ((flags2 & 0x10) != 0 && has_character)) {
            let _class_name = r.c_string(self.swf_version);
        }
        let new_char = if has_character { r.u16() } else { None };
        let new_matrix = if has_matrix {
            r.matrix_px().map(|mut m| {
                m[4] += root_offset.0;
                m[5] += root_offset.1;
                m
            })
        } else {
            None
        };
        let new_cxform = if has_cxform { r.cxform(true) } else { None };
        let new_ratio = if has_ratio { r.u16() } else { None };
        let new_name = if has_name {
            r.c_string(self.swf_version).map(|(s, cp)| {
                self.legacy_cp_warned |= cp;
                s
            })
        } else {
            None
        };
        let new_clip_depth = if has_clip_depth { r.u16() } else { None };

        let mut new_visible: Option<bool> = None;
        let mut new_filters: Option<Vec<neoflash_core::FilterSpec>> = None;
        let mut new_blend_mode: Option<neoflash_core::BlendMode> = None;
        let mut new_cache_as_bitmap: Option<bool> = None;
        if tag_code == 70 {
            if (flags2 & 0x01) != 0 {
                let (parsed, unmodeled) = parse_swf_filter_list(&mut r);
                new_filters = Some(parsed);
                if unmodeled {
                    self.warn_filter_once();
                }
            }
            if (flags2 & 0x02) != 0 {
                let blend = r.u8().unwrap_or(0);
                if let Some(bm) = parse_swf_blend_mode(blend) {
                    new_blend_mode = Some(bm);
                } else if blend > 1 {
                    self.warn_blend_once();
                }
            }
            if (flags2 & 0x04) != 0 {
                new_cache_as_bitmap = r.u8().map(|v| v != 0);
            }
            if (flags2 & 0x20) != 0 {
                new_visible = r.u8().map(|v| v != 0);
            }
            if (flags2 & 0x40) != 0 {
                let _bg_rgba = r.rgba();
            }
        }

        let new_handlers = if has_clip_actions {
            let hs = parse_and_decompile_clip_actions(
                &mut r,
                self.swf_version,
                &mut self.legacy_cp_warned,
            );
            for h in &hs {
                self.unrecognized_avm1 += h.unrecognized_opcodes;
                if h.approximated_event && !self.clip_event_warned {
                    self.clip_event_warned = true;
                    self.report.warning(
                        "swf_clip_event_approximated",
                        self.location,
                        "Non-core AVM1 clip/button events (e.g. rollOver, keyPress, mouseMove) were preserved with event-header comments on ObjectScript.",
                    );
                }
            }
            Some(hs)
        } else {
            None
        };

        if is_move && let Some(existing) = active_slots.get_mut(&depth) {
            if let Some(cid) = new_char
                && cid != existing.character_id
            {
                existing.character_id = cid;
                existing.span_id = *next_span_id;
                *next_span_id += 1;
            }
            if let Some(m) = new_matrix {
                existing.matrix = m;
            }
            if let Some((ct, a)) = new_cxform {
                existing.color_transform = ct;
                existing.alpha = a;
            }
            if let Some(rat) = new_ratio {
                existing.ratio = Some(rat);
            }
            if let Some(nm) = new_name {
                existing.name = nm;
            }
            if let Some(cd) = new_clip_depth {
                existing.clip_depth = if cd > 0 { Some(cd) } else { None };
            }
            if let Some(vis) = new_visible {
                existing.visible = vis;
            }
            if let Some(cab) = new_cache_as_bitmap {
                existing.cache_as_bitmap = cab;
            }
            if let Some(bm) = new_blend_mode {
                existing.blend_mode = bm;
            }
            if let Some(fl) = new_filters {
                existing.filters = fl;
            }
            if let Some(hs) = new_handlers {
                existing.handlers = hs;
            }
            return;
        }

        let Some(character_id) = new_char else {
            return;
        };
        let matrix = new_matrix.unwrap_or([1.0, 0.0, 0.0, 1.0, root_offset.0, root_offset.1]);
        let (color_transform, alpha) =
            new_cxform.unwrap_or_else(|| (ColorTransform::default(), 1.0));
        let span_id = *next_span_id;
        *next_span_id += 1;
        active_slots.insert(
            depth,
            ActiveSlot {
                span_id,
                character_id,
                matrix,
                color_transform,
                alpha,
                ratio: new_ratio,
                name: new_name.unwrap_or_default(),
                clip_depth: new_clip_depth.filter(|&cd| cd > 0),
                visible: new_visible.unwrap_or(true),
                cache_as_bitmap: new_cache_as_bitmap.unwrap_or(false),
                blend_mode: new_blend_mode.unwrap_or(neoflash_core::BlendMode::Normal),
                filters: new_filters.unwrap_or_default(),
                handlers: new_handlers.unwrap_or_default(),
            },
        );
    }

    fn build_timeline_from_snapshots(
        &mut self,
        sym_uid: &str,
        snapshots: &[FrameSnapshot],
        is_button: bool,
    ) -> (Timeline, bool) {
        let frame_count = if is_button {
            4u32
        } else {
            (snapshots.len() as u32).max(1)
        };
        let mut all_depths = std::collections::BTreeSet::new();
        for snap in snapshots {
            for &d in snap.slots.keys() {
                all_depths.insert(d);
            }
        }

        let mut layers: Vec<Layer> = Vec::new();
        let mut depth_to_layer_idx: BTreeMap<u16, usize> = BTreeMap::new();
        let mut depth_clip_targets: BTreeMap<u16, Option<u16>> = BTreeMap::new();
        let mut depth_is_pure_vector: BTreeMap<u16, bool> = BTreeMap::new();
        let mut all_layers_pure_vector = true;

        for &depth in &all_depths {
            let mut keyframes: Vec<Keyframe> = Vec::new();
            let mut prev_slot: Option<&ActiveSlot> = None;
            let mut uniform_clip: Option<Option<u16>> = None;
            let mut clip_consistent = true;
            let mut layer_pure_vector = true;

            for (f_idx, snap) in snapshots.iter().enumerate() {
                let frame = f_idx as u32;
                let cur_slot = snap.slots.get(&depth);
                if let Some(slot) = cur_slot {
                    match uniform_clip {
                        None => uniform_clip = Some(slot.clip_depth),
                        Some(prev_cd) if prev_cd != slot.clip_depth => clip_consistent = false,
                        _ => {}
                    }
                }
                let unchanged = match (prev_slot, cur_slot) {
                    (None, None) => frame > 0,
                    (Some(a), Some(b)) => a.same_visual_state(b),
                    _ => false,
                };
                if unchanged {
                    continue;
                }
                prev_slot = cur_slot;

                let mut elements = Vec::new();
                if let Some(slot) = cur_slot
                    && self.total_elements < MAX_AUTHORED_ELEMENTS
                    && let Some((el, el_vec)) =
                        self.instantiate_slot(sym_uid, depth, slot, is_button)
                {
                    if !el_vec {
                        layer_pure_vector = false;
                        all_layers_pure_vector = false;
                    }
                    self.total_elements += 1;
                    elements.push(el);
                }

                keyframes.push(Keyframe {
                    frame,
                    elements,
                    tween: None,
                    label: String::new(),
                    ..Default::default()
                });
            }

            if keyframes.is_empty() {
                keyframes.push(Keyframe {
                    frame: 0,
                    ..Default::default()
                });
            }

            // Connect consecutive 1-frame step poses of the same continuous symbol instance with Classic tweens
            // and collapse smooth linear/eased runs into start + end keyframes with recovered Easing.
            if !is_button {
                recover_classic_tween_spans(&mut keyframes);
            }

            let layer_idx = layers.len();
            let layer_id = format!("{sym_uid}-d{depth}");
            layers.push(Layer {
                id: layer_id,
                name: format!("Layer {}", layer_idx + 1),
                visible: true,
                locked: false,
                keyframes,
                properties: LayerProperties::default(),
            });
            depth_to_layer_idx.insert(depth, layer_idx);
            depth_clip_targets.insert(
                depth,
                if clip_consistent {
                    uniform_clip.flatten()
                } else {
                    None
                },
            );
            depth_is_pure_vector.insert(depth, layer_pure_vector);
        }

        // Resolve valid vector SWF `clip_depth` spans into NeoFlash `LayerRole::Mask` + `properties.mask`.
        if !is_button {
            for (&mask_depth, &clip_opt) in &depth_clip_targets {
                let Some(clip_end) = clip_opt else {
                    // Check if any snapshot on `mask_depth` had an inconsistent `clip_depth`
                    if snapshots.iter().any(|s| {
                        s.slots
                            .get(&mask_depth)
                            .is_some_and(|sl| sl.clip_depth.is_some())
                    }) {
                        self.report.warning(
                            "swf_mask_approximated",
                            format!("{}#{sym_uid}:depth-{mask_depth}", self.location),
                            "Time-varying SWF clip_depth mask imported as a normal visible layer.",
                        );
                    }
                    continue;
                };
                let mask_lidx = depth_to_layer_idx[&mask_depth];
                let mask_is_vec = depth_is_pure_vector
                    .get(&mask_depth)
                    .copied()
                    .unwrap_or(false);
                if !mask_is_vec
                    || layers[mask_lidx].properties.mask.is_some()
                    || clip_end <= mask_depth
                {
                    self.report.warning(
                        "swf_mask_approximated",
                        format!("{}#{sym_uid}:depth-{mask_depth}", self.location),
                        "Non-vector or nested SWF clip_depth mask imported as a normal layer to satisfy authored mask constraints.",
                    );
                    continue;
                }
                let masked_indices: Vec<usize> = depth_to_layer_idx
                    .range((
                        std::ops::Bound::Excluded(mask_depth),
                        std::ops::Bound::Included(clip_end),
                    ))
                    .map(|(_, &idx)| idx)
                    .filter(|&idx| {
                        layers[idx].properties.role == LayerRole::Normal
                            && layers[idx].properties.mask.is_none()
                            && !depth_clip_targets
                                .values()
                                .flatten()
                                .any(|_| layers[idx].properties.role == LayerRole::Mask)
                    })
                    .collect();
                if masked_indices.is_empty() {
                    continue;
                }
                let mask_id = layers[mask_lidx].id.clone();
                layers[mask_lidx].properties.role = LayerRole::Mask;
                all_layers_pure_vector = false;
                for idx in masked_indices {
                    layers[idx].properties.mask = Some(mask_id.clone());
                }
            }
        }

        // Dedicated Sound layers for timeline/button sounds
        let max_simultaneous_sounds = snapshots.iter().map(|s| s.sounds.len()).max().unwrap_or(0);
        for track in 0..max_simultaneous_sounds {
            let mut skeys = Vec::new();
            for (f_idx, snap) in snapshots.iter().enumerate() {
                if let Some(fs) = snap.sounds.get(track) {
                    skeys.push(Keyframe {
                        frame: f_idx as u32,
                        sound: Some(fs.clone()),
                        ..Default::default()
                    });
                }
            }
            if !skeys.is_empty() {
                if skeys[0].frame != 0 {
                    skeys.insert(
                        0,
                        Keyframe {
                            frame: 0,
                            ..Default::default()
                        },
                    );
                }
                layers.push(Layer {
                    id: format!("{sym_uid}-sound-{}", track + 1),
                    name: if track == 0 {
                        "Sound".into()
                    } else {
                        format!("Sound {}", track + 1)
                    },
                    visible: true,
                    locked: false,
                    keyframes: skeys,
                    properties: LayerProperties::default(),
                });
            }
        }

        // Dedicated Labels layer when frame labels exist
        if !is_button && snapshots.iter().any(|s| s.label.is_some()) {
            let mut lkeys = Vec::new();
            for (f_idx, snap) in snapshots.iter().enumerate() {
                if let Some(lbl) = &snap.label {
                    lkeys.push(Keyframe {
                        frame: f_idx as u32,
                        label: lbl.clone(),
                        ..Default::default()
                    });
                }
            }
            if !lkeys.is_empty() {
                if lkeys[0].frame != 0 {
                    lkeys.insert(
                        0,
                        Keyframe {
                            frame: 0,
                            ..Default::default()
                        },
                    );
                }
                layers.push(Layer {
                    id: format!("{sym_uid}-labels"),
                    name: "Labels".into(),
                    visible: true,
                    locked: false,
                    keyframes: lkeys,
                    properties: LayerProperties::default(),
                });
            }
        }

        if layers.is_empty() {
            layers.push(Layer {
                id: format!("{sym_uid}-layer-1"),
                name: "Layer 1".into(),
                visible: true,
                locked: false,
                keyframes: vec![Keyframe {
                    frame: 0,
                    ..Default::default()
                }],
                properties: LayerProperties::default(),
            });
        }

        // Frame scripts
        let mut scripts = Vec::new();
        if !is_button {
            let lang = script_language_for_swf(self.swf_version);
            for (f_idx, snap) in snapshots.iter().enumerate() {
                if !snap.actions.is_empty() {
                    let source = snap.actions.join("\n\n");
                    if !source.trim().is_empty() {
                        scripts.push(FrameScript {
                            frame: f_idx as u32,
                            script: Script {
                                language: lang,
                                source,
                            },
                        });
                    }
                }
            }
        }

        (
            Timeline {
                frame_count,
                layers,
                scripts,
                guides: Vec::new(),
            },
            all_layers_pure_vector,
        )
    }

    fn instantiate_slot(
        &mut self,
        sym_uid: &str,
        depth: u16,
        slot: &ActiveSlot,
        is_button: bool,
    ) -> Option<(Element, bool)> {
        let mut transform = Transform::from_matrix(slot.matrix)?;
        transform.color_transform = slot.color_transform;
        transform.alpha = if slot.visible {
            slot.alpha.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let elem_id = format!("swf-el-{sym_uid}-d{depth}-s{}", slot.span_id);
        let mut scripts: Vec<ObjectScript> =
            slot.handlers.iter().map(|h| h.script.clone()).collect();
        let filters = if is_button {
            Vec::new()
        } else {
            slot.filters.clone()
        };
        let has_filters = !filters.is_empty();
        let blend_mode = slot.blend_mode;
        let cache_as_bitmap = slot.cache_as_bitmap;

        match self.characters.get(&slot.character_id)? {
            SwfCharacter::InlineText {
                element,
                fallback_symbol_id,
            } => {
                // Compose placement matrix with the TextBox's local transform so text sits directly on the timeline!
                let composed = mul_matrix(slot.matrix, transform_to_matrix(element.transform));
                if let Some(mut tf) = Transform::from_matrix(composed) {
                    tf.color_transform = slot.color_transform;
                    tf.alpha = transform.alpha;
                    let name = if !slot.name.is_empty() {
                        slot.name.clone()
                    } else {
                        element.name.clone()
                    };
                    Some((
                        Element {
                            id: elem_id,
                            name,
                            transform: tf,
                            kind: element.kind.clone(),
                            scripts,
                            visible: slot.visible,
                            cache_as_bitmap,
                            blend_mode,
                            filters,
                            ..Element::default()
                        },
                        false,
                    ))
                } else {
                    fallback_symbol_id.as_ref().map(|fb| {
                        (
                            Element {
                                id: elem_id,
                                name: slot.name.clone(),
                                transform,
                                kind: ElementKind::Instance {
                                    symbol_id: fb.clone(),
                                    playback: InstancePlayback::SingleFrame(0),
                                },
                                scripts,
                                visible: slot.visible,
                                cache_as_bitmap,
                                blend_mode,
                                filters,
                                ..Element::default()
                            },
                            false,
                        )
                    })
                }
            }
            SwfCharacter::GraphicSymbol {
                symbol_id,
                is_morph,
                is_pure_vector,
            } => {
                let playback = if *is_morph {
                    let r = u32::from(slot.ratio.unwrap_or(0));
                    let morph_frame = ((r * 100 + 32767) / 65535).min(100);
                    InstancePlayback::SingleFrame(morph_frame)
                } else {
                    InstancePlayback::SingleFrame(0)
                };
                Some((
                    Element {
                        id: elem_id,
                        name: slot.name.clone(),
                        transform,
                        kind: ElementKind::Instance {
                            symbol_id: symbol_id.clone(),
                            playback,
                        },
                        scripts,
                        visible: slot.visible,
                        cache_as_bitmap,
                        blend_mode,
                        filters,
                        ..Element::default()
                    },
                    *is_pure_vector
                        && !has_filters
                        && blend_mode == neoflash_core::BlendMode::Normal,
                ))
            }
            SwfCharacter::Button {
                symbol_id,
                instance_scripts,
                is_pure_vector,
            } => {
                for h in instance_scripts {
                    if h.approximated_event && !self.clip_event_warned {
                        self.clip_event_warned = true;
                        self.report.warning(
                            "swf_clip_event_approximated",
                            self.location,
                            "Non-core AVM1 clip/button events (e.g. rollOver, keyPress, mouseMove) were preserved with event-header comments on ObjectScript.",
                        );
                    }
                    scripts.push(h.script.clone());
                }
                Some((
                    Element {
                        id: elem_id,
                        name: slot.name.clone(),
                        transform,
                        kind: ElementKind::Instance {
                            symbol_id: symbol_id.clone(),
                            playback: InstancePlayback::Independent,
                        },
                        scripts,
                        visible: slot.visible,
                        cache_as_bitmap,
                        blend_mode,
                        filters,
                        ..Element::default()
                    },
                    *is_pure_vector
                        && !has_filters
                        && blend_mode == neoflash_core::BlendMode::Normal,
                ))
            }
            SwfCharacter::Sprite {
                symbol_id,
                is_pure_vector,
            } => Some((
                Element {
                    id: elem_id,
                    name: slot.name.clone(),
                    transform,
                    kind: ElementKind::Instance {
                        symbol_id: symbol_id.clone(),
                        playback: InstancePlayback::Independent,
                    },
                    scripts,
                    visible: slot.visible,
                    cache_as_bitmap,
                    blend_mode,
                    filters,
                    ..Element::default()
                },
                *is_pure_vector && !has_filters && blend_mode == neoflash_core::BlendMode::Normal,
            )),
        }
    }

    fn warn_filter_once(&mut self) {
        if !self.filter_warned {
            self.filter_warned = true;
            self.report.warning(
                "swf_filter_approximated",
                self.location,
                "SWF bitmap filters (DropShadow/Blur/Glow/Bevel/ColorMatrix) are not modeled in the authoring document; unfiltered artwork is preserved.",
            );
        }
    }

    fn warn_blend_once(&mut self) {
        if !self.blend_warned {
            self.blend_warned = true;
            self.report.warning(
                "swf_blend_mode_approximated",
                self.location,
                "SWF non-normal blend modes are rendered with standard normal compositing.",
            );
        }
    }
}

fn single_frame_timeline(sym_uid: &str, mut elements: Vec<Element>) -> Timeline {
    for (idx, el) in elements.iter_mut().enumerate() {
        el.id = format!("{sym_uid}-el-{idx}");
    }
    Timeline {
        frame_count: 1,
        layers: vec![Layer {
            id: format!("{sym_uid}-layer-1"),
            name: "Layer 1".into(),
            visible: true,
            locked: false,
            keyframes: vec![Keyframe {
                frame: 0,
                elements,
                ..Default::default()
            }],
            properties: LayerProperties::default(),
        }],
        scripts: Vec::new(),
        guides: Vec::new(),
    }
}

fn morph_shape_timeline(
    sym_uid: &str,
    mut start_elements: Vec<Element>,
    mut end_elements: Vec<Element>,
) -> Timeline {
    for (idx, el) in start_elements.iter_mut().enumerate() {
        el.id = format!("{sym_uid}-morph-{idx}");
    }
    for (idx, el) in end_elements.iter_mut().enumerate() {
        el.id = format!("{sym_uid}-morph-end-{idx}");
    }
    Timeline {
        frame_count: 101,
        layers: vec![Layer {
            id: format!("{sym_uid}-layer-1"),
            name: "Morph".into(),
            visible: true,
            locked: false,
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    elements: start_elements,
                    tween: Some(Easing::Linear),
                    tween_kind: TweenKind::Shape,
                    ..Default::default()
                },
                Keyframe {
                    frame: 100,
                    elements: end_elements,
                    ..Default::default()
                },
            ],
            properties: LayerProperties::default(),
        }],
        scripts: Vec::new(),
        guides: Vec::new(),
    }
}

fn element_is_pure_vector(el: &Element) -> bool {
    matches!(
        el.kind,
        ElementKind::Path { .. }
            | ElementKind::PaintedPath { .. }
            | ElementKind::Rectangle { .. }
            | ElementKind::Ellipse { .. }
    )
}

fn transform_to_matrix(t: Transform) -> [f64; 6] {
    let rot = t.rotation.to_radians();
    let y_angle = rot + t.skew.to_radians();
    [
        t.scale_x * rot.cos(),
        t.scale_x * rot.sin(),
        -t.scale_y * y_angle.sin(),
        t.scale_y * y_angle.cos(),
        t.x,
        t.y,
    ]
}

fn mul_matrix(p: [f64; 6], l: [f64; 6]) -> [f64; 6] {
    [
        p[0] * l[0] + p[2] * l[1],
        p[1] * l[0] + p[3] * l[1],
        p[0] * l[2] + p[2] * l[3],
        p[1] * l[2] + p[3] * l[3],
        p[0] * l[4] + p[2] * l[5] + p[4],
        p[1] * l[4] + p[3] * l[5] + p[5],
    ]
}

fn parse_swf_blend_mode(blend: u8) -> Option<neoflash_core::BlendMode> {
    use neoflash_core::BlendMode;
    match blend {
        0 | 1 => Some(BlendMode::Normal),
        2 => Some(BlendMode::Layer),
        3 => Some(BlendMode::Multiply),
        4 => Some(BlendMode::Screen),
        5 => Some(BlendMode::Lighten),
        6 => Some(BlendMode::Darken),
        7 => Some(BlendMode::Difference),
        8 => Some(BlendMode::Add),
        9 => Some(BlendMode::Subtract),
        10 => Some(BlendMode::Invert),
        11 => Some(BlendMode::Alpha),
        12 => Some(BlendMode::Erase),
        13 => Some(BlendMode::Overlay),
        14 => Some(BlendMode::Hardlight),
        _ => None,
    }
}

pub(super) fn parse_swf_filter_list(
    r: &mut SwfSlice<'_>,
) -> (Vec<neoflash_core::FilterSpec>, bool) {
    use neoflash_core::{FilterSpec, MAX_OBJECT_FILTERS};
    let Some(count) = r.u8().map(usize::from) else {
        return (Vec::new(), true);
    };
    let mut filters = Vec::new();
    let mut unmodeled = false;

    for _ in 0..count {
        let Some(filter_id) = r.u8() else {
            unmodeled = true;
            break;
        };
        let parsed = match filter_id {
            0 => {
                // DropShadowFilter (23 bytes)
                let Some(rgba) = r.rgba() else {
                    unmodeled = true;
                    break;
                };
                let Some(bx) = r.i32().map(|v| (f64::from(v) / 65536.0).clamp(0.0, 255.0)) else {
                    unmodeled = true;
                    break;
                };
                let Some(by) = r.i32().map(|v| (f64::from(v) / 65536.0).clamp(0.0, 255.0)) else {
                    unmodeled = true;
                    break;
                };
                let Some(angle_rad) = r.i32().map(|v| f64::from(v) / 65536.0) else {
                    unmodeled = true;
                    break;
                };
                let Some(distance) = r
                    .i32()
                    .map(|v| (f64::from(v) / 65536.0).clamp(-255.0, 255.0))
                else {
                    unmodeled = true;
                    break;
                };
                let Some(strength) = r.fixed8().map(|v| v.clamp(0.0, 255.0)) else {
                    unmodeled = true;
                    break;
                };
                let Some(flags) = r.u8() else {
                    unmodeled = true;
                    break;
                };
                let color =
                    (u32::from(rgba[0]) << 16) | (u32::from(rgba[1]) << 8) | u32::from(rgba[2]);
                let alpha = (f64::from(rgba[3]) / 255.0).clamp(0.0, 1.0);
                let inner = (flags & 0x80) != 0;
                let knockout = (flags & 0x40) != 0;
                let composite_source = (flags & 0x20) != 0;
                let hide_object = !composite_source;
                let quality = (flags & 0x1F).clamp(1, 15);
                let angle = angle_rad.to_degrees().rem_euclid(360.0);
                Some(FilterSpec::DropShadow {
                    distance,
                    angle,
                    color,
                    alpha,
                    blur_x: bx,
                    blur_y: by,
                    strength,
                    quality,
                    inner,
                    knockout,
                    hide_object,
                })
            }
            1 => {
                // BlurFilter (9 bytes)
                let Some(bx) = r.i32().map(|v| (f64::from(v) / 65536.0).clamp(0.0, 255.0)) else {
                    unmodeled = true;
                    break;
                };
                let Some(by) = r.i32().map(|v| (f64::from(v) / 65536.0).clamp(0.0, 255.0)) else {
                    unmodeled = true;
                    break;
                };
                let Some(flags) = r.u8() else {
                    unmodeled = true;
                    break;
                };
                let passes = if (flags >> 3) != 0 {
                    flags >> 3
                } else {
                    flags & 0x1F
                };
                let quality = passes.clamp(1, 15);
                Some(FilterSpec::Blur {
                    blur_x: bx,
                    blur_y: by,
                    quality,
                })
            }
            2 => {
                // GlowFilter (15 bytes)
                let Some(rgba) = r.rgba() else {
                    unmodeled = true;
                    break;
                };
                let Some(bx) = r.i32().map(|v| (f64::from(v) / 65536.0).clamp(0.0, 255.0)) else {
                    unmodeled = true;
                    break;
                };
                let Some(by) = r.i32().map(|v| (f64::from(v) / 65536.0).clamp(0.0, 255.0)) else {
                    unmodeled = true;
                    break;
                };
                let Some(strength) = r.fixed8().map(|v| v.clamp(0.0, 255.0)) else {
                    unmodeled = true;
                    break;
                };
                let Some(flags) = r.u8() else {
                    unmodeled = true;
                    break;
                };
                let color =
                    (u32::from(rgba[0]) << 16) | (u32::from(rgba[1]) << 8) | u32::from(rgba[2]);
                let alpha = (f64::from(rgba[3]) / 255.0).clamp(0.0, 1.0);
                let inner = (flags & 0x80) != 0;
                let knockout = (flags & 0x40) != 0;
                let quality = (flags & 0x1F).clamp(1, 15);
                Some(FilterSpec::Glow {
                    color,
                    alpha,
                    blur_x: bx,
                    blur_y: by,
                    strength,
                    quality,
                    inner,
                    knockout,
                })
            }
            3 => {
                // BevelFilter (27 bytes): ShadowColor (RGBA), HighlightColor (RGBA)
                let Some(s_rgba) = r.rgba() else {
                    unmodeled = true;
                    break;
                };
                let Some(h_rgba) = r.rgba() else {
                    unmodeled = true;
                    break;
                };
                let Some(bx) = r.i32().map(|v| (f64::from(v) / 65536.0).clamp(0.0, 255.0)) else {
                    unmodeled = true;
                    break;
                };
                let Some(by) = r.i32().map(|v| (f64::from(v) / 65536.0).clamp(0.0, 255.0)) else {
                    unmodeled = true;
                    break;
                };
                let Some(angle_rad) = r.i32().map(|v| f64::from(v) / 65536.0) else {
                    unmodeled = true;
                    break;
                };
                let Some(distance) = r
                    .i32()
                    .map(|v| (f64::from(v) / 65536.0).clamp(-255.0, 255.0))
                else {
                    unmodeled = true;
                    break;
                };
                let Some(strength) = r.fixed8().map(|v| v.clamp(0.0, 255.0)) else {
                    unmodeled = true;
                    break;
                };
                let Some(flags) = r.u8() else {
                    unmodeled = true;
                    break;
                };
                let shadow_color = (u32::from(s_rgba[0]) << 16)
                    | (u32::from(s_rgba[1]) << 8)
                    | u32::from(s_rgba[2]);
                let shadow_alpha = (f64::from(s_rgba[3]) / 255.0).clamp(0.0, 1.0);
                let highlight_color = (u32::from(h_rgba[0]) << 16)
                    | (u32::from(h_rgba[1]) << 8)
                    | u32::from(h_rgba[2]);
                let highlight_alpha = (f64::from(h_rgba[3]) / 255.0).clamp(0.0, 1.0);
                let inner = (flags & 0x80) != 0;
                let knockout = (flags & 0x40) != 0;
                let on_top = (flags & 0x10) != 0;
                let placement = if on_top {
                    "full".to_owned()
                } else if inner {
                    "inner".to_owned()
                } else {
                    "outer".to_owned()
                };
                let quality = (flags & 0x0F).clamp(1, 15);
                let angle = angle_rad.to_degrees().rem_euclid(360.0);
                Some(FilterSpec::Bevel {
                    distance,
                    angle,
                    highlight_color,
                    highlight_alpha,
                    shadow_color,
                    shadow_alpha,
                    blur_x: bx,
                    blur_y: by,
                    strength,
                    quality,
                    placement,
                    knockout,
                })
            }
            4 | 7 => {
                // GradientGlowFilter (4) / GradientBevelFilter (7)
                let Some(num_colors) = r.u8().map(usize::from) else {
                    unmodeled = true;
                    break;
                };
                let mut raw_colors = Vec::with_capacity(num_colors);
                for _ in 0..num_colors {
                    let Some(rgba) = r.rgba() else {
                        unmodeled = true;
                        break;
                    };
                    raw_colors.push(rgba);
                }
                if raw_colors.len() != num_colors {
                    break;
                }
                let mut raw_ratios = Vec::with_capacity(num_colors);
                for _ in 0..num_colors {
                    let Some(rat) = r.u8() else {
                        unmodeled = true;
                        break;
                    };
                    raw_ratios.push(rat);
                }
                if raw_ratios.len() != num_colors {
                    break;
                }
                let Some(bx) = r.i32().map(|v| (f64::from(v) / 65536.0).clamp(0.0, 255.0)) else {
                    unmodeled = true;
                    break;
                };
                let Some(by) = r.i32().map(|v| (f64::from(v) / 65536.0).clamp(0.0, 255.0)) else {
                    unmodeled = true;
                    break;
                };
                let Some(angle_rad) = r.i32().map(|v| f64::from(v) / 65536.0) else {
                    unmodeled = true;
                    break;
                };
                let Some(distance) = r
                    .i32()
                    .map(|v| (f64::from(v) / 65536.0).clamp(-255.0, 255.0))
                else {
                    unmodeled = true;
                    break;
                };
                let Some(strength) = r.fixed8().map(|v| v.clamp(0.0, 255.0)) else {
                    unmodeled = true;
                    break;
                };
                let Some(flags) = r.u8() else {
                    unmodeled = true;
                    break;
                };
                let inner = (flags & 0x80) != 0;
                let knockout = (flags & 0x40) != 0;
                let on_top = (flags & 0x10) != 0;
                let placement = if on_top {
                    "full".to_owned()
                } else if inner {
                    "inner".to_owned()
                } else {
                    "outer".to_owned()
                };
                let quality = (flags & 0x0F).clamp(1, 15);
                let angle = angle_rad.to_degrees().rem_euclid(360.0);
                let mut stops: Vec<(u32, f64, u8)> = raw_colors
                    .into_iter()
                    .zip(raw_ratios)
                    .map(|(rgba, rat)| {
                        let c = (u32::from(rgba[0]) << 16)
                            | (u32::from(rgba[1]) << 8)
                            | u32::from(rgba[2]);
                        let a = (f64::from(rgba[3]) / 255.0).clamp(0.0, 1.0);
                        (c, a, rat)
                    })
                    .collect();
                stops.sort_by_key(|s| s.2);
                stops.truncate(16);
                if stops.is_empty() {
                    stops.push((0x000000, 0.0, 0));
                    stops.push((0xFFFFFF, 1.0, 255));
                }
                let colors: Vec<u32> = stops.iter().map(|s| s.0).collect();
                let alphas: Vec<f64> = stops.iter().map(|s| s.1).collect();
                let ratios: Vec<u8> = stops.iter().map(|s| s.2).collect();
                if filter_id == 4 {
                    Some(FilterSpec::GradientGlow {
                        distance,
                        angle,
                        colors,
                        alphas,
                        ratios,
                        blur_x: bx,
                        blur_y: by,
                        strength,
                        quality,
                        placement,
                        knockout,
                    })
                } else {
                    Some(FilterSpec::GradientBevel {
                        distance,
                        angle,
                        colors,
                        alphas,
                        ratios,
                        blur_x: bx,
                        blur_y: by,
                        strength,
                        quality,
                        placement,
                        knockout,
                    })
                }
            }
            5 => {
                // ConvolutionFilter
                let Some(mx) = r.u8() else {
                    unmodeled = true;
                    break;
                };
                let Some(my) = r.u8() else {
                    unmodeled = true;
                    break;
                };
                let Some(div_bytes) = r.take(4) else {
                    unmodeled = true;
                    break;
                };
                let divisor = f64::from(f32::from_le_bytes(div_bytes.try_into().unwrap()));
                let Some(bias_bytes) = r.take(4) else {
                    unmodeled = true;
                    break;
                };
                let bias = f64::from(f32::from_le_bytes(bias_bytes.try_into().unwrap()));
                let cell_count = usize::from(mx) * usize::from(my);
                let mut matrix = Vec::with_capacity(cell_count);
                for _ in 0..cell_count {
                    let Some(mb) = r.take(4) else {
                        unmodeled = true;
                        break;
                    };
                    let val = f64::from(f32::from_le_bytes(mb.try_into().unwrap()));
                    matrix.push(if val.is_finite() { val } else { 0.0 });
                }
                if matrix.len() != cell_count {
                    break;
                }
                let Some(rgba) = r.rgba() else {
                    unmodeled = true;
                    break;
                };
                let Some(flags) = r.u8() else {
                    unmodeled = true;
                    break;
                };
                let color =
                    (u32::from(rgba[0]) << 16) | (u32::from(rgba[1]) << 8) | u32::from(rgba[2]);
                let alpha = (f64::from(rgba[3]) / 255.0).clamp(0.0, 1.0);
                let clamp = (flags & 0x02) != 0;
                let preserve_alpha = (flags & 0x01) != 0;
                if (1..=15).contains(&mx) && (1..=15).contains(&my) {
                    Some(FilterSpec::Convolution {
                        matrix_x: mx,
                        matrix_y: my,
                        matrix,
                        divisor: if divisor.is_finite() && divisor != 0.0 {
                            divisor
                        } else {
                            1.0
                        },
                        bias: if bias.is_finite() { bias } else { 0.0 },
                        preserve_alpha,
                        clamp,
                        color,
                        alpha,
                    })
                } else {
                    unmodeled = true;
                    None
                }
            }
            6 => {
                // ColorMatrixFilter (80 bytes = 20 x f32)
                let Some(raw) = r.take(80) else {
                    unmodeled = true;
                    break;
                };
                let mut matrix = [0.0f64; 20];
                for (i, chunk) in raw.as_chunks::<4>().0.iter().enumerate() {
                    let val = f64::from(f32::from_le_bytes(*chunk));
                    matrix[i] = if val.is_finite() {
                        val.clamp(-10_000.0, 10_000.0)
                    } else {
                        0.0
                    };
                }
                Some(FilterSpec::ColorMatrix { matrix })
            }
            _ => {
                unmodeled = true;
                break;
            }
        };
        if let Some(f) = parsed {
            if filters.len() < MAX_OBJECT_FILTERS {
                filters.push(f);
            } else {
                unmodeled = true;
            }
        }
    }

    (filters, unmodeled)
}

fn recover_classic_tween_spans(keyframes: &mut Vec<Keyframe>) {
    if keyframes.len() < 2 {
        return;
    }
    // First mark all 1-frame step intervals of the same continuous instance as Linear tweens.
    for ki in 0..keyframes.len().saturating_sub(1) {
        let next_frame = keyframes[ki + 1].frame;
        let cur_frame = keyframes[ki].frame;
        if next_frame == cur_frame + 1
            && keyframes[ki].elements.len() == 1
            && keyframes[ki + 1].elements.len() == 1
            && keyframes[ki].elements[0].id == keyframes[ki + 1].elements[0].id
            && matches!(keyframes[ki].elements[0].kind, ElementKind::Instance { .. })
            && keyframes[ki].elements[0].transform != keyframes[ki + 1].elements[0].transform
        {
            keyframes[ki].tween = Some(Easing::Linear);
        }
    }

    // Now scan for multi-frame runs [start_idx ..= end_idx] (end_idx - start_idx >= 2)
    // where intermediate frames match a single Classic tween curve (Linear, EaseIn/Out, or Flash acceleration).
    let mut out = Vec::with_capacity(keyframes.len());
    let mut i = 0usize;
    while i < keyframes.len() {
        let mut run_end = i;
        while run_end + 1 < keyframes.len()
            && keyframes[run_end + 1].frame == keyframes[run_end].frame + 1
            && keyframes[run_end].elements.len() == 1
            && keyframes[run_end + 1].elements.len() == 1
            && keyframes[run_end].elements[0].id == keyframes[i].elements[0].id
            && keyframes[run_end + 1].elements[0].id == keyframes[i].elements[0].id
            && keyframes[run_end + 1].elements[0].kind == keyframes[i].elements[0].kind
            && keyframes[run_end + 1].elements[0].filters == keyframes[i].elements[0].filters
            && keyframes[run_end + 1].elements[0].blend_mode == keyframes[i].elements[0].blend_mode
            && (run_end == i
                || (keyframes[run_end].label.is_empty() && keyframes[run_end].sound.is_none()))
        {
            run_end += 1;
        }

        if run_end >= i + 2 {
            // Try fitting the longest prefix [i ..= end_cand] (or full run [i ..= run_end])
            if let Some(fitted_ease) = fit_classic_tween_easing(&keyframes[i..=run_end]) {
                let mut start_kf = keyframes[i].clone();
                start_kf.tween = Some(fitted_ease);
                out.push(start_kf);
                i = run_end;
                continue;
            }
        }

        out.push(keyframes[i].clone());
        i += 1;
    }
    *keyframes = out;
}

fn fit_classic_tween_easing(run: &[Keyframe]) -> Option<Easing> {
    if run.len() < 3 {
        return None;
    }
    let a = &run.first()?.elements.first()?.transform;
    let b = &run.last()?.elements.first()?.transform;
    let total_frames = f64::from(run.last()?.frame.saturating_sub(run.first()?.frame));
    if total_frames < 2.0 {
        return None;
    }

    // Check that start and end poses are actually distinct (not a stationary wiggle that happens to end at start).
    let dx = (b.x - a.x).abs();
    let dy = (b.y - a.y).abs();
    let dsx = (b.scale_x - a.scale_x).abs();
    let dsy = (b.scale_y - a.scale_y).abs();
    let drot = (b.rotation - a.rotation).abs();
    let dalpha = (b.alpha - a.alpha).abs();
    if dx < 1e-3 && dy < 1e-3 && dsx < 1e-4 && dsy < 1e-4 && drot < 1e-3 && dalpha < 1e-4 {
        return None;
    }

    let eval_candidate = |ease: Easing| -> Option<f64> {
        let mut max_norm_err = 0.0f64;
        for kf in &run[1..run.len() - 1] {
            let t = f64::from(kf.frame - run[0].frame) / total_frames;
            let p = neoflash_core::easing(t, ease);
            let cur = &kf.elements[0].transform;

            let ex = (cur.x - (a.x + (b.x - a.x) * p)).abs();
            let ey = (cur.y - (a.y + (b.y - a.y) * p)).abs();
            let esx = (cur.scale_x - (a.scale_x + (b.scale_x - a.scale_x) * p)).abs();
            let esy = (cur.scale_y - (a.scale_y + (b.scale_y - a.scale_y) * p)).abs();
            let erot = (cur.rotation - (a.rotation + (b.rotation - a.rotation) * p)).abs();
            let eskew = (cur.skew - (a.skew + (b.skew - a.skew) * p)).abs();
            let ealpha = (cur.alpha - (a.alpha + (b.alpha - a.alpha) * p)).abs();

            // Allow up to 0.15 px for integer-twip rounding, 0.015 for scale/alpha, 0.5 deg for rotation.
            if ex > 0.15
                || ey > 0.15
                || esx > 0.015
                || esy > 0.015
                || erot > 0.5
                || eskew > 0.5
                || ealpha > 0.015
            {
                return None;
            }
            for ci in 0..4 {
                let em = (cur.color_transform.multipliers[ci]
                    - (a.color_transform.multipliers[ci]
                        + (b.color_transform.multipliers[ci] - a.color_transform.multipliers[ci])
                            * p))
                    .abs();
                let eo = (cur.color_transform.offsets[ci]
                    - (a.color_transform.offsets[ci]
                        + (b.color_transform.offsets[ci] - a.color_transform.offsets[ci]) * p))
                    .abs();
                if em > 0.02 || eo > 2.0 {
                    return None;
                }
            }
            let norm = (ex / 0.15)
                .max(ey / 0.15)
                .max(esx / 0.015)
                .max(esy / 0.015)
                .max(erot / 0.5)
                .max(ealpha / 0.015);
            max_norm_err = max_norm_err.max(norm);
        }
        Some(max_norm_err)
    };

    if let Some(lin_err) = eval_candidate(Easing::Linear)
        && lin_err <= 0.45
    {
        return Some(Easing::Linear);
    }

    let mut best: Option<(f64, Easing)> =
        eval_candidate(Easing::Linear).map(|e| (e, Easing::Linear));
    for named in [Easing::EaseIn, Easing::EaseOut, Easing::EaseInOut] {
        if let Some(err) = eval_candidate(named)
            && best.is_none_or(|(b_err, _)| err + 0.02 < b_err)
        {
            best = Some((err, named));
        }
    }
    for acc in -100..=100 {
        if acc == 0 {
            continue;
        }
        let cand = Easing::Flash {
            acceleration: f64::from(acc),
        };
        if let Some(err) = eval_candidate(cand)
            && best.is_none_or(|(b_err, _)| err + 0.01 < b_err)
        {
            best = Some((err, cand));
        }
    }

    best.map(|(_, ease)| match ease {
        Easing::Flash { acceleration } if (acceleration - (-100.0)).abs() < 1e-6 => Easing::EaseIn,
        Easing::Flash { acceleration } if (acceleration - 100.0).abs() < 1e-6 => Easing::EaseOut,
        other => other,
    })
}

fn attach_stream_sound(
    snapshots: &mut [FrameSnapshot],
    pending_sounds: &mut Vec<FrameSound>,
    start_frame: u32,
    fs: FrameSound,
) {
    if let Some(snap) = snapshots.get_mut(start_frame as usize) {
        snap.sounds.push(fs);
    } else {
        pending_sounds.push(fs);
    }
}

fn prepend_frame_script(timeline: &mut Timeline, frame: u32, lang: ScriptLanguage, code: String) {
    if frame >= timeline.frame_count {
        return;
    }
    if let Some(existing) = timeline.scripts.iter_mut().find(|s| s.frame == frame) {
        existing.script.source = format!("{code}\n\n{}", existing.script.source);
    } else {
        timeline.scripts.insert(
            0,
            FrameScript {
                frame,
                script: Script {
                    language: lang,
                    source: code,
                },
            },
        );
    }
}

fn normalized_scene_ranges(
    meta: &[(u32, String)],
    total_frames: usize,
) -> Vec<(usize, usize, String)> {
    if meta.is_empty() || total_frames == 0 {
        return Vec::new();
    }
    let mut sorted: Vec<(usize, String)> = meta
        .iter()
        .filter_map(|(off, name)| {
            let idx = *off as usize;
            (idx < total_frames).then(|| {
                let clean = if name.trim().is_empty() {
                    "Scene".to_owned()
                } else {
                    name.trim().to_owned()
                };
                (idx, clean)
            })
        })
        .collect();
    sorted.sort_by_key(|(idx, _)| *idx);
    sorted.dedup_by_key(|(idx, _)| *idx);
    if sorted.first().is_none_or(|(idx, _)| *idx != 0) {
        sorted.insert(0, (0, "Scene 1".into()));
    }
    let mut out = Vec::with_capacity(sorted.len());
    for i in 0..sorted.len() {
        let start = sorted[i].0;
        let end = sorted.get(i + 1).map_or(total_frames, |(next, _)| *next);
        if start < end {
            out.push((start, end, sorted[i].1.clone()));
        }
    }
    out
}

fn is_valid_class_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 1024
        && name.split('.').all(|part| {
            !part.is_empty()
                && part.chars().enumerate().all(|(i, c)| {
                    c == '_' || c == '$' || c.is_alphabetic() || (i > 0 && c.is_numeric())
                })
        })
}

fn char_symbol_id(characters: &HashMap<u16, SwfCharacter>, cid: u16) -> Option<Id> {
    match characters.get(&cid)? {
        SwfCharacter::GraphicSymbol { symbol_id, .. }
        | SwfCharacter::Button { symbol_id, .. }
        | SwfCharacter::Sprite { symbol_id, .. } => Some(symbol_id.clone()),
        SwfCharacter::InlineText {
            fallback_symbol_id, ..
        } => fallback_symbol_id.clone(),
    }
}

fn apply_linkage_and_abc(
    project: &mut Project,
    characters: &HashMap<u16, SwfCharacter>,
    exports: &[(u16, String)],
    symbol_classes: &[(u16, String)],
    abc_classes: Vec<RecoveredAs3Class>,
    _location: &str,
    _report: &mut ImportReport,
) {
    let mut pure_timeline_classes: HashMap<String, Vec<(u32, String)>> = HashMap::new();
    for cls in &abc_classes {
        if cls.is_pure_timeline_class {
            pure_timeline_classes.insert(
                cls.qualified_name.clone(),
                cls.timeline_frame_scripts.clone(),
            );
        }
    }

    let mut used_export_ids: HashSet<String> = HashSet::new();
    let mut used_exported_classes: HashSet<String> = HashSet::new();

    // 1. Apply AVM1 `ExportAssets`
    for (cid, raw_linkage) in exports {
        let clean: String = raw_linkage
            .chars()
            .filter(|c| !c.is_control())
            .take(512)
            .collect();
        if clean.is_empty() || !used_export_ids.insert(clean.clone()) {
            continue;
        }
        let Some(sym_id) = char_symbol_id(characters, *cid) else {
            continue;
        };
        if sym_id == project.root {
            continue;
        }
        if let Some(sym) = project.symbol_mut(&sym_id) {
            if sym.kind == SymbolKind::Graphic {
                sym.kind = SymbolKind::MovieClip;
            }
            sym.linkage_id = Some(clean.clone());
            // Rename generic `"Sprite N"` to the readable export identifier!
            if sym.name.starts_with("Sprite ") || sym.name.starts_with("Button ") {
                sym.name = clean;
            }
        }
    }

    // 2. Apply AVM2 `SymbolClass`
    let mut omitted_pure_classes: HashSet<String> = HashSet::new();
    for (cid, raw_class) in symbol_classes {
        if !is_valid_class_name(raw_class) {
            continue;
        }
        if *cid == 0 {
            if let Some(fscripts) = pure_timeline_classes.get(raw_class) {
                attach_root_as3_frame_scripts(project, fscripts);
                omitted_pure_classes.insert(raw_class.clone());
            } else if project.document_class.is_none() {
                project.document_class = Some(raw_class.clone());
            }
            continue;
        }

        let Some(sym_id) = char_symbol_id(characters, *cid) else {
            continue;
        };
        if sym_id == project.root {
            continue;
        }
        if let Some(fscripts) = pure_timeline_classes.get(raw_class) {
            if let Some(sym) = project.symbol_mut(&sym_id) {
                if sym.kind == SymbolKind::Graphic {
                    sym.kind = SymbolKind::MovieClip;
                }
                attach_as3_frame_scripts(&mut sym.timeline, fscripts);
            }
            omitted_pure_classes.insert(raw_class.clone());
            continue;
        }

        if !used_exported_classes.insert(raw_class.clone()) {
            continue;
        }
        if let Some(sym) = project.symbol_mut(&sym_id) {
            if sym.kind == SymbolKind::Graphic {
                sym.kind = SymbolKind::MovieClip;
            }
            sym.class_name = Some(raw_class.clone());
            if sym.linkage_id.is_none() && used_export_ids.insert(raw_class.clone()) {
                sym.linkage_id = Some(raw_class.clone());
            }
            let short = raw_class.rsplit('.').next().unwrap_or(raw_class);
            if sym.name.starts_with("Sprite ") || sym.name.starts_with("Button ") {
                sym.name = short.to_owned();
            }
        }
    }

    // Also handle Flash SWFs that omit `SymbolClass(0, ...MainTimeline)` when `MainTimeline` is a pure timeline class.
    for (qname, fscripts) in &pure_timeline_classes {
        if (qname.ends_with(".MainTimeline") || qname == "MainTimeline")
            && !omitted_pure_classes.contains(qname)
        {
            attach_root_as3_frame_scripts(project, fscripts);
            omitted_pure_classes.insert(qname.clone());
        }
    }

    // 3. Populate `project.sources` with decompiled `.as` files (excluding omitted auto-generated timeline classes).
    let mut used_paths_lower: HashSet<String> = HashSet::new();
    let mut total_src_bytes = 0usize;
    for (idx, cls) in abc_classes.into_iter().enumerate() {
        if omitted_pure_classes.contains(&cls.qualified_name) {
            continue;
        }
        if cls.source.len() > 1_048_576
            || total_src_bytes + cls.source.len() > 16 * 1024 * 1024
            || project.sources.len() >= 4096
        {
            break;
        }
        let mut path = cls.source_path;
        if !used_paths_lower.insert(path.to_lowercase()) {
            path = format!("{}-{idx}.as", path.strip_suffix(".as").unwrap_or(&path));
            if !used_paths_lower.insert(path.to_lowercase()) {
                continue;
            }
        }
        total_src_bytes += cls.source.len();
        project.sources.push(SourceUnit {
            id: format!("swf-as3-src-{}", idx + 1),
            path,
            language: ScriptLanguage::ActionScript3,
            source: cls.source,
        });
    }
}

fn attach_root_as3_frame_scripts(project: &mut Project, fscripts: &[(u32, String)]) {
    if project.scenes.len() > 1 {
        let mut ranges: Vec<(Id, u32, u32)> = Vec::with_capacity(project.scenes.len());
        let mut offset = 0u32;
        for scene_id in project.scenes.clone() {
            if let Some(sym) = project.symbol(&scene_id) {
                let len = sym.timeline.frame_count.max(1);
                ranges.push((scene_id, offset, offset.saturating_add(len)));
                offset = offset.saturating_add(len);
            }
        }
        for (scene_id, start, end) in ranges {
            let local_scripts: Vec<(u32, String)> = fscripts
                .iter()
                .filter(|(f, _)| *f >= start && *f < end)
                .map(|(f, code)| (*f - start, code.clone()))
                .collect();
            if !local_scripts.is_empty()
                && let Some(sym) = project.symbol_mut(&scene_id)
            {
                attach_as3_frame_scripts(&mut sym.timeline, &local_scripts);
            }
        }
    } else if let Some(root_sym) = project.symbol_mut(&project.root.clone()) {
        attach_as3_frame_scripts(&mut root_sym.timeline, fscripts);
    }
}

fn attach_as3_frame_scripts(timeline: &mut Timeline, fscripts: &[(u32, String)]) {
    for (frame, code) in fscripts {
        if *frame < timeline.frame_count && !code.trim().is_empty() {
            if let Some(existing) = timeline.scripts.iter_mut().find(|s| s.frame == *frame) {
                existing.script.language = ScriptLanguage::ActionScript3;
                existing.script.source.push_str("\n\n");
                existing.script.source.push_str(code);
            } else {
                timeline.scripts.push(FrameScript {
                    frame: *frame,
                    script: Script {
                        language: ScriptLanguage::ActionScript3,
                        source: code.clone(),
                    },
                });
            }
        }
    }
    timeline.scripts.sort_by_key(|s| s.frame);
}

fn embed_swf_provenance_and_source(
    project: &mut Project,
    container: &SwfContainer,
    inspection: &SwfInspection,
    mut resource_bytes_total: usize,
    location: &str,
    report: &mut ImportReport,
) {
    let raw = &container.raw_bytes;
    let fnv = fnv1a_64(raw);
    let prov_json = serde_json::json!({
        "format": "swf",
        "swf_version": container.swf_version,
        "compression": inspection.compression,
        "raw_bytes": raw.len(),
        "declared_file_length": container.declared_file_length,
        "fnv1a64": format!("{fnv:016x}"),
        "stage_bounds_twips": inspection.stage_bounds_twips,
        "frame_rate": inspection.frame_rate,
        "frame_count": inspection.frame_count,
        "as3_flag": inspection.as3_flag,
        "tags": inspection.tags,
    });
    if let Ok(prov_bytes) = serde_json::to_vec_pretty(&prov_json)
        && prov_bytes.len() <= MAX_RESOURCE_BYTES
        && project.resources.len() < MAX_RESOURCES
        && resource_bytes_total + prov_bytes.len() <= MAX_RESOURCE_TOTAL_BYTES
        && let Ok(unit) = ResourceUnit::from_bytes(
            "swf-provenance",
            "legacy/swf-provenance.json",
            "application/json",
            &prov_bytes,
        )
    {
        resource_bytes_total += prov_bytes.len();
        project.resources.push(unit);
    }

    // Embed original `legacy/source.swf` when <= 16 MiB and total resources remain <= 48 MiB
    // so base64 expansion stays safely under `MAX_PROJECT_JSON_BYTES` (128 MiB).
    let safe_embed_ceiling = 48 * 1024 * 1024;
    if !raw.is_empty()
        && raw.len() <= MAX_RESOURCE_BYTES
        && project.resources.len() < MAX_RESOURCES
        && resource_bytes_total + raw.len() <= safe_embed_ceiling
        && let Ok(unit) = ResourceUnit::from_bytes(
            "swf-original-source",
            "legacy/source.swf",
            "application/x-shockwave-flash",
            raw,
        )
    {
        project.resources.push(unit);
    } else if !raw.is_empty() {
        report.warning(
            "swf_source_not_embedded",
            location,
            format!(
                "Original SWF ({} bytes) exceeds single-resource (16 MiB) or document budget; provenance digest ({fnv:016x}) was recorded without embedding raw SWF bytes.",
                raw.len()
            ),
        );
    }
}

fn finalize_project_format_version(project: &mut Project) {
    project.format_version = 1;
    if project.has_program() {
        project.enable_program();
    }
    if !project.resources.is_empty() {
        project.format_version = project.format_version.max(3);
    }
    let uses_v4 = !project.library_folders.is_empty()
        || project.symbols.iter().any(|s| {
            !s.folder.is_empty() || s.timeline.layers.iter().any(|l| !l.properties.is_default())
        });
    if uses_v4 {
        project.format_version = project.format_version.max(4);
    }
    let uses_v5 = project.symbols.iter().any(|s| {
        s.timeline.layers.iter().any(|l| {
            l.properties.end_frame.is_some()
                || l.keyframes.iter().any(|k| {
                    matches!(k.tween, Some(Easing::Flash { .. }))
                        || k.elements.iter().any(|e| {
                            e.transform.skew != 0.0
                                || !e.transform.color_transform.is_identity()
                                || matches!(e.kind, ElementKind::PaintedPath { .. })
                        })
                })
        })
    });
    if uses_v5 {
        project.format_version = project.format_version.max(5);
    }
    project.upgrade_format_for_features();
}

fn populate_project_statistics(project: &Project, report: &mut ImportReport) {
    report.statistics.symbols = project.symbols.len();
    report.statistics.layers = project
        .symbols
        .iter()
        .map(|s| s.timeline.layers.len())
        .sum();
    report.statistics.keyframes = project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .map(|l| l.keyframes.len())
        .sum();
    report.statistics.elements = project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .flat_map(|l| &l.keyframes)
        .map(|k| k.elements.len())
        .sum();
    let timeline_scripts: usize = project
        .symbols
        .iter()
        .map(|s| s.timeline.scripts.len())
        .sum();
    let object_scripts: usize = project
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .map(|e| e.scripts.len())
        .sum();
    report.statistics.scripts = timeline_scripts + object_scripts + project.sources.len();
}

fn parse_hex_rgb(hex: &str) -> Option<Color> {
    let s = hex.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some([r, g, b, 255])
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}
