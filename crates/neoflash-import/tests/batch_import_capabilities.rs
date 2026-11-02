//! End-to-end tests for binary FLA audio/frame sounds, XFL timeline/tween/layer
//! expansion, and bounded SWF decompression & structural inspection.
//! All synthetic fixtures are generated inline; no private corpus data is used.

use neoflash_core::{
    Easing, ElementKind, InstancePlayback, LayerRole, PathCommand, Player, ScriptLanguage,
    SoundCommand, SoundEffect, SoundSync, SymbolKind, TweenKind, evaluate,
    flatten_scenes_with_ranges,
};
use neoflash_import::{
    ImportFormat, Severity, SwfCompression, export_fla, import_project, inspect,
};
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
    // Media 3: codec 1 (ADPCM -> 0x10) | 22.05kHz (0x08) | 16-bit (0x02) -> 0x1a, 4 samples (5 encoded bytes)
    cmedia_sound(&mut contents, "Media 3", "VoiceAdpcm.wav", 3, 0x1a, 4, 5);
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
    // Frame 0 (dur 2): Media 1 (PCM), Stream (3), loops=1, FadeIn (5)
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
    // 4-bit mono ADPCM (code_size_index=2): initial=1000, step_index=10, codes=[0x4, 0xa, 0x1]
    let adpcm_bytes = [0x80_u8, 0xfa, 0x0a, 0x4a, 0x10];

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
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.code != "fla_media_not_imported" && d.code != "fla_frame_sound"),
        "{:?}",
        report.diagnostics
    );

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

    let adpcm_res = project
        .resources
        .iter()
        .find(|r| r.id == "legacy-sound-3")
        .unwrap();
    assert_eq!(adpcm_res.media_type, "audio/wav");
    assert!(adpcm_res.path.ends_with("VoiceAdpcm.wav"));
    let adpcm_decoded = adpcm_res.decoded_bytes().unwrap();
    assert_eq!(&adpcm_decoded[0..4], b"RIFF");
    assert_eq!(
        u32::from_le_bytes(adpcm_decoded[24..28].try_into().unwrap()),
        22_050
    );
    let adpcm_samples: Vec<i16> = adpcm_decoded[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .map(i16::from_le_bytes)
        .collect();
    assert_eq!(adpcm_samples, vec![1000, 1021, 1008, 1015]);

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
    let s2 = kfs[2].sound.as_ref().unwrap();
    assert_eq!(s2.resource, "legacy-sound-3");
    assert_eq!(s2.sync, SoundSync::Event);
    assert_eq!(s2.loops, 2);
    assert!(!s2.loop_forever);
    assert_eq!(s2.effect, SoundEffect::None);

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

    // CS4+ <MotionObjectXML> / tweenType="motion object" imports as approximated keyframe pose with warning:
    let motion_dir = TempDir::new();
    fs::write(
        motion_dir.0.join("DOMDocument.xml"),
        r#"<DOMDocument><timelines><DOMTimeline><layers><DOMLayer><frames><DOMFrame index="0" duration="4" tweenType="motion object"><MotionObjectXML/></DOMFrame></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#,
    )
    .unwrap();
    let motion_report = inspect(&motion_dir.0).unwrap();
    assert!(motion_report.can_import, "{:?}", motion_report.diagnostics);
    assert!(
        motion_report
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning && d.code == "motion_object_not_supported"),
        "{:?}",
        motion_report.diagnostics
    );
}

