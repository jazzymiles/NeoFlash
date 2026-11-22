//! CS4+ XFL Inverse Kinematics (`layerType="armature"`, `animationType="IK pose"`,
//! `<IKTree>`, `<DOMIKTree>`, `<bones><DOMBone>`, `<ikNode>`, and
//! `<betweenFrameMatrixList>`) forward-kinematics and pose-matrix baking.
use super::{child, elements};
use neoflash_core::{
    Element, ElementKind, InstancePlayback, Keyframe, SymbolKind, Transform, transform_matrix,
};
use roxmltree::Node;
use std::collections::HashMap;

fn mul_affine(m1: [f64; 6], m2: [f64; 6]) -> [f64; 6] {
    [
        m1[0] * m2[0] + m1[2] * m2[1],
        m1[1] * m2[0] + m1[3] * m2[1],
        m1[0] * m2[2] + m1[2] * m2[3],
        m1[1] * m2[2] + m1[3] * m2[3],
        m1[0] * m2[4] + m1[2] * m2[5] + m1[4],
        m1[1] * m2[4] + m1[3] * m2[5] + m1[5],
    ]
}

fn inv_affine(m: [f64; 6]) -> Option<[f64; 6]> {
    let det = m[0] * m[3] - m[1] * m[2];
    if !det.is_finite() || det.abs() < 1e-10 {
        return None;
    }
    let inv_det = 1.0 / det;
    let a = m[3] * inv_det;
    let b = -m[1] * inv_det;
    let c = -m[2] * inv_det;
    let d = m[0] * inv_det;
    let tx = -(a * m[4] + c * m[5]);
    let ty = -(b * m[4] + d * m[5]);
    Some([a, b, c, d, tx, ty])
}

fn rigid_matrix(pos: [f64; 2], angle_rad: f64) -> [f64; 6] {
    let (s, c) = angle_rad.sin_cos();
    [c, s, -s, c, pos[0], pos[1]]
}

fn parse_num(node: Node<'_, '_>, attr: &str) -> Option<f64> {
    node.attribute(attr)?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && v.abs() <= 1e12)
}

fn parse_matrix_node(node: Node<'_, '_>) -> [f64; 6] {
    [
        parse_num(node, "a").unwrap_or(1.0),
        parse_num(node, "b").unwrap_or(0.0),
        parse_num(node, "c").unwrap_or(0.0),
        parse_num(node, "d").unwrap_or(1.0),
        parse_num(node, "tx").unwrap_or(0.0),
        parse_num(node, "ty").unwrap_or(0.0),
    ]
}

fn is_identity_matrix(m: [f64; 6]) -> bool {
    (m[0] - 1.0).abs() <= 1e-9
        && m[1].abs() <= 1e-9
        && m[2].abs() <= 1e-9
        && (m[3] - 1.0).abs() <= 1e-9
        && m[4].abs() <= 1e-9
        && m[5].abs() <= 1e-9
}

fn parse_float_csv(raw: &str) -> Vec<f64> {
    raw.split([',', ' ', '\t', '\r', '\n'])
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f64>().ok().filter(|v| v.is_finite()))
        .collect()
}

fn parse_bone_angle_rad(node: Node<'_, '_>) -> Option<f64> {
    for deg_attr in ["rotation", "rotationZ", "angleDeg", "headAngleDeg"] {
        if let Some(deg) = parse_num(node, deg_attr) {
            return Some(deg.to_radians());
        }
    }
    if let Some(ang) = parse_num(node, "angle") {
        // Adobe `<State angle="...">` stores radians; synthetic `<DOMBone>` / `<ikNode>`
        // fixtures with magnitudes > 2*pi use degrees.
        if !node.has_tag_name("State") && ang.abs() > std::f64::consts::TAU + 1e-3 {
            return Some(ang.to_radians());
        }
        return Some(ang);
    }
    None
}

fn parse_hex_or_dec_location_px(raw: &str) -> Option<[f64; 2]> {
    let (sx, sy) = raw.split_once(',')?;
    let tx = sx.trim();
    let ty = sy.trim();
    if tx.starts_with('#') || ty.starts_with('#') {
        let vx = super::timeline::parse_xfl_hex_24_8_token(tx)?;
        let vy = super::timeline::parse_xfl_hex_24_8_token(ty)?;
        // In CS4/CS5 `<IKNode location="#11.94, #E.4C">`, 0x1194 = 4500 twips = 225 px (`vx / 20.0`).
        Some([vx as f64 / 20.0, vy as f64 / 20.0])
    } else {
        Some([tx.parse::<f64>().ok()?, ty.parse::<f64>().ok()?])
    }
}

fn apply_ik_state_ease(ease_type: Option<&str>, ease_val: f64, u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    let a = (ease_val / 100.0).clamp(-1.0, 1.0);
    match ease_type.unwrap_or("linear") {
        "linear" if a == 0.0 => u,
        _ => {
            if a == 0.0 {
                u
            } else if a > 0.0 {
                u * (1.0 - a) + (1.0 - (1.0 - u) * (1.0 - u)) * a
            } else {
                let k = -a;
                u * (1.0 - k) + (u * u) * k
            }
        }
    }
}

