//! T-468 shareable public regression suite for authoring-stage (`editable_stage`) vs runtime
//! (`scripted_playback`) timeline fidelity, nested bitmap/gradient/filter composition, and
//! authoring/published layer-mask clipping. All fixtures are small synthetic XFL documents built
//! in temporary directories with zero external or private binary inputs.

use image::ImageEncoder;
use neoflash_core::{
    AuthoredPlayback, AuthoredScene, BlendMode, ElementKind, FilterSpec, MaskCoverage, Player,
    filter_rgba,
};
use neoflash_import::import_project;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct SyntheticXfl(PathBuf);

impl SyntheticXfl {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let dir = std::env::temp_dir().join(format!(
            "neoflash-t468-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("LIBRARY")).unwrap();
        fs::write(dir.join("synthetic.xfl"), "PROXY-CS5").unwrap();
        Self(dir)
    }

    fn write(&self, rel: &str, content: &str) {
        let target = self.0.join(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(target, content).unwrap();
    }

    fn write_bytes(&self, rel: &str, bytes: &[u8]) {
        let target = self.0.join(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(target, bytes).unwrap();
    }
}

impl Drop for SyntheticXfl {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn editable_stage_freezes_movie_clips_while_graphics_advance_and_scripted_playback_diverges() {
    let xfl = SyntheticXfl::new("stage-vs-playback");

    xfl.write(
        "LIBRARY/OrbMC.xml",
        r##"<DOMSymbolItem xmlns="http://ns.adobe.com/xfl/2008/" name="OrbMC" symbolType="movie clip">
  <timeline>
    <DOMTimeline name="OrbMC">
      <layers>
        <DOMLayer name="orb">
          <frames>
            <DOMFrame index="0" duration="1">
              <elements>
                <DOMRectangleObject objectWidth="20" objectHeight="20">
                  <matrix><Matrix tx="10" ty="10"/></matrix>
                  <fill><SolidColor color="#22cc88"/></fill>
                </DOMRectangleObject>
              </elements>
            </DOMFrame>
            <DOMFrame index="1" duration="1">
              <Actionscript><script><![CDATA[trace("orb-frame-1");]]></script></Actionscript>
              <elements>
                <DOMRectangleObject objectWidth="20" objectHeight="20">
                  <matrix><Matrix tx="110" ty="10"/></matrix>
                  <fill><SolidColor color="#3388ff"/></fill>
                </DOMRectangleObject>
              </elements>
            </DOMFrame>
            <DOMFrame index="2" duration="1">
              <elements>
                <DOMRectangleObject objectWidth="20" objectHeight="20">
                  <matrix><Matrix tx="210" ty="10"/></matrix>
                  <fill><SolidColor color="#ff8844"/></fill>
                </DOMRectangleObject>
              </elements>
            </DOMFrame>
            <DOMFrame index="3" duration="1">
              <elements>
                <DOMRectangleObject objectWidth="20" objectHeight="20">
                  <matrix><Matrix tx="310" ty="10"/></matrix>
                  <fill><SolidColor color="#ff3366"/></fill>
                </DOMRectangleObject>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timeline>
</DOMSymbolItem>"##,
    );

    xfl.write(
        "LIBRARY/BadgeGraphic.xml",
        r##"<DOMSymbolItem xmlns="http://ns.adobe.com/xfl/2008/" name="BadgeGraphic" symbolType="graphic">
  <timeline>
    <DOMTimeline name="BadgeGraphic">
      <layers>
        <DOMLayer name="badge">
          <frames>
            <DOMFrame index="0" duration="1">
              <elements>
                <DOMRectangleObject objectWidth="16" objectHeight="16">
                  <matrix><Matrix tx="20" ty="80"/></matrix>
                  <fill><SolidColor color="#ffcc00"/></fill>
                </DOMRectangleObject>
              </elements>
            </DOMFrame>
            <DOMFrame index="1" duration="1">
              <elements>
                <DOMRectangleObject objectWidth="16" objectHeight="16">
                  <matrix><Matrix tx="40" ty="80"/></matrix>
                  <fill><SolidColor color="#ffcc00"/></fill>
                </DOMRectangleObject>
              </elements>
            </DOMFrame>
            <DOMFrame index="2" duration="1">
              <elements>
                <DOMRectangleObject objectWidth="16" objectHeight="16">
                  <matrix><Matrix tx="60" ty="80"/></matrix>
                  <fill><SolidColor color="#ffcc00"/></fill>
                </DOMRectangleObject>
              </elements>
            </DOMFrame>
            <DOMFrame index="3" duration="1">
              <elements>
                <DOMRectangleObject objectWidth="16" objectHeight="16">
                  <matrix><Matrix tx="80" ty="80"/></matrix>
                  <fill><SolidColor color="#ffcc00"/></fill>
                </DOMRectangleObject>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timeline>
</DOMSymbolItem>"##,
    );

    xfl.write(
        "DOMDocument.xml",
        r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="400" height="200" frameRate="24" backgroundColor="#101820" currentTimeline="1">
  <symbols>
    <Include href="OrbMC.xml"/>
    <Include href="BadgeGraphic.xml"/>
  </symbols>
  <timelines>
    <DOMTimeline name="Scene 1">
      <layers>
        <DOMLayer name="actions">
          <frames>
            <DOMFrame index="0" duration="4"/>
            <DOMFrame index="4" duration="16">
              <Actionscript><script><![CDATA[stop(); trace("root-stopped-at-4");]]></script></Actionscript>
            </DOMFrame>
          </frames>
        </DOMLayer>
        <DOMLayer name="short_banner">
          <frames>
            <DOMFrame index="0" duration="10">
              <elements>
                <DOMRectangleObject objectWidth="50" objectHeight="12">
                  <matrix><Matrix tx="5" ty="150"/></matrix>
                  <fill><SolidColor color="#ffffff"/></fill>
                </DOMRectangleObject>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
        <DOMLayer name="content">
          <frames>
            <DOMFrame index="0" duration="20">
              <elements>
                <DOMSymbolInstance libraryItemName="OrbMC" name="orb"/>
                <DOMSymbolInstance libraryItemName="BadgeGraphic" symbolType="graphic" loop="loop"/>
                <DOMSymbolInstance libraryItemName="BadgeGraphic" symbolType="graphic" loop="single frame" firstFrame="2">
                  <matrix><Matrix ty="40"/></matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timelines>
</DOMDocument>"##,
    );

    let project = import_project(&xfl.0).expect("import synthetic stage-vs-playback XFL");
    project.validate().expect("imported project validates");

    for (frame, expected_loop_x, banner_present) in
        [(0_u32, 20.0, true), (9, 40.0, true), (19, 80.0, false)]
    {
        let editable =
            AuthoredScene::evaluate_editable_stage(&project, &project.root, frame, false, true);
        let xs_at_y10: Vec<f64> = editable
            .draws
            .iter()
            .filter(|d| (d.item.transform[5] - 10.0).abs() < 1e-3)
            .map(|d| d.item.transform[4])
            .collect();
        assert_eq!(
            xs_at_y10,
            vec![10.0],
            "OrbMC stays on frame 0 on editable stage at frame {frame}"
        );

        let loop_x: Vec<f64> = editable
            .draws
            .iter()
            .filter(|d| (d.item.transform[5] - 80.0).abs() < 1e-3)
            .map(|d| d.item.transform[4])
            .collect();
        assert_eq!(
            loop_x,
            vec![expected_loop_x],
            "looping Graphic advances at frame {frame}"
        );

        let single_x: Vec<f64> = editable
            .draws
            .iter()
            .filter(|d| (d.item.transform[5] - 120.0).abs() < 1e-3)
            .map(|d| d.item.transform[4])
            .collect();
        assert_eq!(
            single_x,
            vec![60.0],
            "single-frame Graphic holds frame 2 at frame {frame}"
        );

        let has_banner = editable
            .draws
            .iter()
            .any(|d| (d.item.transform[5] - 150.0).abs() < 1e-3);
        assert_eq!(
            has_banner, banner_present,
            "unextended layer visibility at frame {frame}"
        );
    }

    let mut playback = AuthoredPlayback::with_live_scripts(&project, &project.root, 0, true);
    let mut traces = playback.drain_traces();
    for _ in 0..9 {
        playback.tick(&project);
        traces.extend(playback.drain_traces());
    }
    assert_eq!(
        playback.frame(),
        4,
        "root timeline halted by stop() on frame 4"
    );
    assert!(playback.is_root_halted_by_script());
    assert!(traces.iter().any(|t| t == "root-stopped-at-4"));
    assert!(traces.iter().any(|t| t == "orb-frame-1"));

    let runtime_scene = playback.scene(&project, false, true);
    let runtime_orb_x: Vec<f64> = runtime_scene
        .draws
        .iter()
        .filter(|d| (d.item.transform[5] - 10.0).abs() < 1e-3)
        .map(|d| d.item.transform[4])
        .collect();
    assert_eq!(runtime_orb_x, vec![110.0]);
}

#[test]
fn nested_bitmap_fill_gradient_color_transform_and_filters_preserve_composition() {
    let xfl = SyntheticXfl::new("nested-paint-filters");

    let mut png_bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png_bytes)
        .write_image(
            &[200, 40, 20, 255, 20, 60, 220, 255],
            2,
            1,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    xfl.write_bytes("LIBRARY/stripes.png", &png_bytes);

    xfl.write(
        "LIBRARY/CardMC.xml",
        r##"<DOMSymbolItem xmlns="http://ns.adobe.com/xfl/2008/" name="CardMC" symbolType="movie clip">
  <timeline>
    <DOMTimeline name="CardMC">
      <layers>
        <DOMLayer name="art">
          <frames>
            <DOMFrame index="0" duration="1">
              <elements>
                <DOMShape>
                  <fills>
                    <FillStyle index="1">
                      <BitmapFill bitmapPath="stripes.png" bitmapIsClipped="false">
                        <matrix><Matrix a="20" d="20" tx="4" ty="2"/></matrix>
                      </BitmapFill>
                    </FillStyle>
                    <FillStyle index="2">
                      <LinearGradient>
                        <matrix><Matrix a="0.05" d="0.05" tx="40" ty="20"/></matrix>
                        <GradientEntry color="#ff0000" ratio="0"/>
                        <GradientEntry color="#0000ff" ratio="1"/>
                      </LinearGradient>
                    </FillStyle>
                  </fills>
                  <edges>
                    <Edge fillStyle1="1" edges="!0 0|400 0!400 0|400 400!400 400|0 400!0 400|0 0"/>
                    <Edge fillStyle1="2" edges="!500 0|900 0!900 0|900 400!900 400|500 400!500 400|500 0"/>
                  </edges>
                </DOMShape>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timeline>
</DOMSymbolItem>"##,
    );

    xfl.write(
        "DOMDocument.xml",
        r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="240" height="160" frameRate="24" backgroundColor="#ffffff" currentTimeline="1">
  <media>
    <DOMBitmapItem name="stripes.png" href="stripes.png" allowSmoothing="false"/>
  </media>
  <symbols>
    <Include href="CardMC.xml"/>
  </symbols>
  <timelines>
    <DOMTimeline name="Scene 1">
      <layers>
        <DOMLayer name="main">
          <frames>
            <DOMFrame index="0" duration="1">
              <elements>
                <DOMSymbolInstance libraryItemName="CardMC" name="card" blendMode="multiply">
                  <matrix><Matrix a="2" d="1.5" tx="30" ty="12"/></matrix>
                  <color><Color alphaMultiplier="0.75" redOffset="32"/></color>
                  <filters>
                    <AdjustColorFilter brightness="40" contrast="25" saturation="10" hue="0"/>
                    <GlowFilter blurX="6" blurY="6" color="#00ffcc" alpha="0.8" strength="2" quality="1"/>
                  </filters>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timelines>
</DOMDocument>"##,
    );

    let project = import_project(&xfl.0).expect("import nested bitmap/gradient/filter XFL");
    project.validate().expect("imported project validates");

    let (scene, filter_nodes) = AuthoredScene::evaluate_editable_stage_with_filters(
        &project,
        &project.root,
        0,
        false,
        true,
    );
    assert_eq!(
        scene.draws.len(),
        2,
        "both bitmap-filled and gradient-filled contours materialize"
    );

    let bm_draw = scene
        .draws
        .iter()
        .find(|d| d.item.bitmap_fill.is_some())
        .expect("bitmap-filled draw");
    let grad_draw = scene
        .draws
        .iter()
        .find(|d| {
            matches!(
                &d.item.kind,
                ElementKind::PaintedPath {
                    fill_gradient: Some(_),
                    ..
                }
            )
        })
        .expect("gradient-filled draw");

    assert!((bm_draw.item.transform[0] - 2.0).abs() < 1e-6);
    assert!((bm_draw.item.transform[3] - 1.5).abs() < 1e-6);
    assert!((bm_draw.item.transform[4] - 30.0).abs() < 1e-6);
    assert!((bm_draw.item.transform[5] - 12.0).abs() < 1e-6);
    assert!((bm_draw.item.color_transform.multipliers[3] - 0.75).abs() < 1e-3);
    assert!((bm_draw.item.color_transform.offsets[0] - 32.0).abs() < 1e-3);
    assert!((grad_draw.item.transform[0] - 2.0).abs() < 1e-6);

    let bm = bm_draw.item.bitmap_fill.as_ref().unwrap();
    assert!(bm.repeat);
    assert!(
        (bm.matrix[0] - 1.0).abs() < 1e-6,
        "20 twips -> 1.0 px bitmap UV scale"
    );
    assert!((bm.matrix[4] - 4.0).abs() < 1e-6);

    assert_eq!(filter_nodes.len(), 1);
    let (ref filter_path, ref specs, ref outer_clips, blend) = filter_nodes[0];
    assert_eq!(filter_path.len(), 1);
    assert!(bm_draw.item.instance_path.starts_with(filter_path));
    assert!(outer_clips.is_empty());
    assert_eq!(blend, BlendMode::Multiply);
    assert_eq!(specs.len(), 2);
    assert!(matches!(specs[0], FilterSpec::ColorMatrix { .. }));
    assert!(matches!(specs[1], FilterSpec::Glow { .. }));

    let mut src_rgba = vec![0_u8; 12 * 12 * 4];
    for py in 4..8 {
        for px in 4..8 {
            let idx = (py * 12 + px) * 4;
            src_rgba[idx..idx + 4].copy_from_slice(&[100, 80, 60, 255]);
        }
    }
    let center = (6 * 12 + 6) * 4;
    let filtered = filter_rgba(12, 12, &src_rgba, specs).expect("filter_rgba");
    assert!(
        filtered[center] > 120,
        "AdjustColorFilter ColorMatrix brightens center pixel (got {})",
        filtered[center]
    );
    let neighbor = (6 * 12 + 3) * 4;
    assert!(
        filtered[neighbor + 3] > 0,
        "GlowFilter spreads alpha to neighboring pixels"
    );
}

#[test]
fn hidden_masks_stroke_exclusion_and_empty_mask_frames_match_authoring_and_runtime() {
    let xfl = SyntheticXfl::new("mask-fidelity");

    xfl.write(
        "LIBRARY/InnerMasked.xml",
        r##"<DOMSymbolItem xmlns="http://ns.adobe.com/xfl/2008/" name="InnerMasked" symbolType="graphic">
  <timeline>
    <DOMTimeline name="InnerMasked">
      <layers>
        <DOMLayer name="inner_mask" layerType="mask" visible="false" locked="true">
          <frames>
            <DOMFrame index="0" duration="10">
              <elements>
                <DOMRectangleObject objectWidth="80" objectHeight="80">
                  <matrix><Matrix tx="0" ty="0"/></matrix>
                  <fill><SolidColor color="#ffffff"/></fill>
                </DOMRectangleObject>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
        <DOMLayer name="inner_art" parentLayerIndex="0" locked="true">
          <frames>
            <DOMFrame index="0" duration="10">
              <elements>
                <DOMRectangleObject objectWidth="200" objectHeight="200">
                  <matrix><Matrix tx="0" ty="0"/></matrix>
                  <fill><SolidColor color="#dd2244"/></fill>
                </DOMRectangleObject>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timeline>
</DOMSymbolItem>"##,
    );

    xfl.write(
        "DOMDocument.xml",
        r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="200" height="200" frameRate="24" backgroundColor="#ffffff" currentTimeline="1">
  <symbols>
    <Include href="InnerMasked.xml"/>
  </symbols>
  <timelines>
    <DOMTimeline name="Scene 1">
      <layers>
        <DOMLayer name="outer_mask" layerType="mask" visible="false" locked="true">
          <frames>
            <DOMFrame index="0" duration="5">
              <elements>
                <DOMShape>
                  <fills>
                    <FillStyle index="1"><SolidColor color="#000000"/></FillStyle>
                  </fills>
                  <strokes>
                    <StrokeStyle index="1"><SolidStroke scaleMode="normal" weight="4"><fill><SolidColor color="#00ff00"/></fill></SolidStroke></StrokeStyle>
                  </strokes>
                  <edges>
                    <Edge fillStyle1="1" edges="!0 0|1200 0!1200 0|1200 1200!1200 1200|0 1200!0 1200|0 0"/>
                    <Edge strokeStyle="1" edges="!0 0|3600 0!3600 0|3600 3600!3600 3600|0 3600!0 3600|0 0"/>
                  </edges>
                </DOMShape>
              </elements>
            </DOMFrame>
            <DOMFrame index="5" duration="5"/>
          </frames>
        </DOMLayer>
        <DOMLayer name="masked_target" parentLayerIndex="0">
          <frames>
            <DOMFrame index="0" duration="10">
              <elements>
                <DOMSymbolInstance libraryItemName="InnerMasked" symbolType="graphic" loop="loop"/>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timelines>
</DOMDocument>"##,
    );

    let project = import_project(&xfl.0).expect("import mask fidelity XFL");
    project.validate().expect("imported project validates");

    let stage_f2 = AuthoredScene::evaluate_editable_stage(&project, &project.root, 2, false, true);
    assert_eq!(
        stage_f2.draws.len(),
        1,
        "only masked content draw is emitted; mask shapes are consumed"
    );
    let cov_f2 = MaskCoverage::new(&project, &stage_f2.clips).unwrap();
    let clip_ids_f2 = &stage_f2.draws[0].clip_ids;
    assert_eq!(cov_f2.alpha(clip_ids_f2, [30.0, 30.0]), 1.0);
    assert_eq!(
        cov_f2.alpha(clip_ids_f2, [70.0, 30.0]),
        0.0,
        "unfilled stroke on mask layer must not expand geometric mask coverage"
    );
    assert_eq!(cov_f2.alpha(clip_ids_f2, [120.0, 30.0]), 0.0);

    let mut player = Player::new(&project);
    player.seek(&project, 2);
    let snap_f2 = player.runtime_snapshot(&project);
    let rt_cov_f2 = MaskCoverage::new(&project, &snap_f2.clips).unwrap();
    assert_eq!(
        rt_cov_f2.alpha(&snap_f2.draws[0].clip_ids, [30.0, 30.0]),
        1.0
    );
    assert_eq!(
        rt_cov_f2.alpha(&snap_f2.draws[0].clip_ids, [70.0, 30.0]),
        0.0
    );

    let stage_f7 = AuthoredScene::evaluate_editable_stage(&project, &project.root, 7, false, true);
    let cov_f7 = MaskCoverage::new(&project, &stage_f7.clips).unwrap();
    assert_eq!(
        cov_f7.alpha(&stage_f7.draws[0].clip_ids, [30.0, 30.0]),
        0.0,
        "empty mask keyframe clips masked target completely"
    );
    player.seek(&project, 7);
    let snap_f7 = player.runtime_snapshot(&project);
    let rt_cov_f7 = MaskCoverage::new(&project, &snap_f7.clips).unwrap();
    assert_eq!(
        rt_cov_f7.alpha(&snap_f7.draws[0].clip_ids, [30.0, 30.0]),
        0.0
    );
}