#[test]
fn swf_inspects_fws_cws_zws_and_rejects_truncated_or_bomb_containers() {
    let corpus =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fla-corpus/upstream/ruffle");
    let temp = TempDir::new();

    // 1. Companion AVM1 CWS SWF with nested sprite DoAction scripts:
    let avm1_path = corpus.join("avm1-frame-labels/test.swf");
    let avm1 = inspect(&avm1_path).unwrap();
    assert_eq!(avm1.format, ImportFormat::Swf);
    assert!(avm1.can_import, "{:?}", avm1.diagnostics);
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

    let avm1_proj = import_project(&avm1_path).unwrap();
    avm1_proj.validate().unwrap();
    let all_avm1_scripts: String = avm1_proj
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.scripts)
        .map(|fs| {
            assert_eq!(fs.script.language, ScriptLanguage::ActionScript2);
            fs.script.source.as_str()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        all_avm1_scripts.contains("trace("),
        "expected decompiled trace() in AVM1 frame scripts:\n{all_avm1_scripts}"
    );
    assert!(
        all_avm1_scripts.contains("gotoAnd"),
        "expected decompiled gotoAndPlay/gotoAndStop in AVM1 frame scripts:\n{all_avm1_scripts}"
    );
    let avm1_fla = temp.0.join("avm1_labels.fla");
    export_fla(&avm1_proj, &avm1_fla).unwrap();
    let avm1_roundtrip = import_project(&avm1_fla).unwrap();
    avm1_roundtrip.validate().unwrap();

    // 1b. Companion AVM1 clip-events and nested button/text SWFs:
    let clip_ev_path = corpus.join("avm1-clip-events/test.swf");
    assert!(inspect(&clip_ev_path).unwrap().can_import);
    let clip_ev_proj = import_project(&clip_ev_path).unwrap();
    clip_ev_proj.validate().unwrap();
    let has_clip_obj_scripts = clip_ev_proj
        .symbols
        .iter()
        .flat_map(|s| &s.timeline.layers)
        .flat_map(|l| &l.keyframes)
        .flat_map(|k| &k.elements)
        .any(|el| !el.scripts.is_empty());
    assert!(
        has_clip_obj_scripts,
        "expected PlaceObject2 CLIPACTIONS recovered as instance ObjectScripts"
    );
    let clip_ev_fla = temp.0.join("avm1_clip_events.fla");
    export_fla(&clip_ev_proj, &clip_ev_fla).unwrap();

    let btn_txt_path = corpus.join("avm1-nested-button-text/test.swf");
    let btn_txt_proj = import_project(&btn_txt_path).unwrap();
    btn_txt_proj.validate().unwrap();
    assert!(
        btn_txt_proj
            .symbols
            .iter()
            .any(|s| s.kind == SymbolKind::Button)
    );
    export_fla(&btn_txt_proj, &temp.0.join("avm1_btn_txt.fla")).unwrap();

    // 2. Companion AVM2 CWS SWFs with FileAttributes AS3 flag and DoABC2:
    let avm2_path = corpus.join("avm2-nested-construction/test.swf");
    let avm2 = inspect(&avm2_path).unwrap();
    assert!(avm2.can_import, "{:?}", avm2.diagnostics);
    let avm2_swf = avm2.swf.as_ref().unwrap();
    assert_eq!(avm2_swf.compression, SwfCompression::Zlib);
    assert!(avm2_swf.as3_flag);
    assert!(avm2_swf.has_do_abc);
    assert!(!avm2_swf.has_do_action);
    assert!(avm2_swf.end_tag_reached);
    assert!(avm2_swf.tags.define_sprite >= 5);
    assert!(avm2_swf.tags.buttons >= 1);

    let avm2_proj = import_project(&avm2_path).unwrap();
    avm2_proj.validate().unwrap();
    assert_eq!(avm2_proj.document_class, None);
    let container_src = avm2_proj
        .sources
        .iter()
        .find(|s| s.language == ScriptLanguage::ActionScript3 && s.path.ends_with("Container.as"))
        .unwrap_or_else(|| {
            panic!(
                "expected decompiled Container.as in sources: {:?}",
                avm2_proj
                    .sources
                    .iter()
                    .map(|s| &s.path)
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        container_src.source.contains("class Container")
            && container_src.source.contains("Calling Container super()")
            && container_src
                .source
                .contains("Running Container framescript"),
        "unexpected decompiled Container.as:\n{}",
        container_src.source
    );
    assert!(
        avm2_proj
            .symbols
            .iter()
            .any(|s| s.class_name.as_deref() == Some("Container")),
        "expected SymbolClass binding for Container symbol"
    );
    let avm2_fla = temp.0.join("avm2_nested.fla");
    export_fla(&avm2_proj, &avm2_fla).unwrap();
    let avm2_zip = zip::ZipArchive::new(fs::File::open(&avm2_fla).unwrap()).unwrap();
    assert!(
        avm2_zip.file_names().any(|n| n.ends_with("Container.as")),
        "expected Container.as in exported .fla archive"
    );

    let avm2_btn_path = corpus.join("avm2-button-frame-scripts/test.swf");
    let avm2_btn_proj = import_project(&avm2_btn_path).unwrap();
    avm2_btn_proj.validate().unwrap();
    assert_eq!(avm2_btn_proj.document_class.as_deref(), Some("Main"));
    let main_src = avm2_btn_proj
        .sources
        .iter()
        .find(|s| s.language == ScriptLanguage::ActionScript3 && s.path.ends_with("Main.as"))
        .expect("expected decompiled Main.as in avm2-button-frame-scripts");
    assert!(
        main_src.source.contains("class Main")
            && main_src.source.contains("Main framescript 1")
            && main_src.source.contains("Main framescript 2")
            && main_src.source.contains("buttonHolder"),
        "unexpected decompiled Main.as:\n{}",
        main_src.source
    );
    export_fla(&avm2_btn_proj, &temp.0.join("avm2_btn_frames.fla")).unwrap();

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
        let swf_path = corpus.join(rel);
        let rep = inspect(&swf_path).unwrap();
        assert!(rep.can_import, "{rel}: {:?}", rep.diagnostics);
        let swf = rep.swf.unwrap();
        assert_eq!(swf.compression, SwfCompression::Uncompressed);
        assert_eq!((swf.width_px, swf.height_px), (Some(550.0), Some(400.0)));
        assert_eq!(swf.frame_rate, Some(12.0));
        assert_eq!(swf.frame_count, Some(expect_frames));
        assert_eq!(swf.tags.buttons, expect_buttons);
        assert_eq!(swf.tags.shapes, expect_shapes);
        assert_eq!(swf.tags.texts, expect_texts);
        assert!(swf.end_tag_reached);

        let proj = import_project(&swf_path).unwrap();
        proj.validate().unwrap();
        if expect_buttons > 0 {
            assert!(proj.symbols.iter().any(|s| s.kind == SymbolKind::Button));
        }
        if rel.contains("MorphShape") {
            let has_shape_tween = proj
                .symbols
                .iter()
                .flat_map(|s| &s.timeline.layers)
                .flat_map(|l| &l.keyframes)
                .any(|k| k.tween.is_some() && k.tween_kind == TweenKind::Shape);
            assert!(
                has_shape_tween,
                "expected shape tween symbol from DefineMorphShape"
            );
        }
        if expect_texts > 0 {
            let has_text = proj
                .symbols
                .iter()
                .flat_map(|s| &s.timeline.layers)
                .flat_map(|l| &l.keyframes)
                .flat_map(|k| &k.elements)
                .any(
                    |el| matches!(&el.kind, ElementKind::TextBox { text, .. } if !text.is_empty()),
                );
            assert!(has_text, "expected static TextBox from DefineText2");
        }
        let fla_out = temp.0.join(format!(
            "{}.fla",
            Path::new(rel).file_stem().unwrap().to_string_lossy()
        ));
        export_fla(&proj, &fla_out).unwrap();
        let reimported = import_project(&fla_out).unwrap();
        reimported.validate().unwrap();
    }

    // 4. Valid LZMA1 ZWS SWF, corrupt ZWS payload, and truncated ZWS header:
    let zws_path = temp.0.join("lzma.swf");
    let lzma_props = [93_u8, 0, 0, 128, 0];
    let lzma_payload = [
        0_u8, 0, 96, 94, 129, 15, 18, 253, 175, 223, 253, 199, 255, 255, 226, 51, 128, 0,
    ];
    let mut valid_zws = Vec::from(*b"ZWS\x0d");
    valid_zws.extend_from_slice(&17_u32.to_le_bytes()); // 8 + 9 uncompressed bytes
    valid_zws.extend_from_slice(&(lzma_payload.len() as u32).to_le_bytes());
    valid_zws.extend_from_slice(&lzma_props);
    valid_zws.extend_from_slice(&lzma_payload);
    fs::write(&zws_path, &valid_zws).unwrap();
    let valid_zws_rep = inspect(&zws_path).unwrap();
    assert!(valid_zws_rep.can_import, "{:?}", valid_zws_rep.diagnostics);
    assert_eq!(valid_zws_rep.swf.unwrap().compression, SwfCompression::Lzma);
    let zws_proj = import_project(&zws_path).unwrap();
    zws_proj.validate().unwrap();

    let mut bad_zws = Vec::from(*b"ZWS\x0d");
    bad_zws.extend_from_slice(&128_u32.to_le_bytes()); // declared uncompressed length
    bad_zws.extend_from_slice(&4_u32.to_le_bytes()); // compressed payload length
    bad_zws.extend_from_slice(&[0x5d, 0, 0, 0x10, 0]); // 5-byte LZMA properties
    bad_zws.extend_from_slice(&[1, 2, 3, 4]); // 4 corrupt compressed bytes
    fs::write(&zws_path, &bad_zws).unwrap();
    let zws_rep = inspect(&zws_path).unwrap();
    assert!(!zws_rep.can_import);
    assert!(
        zws_rep
            .diagnostics
            .iter()
            .any(|d| d.code == "swf_lzma_corrupt")
    );
    assert_eq!(zws_rep.swf.unwrap().compression, SwfCompression::Lzma);

    fs::write(&zws_path, &bad_zws[..12]).unwrap();
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

#[test]
fn binary_fla_exceeding_128_mib_json_sheds_largest_pcm_wav_and_validates() {
    use neoflash_core::MAX_PROJECT_JSON_BYTES;

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
    fn dense_quadratic_shape(out: &mut Vec<u8>, edge_count: usize) {
        out.extend_from_slice(&[1, 0, 0, 0]);
        point_min(out);
        out.push(2); // CPicShape rev 2
        for v in [65536_i32, 0, 0, 65536, 0, 0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.push(5); // data rev 5
        u32b(out, 0); // metadata
        u16b(out, 0); // 0 fills
        u16b(out, 1); // 1 stroke
        u32b(out, 0xff336699);
        u16b(out, 40);
        u16b(out, 0);
        u16b(out, 0);
        for i in 0..edge_count {
            if i == 0 {
                // Byte style indices [stroke=1, fill0=0, fill1=0] + i16 control/end deltas
                out.push(0xd4);
                out.extend_from_slice(&[1, 0, 0]);
            } else {
                // Inherited styles + zero start delta + i16 control/end deltas
                out.push(0x14);
            }
            for delta in [3_i16, -7, 11, 13] {
                out.extend_from_slice(&delta.to_le_bytes());
            }
        }
        out.push(0); // edge terminator
        u32b(out, 0); // 0 auxiliary curves
    }
    fn vector_and_sound_frame(out: &mut Vec<u8>, edge_count: usize, sound_media_id: u16) {
        dense_quadratic_shape(out, edge_count);
        out.push(21); // CPicFrame rev 21
        u16b(out, 1); // duration = 1
        u16b(out, 0); // key_mode
        u16b(out, 0); // acceleration
        u16b(out, sound_media_id);
        u16b(out, 0); // envelope count
        u16b(out, 1); // loop_count
        out.push(0); // Event sync
        u32b(out, 0); // in_point
        u32b(out, 0); // out_point
        u16b(out, 0); // sound_zoom
        string(out, "");
        u32b(out, 0);
        u32b(out, 1);
        string(out, "");
        u32b(out, 0);
        u32b(out, 0);
        u32b(out, 0);
        u16b(out, 0);
        u32b(out, 0);
        u16b(out, 0);
        string(out, "");
        u32b(out, 0);
        u32b(out, 0);
    }
    fn cmedia_sound(
        out: &mut Vec<u8>,
        stream_name: &str,
        item_name: &str,
        media_id: u16,
        sample_count: u32,
        encoded_byte_count: u32,
    ) {
        out.push(6);
        compact_utf16(out, stream_name);
        string(out, item_name);
        u16b(out, media_id);
        string(out, "");
        u32b(out, 0);
        u32b(out, 3);
        u32b(out, 0);
        u32b(out, 0);
        u32b(out, 0);
        u32b(out, 0);
        out.push(8);
        u16b(out, 0x0e); // 44.1 kHz, 16-bit, mono uncompressed PCM
        u32b(out, sample_count);
        out.extend_from_slice(&0_i16.to_le_bytes());
        out.extend_from_slice(&0_i16.to_le_bytes());
        u32b(out, encoded_byte_count);
        out.push(0);
        u16b(out, 0);
        u32b(out, 0);
    }

    let mut contents = vec![0x3f]; // Flash 8
    u32b(&mut contents, 1);
    u32b(&mut contents, 0);
    contents.push(0);
    contents.push(0);
    for _ in [0x21, 0x2b, 0x3b] {
        u32b(&mut contents, 0);
    }
    class(&mut contents, "CDocumentPage");
    contents.push(23);
    compact_utf16(&mut contents, "Page 1");
    string(&mut contents, "Scene 1");
    u32b(&mut contents, 7);
    contents.push(0);
    string(&mut contents, "");
    u32b(&mut contents, 0);
    u32b(&mut contents, 6);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
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
    u32b(&mut contents, 0);
    u32b(&mut contents, 0);
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

    u16b(&mut contents, 0); // end scenes
    u16b(&mut contents, 1);
    u16b(&mut contents, 0);
    u16b(&mut contents, 2); // selected_scene
    u16b(&mut contents, 0); // end symbols
    u16b(&mut contents, 0);

    // 4 PCM sounds totaling ~54.8 MB decoded (< 56 MiB resource media budget, ~73.1 MB base64):
    // Media 4 is largest (14.0 MB) and should be shed first when combined with ~66 MB vector JSON.
    let small_samples = 6_800_000_u32;
    let small_bytes = small_samples * 2;
    let large_samples = 7_000_000_u32;
    let large_bytes = large_samples * 2;
    class(&mut contents, "CMediaSound");
    cmedia_sound(
        &mut contents,
        "Media 1",
        "One.wav",
        1,
        small_samples,
        small_bytes,
    );
    u16b(&mut contents, 0x8003);
    cmedia_sound(
        &mut contents,
        "Media 2",
        "Two.wav",
        2,
        small_samples,
        small_bytes,
    );
    u16b(&mut contents, 0x8003);
    cmedia_sound(
        &mut contents,
        "Media 3",
        "Three.wav",
        3,
        small_samples,
        small_bytes,
    );
    u16b(&mut contents, 0x8003);
    cmedia_sound(
        &mut contents,
        "Media 4",
        "Largest.wav",
        4,
        large_samples,
        large_bytes,
    );
    u16b(&mut contents, 0); // end media
    u16b(&mut contents, 4);

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

    // 25 keyframes * 9,600 quadratic stroke edges = 240,000 QuadraticTo commands (~67 MB JSON).
    // Combined with ~73.1 MB base64 WAV media = ~140 MB > 128 MiB MAX_PROJECT_JSON_BYTES.
    let frame_count = 25usize;
    let edges_per_frame = 9_600usize;
    let mut page = vec![1];
    class(&mut page, "CPicPage"); // 1
    page.extend_from_slice(&[1, 0]);
    class(&mut page, "CPicLayer"); // 3
    page.extend_from_slice(&[1, 0]);
    class(&mut page, "CPicFrame"); // 5
    for idx in 0..frame_count {
        if idx > 0 {
            u16b(&mut page, 0x8005);
        }
        let sound_id = if idx < 4 { (idx + 1) as u16 } else { 0 };
        vector_and_sound_frame(&mut page, edges_per_frame, sound_id);
    }
    u16b(&mut page, 0); // end layer children
    point_min(&mut page);
    page.push(8);
    string(&mut page, "HeavyArtAndAudio");
    page.extend_from_slice(&[0; 3]);
    page.extend_from_slice(&[0; 16]);
    page.push(0);
    page.extend_from_slice(&[0; 5]);
    u16b(&mut page, 0); // end page children
    point_min(&mut page);
    page.push(3);
    u16b(&mut page, 1);
    page.extend_from_slice(&[0; 4]);

    let small_pcm = vec![0u8; small_bytes as usize];
    let large_pcm = vec![0u8; large_bytes as usize];
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
    for (name, payload) in [
        ("Media 1", &small_pcm),
        ("Media 2", &small_pcm),
        ("Media 3", &small_pcm),
        ("Media 4", &large_pcm),
    ] {
        compound
            .create_stream(name)
            .unwrap()
            .write_all(payload)
            .unwrap();
    }

    let temp = TempDir::new();
    let fla_path = temp.0.join("budget_128m.fla");
    fs::write(&fla_path, compound.into_inner().into_inner()).unwrap();

    let report = inspect(&fla_path).unwrap();
    assert!(report.can_import, "{:?}", report.diagnostics);
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error),
        "{:?}",
        report.diagnostics
    );
    assert!(
        report.diagnostics.iter().any(|d| {
            d.code == "fla_media_not_imported"
                && d.location == "media 4"
                && d.message.contains("128 MiB")
        }),
        "{:?}",
        report.diagnostics
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| { d.code == "fla_frame_sound" && d.message.contains("media #4") }),
        "{:?}",
        report.diagnostics
    );

    let project = import_project(&fla_path).unwrap();
    project.validate().unwrap();
    let json = project.to_json().unwrap();
    assert!(
        json.len() <= MAX_PROJECT_JSON_BYTES,
        "encoded JSON {} exceeded {}",
        json.len(),
        MAX_PROJECT_JSON_BYTES
    );

    // Largest sound (legacy-sound-4) was shed, while sounds 1..=3 and all 25 vector frames remain:
    assert!(project.resources.iter().all(|r| r.id != "legacy-sound-4"));
    for kept in ["legacy-sound-1", "legacy-sound-2", "legacy-sound-3"] {
        assert!(
            project.resources.iter().any(|r| r.id == kept),
            "expected {kept} to remain imported"
        );
    }
    let root = project.symbol(&project.root).unwrap();
    let kfs = &root.timeline.layers[0].keyframes;
    assert_eq!(kfs.len(), frame_count);
    assert_eq!(kfs[0].sound.as_ref().unwrap().resource, "legacy-sound-1");
    assert_eq!(kfs[1].sound.as_ref().unwrap().resource, "legacy-sound-2");
    assert_eq!(kfs[2].sound.as_ref().unwrap().resource, "legacy-sound-3");
    assert!(kfs[3].sound.is_none());
    let total_quads: usize = kfs
        .iter()
        .flat_map(|k| &k.elements)
        .map(|e| match &e.kind {
            ElementKind::PaintedPath { commands, .. } => commands
                .iter()
                .filter(|c| matches!(c, PathCommand::QuadraticTo { .. }))
                .count(),
            _ => 0,
        })
        .sum();
    assert_eq!(total_quads, frame_count * edges_per_frame);
}