fn advance_graphic_playback(
    el: &mut Element,
    frame_offset: u32,
    symbols: &HashMap<String, (String, SymbolKind)>,
) {
    if let ElementKind::Instance {
        symbol_id,
        playback,
    } = &mut el.kind
    {
        let is_graphic = symbols
            .values()
            .any(|(sid, kind)| sid == symbol_id && *kind == SymbolKind::Graphic);
        if is_graphic {
            *playback = match *playback {
                InstancePlayback::Loop => InstancePlayback::LoopFrom(frame_offset),
                InstancePlayback::LoopFrom(f0) => {
                    InstancePlayback::LoopFrom(f0.saturating_add(frame_offset))
                }
                InstancePlayback::PlayOnce => InstancePlayback::PlayOnceFrom(frame_offset),
                InstancePlayback::PlayOnceFrom(f0) => {
                    InstancePlayback::PlayOnceFrom(f0.saturating_add(frame_offset))
                }
                other => other,
            };
        }
    }
}

#[derive(Clone, Debug)]
struct IkStateSample {
    duration: u32,
    x_px: Option<f64>,
    y_px: Option<f64>,
    angle_rad: f64,
    ease_type: Option<String>,
    ease_val: f64,
}

#[derive(Clone, Debug)]
pub(super) struct IkBoneNode {
    id: String,
    bone_name: String,
    reference_id: Option<String>,
    target_name: Option<String>,
    parent: Option<usize>,
    bind_world_pos: Option<[f64; 2]>,
    bind_length: Option<f64>,
    bind_tail_pos: Option<[f64; 2]>,
    bind_angle_rad: f64,
    x_array_px: Vec<f64>,
    y_array_px: Vec<f64>,
    angle_array_rad: Vec<f64>,
    states: Vec<IkStateSample>,
    is_adobe_twip_state: bool,
}

#[derive(Clone, Debug)]
pub(super) struct PlacedElementMeta {
    pub reference_id: Option<String>,
    pub xml_name: Option<String>,
    pub library_item: Option<String>,
    pub one_to_one: bool,
}

fn collect_adobe_ik_nodes(node: Node<'_, '_>, parent: Option<usize>, out: &mut Vec<IkBoneNode>) {
    let idx = out.len();
    let id = node
        .attribute("id")
        .or_else(|| node.attribute("name"))
        .unwrap_or("")
        .to_owned();
    let bone_name = node.attribute("boneName").unwrap_or(&id).to_owned();
    let reference_id = node
        .attribute("referenceID")
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let target_name = node
        .attribute("target")
        .or_else(|| node.attribute("element"))
        .or_else(|| node.attribute("instanceName"))
        .or_else(|| node.attribute("symbol"))
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let world_mat_pos = child(node, "worldMatrix")
        .and_then(|wm| child(wm, "Matrix"))
        .map(|m| {
            [
                parse_num(m, "tx").unwrap_or(0.0),
                parse_num(m, "ty").unwrap_or(0.0),
            ]
        });
    let loc_pos = node
        .attribute("location")
        .and_then(parse_hex_or_dec_location_px);
    let direct_pos = match (
        parse_num(node, "headX").or_else(|| parse_num(node, "x")),
        parse_num(node, "headY").or_else(|| parse_num(node, "y")),
    ) {
        (Some(x), Some(y)) => Some([x, y]),
        _ => None,
    };
    let tail_pos = match (parse_num(node, "tailX"), parse_num(node, "tailY")) {
        (Some(tx), Some(ty)) => Some([tx, ty]),
        _ => None,
    };
    let bind_length = parse_num(node, "length");

    let x_array_px: Vec<f64> = node
        .attribute("xArray")
        .map(|s| parse_float_csv(s).into_iter().map(|v| v / 20.0).collect())
        .unwrap_or_default();
    let y_array_px: Vec<f64> = node
        .attribute("yArray")
        .map(|s| parse_float_csv(s).into_iter().map(|v| v / 20.0).collect())
        .unwrap_or_default();
    let angle_array_rad: Vec<f64> = node
        .attribute("angleArray")
        .map(parse_float_csv)
        .unwrap_or_default();

    let mut states = Vec::new();
    if let Some(states_wrap) = child(node, "states") {
        for st in elements(states_wrap).filter(|n| n.has_tag_name("State")) {
            let dur = parse_num(st, "duration")
                .map(|d| d.max(1.0).round() as u32)
                .unwrap_or(1);
            states.push(IkStateSample {
                duration: dur,
                x_px: parse_num(st, "x").map(|v| v / 20.0),
                y_px: parse_num(st, "y").map(|v| v / 20.0),
                angle_rad: parse_bone_angle_rad(st).unwrap_or(0.0),
                ease_type: st.attribute("easeType").map(str::to_owned),
                ease_val: parse_num(st, "easeValue")
                    .or_else(|| parse_num(st, "strength"))
                    .unwrap_or(0.0),
            });
        }
    }

    let bind_angle_rad = parse_bone_angle_rad(node)
        .or_else(|| angle_array_rad.first().copied())
        .or_else(|| states.first().map(|s| s.angle_rad))
        .or_else(|| {
            child(node, "worldMatrix")
                .and_then(|wm| child(wm, "Matrix"))
                .map(|m| {
                    let a = parse_num(m, "a").unwrap_or(1.0);
                    let b = parse_num(m, "b").unwrap_or(0.0);
                    b.atan2(a)
                })
        })
        .unwrap_or(0.0);

    out.push(IkBoneNode {
        id,
        bone_name,
        reference_id,
        target_name,
        parent,
        bind_world_pos: world_mat_pos.or(loc_pos).or(direct_pos),
        bind_length,
        bind_tail_pos: tail_pos,
        bind_angle_rad,
        x_array_px,
        y_array_px,
        angle_array_rad,
        states,
        is_adobe_twip_state: true,
    });

    if let Some(children_wrap) = child(node, "childNodes") {
        for ch in elements(children_wrap).filter(|n| {
            n.has_tag_name("ChildNode") || n.has_tag_name("IKNode") || n.has_tag_name("ikNode")
        }) {
            collect_adobe_ik_nodes(ch, Some(idx), out);
        }
    }
    for ch in elements(node).filter(|n| {
        n.has_tag_name("DOMBone") || n.has_tag_name("IKNode") || n.has_tag_name("ikNode")
    }) {
        collect_adobe_ik_nodes(ch, Some(idx), out);
    }
}

