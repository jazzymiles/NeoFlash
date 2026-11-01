//! End-to-end tests for binary FLA audio/frame sounds, XFL timeline/tween/layer
//! expansion, and bounded SWF decompression & structural inspection.
//! All synthetic fixtures are generated inline; no private corpus data is used.

use neoflash_core::{
    Easing, ElementKind, InstancePlayback, LayerRole, PathCommand, Player, SoundCommand,
    SoundEffect, SoundSync, TweenKind, evaluate, flatten_scenes_with_ranges,
};
use neoflash_import::{ImportFormat, Severity, SwfCompression, import_project, inspect};
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "neoflash-import-batch-{}-{}",
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

#[test]
fn binary_fla_imports_pcm_wav_and_mp3_sounds_with_frame_sounds_and_warns_on_adpcm() {
    fn u16b(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    fn u32b(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    fn string(out: &mut Vec<u8>, s: &str) {
        out.push(s.len() as u8);
        out.extend_from_slice(s.as_bytes());
    }
    fn compact_utf16(out: &mut Vec<u8>, s: &str) {
        out.push(s.len() as u8);
        for b in s.bytes() {
            u16b(out, u16::from(b));
        }
    }
    fn class(out: &mut Vec<u8>, name: &str) {
        u16b(out, 0xffff);
        u16b(out, 1);
        u16b(out, name.len() as u16);
        out.extend_from_slice(name.as_bytes());
    }
    fn point_min(out: &mut Vec<u8>) {
        out.extend_from_slice(&i32::MIN.to_le_bytes());
        out.extend_from_slice(&i32::MIN.to_le_bytes());
    }
    fn empty_shape(out: &mut Vec<u8>) {
        out.extend_from_slice(&[1, 0, 0, 0]);
        point_min(out);
        out.push(2);
        for v in [65536_i32, 0, 0, 65536, 0, 0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.push(5);
        out.extend_from_slice(&[0; 9]);
        out.extend_from_slice(&[0; 4]);
    }
    fn sound_frame(
        out: &mut Vec<u8>,
        duration: u16,
        sound_media_id: u16,
        sound_sync: u8,
        loop_count: u16,
        sound_effect: u32,
    ) {
        empty_shape(out);
        out.push(21); // CPicFrame rev 21
        u16b(out, duration);
        u16b(out, 0); // key_mode
        u16b(out, 0); // acceleration
        u16b(out, sound_media_id);
        u16b(out, 0); // envelope count = 0
        u16b(out, loop_count);
        out.push(sound_sync);
        u32b(out, 0); // in_point
        u32b(out, 0); // out_point
        u16b(out, 0); // sound_zoom
        string(out, ""); // label
        u32b(out, 0); // actions rev = 0
        u32b(out, 1); // actions mode = 1 (source)
        string(out, ""); // empty actions source
        u32b(out, 0); // rotation_mode
        u32b(out, 0); // rotation_count
        u32b(out, 0); // label_type
        u16b(out, 0); // morph_shape = null
        u32b(out, 0); // shape_tween_blend
        u16b(out, 0); // sound_settings = null
        string(out, ""); // extension
        u32b(out, 0); // reserved
        u32b(out, sound_effect);
    }
    fn cmedia_sound(
        out: &mut Vec<u8>,
        stream_name: &str,
        item_name: &str,
        media_id: u16,
        sound_flags: u8,
        sample_count: u32,
        encoded_byte_count: u32,
    ) {
        // CMedia base rev 6
        out.push(6);
        compact_utf16(out, stream_name);
        string(out, item_name);
        u16b(out, media_id);
        string(out, ""); // source_path
        u32b(out, 0); // timestamp
        // metadata v=3
        u32b(out, 3);
        u32b(out, 0);
        u32b(out, 0);
        u32b(out, 0);
        u32b(out, 0); // base >= 5 reserved
        // CMediaSound sub rev 8
        out.push(8);
        u16b(out, u16::from(sound_flags)); // format_flags
        u32b(out, sample_count); // sample_count
        out.extend_from_slice(&0_i16.to_le_bytes()); // export_format
        out.extend_from_slice(&0_i16.to_le_bytes()); // export_bits
        u32b(out, encoded_byte_count); // encoded_byte_count
        out.push(0); // imported_mp3_quality
        u16b(out, 0); // secondary_format_flags
        u32b(out, 0); // secondary_sample_count
    }

    // Build Contents stream (Flash 8 = 0x3f):
    let mut contents = vec![0x3f];
    u32b(&mut contents, 1); // >= 0x20
    u32b(&mut contents, 0);
    contents.push(0);
    contents.push(0); // >= 0x15
    for _ in [0x21, 0x2b, 0x3b] {
        u32b(&mut contents, 0);
    }
    // Scene list: 1 scene ("Page 1")
    class(&mut contents, "CDocumentPage");
    contents.push(23); // CDocumentPage rev 23
    compact_utf16(&mut contents, "Page 1");
    string(&mut contents, "Scene 1");
    u32b(&mut contents, 7);
    contents.push(0);
    string(&mut contents, "");
    u32b(&mut contents, 0);
    // metadata v=6
    u32b(&mut contents, 6);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    // linkage v=2
    contents.push(2);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    string(&mut contents, "");
    contents.push(0);
    u32b(&mut contents, 1);
    string(&mut contents, "");
    string(&mut contents, "");
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    contents.push(0);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    string(&mut contents, "");
    contents.push(1);
    u32b(&mut contents, 0); // properties
    u32b(&mut contents, 0);
    // page linkage v=7
    contents.push(7);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    string(&mut contents, "");
    string(&mut contents, "");
    contents.push(0);
    u32b(&mut contents, 1);
    string(&mut contents, "");
    string(&mut contents, "");
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    contents.push(0);
    string(&mut contents, "");
    // component v=8
    contents.push(8);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    contents.extend_from_slice(&[0; 12]);
    string(&mut contents, "");
    for _ in 0..3 {
        contents.push(2);
        string(&mut contents, "");
        u32b(&mut contents, 0);
        u32b(&mut contents, 0);
    }
    string(&mut contents, "");
    contents.push(1);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    u32b(&mut contents, u32::MAX);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    contents.extend_from_slice(&[0; 8]);
    contents.push(3);
    string(&mut contents, "");
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    string(&mut contents, "");
    string(&mut contents, "");
    u32b(&mut contents, 0);
    string(&mut contents, "");
    contents.extend_from_slice(&[0; 20]);

    u16b(&mut contents, 0); // end scenes list
    u16b(&mut contents, 1); // next_scene_id
    u16b(&mut contents, 0); // next_item_tag
    u16b(&mut contents, 2); // selected_scene = object 2 (Page 1)
    u16b(&mut contents, 0); // end symbols list
    u16b(&mut contents, 0); // library_counter

    // Media list: 3 sounds (Media 1 = PCM, Media 2 = MP3, Media 3 = ADPCM)
    class(&mut contents, "CMediaSound"); // class index 3 (after CDocumentPage=1 + Page 1=2)
    // Media 1: 44.1 kHz (3<<2=0x0c), 16-bit (0x02), mono (0x00), codec 0 -> flags 0x0e, 4 samples (8 bytes)
    cmedia_sound(&mut contents, "Media 1", "Click.wav", 1, 0x0e, 4, 8);
    // Next media uses class ref 0x8003:
    u16b(&mut contents, 0x8003);
    // Media 2: codec 2 (MP3 -> 0x20) | 44.1kHz (0x0c) | 16-bit (0x02) -> 0x2e, 8 encoded bytes
    cmedia_sound(&mut contents, "Media 2", "Theme.mp3", 2, 0x2e, 576, 8);
    u16b(&mut contents, 0x8003);
    // Media 3: codec 1 (ADPCM -> 0x10) | 22.05kHz (0x08) | 16-bit (0x02) -> 0x1a, 4 encoded bytes
    cmedia_sound(&mut contents, "Media 3", "VoiceAdpcm.wav", 3, 0x1a, 32, 4);
    u16b(&mut contents, 0); // end media list
    u16b(&mut contents, 3); // media_counter

    // Document settings:
    u16b(&mut contents, 2);
    contents.push(0);
    for word in [0_i32, 11000, 0, 8000] {
        contents.extend_from_slice(&word.to_le_bytes());
    }
    u32b(&mut contents, 20 * 65536);
    contents.extend_from_slice(&[0; 5]);
    contents.extend_from_slice(&[0; 16]);
    contents.extend_from_slice(&[0; 4]);
    u32b(&mut contents, 65536);
    u16b(&mut contents, 0);
    u16b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0xffffffff);
    u32b(&mut contents, 24 * 65536);
    contents.extend_from_slice(&[0; 3]);
    u32b(&mut contents, 20);

    // Build Page 1 stream with 3 keyframes on 1 layer:
    // Frame 0 (dur 2): Media 1 (PCM), Stream (3), loops=1, FadeIn (4)
    // Frame 2 (dur 2): Media 2 (MP3), Start (1), loop_forever (0x7fff), None (0)
    // Frame 4 (dur 1): Media 3 (ADPCM), Event (0), loops=2, None (0)
    let mut page = vec![1];
    class(&mut page, "CPicPage"); // 1
    page.extend_from_slice(&[1, 0]);
    class(&mut page, "CPicLayer"); // 3
    page.extend_from_slice(&[1, 0]);
    class(&mut page, "CPicFrame"); // 5
    sound_frame(&mut page, 2, 1, 3, 1, 5);
    u16b(&mut page, 0x8005);
    sound_frame(&mut page, 2, 2, 1, 0x7fff, 0);
    u16b(&mut page, 0x8005);
    sound_frame(&mut page, 1, 3, 0, 2, 0);
    u16b(&mut page, 0); // end layer children
    point_min(&mut page);
    page.push(8); // CPicLayer rev 8
    string(&mut page, "Audio");
    page.extend_from_slice(&[0; 3]);
    page.extend_from_slice(&[0; 16]);
    page.push(0);
    page.extend_from_slice(&[0; 5]);
    u16b(&mut page, 0); // end page children
    point_min(&mut page);
    page.push(3);
    u16b(&mut page, 1);
    page.extend_from_slice(&[0; 4]);

    let pcm_bytes = [0x00_u8, 0x10, 0x00, 0x20, 0x00, 0xf0, 0x00, 0xe0];
    let mp3_bytes = [0xff_u8, 0xfb, 0x90, 0x64, 0x00, 0x00, 0x00, 0x00];
    let adpcm_bytes = [0xaa_u8, 0xbb, 0xcc, 0xdd];

    let mut compound = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    compound
        .create_stream("Contents")
        .unwrap()
        .write_all(&contents)
        .unwrap();
    compound
        .create_stream("Page 1")
        .unwrap()
        .write_all(&page)
        .unwrap();
    compound
        .create_stream("Media 1")
        .unwrap()
        .write_all(&pcm_bytes)
        .unwrap();
    compound
        .create_stream("Media 2")
        .unwrap()
        .write_all(&mp3_bytes)
        .unwrap();
    compound
        .create_stream("Media 3")
        .unwrap()
        .write_all(&adpcm_bytes)
        .unwrap();

    let temp = TempDir::new();
    let fla_path = temp.0.join("audio_test.fla");
    fs::write(&fla_path, compound.into_inner().into_inner()).unwrap();

    let report = inspect(&fla_path).unwrap();
    assert_eq!(report.format, ImportFormat::LegacyBinaryFla);
    assert!(report.can_import, "{:?}", report.diagnostics);
    // Only Media 3 (ADPCM) should emit fla_media_not_imported and fla_frame_sound:
    let media_warns: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "fla_media_not_imported")
        .collect();
    assert_eq!(media_warns.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(media_warns[0].location, "media 3");
    assert!(media_warns[0].message.contains("ADPCM"));
    let frame_sound_warns: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "fla_frame_sound")
        .collect();
    assert_eq!(frame_sound_warns.len(), 1, "{:?}", report.diagnostics);

    let project = import_project(&fla_path).unwrap();
    project.validate().unwrap();
    assert_eq!(project.format_version, 6);

    let wav_res = project
        .resources
        .iter()
        .find(|r| r.id == "legacy-sound-1")
        .unwrap();
    assert_eq!(wav_res.media_type, "audio/wav");
    assert!(wav_res.path.ends_with("Click.wav"));
    let wav_decoded = wav_res.decoded_bytes().unwrap();
    assert_eq!(&wav_decoded[0..4], b"RIFF");
    assert_eq!(&wav_decoded[44..], &pcm_bytes);

    let mp3_res = project
        .resources
        .iter()
        .find(|r| r.id == "legacy-sound-2")
        .unwrap();
    assert_eq!(mp3_res.media_type, "audio/mpeg");
    assert!(mp3_res.path.ends_with("Theme.mp3"));
    assert_eq!(mp3_res.decoded_bytes().unwrap(), mp3_bytes);

    let root = project.symbol(&project.root).unwrap();
    let kfs = &root.timeline.layers[0].keyframes;
    assert_eq!(kfs.len(), 3);
    let s0 = kfs[0].sound.as_ref().unwrap();
    assert_eq!(s0.resource, "legacy-sound-1");
    assert_eq!(s0.sync, SoundSync::Stream);
    assert_eq!(s0.effect, SoundEffect::FadeIn);
    let s1 = kfs[1].sound.as_ref().unwrap();
    assert_eq!(s1.resource, "legacy-sound-2");
    assert_eq!(s1.sync, SoundSync::Start);
    assert!(s1.loop_forever);
    assert!(kfs[2].sound.is_none());

    // Runtime Player schedules imported frame sounds cleanly:
    let mut player = Player::new(&project);
    let initial_cmds = player.drain_sound_commands();
    assert!(
        initial_cmds.iter().any(|cmd| matches!(
            cmd,
            SoundCommand::StreamStart { resource, .. } if resource == "legacy-sound-1"
        )),
        "{initial_cmds:?}"
    );
}

#[test]
fn xfl_imports_multi_scenes_tweens_guides_masks_folders_skew_and_graphic_offsets() {
    let temp = TempDir::new();
    fs::create_dir_all(temp.0.join("LIBRARY/UI")).unwrap();

    fs::write(
        temp.0.join("LIBRARY/UI/Orb.xml"),
        r##"<DOMSymbolItem name="UI/Orb" symbolType="graphic">
  <timeline>
    <DOMTimeline name="Orb">
      <layers>
        <DOMLayer name="Art">
          <frames>
            <DOMFrame index="0" duration="2">
              <elements>
                <DOMOvalObject objectWidth="20" objectHeight="20"/>
              </elements>
            </DOMFrame>
            <DOMFrame index="2" duration="2">
              <elements>
                <DOMRectangleObject objectWidth="20" objectHeight="20"/>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timeline>
</DOMSymbolItem>"##,
    )
    .unwrap();

    fs::write(
        temp.0.join("DOMDocument.xml"),
        r##"<DOMDocument width="400" height="300" frameRate="24">
  <folders>
    <DOMFolderItem name="UI"/>
    <DOMFolderItem name="UI/Unused"/>
  </folders>
  <symbols>
    <Include href="UI/Orb.xml"/>
  </symbols>
  <timelines>
    <DOMTimeline name="Intro">
      <layers>
        <DOMLayer name="PathGuide" layerType="guide">
          <frames>
            <DOMFrame index="0" duration="5">
              <elements>
                <DOMShape>
                  <strokes>
                    <StrokeStyle index="1">
                      <SolidStroke scaleMode="normal" weight="1">
                        <fill><SolidColor color="#00FF00"/></fill>
                      </SolidStroke>
                    </StrokeStyle>
                  </strokes>
                  <edges>
                    <Edge strokeStyle="1" edges="!0 0[2000 -2000 4000 0"/>
                  </edges>
                </DOMShape>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
        <DOMLayer name="GuidedOrb" parentLayerIndex="0">
          <frames>
            <DOMFrame index="0" duration="4" tweenType="motion" acceleration="-50" motionTweenOrientToPath="true">
              <elements>
                <DOMSymbolInstance libraryItemName="UI/Orb" symbolType="graphic" loop="loop" firstFrame="2">
                  <matrix><Matrix tx="0" ty="0"/></matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
            <DOMFrame index="4" duration="1">
              <elements>
                <DOMSymbolInstance libraryItemName="UI/Orb" symbolType="graphic" loop="play once" firstFrame="1">
                  <matrix><Matrix a="1" b="0.2" c="0.3" d="1" tx="200" ty="0"/></matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
        <DOMLayer name="Group" layerType="folder" open="false"/>
        <DOMLayer name="Masker" layerType="mask" parentLayerIndex="2">
          <frames>
            <DOMFrame index="0" duration="5">
              <elements>
                <DOMRectangleObject objectWidth="300" objectHeight="200"/>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
        <DOMLayer name="MaskedMorph" parentLayerIndex="3">
          <frames>
            <DOMFrame index="0" duration="4" tweenType="shape">
              <tweens>
                <CustomEase target="all">
                  <Point x="0" y="0"/>
                  <Point x="0.25" y="0.1"/>
                  <Point x="0.75" y="0.9"/>
                  <Point x="1" y="1"/>
                </CustomEase>
              </tweens>
              <elements>
                <DOMRectangleObject objectWidth="40" objectHeight="40"/>
              </elements>
            </DOMFrame>
            <DOMFrame index="4" duration="1">
              <elements>
                <DOMOvalObject objectWidth="80" objectHeight="80"/>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
    <DOMTimeline name="Ending">
      <layers>
        <DOMLayer name="Outro">
          <frames>
            <DOMFrame index="0" duration="3">
              <elements>
                <DOMRectangleObject objectWidth="50" objectHeight="50"/>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timelines>
</DOMDocument>"##,
    )
    .unwrap();

    let report = inspect(&temp.0).unwrap();
    assert!(report.can_import, "{:?}", report.diagnostics);
    let project = import_project(&temp.0).unwrap();
    project.validate().unwrap();
    assert_eq!(project.format_version, 6);

    // Library folders & symbol folder assignment:
    assert_eq!(project.library_folders, ["UI", "UI/Unused"]);
    let orb = project.symbols.iter().find(|s| s.name == "Orb").unwrap();
    assert_eq!(orb.folder, "UI");

    // Multi-scene timelines:
    assert_eq!(project.scenes.len(), 2);
    assert_eq!(project.scenes[0], project.root);
    let (flattened, ranges) = flatten_scenes_with_ranges(&project);
    assert_eq!(ranges.len(), 2);
    assert_eq!(
        (ranges[0].name.as_str(), ranges[0].start, ranges[0].frames),
        ("Intro", 0, 5)
    );
    assert_eq!(
        (ranges[1].name.as_str(), ranges[1].start, ranges[1].frames),
        ("Ending", 5, 3)
    );
    assert_eq!(
        flattened
            .symbol(&flattened.root)
            .unwrap()
            .timeline
            .frame_count,
        8
    );

    // Layer roles, hierarchy, tweens, guide path, and continuous classic-tween IDs:
    let intro = project.symbol(&project.root).unwrap();
    // Bottom-to-top reverse order of XFL layers:
    // 0: MaskedMorph, 1: Masker, 2: Group, 3: GuidedOrb, 4: PathGuide
    let layers = &intro.timeline.layers;
    assert_eq!(layers.len(), 5);
    let morph_layer = layers.iter().find(|l| l.name == "MaskedMorph").unwrap();
    let mask_layer = layers.iter().find(|l| l.name == "Masker").unwrap();
    let folder_layer = layers.iter().find(|l| l.name == "Group").unwrap();
    let guided_layer = layers.iter().find(|l| l.name == "GuidedOrb").unwrap();
    let guide_layer = layers.iter().find(|l| l.name == "PathGuide").unwrap();

    assert_eq!(guide_layer.properties.role, LayerRole::Guide);
    let ElementKind::Path {
        commands,
        fill,
        stroke_width,
        ..
    } = &guide_layer.keyframes[0].elements[0].kind
    else {
        panic!("expected stroke-only guide path")
    };
    assert_eq!(*fill, [0, 0, 0, 0]);
    assert_eq!(*stroke_width, 1.0);
    assert!(matches!(commands[1], PathCommand::QuadraticTo { .. }));

    assert_eq!(
        guided_layer.properties.motion_guide.as_deref(),
        Some(guide_layer.id.as_str())
    );
    assert_eq!(guided_layer.keyframes[0].tween_kind, TweenKind::Classic);
    assert_eq!(
        guided_layer.keyframes[0].tween,
        Some(Easing::Flash {
            acceleration: -50.0
        })
    );
    assert!(guided_layer.keyframes[0].orient_to_path);
    // Element ID is preserved across the classic tween span:
    assert_eq!(
        guided_layer.keyframes[0].elements[0].id,
        guided_layer.keyframes[1].elements[0].id
    );
    // Graphic firstFrame offsets & skew on end keyframe:
    assert!(matches!(
        guided_layer.keyframes[0].elements[0].kind,
        ElementKind::Instance {
            playback: InstancePlayback::LoopFrom(2),
            ..
        }
    ));
    assert!(matches!(
        guided_layer.keyframes[1].elements[0].kind,
        ElementKind::Instance {
            playback: InstancePlayback::PlayOnceFrom(1),
            ..
        }
    ));
    assert!(guided_layer.keyframes[1].elements[0].transform.skew.abs() > 0.1);

    assert_eq!(folder_layer.properties.role, LayerRole::Folder);
    assert!(folder_layer.properties.collapsed);
    assert_eq!(mask_layer.properties.role, LayerRole::Mask);
    assert_eq!(
        mask_layer.properties.parent.as_deref(),
        Some(folder_layer.id.as_str())
    );
    assert_eq!(
        morph_layer.properties.mask.as_deref(),
        Some(mask_layer.id.as_str())
    );
    assert_eq!(morph_layer.keyframes[0].tween_kind, TweenKind::Shape);
    assert!(matches!(
        morph_layer.keyframes[0].tween,
        Some(Easing::CubicBezier { .. })
    ));

    // Midpoint evaluation follows the curved motion guide (y < 0 at midpoint of arc !0 0[2000 -2000 4000 0):
    let mid_items = evaluate(&project, &project.root, 2);
    assert!(
        mid_items
            .iter()
            .any(|item| item.transform[4] > 10.0 && item.transform[5] < -10.0),
        "guided instance did not follow quadratic guide arc: {mid_items:?}"
    );

    // Explicit blocking of CS4+ <MotionObjectXML> / tweenType="motion object":
    let blocked_dir = TempDir::new();
    fs::write(
        blocked_dir.0.join("DOMDocument.xml"),
        r#"<DOMDocument><timelines><DOMTimeline><layers><DOMLayer><frames><DOMFrame index="0" duration="4" tweenType="motion object"><MotionObjectXML/></DOMFrame></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#,
    )
    .unwrap();
    let blocked_report = inspect(&blocked_dir.0).unwrap();
    assert!(!blocked_report.can_import);
    assert!(
        blocked_report
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.code == "motion_object_not_supported"),
        "{:?}",
        blocked_report.diagnostics
    );
}