#[test]
fn public_corpus_all_21_cases_and_real_world_xfl_features_import_cleanly() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fla-corpus");
    for rel in [
        "upstream/ruffle/avm1-clip-events/test.fla",
        "upstream/ruffle/avm2-button-frame-scripts/test.fla",
        "upstream/ruffle/avm2-nested-construction/test.fla",
        "upstream/flump/rotate_test2/rotate_test2.fla",
        "upstream/flump/potion_simple/potion_simple.fla",
        "upstream/flump/flump/flump.fla",
    ] {
        let path = corpus.join(rel);
        let report = inspect(&path).unwrap();
        assert!(report.can_import, "{rel}: {:?}", report.diagnostics);
        let project = import_project(&path).unwrap();
        project.validate().unwrap();
    }

    // Real-world XFL CS5/CS6/Animate CC feature coverage (DOMDynamicText, DOMInputText,
    // DOMTLFText, DOMGroup, DOMVideoItem/DOMVideoInstance, DOMCompiledClipItem/Instance,
    // DOMComponentItem/Instance, DOMFontItem, filters, color tint, persistentData <PD/>,
    // __Camera__, and bundled .as source files).
    let temp = TempDir::new();
    fs::create_dir_all(temp.0.join("LIBRARY/Assets")).unwrap();
    fs::create_dir_all(temp.0.join("com/example")).unwrap();
    fs::write(
        temp.0.join("PublishSettings.xml"),
        r#"<flash_profiles><flash_profile version="1.0" name="Default" current="true"><PublishFlashProperties enabled="true"><ActionScriptVersion>3</ActionScriptVersion><DocumentClass>com.example.AppMain</DocumentClass><AS3PackagePaths>.</AS3PackagePaths></PublishFlashProperties></flash_profile></flash_profiles>"#,
    )
    .unwrap();
    fs::write(
        temp.0.join("com/example/AppMain.as"),
        "package com.example { import flash.display.MovieClip; public class AppMain extends MovieClip { public function AppMain() {} } }\n",
    )
    .unwrap();
    fs::write(
        temp.0.join("LIBRARY/Assets/Widget.xml"),
        r#"<DOMComponentItem name="Assets/ Widget" linkageExportForAS="true" linkageClassName="com.example.Widget"><timeline><DOMTimeline name="Widget"><layers><DOMLayer name="Layer 1"><frames><DOMFrame index="0"><elements><DOMRectangleObject objectWidth="40" objectHeight="20"/></elements></DOMFrame></frames></DOMLayer></layers></DOMTimeline></timeline><parametersAsXML><property id="label"/></parametersAsXML></DOMComponentItem>"#,
    )
    .unwrap();
    fs::write(
        temp.0.join("DOMDocument.xml"),
        r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="550" height="400" frameRate="24">
  <folders>
    <DOMFolderItem name="Assets"/>
  </folders>
  <fonts>
    <DOMFontItem name="EmbeddedSans" font="Arial" size="14"/>
  </fonts>
  <media>
    <DOMVideoItem name="intro.flv" width="320" height="180"/>
    <DOMCompiledClipItem name="PrebuiltClip" movieRectLeft="0" movieRectTop="0" movieRectRight="1600" movieRectBottom="800"/>
  </media>
  <symbols>
    <Include href="Assets/Widget.xml"/>
  </symbols>
  <persistentData>
    <PD n="com.adobe.animate" t="s" v="ok"/>
  </persistentData>
  <timelines>
    <DOMTimeline name="Scene 1">
      <layers>
        <DOMLayer name="Main">
          <frames>
            <DOMFrame index="0" duration="2">
              <elements>
                <DOMSymbolInstance libraryItemName="__Camera__"/>
                <DOMComponentInstance libraryItemName="Assets/ Widget" name="widget1">
                  <color><Color tintColor="#FF0000" tintMultiplier="0.5"/></color>
                  <filters><BlurFilter blurX="4" blurY="4" quality="1"/></filters>
                </DOMComponentInstance>
                <DOMCompiledClipInstance libraryItemName="PrebuiltClip" name="swc1"/>
                <DOMVideoInstance libraryItemName="intro.flv" name="vid1" frameRight="6400" frameBottom="3600"/>
                <DOMDynamicText name="score_txt" width="120" height="24" isSelectable="false">
                  <textRuns>
                    <DOMTextRun>
                      <characters>Score: 0</characters>
                      <textAttrs><DOMTextAttrs face="EmbeddedSans" size="14" fillColor="#336699" alignment="center"/></textAttrs>
                    </DOMTextRun>
                  </textRuns>
                </DOMDynamicText>
                <DOMInputText name="input_txt" width="120" height="24" lineType="multiline">
                  <textRuns>
                    <DOMTextRun>
                      <characters>Type here</characters>
                      <textAttrs><DOMTextAttrs face="Arial" size="12" fillColor="#000000"/></textAttrs>
                    </DOMTextRun>
                  </textRuns>
                </DOMInputText>
                <DOMTLFText name="tlf_txt" left="0" top="0" right="200" bottom="40">
                  <tlfTextObject>
                    <TextFlow xmlns="http://ns.adobe.com/textLayout/2008" fontFamily="Arial" fontSize="16" color="#112233">
                      <p><span>Hello TLF</span></p>
                    </TextFlow>
                  </tlfTextObject>
                </DOMTLFText>
                <DOMGroup>
                  <members>
                    <DOMRectangleObject objectWidth="30" objectHeight="15" topLeftRadius="4" topRightRadius="4" bottomLeftRadius="4" bottomRightRadius="4"/>
                    <DOMOvalObject objectWidth="20" objectHeight="20" startAngle="0" endAngle="180" innerRadius="25" closePath="true"/>
                  </members>
                </DOMGroup>
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
    assert_eq!(
        project.document_class.as_deref(),
        Some("com.example.AppMain")
    );
    assert_eq!(project.sources.len(), 1);
    assert_eq!(project.sources[0].path, "com/example/AppMain.as");
}
