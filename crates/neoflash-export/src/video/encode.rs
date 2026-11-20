use std::{
    collections::HashMap,
    env, fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::ExportError;
use color_quant::NeuQuant;
use image::{RgbaImage, imageops::FilterType};
use neoflash_core::{
    AuthoredPlayback, DrawItem, ElementKind, Project, SoundCommand, TimelineVideoTracker,
    evaluate_scene, resolve_scene_video_placements,
};

use super::decoder::StagedMediaFile;
use super::progressive::{
    insert_decoded_video_stream, placement_video_cache_key, preflight_and_decode_timeline_videos,
};
use super::*;

/// Export a project (or symbol) to MP4, WebM, MOV, GIF, or PNG sequence.
pub fn export_project_video(
    project: &Project,
    output_path: &Path,
    options: &VideoOptions,
) -> Result<VideoExportReport, ExportError> {
    export_project_video_with_progress(project, output_path, options, |_| true)
}

/// Export a project (or symbol) to MP4, WebM, MOV, GIF, or PNG sequence while reporting frame-by-frame
/// rendering progress (including the rendered RGBA frame for live preview).
pub fn export_project_video_with_progress<F>(
    project: &Project,
    output_path: &Path,
    options: &VideoOptions,
    mut on_progress: F,
) -> Result<VideoExportReport, ExportError>
where
    F: FnMut(VideoProgress) -> bool,
{
    let prepared = prepare_timeline(project, options)?;
    let (width, height) = options.resolved_dimensions(project);
    let fps = options.resolved_fps(project);
    let transparent_bg = options.transparent_bg && options.format.supports_transparent_bg();
    let base_dir = options
        .base_dir
        .as_deref()
        .or_else(|| output_path.parent().filter(|p| !p.as_os_str().is_empty()));

    let mut png_seq_state = if options.format == VideoFormat::PngSequence {
        let (parent, stem, planned_png_names) =
            png_sequence_plan(output_path, prepared.frame_count)?;
        let tx = crate::SequenceSetTransaction::begin_for_family(parent, &stem, "png")?;
        tx.preflight(&planned_png_names)?;
        Some((tx, stem, planned_png_names, 0u64))
    } else {
        None
    };

    // Preflight and decode all reachable timeline video streams before creating output files (`T-201`).
    let mut video_cache =
        preflight_and_decode_timeline_videos(&prepared.eval_project, &prepared.root_id, base_dir)?;

    let dt_ms = ((1000.0 / f64::from(fps)).round() as u32).max(1);
    let mut playback = AuthoredPlayback::with_live_scripts(
        &prepared.eval_project,
        &prepared.root_id,
        0,
        options.runtime_capture,
    );
    let mut video_tracker = TimelineVideoTracker::new();
    if prepared.start_frame > 0 {
        let _ = playback.drain_sound_commands();
        video_tracker.step_with_commands(playback.drain_video_commands(), 0);
        for step in 0..prepared.start_frame {
            playback.tick(&prepared.eval_project);
            video_tracker.step_with_commands(playback.drain_video_commands(), dt_ms);
            if step + 1 < prepared.start_frame {
                let _ = playback.drain_sound_commands();
            }
        }
    }

    let mut bitmap_cache = HashMap::new();
    let mut rendered_frames: Vec<Vec<u8>> = Vec::with_capacity(prepared.frame_count as usize);
    let mut sound_events: Vec<(u32, Vec<SoundCommand>)> = Vec::new();
    let mut video_audio = crate::video_audio::VideoSoundChannels::default();
    let mut warnings: Vec<String> = playback.drain_diagnostics();

    for idx in 0..prepared.frame_count {
        if idx == 0 {
            if prepared.start_frame == 0 {
                video_tracker.step_with_commands(playback.drain_video_commands(), 0);
            }
        } else if idx % prepared.authored_span == 0 && prepared.start_frame > 0 {
            // Re-seek to start_frame when looping a sub-range under StopMode::AfterSeconds.
            playback = AuthoredPlayback::with_live_scripts(
                &prepared.eval_project,
                &prepared.root_id,
                0,
                options.runtime_capture,
            );
            video_tracker.clear();
            let _ = playback.drain_sound_commands();
            video_tracker.step_with_commands(playback.drain_video_commands(), 0);
            for step in 0..prepared.start_frame {
                playback.tick(&prepared.eval_project);
                video_tracker.step_with_commands(playback.drain_video_commands(), dt_ms);
                if step + 1 < prepared.start_frame {
                    let _ = playback.drain_sound_commands();
                }
            }
        } else {
            playback.tick(&prepared.eval_project);
            video_tracker.step_with_commands(playback.drain_video_commands(), dt_ms);
        }
        warnings.extend(playback.drain_diagnostics());
        let mut cmds = playback.drain_sound_commands();
        if options.include_audio {
            cmds.extend(video_audio.sync(video_tracker.channels.values(), f64::from(fps)));
        }
        if !cmds.is_empty() {
            sound_events.push((idx, cmds));
        }

        let (scene, placements) = if options.runtime_capture {
            let scene = playback.scene(&prepared.eval_project, false, true);
            let mut map = HashMap::new();
            for draw in &scene.draws {
                if matches!(draw.item.kind, ElementKind::Video { .. })
                    && let Some(p) = video_tracker.placement_for_target(&draw.item.instance_path)
                {
                    map.insert(draw.item.instance_path.clone(), p.clone());
                }
            }
            (scene, map)
        } else {
            let f = prepared.start_frame + (idx % prepared.authored_span.max(1));
            let scene = evaluate_scene(&prepared.eval_project, &prepared.root_id, f, false, true);
            let items: Vec<DrawItem> = scene.draws.iter().map(|d| d.item.clone()).collect();
            let map = resolve_scene_video_placements(
                &prepared.eval_project,
                &prepared.root_id,
                f,
                &HashMap::new(),
                &items,
            );
            (scene, map)
        };

        for placement in placements.values() {
            let cache_key = placement_video_cache_key(placement);
            if !video_cache.contains_key(&cache_key) {
                let stream = resolve_and_decode_project_video(
                    &prepared.eval_project,
                    &placement.resource,
                    placement.source.as_ref(),
                    base_dir,
                )
                .map_err(ExportError::Options)?;
                insert_decoded_video_stream(&mut video_cache, cache_key, stream)
                    .map_err(ExportError::Options)?;
            }
        }

        let rgba = render_authored_scene_with_video_rgba(
            &prepared.eval_project,
            &scene,
            width,
            height,
            transparent_bg,
            &mut bitmap_cache,
            &mut video_cache,
            &placements,
        );
        if let Some((tx, _, planned_png_names, total_png_bytes)) = &mut png_seq_state {
            let png_bytes = encode_rgba_png_frame(&rgba, width, height)?;
            tx.stage_file(&planned_png_names[idx as usize], &png_bytes)?;
            *total_png_bytes = total_png_bytes.saturating_add(png_bytes.len() as u64);
        }

        let keep_going = on_progress(VideoProgress {
            frame_index: idx + 1,
            frame_count: prepared.frame_count,
            width,
            height,
            rgba: rgba.clone(),
        });
        if !keep_going {
            return Err(ExportError::Options("Video export cancelled".into()));
        }
        if png_seq_state.is_none() {
            rendered_frames.push(rgba);
        }
    }

    if png_seq_state.is_none()
        && let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    if let Some(warning) = video_audio.external_audio_warning() {
        warnings.push(warning.to_owned());
    }
    let wav_audio =
        if options.include_audio && options.format.supports_audio() && !sound_events.is_empty() {
            let (frames, used) = crate::audio::mix_sound_commands(
                &prepared.eval_project,
                std::mem::take(&mut sound_events),
                44_100,
                f64::from(fps),
                0..=prepared.frame_count.saturating_sub(1),
                false,
                &mut HashMap::new(),
            )?;
            (!used.is_empty()).then(|| {
                crate::audio::encode_wav(
                    &frames,
                    44_100,
                    crate::AudioChannels::Stereo,
                    crate::AudioBitDepth::Bit16,
                )
            })
        } else {
            None
        };

    match options.format {
        VideoFormat::Gif => {
            if options.include_audio && !sound_events.is_empty() {
                warnings.push("GIF does not support audio; sound tracks were omitted.".into());
            }
            encode_gif_file(
                output_path,
                &rendered_frames,
                width,
                height,
                fps,
                options.quality,
                transparent_bg,
            )?;
            let bytes = fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);
            Ok(VideoExportReport {
                output_path: output_path.to_path_buf(),
                additional_files: Vec::new(),
                bytes,
                frame_count: prepared.frame_count,
                width,
                height,
                audio_included: false,
                warnings,
            })
        }
        VideoFormat::PngSequence => {
            if let Some((tx, stem, _, total_png_bytes)) = png_seq_state.take() {
                finalize_staged_png_sequence(
                    tx,
                    &stem,
                    total_png_bytes,
                    prepared.frame_count,
                    width,
                    height,
                    wav_audio.as_deref(),
                    warnings,
                )
            } else {
                encode_png_sequence(
                    output_path,
                    &rendered_frames,
                    width,
                    height,
                    wav_audio.as_deref(),
                    warnings,
                )
            }
        }
        VideoFormat::Mp4 | VideoFormat::WebM | VideoFormat::Mov => encode_with_ffmpeg(
            output_path,
            options,
            &rendered_frames,
            width,
            height,
            fps,
            transparent_bg,
            wav_audio.as_deref(),
            warnings,
        ),
    }
}