fn parse_dom_bones_list(container: Node<'_, '_>) -> Vec<IkBoneNode> {
    let mut raw_bones: Vec<(Node<'_, '_>, String, Option<String>)> = Vec::new();
    let collect_from = if let Some(bones_wrap) = child(container, "bones") {
        bones_wrap
    } else {
        container
    };
    for b in elements(collect_from).filter(|n| {
        matches!(
            n.tag_name().name(),
            "DOMBone" | "Bone" | "IKNode" | "ikNode"
        )
    }) {
        let id = b
            .attribute("id")
            .or_else(|| b.attribute("name"))
            .or_else(|| b.attribute("boneName"))
            .unwrap_or("")
            .to_owned();
        let parent_name = b
            .attribute("parent")
            .or_else(|| b.attribute("parentBone"))
            .or_else(|| b.attribute("parentID"))
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        raw_bones.push((b, id, parent_name));
    }
    if raw_bones.is_empty() {
        return Vec::new();
    }
    // Check if nested `<ChildNode>` / `<DOMBone>` hierarchy is used instead of flat `parent="..."`.
    if raw_bones.len() == 1
        && (child(raw_bones[0].0, "childNodes").is_some()
            || elements(raw_bones[0].0).any(|c| {
                matches!(
                    c.tag_name().name(),
                    "DOMBone" | "ChildNode" | "IKNode" | "ikNode"
                )
            }))
    {
        let mut out = Vec::new();
        collect_adobe_ik_nodes(raw_bones[0].0, None, &mut out);
        return out;
    }

    let mut out = Vec::with_capacity(raw_bones.len());
    for (b, id, _) in &raw_bones {
        let bone_name = b.attribute("boneName").unwrap_or(id).to_owned();
        let reference_id = b
            .attribute("referenceID")
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let target_name = b
            .attribute("target")
            .or_else(|| b.attribute("element"))
            .or_else(|| b.attribute("instanceName"))
            .or_else(|| b.attribute("symbol"))
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let bind_world_pos = match (
            parse_num(*b, "headX").or_else(|| parse_num(*b, "x")),
            parse_num(*b, "headY").or_else(|| parse_num(*b, "y")),
        ) {
            (Some(x), Some(y)) => Some([x, y]),
            _ => b
                .attribute("location")
                .and_then(parse_hex_or_dec_location_px),
        };
        let bind_tail_pos = match (parse_num(*b, "tailX"), parse_num(*b, "tailY")) {
            (Some(tx), Some(ty)) => Some([tx, ty]),
            _ => None,
        };
        let bind_length = parse_num(*b, "length");
        let bind_angle_rad = parse_bone_angle_rad(*b).unwrap_or(0.0);

        let mut states = Vec::new();
        if let Some(states_wrap) = child(*b, "states") {
            for st in elements(states_wrap).filter(|n| n.has_tag_name("State")) {
                let dur = parse_num(st, "duration")
                    .map(|d| d.max(1.0).round() as u32)
                    .unwrap_or(1);
                states.push(IkStateSample {
                    duration: dur,
                    x_px: parse_num(st, "x"),
                    y_px: parse_num(st, "y"),
                    angle_rad: parse_bone_angle_rad(st).unwrap_or(0.0),
                    ease_type: st.attribute("easeType").map(str::to_owned),
                    ease_val: parse_num(st, "easeValue")
                        .or_else(|| parse_num(st, "strength"))
                        .unwrap_or(0.0),
                });
            }
        }

        out.push(IkBoneNode {
            id: id.clone(),
            bone_name,
            reference_id,
            target_name,
            parent: None,
            bind_world_pos,
            bind_length,
            bind_tail_pos,
            bind_angle_rad,
            x_array_px: b
                .attribute("xArray")
                .map(|s| parse_float_csv(s).into_iter().map(|v| v / 20.0).collect())
                .unwrap_or_default(),
            y_array_px: b
                .attribute("yArray")
                .map(|s| parse_float_csv(s).into_iter().map(|v| v / 20.0).collect())
                .unwrap_or_default(),
            angle_array_rad: b
                .attribute("angleArray")
                .map(parse_float_csv)
                .unwrap_or_default(),
            states,
            is_adobe_twip_state: false,
        });
    }
    for (idx, (_, _, parent_opt)) in raw_bones.iter().enumerate() {
        if let Some(pname) = parent_opt {
            out[idx].parent = out
                .iter()
                .enumerate()
                .position(|(j, cand)| j != idx && (cand.id == *pname || cand.bone_name == *pname));
        }
    }
    out
}

