use crate::{SequencePublishPolicy, video};
use neoflash_core::{ElementKind, Project};
use std::{collections::BTreeSet, fs};

pub(crate) fn apply_sequence_publish_policy(
    project: &mut Project,
    policy: SequencePublishPolicy,
) -> (bool, Vec<String>) {
    match policy {
        SequencePublishPolicy::KeepFrames => (false, Vec::new()),
        SequencePublishPolicy::PackAtlas => (true, Vec::new()),
        SequencePublishPolicy::TranscodeVideo => {
            let mut warnings = Vec::new();
            let Some(ffmpeg_bin) = video::detect_ffmpeg() else {
                warnings.push(
                    "Image sequence policy 'Transcode video' requested, but ffmpeg was not found; falling back to 'Pack atlas'."
                        .to_owned(),
                );
                return (true, warnings);
            };
            struct SeqTempGuard(std::path::PathBuf);
            impl Drop for SeqTempGuard {
                fn drop(&mut self) {
                    let _ = fs::remove_dir_all(&self.0);
                }
            }

            let fps = project.fps.max(1);
            let mut transcoded_any = false;
            let mut fallback_pack_atlas = false;
            let mut candidate_orphan_res_ids = BTreeSet::<String>::new();
            let mut next_seq_idx = 0usize;
            for sym_idx in 0..project.symbols.len() {
                for layer_idx in 0..project.symbols[sym_idx].timeline.layers.len() {
                    let is_seq_layer = {
                        let layer = &project.symbols[sym_idx].timeline.layers[layer_idx];
                        layer.keyframes.len() >= 3
                            && layer.keyframes.iter().enumerate().all(|(idx, kf)| {
                                kf.frame == idx as u32
                                    && kf.video.is_none()
                                    && kf.sound.is_none()
                                    && kf.tween.is_none()
                                    && kf.elements.len() == 1
                                    && matches!(kf.elements[0].kind, ElementKind::Bitmap { .. })
                            })
                    };
                    if !is_seq_layer {
                        continue;
                    }
                    let frame_ids: Vec<(String, u32, u32, [f64; 2])> =
                        project.symbols[sym_idx].timeline.layers[layer_idx]
                            .keyframes
                            .iter()
                            .filter_map(|kf| {
                                let el = kf.elements.first()?;
                                if let ElementKind::Bitmap {
                                    resource_id,
                                    width,
                                    height,
                                    ..
                                } = &el.kind
                                {
                                    Some((
                                        resource_id.clone(),
                                        *width,
                                        *height,
                                        [el.transform.x, el.transform.y],
                                    ))
                                } else {
                                    None
                                }
                            })
                            .collect();
                    if frame_ids.len() < 3 {
                        continue;
                    }
                    let (_, first_w, first_h, first_pos) = frame_ids[0];
                    if frame_ids
                        .iter()
                        .any(|(_, w, h, _)| *w != first_w || *h != first_h)
                    {
                        fallback_pack_atlas = true;
                        continue;
                    }
                    let nonce = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0);
                    let temp_dir = std::env::temp_dir().join(format!(
                        "neoflash-seq-video-{}-{nonce}-{next_seq_idx}",
                        std::process::id()
                    ));
                    next_seq_idx += 1;
                    if fs::create_dir_all(&temp_dir).is_err() {
                        fallback_pack_atlas = true;
                        continue;
                    }
                    let _temp_guard = SeqTempGuard(temp_dir.clone());
                    let mut wrote_all = true;
                    for (idx, (res_id, _, _, _)) in frame_ids.iter().enumerate() {
                        let Some(res) = project.resources.iter().find(|r| r.id == *res_id) else {
                            wrote_all = false;
                            break;
                        };
                        let Ok(img) = res.decode_image() else {
                            wrote_all = false;
                            break;
                        };
                        let Some(rgba) =
                            image::RgbaImage::from_raw(img.width, img.height, img.rgba.to_vec())
                        else {
                            wrote_all = false;
                            break;
                        };
                        if rgba
                            .save(temp_dir.join(format!("frame_{idx:05}.png")))
                            .is_err()
                        {
                            wrote_all = false;
                            break;
                        }
                    }
                    let out_mp4 = temp_dir.join("seq.mp4");
                    let mp4_bytes = if wrote_all {
                        std::process::Command::new(&ffmpeg_bin)
                            .args([
                                "-y",
                                "-hide_banner",
                                "-loglevel",
                                "error",
                                "-framerate",
                                &fps.to_string(),
                                "-i",
                            ])
                            .arg(temp_dir.join("frame_%05d.png"))
                            .args([
                                "-vf",
                                "scale=trunc(iw/2)*2:trunc(ih/2)*2",
                                "-c:v",
                                "libx264",
                                "-pix_fmt",
                                "yuv420p",
                                "-movflags",
                                "+faststart",
                            ])
                            .arg(&out_mp4)
                            .status()
                            .ok()
                            .filter(|s| s.success())
                            .and_then(|_| fs::read(&out_mp4).ok())
                    } else {
                        None
                    };
                    let Some(mp4_bytes) = mp4_bytes else {
                        warnings.push(format!(
                            "Sequence layer '{}' could not be transcoded to MP4; falling back to 'Pack atlas'.",
                            project.symbols[sym_idx].timeline.layers[layer_idx].name
                        ));
                        fallback_pack_atlas = true;
                        continue;
                    };
                    if mp4_bytes.is_empty() || mp4_bytes.len() > neoflash_core::MAX_RESOURCE_BYTES {
                        fallback_pack_atlas = true;
                        continue;
                    }
                    let mut vid_res_id = format!("__seq_video_{next_seq_idx}");
                    let mut vid_res_path = format!("video/seq_{next_seq_idx}.mp4");
                    while project
                        .resources
                        .iter()
                        .any(|r| r.id == vid_res_id || r.path == vid_res_path)
                    {
                        next_seq_idx += 1;
                        vid_res_id = format!("__seq_video_{next_seq_idx}");
                        vid_res_path = format!("video/seq_{next_seq_idx}.mp4");
                    }
                    let Ok(vid_unit) = neoflash_core::ResourceUnit::from_bytes(
                        &vid_res_id,
                        &vid_res_path,
                        "video/mp4",
                        &mp4_bytes,
                    ) else {
                        fallback_pack_atlas = true;
                        continue;
                    };
                    for (res_id, _, _, _) in &frame_ids {
                        candidate_orphan_res_ids.insert(res_id.clone());
                    }
                    project.resources.push(vid_unit);
                    project.format_version = project.format_version.max(11);
                    let layer = &mut project.symbols[sym_idx].timeline.layers[layer_idx];
                    let mut kf0 = layer.keyframes[0].clone();
                    kf0.frame = 0;
                    kf0.tween = None;
                    kf0.keyed_properties = None;
                    kf0.element_parents.clear();
                    kf0.controller_elements.clear();
                    kf0.appearances.clear();
                    let mut vid_el = kf0.elements[0].clone();
                    vid_el.transform.x = first_pos[0];
                    vid_el.transform.y = first_pos[1];
                    vid_el.kind = ElementKind::Video {
                        width: f64::from(first_w),
                        height: f64::from(first_h),
                    };
                    kf0.elements = vec![vid_el];
                    kf0.video = Some(neoflash_core::TimelineVideoSpan::embedded(&vid_res_id));
                    layer.keyframes = vec![kf0];
                    transcoded_any = true;
                }
            }
            if !candidate_orphan_res_ids.is_empty() {
                let mut still_referenced = BTreeSet::<String>::new();
                for symbol in &project.symbols {
                    for layer in &symbol.timeline.layers {
                        for kf in &layer.keyframes {
                            for el in &kf.elements {
                                if let ElementKind::Bitmap { resource_id, .. } = &el.kind {
                                    still_referenced.insert(resource_id.clone());
                                }
                            }
                            for bf in &kf.bitmap_fills {
                                still_referenced.insert(bf.resource_id.clone());
                            }
                            if let Some(snd) = &kf.sound {
                                still_referenced.insert(snd.resource.clone());
                            }
                            if let Some(vid) = &kf.video {
                                still_referenced.insert(vid.resource.clone());
                            }
                        }
                    }
                }
                project.resources.retain(|r| {
                    !candidate_orphan_res_ids.contains(&r.id) || still_referenced.contains(&r.id)
                });
            }
            if !transcoded_any || fallback_pack_atlas {
                return (true, warnings);
            }
            (false, warnings)
        }
    }
}
