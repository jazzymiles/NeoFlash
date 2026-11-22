//! Stateful lossless ScreenVideo (`codec_id == 3`) and ScreenVideo v2 (`codec_id == 6`)
//! frame decoder and Stage projector for `SwfBytecodePlayer`.

use super::*;
use crate::swf::media::{
    apply_screen_video_packet, collect_swf_video_streams, encode_rgba_png,
    is_screen_video_keyframe, is_screen_video_v2_keyframe,
};
use neoflash_core::ResourceUnit;
use std::ops::Bound::{Excluded, Included};

const MAX_SCANLINE_VIDEO_PIXELS: usize = 16_384;
const MAX_LIVE_VIDEO_RASTER_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(super) struct PlayerVideoStreamDef {
    pub width: u16,
    pub height: u16,
    pub smoothing: bool,
    pub codec_id: u8,
    pub packets: BTreeMap<u16, Vec<u8>>,
    pub keyframes: Vec<u16>,
}

#[derive(Clone, Debug)]
pub(super) struct LiveVideoStreamState {
    pub decoded_frame: Option<u16>,
    pub rgba: Vec<u8>,
}

pub(super) fn collect_player_video_streams(
    tag_stream: &[u8],
) -> BTreeMap<u16, PlayerVideoStreamDef> {
    let raw_streams = collect_swf_video_streams(tag_stream);
    let mut out = BTreeMap::new();
    for (cid, def) in raw_streams {
        if def.budget_exceeded || def.frames.is_empty() || !matches!(def.codec_id, 3 | 6) {
            continue;
        }
        let width = def.raw_width.clamp(1, 4096);
        let height = def.raw_height.clamp(1, 4096);
        let raster_bytes = usize::from(width)
            .saturating_mul(usize::from(height))
            .saturating_mul(4);
        if raster_bytes == 0 || raster_bytes > MAX_LIVE_VIDEO_RASTER_BYTES {
            continue;
        }
        let mut keyframes = Vec::new();
        for (&fnum, pkt) in &def.frames {
            let is_kf = if def.codec_id == 6 {
                is_screen_video_v2_keyframe(pkt, width, height)
            } else {
                is_screen_video_keyframe(pkt, width, height)
            };
            if is_kf == Some(true) {
                keyframes.push(fnum);
            }
        }
        out.insert(
            cid,
            PlayerVideoStreamDef {
                width,
                height,
                smoothing: def.smoothing,
                codec_id: def.codec_id,
                packets: def.frames,
                keyframes,
            },
        );
    }
    out
}

fn collect_active_video_targets(
    display_list: &BTreeMap<u16, LiveSlot>,
    parent_frame: u32,
    defs: &BTreeMap<u16, PlayerVideoStreamDef>,
    active: &mut BTreeMap<u16, u16>,
) {
    for slot in display_list.values() {
        if let Some(def) = defs.get(&slot.character_id) {
            let max_pkt = def.packets.keys().next_back().copied().unwrap_or(0);
            let target = slot
                .ratio
                .unwrap_or_else(|| (parent_frame.min(u32::from(u16::MAX)) as u16).min(max_pkt));
            active.entry(slot.character_id).or_insert(target);
        }
        if !slot.children.is_empty() {
            collect_active_video_targets(&slot.children, slot.current_frame, defs, active);
        }
    }
}