pub(super) fn parse_tree_bones(tree_node: Node<'_, '_>) -> Vec<IkBoneNode> {
    if child(tree_node, "bones").is_some()
        || elements(tree_node).any(|n| n.has_tag_name("DOMBone") || n.has_tag_name("Bone"))
    {
        return parse_dom_bones_list(tree_node);
    }
    let mut out = Vec::new();
    for root_ik in elements(tree_node)
        .filter(|n| matches!(n.tag_name().name(), "IKNode" | "ikNode" | "ChildNode"))
    {
        collect_adobe_ik_nodes(root_ik, None, &mut out);
    }
    out
}

fn match_bone_to_element(
    bone_idx: usize,
    bones: &[IkBoneNode],
    elems: &[Element],
    metas: &[PlacedElementMeta],
    used_elems: &mut [bool],
) -> Option<usize> {
    let bone = &bones[bone_idx];
    if let Some(ref rid) = bone.reference_id
        && let Some(pos) = metas
            .iter()
            .enumerate()
            .position(|(i, m)| !used_elems[i] && m.reference_id.as_deref() == Some(rid.as_str()))
    {
        used_elems[pos] = true;
        return Some(pos);
    }
    for candidate_key in [
        bone.target_name.as_deref(),
        (!bone.id.is_empty()).then_some(bone.id.as_str()),
        (!bone.bone_name.is_empty()).then_some(bone.bone_name.as_str()),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(pos) = elems.iter().enumerate().position(|(i, el)| {
            !used_elems[i]
                && (el.name == candidate_key
                    || metas
                        .get(i)
                        .and_then(|m| m.xml_name.as_deref())
                        .is_some_and(|n| n == candidate_key)
                    || metas
                        .get(i)
                        .and_then(|m| m.library_item.as_deref())
                        .is_some_and(|n| n == candidate_key))
        }) {
            used_elems[pos] = true;
            return Some(pos);
        }
    }
    None
}

fn bind_bones_to_elements(
    bones: &[IkBoneNode],
    elems: &[Element],
    metas: &[PlacedElementMeta],
) -> Vec<Option<usize>> {
    let mut mapping = vec![None; bones.len()];
    let mut used = vec![false; elems.len()];
    for (b_idx, slot) in mapping.iter_mut().enumerate() {
        *slot = match_bone_to_element(b_idx, bones, elems, metas, &mut used);
    }
    // Fallback positional pairing for unmapped active bones (skipping dummy container roots
    // in Adobe `<IKTree>` when `bones.len() == elems.len() + 1` and bone 0 has no states/arrays).
    if mapping.iter().all(Option::is_none) && !elems.is_empty() {
        let start_bone = if bones.len() == elems.len() + 1
            && bones[0].states.is_empty()
            && bones[0].angle_array_rad.is_empty()
            && bones[0].reference_id.is_none()
        {
            1
        } else {
            0
        };
        for (offset, slot) in mapping.iter_mut().skip(start_bone).enumerate() {
            if offset < elems.len() && !used[offset] {
                *slot = Some(offset);
                used[offset] = true;
            }
        }
    }
    mapping
}

fn eval_bone_state_at_frame(states: &[IkStateSample], f: u32) -> (Option<[f64; 2]>, f64) {
    if states.is_empty() {
        return (None, 0.0);
    }
    if states.len() == 1 {
        let s0 = &states[0];
        let xy = match (s0.x_px, s0.y_px) {
            (Some(x), Some(y)) => Some([x, y]),
            _ => None,
        };
        return (xy, s0.angle_rad);
    }
    let mut cursor = 0u32;
    for i in 0..states.len() - 1 {
        let dur = states[i].duration.max(1);
        let next_f = cursor.saturating_add(dur);
        if f <= next_f || i + 2 == states.len() {
            let raw_u = if f >= next_f {
                1.0
            } else {
                f64::from(f.saturating_sub(cursor)) / f64::from(dur)
            };
            let u = apply_ik_state_ease(states[i].ease_type.as_deref(), states[i].ease_val, raw_u);
            let a0 = states[i].angle_rad;
            let a1 = states[i + 1].angle_rad;
            // Shortest angular interpolation in radians
            let mut da = (a1 - a0) % std::f64::consts::TAU;
            if da > std::f64::consts::PI {
                da -= std::f64::consts::TAU;
            } else if da < -std::f64::consts::PI {
                da += std::f64::consts::TAU;
            }
            let ang = a0 + da * u;
            let xy = match (
                states[i].x_px,
                states[i].y_px,
                states[i + 1].x_px,
                states[i + 1].y_px,
            ) {
                (Some(x0), Some(y0), Some(x1), Some(y1)) => {
                    Some([x0 + (x1 - x0) * u, y0 + (y1 - y0) * u])
                }
                (Some(x0), Some(y0), _, _) => Some([x0, y0]),
                _ => None,
            };
            return (xy, ang);
        }
        cursor = next_f;
    }
    let last = states.last().unwrap();
    let xy = match (last.x_px, last.y_px) {
        (Some(x), Some(y)) => Some([x, y]),
        _ => None,
    };
    (xy, last.angle_rad)
}