fn encode_gif_file(
    output_path: &Path,
    frames: &[Vec<u8>],
    width: u32,
    height: u32,
    fps: f32,
    quality: u8,
    transparent_bg: bool,
) -> Result<(), ExportError> {
    let w = u16::try_from(width)
        .map_err(|_| ExportError::Options("GIF width exceeds 65535 pixels".into()))?;
    let h = u16::try_from(height)
        .map_err(|_| ExportError::Options("GIF height exceeds 65535 pixels".into()))?;
    let mut gif_bytes = Vec::new();
    {
        let mut encoder = gif::Encoder::new(BufWriter::new(&mut gif_bytes), w, h, &[])
            .map_err(|e| ExportError::Options(format!("GIF encoder error: {e}")))?;
        encoder
            .set_repeat(gif::Repeat::Infinite)
            .map_err(|e| ExportError::Options(format!("GIF repeat error: {e}")))?;

        let delay_cs = ((100.0 / f64::from(fps)).round() as u16).max(1);
        let sample_fac = (30 - i32::from(quality.clamp(1, 100)) * 29 / 100).clamp(1, 30);

        for rgba in frames {
            // If GIF dimensions exceed 2048, scale down or quantize directly.
            let nq = NeuQuant::new(sample_fac, 255, rgba);
            let mut palette = nq.color_map_rgb();
            // Reserve index 255 for transparent pixels when transparent_bg is enabled.
            while palette.len() < 256 * 3 {
                palette.push(0);
            }
            let mut indexed = Vec::with_capacity((width * height) as usize);
            for px in rgba.as_chunks::<4>().0 {
                if transparent_bg && px[3] < 128 {
                    indexed.push(255u8);
                } else {
                    let idx = (nq.index_of(px) as u8).min(254);
                    indexed.push(idx);
                }
            }
            let mut frame = gif::Frame::from_palette_pixels(
                w,
                h,
                indexed,
                palette,
                transparent_bg.then_some(255),
            );
            frame.delay = delay_cs;
            frame.dispose = if transparent_bg {
                gif::DisposalMethod::Background
            } else {
                gif::DisposalMethod::Any
            };
            encoder
                .write_frame(&frame)
                .map_err(|e| ExportError::Options(format!("GIF frame write error: {e}")))?;
        }
    }
    crate::atomic_write(output_path, &gif_bytes)?;
    Ok(())
}

