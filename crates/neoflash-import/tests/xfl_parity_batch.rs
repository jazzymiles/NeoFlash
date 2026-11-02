//! End-to-end parity tests for XFL directories and `.fla` ZIP archives:
//! general `DOMShape` contours & gradients, `Button` symbols & instances,
//! `<media>` bitmaps/sounds (`LIBRARY/*` and `bin/*.dat`), and `DOMFrame`
//! frame sounds with format-7 custom `<SoundEnvelope>`.
//! All synthetic media fixtures are generated inline; no private corpus data is used.

use image::{ExtendedColorType, ImageEncoder};
use neoflash_core::{
    ElementKind, GradientKind, InstancePlayback, PathCommand, Player, RuntimeDrawKind,
    SoundCommand, SoundEffect, SoundSync, SymbolKind, envelope_gains, evaluate,
};
use neoflash_import::{ImportFormat, Severity, import_project, inspect};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use zip::write::SimpleFileOptions;

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "neoflash-xfl-parity-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn synthetic_png(width: u32, height: u32) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            rgba.extend_from_slice(&[(x * 40) as u8, (y * 60) as u8, 200, 255]);
        }
    }
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&rgba, width, height, ExtendedColorType::Rgba8)
        .unwrap();
    out
}

fn synthetic_jpeg(width: u32, height: u32) -> Vec<u8> {
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            rgb.extend_from_slice(&[220, (x * 30) as u8, (y * 40) as u8]);
        }
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .write_image(&rgb, width, height, ExtendedColorType::Rgb8)
        .unwrap();
    out
}