fn decode_stream_to_target(
    def: &PlayerVideoStreamDef,
    state: &mut LiveVideoStreamState,
    target_frame: u16,
) {
    let Some((&effective_frame, _)) = def.packets.range(..=target_frame).next_back() else {
        state.decoded_frame = None;
        state.rgba.clear();
        return;
    };
    let expected_len = usize::from(def.width) * usize::from(def.height) * 4;
    if state.decoded_frame == Some(effective_frame) && state.rgba.len() == expected_len {
        return;
    }

    let nearest_kf = def
        .keyframes
        .iter()
        .copied()
        .rfind(|&kf| kf <= effective_frame);

    if let Some(cur) = state.decoded_frame
        && cur < effective_frame
        && state.rgba.len() == expected_len
        && nearest_kf.is_none_or(|kf| kf <= cur)
    {
        for (_, pkt) in def
            .packets
            .range((Excluded(cur), Included(effective_frame)))
        {
            let _ = apply_screen_video_packet(
                def.codec_id,
                pkt,
                def.width,
                def.height,
                &mut state.rgba,
            );
        }
        state.decoded_frame = Some(effective_frame);
        return;
    }

    state.rgba.resize(expected_len, 0);
    for px in state.rgba.as_chunks_mut::<4>().0 {
        *px = [0, 0, 0, 255];
    }
    let replay_start = nearest_kf.unwrap_or(0);
    let mut any_applied = false;
    for (_, pkt) in def.packets.range(replay_start..=effective_frame) {
        if apply_screen_video_packet(def.codec_id, pkt, def.width, def.height, &mut state.rgba) {
            any_applied = true;
        }
    }
    if any_applied {
        state.decoded_frame = Some(effective_frame);
    } else {
        state.decoded_frame = None;
        state.rgba.clear();
    }
}

fn rgba_to_scanline_rectangles(sym_id: &str, width: u16, height: u16, rgba: &[u8]) -> Vec<Element> {
    let w = usize::from(width);
    let h = usize::from(height);
    if w == 0 || h == 0 || w.saturating_mul(h) > MAX_SCANLINE_VIDEO_PIXELS || rgba.len() < w * h * 4
    {
        return Vec::new();
    }
    let mut elements = Vec::new();
    let mut seq = 0usize;
    for y in 0..h {
        let mut x = 0usize;
        while x < w {
            let base = (y * w + x) * 4;
            let color = [rgba[base], rgba[base + 1], rgba[base + 2], rgba[base + 3]];
            let mut end_x = x + 1;
            while end_x < w {
                let nb = (y * w + end_x) * 4;
                if [rgba[nb], rgba[nb + 1], rgba[nb + 2], rgba[nb + 3]] != color {
                    break;
                }
                end_x += 1;
            }
            seq += 1;
            let tf = Transform {
                x: x as f64,
                y: y as f64,
                ..Transform::default()
            };
            elements.push(Element {
                id: format!("{sym_id}-svpx-{seq}"),
                name: String::new(),
                transform: tf,
                kind: ElementKind::Rectangle {
                    width: (end_x - x) as f64,
                    height: 1.0,
                    fill: color,
                    stroke: [0, 0, 0, 0],
                    stroke_width: 0.0,
                },
                ..Element::default()
            });
            x = end_x;
        }
    }
    elements
}

impl SwfBytecodePlayer {
    /// Return the decoded straight-RGBA pixels `(width, height, rgba)` of a currently displayed
    /// embedded `ScreenVideo` (`codec_id == 3` or `6`) character on Stage.
    pub fn video_frame_rgba(&self, character_id: u16) -> Option<(u32, u32, &[u8])> {
        let def = self.video_streams.get(&character_id)?;
        let state = self.live_videos.get(&character_id)?;
        state.decoded_frame?;
        let expected = usize::from(def.width) * usize::from(def.height) * 4;
        if state.rgba.len() != expected {
            return None;
        }
        Some((u32::from(def.width), u32::from(def.height), &state.rgba))
    }

    pub(super) fn sync_live_video_streams(&mut self, root_displayed_frame: u32) {
        if self.video_streams.is_empty() {
            return;
        }
        let mut active_targets = BTreeMap::new();
        collect_active_video_targets(
            &self.display_list,
            root_displayed_frame,
            &self.video_streams,
            &mut active_targets,
        );
        self.live_videos
            .retain(|cid, _| active_targets.contains_key(cid));
        for (cid, target_frame) in active_targets {
            if let Some(def) = self.video_streams.get(&cid) {
                let state = self
                    .live_videos
                    .entry(cid)
                    .or_insert_with(|| LiveVideoStreamState {
                        decoded_frame: None,
                        rgba: Vec::new(),
                    });
                decode_stream_to_target(def, state, target_frame);
            }
        }
    }