#[test]
fn swf_inspects_fws_cws_zws_and_rejects_truncated_or_bomb_containers() {
    let corpus =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fla-corpus/upstream/ruffle");

    // 1. Companion AVM1 CWS SWF with nested sprite DoAction scripts:
    let avm1_path = corpus.join("avm1-frame-labels/test.swf");
    let avm1 = inspect(&avm1_path).unwrap();
    assert_eq!(avm1.format, ImportFormat::Swf);
    assert!(!avm1.can_import);
    assert!(
        avm1.diagnostics
            .iter()
            .any(|d| d.code == "swf_not_implemented")
    );
    assert!(import_project(&avm1_path).is_err());
    let avm1_swf = avm1.swf.as_ref().unwrap();
    assert_eq!(avm1_swf.compression, SwfCompression::Zlib);
    assert_eq!(
        (avm1_swf.width_px, avm1_swf.height_px),
        (Some(550.0), Some(400.0))
    );
    assert_eq!(avm1_swf.frame_rate, Some(24.0));
    assert_eq!(avm1_swf.frame_count, Some(7));
    assert!(!avm1_swf.as3_flag);
    assert!(avm1_swf.has_do_action);
    assert!(!avm1_swf.has_do_abc);
    assert!(avm1_swf.end_tag_reached);
    assert_eq!(avm1_swf.tags.do_action, 9);
    assert_eq!(avm1_swf.tags.define_sprite, 1);

    // 2. Companion AVM2 CWS SWF with FileAttributes AS3 flag and DoABC2:
    let avm2_path = corpus.join("avm2-nested-construction/test.swf");
    let avm2 = inspect(&avm2_path).unwrap();
    assert!(!avm2.can_import);
    let avm2_swf = avm2.swf.as_ref().unwrap();
    assert_eq!(avm2_swf.compression, SwfCompression::Zlib);
    assert!(avm2_swf.as3_flag);
    assert!(avm2_swf.has_do_abc);
    assert!(!avm2_swf.has_do_action);
    assert!(avm2_swf.end_tag_reached);
    assert!(avm2_swf.tags.define_sprite >= 5);
    assert!(avm2_swf.tags.buttons >= 1);

    // 3. Companion MX FWS SWFs (button, morph shape, static text):
    for (rel, expect_buttons, expect_shapes, expect_texts, expect_frames) in [
        (
            "mx-button/DefineButton-MX.swf",
            1usize,
            2usize,
            0usize,
            1u16,
        ),
        ("mx-shape-tween/DefineMorphShape-MX.swf", 0, 2, 0, 30),
        ("mx-static-text/DefineText2-MX.swf", 0, 0, 1, 1),
    ] {
        let rep = inspect(&corpus.join(rel)).unwrap();
        assert!(!rep.can_import);
        let swf = rep.swf.unwrap();
        assert_eq!(swf.compression, SwfCompression::Uncompressed);
        assert_eq!((swf.width_px, swf.height_px), (Some(550.0), Some(400.0)));
        assert_eq!(swf.frame_rate, Some(12.0));
        assert_eq!(swf.frame_count, Some(expect_frames));
        assert_eq!(swf.tags.buttons, expect_buttons);
        assert_eq!(swf.tags.shapes, expect_shapes);
        assert_eq!(swf.tags.texts, expect_texts);
        assert!(swf.end_tag_reached);
    }

    // 4. Valid & truncated ZWS headers:
    let temp = TempDir::new();
    let zws_path = temp.0.join("lzma.swf");
    let mut zws = Vec::from(*b"ZWS\x0d");
    zws.extend_from_slice(&128_u32.to_le_bytes()); // declared uncompressed length
    zws.extend_from_slice(&4_u32.to_le_bytes()); // compressed payload length
    zws.extend_from_slice(&[0x5d, 0, 0, 0x10, 0]); // 5-byte LZMA properties
    zws.extend_from_slice(&[1, 2, 3, 4]); // 4 compressed bytes
    fs::write(&zws_path, &zws).unwrap();
    let zws_rep = inspect(&zws_path).unwrap();
    assert!(!zws_rep.can_import);
    assert!(
        zws_rep
            .diagnostics
            .iter()
            .any(|d| d.code == "swf_lzma_unsupported")
    );
    assert_eq!(zws_rep.swf.unwrap().compression, SwfCompression::Lzma);

    fs::write(&zws_path, &zws[..12]).unwrap();
    assert!(inspect(&zws_path).is_err());

    // 5. Truncated CWS, length-mismatch CWS, trailing-byte CWS, and >64 MiB CWS bomb rejection:
    let cws_path = temp.0.join("bad_cws.swf");
    let body = [
        0x00_u8, // RECT nbits=0 (1 byte)
        0x00, 0x18, // 24.0 fps
        0x01, 0x00, // 1 frame
        0x40, 0x00, // ShowFrame (tag 1, len 0)
        0x00, 0x00, // End (tag 0, len 0)
    ];
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&body).unwrap();
    let compressed = encoder.finish().unwrap();

    // Valid synthetic CWS succeeds:
    let mut valid_cws = Vec::from(*b"CWS\x0a");
    valid_cws.extend_from_slice(&((8 + body.len()) as u32).to_le_bytes());
    valid_cws.extend_from_slice(&compressed);
    fs::write(&cws_path, &valid_cws).unwrap();
    let ok_rep = inspect(&cws_path).unwrap();
    assert!(ok_rep.swf.unwrap().end_tag_reached);

    // Truncated compressed stream -> Err:
    fs::write(&cws_path, &valid_cws[..valid_cws.len() - 3]).unwrap();
    assert!(inspect(&cws_path).is_err());

    // Trailing garbage after zlib stream -> Err:
    let mut trailing = valid_cws.clone();
    trailing.push(0x99);
    fs::write(&cws_path, &trailing).unwrap();
    assert!(inspect(&cws_path).is_err());

    // Declared length smaller than actual decompressed output (mini bomb) -> Err:
    let mut under_declared = Vec::from(*b"CWS\x0a");
    under_declared.extend_from_slice(&10_u32.to_le_bytes());
    under_declared.extend_from_slice(&compressed);
    fs::write(&cws_path, &under_declared).unwrap();
    assert!(inspect(&cws_path).is_err());

    // Declared length > 64 MiB -> Err immediately without decompressing:
    let mut over_64m = Vec::from(*b"CWS\x0a");
    over_64m.extend_from_slice(&((64 * 1024 * 1024 + 16) as u32).to_le_bytes());
    over_64m.extend_from_slice(&compressed);
    fs::write(&cws_path, &over_64m).unwrap();
    assert!(inspect(&cws_path).is_err());
}