/// Native Flash CS5/CS6 `bin/*.dat` revision-3 uncompressed 32-bit ARGB raster.
fn synthetic_flash_bitmap_dat(width: u16, height: u16) -> Vec<u8> {
    let stride = width * 4;
    let mut out = Vec::new();
    out.push(3); // revision = 3
    out.push(5); // format = 5 (32-bit ARGB)
    out.extend_from_slice(&stride.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    for word in [0_i32, i32::from(width) * 20, 0, i32::from(height) * 20] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.push(1); // alpha present
    out.push(0); // uncompressed
    for _ in 0..(usize::from(width) * usize::from(height)) {
        out.extend_from_slice(&[255, 40, 180, 90]); // ARGB premultiplied opaque pixel
    }
    out
}

fn synthetic_wav_44k_stereo(sample_frames: u32) -> Vec<u8> {
    let channels = 2_u16;
    let bits_per_sample = 16_u16;
    let block_align = channels * (bits_per_sample / 8);
    let sample_rate = 44_100_u32;
    let byte_rate = sample_rate * u32::from(block_align);
    let data_bytes = sample_frames * u32::from(block_align);
    let riff_size = 36 + data_bytes;

    let mut out = Vec::with_capacity((44 + data_bytes) as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for i in 0..sample_frames {
        let sample = ((i as i32 % 64) - 32) as i16 * 256;
        out.extend_from_slice(&sample.to_le_bytes());
        out.extend_from_slice(&(-sample).to_le_bytes());
    }
    out
}

/// Valid MPEG-1 Layer III 128 kbps 44.1 kHz stereo frame (417 bytes).
fn synthetic_mp3_frame() -> Vec<u8> {
    let mut frame = vec![0_u8; 417];
    // 0xFFFB9000: sync=0x7FF, MPEG-1 (11), Layer III (01), no CRC (1), 128kbps (1001), 44.1kHz (00)
    frame[0] = 0xFF;
    frame[1] = 0xFB;
    frame[2] = 0x90;
    frame[3] = 0x00;
    frame
}

fn write_directory_fixture(root: &Path, entries: &BTreeMap<String, Vec<u8>>) {
    for (rel, bytes) in entries {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }
}

fn write_zip_fla_fixture(fla_path: &Path, entries: &BTreeMap<String, Vec<u8>>) {
    let file = fs::File::create(fla_path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default();
    for (rel, bytes) in entries {
        zip.start_file(rel, opts).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn xfl_directory_and_zip_fla_import_general_shapes_gradients_buttons_media_and_frame_sounds() {
    let button_xml = r##"<DOMSymbolItem xmlns="http://ns.adobe.com/xfl/2008/" name="MenuButton" symbolType="button">
  <timeline>
    <DOMTimeline name="MenuButton">
      <layers>
        <DOMLayer name="States">
          <frames>
            <DOMFrame index="0">
              <elements>
                <DOMShape>
                  <fills><FillStyle index="1"><SolidColor color="#FF0000"/></FillStyle></fills>
                  <edges><Edge fillStyle1="1" edges="!0 0|800 0|800 400|0 400|0 0"/></edges>
                </DOMShape>
              </elements>
            </DOMFrame>
            <DOMFrame index="1">
              <elements>
                <DOMShape>
                  <fills><FillStyle index="1"><SolidColor color="#00FF00"/></FillStyle></fills>
                  <edges><Edge fillStyle1="1" edges="!0 0|800 0|800 400|0 400|0 0"/></edges>
                </DOMShape>
              </elements>
            </DOMFrame>
            <DOMFrame index="2">
              <elements>
                <DOMShape>
                  <fills><FillStyle index="1"><SolidColor color="#0000FF"/></FillStyle></fills>
                  <edges><Edge fillStyle1="1" edges="!0 0|800 0|800 400|0 400|0 0"/></edges>
                </DOMShape>
              </elements>
            </DOMFrame>
            <DOMFrame index="3">
              <elements>
                <DOMShape>
                  <fills><FillStyle index="1"><SolidColor color="#FFFF00"/></FillStyle></fills>
                  <edges><Edge fillStyle1="1" edges="!-200 -200|1000 -200|1000 600|-200 600|-200 -200"/></edges>
                </DOMShape>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timeline>
</DOMSymbolItem>"##;

    let dom_document = r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="640" height="480" frameRate="24" backgroundColor="#112233" xflVersion="2.2" autoSaveHasPrompted="true">
  <media>
    <DOMBitmapItem name="hero.png" href="hero.png" allowSmoothing="true"/>
    <DOMBitmapItem name="bg.jpg" href="bg.jpg" isJPEG="true"/>
    <DOMBitmapItem name="cached_icon" bitmapDataHRef="M 1.dat"/>
    <DOMSoundItem name="click.wav" href="click.wav" format="44kHz 16bit Stereo" sampleCount="44100"/>
    <DOMSoundItem name="music.mp3" href="music.mp3" format="44kHz 16bit Stereo" sampleCount="22050"/>
    <DOMSoundItem name="bin_tone" soundDataHRef="S 1.dat" format="22kHz 16bit Mono" sampleCount="256"/>
  </media>
  <symbols>
    <Include href="MenuButton.xml" itemIcon="0" loadImmediate="false"/>
  </symbols>
  <timelines>
    <DOMTimeline name="Scene 1">
      <layers>
        <DOMLayer name="Audio">
          <frames>
            <DOMFrame index="0" soundName="click.wav" soundSync="start" soundEffect="custom" soundLoopMode="repeat" soundLoop="2">
              <SoundEnvelope>
                <SoundEnvelopePoint mark44="0" level0="32768" level1="16384"/>
                <SoundEnvelopePoint mark44="11025" level0="24576" level1="24576"/>
                <SoundEnvelopePoint mark44="22050" level0="8192" level1="32768"/>
                <SoundEnvelopePoint mark44="44100" level0="0" level1="0"/>
              </SoundEnvelope>
              <elements/>
            </DOMFrame>
            <DOMFrame index="1" duration="2" soundName="music.mp3" soundSync="stream" soundEffect="fade out">
              <elements/>
            </DOMFrame>
            <DOMFrame index="3" soundName="bin_tone" soundSync="event" soundLoopMode="loop">
              <elements/>
            </DOMFrame>
            <DOMFrame index="4" soundName="click.wav" soundSync="stop">
              <elements/>
            </DOMFrame>
          </frames>
        </DOMLayer>
        <DOMLayer name="Artwork">
          <frames>
            <DOMFrame index="0" duration="5">
              <elements>
                <DOMShape>
                  <fills>
                    <FillStyle index="1">
                      <LinearGradient>
                        <matrix><Matrix a="0.061" b="0" c="0" d="0.061" tx="50" ty="50"/></matrix>
                        <GradientEntry color="#FF0000" ratio="0"/>
                        <GradientEntry color="#0000FF" ratio="1"/>
                      </LinearGradient>
                    </FillStyle>
                    <FillStyle index="2">
                      <SolidColor color="#00AA88" alpha="0.75"/>
                    </FillStyle>
                  </fills>
                  <strokes>
                    <StrokeStyle index="1">
                      <SolidStroke scaleMode="none" weight="0" caps="square" joints="miter">
                        <fill>
                          <RadialGradient focalPointRatio="-0.4">
                            <matrix><Matrix a="0.05" b="0" c="0" d="0.05" tx="50" ty="50"/></matrix>
                            <GradientEntry color="#FFFF00" ratio="0"/>
                            <GradientEntry color="#00FF00" ratio="1"/>
                          </RadialGradient>
                        </fill>
                      </SolidStroke>
                    </StrokeStyle>
                    <StrokeStyle index="2">
                      <DashedStroke scaleMode="normal" weight="3" dash1="4" dash2="2">
                        <fill><SolidColor color="#FFFFFF"/></fill>
                      </DashedStroke>
                    </StrokeStyle>
                  </strokes>
                  <edges>
                    <Edge fillStyle1="1" strokeStyle="1" edges="!0 0|1000 0[1200 500 #3E8.00 #3E8.00|500 700|0 1000|0 0"/>
                    <Edge fillStyle1="1" edges="!200 200|200 600|600 600|600 200|200 200"/>
                    <Edge fillStyle0="1" fillStyle1="1" edges="!0 0|200 200"/>
                    <Edge fillStyle1="2" strokeStyle="2" edges="!1400.5 0|1800 0|1600 400|1400.5 0"/>
                    <Edge cubics="!0 0(;0,0 400,0 1000,0)q0 0 1000 0;"/>
                  </edges>
                </DOMShape>
                <DOMBitmapInstance libraryItemName="hero.png" name="hero_bmp">
                  <matrix><Matrix tx="200" ty="20"/></matrix>
                </DOMBitmapInstance>
                <DOMBitmapInstance libraryItemName="bg.jpg">
                  <matrix><Matrix tx="260" ty="20"/></matrix>
                </DOMBitmapInstance>
                <DOMBitmapInstance libraryItemName="cached_icon">
                  <matrix><Matrix tx="320" ty="20"/></matrix>
                </DOMBitmapInstance>
                <DOMSymbolInstance libraryItemName="MenuButton" symbolType="button" name="play_btn" trackAsMenu="true">
                  <matrix><Matrix tx="120" ty="240"/></matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timelines>
</DOMDocument>"##;

    let mut pcm_22k_mono = Vec::with_capacity(256 * 2);
    for i in 0..256_i16 {
        pcm_22k_mono.extend_from_slice(&(i * 64).to_le_bytes());
    }

    let mut files = BTreeMap::new();
    files.insert("DOMDocument.xml".into(), dom_document.as_bytes().to_vec());
    files.insert(
        "LIBRARY/MenuButton.xml".into(),
        button_xml.as_bytes().to_vec(),
    );
    files.insert("LIBRARY/hero.png".into(), synthetic_png(4, 3));
    files.insert("LIBRARY/bg.jpg".into(), synthetic_jpeg(6, 4));
    files.insert("bin/M 1.dat".into(), synthetic_flash_bitmap_dat(2, 2));
    files.insert("LIBRARY/click.wav".into(), synthetic_wav_44k_stereo(44_100));
    files.insert("LIBRARY/music.mp3".into(), synthetic_mp3_frame());
    files.insert("bin/S 1.dat".into(), pcm_22k_mono);

    let tmp = TempDir::new();
    let xfl_dir = tmp.0.join("unpacked_xfl");
    let fla_zip = tmp.0.join("packaged.fla");
    write_directory_fixture(&xfl_dir, &files);
    write_zip_fla_fixture(&fla_zip, &files);

    for (source_path, expected_format) in [
        (xfl_dir.as_path(), ImportFormat::XflDirectory),
        (fla_zip.as_path(), ImportFormat::XflArchive),
    ] {
        let report = inspect(source_path).unwrap();
        assert_eq!(report.format, expected_format);
        assert!(report.can_import, "{:?}", report.diagnostics);
        for expected_warning in [
            "xfl_cubic_editing_metadata",
            "xfl_stroke_rendering",
            "xfl_shape_approximated",
            "xfl_bitmap_instance_name",
            "xfl_button_track_as_menu",
        ] {
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|d| d.severity == Severity::Warning && d.code == expected_warning),
                "missing warning {expected_warning} in {:?}",
                report.diagnostics
            );
        }

        let project = import_project(source_path).unwrap();
        assert_eq!(project.format_version, 7);
        project.validate().unwrap();
        assert_eq!(project.resources.len(), 6);

        // 1. Verify decoded bitmap resources & stage DOMBitmapInstances.
        for (id, expected_dims) in [
            ("xfl-bitmap-1", (4, 3)),
            ("xfl-bitmap-2", (6, 4)),
            ("xfl-bitmap-3", (2, 2)),
        ] {
            let unit = project.resources.iter().find(|r| r.id == id).expect(id);
            let decoded = unit.decode_image().unwrap();
            assert_eq!((decoded.width, decoded.height), expected_dims);
        }

        let items = evaluate(&project, &project.root, 0);
        let bitmaps: Vec<_> = items
            .iter()
            .filter_map(|item| match &item.kind {
                ElementKind::Bitmap {
                    resource_id,
                    width,
                    height,
                    smoothing,
                } => Some((resource_id.as_str(), *width, *height, *smoothing)),
                _ => None,
            })
            .collect();
        assert_eq!(
            bitmaps,
            vec![
                ("xfl-bitmap-1", 4, 3, true),
                ("xfl-bitmap-2", 6, 4, false),
                ("xfl-bitmap-3", 2, 2, false),
            ]
        );

        // 2. Verify general DOMShape gradients, curves, and hole topology.
        let linear_fill = items
            .iter()
            .find_map(|item| match &item.kind {
                ElementKind::PaintedPath {
                    commands,
                    fill_gradient: Some(grad),
                    ..
                } if grad.kind == GradientKind::Linear => Some((commands, grad)),
                _ => None,
            })
            .expect("linear gradient fill PaintedPath");
        assert!(
            linear_fill
                .0
                .iter()
                .any(|c| matches!(c, PathCommand::QuadraticTo { .. }))
        );
        assert_eq!(
            linear_fill
                .0
                .iter()
                .filter(|c| matches!(c, PathCommand::Close))
                .count(),
            2,
            "outer boundary + interior hole must emit two closed subpaths"
        );
        assert_eq!(linear_fill.1.stops.len(), 2);
        assert_eq!(linear_fill.1.stops[0].color, [255, 0, 0, 255]);
        assert_eq!(linear_fill.1.stops[1].color, [0, 0, 255, 255]);

        let radial_stroke = items
            .iter()
            .find_map(|item| match &item.kind {
                ElementKind::PaintedPath {
                    stroke_gradient: Some(grad),
                    stroke_width,
                    ..
                } if grad.kind == GradientKind::Radial => Some((grad, *stroke_width)),
                _ => None,
            })
            .expect("radial gradient stroke PaintedPath");
        assert_eq!(radial_stroke.1, 1.0); // hairline weight="0" -> 1.0px
        assert!((radial_stroke.0.focal_point_ratio - (-0.4)).abs() < 1e-6);

        // 3. Verify Button symbol, Hit-frame isolation, and Up/Over/Down state transitions.
        let button_symbol = project
            .symbols
            .iter()
            .find(|s| s.name == "MenuButton")
            .expect("MenuButton symbol");
        assert_eq!(button_symbol.kind, SymbolKind::Button);
        assert_eq!(button_symbol.timeline.frame_count, 4);

        let root_symbol = project.symbol(&project.root).unwrap();
        let artwork_layer = root_symbol
            .timeline
            .layers
            .iter()
            .find(|l| l.name == "Artwork")
            .unwrap();
        let has_independent_btn = artwork_layer.keyframes[0].elements.iter().any(|el| {
            matches!(
                &el.kind,
                ElementKind::Instance {
                    symbol_id,
                    playback: InstancePlayback::Independent,
                } if symbol_id == &button_symbol.id
            )
        });
        assert!(has_independent_btn);

        let mut player = Player::new(&project);
        let snap0 = player.runtime_snapshot(&project);
        let btn_obj = snap0
            .objects
            .iter()
            .find(|o| o.display.name == "play_btn")
            .expect("play_btn runtime object");
        assert_eq!(btn_obj.display.owner, "button");
        assert_eq!(
            btn_obj.button_hits.len(),
            1,
            "Hit frame (frame 3) geometry must populate button_hits"
        );
        let btn_ref = btn_obj.object_ref.clone();

        let button_fill_color = |p: &Player| -> [u8; 4] {
            let s = p.runtime_snapshot(&project);
            let btn = s
                .objects
                .iter()
                .find(|o| o.object_ref == btn_ref)
                .expect("live play_btn");
            s.draws
                .iter()
                .filter(|d| d.owner_ref == btn_ref || btn.children.contains(&d.owner_ref))
                .find_map(|d| match &d.item.kind {
                    RuntimeDrawKind::Element(ElementKind::Path { fill, .. }) => Some(*fill),
                    _ => None,
                })
                .expect("button state fill path")
        };
        assert_eq!(button_fill_color(&player), [255, 0, 0, 255]); // Up = red
        assert!(player.object_button_state(&project, &btn_ref, 1));
        assert_eq!(button_fill_color(&player), [0, 255, 0, 255]); // Over = green
        assert!(player.object_button_state(&project, &btn_ref, 2));
        assert_eq!(button_fill_color(&player), [0, 0, 255, 255]); // Down = blue
        assert!(!player.object_button_state(&project, &btn_ref, 3)); // Hit state is not selectable

        // 4. Verify DOMFrame sounds, format-7 custom SoundEnvelope, and Player SoundCommands.
        let audio_layer = root_symbol
            .timeline
            .layers
            .iter()
            .find(|l| l.name == "Audio")
            .unwrap();
        let kf0_sound = audio_layer.keyframes[0].sound.as_ref().unwrap();
        assert_eq!(kf0_sound.resource, "xfl-sound-1");
        assert_eq!(kf0_sound.sync, SoundSync::Start);
        assert_eq!(kf0_sound.loops, 2);
        assert!(!kf0_sound.loop_forever);
        assert_eq!(kf0_sound.effect, SoundEffect::Custom);
        assert_eq!(kf0_sound.envelope.as_ref().unwrap().len(), 4);
        let env_pts = kf0_sound.envelope_points();
        let (left0, right0) = envelope_gains(&env_pts, 0.0);
        assert!((left0 - 1.0).abs() < 1e-4 && (right0 - 0.5).abs() < 1e-4);
        let (left_end, right_end) = envelope_gains(&env_pts, 1.0);
        assert!(left_end.abs() < 1e-4 && right_end.abs() < 1e-4);

        let kf1_sound = audio_layer.keyframes[1].sound.as_ref().unwrap();
        assert_eq!(kf1_sound.resource, "xfl-sound-2");
        assert_eq!(kf1_sound.sync, SoundSync::Stream);
        assert_eq!(kf1_sound.effect, SoundEffect::FadeOut);

        let kf3_sound = audio_layer.keyframes[2].sound.as_ref().unwrap();
        assert_eq!(kf3_sound.resource, "xfl-sound-3");
        assert_eq!(kf3_sound.sync, SoundSync::Event);
        assert!(kf3_sound.loop_forever);

        let kf4_sound = audio_layer.keyframes[3].sound.as_ref().unwrap();
        assert_eq!(kf4_sound.resource, "xfl-sound-1");
        assert_eq!(kf4_sound.sync, SoundSync::Stop);

        // Frame 0 entered on Player::new -> Start Play command with 4-point custom envelope.
        let cmds0 = player.drain_sound_commands();
        assert!(cmds0.iter().any(|c| matches!(
            c,
            SoundCommand::Play {
                resource,
                sync: SoundSync::Start,
                loops: 2,
                envelope,
                ..
            } if resource == "xfl-sound-1" && envelope.len() == 4
        )));

        // Advance to frame 1 -> StreamStart on music.mp3.
        player.tick(&project);
        let cmds1 = player.drain_sound_commands();
        assert!(cmds1.iter().any(|c| matches!(
            c,
            SoundCommand::StreamStart { resource, .. } if resource == "xfl-sound-2"
        )));

        // Advance through frame 2 to frame 3 -> StreamStop + Event loop_forever on bin_tone.
        player.tick(&project);
        let _ = player.drain_sound_commands();
        player.tick(&project);
        let cmds3 = player.drain_sound_commands();
        assert!(cmds3.iter().any(|c| matches!(
            c,
            SoundCommand::Play {
                resource,
                sync: SoundSync::Event,
                loop_forever: true,
                ..
            } if resource == "xfl-sound-3"
        )));

        // Advance to frame 4 -> Stop command on click.wav.
        player.tick(&project);
        let cmds4 = player.drain_sound_commands();
        assert!(cmds4.iter().any(|c| matches!(
            c,
            SoundCommand::Stop { resource } if resource == "xfl-sound-1"
        )));
    }
}

#[test]
fn xfl_rejects_broken_media_mismatched_button_types_and_invalid_sound_envelopes() {
    let assert_error = |files: BTreeMap<String, Vec<u8>>, code: &str| {
        let tmp = TempDir::new();
        write_directory_fixture(&tmp.0, &files);
        let report = inspect(&tmp.0).unwrap();
        assert!(
            !report.can_import,
            "unexpectedly accepted fixture for {code}"
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error && d.code == code),
            "expected error {code}; got {:?}",
            report.diagnostics
        );
        assert!(import_project(&tmp.0).is_err());
    };

    // 1. Missing external bitmap file.
    assert_error(
        BTreeMap::from([(
            "DOMDocument.xml".into(),
            br#"<DOMDocument><media><DOMBitmapItem name="gone.png" href="gone.png"/></media><timelines><DOMTimeline><layers><DOMLayer><frames><DOMFrame index="0"/></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#.to_vec(),
        )]),
        "xfl_media_invalid",
    );

    // 2. Unsafe path traversal on DOMBitmapItem href.
    assert_error(
        BTreeMap::from([(
            "DOMDocument.xml".into(),
            br#"<DOMDocument><media><DOMBitmapItem name="escape" href="../escape.png"/></media><timelines><DOMTimeline><layers><DOMLayer><frames><DOMFrame index="0"/></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#.to_vec(),
        )]),
        "unsafe_reference",
    );

    // 3. DOMBitmapInstance referencing undeclared bitmap.
    assert_error(
        BTreeMap::from([(
            "DOMDocument.xml".into(),
            br#"<DOMDocument><timelines><DOMTimeline><layers><DOMLayer><frames><DOMFrame index="0"><elements><DOMBitmapInstance libraryItemName="ghost.png"/></elements></DOMFrame></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#.to_vec(),
        )]),
        "missing_bitmap",
    );

    // 4. DOMSymbolInstance with invalid symbolType.
    assert_error(
        BTreeMap::from([
            (
                "LIBRARY/Clip.xml".into(),
                br#"<DOMSymbolItem name="Clip"><timeline><DOMTimeline><layers><DOMLayer><frames><DOMFrame index="0"/></frames></DOMLayer></layers></DOMTimeline></timeline></DOMSymbolItem>"#.to_vec(),
            ),
            (
                "DOMDocument.xml".into(),
                br#"<DOMDocument><symbols><Include href="Clip.xml"/></symbols><timelines><DOMTimeline><layers><DOMLayer><frames><DOMFrame index="0"><elements><DOMSymbolInstance libraryItemName="Clip" symbolType="unknown"/></elements></DOMFrame></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#.to_vec(),
            ),
        ]),
        "instance_type",
    );

    // 5. DOMFrame soundName referencing undeclared sound.
    assert_error(
        BTreeMap::from([(
            "DOMDocument.xml".into(),
            br#"<DOMDocument><timelines><DOMTimeline><layers><DOMLayer><frames><DOMFrame index="0" soundName="missing.wav"/></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#.to_vec(),
        )]),
        "missing_sound",
    );

    // 6. Unsorted SoundEnvelopePoint mark44.
    assert_error(
        BTreeMap::from([
            ("LIBRARY/tone.wav".into(), synthetic_wav_44k_stereo(4410)),
            (
                "DOMDocument.xml".into(),
                br#"<DOMDocument><media><DOMSoundItem name="tone.wav" href="tone.wav"/></media><timelines><DOMTimeline><layers><DOMLayer><frames><DOMFrame index="0" soundName="tone.wav" soundEffect="custom"><SoundEnvelope><SoundEnvelopePoint mark44="2000" level0="32768" level1="32768"/><SoundEnvelopePoint mark44="1000" level0="0" level1="0"/></SoundEnvelope></DOMFrame></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#.to_vec(),
            ),
        ]),
        "invalid_sound_envelope",
    );
}