fn png_sequence_plan(
    output_path: &Path,
    frame_count: u32,
) -> Result<(&Path, String, Vec<String>), ExportError> {
    let parent = output_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let raw_stem = output_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("frame");
    let stem = raw_stem
        .strip_suffix("_0001")
        .filter(|s| !s.is_empty())
        .unwrap_or(raw_stem);

    let count = frame_count.max(1);
    let mut names = Vec::with_capacity(count as usize);
    for idx in 0..count {
        if count == 1 && raw_stem == stem && output_path.extension().is_some() {
            let name = output_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| {
                    ExportError::Options(format!(
                        "Invalid PNG sequence output path '{}'.",
                        output_path.display()
                    ))
                })?;
            names.push(name.to_owned());
        } else {
            names.push(format!("{stem}_{:04}.png", idx + 1));
        }
    }
    Ok((parent, stem.to_owned(), names))
}

pub(super) fn encode_rgba_png_frame(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, ExportError> {
    let img = RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| ExportError::Options("PNG sequence buffer size mismatch".into()))?;
    let mut png_bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )
    .map_err(|e| ExportError::Options(format!("Could not encode PNG frame: {e}")))?;
    Ok(png_bytes)
}

#[allow(clippy::too_many_arguments)]
fn finalize_staged_png_sequence(
    mut tx: crate::SequenceSetTransaction,
    stem: &str,
    mut total_bytes: u64,
    frame_count: u32,
    width: u32,
    height: u32,
    wav_audio: Option<&[u8]>,
    warnings: Vec<String>,
) -> Result<VideoExportReport, ExportError> {
    let mut audio_included = false;
    if let Some(wav) = wav_audio {
        let wav_name = format!("{stem}.wav");
        tx.stage_file(&wav_name, wav)?;
        total_bytes = total_bytes.saturating_add(wav.len() as u64);
        audio_included = true;
    }

    let committed = tx.commit()?;
    let mut iter = committed.into_iter();
    let first_path = iter.next().ok_or_else(|| {
        ExportError::Options("PNG sequence transaction committed zero files.".into())
    })?;
    let additional_files: Vec<PathBuf> = iter.collect();

    Ok(VideoExportReport {
        output_path: first_path,
        additional_files,
        bytes: total_bytes,
        frame_count,
        width,
        height,
        audio_included,
        warnings,
    })
}