/// Sample a single `<DOMFrame>` IK pose span (`duration > 1`) using `<betweenFrameMatrixList>`
/// or `<IKTree>` / `<DOMIKTree>` arrays/states.
#[allow(clippy::too_many_arguments)]
pub(super) fn sample_single_frame_ik_span(
    frame_node: Node<'_, '_>,
    key: &Keyframe,
    metas: &[PlacedElementMeta],
    layer_bones: &[IkBoneNode],
    duration: u32,
    symbols: &HashMap<String, (String, SymbolKind)>,
    budget: &mut usize,
) -> Option<Vec<Keyframe>> {
    if duration <= 1 || key.elements.is_empty() {
        return None;
    }
    let count = (duration - 1) as usize;
    let elem_bytes: usize = key
        .elements
        .iter()
        .map(|el| serde_json::to_vec(el).map_or(256, |v| v.len()))
        .sum();
    let cost = count.saturating_mul(elem_bytes.div_ceil(2048).max(1));
    if count > 4095 || cost > *budget {
        return None;
    }

    // 1. Prefer `<betweenFrameMatrixList>` when it has `elements.len() * duration` matrices
    //    and every XML element mapped 1-to-1 to a `Keyframe` element.
    if metas.len() == key.elements.len()
        && metas.iter().all(|m| m.one_to_one)
        && let Some(bf_list) = child(frame_node, "betweenFrameMatrixList")
    {
        let mats: Vec<[f64; 6]> = elements(bf_list)
            .filter(|n| n.has_tag_name("Matrix"))
            .map(parse_matrix_node)
            .collect();
        let expected = key.elements.len().saturating_mul(duration as usize);
        if mats.len() == expected && mats.iter().any(|m| !is_identity_matrix(*m)) {
            let mut poses = Vec::with_capacity(count);
            for f in 1..duration {
                let mut frame_elems = Vec::with_capacity(key.elements.len());
                for (e_idx, base_el) in key.elements.iter().enumerate() {
                    let mut el = base_el.clone();
                    let m0 = mats[e_idx * (duration as usize)];
                    let mf = mats[e_idx * (duration as usize) + (f as usize)];
                    let base_m = transform_matrix(base_el.transform);
                    let delta = inv_affine(m0).map_or(mf, |inv0| mul_affine(mf, inv0));
                    let composed = mul_affine(delta, base_m);
                    if let Some(mut tr) = Transform::from_matrix(composed) {
                        tr.alpha = base_el.transform.alpha;
                        tr.color_transform = base_el.transform.color_transform;
                        el.transform = tr;
                    }
                    advance_graphic_playback(&mut el, f, symbols);
                    frame_elems.push(el);
                }
                poses.push(Keyframe {
                    frame: key.frame.saturating_add(f),
                    elements: frame_elems,
                    ..Default::default()
                });
            }
            *budget -= cost;
            return Some(poses);
        }
    }

    // 2. Otherwise check `<IKTree>` / `<DOMIKTree>` on this frame (or inherited `layer_bones`).
    let frame_bones = child(frame_node, "DOMIKTree")
        .or_else(|| child(frame_node, "IKTree"))
        .map(parse_tree_bones)
        .unwrap_or_default();
    let bones = if !frame_bones.is_empty() {
        frame_bones.as_slice()
    } else {
        layer_bones
    };
    if bones.is_empty() {
        return None;
    }

    let mapping = bind_bones_to_elements(bones, &key.elements, metas);
    if mapping.iter().all(Option::is_none) {
        return None;
    }

    let has_arrays = bones
        .iter()
        .any(|b| b.angle_array_rad.len() >= 2 || b.x_array_px.len() >= 2);
    let has_multi_states = bones.iter().any(|b| b.states.len() >= 2);
    if !has_arrays && !has_multi_states {
        return None;
    }

    // Solve bone world matrices `[a, b, c, d, tx, ty]` at every frame `0..duration`.
    let solve_at_frame = |f: u32, bind_offsets: Option<&[[f64; 2]]>| -> Vec<[f64; 6]> {
        let mut world_pos = vec![[0.0, 0.0]; bones.len()];
        let mut world_ang = vec![0.0; bones.len()];
        for b_idx in 0..bones.len() {
            let b = &bones[b_idx];
            if !b.angle_array_rad.is_empty() && !b.x_array_px.is_empty() && !b.y_array_px.is_empty()
            {
                let fi = (f as usize).min(b.angle_array_rad.len() - 1);
                let xi = (f as usize).min(b.x_array_px.len() - 1);
                let yi = (f as usize).min(b.y_array_px.len() - 1);
                world_pos[b_idx] = [b.x_array_px[xi], b.y_array_px[yi]];
                world_ang[b_idx] = b.angle_array_rad[fi];
                continue;
            }
            let (st_xy, st_ang) = if !b.states.is_empty() {
                eval_bone_state_at_frame(&b.states, f)
            } else {
                (None, b.bind_angle_rad)
            };
            if let Some(p_idx) = b.parent {
                let p_pos = world_pos[p_idx];
                let p_ang = world_ang[p_idx];
                let local_off = if b.is_adobe_twip_state {
                    st_xy.unwrap_or_else(|| {
                        bind_offsets
                            .map(|offs| offs[b_idx])
                            .unwrap_or([bones[p_idx].bind_length.unwrap_or(0.0), 0.0])
                    })
                } else {
                    bind_offsets
                        .map(|offs| offs[b_idx])
                        .or(st_xy)
                        .unwrap_or([bones[p_idx].bind_length.unwrap_or(0.0), 0.0])
                };
                let (ps, pc) = p_ang.sin_cos();
                world_pos[b_idx] = [
                    p_pos[0] + pc * local_off[0] - ps * local_off[1],
                    p_pos[1] + ps * local_off[0] + pc * local_off[1],
                ];
                world_ang[b_idx] = p_ang + st_ang;
            } else {
                let base_xy = st_xy
                    .or(b.bind_world_pos)
                    .or_else(|| {
                        mapping[b_idx]
                            .map(|ei| [key.elements[ei].transform.x, key.elements[ei].transform.y])
                    })
                    .unwrap_or([0.0, 0.0]);
                world_pos[b_idx] = base_xy;
                world_ang[b_idx] = st_ang;
            }
        }
        (0..bones.len())
            .map(|i| rigid_matrix(world_pos[i], world_ang[i]))
            .collect()
    };

    // Precompute local joint offsets in parent bone space at frame 0 for non-Adobe `<DOMBone>` chains.
    let mut init_world_pos = vec![[0.0_f64, 0.0]; bones.len()];
    let mut init_world_ang = vec![0.0_f64; bones.len()];
    let mut local_bind_offsets = vec![[0.0_f64, 0.0]; bones.len()];
    for b_idx in 0..bones.len() {
        let b = &bones[b_idx];
        let elem_pos =
            mapping[b_idx].map(|ei| [key.elements[ei].transform.x, key.elements[ei].transform.y]);
        if let Some(p_idx) = b.parent {
            let p_pos = init_world_pos[p_idx];
            let p_ang = init_world_ang[p_idx];
            let (ps, pc) = p_ang.sin_cos();
            let candidate_world = b.bind_world_pos.or(bones[p_idx].bind_tail_pos).or(elem_pos);
            let local_off = if let Some(len) = bones[p_idx].bind_length
                && b.bind_world_pos.is_none()
                && bones[p_idx].bind_tail_pos.is_none()
            {
                [len, 0.0]
            } else if let Some(wxy) = candidate_world {
                let dx = wxy[0] - p_pos[0];
                let dy = wxy[1] - p_pos[1];
                [pc * dx + ps * dy, -ps * dx + pc * dy]
            } else {
                [0.0, 0.0]
            };
            local_bind_offsets[b_idx] = local_off;
            init_world_pos[b_idx] = [
                p_pos[0] + pc * local_off[0] - ps * local_off[1],
                p_pos[1] + ps * local_off[0] + pc * local_off[1],
            ];
            let (_, st0_ang) = if !b.states.is_empty() {
                eval_bone_state_at_frame(&b.states, 0)
            } else {
                (None, b.bind_angle_rad)
            };
            init_world_ang[b_idx] = p_ang + st0_ang;
        } else {
            init_world_pos[b_idx] = b.bind_world_pos.or(elem_pos).unwrap_or([0.0, 0.0]);
            let (_, st0_ang) = if !b.states.is_empty() {
                eval_bone_state_at_frame(&b.states, 0)
            } else {
                (None, b.bind_angle_rad)
            };
            init_world_ang[b_idx] = st0_ang;
        }
    }

    let bone_mats_0 = solve_at_frame(0, Some(&local_bind_offsets));
    let mut elem_to_bone = vec![None; key.elements.len()];
    for (b_idx, maybe_e) in mapping.iter().enumerate() {
        if let Some(e_idx) = *maybe_e {
            elem_to_bone[e_idx] = Some(b_idx);
        }
    }

    let mut any_changed = false;
    let mut poses = Vec::with_capacity(count);
    for f in 1..duration {
        let bone_mats_f = solve_at_frame(f, Some(&local_bind_offsets));
        let mut frame_elems = Vec::with_capacity(key.elements.len());
        for (e_idx, base_el) in key.elements.iter().enumerate() {
            let mut el = base_el.clone();
            if let Some(b_idx) = elem_to_bone[e_idx]
                && let Some(inv_b0) = inv_affine(bone_mats_0[b_idx])
            {
                let base_m = transform_matrix(base_el.transform);
                let delta = mul_affine(bone_mats_f[b_idx], inv_b0);
                if !is_identity_matrix(delta) {
                    any_changed = true;
                }
                let composed = mul_affine(delta, base_m);
                if let Some(mut tr) = Transform::from_matrix(composed) {
                    tr.alpha = base_el.transform.alpha;
                    tr.color_transform = base_el.transform.color_transform;
                    el.transform = tr;
                }
            }
            advance_graphic_playback(&mut el, f, symbols);
            frame_elems.push(el);
        }
        poses.push(Keyframe {
            frame: key.frame.saturating_add(f),
            elements: frame_elems,
            ..Default::default()
        });
    }

    if !any_changed {
        return None;
    }
    *budget -= cost;
    Some(poses)
}

