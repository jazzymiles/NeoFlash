use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("Invalid project JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid project: {0}")]
    Invalid(String),
}

impl Project {
    pub fn validate(&self) -> Result<(), ProjectError> {
        let issues = validate(self);
        if issues.is_empty() {
            crate::model::reserve_ids(self);
            Ok(())
        } else {
            Err(ProjectError::Invalid(
                issues
                    .into_iter()
                    .map(|i| format!("{}: {}", i.path, i.message))
                    .collect::<Vec<_>>()
                    .join("; "),
            ))
        }
    }

    pub fn to_json(&self) -> Result<String, ProjectError> {
        self.validate()?;
        let json = serde_json::to_string_pretty(self)?;
        if json.len() > MAX_PROJECT_JSON_BYTES {
            return Err(ProjectError::Invalid(
                "document exceeds the 128 MiB encoded JSON limit".into(),
            ));
        }
        Ok(json)
    }

    pub fn from_json(json: &str) -> Result<Self, ProjectError> {
        if json.len() > MAX_PROJECT_JSON_BYTES {
            return Err(ProjectError::Invalid(
                "document exceeds the 128 MiB encoded JSON limit".into(),
            ));
        }
        let project: Self = serde_json::from_str(json)?;
        project.validate()?;
        Ok(project)
    }
}