fn encode_png_sequence(
    output_path: &Path,
    frames: &[Vec<u8>],
    width: u32,
    height: u32,
    wav_audio: Option<&[u8]>,
    warnings: Vec<String>,
) -> Result<VideoExportReport, ExportError> {
    let frame_count = frames.len() as u32;
    let (parent, stem, planned_png_names) = png_sequence_plan(output_path, frame_count)?;
    let mut tx = crate::SequenceSetTransaction::begin_for_family(parent, &stem, "png")?;
    let mut preflight_names = planned_png_names.clone();
    if wav_audio.is_some() {
        preflight_names.push(format!("{stem}.wav"));
    }
    tx.preflight(&preflight_names)?;

    let mut total_bytes = 0u64;
    for (idx, rgba) in frames.iter().enumerate() {
        let png_bytes = encode_rgba_png_frame(rgba, width, height)?;
        tx.stage_file(&planned_png_names[idx], &png_bytes)?;
        total_bytes = total_bytes.saturating_add(png_bytes.len() as u64);
    }

    finalize_staged_png_sequence(
        tx,
        &stem,
        total_bytes,
        frame_count,
        width,
        height,
        wav_audio,
        warnings,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_ffmpeg_encode_attempt(
    ffmpeg: &Path,
    output_path: &Path,
    options: &VideoOptions,
    frames: &[Vec<u8>],
    width: u32,
    height: u32,
    fps: f32,
    transparent_bg: bool,
    staged_audio_path: Option<&Path>,
    video_encoder: &str,
) -> Result<(), ExportError> {
    let mut cmd = Command::new(ffmpeg);
    configure_media_subprocess_command(&mut cmd, ffmpeg);
    cmd.arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-f")
        .arg("rawvideo")
        .arg("-pixel_format")
        .arg("rgba")
        .arg("-video_size")
        .arg(format!("{width}x{height}"))
        .arg("-framerate")
        .arg(format!("{fps:.3}"))
        .arg("-i")
        .arg("pipe:0");

    if let Some(wav_path) = staged_audio_path {
        cmd.arg("-i").arg(wav_path);
    }

    let codec = options.resolved_codec();
    let q = i32::from(options.quality.clamp(1, 100));
    let crf_h264 = (51 - (q * 35) / 100).clamp(14, 45);
    let crf_vp9 = (63 - (q * 48) / 100).clamp(15, 55);
    let target_kbps = ((options.target_bitrate_mbps(width, height, fps) * 1000.0).round() as u32)
        .clamp(200, 80_000);

    match (options.format, codec) {
        (VideoFormat::Mp4, VideoCodec::H265) | (VideoFormat::Mov, VideoCodec::H265) => {
            cmd.arg("-c:v").arg(video_encoder);
            if video_encoder == "libx265" {
                cmd.arg("-crf").arg(crf_h264.to_string());
            } else {
                cmd.arg("-b:v").arg(format!("{target_kbps}k"));
            }
            cmd.arg("-pix_fmt").arg("yuv420p").arg("-tag:v").arg("hvc1");
        }
        (VideoFormat::Mp4, _) | (VideoFormat::Mov, VideoCodec::H264) => {
            cmd.arg("-c:v").arg(video_encoder);
            if video_encoder == "libx264" {
                cmd.arg("-preset")
                    .arg("medium")
                    .arg("-crf")
                    .arg(crf_h264.to_string());
            } else {
                cmd.arg("-b:v").arg(format!("{target_kbps}k"));
            }
            cmd.arg("-pix_fmt").arg("yuv420p");
            if options.format == VideoFormat::Mp4 {
                cmd.arg("-movflags").arg("+faststart");
            }
        }
        (VideoFormat::Mov, VideoCodec::ProRes) => {
            cmd.arg("-c:v")
                .arg(video_encoder)
                .arg("-profile:v")
                .arg(if transparent_bg { "4" } else { "3" })
                .arg("-pix_fmt")
                .arg(if transparent_bg {
                    "yuva444p10le"
                } else {
                    "yuv422p10le"
                });
        }
        (VideoFormat::WebM, VideoCodec::Av1) => {
            cmd.arg("-c:v")
                .arg(video_encoder)
                .arg("-crf")
                .arg(crf_vp9.to_string())
                .arg("-pix_fmt")
                .arg("yuv420p");
        }
        (VideoFormat::WebM, _) => {
            cmd.arg("-c:v")
                .arg(video_encoder)
                .arg("-b:v")
                .arg("0")
                .arg("-crf")
                .arg(crf_vp9.to_string())
                .arg("-pix_fmt")
                .arg(if transparent_bg {
                    "yuva420p"
                } else {
                    "yuv420p"
                });
        }
        _ => {}
    }

    if staged_audio_path.is_some() {
        match (options.format, options.audio_bitrate) {
            (VideoFormat::WebM, _) | (_, VideoAudioBitrate::Opus160) => {
                cmd.arg("-c:a").arg("libopus").arg("-b:a").arg("160k");
            }
            (VideoFormat::Mov, VideoAudioBitrate::Pcm16) => {
                cmd.arg("-c:a").arg("pcm_s16le");
            }
            (_, bitrate) => {
                let kbps = bitrate.kbps().clamp(96, 320);
                cmd.arg("-c:a")
                    .arg("aac")
                    .arg("-b:a")
                    .arg(format!("{kbps}k"));
            }
        }
        cmd.arg("-shortest");
    } else {
        cmd.arg("-an");
    }

    cmd.arg(output_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| ExportError::Options(format!("Failed to launch ffmpeg: {e}")))?;

    let mut write_err = None;
    if let Some(mut stdin) = child.stdin.take() {
        for frame in frames {
            if let Err(e) = stdin.write_all(frame) {
                write_err = Some(e);
                break;
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|e| ExportError::Options(format!("Failed waiting for ffmpeg: {e}")))?;

    if !output.status.success() {
        let _ = fs::remove_file(output_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ExportError::Options(format!(
            "ffmpeg encoding failed ({video_encoder}): {}",
            stderr.trim()
        )));
    }
    if let Some(e) = write_err {
        let _ = fs::remove_file(output_path);
        return Err(ExportError::Io(e));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_with_ffmpeg(
    output_path: &Path,
    options: &VideoOptions,
    frames: &[Vec<u8>],
    width: u32,
    height: u32,
    fps: f32,
    transparent_bg: bool,
    wav_audio: Option<&[u8]>,
    mut warnings: Vec<String>,
) -> Result<VideoExportReport, ExportError> {
    let ffmpeg = detect_ffmpeg().ok_or_else(|| {
        ExportError::Options(
            "FFmpeg was not found. Install ffmpeg (e.g. `brew install ffmpeg`), set NEOFLASH_FFMPEG, or export as GIF / PNG seq."
                .into(),
        )
    })?;

    let staged_audio = wav_audio
        .map(|wav| StagedMediaFile::create("timeline audio", "wav", wav))
        .transpose()
        .map_err(ExportError::Options)?;
    let staged_audio_path = staged_audio.as_ref().map(|s| s.path());

    let codec = options.resolved_codec();
    let prefer_hardware = match env::var("NEOFLASH_VIDEO_ENCODER_MODE")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("hardware") => true,
        Some("software") => false,
        _ => false,
    };
    let (primary_encoder, is_hw) = select_video_encoder(&ffmpeg, codec, prefer_hardware);

    let parent = output_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let stem = output_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    static FFMPEG_STAGE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let seq = FFMPEG_STAGE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let staged_output = parent.join(format!(
        ".{stem}.neoflash-video-tmp-{}-{seq}.{}",
        std::process::id(),
        options.format.extension()
    ));

    if let Err(primary_err) = run_ffmpeg_encode_attempt(
        &ffmpeg,
        &staged_output,
        options,
        frames,
        width,
        height,
        fps,
        transparent_bg,
        staged_audio_path,
        primary_encoder,
    ) {
        let _ = fs::remove_file(&staged_output);
        if is_hw {
            let (sw_encoder, _) = select_video_encoder(&ffmpeg, codec, false);
            if sw_encoder != primary_encoder && probe_ffmpeg_encoders(&ffmpeg).contains(sw_encoder)
            {
                warnings.push(format!(
                    "Hardware video encoder '{primary_encoder}' was unavailable for this configuration; fell back to software encoder '{sw_encoder}'."
                ));
                if let Err(sw_err) = run_ffmpeg_encode_attempt(
                    &ffmpeg,
                    &staged_output,
                    options,
                    frames,
                    width,
                    height,
                    fps,
                    transparent_bg,
                    staged_audio_path,
                    sw_encoder,
                ) {
                    let _ = fs::remove_file(&staged_output);
                    return Err(sw_err);
                }
            } else {
                return Err(primary_err);
            }
        } else {
            return Err(primary_err);
        }
    }

    if let Err(err) =
        crate::commit_staged_files_atomically(&[(staged_output.clone(), output_path.to_path_buf())])
    {
        let _ = fs::remove_file(&staged_output);
        return Err(ExportError::Io(err));
    }

    let bytes = fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);
    // Unused imageops import guard if resizing thumbnails is used by tests/callers.
    let _ = FilterType::Triangle;

    Ok(VideoExportReport {
        output_path: output_path.to_path_buf(),
        additional_files: Vec::new(),
        bytes,
        frame_count: frames.len() as u32,
        width,
        height,
        audio_included: wav_audio.is_some(),
        warnings,
    })
}