/// Sample multi-keyframe `<DOMLayer layerType="armature">` pose keyframes where
/// consecutive `<DOMFrame>` entries carry `<ikNode>` / `<DOMBone>` / `<DOMIKTree>` poses!
#[derive(Clone, Debug, Default)]
pub(super) struct FrameIkPoseOverride {
    pub bone_angles_rad: HashMap<String, f64>,
    pub root_pos_px: HashMap<String, [f64; 2]>,
}

pub(super) fn parse_frame_ik_pose_override(frame_node: Node<'_, '_>) -> FrameIkPoseOverride {
    let mut out = FrameIkPoseOverride::default();
    for ch in frame_node.descendants().filter(Node::is_element) {
        if matches!(
            ch.tag_name().name(),
            "ikNode" | "IKNode" | "ChildNode" | "DOMBone" | "Bone"
        ) {
            let key = ch
                .attribute("bone")
                .or_else(|| ch.attribute("id"))
                .or_else(|| ch.attribute("name"))
                .or_else(|| ch.attribute("boneName"))
                .or_else(|| ch.attribute("referenceID"))
                .unwrap_or("");
            if key.is_empty() {
                continue;
            }
            if let Some(ang) = parse_bone_angle_rad(ch) {
                out.bone_angles_rad.insert(key.to_owned(), ang);
            }
            if let (Some(x), Some(y)) = (
                parse_num(ch, "x").or_else(|| parse_num(ch, "headX")),
                parse_num(ch, "y").or_else(|| parse_num(ch, "headY")),
            ) {
                out.root_pos_px.insert(key.to_owned(), [x, y]);
            }
        }
    }
    out
}