pub fn validate(project: &Project) -> Vec<ValidationIssue> {
    let mut issues = vec![];
    let mut add = |path: String, message: String| issues.push(ValidationIssue { path, message });
    if !matches!(project.format_version, 1..=7) {
        add(
            "format_version".into(),
            format!(
                "unsupported version {}; supported versions are 1, 2, 3, 4, 5, 6 and 7",
                project.format_version
            ),
        );
    }
    if project.uses_format_6() && project.format_version < 6 {
        add(
            "format_version".into(),
            "scenes, shape/motion tweens, motion guides and frame sounds require document version 6"
                .into(),
        );
    }
    if project.uses_format_7() && project.format_version < 7 {
        add(
            "format_version".into(),
            "parametric shape options, custom transformation points, Bounce/Elastic and per-property eases, motion sync/snap options, and custom sound envelopes require document version 7"
                .into(),
        );
    }
    let mut scene_ids = HashSet::new();
    for (index, id) in project.scenes.iter().enumerate() {
        let path = format!("scenes[{index}]");
        if index == 0 && id != &project.root {
            add(
                path.clone(),
                "the first scene must be the root timeline".into(),
            );
        }
        if !scene_ids.insert(id) {
            add(path.clone(), "a scene is listed twice".into());
        }
        match project.symbol(id) {
            Some(symbol) if symbol.kind == SymbolKind::MovieClip => {}
            Some(_) => add(path, "a scene timeline must be a MovieClip symbol".into()),
            None => add(path, "scene symbol does not exist".into()),
        }
    }
    if !scene_ids.is_empty() {
        for symbol in &project.symbols {
            for element in symbol
                .timeline
                .layers
                .iter()
                .flat_map(|l| &l.keyframes)
                .flat_map(|k| &k.elements)
            {
                if let ElementKind::Instance { symbol_id, .. } = &element.kind
                    && scene_ids.contains(symbol_id)
                {
                    add(
                        format!("symbols.{}", symbol.id),
                        format!("scene '{symbol_id}' cannot be placed as an instance"),
                    );
                }
            }
        }
    }
    if project.has_program() && project.format_version < 2 {
        add(
            "format_version".into(),
            "project sources and class linkage require document version 2".into(),
        );
    }
    if !project.resources.is_empty() && project.format_version < 3 {
        add(
            "format_version".into(),
            "portable resources require document version 3".into(),
        );
    }
    if (!project.library_folders.is_empty() || project.symbols.iter().any(|s| !s.folder.is_empty()))
        && project.format_version < 4
    {
        add(
            "format_version".into(),
            "library folders require document version 4".into(),
        );
    }
    if project.library_folders.len() > 10000 {
        add(
            "library_folders".into(),
            "folder count exceeds 10000".into(),
        );
    }
    let mut library_folders = HashSet::new();
    for (index, folder) in project.library_folders.iter().enumerate() {
        if folder.is_empty() || !valid_library_folder(folder) {
            add(
                format!("library_folders[{index}]"),
                "expected a named, slash-separated logical folder path".into(),
            );
        } else if !library_folders.insert(folder) {
            add(
                format!("library_folders[{index}]"),
                "folder paths must be unique".into(),
            );
        }
    }
    if project.width == 0 || project.height == 0 || project.width > 65535 || project.height > 65535
    {
        add("stage".into(), "dimensions must be in 1..=65535".into());
    }
    if project.fps == 0 || project.fps > 1000 {
        add("fps".into(), "frame rate must be in 1..=1000".into());
    }
    if project.symbols.len() > 10000 {
        add("symbols".into(), "symbol count exceeds 10000".into());
    }
    if project.symbol(&project.root).is_none() {
        add("root".into(), "root symbol does not exist".into());
    }
    let mut identities: HashMap<&str, String> = HashMap::new();
    let mut resource_paths = HashSet::new();
    let mut resource_bytes = 0usize;
    if project.resources.len() > MAX_RESOURCES {
        add("resources".into(), "resource count exceeds 1024".into());
    }
    let encoded_bytes = project.resources.iter().fold(0usize, |total, resource| {
        total.saturating_add(resource.data_base64.len())
    });
    let encoded_size_ok = encoded_bytes <= crate::resources::MAX_ENCODED_TOTAL_BYTES;
    if !encoded_size_ok {
        add(
            "resources".into(),
            "encoded resources exceed the 64 MiB decoded total budget".into(),
        );
    }
    for (index, resource) in project.resources.iter().take(MAX_RESOURCES).enumerate() {
        let path = format!("resources[{index}]");
        register(
            &mut identities,
            &resource.id,
            &format!("resource:{index}"),
            &format!("{path}.id"),
            &mut add,
        );
        if let Err(error) = resource.validate_metadata() {
            add(path.clone(), error.to_string());
        }
        if !resource_paths.insert(resource.path.to_lowercase()) {
            add(
                format!("{path}.path"),
                "resource paths must be unique, including case-insensitive matches".into(),
            );
        }
        if encoded_size_ok {
            match resource.decoded_bytes() {
                Ok(bytes) => resource_bytes = resource_bytes.saturating_add(bytes.len()),
                Err(error) => add(format!("{path}.data_base64"), error.to_string()),
            }
        }
    }
    if resource_bytes > MAX_RESOURCE_TOTAL_BYTES {
        add(
            "resources".into(),
            "decoded resources exceed the 64 MiB total budget".into(),
        );
    }
    let mut image_dimensions: HashMap<&str, Result<(u32, u32), String>> = HashMap::new();
    let mut image_bytes = 0usize;
    let mut image_count = 0usize;
    let mut source_paths = HashSet::new();
    let mut source_bytes = 0usize;
    if project.sources.len() > 4096 {
        add("sources".into(), "source file count exceeds 4096".into());
    }
    for (index, source) in project.sources.iter().enumerate() {
        let path = format!("sources[{index}]");
        register(
            &mut identities,
            &source.id,
            &format!("source:{index}"),
            &format!("{path}.id"),
            &mut add,
        );
        let parts: Vec<_> = source.path.split('/').collect();
        if source.path.len() > 1024
            || source.path.contains(['\\', '\0', ':'])
            || parts
                .iter()
                .any(|part| part.is_empty() || matches!(*part, "." | ".."))
        {
            add(
                format!("{path}.path"),
                "source path must be a normalized relative project path".into(),
            );
        }
        if !source_paths.insert(source.path.to_lowercase()) {
            add(
                format!("{path}.path"),
                "source paths must be unique, including on case-insensitive filesystems".into(),
            );
        }
        source_bytes = source_bytes.saturating_add(source.source.len());
        if source.source.len() > 1_048_576 {
            add(
                format!("{path}.source"),
                "a source file exceeds 1 MiB".into(),
            );
        }
    }
    if source_bytes > 16 * 1024 * 1024 {
        add("sources".into(), "combined source exceeds 16 MiB".into());
    }
    let class_name_valid = |name: &str| {
        !name.is_empty()
            && name.len() <= 1024
            && name.split('.').all(|part| {
                !part.is_empty()
                    && part.chars().enumerate().all(|(i, c)| {
                        c == '_' || c == '$' || c.is_alphabetic() || (i > 0 && c.is_numeric())
                    })
            })
    };
    if let Some(name) = &project.document_class
        && !class_name_valid(name)
    {
        add(
            "document_class".into(),
            "expected a qualified class name".into(),
        );
    }
    let mut symbol_ids = HashSet::new();
    for symbol in &project.symbols {
        symbol_ids.insert(symbol.id.as_str());
    }
    let mut elements_count = 0usize;
    let mut export_ids = HashSet::new();
    let mut exported_classes = HashSet::new();
    for (si, symbol) in project.symbols.iter().enumerate() {
        let sp = format!("symbols[{si}]");
        if !valid_library_folder(&symbol.folder)
            || (symbol.id == project.root && !symbol.folder.is_empty())
        {
            add(
                format!("{sp}.folder"),
                "expected a logical library folder; the scene must remain at root".into(),
            );
        }
        if let Some(linkage) = &symbol.linkage_id {
            if linkage.is_empty() || linkage.len() > 512 || linkage.chars().any(char::is_control) {
                add(
                    format!("{sp}.linkage_id"),
                    "export identifier must contain 1..512 UTF-8 bytes and no control characters"
                        .into(),
                );
            }
            if !export_ids.insert(linkage.as_str()) {
                add(
                    format!("{sp}.linkage_id"),
                    "export identifier must be unique".into(),
                );
            }
            if symbol.id == project.root
                || !matches!(symbol.kind, SymbolKind::MovieClip | SymbolKind::Button)
            {
                add(
                    format!("{sp}.linkage_id"),
                    "only a non-root MovieClip or Button symbol can be exported for runtime creation".into(),
                );
            }
            if let Some(class) = &symbol.class_name
                && !exported_classes.insert(class.as_str())
            {
                add(
                    format!("{sp}.class_name"),
                    "an exported class must identify exactly one exported symbol".into(),
                );
            }
        }
        if let Some(name) = &symbol.class_name
            && !class_name_valid(name)
        {
            add(
                format!("{sp}.class_name"),
                "expected a qualified class name".into(),
            );
        }
        if symbol.id == project.root
            && symbol.class_name.is_some()
            && project.document_class.is_some()
            && symbol.class_name != project.document_class
        {
            add(
                format!("{sp}.class_name"),
                "root class linkage conflicts with the document class".into(),
            );
        }
        register(
            &mut identities,
            &symbol.id,
            &format!("symbol:{si}"),
            &format!("{sp}.id"),
            &mut add,
        );
        if symbol.timeline.frame_count == 0 || symbol.timeline.frame_count > 1_000_000 {
            add(
                format!("{sp}.timeline.frame_count"),
                "frame count must be in 1..=1000000".into(),
            );
        }
        let guides = &symbol.timeline.guides;
        if !guides.is_empty() && project.format_version < Guide::FORMAT_VERSION {
            add(
                format!("{sp}.timeline.guides"),
                "ruler guides require document version 6".into(),
            );
        }
        if guides.len() > Guide::LIMIT {
            add(
                format!("{sp}.timeline.guides"),
                format!("guide count exceeds {}", Guide::LIMIT),
            );
        }
        for (gi, guide) in guides.iter().enumerate() {
            if !guide.position.is_finite() || guide.position.abs() > Guide::MAX_POSITION {
                add(
                    format!("{sp}.timeline.guides[{gi}].position"),
                    "guide position must be finite and within ±1000000 px".into(),
                );
            }
        }
        for script in &symbol.timeline.scripts {
            if script.frame >= symbol.timeline.frame_count {
                add(
                    format!("{sp}.timeline.scripts"),
                    "script frame is outside the timeline".into(),
                );
            }
        }
        for error in crate::authored_layers::validate_layers(project, symbol) {
            add(format!("{sp}.timeline.layers"), error);
        }
        for (li, layer) in symbol.timeline.layers.iter().enumerate() {
            let lp = format!("{sp}.timeline.layers[{li}]");
            if let Some(end) = layer.properties.end_frame {
                if end > symbol.timeline.frame_count {
                    add(
                        format!("{lp}.properties.end_frame"),
                        "layer exposure ends outside the timeline".into(),
                    );
                }
                if project.format_version < 5 {
                    add(
                        format!("{lp}.properties.end_frame"),
                        "independent layer exposure requires document version 5".into(),
                    );
                }
            }
            register(
                &mut identities,
                &layer.id,
                &format!("layer:{si}:{li}"),
                &format!("{lp}.id"),
                &mut add,
            );
            let members: HashSet<_> = layer
                .keyframes
                .iter()
                .flat_map(|k| &k.elements)
                .map(|e| e.id.as_str())
                .collect();
            let mut group_ids = HashSet::new();
            let mut grouped = HashSet::new();
            for group in &layer.properties.groups {
                if group.id.is_empty() || !group_ids.insert(group.id.as_str()) {
                    add(
                        format!("{lp}.properties.groups"),
                        "group IDs must be nonempty and unique within a layer".into(),
                    );
                }
                if group.members.len() < 2
                    || group
                        .members
                        .iter()
                        .any(|id| !members.contains(id.as_str()) || !grouped.insert(id.as_str()))
                {
                    add(format!("{lp}.properties.groups"), "each group requires at least two distinct layer members; groups must not overlap".into());
                }
            }
            let mut keys = HashSet::new();
            for (ki, key) in layer.keyframes.iter().enumerate() {
                let kp = format!("{lp}.keyframes[{ki}]");
                if key.frame >= symbol.timeline.frame_count {
                    add(
                        format!("{kp}.frame"),
                        "keyframe is outside the timeline".into(),
                    );
                }
                if layer
                    .properties
                    .end_frame
                    .is_some_and(|end| key.frame >= end)
                {
                    add(format!("{kp}.frame"), "keyframe is outside this layer's exposure; explicitly extend the layer before inserting a key".into());
                }
                if !keys.insert(key.frame) {
                    add(format!("{kp}.frame"), "duplicate frame in a layer".into());
                }
                let check_easing = |ease: Easing,
                                    field_path: String,
                                    add: &mut dyn FnMut(String, String)| {
                    match ease {
                        Easing::CubicBezier { x1, y1, x2, y2 } => {
                            if ![x1, y1, x2, y2].iter().all(|v| v.is_finite())
                                || !(0.0..=1.0).contains(&x1)
                                || !(0.0..=1.0).contains(&x2)
                                || y1.abs() > 100.
                                || y2.abs() > 100.
                            {
                                add(
                                    field_path,
                                    "Bézier controls must be finite, x in [0,1], |y| <= 100"
                                        .into(),
                                );
                            }
                        }
                        Easing::Flash { acceleration } => {
                            if !acceleration.is_finite() || !(-100.0..=100.0).contains(&acceleration)
                            {
                                add(
                                    field_path.clone(),
                                    "Flash acceleration must be finite and within [-100,100]"
                                        .into(),
                                );
                            }
                            if project.format_version < 5 {
                                add(
                                    field_path,
                                    "Flash easing requires format version 5".into(),
                                );
                            }
                        }
                        _ => {}
                    }
                };
                if let Some(ease) = key.tween {
                    check_easing(ease, format!("{kp}.tween"), &mut add);
                }
                if !key.property_eases.is_empty() {
                    if !(key.tween.is_some()
                        && key.tween_kind == TweenKind::Motion
                        && key.keyed_properties.is_none())
                    {
                        add(
                            format!("{kp}.property_eases"),
                            "per-property eases are only valid on a motion tween start keyframe"
                                .into(),
                        );
                    }
                    let mut seen_props = HashSet::new();
                    for (pi, &(prop, ease)) in key.property_eases.iter().enumerate() {
                        if !seen_props.insert(prop) {
                            add(
                                format!("{kp}.property_eases[{pi}]"),
                                "duplicate property in property_eases".into(),
                            );
                        }
                        check_easing(ease, format!("{kp}.property_eases[{pi}]"), &mut add);
                    }
                }
                if let Some(sound) = &key.sound {
                    match project.resources.iter().find(|r| r.id == sound.resource) {
                        Some(resource) if resource.is_audio() => {}
                        Some(_) => add(
                            format!("{kp}.sound.resource"),
                            format!("resource '{}' is not audio (audio/*)", sound.resource),
                        ),
                        None => add(
                            format!("{kp}.sound.resource"),
                            format!("sound resource '{}' does not exist", sound.resource),
                        ),
                    }
                    if !(1..=FrameSound::MAX_LOOPS).contains(&sound.loops) {
                        add(
                            format!("{kp}.sound.loops"),
                            format!("repeat count must be in 1..={}", FrameSound::MAX_LOOPS),
                        );
                    }
                    if let Some(points) = &sound.envelope {
                        if !(1..=FrameSound::MAX_ENVELOPE_POINTS).contains(&points.len()) {
                            add(
                                format!("{kp}.sound.envelope"),
                                format!(
                                    "sound envelope must contain 1..={} points",
                                    FrameSound::MAX_ENVELOPE_POINTS
                                ),
                            );
                        }
                        let mut prev_pos = 0.0_f64;
                        for (pi, &[pos, left, right]) in points.iter().enumerate() {
                            if ![pos, left, right].iter().all(|v| v.is_finite())
                                || !(0.0..=1.0).contains(&pos)
                                || !(0.0..=1.0).contains(&left)
                                || !(0.0..=1.0).contains(&right)
                                || (pi > 0 && pos < prev_pos)
                            {
                                add(
                                    format!("{kp}.sound.envelope[{pi}]"),
                                    "envelope points must be finite, in 0..=1, and sorted by position"
                                        .into(),
                                );
                            }
                            if pos.is_finite() {
                                prev_pos = pos;
                            }
                        }
                    }
                }
                let mut placed = HashSet::new();
                for (ei, element) in key.elements.iter().enumerate() {
                    elements_count += 1;
                    let ep = format!("{kp}.elements[{ei}]");
                    register(
                        &mut identities,
                        &element.id,
                        &format!("element:{si}:{li}"),
                        &format!("{ep}.id"),
                        &mut add,
                    );
                    if !placed.insert(&element.id) {
                        add(
                            format!("{ep}.id"),
                            "duplicate element in one keyframe".into(),
                        );
                    }
                    let t = element.transform;
                    if let Some(accessibility) = &element.accessibility {
                        if accessibility.name.trim().is_empty()
                            || accessibility.name.chars().count() > 256
                        {
                            add(
                                format!("{ep}.accessibility.name"),
                                "accessible name must contain 1..=256 characters".into(),
                            );
                        }
                        if accessibility.description.chars().count() > 4096 {
                            add(
                                format!("{ep}.accessibility.description"),
                                "accessible description exceeds 4096 characters".into(),
                            );
                        }
                        if accessibility.tab_order > 10000 {
                            add(
                                format!("{ep}.accessibility.tab_order"),
                                "focus order must be in 0..=10000".into(),
                            );
                        }
                    }
                    if ![t.x, t.y, t.scale_x, t.scale_y, t.rotation, t.alpha, t.skew]
                        .iter()
                        .all(|v| v.is_finite())
                        || t.x.abs() > 1e12
                        || t.y.abs() > 1e12
                        || t.scale_x.abs() > 1e4
                        || t.scale_y.abs() > 1e4
                        || t.rotation.abs() > 1e9
                        || t.skew.abs() > 1e9
                        || t.color_transform.validate().is_err()
                        || !(0.0..=1.0).contains(&t.alpha)
                    {
                        add(
                            format!("{ep}.transform"),
                            "invalid or out-of-range transform".into(),
                        );
                    }
                    if project.format_version < 5
                        && (t.skew != 0.
                            || !t.color_transform.is_identity()
                            || matches!(element.kind, ElementKind::PaintedPath { .. }))
                    {
                        add(format!("{ep}.transform"), "affine axes, authored color transforms and gradient paints require document version 5".into());
                    }
                    if let Some([px, py]) = t.pivot
                        && (!px.is_finite()
                            || !py.is_finite()
                            || px.abs() > 1e9
                            || py.abs() > 1e9)
                    {
                        add(
                            format!("{ep}.transform.pivot"),
                            "transformation point coordinates must be finite and within +-1e9"
                                .into(),
                        );
                    }
                    if let Some(primitive) = t.primitive {
                        match primitive {
                            PrimitiveOptions::Rectangle(r) => {
                                if !matches!(element.kind, ElementKind::Rectangle { .. }) {
                                    add(
                                        format!("{ep}.transform.primitive"),
                                        "rectangle options apply only to rectangle elements".into(),
                                    );
                                }
                                if !r
                                    .corner_radii
                                    .iter()
                                    .all(|v| v.is_finite() && *v >= 0.0 && *v <= 1e9)
                                {
                                    add(
                                        format!("{ep}.transform.primitive.corner_radii"),
                                        "corner radii must be finite and in 0..=1e9".into(),
                                    );
                                }
                            }
                            PrimitiveOptions::Oval(o) => {
                                if !matches!(element.kind, ElementKind::Ellipse { .. }) {
                                    add(
                                        format!("{ep}.transform.primitive"),
                                        "oval options apply only to ellipse elements".into(),
                                    );
                                }
                                if !o.start_angle.is_finite()
                                    || !(0.0..=360.0).contains(&o.start_angle)
                                    || !o.end_angle.is_finite()
                                    || !(0.0..=360.0).contains(&o.end_angle)
                                {
                                    add(
                                        format!("{ep}.transform.primitive"),
                                        "oval sweep angles must be finite and in 0..=360".into(),
                                    );
                                }
                                if !o.inner_radius.is_finite()
                                    || !(0.0..=99.0).contains(&o.inner_radius)
                                {
                                    add(
                                        format!("{ep}.transform.primitive.inner_radius"),
                                        "oval inner radius must be finite and in 0..=99".into(),
                                    );
                                }
                            }
                        }
                    }
                    if let ElementKind::PaintedPath {
                        fill_gradient,
                        stroke_gradient,
                        ..
                    } = &element.kind
                    {
                        for paint in [fill_gradient, stroke_gradient].into_iter().flatten() {
                            if let Err(error) = paint.validate() {
                                add(format!("{ep}.kind"), error.into());
                            }
                        }
                    }
                    match &element.kind {
                        ElementKind::Video { .. } => {
                            if let Some(state) = VideoState::from_kind(&element.kind)
                                && let Err(error) = state.validate()
                            {
                                add(format!("{ep}.kind"), error.to_string());
                            }
                        }
                        ElementKind::Bitmap {
                            resource_id,
                            width,
                            height,
                            ..
                        } => {
                            let dimensions = image_dimensions.entry(resource_id.as_str()).or_insert_with(|| {
                                image_count += 1;
                                if image_count > 256 {return Err("authored bitmaps exceed the 256 buffer record budget".into())}
                                let resource=project.resources.iter().take(MAX_RESOURCES).find(|r|r.id==*resource_id).ok_or_else(||"missing bitmap resource".to_owned())?;
                                let (w,h)=resource.image_dimensions().map_err(|e|e.to_string())?;
                                let bytes=w as usize*h as usize*4;
                                if image_bytes+bytes>MAX_BITMAP_BYTES {return Err("authored image buffers exceed the 128 MiB runtime pixel budget".into())}
                                image_bytes+=bytes;
                                resource.decode_image().map_err(|e|e.to_string())?;
                                Ok((w,h))
                            });
                            match dimensions {
                                Ok((w, h)) if *width == *w && *height == *h => {}
                                Ok(_) => add(
                                    format!("{ep}.kind"),
                                    "bitmap dimensions must exactly match the decoded resource"
                                        .into(),
                                ),
                                Err(error) => add(format!("{ep}.kind.resource_id"), error.clone()),
                            }
                        }
                        ElementKind::Path {
                            commands,
                            stroke_width,
                            ..
                        }
                        | ElementKind::PaintedPath {
                            commands,
                            stroke_width,
                            ..
                        } => {
                            if let Err(error) = validate_path_commands(commands) {
                                add(format!("{ep}.kind.commands"), error.to_string());
                            }
                            if !stroke_width.is_finite()
                                || *stroke_width < 0.
                                || *stroke_width > 1e9
                            {
                                add(
                                    format!("{ep}.kind.stroke_width"),
                                    "stroke width must be finite and in 0..=1e9".into(),
                                );
                            }
                        }
                        ElementKind::Rectangle {
                            width,
                            height,
                            stroke_width,
                            ..
                        }
                        | ElementKind::Ellipse {
                            width,
                            height,
                            stroke_width,
                            ..
                        } => {
                            if ![width, height, stroke_width]
                                .iter()
                                .all(|v| v.is_finite() && **v >= 0. && **v <= 1e9)
                            {
                                add(
                                    format!("{ep}.kind"),
                                    "shape dimensions/stroke must be finite and in 0..=1e9".into(),
                                );
                            }
                        }
                        ElementKind::TextBox { .. } => {
                            if let Some(field) = TextFieldState::from_kind(&element.kind)
                                && let Err(error) = field.validate()
                            {
                                add(format!("{ep}.kind"), error.to_string());
                            }
                        }
                        ElementKind::Text { font_size, .. } => {
                            if !font_size.is_finite() || *font_size <= 0. || *font_size > 10000. {
                                add(
                                    format!("{ep}.kind.font_size"),
                                    "font size must be finite and in (0,10000]".into(),
                                );
                            }
                        }
                        ElementKind::Instance {
                            symbol_id,
                            playback,
                        } => {
                            if !symbol_ids.contains(symbol_id.as_str()) {
                                add(
                                    format!("{ep}.kind.symbol_id"),
                                    format!("missing symbol {symbol_id}"),
                                );
                            }
                            if let InstancePlayback::SingleFrame(frame)
                            | InstancePlayback::LoopFrom(frame)
                            | InstancePlayback::PlayOnceFrom(frame) = playback
                                && project
                                    .symbol(symbol_id)
                                    .is_some_and(|s| *frame >= s.timeline.frame_count)
                            {
                                add(
                                    format!("{ep}.kind.playback"),
                                    "single-frame index is outside the referenced symbol".into(),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    if image_dimensions.len() > 256 {
        add(
            "resources".into(),
            "authored bitmaps exceed the 256 buffer record budget".into(),
        );
    }
    if elements_count > 100000 {
        add(
            "symbols".into(),
            "document exceeds 100000 authored element records".into(),
        );
    }
    // Cycles are rejected even in unused library symbols, ensuring safe nesting later.
    let graph: HashMap<&str, Vec<&str>> = project
        .symbols
        .iter()
        .map(|s| {
            let refs = s
                .timeline
                .layers
                .iter()
                .flat_map(|l| &l.keyframes)
                .flat_map(|k| &k.elements)
                .filter_map(|e| match &e.kind {
                    ElementKind::Instance { symbol_id, .. } => Some(symbol_id.as_str()),
                    _ => None,
                })
                .collect();
            (s.id.as_str(), refs)
        })
        .collect();
    let mut active = HashSet::new();
    let mut depths = HashMap::new();
    let mut valid_graph = true;
    for symbol in &project.symbols {
        match depth(&symbol.id, &graph, &mut active, &mut depths) {
            Ok(d) if d > crate::evaluate::MAX_DEPTH => {
                valid_graph = false;
                add(
                    format!("symbol:{}", symbol.id),
                    "symbol nesting exceeds 64 levels".into(),
                );
            }
            Err(id) => {
                valid_graph = false;
                add(format!("symbol:{id}"), "cyclic symbol reference".into());
                break;
            }
            _ => {}
        }
    }
    if valid_graph {
        let mut counts = HashMap::new();
        for symbol in &project.symbols {
            if expanded_node_bound(project, &symbol.id, &mut counts) > crate::evaluate::MAX_NODES {
                add(
                    format!("symbol:{}", symbol.id),
                    "expanded scene may exceed 100000 nodes; reduce nested instance duplication"
                        .into(),
                );
            }
        }
    }
    issues
}

/// Conservative maximum across all keyframes, including independently timed children.
fn expanded_node_bound(project: &Project, id: &str, memo: &mut HashMap<Id, usize>) -> usize {
    if let Some(count) = memo.get(id) {
        return *count;
    }
    let Some(symbol) = project.symbol(id) else {
        return 1;
    };
    let mut count = 1;
    let cap = crate::evaluate::MAX_NODES + 1;
    for layer in &symbol.timeline.layers {
        let mut max_key = 0;
        for key in &layer.keyframes {
            let mut key_count = 0usize;
            for element in &key.elements {
                let nodes = match &element.kind {
                    ElementKind::Instance { symbol_id, .. } => {
                        expanded_node_bound(project, symbol_id, memo)
                    }
                    _ => 1,
                };
                key_count = key_count.saturating_add(nodes).min(cap);
            }
            max_key = max_key.max(key_count);
        }
        count = (count + max_key).min(cap);
    }
    memo.insert(id.into(), count);
    count
}

fn register<'a>(
    used: &mut HashMap<&'a str, String>,
    id: &'a str,
    owner: &str,
    path: &str,
    add: &mut impl FnMut(String, String),
) {
    if id.is_empty() || id.len() > 256 || id.contains('\0') {
        add(
            path.into(),
            "identity must be nonempty, <=256 bytes, without NUL".into(),
        );
    }
    if let Some(previous) = used.get(id) {
        if previous != owner {
            add(
                path.into(),
                format!("identity {id} is already owned by {previous}"),
            );
        }
    } else {
        used.insert(id, owner.into());
    }
}

fn depth<'a>(
    id: &'a str,
    graph: &HashMap<&'a str, Vec<&'a str>>,
    active: &mut HashSet<&'a str>,
    memo: &mut HashMap<&'a str, usize>,
) -> Result<usize, String> {
    if let Some(depth) = memo.get(id) {
        return Ok(*depth);
    }
    if !active.insert(id) {
        return Err(id.into());
    }
    // Hard recursion ceiling also protects validation of malicious inputs.
    if active.len() > crate::evaluate::MAX_DEPTH {
        active.remove(id);
        return Ok(crate::evaluate::MAX_DEPTH + 1);
    }
    let mut longest = 1;
    if let Some(children) = graph.get(id) {
        for child in children {
            longest = longest.max(1 + depth(child, graph, active, memo)?);
        }
    }
    active.remove(id);
    memo.insert(id, longest);
    Ok(longest)
}