    pub(super) fn build_video_scanline_symbol_overrides(
        &self,
        project: &Project,
        overrides: &mut HashMap<String, Symbol>,
    ) {
        for (&cid, state) in &self.live_videos {
            if state.decoded_frame.is_none() || state.rgba.is_empty() {
                continue;
            }
            let Some(def) = self.video_streams.get(&cid) else {
                continue;
            };
            let sym_id = format!("swf-sym-{cid}");
            if project.symbol(&sym_id).is_none() && !overrides.contains_key(&sym_id) {
                continue;
            }
            let rects = rgba_to_scanline_rectangles(&sym_id, def.width, def.height, &state.rgba);
            if rects.is_empty() {
                continue;
            }
            overrides.insert(
                sym_id.clone(),
                Symbol {
                    id: sym_id.clone(),
                    name: format!("Video {cid}"),
                    folder: "Media".into(),
                    kind: SymbolKind::MovieClip,
                    class_name: None,
                    linkage_id: None,
                    timeline: Timeline {
                        frame_count: 1,
                        layers: vec![Layer {
                            id: format!("{sym_id}-rt-video"),
                            name: "Video".into(),
                            visible: true,
                            locked: false,
                            keyframes: vec![Keyframe {
                                frame: 0,
                                elements: rects,
                                ..Default::default()
                            }],
                            properties: LayerProperties::default(),
                        }],
                        scripts: Vec::new(),
                        guides: Vec::new(),
                    },
                },
            );
        }
    }

    pub(super) fn inject_video_png_resources(&self, overlay: &mut Project) {
        for (&cid, state) in &self.live_videos {
            if state.decoded_frame.is_none() || state.rgba.is_empty() {
                continue;
            }
            let Some(def) = self.video_streams.get(&cid) else {
                continue;
            };
            let w = u32::from(def.width);
            let h = u32::from(def.height);
            let Some(png) = encode_rgba_png(&state.rgba, w, h) else {
                continue;
            };
            let res_id = format!("swf-rt-video-bmp-{cid}");
            let Ok(unit) = ResourceUnit::from_bytes(
                res_id.clone(),
                format!("videos/rt-video-{cid}.png"),
                "image/png",
                &png,
            ) else {
                continue;
            };
            overlay.resources.retain(|r| r.id != res_id);
            overlay.resources.push(unit);
            let sym_id = format!("swf-sym-{cid}");
            let bmp_el = Element {
                id: format!("{sym_id}-rt-bmp"),
                name: String::new(),
                transform: Transform::default(),
                kind: ElementKind::Bitmap {
                    resource_id: res_id,
                    width: w,
                    height: h,
                    smoothing: def.smoothing,
                },
                ..Element::default()
            };
            let video_tl = Timeline {
                frame_count: 1,
                layers: vec![Layer {
                    id: format!("{sym_id}-rt-video"),
                    name: "Video".into(),
                    visible: true,
                    locked: false,
                    keyframes: vec![Keyframe {
                        frame: 0,
                        elements: vec![bmp_el],
                        ..Default::default()
                    }],
                    properties: LayerProperties::default(),
                }],
                scripts: Vec::new(),
                guides: Vec::new(),
            };
            if let Some(sym) = overlay.symbol_mut(&sym_id) {
                sym.timeline = video_tl;
            } else {
                overlay.symbols.push(Symbol {
                    id: sym_id,
                    name: format!("Video {cid}"),
                    folder: "Media".into(),
                    kind: SymbolKind::MovieClip,
                    class_name: None,
                    linkage_id: None,
                    timeline: video_tl,
                });
            }
        }
    }
}
