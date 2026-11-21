//! Bounded SWF reconstruction for binary FLA `CSwfPage` (`CPicSwf`) compiled clips.

use super::assets::BitmapAsset;
use crate::legacy::contents::ContentsDescriptor;
use crate::{ImportFormat, ImportReport, Severity, Statistics};
use neoflash_core::{
    Element, ElementKind, MAX_RESOURCE_BYTES, MAX_RESOURCES, Project, ResourceUnit, ScriptLanguage,
    SourceUnit, Symbol, SymbolKind, Transform,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const MAX_TOTAL_COMPILED_CLIP_SWF_BYTES: usize = crate::swf::MAX_SWF_BYTES;
pub(super) const MAX_TOTAL_COMPILED_CLIP_SYMBOLS: usize = 2_000;
pub(super) const MAX_TOTAL_COMPILED_CLIP_ELEMENTS: usize = 25_000;

#[derive(Clone, Debug)]
pub(super) struct ReconstructedSwfClip {
    pub main_symbol: Symbol,
    pub helper_symbols: Vec<Symbol>,
    pub sources: Vec<SourceUnit>,
    pub has_avm1_scripts: bool,
}

#[derive(Default)]
pub(super) struct CompiledClipSwfBudget {
    pub decompressed_bytes: usize,
    pub symbols: usize,
    pub elements: usize,
}

fn symbol_has_visual_elements(sym: &Symbol) -> bool {
    sym.timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .any(|k| !k.elements.is_empty())
}

fn symbol_has_scripts(sym: &Symbol) -> bool {
    !sym.timeline.scripts.is_empty()
        || sym
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .any(|e| !e.scripts.is_empty())
}

fn count_symbol_elements(sym: &Symbol) -> usize {
    sym.timeline
        .layers
        .iter()
        .flat_map(|l| &l.keyframes)
        .map(|k| k.elements.len())
        .sum()
}

fn remap_symbol_references(
    sym: &mut Symbol,
    clip_id: u16,
    sym_idx: usize,
    sym_map: &BTreeMap<String, String>,
    res_map: &BTreeMap<String, String>,
    doc_language: ScriptLanguage,
    dropped_mismatched_vm: &mut bool,
) {
    let doc_is_as3 = doc_language == ScriptLanguage::ActionScript3;
    let mut layer_map = BTreeMap::new();
    for (li, layer) in sym.timeline.layers.iter_mut().enumerate() {
        let new_lid = format!("fla-cc-{clip_id}-s{sym_idx}-layer-{li}");
        layer_map.insert(layer.id.clone(), new_lid.clone());
        layer.id = new_lid;
    }
    for layer in &mut sym.timeline.layers {
        if let Some(parent_id) = &mut layer.properties.parent
            && let Some(mapped) = layer_map.get(parent_id)
        {
            *parent_id = mapped.clone();
        }
        if let Some(mask_id) = &mut layer.properties.mask
            && let Some(mapped) = layer_map.get(mask_id)
        {
            *mask_id = mapped.clone();
        }
        if let Some(guide_id) = &mut layer.properties.motion_guide
            && let Some(mapped) = layer_map.get(guide_id)
        {
            *guide_id = mapped.clone();
        }
        for kf in &mut layer.keyframes {
            let map_el_id = |old: &str| format!("fla-cc-{clip_id}-s{sym_idx}-{old}");
            if let Some(snd) = &mut kf.sound {
                if let Some(mapped) = res_map.get(&snd.resource) {
                    snd.resource = mapped.clone();
                } else {
                    kf.sound = None;
                }
            }
            if let Some(vid) = &mut kf.video {
                if let Some(mapped) = res_map.get(&vid.resource) {
                    vid.resource = mapped.clone();
                } else {
                    kf.video = None;
                }
            }
            for bf in &mut kf.bitmap_fills {
                bf.element_id = map_el_id(&bf.element_id);
                if let Some(mapped) = res_map.get(&bf.resource_id) {
                    bf.resource_id = mapped.clone();
                }
            }
            kf.bitmap_fills
                .retain(|bf| bf.resource_id.starts_with("legacy-cc-"));
            for app in &mut kf.appearances {
                app.element_id = map_el_id(&app.element_id);
            }
            for ve in &mut kf.vector_effects {
                ve.element_id = map_el_id(&ve.element_id);
            }
            for w in &mut kf.asset_warps {
                w.element_id = map_el_id(&w.element_id);
            }
            for ep in &mut kf.element_parents {
                ep.child_id = map_el_id(&ep.child_id);
                ep.parent_id = map_el_id(&ep.parent_id);
            }
            for el in &mut kf.elements {
                el.id = map_el_id(&el.id);
                let before_len = el.scripts.len();
                el.scripts.retain(|os| {
                    let is_as3 = os.script.language == ScriptLanguage::ActionScript3;
                    !doc_is_as3 && !is_as3
                });
                if el.scripts.len() != before_len {
                    *dropped_mismatched_vm = true;
                }
                match &mut el.kind {
                    ElementKind::Instance { symbol_id, .. } => {
                        if let Some(mapped) = sym_map.get(symbol_id) {
                            *symbol_id = mapped.clone();
                        }
                    }
                    ElementKind::Bitmap { resource_id, .. } => {
                        if let Some(mapped) = res_map.get(resource_id) {
                            *resource_id = mapped.clone();
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    let before_fs = sym.timeline.scripts.len();
    sym.timeline.scripts.retain_mut(|fs| {
        if let Some(lid) = &mut fs.layer {
            if let Some(mapped) = layer_map.get(lid) {
                *lid = mapped.clone();
            } else {
                fs.layer = None;
            }
        }
        let is_as3 = fs.script.language == ScriptLanguage::ActionScript3;
        is_as3 == doc_is_as3
    });
    if sym.timeline.scripts.len() != before_fs {
        *dropped_mismatched_vm = true;
    }
}

fn collect_reachable_symbols(proj: &Project, entry_id: &str, out_ids: &mut BTreeSet<String>) {
    let mut stack = vec![entry_id.to_owned()];
    while let Some(cur) = stack.pop() {
        if !out_ids.insert(cur.clone()) {
            continue;
        }
        let Some(sym) = proj.symbol(&cur) else {
            continue;
        };
        for el in sym
            .timeline
            .layers
            .iter()
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
        {
            if let ElementKind::Instance { symbol_id, .. } = &el.kind
                && !out_ids.contains(symbol_id)
            {
                stack.push(symbol_id.clone());
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn reconstruct_compiled_clip_swf(
    clip_id: u16,
    swf_bytes: &[u8],
    contents_bytes: &[u8],
    descriptor: &ContentsDescriptor,
    width: f64,
    height: f64,
    icon_bitmap: &Option<BitmapAsset>,
    doc_language: ScriptLanguage,
    location: &str,
    swf_budget: &mut CompiledClipSwfBudget,
    resources: &mut Vec<ResourceUnit>,
    used_ids: &mut BTreeSet<String>,
    used_paths: &mut BTreeSet<String>,
    total_resource_bytes: &mut usize,
    resource_byte_budget: usize,
    report: &mut ImportReport,
) -> Option<(ReconstructedSwfClip, bool)> {
    let Some(header_len_bytes) = swf_bytes.get(4..8).and_then(|s| s.try_into().ok()) else {
        report.warning(
            "fla_media_not_imported",
            location,
            "Compiled-clip SWF stream is truncated before its header; a placeholder symbol keeps its placements.",
        );
        return None;
    };
    let declared_len = u32::from_le_bytes(header_len_bytes) as usize;
    if declared_len < 8
        || swf_bytes.len() > MAX_TOTAL_COMPILED_CLIP_SWF_BYTES
        || swf_budget
            .decompressed_bytes
            .checked_add(declared_len)
            .is_none_or(|n| n > MAX_TOTAL_COMPILED_CLIP_SWF_BYTES)
    {
        report.warning(
            "fla_media_not_imported",
            location,
            "Embedded compiled-clip SWF exceeds the 64 MiB shared SWF decompression budget; a placeholder symbol keeps its placements.",
        );
        return None;
    }
    swf_budget.decompressed_bytes += declared_len;

    let container = match crate::swf::read_container_from_bytes(swf_bytes.to_vec()) {
        Ok(c) => c,
        Err(err) => {
            report.warning(
                "fla_media_not_imported",
                location,
                format!("Compiled-clip SWF stream could not be read ({err}); a placeholder symbol keeps its placements."),
            );
            return None;
        }
    };

    let mut swf_report = ImportReport {
        source: location.to_owned(),
        format: ImportFormat::Swf,
        version: None,
        can_import: false,
        statistics: Statistics::default(),
        diagnostics: Vec::new(),
        binary_fla: None,
        swf: None,
        swc: None,
    };
    let swf_proj = crate::swf::convert(&container, location, &mut swf_report);
    let had_swf_error = swf_report
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error);
    for diag in swf_report.diagnostics {
        if diag.severity == Severity::Warning {
            report.warning(&diag.code, &diag.location, diag.message);
        }
    }
    let Some(swf_proj) = swf_proj.filter(|_| !had_swf_error) else {
        report.warning(
            "fla_media_not_imported",
            location,
            "Compiled-clip SWF could not be converted into editable symbols; a placeholder symbol keeps its placements.",
        );
        return None;
    };

    let linkage = descriptor.linkage.as_ref().or_else(|| {
        descriptor
            .library_metadata
            .as_ref()
            .and_then(|m| m.linkage.as_ref())
    });
    let want_ident = linkage
        .and_then(|l| super::read_string(contents_bytes, &l.identifier).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let want_class = linkage
        .and_then(|l| l.class_name.as_ref())
        .and_then(|c| super::read_string(contents_bytes, c).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());

    let entry_id = swf_proj
        .symbols
        .iter()
        .find(|s| {
            s.id != swf_proj.root
                && ((want_ident.is_some() && s.linkage_id == want_ident)
                    || (want_class.is_some() && s.class_name == want_class))
        })
        .map(|s| s.id.clone())
        .or_else(|| {
            let root_sym = swf_proj.symbol(&swf_proj.root)?;
            (symbol_has_visual_elements(root_sym) || symbol_has_scripts(root_sym))
                .then(|| swf_proj.root.clone())
        })
        .or_else(|| {
            swf_proj
                .symbols
                .iter()
                .rev()
                .find(|s| {
                    s.id != swf_proj.root
                        && (s.linkage_id.is_some() || s.class_name.is_some())
                        && (symbol_has_visual_elements(s) || symbol_has_scripts(s))
                })
                .or_else(|| {
                    swf_proj.symbols.iter().rev().find(|s| {
                        s.id != swf_proj.root
                            && (symbol_has_visual_elements(s) || symbol_has_scripts(s))
                    })
                })
                .map(|s| s.id.clone())
        })
        .unwrap_or_else(|| swf_proj.root.clone());

    let mut reachable = BTreeSet::new();
    collect_reachable_symbols(&swf_proj, &entry_id, &mut reachable);

    let doc_is_as3 = doc_language == ScriptLanguage::ActionScript3;
    let compatible_sources: Vec<SourceUnit> = swf_proj
        .sources
        .into_iter()
        .enumerate()
        .filter(|(_, src)| (src.language == ScriptLanguage::ActionScript3) == doc_is_as3)
        .map(|(idx, mut src)| {
            src.id = format!("fla-cc-{clip_id}-src-{}", idx + 1);
            src
        })
        .collect();

    let has_visual = swf_proj
        .symbols
        .iter()
        .filter(|s| reachable.contains(&s.id))
        .any(symbol_has_visual_elements);
    let has_any_scripts = !compatible_sources.is_empty()
        || swf_proj
            .symbols
            .iter()
            .filter(|s| reachable.contains(&s.id))
            .any(symbol_has_scripts);

    if !has_visual && !has_any_scripts {
        report.warning(
            "fla_media_not_imported",
            location,
            "Compiled-clip SWF contains no reconstructable artwork or scripts; a placeholder symbol keeps its placements.",
        );
        return None;
    }

    let total_elems: usize = swf_proj
        .symbols
        .iter()
        .filter(|s| reachable.contains(&s.id))
        .map(count_symbol_elements)
        .sum();
    if swf_budget
        .symbols
        .checked_add(reachable.len())
        .is_none_or(|n| n > MAX_TOTAL_COMPILED_CLIP_SYMBOLS)
        || swf_budget
            .elements
            .checked_add(total_elems)
            .is_none_or(|n| n > MAX_TOTAL_COMPILED_CLIP_ELEMENTS)
    {
        report.warning(
            "fla_media_not_imported",
            location,
            "Compiled-clip SWF exceeds the document's symbol or element budget; a placeholder symbol keeps its placements.",
        );
        return None;
    }

    // Stage SWF media resources (excluding SWF provenance blobs).
    let mut staged_resources = Vec::new();
    let mut res_map = BTreeMap::new();
    let mut added_bytes = 0usize;
    for (r_idx, res) in swf_proj.resources.iter().enumerate() {
        if res.path.starts_with("legacy/source.swf")
            || res.id == "swf-provenance"
            || res
                .media_type
                .eq_ignore_ascii_case("application/x-neoflash-swf-source")
            || res.media_type.eq_ignore_ascii_case("application/json")
        {
            continue;
        }
        let Ok(raw) = res.decoded_bytes() else {
            continue;
        };
        if raw.len() > MAX_RESOURCE_BYTES
            || resources.len() + staged_resources.len() >= MAX_RESOURCES - 2
            || total_resource_bytes
                .checked_add(added_bytes)
                .and_then(|n| n.checked_add(raw.len()))
                .is_none_or(|n| n > resource_byte_budget)
        {
            report.warning(
                "fla_media_not_imported",
                location,
                "Compiled-clip SWF media exceeds the 64 MiB project resource budget; a placeholder symbol keeps its placements.",
            );
            return None;
        }
        let new_res_id = format!("legacy-cc-{clip_id}-res-{}", r_idx + 1);
        let file_name = res
            .path
            .rsplit_once('/')
            .map_or(res.path.as_str(), |(_, f)| f);
        let mut new_path = format!("legacy/compiled-clips/{clip_id}/{file_name}");
        let mut dedup = 2usize;
        while used_paths.contains(&new_path.to_ascii_lowercase())
            || staged_resources
                .iter()
                .any(|u: &ResourceUnit| u.path.eq_ignore_ascii_case(&new_path))
        {
            let (stem, ext) = file_name.rsplit_once('.').unwrap_or((file_name, "bin"));
            new_path = format!("legacy/compiled-clips/{clip_id}/{stem}-{dedup}.{ext}");
            dedup += 1;
        }
        let Ok(unit) = ResourceUnit::from_bytes(&new_res_id, &new_path, &res.media_type, &raw)
        else {
            continue;
        };
        added_bytes += raw.len();
        res_map.insert(res.id.clone(), new_res_id);
        staged_resources.push(unit);
    }

    let mut sym_map = BTreeMap::new();
    let main_id = format!("fla-compiled-clip-{clip_id}");
    sym_map.insert(entry_id.clone(), main_id.clone());
    let mut helper_counter = 0usize;
    for sym in &swf_proj.symbols {
        if reachable.contains(&sym.id) && sym.id != entry_id {
            helper_counter += 1;
            sym_map.insert(
                sym.id.clone(),
                format!("fla-compiled-clip-{clip_id}-sym-{helper_counter}"),
            );
        }
    }

    let mut dropped_mismatched_vm = false;
    let mut main_symbol_opt = None;
    let mut helper_symbols = Vec::new();
    for (s_idx, mut sym) in swf_proj.symbols.into_iter().enumerate() {
        let Some(new_sid) = sym_map.get(&sym.id).cloned() else {
            continue;
        };
        let is_main = sym.id == entry_id;
        sym.id = new_sid;
        sym.folder = String::new();
        sym.linkage_id = None;
        sym.class_name = None;
        remap_symbol_references(
            &mut sym,
            clip_id,
            s_idx,
            &sym_map,
            &res_map,
            doc_language,
            &mut dropped_mismatched_vm,
        );
        if is_main {
            sym.kind = SymbolKind::MovieClip;
            sym.name = super::read_string(contents_bytes, &descriptor.name)
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| format!("Compiled clip #{clip_id}"));
            main_symbol_opt = Some(sym);
        } else {
            helper_symbols.push(sym);
        }
    }

    let Some(mut main_symbol) = main_symbol_opt else {
        report.warning(
                "fla_media_not_imported",
                location,
                "Compiled-clip SWF entry symbol could not be resolved; a placeholder symbol keeps its placements.",
            );
        return None;
    };
    let mut used_icon_fallback = false;
    if !symbol_has_visual_elements(&main_symbol) {
        // Code-only component SWF with no Stage shapes: keep scripts and attach authoring preview/placeholder.
        let has_remaining_scripts = !compatible_sources.is_empty()
            || symbol_has_scripts(&main_symbol)
            || helper_symbols.iter().any(symbol_has_scripts);
        if !has_remaining_scripts {
            report.warning(
                    "fla_media_not_imported",
                    location,
                    "Compiled-clip SWF uses scripts from an incompatible ActionScript VM and contains no Stage artwork; a placeholder symbol keeps its placements.",
                );
            return None;
        }
        let fallback_el = if let Some(icon) = icon_bitmap {
            used_icon_fallback = true;
            let mut tf = Transform::default();
            if icon.width > 0.0 && icon.height > 0.0 {
                tf.scale_x = (width / icon.width).clamp(0.01, 1000.0);
                tf.scale_y = (height / icon.height).clamp(0.01, 1000.0);
            }
            Element {
                id: format!("fla-compiled-clip-{clip_id}-icon"),
                name: String::new(),
                transform: tf,
                kind: ElementKind::Bitmap {
                    resource_id: icon.resource_id.clone(),
                    width: icon.width as u32,
                    height: icon.height as u32,
                    smoothing: icon.smoothing,
                },
                ..Element::default()
            }
        } else {
            Element {
                id: format!("fla-compiled-clip-{clip_id}-avatar"),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Rectangle {
                    width,
                    height,
                    fill: [180, 180, 180, 64],
                    stroke: [120, 120, 120, 160],
                    stroke_width: 1.0,
                },
                ..Element::default()
            }
        };
        if main_symbol.timeline.layers.is_empty() {
            main_symbol.timeline = super::empty_timeline(&format!("fla-cc-{clip_id}-main-layer-0"));
        }
        if let Some(first_layer) = main_symbol.timeline.layers.first_mut() {
            if first_layer.keyframes.is_empty() {
                first_layer.keyframes.push(neoflash_core::Keyframe {
                    frame: 0,
                    elements: vec![fallback_el],
                    ..Default::default()
                });
            } else {
                first_layer.keyframes[0].elements.push(fallback_el);
            }
        }
        report.warning(
            "fla_media_not_imported",
            location,
            "Compiled-clip SWF has no Stage artwork; component scripts were retained alongside an authoring preview/placeholder.",
        );
    } else if dropped_mismatched_vm {
        report.warning(
            "fla_media_not_imported",
            location,
            "Compiled-clip SWF scripts target a different ActionScript VM than the parent FLA and were omitted; visual timelines were reconstructed.",
        );
    }

    for unit in staged_resources {
        used_ids.insert(unit.id.clone());
        used_paths.insert(unit.path.to_ascii_lowercase());
        resources.push(unit);
    }
    *total_resource_bytes += added_bytes;
    swf_budget.symbols += 1 + helper_symbols.len();
    swf_budget.elements += total_elems;

    let has_avm1_scripts = !doc_is_as3
        && (symbol_has_scripts(&main_symbol) || helper_symbols.iter().any(symbol_has_scripts));

    Some((
        ReconstructedSwfClip {
            main_symbol,
            helper_symbols,
            sources: compatible_sources,
            has_avm1_scripts,
        },
        used_icon_fallback,
    ))
}
