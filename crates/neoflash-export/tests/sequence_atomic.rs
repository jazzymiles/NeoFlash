use neoflash_core::{
    Element, ElementKind, FrameSound, Keyframe, Layer, LayerProperties, Project, ResourceUnit,
    SymbolKind, Timeline,
};
use neoflash_export::{
    AtlasFormat, AtlasImageFormat, AtlasOptions, ExportError, ImageExportFormat,
    ImageExportOptions, ImageExportRange, SEQUENCE_COMMIT_LOCK_FILENAME,
    SEQUENCE_MANIFEST_FILENAME, SEQUENCE_STAGE_PREFIX, SequenceSetTransaction, VideoFormat,
    VideoOptions, export_project_atlas, export_project_image, export_project_image_with_progress,
    export_project_video, export_project_video_with_progress, read_sequence_manifest, sha256_hex,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

static TEST_DIR_SEQ: AtomicU64 = AtomicU64::new(1);

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(label: &str) -> Self {
        let id = TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "neoflash-seq-atomic-{label}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp test dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn build_animated_project(frame_count: u32, seed_offset: u8) -> Project {
    let mut project = Project::blank();
    project.name = "AtomicSeqProject".into();
    project.width = 48;
    project.height = 36;
    project.fps = 12;

    let template_kf = project.symbols[0].timeline.layers[0].keyframes[0].clone();
    let mut keyframes = Vec::with_capacity(frame_count as usize);
    for idx in 0..frame_count {
        let r = seed_offset.wrapping_add(((idx * 37) % 251) as u8);
        let g = seed_offset.wrapping_add(((idx * 73 + 40) % 251) as u8);
        let b = seed_offset.wrapping_add(((idx * 113 + 90) % 251) as u8);
        let mut kf: Keyframe = template_kf.clone();
        kf.frame = idx;
        kf.elements = vec![Element {
            id: format!("rect_{idx}"),
            transform: neoflash_core::Transform {
                x: (idx * 3) as f64,
                y: 2.0,
                ..neoflash_core::Transform::default()
            },
            kind: ElementKind::Rectangle {
                width: 20.0,
                height: 20.0,
                fill: [r, g, b, 255],
                stroke: [15, 25, 35, 255],
                stroke_width: 1.0,
            },
            ..Element::default()
        }];
        keyframes.push(kf);
    }

    let root_id = project.root.clone();
    let root = project.symbol_mut(&root_id).expect("root symbol");
    root.kind = SymbolKind::MovieClip;
    root.timeline = Timeline {
        frame_count: frame_count.max(1),
        layers: vec![Layer {
            id: "layer_main".into(),
            name: "Main".into(),
            visible: true,
            locked: false,
            properties: LayerProperties::default(),
            keyframes,
        }],
        scripts: Vec::new(),
        guides: Vec::new(),
    };
    project.validate().expect("valid animated project");
    project
}

fn make_sine_pcm16_wav(sample_rate: u32, samples: usize) -> Vec<u8> {
    let data_len = (samples * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..samples {
        let phase = (i as f64) * 440.0 * std::f64::consts::TAU / (sample_rate as f64);
        let sample = (phase.sin() * 12_000.0) as i16;
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

fn snapshot_directory_bytes(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    for entry in fs::read_dir(dir).expect("read_dir snapshot") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            !name.starts_with(SEQUENCE_STAGE_PREFIX),
            "staging directory leaked in {}: {name}",
            dir.display()
        );
        if path.is_file() {
            out.insert(name, fs::read(&path).expect("read file for snapshot"));
        }
    }
    out
}

#[test]
fn all_image_sequence_formats_replace_atomically_and_prune_stale_tail_frames() {
    let guard = TempDirGuard::new("all-formats");
    let all_formats = [
        ImageExportFormat::Png,
        ImageExportFormat::Jpeg,
        ImageExportFormat::Webp,
        ImageExportFormat::Gif,
        ImageExportFormat::Bmp,
        ImageExportFormat::Svg,
        ImageExportFormat::Ai,
        ImageExportFormat::Psd,
        ImageExportFormat::Pdf,
    ];

    let proj_long = build_animated_project(6, 10);
    let proj_short = build_animated_project(3, 90);

    for fmt in all_formats {
        let ext = fmt.extension();
        let sub = guard.path().join(ext);
        fs::create_dir_all(&sub).unwrap();

        let mut opts = ImageExportOptions::for_project(&proj_long, 0);
        opts.format = fmt;
        opts.range = ImageExportRange::WholeMovie;
        opts.symbol_id = None;
        let base = sub.join(format!("shot.{ext}"));

        let r1 = export_project_image(&proj_long, &base, &opts)
            .unwrap_or_else(|e| panic!("6-frame {fmt:?} export failed: {e}"));
        assert_eq!(r1.files.len(), 6);
        for n in 1..=6 {
            assert!(sub.join(format!("shot_{n:04}.{ext}")).is_file());
        }

        // Re-export shorter 3-frame sequence with updated artwork: tail frames 0004..0006 must disappear.
        let r2 = export_project_image(&proj_short, &base, &opts)
            .unwrap_or_else(|e| panic!("3-frame {fmt:?} re-export failed: {e}"));
        assert_eq!(r2.files.len(), 3);
        for n in 1..=3 {
            let p = sub.join(format!("shot_{n:04}.{ext}"));
            assert!(p.is_file(), "expected {p:?} to exist");
        }
        for stale in 4..=6 {
            let p = sub.join(format!("shot_{stale:04}.{ext}"));
            assert!(!p.exists(), "stale tail frame {p:?} must be removed");
        }

        let (manifest, existed) = read_sequence_manifest(&sub).unwrap();
        assert!(existed);
        let entry = manifest
            .sequences
            .values()
            .find(|entry| entry.stem == "shot" && entry.family.as_deref() == Some(ext))
            .expect("shot manifest entry for output family");
        assert_eq!(entry.files.len(), 3);
        for rec in &entry.files {
            let disk_bytes = fs::read(sub.join(&rec.name)).unwrap();
            assert_eq!(rec.bytes, disk_bytes.len() as u64);
            assert_eq!(rec.sha256, sha256_hex(&disk_bytes));
        }
    }
}

#[test]
fn same_stem_different_formats_coexist_and_legacy_stem_only_entries_migrate() {
    let guard = TempDirGuard::new("families-and-migration");
    let coexist_dir = guard.path().join("coexist");
    let project = build_animated_project(3, 21);

    let mut options = ImageExportOptions::for_project(&project, 0);
    options.range = ImageExportRange::WholeMovie;
    options.symbol_id = None;
    options.format = ImageExportFormat::Png;
    export_project_image(&project, &coexist_dir.join("shot.png"), &options)
        .expect("export PNG family");
    let png_before: Vec<Vec<u8>> = (1..=3)
        .map(|idx| fs::read(coexist_dir.join(format!("shot_{idx:04}.png"))).unwrap())
        .collect();

    options.format = ImageExportFormat::Jpeg;
    export_project_image(&project, &coexist_dir.join("shot.jpg"), &options)
        .expect("same-stem JPEG family must coexist with PNG family");
    for (idx, expected) in (1..=3).zip(&png_before) {
        assert_eq!(
            fs::read(coexist_dir.join(format!("shot_{idx:04}.png"))).unwrap(),
            *expected,
            "JPEG export must not remove or replace PNG frame {idx}"
        );
        assert!(coexist_dir.join(format!("shot_{idx:04}.jpg")).is_file());
    }
    let (manifest, _) = read_sequence_manifest(&coexist_dir).unwrap();
    assert!(
        manifest
            .sequences
            .values()
            .any(|entry| { entry.stem == "shot" && entry.family.as_deref() == Some("png") })
    );
    assert!(
        manifest
            .sequences
            .values()
            .any(|entry| { entry.stem == "shot" && entry.family.as_deref() == Some("jpg") })
    );

    // Simulate a pre-family v1 manifest, then prove the first matching re-export claims and
    // migrates that entry instead of treating its frames as unowned.
    let migration_dir = guard.path().join("migration");
    let long_project = build_animated_project(4, 31);
    let short_project = build_animated_project(2, 41);
    options.format = ImageExportFormat::Png;
    export_project_image(&long_project, &migration_dir.join("legacy.png"), &options)
        .expect("seed PNG family");
    let (mut old_manifest, _) = read_sequence_manifest(&migration_dir).unwrap();
    let (_, mut old_entry) = old_manifest
        .sequences
        .pop_first()
        .expect("one manifest entry");
    old_entry.family = None;
    old_manifest.sequences.insert("legacy".into(), old_entry);
    fs::write(
        migration_dir.join(SEQUENCE_MANIFEST_FILENAME),
        serde_json::to_vec_pretty(&old_manifest).unwrap(),
    )
    .unwrap();

    export_project_image(&short_project, &migration_dir.join("legacy.png"), &options)
        .expect("matching legacy stem-only entry must migrate");
    assert!(migration_dir.join("legacy_0001.png").is_file());
    assert!(migration_dir.join("legacy_0002.png").is_file());
    assert!(!migration_dir.join("legacy_0003.png").exists());
    assert!(!migration_dir.join("legacy_0004.png").exists());
    let (migrated, _) = read_sequence_manifest(&migration_dir).unwrap();
    assert_eq!(migrated.sequences.len(), 1);
    assert!(
        migrated
            .sequences
            .values()
            .any(|entry| { entry.stem == "legacy" && entry.family.as_deref() == Some("png") })
    );
}

#[test]
fn reserved_manifest_key_delimiter_is_rejected_without_creating_output() {
    let guard = TempDirGuard::new("reserved-family-key");
    let out_dir = guard.path().join("output");
    let error = match SequenceSetTransaction::begin_for_family(&out_dir, "shot\u{1f}png", "png") {
        Ok(_) => panic!("control characters must not alias family-scoped manifest keys"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("Invalid sequence base name"));
    assert!(!out_dir.exists());
}

#[test]
fn failed_first_commit_keeps_only_the_stable_lock_and_allows_retry() {
    let guard = TempDirGuard::new("failed-first-commit-lock");
    let out_dir = guard.path().join("output");
    let mut first = SequenceSetTransaction::begin_for_family(&out_dir, "shot", "png").unwrap();
    first.preflight(&["shot_0001.png".into()]).unwrap();
    first.stage_file("shot_0001.png", b"first attempt").unwrap();
    let error = first
        .commit_with_fault_hook(|step| {
            if step == 0 {
                Err(std::io::Error::other("injected first-swap failure"))
            } else {
                Ok(())
            }
        })
        .expect_err("injected first commit must fail");
    assert!(error.to_string().contains("injected first-swap failure"));
    let mut remaining = fs::read_dir(&out_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    remaining.sort();
    assert_eq!(remaining, vec![SEQUENCE_COMMIT_LOCK_FILENAME]);

    let mut retry = SequenceSetTransaction::begin_for_family(&out_dir, "shot", "png").unwrap();
    retry.preflight(&["shot_0001.png".into()]).unwrap();
    retry.stage_file("shot_0001.png", b"retry frame").unwrap();
    retry.commit().unwrap();
    assert_eq!(
        fs::read(out_dir.join("shot_0001.png")).unwrap(),
        b"retry frame"
    );
    assert!(out_dir.join(SEQUENCE_MANIFEST_FILENAME).is_file());
    assert!(out_dir.join(SEQUENCE_COMMIT_LOCK_FILENAME).is_file());
}

#[test]
fn concurrent_different_stem_commits_serialize_manifest_updates() {
    let guard = TempDirGuard::new("concurrent-manifest");
    let out_dir = guard.path().join("shared");
    let mut first = SequenceSetTransaction::begin_for_family(&out_dir, "first", "png").unwrap();
    first.preflight(&["first_0001.png".into()]).unwrap();
    first.stage_file("first_0001.png", b"first frame").unwrap();
    let mut second = SequenceSetTransaction::begin_for_family(&out_dir, "second", "jpg").unwrap();
    second.preflight(&["second_0001.jpg".into()]).unwrap();
    second
        .stage_file("second_0001.jpg", b"second frame")
        .unwrap();

    let (first_entered_tx, first_entered_rx) = mpsc::channel();
    let (release_first_tx, release_first_rx) = mpsc::channel();
    let first_worker = thread::spawn(move || {
        first.commit_with_fault_hook(|step| {
            if step == 0 {
                first_entered_tx.send(()).unwrap();
                release_first_rx.recv().unwrap();
            }
            Ok(())
        })
    });
    first_entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("first commit reached locked swap");

    let (second_started_tx, second_started_rx) = mpsc::channel();
    let (second_entered_tx, second_entered_rx) = mpsc::channel();
    let second_worker = thread::spawn(move || {
        second_started_tx.send(()).unwrap();
        second.commit_with_fault_hook(|step| {
            if step == 0 {
                second_entered_tx.send(()).unwrap();
            }
            Ok(())
        })
    });
    second_started_rx.recv().unwrap();
    assert!(
        matches!(
            second_entered_rx.recv_timeout(Duration::from_millis(250)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ),
        "second commit must wait for the directory manifest lock"
    );

    release_first_tx.send(()).unwrap();
    first_worker.join().unwrap().unwrap();
    second_entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("second commit resumed after lock release");
    second_worker.join().unwrap().unwrap();

    assert_eq!(
        fs::read(out_dir.join("first_0001.png")).unwrap(),
        b"first frame"
    );
    assert_eq!(
        fs::read(out_dir.join("second_0001.jpg")).unwrap(),
        b"second frame"
    );
    let (manifest, _) = read_sequence_manifest(&out_dir).unwrap();
    assert!(
        manifest
            .sequences
            .values()
            .any(|entry| { entry.stem == "first" && entry.family.as_deref() == Some("png") })
    );
    assert!(
        manifest
            .sequences
            .values()
            .any(|entry| { entry.stem == "second" && entry.family.as_deref() == Some("jpg") })
    );
}

#[test]
fn atlas_files_second_sequences_and_user_files_coexist_untouched_across_shorter_reexports_and_failures()
 {
    let guard = TempDirGuard::new("coexistence");
    let out_dir = guard.path().join("MyProject");
    fs::create_dir_all(&out_dir).unwrap();

    let proj_10 = build_animated_project(10, 5);
    let proj_4 = build_animated_project(4, 77);
    let proj_1 = build_animated_project(1, 140);

    // 1. Export sprite-sheet atlases (`spritesheet.json` + `spritesheet.png` AND `atlas.json` + `atlas.png`)
    //    into the same project folder (`KeepFramesAndAtlas` / Publish coexistence).
    let atlas_opts = AtlasOptions {
        format: AtlasFormat::JsonHash,
        image_format: AtlasImageFormat::Png,
        ..AtlasOptions::default()
    };
    export_project_atlas(&proj_10, out_dir.join("spritesheet.json"), &atlas_opts)
        .expect("export spritesheet atlas");
    export_project_atlas(&proj_10, out_dir.join("atlas.json"), &atlas_opts)
        .expect("export custom atlas");

    // 2. Add an unrelated user asset and an independent `walk` sequence in the same folder.
    fs::write(
        out_dir.join("README-notes.txt"),
        b"artist hand-written notes",
    )
    .unwrap();
    let mut seq_opts = ImageExportOptions::for_project(&proj_4, 0);
    seq_opts.format = ImageExportFormat::Png;
    seq_opts.range = ImageExportRange::WholeMovie;
    seq_opts.symbol_id = None;
    export_project_image(&proj_4, &out_dir.join("walk.png"), &seq_opts)
        .expect("export sibling walk sequence");

    let preserved_names = [
        "spritesheet.json",
        "spritesheet.png",
        "atlas.json",
        "atlas.png",
        "README-notes.txt",
        "walk_0001.png",
        "walk_0002.png",
        "walk_0003.png",
        "walk_0004.png",
    ];
    let baseline_preserved: BTreeMap<String, Vec<u8>> = preserved_names
        .iter()
        .map(|name| ((*name).to_owned(), fs::read(out_dir.join(name)).unwrap()))
        .collect();

    // 3. Publish 10-frame `frame` sequence (`frame_0001..0010.png`).
    let rep_10 = export_project_image(&proj_10, &out_dir.join("frame.png"), &seq_opts)
        .expect("export 10-frame sequence");
    assert_eq!(rep_10.files.len(), 10);

    // 4. Shorten to 4 frames (`frame_0001..0004.png`): `frame_0005..0010.png` removed, atlases untouched.
    let rep_4 = export_project_image(&proj_4, &out_dir.join("frame.png"), &seq_opts)
        .expect("re-export 4-frame sequence");
    assert_eq!(rep_4.files.len(), 4);
    for idx in 1..=4 {
        assert!(out_dir.join(format!("frame_{idx:04}.png")).is_file());
    }
    for stale in 5..=10 {
        assert!(!out_dir.join(format!("frame_{stale:04}.png")).exists());
    }
    for (name, expected) in &baseline_preserved {
        assert_eq!(
            &fs::read(out_dir.join(name)).unwrap(),
            expected,
            "{name} changed"
        );
    }

    // 5. Inject a late failure on frame 3 of 4 during a subsequent re-export:
    //    the committed 4-frame sequence + manifest + atlases must remain 100% byte-identical!
    let snapshot_before_fail = snapshot_directory_bytes(&out_dir);
    let err = export_project_image_with_progress(
        &proj_10,
        &out_dir.join("frame.png"),
        &seq_opts,
        |frame_1, _total| {
            if frame_1 == 3 {
                Err(ExportError::Options("injected frame 3 failure".into()))
            } else {
                Ok(())
            }
        },
    )
    .expect_err("injected late failure must abort transaction");
    assert!(err.to_string().contains("injected frame 3 failure"));
    assert_eq!(snapshot_directory_bytes(&out_dir), snapshot_before_fail);

    // 6. Shorten from 4 frames -> 1 frame (`WholeMovie` produces `frame.png` and removes `frame_0001..0004.png`),
    //    then expand back 1 frame -> 4 frames (`frame.png` removed, `frame_0001..0004.png` restored).
    let rep_1 = export_project_image(&proj_1, &out_dir.join("frame.png"), &seq_opts)
        .expect("shorten to 1-frame WholeMovie");
    assert_eq!(rep_1.files, vec![out_dir.join("frame.png")]);
    assert!(out_dir.join("frame.png").is_file());
    for stale in 1..=4 {
        assert!(!out_dir.join(format!("frame_{stale:04}.png")).exists());
    }

    let rep_back_4 = export_project_image(&proj_4, &out_dir.join("frame.png"), &seq_opts)
        .expect("expand back to 4 frames");
    assert_eq!(rep_back_4.files.len(), 4);
    assert!(!out_dir.join("frame.png").exists());
    for (name, expected) in &baseline_preserved {
        assert_eq!(
            &fs::read(out_dir.join(name)).unwrap(),
            expected,
            "{name} changed"
        );
    }
}

#[test]
fn video_png_sequence_handles_audio_companion_shorter_reexport_and_mid_render_cancellation() {
    let guard = TempDirGuard::new("video-png-seq");
    let out_dir = guard.path().join("VideoSeq");

    // Build a 5-frame project with timeline sound so `movie.wav` is emitted alongside `movie_0001..0005.png`.
    let mut proj_5 = build_animated_project(5, 22);
    let wav_bytes = make_sine_pcm16_wav(22_050, 4_410);
    proj_5
        .resources
        .push(ResourceUnit::from_bytes("snd_beep", "beep.wav", "audio/wav", &wav_bytes).unwrap());
    let root_id = proj_5.root.clone();
    proj_5.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].sound =
        Some(FrameSound::new("snd_beep"));
    proj_5.upgrade_format_for_features();
    proj_5.validate().unwrap();

    // 1. Cancelled export into a non-existent folder cleans up staging AND the newly created empty folder.
    let cancel_err = export_project_video_with_progress(
        &proj_5,
        &out_dir.join("movie.png"),
        &VideoOptions {
            format: VideoFormat::PngSequence,
            width: 48,
            height: 36,
            include_audio: true,
            ..VideoOptions::default()
        },
        |progress| progress.frame_index < 3,
    )
    .expect_err("cancelling on frame 3 of 5 must fail");
    assert!(cancel_err.to_string().contains("cancelled"));
    assert!(
        !out_dir.exists(),
        "newly created output folder must be removed when initial export cancels"
    );

    // 2. Full 5-frame PNG sequence + audio succeeds.
    let rep_5 = export_project_video(
        &proj_5,
        &out_dir.join("movie_0001.png"),
        &VideoOptions {
            format: VideoFormat::PngSequence,
            width: 48,
            height: 36,
            include_audio: true,
            ..VideoOptions::default()
        },
    )
    .expect("5-frame video PNG sequence + wav");
    assert_eq!(rep_5.frame_count, 5);
    assert!(rep_5.audio_included);
    for idx in 1..=5 {
        assert!(out_dir.join(format!("movie_{idx:04}.png")).is_file());
    }
    assert!(out_dir.join("movie.wav").is_file());

    // 3. Cancelling a subsequent re-export mid-way leaves the 5-frame set + `movie.wav` byte-identical.
    let before_cancel = snapshot_directory_bytes(&out_dir);
    let proj_3 = build_animated_project(3, 120);
    let _ = export_project_video_with_progress(
        &proj_3,
        &out_dir.join("movie_0001.png"),
        &VideoOptions {
            format: VideoFormat::PngSequence,
            width: 48,
            height: 36,
            include_audio: false,
            ..VideoOptions::default()
        },
        |progress| progress.frame_index < 2,
    )
    .expect_err("cancelled re-export must fail");
    assert_eq!(snapshot_directory_bytes(&out_dir), before_cancel);

    // 4. Completing the 3-frame audio-disabled re-export removes `movie_0004.png`, `movie_0005.png`, AND stale `movie.wav`.
    let rep_3 = export_project_video(
        &proj_3,
        &out_dir.join("movie_0001.png"),
        &VideoOptions {
            format: VideoFormat::PngSequence,
            width: 48,
            height: 36,
            include_audio: false,
            ..VideoOptions::default()
        },
    )
    .expect("3-frame video PNG sequence without audio");
    assert_eq!(rep_3.frame_count, 3);
    assert!(!rep_3.audio_included);
    for idx in 1..=3 {
        assert!(out_dir.join(format!("movie_{idx:04}.png")).is_file());
    }
    assert!(!out_dir.join("movie_0004.png").exists());
    assert!(!out_dir.join("movie_0005.png").exists());
    assert!(!out_dir.join("movie.wav").exists());
}

#[test]
fn unowned_collisions_external_edits_case_insensitivity_and_mid_swap_faults_are_protected() {
    let guard = TempDirGuard::new("safety-and-rollback");
    let proj_4 = build_animated_project(4, 30);
    let proj_6 = build_animated_project(6, 80);
    let proj_2 = build_animated_project(2, 160);

    let mut opts = ImageExportOptions::for_project(&proj_4, 0);
    opts.format = ImageExportFormat::Png;
    opts.range = ImageExportRange::WholeMovie;
    opts.symbol_id = None;

    // 1. Unowned file in folder with no manifest refuses cleanly before creating any sibling frame or manifest.
    let unowned_dir = guard.path().join("unowned");
    fs::create_dir_all(&unowned_dir).unwrap();
    fs::write(unowned_dir.join("hero_0003.png"), b"precious user frame").unwrap();
    let err_unowned = export_project_image(&proj_4, &unowned_dir.join("hero.png"), &opts)
        .expect_err("unowned hero_0003.png must block sequence export");
    assert!(matches!(err_unowned, ExportError::Destination(_)));
    assert_eq!(
        fs::read(unowned_dir.join("hero_0003.png")).unwrap(),
        b"precious user frame"
    );
    assert!(!unowned_dir.join("hero_0001.png").exists());
    assert!(!unowned_dir.join(SEQUENCE_MANIFEST_FILENAME).exists());

    // 2. Expanding a 4-frame manifest-owned sequence to 6 frames when `hero_0005.png` is unowned
    //    refuses cleanly and preserves `hero_0001..0004.png` + `hero_0005.png` byte-for-byte.
    let expand_dir = guard.path().join("expand");
    export_project_image(&proj_4, &expand_dir.join("hero.png"), &opts)
        .expect("initial 4-frame export");
    fs::write(expand_dir.join("hero_0005.png"), b"unowned frame 5").unwrap();
    let before_expand = snapshot_directory_bytes(&expand_dir);
    let err_expand = export_project_image(&proj_6, &expand_dir.join("hero.png"), &opts)
        .expect_err("unowned hero_0005.png must block 6-frame expansion");
    assert!(matches!(err_expand, ExportError::Destination(_)));
    assert_eq!(snapshot_directory_bytes(&expand_dir), before_expand);

    // 3. Externally modified stale tail frame (`hero_0004.png` edited by user) is NOT auto-deleted
    //    when shortening from 4 frames -> 2 frames!
    let edited_dir = guard.path().join("edited-tail");
    export_project_image(&proj_4, &edited_dir.join("hero.png"), &opts)
        .expect("initial 4-frame export");
    fs::write(
        edited_dir.join("hero_0004.png"),
        b"user retouched in Photoshop",
    )
    .unwrap();
    let before_shorten = snapshot_directory_bytes(&edited_dir);
    let err_edited = export_project_image(&proj_2, &edited_dir.join("hero.png"), &opts)
        .expect_err("externally modified tail frame must not be auto-deleted");
    assert!(err_edited.to_string().contains("modified outside NeoFlash"));
    assert_eq!(snapshot_directory_bytes(&edited_dir), before_shorten);

    // 4. Case-insensitive collision handling & case-changing re-export.
    let case_dir = guard.path().join("case-policy");
    fs::create_dir_all(&case_dir).unwrap();
    fs::write(case_dir.join("CLIP_0002.PNG"), b"uppercase unowned").unwrap();
    {
        let mut tx = SequenceSetTransaction::begin(&case_dir, "clip").unwrap();
        tx.set_case_insensitive_for_test(true);
        let err_case = tx
            .preflight(&["clip_0001.png".to_owned(), "clip_0002.png".to_owned()])
            .expect_err("case-folded unowned CLIP_0002.PNG must collide");
        assert!(matches!(err_case, ExportError::Destination(_)));
    }
    fs::remove_file(case_dir.join("CLIP_0002.PNG")).unwrap();

    // If the real filesystem is case-insensitive (e.g. default macOS APFS), verify exporting `Clip.png` (4 frames)
    // and then re-exporting `clip.png` (2 frames) replaces cleanly without duplicate manifest keys or stale tail frames.
    let probe_tx = SequenceSetTransaction::begin(&case_dir, "probe").unwrap();
    let fs_is_ci = probe_tx.is_case_insensitive();
    drop(probe_tx);
    if fs_is_ci {
        export_project_image(&proj_4, &case_dir.join("Clip.png"), &opts)
            .expect("export Clip_0001..0004.png");
        export_project_image(&proj_2, &case_dir.join("clip.png"), &opts)
            .expect("re-export shorter clip_0001..0002.png with lowercase stem");
        assert!(!case_dir.join("Clip_0003.png").exists());
        assert!(!case_dir.join("Clip_0004.png").exists());
        let (manifest, _) = read_sequence_manifest(&case_dir).unwrap();
        assert_eq!(
            manifest.sequences.len(),
            1,
            "case-only stem change must replace prior manifest entry"
        );
        assert!(manifest.sequences.values().any(|entry| {
            entry.stem.eq_ignore_ascii_case("clip") && entry.family.as_deref() == Some("png")
        }));
    }

    // 5. Deterministic mid-commit fault injection: after 6-frame export, inject an I/O failure mid-swap
    //    (both at file index 2 of 4 AND at manifest placement step 4 of 4) and prove all 6 prior frames
    //    (including tail frames 0005..0006) + `.neoflash-sequences.json` roll back byte-identically!
    let swap_dir = guard.path().join("mid-swap");
    export_project_image(&proj_6, &swap_dir.join("swap.png"), &opts)
        .expect("seed 6-frame sequence");
    let baseline_swap = snapshot_directory_bytes(&swap_dir);

    for fail_step in [2usize, 4usize] {
        let mut tx = SequenceSetTransaction::begin_for_family(&swap_dir, "swap", "png").unwrap();
        let names: Vec<String> = (1..=4).map(|i| format!("swap_{i:04}.png")).collect();
        tx.preflight(&names).unwrap();
        for name in &names {
            tx.stage_file(name, format!("new-payload-for-{name}").as_bytes())
                .unwrap();
        }
        let swap_err = tx
            .commit_with_fault_hook(|step| {
                if step == fail_step {
                    Err(std::io::Error::other(format!(
                        "injected rename fault at step {step}"
                    )))
                } else {
                    Ok(())
                }
            })
            .expect_err("mid-swap fault must fail commit");
        assert!(swap_err.to_string().contains("injected rename fault"));
        assert_eq!(
            snapshot_directory_bytes(&swap_dir),
            baseline_swap,
            "rollback at step {fail_step} must restore all 6 prior frames and manifest byte-for-byte"
        );
    }
}

#[test]
fn clicked_0001_sequence_reexport_and_silent_video_wav_coexistence_work() {
    let guard = TempDirGuard::new("clicked-0001-and-wav");
    let proj_4 = build_animated_project(4, 15);
    let proj_2 = build_animated_project(2, 95);
    let proj_1 = build_animated_project(1, 175);

    // 1. Standalone image sequence: clicking `walk_0001.png` in the Save dialog replaces
    //    `walk_0001..0004.png` in place and prunes tail frames (and on a 1-frame WholeMovie
    //    writes `walk.png` while pruning `walk_0001..0002.png`), whereas `CurrentFrame` keeps
    //    exact `shot_0001.png`.
    let img_dir = guard.path().join("ImgSeq0001");
    let mut seq_opts = ImageExportOptions::for_project(&proj_4, 0);
    seq_opts.format = ImageExportFormat::Png;
    seq_opts.range = ImageExportRange::WholeMovie;
    seq_opts.symbol_id = None;

    export_project_image(&proj_4, &img_dir.join("walk.png"), &seq_opts)
        .expect("initial 4-frame walk sequence");
    for idx in 1..=4 {
        assert!(img_dir.join(format!("walk_{idx:04}.png")).is_file());
    }

    let rep_2 = export_project_image(&proj_2, &img_dir.join("walk_0001.png"), &seq_opts)
        .expect("re-export 2 frames over clicked walk_0001.png");
    assert_eq!(rep_2.files.len(), 2);
    assert!(img_dir.join("walk_0001.png").is_file());
    assert!(img_dir.join("walk_0002.png").is_file());
    assert!(!img_dir.join("walk_0003.png").exists());
    assert!(!img_dir.join("walk_0004.png").exists());
    assert!(!img_dir.join("walk_0001_0001.png").exists());

    let rep_1 = export_project_image(&proj_1, &img_dir.join("walk_0001.png"), &seq_opts)
        .expect("re-export 1-frame WholeMovie over clicked walk_0001.png");
    assert_eq!(rep_1.files, vec![img_dir.join("walk.png")]);
    assert!(img_dir.join("walk.png").is_file());
    assert!(!img_dir.join("walk_0001.png").exists());
    assert!(!img_dir.join("walk_0002.png").exists());

    let mut single_opts = seq_opts.clone();
    single_opts.range = ImageExportRange::CurrentFrame(0);
    let rep_single = export_project_image(&proj_2, &img_dir.join("shot_0001.png"), &single_opts)
        .expect("single-frame export keeps exact shot_0001.png");
    assert_eq!(rep_single.files, vec![img_dir.join("shot_0001.png")]);
    assert!(img_dir.join("shot_0001.png").is_file());

    // 2. Video PNG sequence: silent project with `include_audio: true` (default) preserves an
    //    unrelated user `silent.wav` in the destination folder; a sound-bearing project refuses
    //    an unowned file or broken symlink `voiced.wav` cleanly without mutating anything; and
    //    `clip_0001_0001.png` strips only a single `_0001` suffix.
    let vid_dir = guard.path().join("VideoWavPolicy");
    fs::create_dir_all(&vid_dir).unwrap();
    fs::write(vid_dir.join("silent.wav"), b"unrelated-voiceover-wav").unwrap();

    let silent_rep = export_project_video(
        &proj_2,
        &vid_dir.join("silent.png"),
        &VideoOptions {
            format: VideoFormat::PngSequence,
            width: 32,
            height: 24,
            include_audio: true,
            ..VideoOptions::default()
        },
    )
    .expect("silent project with include_audio=true must ignore unrelated silent.wav");
    assert!(!silent_rep.audio_included);
    assert!(vid_dir.join("silent_0001.png").is_file());
    assert!(vid_dir.join("silent_0002.png").is_file());
    assert_eq!(
        fs::read(vid_dir.join("silent.wav")).unwrap(),
        b"unrelated-voiceover-wav"
    );

    let mut voiced_proj = build_animated_project(2, 55);
    let wav_bytes = make_sine_pcm16_wav(22_050, 2_205);
    voiced_proj
        .resources
        .push(ResourceUnit::from_bytes("snd_beep", "beep.wav", "audio/wav", &wav_bytes).unwrap());
    let root_id = voiced_proj.root.clone();
    voiced_proj.symbol_mut(&root_id).unwrap().timeline.layers[0].keyframes[0].sound =
        Some(FrameSound::new("snd_beep"));
    voiced_proj.upgrade_format_for_features();
    voiced_proj.validate().unwrap();

    fs::write(vid_dir.join("voiced.wav"), b"user-master-audio").unwrap();
    let unowned_wav_err = export_project_video(
        &voiced_proj,
        &vid_dir.join("voiced.png"),
        &VideoOptions {
            format: VideoFormat::PngSequence,
            width: 32,
            height: 24,
            include_audio: true,
            ..VideoOptions::default()
        },
    )
    .expect_err("voiced project must refuse clobbering unowned voiced.wav");
    assert!(matches!(unowned_wav_err, ExportError::Destination(_)));
    assert!(!vid_dir.join("voiced_0001.png").exists());
    assert_eq!(
        fs::read(vid_dir.join("voiced.wav")).unwrap(),
        b"user-master-audio"
    );
    fs::remove_file(vid_dir.join("voiced.wav")).unwrap();

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            guard.path().join("dangling-target.wav"),
            vid_dir.join("voiced.wav"),
        )
        .unwrap();
        let broken_link_err = export_project_video(
            &voiced_proj,
            &vid_dir.join("voiced.png"),
            &VideoOptions {
                format: VideoFormat::PngSequence,
                width: 32,
                height: 24,
                include_audio: true,
                ..VideoOptions::default()
            },
        )
        .expect_err("broken symlink voiced.wav must fail closed");
        assert!(matches!(broken_link_err, ExportError::Destination(_)));
        assert!(!vid_dir.join("voiced_0001.png").exists());
        assert!(
            fs::symlink_metadata(vid_dir.join("voiced.wav"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        fs::remove_file(vid_dir.join("voiced.wav")).unwrap();
    }

    export_project_video(
        &proj_2,
        &vid_dir.join("clip_0001_0001.png"),
        &VideoOptions {
            format: VideoFormat::PngSequence,
            width: 32,
            height: 24,
            include_audio: false,
            ..VideoOptions::default()
        },
    )
    .expect("single trailing _0001 strip");
    assert!(vid_dir.join("clip_0001_0001.png").is_file());
    assert!(vid_dir.join("clip_0001_0002.png").is_file());
    assert!(!vid_dir.join("clip_0001.png").exists());
}