pub(super) fn bake_multi_keyframe_armature_layer(
    keyframes: &mut Vec<Keyframe>,
    metas_per_key: &[Vec<PlacedElementMeta>],
    pose_overrides: &[FrameIkPoseOverride],
    layer_bones: &[IkBoneNode],
    symbols: &HashMap<String, (String, SymbolKind)>,
    budget: &mut usize,
) -> bool {
    if layer_bones.is_empty() || keyframes.is_empty() {
        return false;
    }
    let has_any_override = pose_overrides
        .iter()
        .any(|ov| !ov.bone_angles_rad.is_empty() || !ov.root_pos_px.is_empty());
    if !has_any_override {
        return false;
    }

    let base_elems = keyframes[0].elements.clone();
    if base_elems.is_empty() {
        return false;
    }
    let empty_meta = Vec::new();
    let metas0 = metas_per_key.first().unwrap_or(&empty_meta);
    let mapping = bind_bones_to_elements(layer_bones, &base_elems, metas0);
    if mapping.iter().all(Option::is_none) {
        return false;
    }

    // Compute local bind offsets at keyframe 0.
    let mut init_world_pos = vec![[0.0_f64, 0.0]; layer_bones.len()];
    let mut init_world_ang = vec![0.0_f64; layer_bones.len()];
    let mut local_bind_offsets = vec![[0.0_f64, 0.0]; layer_bones.len()];
    let lookup_bone_angle = |b: &IkBoneNode, ov: &FrameIkPoseOverride| -> Option<f64> {
        for k in [
            Some(b.id.as_str()),
            Some(b.bone_name.as_str()),
            b.reference_id.as_deref(),
            b.target_name.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if !k.is_empty()
                && let Some(&a) = ov.bone_angles_rad.get(k)
            {
                return Some(a);
            }
        }
        None
    };
    let lookup_bone_pos = |b: &IkBoneNode, ov: &FrameIkPoseOverride| -> Option<[f64; 2]> {
        for k in [
            Some(b.id.as_str()),
            Some(b.bone_name.as_str()),
            b.reference_id.as_deref(),
            b.target_name.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if !k.is_empty()
                && let Some(&xy) = ov.root_pos_px.get(k)
            {
                return Some(xy);
            }
        }
        None
    };

    let ov0 = pose_overrides.first().cloned().unwrap_or_default();
    for b_idx in 0..layer_bones.len() {
        let b = &layer_bones[b_idx];
        let elem_pos =
            mapping[b_idx].map(|ei| [base_elems[ei].transform.x, base_elems[ei].transform.y]);
        let a0 = lookup_bone_angle(b, &ov0).unwrap_or(b.bind_angle_rad);
        if let Some(p_idx) = b.parent {
            let p_pos = init_world_pos[p_idx];
            let p_ang = init_world_ang[p_idx];
            let (ps, pc) = p_ang.sin_cos();
            let candidate_world = b
                .bind_world_pos
                .or(layer_bones[p_idx].bind_tail_pos)
                .or(elem_pos);
            let local_off = if let Some(len) = layer_bones[p_idx].bind_length
                && b.bind_world_pos.is_none()
                && layer_bones[p_idx].bind_tail_pos.is_none()
            {
                [len, 0.0]
            } else if let Some(wxy) = candidate_world {
                let dx = wxy[0] - p_pos[0];
                let dy = wxy[1] - p_pos[1];
                [pc * dx + ps * dy, -ps * dx + pc * dy]
            } else {
                [0.0, 0.0]
            };
            local_bind_offsets[b_idx] = local_off;
            init_world_pos[b_idx] = [
                p_pos[0] + pc * local_off[0] - ps * local_off[1],
                p_pos[1] + ps * local_off[0] + pc * local_off[1],
            ];
            init_world_ang[b_idx] = p_ang + a0;
        } else {
            init_world_pos[b_idx] = lookup_bone_pos(b, &ov0)
                .or(b.bind_world_pos)
                .or(elem_pos)
                .unwrap_or([0.0, 0.0]);
            init_world_ang[b_idx] = a0;
        }
    }
    let bone_mats_0: Vec<[f64; 6]> = (0..layer_bones.len())
        .map(|i| rigid_matrix(init_world_pos[i], init_world_ang[i]))
        .collect();

    // Build per-keyframe resolved local angles & root positions (carrying forward previous keyframe values).
    let mut key_local_angles: Vec<Vec<f64>> = Vec::with_capacity(keyframes.len());
    let mut key_root_pos: Vec<Vec<[f64; 2]>> = Vec::with_capacity(keyframes.len());
    for k_idx in 0..keyframes.len() {
        let ov = pose_overrides.get(k_idx).cloned().unwrap_or_default();
        let mut angs = if k_idx == 0 {
            layer_bones
                .iter()
                .map(|b| b.bind_angle_rad)
                .collect::<Vec<_>>()
        } else {
            key_local_angles[k_idx - 1].clone()
        };
        let mut rpos = if k_idx == 0 {
            init_world_pos.clone()
        } else {
            key_root_pos[k_idx - 1].clone()
        };
        for (b_idx, b) in layer_bones.iter().enumerate() {
            if let Some(a) = lookup_bone_angle(b, &ov) {
                angs[b_idx] = a;
            }
            if b.parent.is_none()
                && let Some(xy) = lookup_bone_pos(b, &ov)
            {
                rpos[b_idx] = xy;
            }
        }
        key_local_angles.push(angs);
        key_root_pos.push(rpos);
    }

    let mut elem_to_bone = vec![None; base_elems.len()];
    for (b_idx, maybe_e) in mapping.iter().enumerate() {
        if let Some(e_idx) = *maybe_e {
            elem_to_bone[e_idx] = Some(b_idx);
        }
    }

    let solve_pose_elems =
        |local_angs: &[f64], root_positions: &[[f64; 2]], frame_off: u32| -> Vec<Element> {
            let mut wpos = vec![[0.0_f64, 0.0]; layer_bones.len()];
            let mut wang = vec![0.0_f64; layer_bones.len()];
            for (b_idx, b) in layer_bones.iter().enumerate() {
                if let Some(p_idx) = b.parent {
                    let (ps, pc) = wang[p_idx].sin_cos();
                    let lo = local_bind_offsets[b_idx];
                    wpos[b_idx] = [
                        wpos[p_idx][0] + pc * lo[0] - ps * lo[1],
                        wpos[p_idx][1] + ps * lo[0] + pc * lo[1],
                    ];
                    wang[b_idx] = wang[p_idx] + local_angs[b_idx];
                } else {
                    wpos[b_idx] = root_positions[b_idx];
                    wang[b_idx] = local_angs[b_idx];
                }
            }
            let mut out_elems = Vec::with_capacity(base_elems.len());
            for (e_idx, base_el) in base_elems.iter().enumerate() {
                let mut el = base_el.clone();
                if let Some(b_idx) = elem_to_bone[e_idx]
                    && let Some(inv_b0) = inv_affine(bone_mats_0[b_idx])
                {
                    let bf = rigid_matrix(wpos[b_idx], wang[b_idx]);
                    let delta = mul_affine(bf, inv_b0);
                    let composed = mul_affine(delta, transform_matrix(base_el.transform));
                    if let Some(mut tr) = Transform::from_matrix(composed) {
                        tr.alpha = base_el.transform.alpha;
                        tr.color_transform = base_el.transform.color_transform;
                        el.transform = tr;
                    }
                }
                if frame_off > 0 {
                    advance_graphic_playback(&mut el, frame_off, symbols);
                }
                out_elems.push(el);
            }
            out_elems
        };

    let start_frame_0 = keyframes[0].frame;
    let total_baked_frames = keyframes
        .last()
        .map_or(0, |last| last.frame.saturating_sub(start_frame_0))
        as usize;
    let elem_bytes: usize = base_elems
        .iter()
        .map(|el| serde_json::to_vec(el).map_or(256, |v| v.len()))
        .sum();
    let cost = total_baked_frames.saturating_mul(elem_bytes.div_ceil(2048).max(1));
    if total_baked_frames > 4095 || cost > *budget {
        return false;
    }

    let mut expanded: Vec<Keyframe> = Vec::new();
    for k_idx in 0..keyframes.len() {
        let mut kf = keyframes[k_idx].clone();
        kf.tween = None;
        kf.tween_kind = neoflash_core::TweenKind::Classic;
        kf.orient_to_path = false;
        let off = kf.frame.saturating_sub(start_frame_0);
        kf.elements = solve_pose_elems(&key_local_angles[k_idx], &key_root_pos[k_idx], off);
        expanded.push(kf);

        if k_idx + 1 < keyframes.len() {
            let f_a = keyframes[k_idx].frame;
            let f_b = keyframes[k_idx + 1].frame;
            let span_dur = f_b.saturating_sub(f_a);
            if span_dur > 1 {
                let ease_opt = keyframes[k_idx].tween;
                for step in 1..span_dur {
                    let raw_u = f64::from(step) / f64::from(span_dur);
                    let u = ease_opt.map_or(raw_u, |e| neoflash_core::easing(raw_u, e));
                    let interp_angs: Vec<f64> = (0..layer_bones.len())
                        .map(|bi| {
                            let a0 = key_local_angles[k_idx][bi];
                            let a1 = key_local_angles[k_idx + 1][bi];
                            let mut da = (a1 - a0) % std::f64::consts::TAU;
                            if da > std::f64::consts::PI {
                                da -= std::f64::consts::TAU;
                            } else if da < -std::f64::consts::PI {
                                da += std::f64::consts::TAU;
                            }
                            a0 + da * u
                        })
                        .collect();
                    let interp_roots: Vec<[f64; 2]> = (0..layer_bones.len())
                        .map(|bi| {
                            let p0 = key_root_pos[k_idx][bi];
                            let p1 = key_root_pos[k_idx + 1][bi];
                            [p0[0] + (p1[0] - p0[0]) * u, p0[1] + (p1[1] - p0[1]) * u]
                        })
                        .collect();
                    let frame_num = f_a + step;
                    expanded.push(Keyframe {
                        frame: frame_num,
                        elements: solve_pose_elems(
                            &interp_angs,
                            &interp_roots,
                            frame_num.saturating_sub(start_frame_0),
                        ),
                        ..Default::default()
                    });
                }
            }
        }
    }

    *budget -= cost;
    *keyframes = expanded;
    true
}
