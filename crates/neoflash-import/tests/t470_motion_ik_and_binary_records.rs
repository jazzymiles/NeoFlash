//! Integration tests for `[T-470]` XFL & Binary FLA CS4+ Motion-Object,
//! Multi-Segment / Per-Property `<CustomEase>`, IK Armature (`<DOMIKTree>` /
//! `<IKTree>` / `layerType="armature"` / `CPicFrame` `pose_matrices`), and
//! Binary FLA single-line Static Point Text inert negative leading.
//! All fixtures are 100% synthetic and self-contained.

use neoflash_core::{
    Easing, ElementKind, FilterSpec, TweenKind, TweenProperty, evaluate, sample_layer,
    transform_matrix,
};
use neoflash_import::{Severity, import_project, inspect};
use std::{
    collections::BTreeMap,
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
            "neoflash-t470-{}-{}",
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

fn write_xfl_dir(root: &Path, entries: &[(&str, &str)]) {
    for (rel, content) in entries {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content.as_bytes()).unwrap();
    }
}

fn box_symbol_xml(name: &str, w_px: i32, h_px: i32, color: &str) -> String {
    let w = w_px * 20;
    let h = h_px * 20;
    format!(
        r##"<DOMSymbolItem xmlns="http://ns.adobe.com/xfl/2008/" name="{name}" symbolType="movie clip">
  <timeline>
    <DOMTimeline name="{name}">
      <layers>
        <DOMLayer name="Art">
          <frames>
            <DOMFrame index="0">
              <elements>
                <DOMShape>
                  <fills><FillStyle index="1"><SolidColor color="{color}"/></FillStyle></fills>
                  <edges><Edge fillStyle1="1" edges="!0 0|{w} 0|{w} {h}|0 {h}|0 0"/></edges>
                </DOMShape>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timeline>
</DOMSymbolItem>"##
    )
}

#[test]
fn xfl_animation_core_and_fl_motion_sample_skew_color_3d_roving_spline_and_timemap() {
    let dom = r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="640" height="480" frameRate="24" viewAngle3D="60" vanishingPoint3DX="320" vanishingPoint3DY="240" xflVersion="2.2">
  <symbols>
    <Include href="Card.xml"/>
  </symbols>
  <timelines>
    <DOMTimeline name="Scene 1">
      <layers>
        <DOMLayer name="SkewSpline" animationType="motion object">
          <frames>
            <DOMFrame index="0" duration="11" tweenType="motion object" isMotionObject="true">
              <MotionObjectXML>
                <AnimationCore TimeScale="24000" Version="1" duration="11000">
                  <TimeMap strength="0" type="Quadratic"/>
                  <metadata><Settings orientToPath="0" xformPtXOffsetPct="0.5" xformPtYOffsetPct="0.5" xformPtZOffsetPixels="0"/></metadata>
                  <PropertyContainer id="headContainer">
                    <PropertyContainer id="Basic_Motion">
                      <Property enabled="1" id="Motion_X" ignoreTimeMap="0">
                        <Keyframe timevalue="0" anchor="0,0" next="7500,0" previous="0,0" roving="0"/>
                        <Keyframe timevalue="10000" anchor="0,200" next="0,200" previous="-7500,200" roving="0"/>
                      </Property>
                      <Property enabled="1" id="Motion_Y" ignoreTimeMap="0">
                        <Keyframe timevalue="0" anchor="0,0" next="0,0" previous="0,0" roving="0"/>
                        <Keyframe timevalue="10000" anchor="0,40" next="0,40" previous="0,40" roving="0"/>
                      </Property>
                    </PropertyContainer>
                    <PropertyContainer id="Transformation">
                      <Property enabled="1" id="Skew_X" ignoreTimeMap="0">
                        <Keyframe timevalue="0" anchor="0,0" next="0,0" previous="0,0"/>
                        <Keyframe timevalue="10000" anchor="0,25" next="0,25" previous="0,25"/>
                      </Property>
                      <Property enabled="1" id="Skew_Y" ignoreTimeMap="0">
                        <Keyframe timevalue="0" anchor="0,0" next="0,0" previous="0,0"/>
                        <Keyframe timevalue="10000" anchor="0,-10" next="0,-10" previous="0,-10"/>
                      </Property>
                    </PropertyContainer>
                  </PropertyContainer>
                </AnimationCore>
              </MotionObjectXML>
              <elements>
                <DOMSymbolInstance libraryItemName="Card" name="skew_mc">
                  <matrix><Matrix tx="50" ty="60"/></matrix>
                  <transformationPoint><Point x="20" y="10"/></transformationPoint>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>

        <DOMLayer name="Colors" animationType="motion object">
          <frames>
            <DOMFrame index="0" duration="3" tweenType="motion object">
              <motionObjectXML>
                <AnimationCore TimeScale="24000" Version="1" duration="3000">
                  <PropertyContainer id="Colors">
                    <Property enabled="1" id="Alpha_Amount">
                      <Keyframe timevalue="0" anchor="0,100" next="0,100" previous="0,100"/>
                      <Keyframe timevalue="2000" anchor="0,25" next="0,25" previous="0,25"/>
                    </Property>
                  </PropertyContainer>
                </AnimationCore>
              </motionObjectXML>
              <elements>
                <DOMSymbolInstance libraryItemName="Card" name="col_mc">
                  <matrix><Matrix tx="80" ty="120"/></matrix>
                  <transformationPoint><Point x="0" y="0"/></transformationPoint>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
            <DOMFrame index="3" duration="3" tweenType="motion object">
              <motionObjectXML>
                <AnimationCore TimeScale="24000" Version="1" duration="3000">
                  <PropertyContainer id="Colors">
                    <Property enabled="1" id="Brightness_Amount">
                      <Keyframe timevalue="0" anchor="0,-50" next="0,-50" previous="0,-50"/>
                      <Keyframe timevalue="2000" anchor="0,60" next="0,60" previous="0,60"/>
                    </Property>
                  </PropertyContainer>
                </AnimationCore>
              </motionObjectXML>
              <elements>
                <DOMSymbolInstance libraryItemName="Card" name="col_mc">
                  <matrix><Matrix tx="80" ty="120"/></matrix>
                  <transformationPoint><Point x="0" y="0"/></transformationPoint>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
            <DOMFrame index="6" duration="3" tweenType="motion object">
              <motionObjectXML>
                <AnimationCore TimeScale="24000" Version="1" duration="3000">
                  <PropertyContainer id="Colors">
                    <Property enabled="1" id="Tint_Color">
                      <Keyframe timevalue="0" value="0xFF0000FF"/>
                      <Keyframe timevalue="2000" value="0x0000FFFF"/>
                    </Property>
                    <Property enabled="1" id="Tint_Amount">
                      <Keyframe timevalue="0" anchor="0,40" next="0,40" previous="0,40"/>
                      <Keyframe timevalue="2000" anchor="0,80" next="0,80" previous="0,80"/>
                    </Property>
                  </PropertyContainer>
                </AnimationCore>
              </motionObjectXML>
              <elements>
                <DOMSymbolInstance libraryItemName="Card" name="col_mc">
                  <matrix><Matrix tx="80" ty="120"/></matrix>
                  <transformationPoint><Point x="0" y="0"/></transformationPoint>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
            <DOMFrame index="9" duration="2" tweenType="motion object">
              <motionObjectXML>
                <AnimationCore TimeScale="24000" Version="1" duration="2000">
                  <PropertyContainer id="Colors">
                    <Property enabled="1" id="AdvClr_R_Pct"><Keyframe timevalue="0" anchor="0,100"/><Keyframe timevalue="1000" anchor="0,50"/></Property>
                    <Property enabled="1" id="AdvClr_G_Pct"><Keyframe timevalue="0" anchor="0,100"/><Keyframe timevalue="1000" anchor="0,75"/></Property>
                    <Property enabled="1" id="AdvClr_B_Pct"><Keyframe timevalue="0" anchor="0,100"/><Keyframe timevalue="1000" anchor="0,25"/></Property>
                    <Property enabled="1" id="AdvClr_A_Pct"><Keyframe timevalue="0" anchor="0,100"/><Keyframe timevalue="1000" anchor="0,80"/></Property>
                    <Property enabled="1" id="AdvClr_R_Offset"><Keyframe timevalue="0" anchor="0,0"/><Keyframe timevalue="1000" anchor="0,64"/></Property>
                    <Property enabled="1" id="AdvClr_G_Offset"><Keyframe timevalue="0" anchor="0,0"/><Keyframe timevalue="1000" anchor="0,-32"/></Property>
                    <Property enabled="1" id="AdvClr_B_Offset"><Keyframe timevalue="0" anchor="0,0"/><Keyframe timevalue="1000" anchor="0,128"/></Property>
                    <Property enabled="1" id="AdvClr_A_Offset"><Keyframe timevalue="0" anchor="0,0"/><Keyframe timevalue="1000" anchor="0,-10"/></Property>
                  </PropertyContainer>
                </AnimationCore>
              </motionObjectXML>
              <elements>
                <DOMSymbolInstance libraryItemName="Card" name="col_mc">
                  <matrix><Matrix tx="80" ty="120"/></matrix>
                  <transformationPoint><Point x="0" y="0"/></transformationPoint>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>

        <DOMLayer name="ThreeD" animationType="motion object">
          <frames>
            <DOMFrame index="0" duration="11" tweenType="motion object">
              <MotionObjectXML>
                <AnimationCore TimeScale="24000" Version="1" duration="11000">
                  <metadata><Settings orientToPath="0" xformPtZOffsetPixels="10"/></metadata>
                  <PropertyContainer id="Basic_Motion">
                    <Property enabled="1" id="Rotation_X">
                      <Keyframe timevalue="0" anchor="0,0" next="0,0" previous="0,0"/>
                      <Keyframe timevalue="10000" anchor="0,45" next="0,45" previous="0,45"/>
                    </Property>
                    <Property enabled="1" id="Rotation_Y">
                      <Keyframe timevalue="0" anchor="0,0" next="0,0" previous="0,0"/>
                      <Keyframe timevalue="10000" anchor="0,90" next="0,90" previous="0,90"/>
                    </Property>
                    <Property enabled="1" id="Motion_Z">
                      <Keyframe timevalue="0" anchor="0,0" next="0,0" previous="0,0"/>
                      <Keyframe timevalue="10000" anchor="0,300" next="0,300" previous="0,300"/>
                    </Property>
                  </PropertyContainer>
                </AnimationCore>
              </MotionObjectXML>
              <elements>
                <DOMSymbolInstance libraryItemName="Card" name="card3d">
                  <matrix><Matrix tx="300" ty="220"/></matrix>
                  <transformationPoint><Point x="20" y="20"/></transformationPoint>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>

        <DOMLayer name="RovingTimeMap" animationType="motion object">
          <frames>
            <DOMFrame index="0" duration="11" tweenType="motion object">
              <MotionObjectXML>
                <AnimationCore TimeScale="24000" Version="1" duration="11000">
                  <TimeMap strength="-100" type="Quadratic"/>
                  <PropertyContainer id="Basic_Motion">
                    <Property enabled="1" id="Motion_X" ignoreTimeMap="0">
                      <Keyframe timevalue="0" anchor="0,0" next="0,0" previous="0,0" roving="0"/>
                      <Keyframe timevalue="9000" anchor="0,50" next="0,50" previous="0,50" roving="1"/>
                      <Keyframe timevalue="10000" anchor="0,50" next="0,50" previous="0,50" roving="0"/>
                    </Property>
                    <Property enabled="1" id="Motion_Y" ignoreTimeMap="0">
                      <Keyframe timevalue="0" anchor="0,0" next="0,0" previous="0,0" roving="0"/>
                      <Keyframe timevalue="9000" anchor="0,0" next="0,0" previous="0,0" roving="1"/>
                      <Keyframe timevalue="10000" anchor="0,150" next="0,150" previous="0,150" roving="0"/>
                    </Property>
                  </PropertyContainer>
                </AnimationCore>
              </MotionObjectXML>
              <elements>
                <DOMSymbolInstance libraryItemName="Card" name="rov_mc">
                  <matrix><Matrix tx="10" ty="10"/></matrix>
                  <transformationPoint><Point x="0" y="0"/></transformationPoint>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>

        <DOMLayer name="FlMotionDialect" animationType="motion object">
          <frames>
            <DOMFrame index="0" duration="11" tweenType="motion object">
              <DOMMotionObject>
                <Motion duration="11" xmlns="fl.motion.*">
                  <source>
                    <Source frameRate="24">
                      <dimensions><GeomRectangle left="0" top="0" width="40" height="40"/></dimensions>
                      <transformationPoint><Point x="0.5" y="0.5"/></transformationPoint>
                    </Source>
                  </source>
                  <Keyframe index="0" x="0" y="0" scaleX="1" scaleY="1" skewX="0" skewY="0">
                    <tweens><Ease method="CubicEaseOut" intensity="100"/></tweens>
                  </Keyframe>
                  <Keyframe index="10" x="100" y="-20" scaleX="1.5" scaleY="0.8" skewX="15" skewY="5">
                    <color><Color tintColor="#00AAFF" tintMultiplier="0.5"/></color>
                  </Keyframe>
                </Motion>
              </DOMMotionObject>
              <elements>
                <DOMSymbolInstance libraryItemName="Card" name="flm_mc">
                  <matrix><Matrix tx="120" ty="300"/></matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timelines>
</DOMDocument>"##;

    let tmp = TempDir::new();
    let card = box_symbol_xml("Card", 40, 40, "#3388EE");
    write_xfl_dir(
        &tmp.0,
        &[("DOMDocument.xml", dom), ("LIBRARY/Card.xml", &card)],
    );

    let report = inspect(&tmp.0).unwrap();
    assert!(report.can_import, "{:?}", report.diagnostics);
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.code != "motion_object_not_supported"),
        "unexpected motion_object_not_supported: {:?}",
        report.diagnostics
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "xfl_3d_approximated" && d.location.contains("DOMDocument.xml")),
        "expected located xfl_3d_approximated warning: {:?}",
        report.diagnostics
    );

    let project = import_project(&tmp.0).unwrap();
    project.validate().unwrap();
    let tl = &project.symbol(&project.root).unwrap().timeline;

    // 1. Skew + S-curve spline (x1=0.75 > x2=0.25)
    let skew_layer = tl.layers.iter().find(|l| l.name == "SkewSpline").unwrap();
    assert_eq!(skew_layer.keyframes.len(), 11);
    let end_skew = &skew_layer.keyframes[10].elements[0].transform;
    assert!((end_skew.rotation - (-10.0)).abs() < 1e-4);
    assert!((end_skew.skew - 35.0).abs() < 1e-4); // skewX(25) - skewY(-10) = 35
    let mid_skew = &skew_layer.keyframes[5].elements[0].transform;
    // At t = 0.5 of symmetrical S-curve (0, 0.75, 0.25, 1), u = 0.5 -> x_offset = 100.
    let m5 = transform_matrix(*mid_skew);
    let piv5_x = m5[0] * 20.0 + m5[2] * 10.0 + m5[4];
    assert!((piv5_x - (50.0 + 20.0 + 100.0)).abs() < 1e-3);

    // 2. Color tracks: Alpha, Brightness, Tint (RGBA hex), and Advanced RGBA
    let col_layer = tl.layers.iter().find(|l| l.name == "Colors").unwrap();
    let f2_alpha = sample_layer(col_layer, 2)[0].transform.alpha;
    assert!((f2_alpha - 0.25).abs() < 1e-4);
    let f3_bright = sample_layer(col_layer, 3)[0].transform.color_transform;
    assert!((f3_bright.multipliers[0] - 0.5).abs() < 1e-4 && f3_bright.offsets[0].abs() < 1e-4);
    let f5_bright = sample_layer(col_layer, 5)[0].transform.color_transform;
    assert!(
        (f5_bright.multipliers[0] - 0.4).abs() < 1e-4
            && (f5_bright.offsets[0] - 153.0).abs() < 1e-3
    );
    let f7_tint = sample_layer(col_layer, 7)[0].transform.color_transform;
    // Midpoint (t=1000 of 2000): amount = 60%, color = (127.5, 0, 127.5)
    assert!((f7_tint.multipliers[0] - 0.4).abs() < 1e-4);
    assert!((f7_tint.offsets[0] - 76.5).abs() < 0.5 && (f7_tint.offsets[2] - 76.5).abs() < 0.5);
    let f10_adv = sample_layer(col_layer, 10)[0].transform;
    assert!((f10_adv.alpha - 0.8).abs() < 1e-4);
    assert!((f10_adv.color_transform.multipliers[0] - 0.5).abs() < 1e-4);
    assert!((f10_adv.color_transform.offsets[2] - 128.0).abs() < 1e-4);

    // 3. 3D perspective foreshortening + edge-on 90-degree Rotation_Y safety
    let td_layer = tl.layers.iter().find(|l| l.name == "ThreeD").unwrap();
    assert_eq!(td_layer.keyframes.len(), 11);
    let td_mid = sample_layer(td_layer, 5)[0].transform;
    assert!(
        td_mid.scale_x < 0.85 && td_mid.scale_y < 0.85,
        "positive Motion_Z + Rotation_X/Y must foreshorten: {td_mid:?}"
    );

    // 4. Roving + Quadratic ease-in TimeMap
    // Segments: (0,0)->(50,0) len 50 and (50,0)->(50,150) len 150 -> roving knot moves from t=9000 to t=2500!
    // At frame 5 (t=5000, r=0.5), Quadratic ease-in (strength=-100) maps r=0.5 -> ratio=0.25 -> eased_t = 2500 -> exact corner (50, 0)!
    let rov_layer = tl
        .layers
        .iter()
        .find(|l| l.name == "RovingTimeMap")
        .unwrap();
    let rov5 = sample_layer(rov_layer, 5)[0].transform;
    assert!(
        (rov5.x - 60.0).abs() < 0.5 && (rov5.y - 10.0).abs() < 0.5,
        "roving + Quadratic ease-in at frame 5 must hit corner (60, 10), got ({}, {})",
        rov5.x,
        rov5.y
    );

    // 5. `<DOMMotionObject><Motion>` dialect with CubicEaseOut + skew + tint
    let flm_layer = tl
        .layers
        .iter()
        .find(|l| l.name == "FlMotionDialect")
        .unwrap();
    let flm_mid = sample_layer(flm_layer, 5)[0].transform;
    // CubicEaseOut at u=0.5 is 1 - 0.5^3 = 0.875 > 0.5 -> x moved more than halfway!
    assert!(
        flm_mid.x > 120.0 + 70.0,
        "expected CubicEaseOut x > 190, got {}",
        flm_mid.x
    );
    let flm_end = sample_layer(flm_layer, 10)[0].transform;
    assert!((flm_end.skew - 10.0).abs() < 1e-3);
    assert!((flm_end.color_transform.multipliers[0] - 0.5).abs() < 1e-3);
}

#[test]
fn xfl_multi_segment_and_per_property_custom_ease_subdivide_and_map_property_eases() {
    let dom = r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="550" height="400" frameRate="24" xflVersion="2.2">
  <symbols>
    <Include href="Dot.xml"/>
  </symbols>
  <timelines>
    <DOMTimeline name="Scene 1">
      <layers>
        <DOMLayer name="GlobalMultiSeg">
          <frames>
            <DOMFrame index="0" duration="10" tweenType="motion" hasCustomEase="true" useSingleEaseCurve="true">
              <tweens>
                <CustomEase target="all">
                  <Point x="0" y="0"/>
                  <Point x="0.1" y="0.0"/>
                  <Point x="0.2" y="0.4"/>
                  <Point x="0.3" y="0.5"/>
                  <Point x="0.45" y="0.65"/>
                  <Point x="0.55" y="0.85"/>
                  <Point x="0.7" y="0.9"/>
                  <Point x="0.8" y="0.95"/>
                  <Point x="0.9" y="1.0"/>
                  <Point x="1" y="1"/>
                </CustomEase>
              </tweens>
              <elements>
                <DOMSymbolInstance libraryItemName="Dot" name="g_mc">
                  <matrix><Matrix tx="0" ty="20"/></matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
            <DOMFrame index="10" duration="1">
              <elements>
                <DOMSymbolInstance libraryItemName="Dot" name="g_mc">
                  <matrix><Matrix tx="200" ty="120"/></matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>

        <DOMLayer name="PerPropertyNative">
          <frames>
            <DOMFrame index="0" duration="10" tweenType="motion" hasCustomEase="true" useSingleEaseCurve="false">
              <tweens>
                <CustomEase target="position">
                  <Point x="0" y="0"/>
                  <Point x="0" y="0.9"/>
                  <Point x="0.2" y="1"/>
                  <Point x="1" y="1"/>
                </CustomEase>
                <CustomEase target="rotation">
                  <Point x="0" y="0"/>
                  <Point x="0.8" y="0"/>
                  <Point x="1" y="0.1"/>
                  <Point x="1" y="1"/>
                </CustomEase>
                <CustomEase target="scale">
                  <Point x="0" y="0"/>
                  <Point x="0.3" y="0.3"/>
                  <Point x="0.7" y="0.7"/>
                  <Point x="1" y="1"/>
                </CustomEase>
                <CustomEase target="color">
                  <Point x="0" y="0"/>
                  <Point x="0" y="1"/>
                  <Point x="0" y="1"/>
                  <Point x="1" y="1"/>
                </CustomEase>
              </tweens>
              <elements>
                <DOMSymbolInstance libraryItemName="Dot" name="pp_mc">
                  <matrix><Matrix a="1" d="1" tx="10" ty="50"/></matrix>
                  <color><Color alphaMultiplier="1"/></color>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
            <DOMFrame index="10" duration="1">
              <elements>
                <DOMSymbolInstance libraryItemName="Dot" name="pp_mc">
                  <matrix><Matrix a="0" b="2" c="-2" d="0" tx="210" ty="50"/></matrix>
                  <color><Color alphaMultiplier="0.2"/></color>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>

        <DOMLayer name="NonIntegerAndFilters">
          <frames>
            <DOMFrame index="0" duration="10" tweenType="motion" hasCustomEase="true" useSingleEaseCurve="false">
              <tweens>
                <CustomEase target="position">
                  <Point x="0" y="0"/>
                  <Point x="0.1" y="0.2"/>
                  <Point x="0.2" y="0.6"/>
                  <Point x="0.33" y="0.7"/>
                  <Point x="0.6" y="0.85"/>
                  <Point x="0.8" y="0.95"/>
                  <Point x="1" y="1"/>
                </CustomEase>
                <CustomEase target="filters">
                  <Point x="0" y="0"/>
                  <Point x="0" y="1"/>
                  <Point x="0" y="1"/>
                  <Point x="1" y="1"/>
                </CustomEase>
              </tweens>
              <elements>
                <DOMSymbolInstance libraryItemName="Dot" name="flt_mc">
                  <matrix><Matrix tx="0" ty="200"/></matrix>
                  <filters><BlurFilter blurX="2" blurY="2" quality="1"/></filters>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
            <DOMFrame index="10" duration="1">
              <elements>
                <DOMSymbolInstance libraryItemName="Dot" name="flt_mc">
                  <matrix><Matrix tx="100" ty="200"/></matrix>
                  <filters><BlurFilter blurX="22" blurY="22" quality="1"/></filters>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timelines>
</DOMDocument>"##;

    let tmp = TempDir::new();
    let dot = box_symbol_xml("Dot", 20, 20, "#EE4433");
    write_xfl_dir(
        &tmp.0,
        &[("DOMDocument.xml", dom), ("LIBRARY/Dot.xml", &dot)],
    );

    let report = inspect(&tmp.0).unwrap();
    assert!(report.can_import, "{:?}", report.diagnostics);
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.code != "custom_ease_not_supported"),
        "unexpected custom_ease_not_supported: {:?}",
        report.diagnostics
    );
    let easing_warns: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.code == "xfl_tween_easing")
        .collect();
    assert_eq!(
        easing_warns.len(),
        1,
        "only NonIntegerAndFilters should emit xfl_tween_easing: {:?}",
        report.diagnostics
    );

    let project = import_project(&tmp.0).unwrap();
    project.validate().unwrap();
    let tl = &project.symbol(&project.root).unwrap().timeline;

    // A. GlobalMultiSeg subdivided at integer knots 3 and 7 into CubicBezier sub-spans.
    let g_layer = tl
        .layers
        .iter()
        .find(|l| l.name == "GlobalMultiSeg")
        .unwrap();
    let g_frames: Vec<u32> = g_layer.keyframes.iter().map(|k| k.frame).collect();
    assert_eq!(g_frames, vec![0, 3, 7, 10]);
    for idx in 0..3 {
        assert_eq!(g_layer.keyframes[idx].tween_kind, TweenKind::Classic);
        assert!(matches!(
            g_layer.keyframes[idx].tween,
            Some(Easing::CubicBezier { .. })
        ));
    }
    assert!((sample_layer(g_layer, 3)[0].transform.x - 100.0).abs() < 1e-4);
    assert!((sample_layer(g_layer, 7)[0].transform.x - 180.0).abs() < 1e-4);

    // B. PerPropertyNative mapped directly to TweenKind::Motion + Keyframe.property_eases!
    let pp_layer = tl
        .layers
        .iter()
        .find(|l| l.name == "PerPropertyNative")
        .unwrap();
    assert_eq!(pp_layer.keyframes[0].tween_kind, TweenKind::Motion);
    let prop_keys: Vec<TweenProperty> = pp_layer.keyframes[0]
        .property_eases
        .iter()
        .map(|(p, _)| *p)
        .collect();
    for expected_prop in [
        TweenProperty::X,
        TweenProperty::Y,
        TweenProperty::ScaleX,
        TweenProperty::ScaleY,
        TweenProperty::Rotation,
        TweenProperty::Skew,
        TweenProperty::Alpha,
        TweenProperty::Color,
    ] {
        assert!(prop_keys.contains(&expected_prop));
    }
    let pp_mid = sample_layer(pp_layer, 5)[0].transform;
    // Fast-out position vs slow-in rotation at frame 5 (u=0.5):
    assert!(
        pp_mid.x > 160.0,
        "fast-out position expected x > 160, got {}",
        pp_mid.x
    );
    assert!(
        pp_mid.rotation < 30.0,
        "slow-in rotation expected rot < 30, got {}",
        pp_mid.rotation
    );
    assert!((pp_mid.scale_x - 1.5).abs() < 0.05);

    // C. NonIntegerAndFilters baked exact per-frame poses including BlurFilter!
    let flt_layer = tl
        .layers
        .iter()
        .find(|l| l.name == "NonIntegerAndFilters")
        .unwrap();
    assert_eq!(flt_layer.keyframes.len(), 11);
    let flt_f3 = &sample_layer(flt_layer, 3)[0];
    assert!(flt_f3.transform.x > 60.0);
    match flt_f3.filters.first() {
        Some(FilterSpec::Blur { blur_x, .. }) => assert!(*blur_x > 15.0),
        other => panic!("expected fast-eased Blur filter at frame 3, got {other:?}"),
    }
}

#[test]
fn xfl_armature_layer_and_ik_tree_bake_forward_kinematics_poses() {
    let dom = r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="550" height="400" frameRate="24" xflVersion="2.2">
  <symbols>
    <Include href="Limb.xml"/>
  </symbols>
  <timelines>
    <DOMTimeline name="Scene 1">
      <layers>
        <DOMLayer name="ArmatureFK" layerType="armature">
          <DOMIKTree name="Arm">
            <bones>
              <DOMBone id="upper" target="upper_mc" x="100" y="150" length="80" rotation="0"/>
              <DOMBone id="lower" parent="upper" target="lower_mc" length="60" rotation="0"/>
            </bones>
          </DOMIKTree>
          <frames>
            <DOMFrame index="0" duration="10">
              <ikNode bone="upper" rotation="0"/>
              <ikNode bone="lower" rotation="0"/>
              <elements>
                <DOMSymbolInstance libraryItemName="Limb" name="upper_mc">
                  <matrix><Matrix tx="100" ty="150"/></matrix>
                </DOMSymbolInstance>
                <DOMSymbolInstance libraryItemName="Limb" name="lower_mc">
                  <matrix><Matrix tx="180" ty="150"/></matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
            <DOMFrame index="10" duration="1">
              <ikNode bone="upper" rotation="90"/>
              <ikNode bone="lower" rotation="-90"/>
              <elements>
                <DOMSymbolInstance libraryItemName="Limb" name="upper_mc">
                  <matrix><Matrix tx="100" ty="150"/></matrix>
                </DOMSymbolInstance>
                <DOMSymbolInstance libraryItemName="Limb" name="lower_mc">
                  <matrix><Matrix tx="180" ty="150"/></matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>

        <DOMLayer name="AdobeIkStates" animationType="IK pose" poseLocations="0,10">
          <frames>
            <DOMFrame index="0" duration="11" tweenType="IK pose" isIKPose="true">
              <elements>
                <DOMSymbolInstance libraryItemName="Limb" name="seg_a" referenceID="refA">
                  <matrix><Matrix tx="300" ty="100"/></matrix>
                </DOMSymbolInstance>
                <DOMSymbolInstance libraryItemName="Limb" name="seg_b" referenceID="refB">
                  <matrix><Matrix tx="380" ty="100"/></matrix>
                </DOMSymbolInstance>
              </elements>
              <IKTree name="Arm2" treeName="tree2">
                <IKNode name="n1" boneName="b1" referenceID="refA" location="300,100">
                  <states>
                    <State duration="10" x="6000" y="2000" angle="0"/>
                    <State duration="1" x="6000" y="2000" angle="1.5707963267948966"/>
                  </states>
                  <childNodes>
                    <ChildNode name="n2" boneName="b2" referenceID="refB" location="380,100">
                      <states>
                        <State duration="10" x="1600" y="0" angle="0"/>
                        <State duration="1" x="1600" y="0" angle="-1.5707963267948966"/>
                      </states>
                    </ChildNode>
                  </childNodes>
                </IKNode>
              </IKTree>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timelines>
</DOMDocument>"##;

    let tmp = TempDir::new();
    let limb = box_symbol_xml("Limb", 80, 12, "#44AA66");
    write_xfl_dir(
        &tmp.0,
        &[("DOMDocument.xml", dom), ("LIBRARY/Limb.xml", &limb)],
    );

    let report = inspect(&tmp.0).unwrap();
    assert!(report.can_import, "{:?}", report.diagnostics);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning && d.code == "xfl_ik_sampled")
            .count(),
        2,
        "both armature layers must emit xfl_ik_sampled: {:?}",
        report.diagnostics
    );

    let project = import_project(&tmp.0).unwrap();
    project.validate().unwrap();
    let tl = &project.symbol(&project.root).unwrap().timeline;

    for layer_name in ["ArmatureFK", "AdobeIkStates"] {
        let layer = tl.layers.iter().find(|l| l.name == layer_name).unwrap();
        assert_eq!(layer.keyframes.len(), 11);
        let base_x = if layer_name == "ArmatureFK" {
            100.0
        } else {
            300.0
        };
        let base_y = if layer_name == "ArmatureFK" {
            150.0
        } else {
            100.0
        };

        // Frame 5 (halfway: upper = +45 deg, lower = -45 deg local -> 0 deg world!)
        let f5 = sample_layer(layer, 5);
        assert!((f5[0].transform.rotation - 45.0).abs() < 0.1);
        let exp_elbow5_x = base_x + 80.0 * std::f64::consts::FRAC_1_SQRT_2;
        let exp_elbow5_y = base_y + 80.0 * std::f64::consts::FRAC_1_SQRT_2;
        assert!((f5[1].transform.x - exp_elbow5_x).abs() < 0.2);
        assert!((f5[1].transform.y - exp_elbow5_y).abs() < 0.2);
        assert!(f5[1].transform.rotation.abs() < 0.1);

        // Frame 10 (end: upper = +90 deg pointing down, lower = 0 deg world at (base_x, base_y + 80)!)
        let f10 = sample_layer(layer, 10);
        assert!((f10[0].transform.rotation - 90.0).abs() < 0.1);
        assert!((f10[1].transform.x - base_x).abs() < 0.2);
        assert!((f10[1].transform.y - (base_y + 80.0)).abs() < 0.2);
        assert!(f10[1].transform.rotation.abs() < 0.1);
        assert_eq!(f10[0].id, layer.keyframes[0].elements[0].id);
        assert_eq!(f10[1].id, layer.keyframes[0].elements[1].id);
    }

    // Verify full evaluation renders all 4 limb instances cleanly.
    let draws = evaluate(&project, &project.root, 5);
    assert_eq!(draws.len(), 4);
}

// --- Synthetic CFB Binary FLA helpers for Deliverables 1, 3, and 4 ---

fn u16b(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn u32b(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn i32b(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn mfc_str(out: &mut Vec<u8>, s: &str) {
    assert!(s.len() < 255);
    out.push(s.len() as u8);
    out.extend_from_slice(s.as_bytes());
}
fn mfc_utf16(out: &mut Vec<u8>, s: &str) {
    let units: Vec<u16> = s.encode_utf16().collect();
    out.extend_from_slice(&[0xff, 0xfe, 0xff]);
    if units.len() < 255 {
        out.push(units.len() as u8);
    } else {
        out.push(0xff);
        u16b(out, units.len() as u16);
    }
    for u in units {
        u16b(out, u);
    }
}
fn mfc_class(out: &mut Vec<u8>, name: &str) {
    u16b(out, 0xffff);
    u16b(out, 1);
    u16b(out, name.len() as u16);
    out.extend_from_slice(name.as_bytes());
}

struct PageWriter {
    bytes: Vec<u8>,
    classes: BTreeMap<&'static str, u16>,
    next: u16,
}

impl PageWriter {
    fn new() -> Self {
        Self {
            bytes: vec![1], // timeline stream header byte
            classes: BTreeMap::new(),
            next: 1,
        }
    }
    fn begin(&mut self, class: &'static str) -> u16 {
        if let Some(id) = self.classes.get(class).copied() {
            u16b(&mut self.bytes, 0x8000 | id);
        } else {
            mfc_class(&mut self.bytes, class);
            self.classes.insert(class, self.next);
            self.next += 1;
        }
        let id = self.next;
        self.next += 1;
        id
    }
    fn base_start(&mut self) {
        self.bytes.push(1);
        self.bytes.push(3);
    }
    fn base_end(&mut self) {
        u16b(&mut self.bytes, 0);
        i32b(&mut self.bytes, i32::MIN);
        i32b(&mut self.bytes, i32::MIN);
    }
    fn base_end_with_reg(&mut self, rx: i32, ry: i32) {
        u16b(&mut self.bytes, 0);
        i32b(&mut self.bytes, rx);
        i32b(&mut self.bytes, ry);
    }
    fn empty_shape(&mut self) {
        self.bytes.push(2);
        for n in [65536_i32, 0, 0, 65536, 0, 0] {
            i32b(&mut self.bytes, n);
        }
        self.bytes.push(5);
        self.bytes.extend_from_slice(&[0; 9]);
        u32b(&mut self.bytes, 0);
    }
}

fn build_minimal_symbol_stream() -> Vec<u8> {
    let mut sym_w = PageWriter::new();
    sym_w.begin("CPicPage");
    sym_w.base_start();
    sym_w.begin("CPicLayer");
    sym_w.base_start();
    sym_w.begin("CPicFrame");
    sym_w.base_start();
    sym_w.base_end();
    sym_w.empty_shape();
    sym_w.bytes.push(1); // frame rev 1
    u16b(&mut sym_w.bytes, 1); // duration 1
    sym_w.bytes.push(0); // key_mode 0
    sym_w.base_end();
    sym_w.bytes.push(2); // layer rev 2
    mfc_str(&mut sym_w.bytes, "SymLayer");
    sym_w.bytes.extend_from_slice(&[1, 0, 0, 0, 1]);
    sym_w.base_end();
    sym_w.bytes.push(1); // page rev 1
    u16b(&mut sym_w.bytes, 1);
    sym_w.bytes
}

fn push_cdocument_page_rev5(
    contents: &mut Vec<u8>,
    stream_name: &str,
    item_name: &str,
    library_id: u32,
    symbol_type: u32,
) {
    contents.push(5);
    mfc_str(contents, stream_name);
    mfc_str(contents, item_name);
    u32b(contents, library_id);
    contents.push(symbol_type as u8);
    u32b(contents, 3);
    u32b(contents, 0);
    u32b(contents, 0);
    u32b(contents, 0);
}

fn build_contents_with_symbol() -> Vec<u8> {
    let mut contents = Vec::new();
    contents.push(0x3f);
    u32b(&mut contents, 1);
    u32b(&mut contents, 0);
    contents.push(0);
    contents.push(0);
    for _ in [0x21, 0x2b, 0x3b] {
        u32b(&mut contents, 0);
    }
    mfc_class(&mut contents, "CDocumentPage");
    push_cdocument_page_rev5(&mut contents, "Page 1", "Scene 1", 0, 2);
    u16b(&mut contents, 0); // end scenes
    u16b(&mut contents, 1);
    u16b(&mut contents, 1);
    u16b(&mut contents, 2); // selected_scene = ref 2
    u16b(&mut contents, 0x8001); // CDocumentPage class ref 1
    push_cdocument_page_rev5(&mut contents, "Symbol 1", "HeroClip", 100, 2);
    u16b(&mut contents, 0); // end symbols
    u16b(&mut contents, 101); // library_counter
    u16b(&mut contents, 0); // end media list
    u16b(&mut contents, 1); // media_counter
    u16b(&mut contents, 0); // ruler_unit
    contents.push(0); // grid_display_mode
    for word in [0_i32, 11000, 0, 8000] {
        i32b(&mut contents, word);
    }
    u32b(&mut contents, 20 * 65536);
    contents.extend_from_slice(&[0; 5]);
    contents.extend_from_slice(&[0; 16]);
    contents.extend_from_slice(&[0; 4]);
    i32b(&mut contents, 65536);
    u16b(&mut contents, 0);
    u16b(&mut contents, 0);
    u32b(&mut contents, 0xffffffff);
    u32b(&mut contents, 0);
    u32b(&mut contents, 24 * 65536);
    contents.extend_from_slice(&[0; 3]);
    i32b(&mut contents, 20);
    contents
}

fn push_cpic_symbol_rev7(w: &mut PageWriter, library_id: u32, tx_twips: i32, ty_twips: i32) {
    w.begin("CPicSymbol");
    w.base_start();
    w.base_end_with_reg(tx_twips, ty_twips);
    w.bytes.push(7); // CPicSymbol rev 7
    for m in [65536_i32, 0, 0, 65536, tx_twips, ty_twips] {
        i32b(&mut w.bytes, m);
    }
    u16b(&mut w.bytes, 0); // first_frame
    u16b(&mut w.bytes, 0); // loop_mode
    w.bytes.push(1); // color_transform rev 1
    for mv in [256_i16, 0, 256, 0, 256, 0, 256, 0] {
        u16b(&mut w.bytes, mv as u16);
    }
    u16b(&mut w.bytes, 0);
    u16b(&mut w.bytes, 0);
    u32b(&mut w.bytes, 0);
    mfc_str(&mut w.bytes, ""); // legacy_name
    u32b(&mut w.bytes, library_id);
}

fn write_cfb_file(path: &Path, streams: &[(&str, &[u8])]) {
    let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    for (name, bytes) in streams {
        cfb.create_stream(name).unwrap().write_all(bytes).unwrap();
    }
    fs::write(path, cfb.into_inner().into_inner()).unwrap();
}

#[test]
fn binary_fla_motion_xml_and_ik_pose_matrices_sample_into_validated_frames() {
    let sym_bytes = build_minimal_symbol_stream();
    let contents = build_contents_with_symbol();

    let mut page = PageWriter::new();
    page.begin("CPicPage");
    page.base_start();

    // Layer 1: CPicFrame rev 28 with CS4 AnimationCore XML (skew + 3D + tint + S-curve spline)!
    page.begin("CPicLayer");
    page.base_start();
    page.begin("CPicFrame");
    page.base_start();
    push_cpic_symbol_rev7(&mut page, 100, 2000, 2000);
    page.base_end();
    page.empty_shape();
    let motion_xml = r#"<AnimationCore TimeScale="24000" Version="1" duration="5000"><TimeMap strength="0" type="Linear"/><metadata><Settings orientToPath="0" xformPtZOffsetPixels="0"/></metadata><PropertyContainer id="headContainer"><Property enabled="1" id="Motion_X"><Keyframe timevalue="0" anchor="0,0" next="3500,0" previous="0,0"/><Keyframe timevalue="4000" anchor="0,160" next="0,160" previous="-3500,160"/></Property><Property enabled="1" id="Skew_X"><Keyframe timevalue="0" anchor="0,0" next="0,0" previous="0,0"/><Keyframe timevalue="4000" anchor="0,20" next="0,20" previous="0,20"/></Property><Property enabled="1" id="Rotation_Y"><Keyframe timevalue="0" anchor="0,0" next="0,0" previous="0,0"/><Keyframe timevalue="4000" anchor="0,35" next="0,35" previous="0,35"/></Property><Property enabled="1" id="Motion_Z"><Keyframe timevalue="0" anchor="0,0" next="0,0" previous="0,0"/><Keyframe timevalue="4000" anchor="0,200" next="0,200" previous="0,200"/></Property><Property enabled="1" id="Tint_Color"><Keyframe timevalue="0" value="0x00FF00FF"/><Keyframe timevalue="4000" value="0x00FF00FF"/></Property><Property enabled="1" id="Tint_Amount"><Keyframe timevalue="0" anchor="0,0" next="0,0" previous="0,0"/><Keyframe timevalue="4000" anchor="0,50" next="0,50" previous="0,50"/></Property></PropertyContainer></AnimationCore>"#;
    page.bytes.push(28); // CPicFrame rev 28
    u16b(&mut page.bytes, 5); // duration = 5
    u16b(&mut page.bytes, 3); // key_mode = 3 (motion object)
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 1);
    page.bytes.push(0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    mfc_utf16(&mut page.bytes, ""); // label
    u32b(&mut page.bytes, 0); // actions rev 0
    u32b(&mut page.bytes, 2); // actions empty
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0); // morph = null
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0); // sound_settings = null
    mfc_str(&mut page.bytes, "");
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 1);
    u32b(&mut page.bytes, 0);
    // rev >= 27 && kind == 3: motion_xml (string), visible_animation_keyframes (u32),
    // rev >= 28 && kind == 3: tween_instance_name (string)
    mfc_utf16(&mut page.bytes, motion_xml);
    u32b(&mut page.bytes, 0);
    mfc_str(&mut page.bytes, "");
    page.base_end(); // end Layer 1
    page.bytes.push(2);
    mfc_str(&mut page.bytes, "Motion3DLayer");
    page.bytes.extend_from_slice(&[1, 0, 0, 0, 1]);

    // Layer 2: CPicFrame rev 29 with `key_mode = 4` (IK pose) and 2 elements x 4 frames `pose_matrices`!
    page.begin("CPicLayer");
    page.base_start();
    page.begin("CPicFrame");
    page.base_start();
    push_cpic_symbol_rev7(&mut page, 100, 1000, 3000);
    push_cpic_symbol_rev7(&mut page, 100, 2600, 3000);
    page.base_end();
    page.empty_shape();
    page.bytes.push(29); // CPicFrame rev 29
    u16b(&mut page.bytes, 4); // duration = 4
    u16b(&mut page.bytes, 4); // key_mode = 4 (IK pose)
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 1);
    page.bytes.push(0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    mfc_utf16(&mut page.bytes, "");
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 2);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    mfc_str(&mut page.bytes, "");
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 1);
    u32b(&mut page.bytes, 0);
    // bone_count = 2 -> 2 * 4 = 8 matrices (element-major)
    u32b(&mut page.bytes, 2);
    for step in 0..4_i32 {
        for m in [65536_i32, 0, 0, 65536, step * 400, 0] {
            i32b(&mut page.bytes, m);
        }
    }
    let elem1_mats = [
        [65536_i32, 0, 0, 65536, 0, 0],
        [46341, 46341, -46341, 46341, 200, 100],
        [0, 65536, -65536, 0, 400, 200],
        [0, 65536, -65536, 0, 600, 400],
    ];
    for words in elem1_mats {
        for m in words {
            i32b(&mut page.bytes, m);
        }
    }
    page.base_end(); // end Layer 2
    page.bytes.push(2);
    mfc_str(&mut page.bytes, "IkPoseLayer");
    page.bytes.extend_from_slice(&[1, 0, 0, 0, 1]);

    page.base_end(); // end CPicPage
    page.bytes.push(1);
    u16b(&mut page.bytes, 1);

    let tmp = TempDir::new();
    let fla_path = tmp.0.join("cs4_motion_and_ik.fla");
    write_cfb_file(
        &fla_path,
        &[
            ("Contents", &contents),
            ("Page 1", &page.bytes),
            ("Symbol 1", &sym_bytes),
        ],
    );

    let rep = inspect(&fla_path).unwrap();
    assert!(rep.can_import, "{:?}", rep.diagnostics);
    assert!(rep.diagnostics.iter().any(|d| d.code == "fla_motion_xml"));
    assert!(rep.diagnostics.iter().any(|d| d.code == "fla_3d_transform"));
    assert!(
        rep.diagnostics
            .iter()
            .any(|d| d.code == "fla_layer_animation")
    );

    let project = import_project(&fla_path).unwrap();
    project.validate().unwrap();
    let tl = &project.symbol(&project.root).unwrap().timeline;

    let m_layer = tl
        .layers
        .iter()
        .find(|l| l.name == "Motion3DLayer")
        .unwrap();
    assert_eq!(m_layer.keyframes.len(), 5);
    let m_end = &m_layer.keyframes[4].elements[0].transform;
    assert!(
        m_end.scale_x < 0.85,
        "3D Motion_Z + Rotation_Y must foreshorten: {m_end:?}"
    );
    assert!((m_end.color_transform.multipliers[0] - 0.5).abs() < 1e-3);
    assert!((m_end.color_transform.offsets[1] - 127.5).abs() < 0.5);

    let ik_layer = tl.layers.iter().find(|l| l.name == "IkPoseLayer").unwrap();
    // 4 baked IK pose keyframes (0..=3) + 1 blank span-end keyframe at frame 4 (< timeline.frame_count 5).
    assert_eq!(ik_layer.keyframes.len(), 5);
    assert!(ik_layer.keyframes[4].elements.is_empty());
    let ik_f3 = &ik_layer.keyframes[3].elements;
    assert!((ik_f3[0].transform.x - (50.0 + 60.0)).abs() < 1e-3);
    assert!((ik_f3[1].transform.rotation - 90.0).abs() < 0.2);
    assert_eq!(ik_f3[0].id, ik_layer.keyframes[0].elements[0].id);
    assert_eq!(ik_f3[1].id, ik_layer.keyframes[0].elements[1].id);
}

#[test]
fn binary_fla_single_line_point_text_ignores_inert_negative_leading_while_multiline_warns() {
    let build_text_fla = |auto_expand: u8, text_str: &str, line_spacing_twips: i16| -> Vec<u8> {
        let contents = build_contents_with_symbol();
        let sym_bytes = build_minimal_symbol_stream();
        let mut page = PageWriter::new();
        page.begin("CPicPage");
        page.base_start();
        page.begin("CPicLayer");
        page.base_start();
        page.begin("CPicFrame");
        page.base_start();

        // CPicText rev 2 + style rev 3
        page.begin("CPicText");
        page.base_start();
        page.base_end();
        page.bytes.push(2); // CPicText rev 2
        for m in [65536_i32, 0, 0, 65536, 400, 400] {
            i32b(&mut page.bytes, m);
        }
        for b in [0_i32, 1200, 0, 300] {
            i32b(&mut page.bytes, b);
        }
        page.bytes.push(auto_expand);
        // 1 run: count (u16), text_style (rev 3), raw bytes, then 0_u16 end-of-runs
        u16b(&mut page.bytes, text_str.len() as u16);
        page.bytes.push(3); // text_style rev 3
        u16b(&mut page.bytes, 180); // 9pt (180 twips)
        mfc_str(&mut page.bytes, "Arial");
        u32b(&mut page.bytes, 0xff000000);
        page.bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]); // pitch, charset, bold, italic, screen_sp, kern, char_pos
        page.bytes.push(0); // left align
        page.bytes
            .extend_from_slice(&line_spacing_twips.to_le_bytes());
        u16b(&mut page.bytes, 0); // indent
        u16b(&mut page.bytes, 0); // left_margin
        u16b(&mut page.bytes, 0); // right_margin
        u16b(&mut page.bytes, 0); // letter_spacing
        page.bytes.extend_from_slice(text_str.as_bytes());
        u16b(&mut page.bytes, 0); // end of runs

        page.base_end(); // Frame
        page.empty_shape();
        page.bytes.push(1); // frame rev 1
        u16b(&mut page.bytes, 1); // duration 1
        page.bytes.push(0); // key_mode 0
        page.base_end(); // Layer
        page.bytes.push(2);
        mfc_str(&mut page.bytes, "TextLayer");
        page.bytes.extend_from_slice(&[1, 0, 0, 0, 1]);
        page.base_end(); // Page
        page.bytes.push(1);
        u16b(&mut page.bytes, 1);

        let mut cfb = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        cfb.create_stream("Contents")
            .unwrap()
            .write_all(&contents)
            .unwrap();
        cfb.create_stream("Page 1")
            .unwrap()
            .write_all(&page.bytes)
            .unwrap();
        cfb.create_stream("Symbol 1")
            .unwrap()
            .write_all(&sym_bytes)
            .unwrap();
        cfb.into_inner().into_inner()
    };

    let tmp = TempDir::new();
    // 1. Single-line auto-expanding Static Point Text (`auto_expand = 1`, no newlines, `-13pt` leading on `9pt` font)
    let single_path = tmp.0.join("single_point_text.fla");
    fs::write(&single_path, build_text_fla(1, "A", -260)).unwrap();
    let single_rep = inspect(&single_path).unwrap();
    assert!(
        single_rep
            .diagnostics
            .iter()
            .all(|d| d.code != "fla_text_approximated"),
        "single-line Point Text with inert negative leading must not emit fla_text_approximated: {:?}",
        single_rep.diagnostics
    );
    let single_proj = import_project(&single_path).unwrap();
    single_proj.validate().unwrap();
    let single_el = &single_proj
        .symbol(&single_proj.root)
        .unwrap()
        .timeline
        .layers[0]
        .keyframes[0]
        .elements[0];
    match &single_el.kind {
        ElementKind::TextBox {
            format, multiline, ..
        } => {
            assert!(!multiline);
            assert_eq!(format.leading, 0.0);
        }
        other => panic!("expected TextBox, got {other:?}"),
    }

    // 2. Multi-line Static Text (`Line 1\rLine 2`, `-13pt` leading on `9pt` font) MUST still warn!
    let multi_path = tmp.0.join("multiline_text.fla");
    fs::write(&multi_path, build_text_fla(1, "Line 1\rLine 2", -260)).unwrap();
    let multi_rep = inspect(&multi_path).unwrap();
    assert!(
        multi_rep
            .diagnostics
            .iter()
            .any(|d| d.code == "fla_text_approximated" && d.message.contains("clamped")),
        "multi-line text with unrepresentable negative leading must emit fla_text_approximated: {:?}",
        multi_rep.diagnostics
    );
    let multi_proj = import_project(&multi_path).unwrap();
    multi_proj.validate().unwrap();
}

#[test]
fn xfl_between_frame_matrix_list_and_animation_core_filter_curves_sample_cleanly() {
    let dom = r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="550" height="400" frameRate="24" xflVersion="2.2">
  <symbols>
    <Include href="Part.xml"/>
  </symbols>
  <timelines>
    <DOMTimeline name="Scene 1">
      <layers>
        <DOMLayer name="IkMatrixList" animationType="IK pose">
          <frames>
            <DOMFrame index="0" duration="3" tweenType="IK pose" isIKPose="true">
              <elements>
                <DOMSymbolInstance libraryItemName="Part" name="p0" referenceID="A">
                  <matrix><Matrix tx="100" ty="0"/></matrix>
                </DOMSymbolInstance>
                <DOMSymbolInstance libraryItemName="Part" name="p1" referenceID="B">
                  <matrix><Matrix tx="200" ty="50"/></matrix>
                </DOMSymbolInstance>
              </elements>
              <betweenFrameMatrixList>
                <Matrix/>
                <Matrix a="0" b="1" c="-1" d="0" tx="0" ty="0"/>
                <Matrix tx="40" ty="10"/>
                <Matrix/>
                <Matrix tx="15" ty="-5"/>
                <Matrix tx="30" ty="-10"/>
              </betweenFrameMatrixList>
            </DOMFrame>
          </frames>
        </DOMLayer>
        <DOMLayer name="MotionFilters" animationType="motion object">
          <frames>
            <DOMFrame index="0" duration="3" tweenType="motion object">
              <MotionObjectXML>
                <AnimationCore TimeScale="24000" Version="1" duration="3000">
                  <PropertyContainer id="headContainer">
                    <Property enabled="1" id="Motion_X">
                      <Keyframe timevalue="0" anchor="0,0"/>
                      <Keyframe timevalue="2000" anchor="0,30"/>
                    </Property>
                    <PropertyContainer id="Filters">
                      <PropertyContainer id="Blur_Filter">
                        <Property enabled="1" id="Blur_BlurX"><Keyframe timevalue="0" anchor="0,4"/><Keyframe timevalue="2000" anchor="0,20"/></Property>
                        <Property enabled="1" id="Blur_BlurY"><Keyframe timevalue="0" anchor="0,2"/><Keyframe timevalue="2000" anchor="0,10"/></Property>
                        <Property enabled="1" id="Blur_Quality" value="2"/>
                      </PropertyContainer>
                      <PropertyContainer id="GradientGlow_Filter">
                        <Property enabled="1" id="GradientGlow_BlurX"><Keyframe timevalue="0" anchor="0,5"/><Keyframe timevalue="2000" anchor="0,15"/></Property>
                      </PropertyContainer>
                    </PropertyContainer>
                  </PropertyContainer>
                </AnimationCore>
              </MotionObjectXML>
              <elements>
                <DOMSymbolInstance libraryItemName="Part" name="flt_part">
                  <matrix><Matrix tx="20" ty="20"/></matrix>
                  <transformationPoint><Point x="0" y="0"/></transformationPoint>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timelines>
</DOMDocument>"##;

    let tmp = TempDir::new();
    let part = box_symbol_xml("Part", 30, 10, "#CC5522");
    write_xfl_dir(
        &tmp.0,
        &[("DOMDocument.xml", dom), ("LIBRARY/Part.xml", &part)],
    );

    let rep = inspect(&tmp.0).unwrap();
    assert!(rep.can_import, "{:?}", rep.diagnostics);
    assert!(rep.diagnostics.iter().any(|d| d.code == "xfl_ik_sampled"));
    assert!(
        rep.diagnostics
            .iter()
            .all(|d| d.code != "xfl_morph_hints_ignored"),
        "IK betweenFrameMatrixList must not emit xfl_morph_hints_ignored: {:?}",
        rep.diagnostics
    );
    assert!(
        rep.diagnostics
            .iter()
            .any(|d| d.code == "xfl_filters_approximated")
    );

    let proj = import_project(&tmp.0).unwrap();
    proj.validate().unwrap();
    let tl = &proj.symbol(&proj.root).unwrap().timeline;

    let ik_layer = tl.layers.iter().find(|l| l.name == "IkMatrixList").unwrap();
    assert_eq!(ik_layer.keyframes.len(), 3);
    // Frame 1: quarter-turn in parent space maps p0 from (100, 0) -> (0, 100)!
    let f1_p0 = &ik_layer.keyframes[1].elements[0].transform;
    assert!((f1_p0.x - 0.0).abs() < 1e-4 && (f1_p0.y - 100.0).abs() < 1e-4);
    assert!((f1_p0.rotation - 90.0).abs() < 1e-3);
    let f2_p1 = &ik_layer.keyframes[2].elements[1].transform;
    assert!((f2_p1.x - 230.0).abs() < 1e-4 && (f2_p1.y - 40.0).abs() < 1e-4);

    let mf_layer = tl
        .layers
        .iter()
        .find(|l| l.name == "MotionFilters")
        .unwrap();
    match mf_layer.keyframes[2].elements[0].filters.first() {
        Some(FilterSpec::Blur { blur_x, blur_y, .. }) => {
            assert!((*blur_x - 20.0).abs() < 1e-4 && (*blur_y - 10.0).abs() < 1e-4);
        }
        other => panic!("expected animated Blur filter at frame 2, got {other:?}"),
    }
}

#[test]
fn xfl_custom_ease_branch_cut_rotation_and_orient_to_path_guard() {
    let mat_rot = |deg: f64, tx: f64, ty: f64| -> String {
        let (s, c) = deg.to_radians().sin_cos();
        format!(
            r#"<Matrix a="{c:.8}" b="{s:.8}" c="{ms:.8}" d="{c:.8}" tx="{tx}" ty="{ty}"/>"#,
            ms = -s
        )
    };
    let m_pos170 = mat_rot(170.0, 50.0, 100.0);
    let m_neg170 = mat_rot(-170.0, 250.0, 100.0);
    let m_neg150 = mat_rot(-150.0, 350.0, 100.0);
    let m_pos45_a = mat_rot(45.0, 20.0, 200.0);
    let m_pos45_b = mat_rot(45.0, 220.0, 200.0);

    let dom = format!(
        r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="550" height="400" frameRate="24" xflVersion="2.2">
  <symbols>
    <Include href="SpinBox.xml"/>
  </symbols>
  <timelines>
    <DOMTimeline name="Scene 1">
      <layers>
        <DOMLayer name="SubdividedBranchCut">
          <frames>
            <DOMFrame index="0" duration="10" tweenType="motion" hasCustomEase="true" useSingleEaseCurve="true">
              <tweens>
                <CustomEase target="all">
                  <Point x="0" y="0"/>
                  <Point x="0.2" y="0.2"/>
                  <Point x="0.3" y="0.3"/>
                  <Point x="0.5" y="0.5"/>
                  <Point x="0.7" y="0.7"/>
                  <Point x="0.8" y="0.8"/>
                  <Point x="1" y="1"/>
                </CustomEase>
              </tweens>
              <elements>
                <DOMSymbolInstance libraryItemName="SpinBox" name="sb1">
                  <matrix>{m_pos170}</matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
            <DOMFrame index="10" duration="1">
              <elements>
                <DOMSymbolInstance libraryItemName="SpinBox" name="sb1">
                  <matrix>{m_neg170}</matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>

        <DOMLayer name="ChainedBranchCut">
          <frames>
            <DOMFrame index="0" duration="6" tweenType="motion" hasCustomEase="true" useSingleEaseCurve="true">
              <tweens>
                <CustomEase target="all">
                  <Point x="0" y="0"/>
                  <Point x="0.15" y="0.15"/>
                  <Point x="0.35" y="0.35"/>
                  <Point x="0.5" y="0.5"/>
                  <Point x="0.65" y="0.65"/>
                  <Point x="0.85" y="0.85"/>
                  <Point x="1" y="1"/>
                </CustomEase>
              </tweens>
              <elements>
                <DOMSymbolInstance libraryItemName="SpinBox" name="sb2">
                  <matrix>{m_pos170}</matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
            <DOMFrame index="6" duration="4" tweenType="motion">
              <elements>
                <DOMSymbolInstance libraryItemName="SpinBox" name="sb2">
                  <matrix>{m_neg170}</matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
            <DOMFrame index="10" duration="1">
              <elements>
                <DOMSymbolInstance libraryItemName="SpinBox" name="sb2">
                  <matrix>{m_neg150}</matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>

        <DOMLayer name="UnguidedOrientPerProp">
          <frames>
            <DOMFrame index="0" duration="10" tweenType="motion" motionTweenOrientToPath="true" hasCustomEase="true" useSingleEaseCurve="false">
              <tweens>
                <CustomEase target="position">
                  <Point x="0" y="0"/>
                  <Point x="0.1" y="0.8"/>
                  <Point x="0.3" y="0.95"/>
                  <Point x="1" y="1"/>
                </CustomEase>
              </tweens>
              <elements>
                <DOMSymbolInstance libraryItemName="SpinBox" name="sb3">
                  <matrix>{m_pos45_a}</matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
            <DOMFrame index="10" duration="1">
              <elements>
                <DOMSymbolInstance libraryItemName="SpinBox" name="sb3">
                  <matrix>{m_pos45_b}</matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timelines>
</DOMDocument>"##
    );

    let tmp = TempDir::new();
    let spin_sym = box_symbol_xml("SpinBox", 30, 10, "#3377DD");
    write_xfl_dir(
        &tmp.0,
        &[
            ("DOMDocument.xml", &dom),
            ("LIBRARY/SpinBox.xml", &spin_sym),
        ],
    );

    let proj = import_project(&tmp.0).unwrap();
    proj.validate().unwrap();
    let tl = &proj.symbol(&proj.root).unwrap().timeline;

    // 1. SubdividedBranchCut: +170 -> +180 -> +190 (never spins -350 backwards!)
    let sub_layer = tl
        .layers
        .iter()
        .find(|l| l.name == "SubdividedBranchCut")
        .unwrap();
    assert_eq!(sub_layer.keyframes.len(), 3);
    for frame in 0..=10_u32 {
        let expected = 170.0 + 2.0 * f64::from(frame);
        let actual = sample_layer(sub_layer, frame)[0].transform.rotation;
        assert!(
            (actual - expected).abs() < 0.25,
            "frame {frame}: expected smooth branch-cut rotation {expected}, got {actual}"
        );
    }

    // 2. ChainedBranchCut: bails out to baking on 0..6 so frame 6..10 still tweens -170 -> -150 cleanly!
    let ch_layer = tl
        .layers
        .iter()
        .find(|l| l.name == "ChainedBranchCut")
        .unwrap();
    for frame in 0..=10_u32 {
        let rot = sample_layer(ch_layer, frame)[0].transform.rotation;
        let norm = ((rot + 180.0).rem_euclid(360.0)) - 180.0;
        // Must stay in [+170..+180] U [-180..-150], never near 0 deg (which a -350 spin crosses!).
        assert!(
            !(norm > -145.0 && norm < 165.0),
            "chained branch-cut spun backwards at frame {frame}: rot={rot} (norm={norm})"
        );
    }

    // 3. UnguidedOrientPerProp: unguided classic tween ignores orient_to_path and preserves 45 deg!
    let op_layer = tl
        .layers
        .iter()
        .find(|l| l.name == "UnguidedOrientPerProp")
        .unwrap();
    for frame in 0..=10_u32 {
        let rot = sample_layer(op_layer, frame)[0].transform.rotation;
        assert!(
            (rot - 45.0).abs() < 0.1,
            "unguided orient_to_path must keep 45 deg at frame {frame}, got {rot}"
        );
    }
}

#[test]
fn graphic_symbol_playback_wraps_and_advances_cleanly_on_ik_and_custom_ease() {
    let graphic_1f = r##"<DOMSymbolItem xmlns="http://ns.adobe.com/xfl/2008/" name="StaticGraphic" symbolType="graphic">
  <timeline>
    <DOMTimeline name="StaticGraphic">
      <layers>
        <DOMLayer name="Layer 1">
          <frames>
            <DOMFrame index="0" duration="1">
              <elements>
                <DOMRectangleObject objectWidth="20" objectHeight="10">
                  <fill><SolidColor color="#AA2244"/></fill>
                </DOMRectangleObject>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timeline>
</DOMSymbolItem>"##;

    let graphic_4f = r##"<DOMSymbolItem xmlns="http://ns.adobe.com/xfl/2008/" name="CycleGraphic" symbolType="graphic">
  <timeline>
    <DOMTimeline name="CycleGraphic">
      <layers>
        <DOMLayer name="Layer 1">
          <frames>
            <DOMFrame index="0" duration="1"><elements><DOMRectangleObject objectWidth="10" objectHeight="10"><fill><SolidColor color="#112233"/></fill></DOMRectangleObject></elements></DOMFrame>
            <DOMFrame index="1" duration="1"><elements><DOMRectangleObject objectWidth="20" objectHeight="10"><fill><SolidColor color="#224466"/></fill></DOMRectangleObject></elements></DOMFrame>
            <DOMFrame index="2" duration="1"><elements><DOMRectangleObject objectWidth="30" objectHeight="10"><fill><SolidColor color="#336699"/></fill></DOMRectangleObject></elements></DOMFrame>
            <DOMFrame index="3" duration="1"><elements><DOMRectangleObject objectWidth="40" objectHeight="10"><fill><SolidColor color="#4488CC"/></fill></DOMRectangleObject></elements></DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timeline>
</DOMSymbolItem>"##;

    let dom = r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="550" height="400" frameRate="24" xflVersion="2.2">
  <symbols>
    <Include href="StaticGraphic.xml"/>
    <Include href="CycleGraphic.xml"/>
  </symbols>
  <timelines>
    <DOMTimeline name="Scene 1">
      <layers>
        <DOMLayer name="IkGraphicArmature" layerType="armature">
          <DOMIKTree name="GTree">
            <bones>
              <DOMBone id="b_loop" target="g_loop_ik" x="50" y="50" length="40" rotation="0"/>
              <DOMBone id="b_once" parent="b_loop" target="g_once_ik" length="40" rotation="0"/>
              <DOMBone id="b_one" parent="b_once" target="g_static_ik" length="40" rotation="0"/>
            </bones>
          </DOMIKTree>
          <frames>
            <DOMFrame index="0" duration="9">
              <ikNode bone="b_loop" rotation="0"/>
              <ikNode bone="b_once" rotation="0"/>
              <ikNode bone="b_one" rotation="0"/>
              <elements>
                <DOMSymbolInstance libraryItemName="CycleGraphic" name="g_loop_ik" symbolType="graphic" loop="loop">
                  <matrix><Matrix tx="50" ty="50"/></matrix>
                </DOMSymbolInstance>
                <DOMSymbolInstance libraryItemName="CycleGraphic" name="g_once_ik" symbolType="graphic" loop="play once">
                  <matrix><Matrix tx="90" ty="50"/></matrix>
                </DOMSymbolInstance>
                <DOMSymbolInstance libraryItemName="StaticGraphic" name="g_static_ik" symbolType="graphic" loop="loop">
                  <matrix><Matrix tx="130" ty="50"/></matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
            <DOMFrame index="9" duration="1">
              <ikNode bone="b_loop" rotation="45"/>
              <ikNode bone="b_once" rotation="-20"/>
              <ikNode bone="b_one" rotation="10"/>
              <elements>
                <DOMSymbolInstance libraryItemName="CycleGraphic" name="g_loop_ik" symbolType="graphic" loop="loop">
                  <matrix><Matrix tx="50" ty="50"/></matrix>
                </DOMSymbolInstance>
                <DOMSymbolInstance libraryItemName="CycleGraphic" name="g_once_ik" symbolType="graphic" loop="play once">
                  <matrix><Matrix tx="90" ty="50"/></matrix>
                </DOMSymbolInstance>
                <DOMSymbolInstance libraryItemName="StaticGraphic" name="g_static_ik" symbolType="graphic" loop="loop">
                  <matrix><Matrix tx="130" ty="50"/></matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>

        <DOMLayer name="CustomEaseGraphicLoop">
          <frames>
            <DOMFrame index="0" duration="9" tweenType="motion" hasCustomEase="true" useSingleEaseCurve="false">
              <tweens>
                <CustomEase target="position">
                  <Point x="0" y="0"/>
                  <Point x="0.1" y="0.6"/>
                  <Point x="0.2" y="0.8"/>
                  <Point x="0.5" y="0.9"/>
                  <Point x="0.7" y="0.95"/>
                  <Point x="0.9" y="0.99"/>
                  <Point x="1" y="1"/>
                </CustomEase>
              </tweens>
              <elements>
                <DOMSymbolInstance libraryItemName="CycleGraphic" name="g_baked_ce" symbolType="graphic" loop="loop">
                  <matrix><Matrix tx="20" ty="250"/></matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
            <DOMFrame index="9" duration="1">
              <elements>
                <DOMSymbolInstance libraryItemName="CycleGraphic" name="g_baked_ce" symbolType="graphic" loop="loop">
                  <matrix><Matrix tx="220" ty="250"/></matrix>
                </DOMSymbolInstance>
              </elements>
            </DOMFrame>
          </frames>
        </DOMLayer>
      </layers>
    </DOMTimeline>
  </timelines>
</DOMDocument>"##;

    let tmp = TempDir::new();
    write_xfl_dir(
        &tmp.0,
        &[
            ("DOMDocument.xml", dom),
            ("LIBRARY/StaticGraphic.xml", graphic_1f),
            ("LIBRARY/CycleGraphic.xml", graphic_4f),
        ],
    );

    let rep = inspect(&tmp.0).unwrap();
    assert!(rep.can_import, "{:?}", rep.diagnostics);
    assert!(
        rep.diagnostics
            .iter()
            .all(|d| d.code != "xfl_playback_frame_clamped"
                && d.code != "ambiguous_instance_continuity"),
        "valid Graphic firstFrame=0 must never emit xfl_playback_frame_clamped or ambiguous_instance_continuity: {:?}",
        rep.diagnostics
    );

    let proj = import_project(&tmp.0).unwrap();
    proj.validate().unwrap();
    let root_tl = &proj.symbol(&proj.root).unwrap().timeline;
    let ik_g_layer = root_tl
        .layers
        .iter()
        .find(|l| l.name == "IkGraphicArmature")
        .unwrap();
    let ce_g_layer = root_tl
        .layers
        .iter()
        .find(|l| l.name == "CustomEaseGraphicLoop")
        .unwrap();

    for frame in 0..10_u32 {
        let eval_inst_frame = |layer: &neoflash_core::Layer, inst_name: &str| -> u32 {
            let eid = layer.keyframes[0]
                .elements
                .iter()
                .find(|e| e.name == inst_name)
                .unwrap()
                .id
                .as_str();
            neoflash_core::evaluate_instance_pose(&proj, &proj.root, frame, &layer.id, eid)
                .unwrap()
                .frame
        };
        assert_eq!(
            eval_inst_frame(ik_g_layer, "g_loop_ik"),
            frame % 4,
            "4-frame looping Graphic on IK armature must wrap modulo 4 at frame {frame}"
        );
        assert_eq!(
            eval_inst_frame(ik_g_layer, "g_once_ik"),
            frame.min(3),
            "4-frame play-once Graphic on IK armature must advance then clamp to 3 at frame {frame}"
        );
        assert_eq!(
            eval_inst_frame(ik_g_layer, "g_static_ik"),
            0,
            "1-frame static Graphic must stay at frame 0 at frame {frame}"
        );
        assert_eq!(
            eval_inst_frame(ce_g_layer, "g_baked_ce"),
            frame % 4,
            "4-frame looping Graphic on baked CustomEase must wrap modulo 4 at frame {frame}"
        );
    }

    // Binary FLA: 2-frame Graphic (`symbol_type = 0`) with `PlayOnce` (`loop_mode = 1`) and
    // `Loop` (`loop_mode = 0`) on a 4-frame IK `pose_matrices` span must NOT emit `fla_graphic_first_frame`!
    let mut g2_sym = PageWriter::new();
    g2_sym.begin("CPicPage");
    g2_sym.base_start();
    g2_sym.begin("CPicLayer");
    g2_sym.base_start();
    for _ in 0..2 {
        g2_sym.begin("CPicFrame");
        g2_sym.base_start();
        g2_sym.base_end();
        g2_sym.empty_shape();
        g2_sym.bytes.push(1);
        u16b(&mut g2_sym.bytes, 1);
        g2_sym.bytes.push(0);
    }
    g2_sym.base_end();
    g2_sym.bytes.push(2);
    mfc_str(&mut g2_sym.bytes, "GLayer");
    g2_sym.bytes.extend_from_slice(&[1, 0, 0, 0, 1]);
    g2_sym.base_end();
    g2_sym.bytes.push(1);
    u16b(&mut g2_sym.bytes, 1);

    let mut contents = Vec::new();
    contents.push(0x3f);
    u32b(&mut contents, 1);
    u32b(&mut contents, 0);
    contents.push(0);
    contents.push(0);
    for _ in [0x21, 0x2b, 0x3b] {
        u32b(&mut contents, 0);
    }
    mfc_class(&mut contents, "CDocumentPage");
    push_cdocument_page_rev5(&mut contents, "Page 1", "Scene 1", 0, 2);
    u16b(&mut contents, 0);
    u16b(&mut contents, 1);
    u16b(&mut contents, 1);
    u16b(&mut contents, 2);
    u16b(&mut contents, 0x8001);
    push_cdocument_page_rev5(&mut contents, "Symbol 1", "Graphic2F", 100, 0); // 0 = Graphic!
    u16b(&mut contents, 0);
    u16b(&mut contents, 101);
    u16b(&mut contents, 0);
    u16b(&mut contents, 1);
    u16b(&mut contents, 0);
    contents.push(0);
    for word in [0_i32, 11000, 0, 8000] {
        i32b(&mut contents, word);
    }
    u32b(&mut contents, 20 * 65536);
    contents.extend_from_slice(&[0; 5]);
    contents.extend_from_slice(&[0; 16]);
    contents.extend_from_slice(&[0; 4]);
    i32b(&mut contents, 65536);
    u16b(&mut contents, 0);
    u16b(&mut contents, 0);
    u32b(&mut contents, 0xffffffff);
    u32b(&mut contents, 0);
    u32b(&mut contents, 24 * 65536);
    contents.extend_from_slice(&[0; 3]);
    i32b(&mut contents, 20);

    let push_graphic_inst = |w: &mut PageWriter, tx_twips: i32, loop_mode: u16, name: &str| {
        w.begin("CPicSymbol");
        w.base_start();
        w.base_end_with_reg(tx_twips, 1000);
        w.bytes.push(7);
        for m in [65536_i32, 0, 0, 65536, tx_twips, 1000] {
            i32b(&mut w.bytes, m);
        }
        u16b(&mut w.bytes, 0); // first_frame = 0
        u16b(&mut w.bytes, loop_mode); // 0 = Loop, 1 = PlayOnce
        w.bytes.push(1);
        for mv in [256_i16, 0, 256, 0, 256, 0, 256, 0] {
            u16b(&mut w.bytes, mv as u16);
        }
        u16b(&mut w.bytes, 0);
        u16b(&mut w.bytes, 0);
        u32b(&mut w.bytes, 0);
        mfc_str(&mut w.bytes, name);
        u32b(&mut w.bytes, 100);
    };

    let mut page = PageWriter::new();
    page.begin("CPicPage");
    page.base_start();
    page.begin("CPicLayer");
    page.base_start();
    page.begin("CPicFrame");
    page.base_start();
    push_graphic_inst(&mut page, 1000, 0, "g_fla_loop");
    push_graphic_inst(&mut page, 3000, 1, "g_fla_once");
    page.base_end();
    page.empty_shape();
    page.bytes.push(29);
    u16b(&mut page.bytes, 4); // 4 frames
    u16b(&mut page.bytes, 4); // IK pose
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 1);
    page.bytes.push(0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    mfc_utf16(&mut page.bytes, "");
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 2);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u16b(&mut page.bytes, 0);
    mfc_str(&mut page.bytes, "");
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 1);
    u32b(&mut page.bytes, 0);
    u32b(&mut page.bytes, 2); // 2 elements x 4 frames
    for step in 0..4_i32 {
        for m in [65536_i32, 0, 0, 65536, step * 200, 0] {
            i32b(&mut page.bytes, m);
        }
    }
    for step in 0..4_i32 {
        for m in [65536_i32, 0, 0, 65536, 0, step * 200] {
            i32b(&mut page.bytes, m);
        }
    }
    u32b(&mut page.bytes, 0);
    mfc_str(&mut page.bytes, "");
    page.base_end();
    page.bytes.push(2);
    mfc_str(&mut page.bytes, "FlaGraphicIk");
    page.bytes.extend_from_slice(&[1, 0, 0, 0, 1]);
    page.base_end();
    page.bytes.push(1);
    u16b(&mut page.bytes, 1);

    let fla_path = tmp.0.join("graphic_ik.fla");
    write_cfb_file(
        &fla_path,
        &[
            ("Contents", &contents),
            ("Page 1", &page.bytes),
            ("Symbol 1", &g2_sym.bytes),
        ],
    );
    let fla_rep = inspect(&fla_path).unwrap();
    assert!(
        fla_rep
            .diagnostics
            .iter()
            .all(|d| d.code != "fla_graphic_first_frame"),
        "2-frame Graphic on 4-frame IK pose_matrices must not emit fla_graphic_first_frame: {:?}",
        fla_rep.diagnostics
    );
    let fla_proj = import_project(&fla_path).unwrap();
    fla_proj.validate().unwrap();
    let fla_layer = &fla_proj.symbol(&fla_proj.root).unwrap().timeline.layers[0];
    let loop_id = fla_layer.keyframes[0].elements[0].id.clone();
    let once_id = fla_layer.keyframes[0].elements[1].id.clone();
    for frame in 0..4_u32 {
        let f_loop = neoflash_core::evaluate_instance_pose(
            &fla_proj,
            &fla_proj.root,
            frame,
            &fla_layer.id,
            &loop_id,
        )
        .unwrap()
        .frame;
        let f_once = neoflash_core::evaluate_instance_pose(
            &fla_proj,
            &fla_proj.root,
            frame,
            &fla_layer.id,
            &once_id,
        )
        .unwrap()
        .frame;
        assert_eq!(f_loop, frame % 2);
        assert_eq!(f_once, frame.min(1));
    }
}

#[test]
fn xfl_motion_object_audits_attribute_namespaces_and_unknown_fl_motion_elements() {
    let box_sym = box_symbol_xml("Box", 20, 20, "#336699");

    // 1. Namespaced attribute inside a valid sampled `<MotionObjectXML><AnimationCore>` MUST fail with `unsupported_attribute_namespace`.
    let dom_ns_attr = r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="550" height="400" frameRate="24" xflVersion="2.2">
  <symbols><Include href="Box.xml"/></symbols>
  <timelines><DOMTimeline name="Scene 1"><layers><DOMLayer name="L1"><frames>
    <DOMFrame index="0" duration="3" tweenType="motion object">
      <MotionObjectXML xmlns:e="urn:unrecognized">
        <AnimationCore Version="1" TimeScale="24000" duration="3000">
          <Property id="Motion_X" enabled="1">
            <Keyframe timevalue="0" anchor="0,0" e:anchor="0,999"/>
            <Keyframe timevalue="2000" anchor="0,40"/>
          </Property>
        </AnimationCore>
      </MotionObjectXML>
      <elements><DOMSymbolInstance libraryItemName="Box"/></elements>
    </DOMFrame>
  </frames></DOMLayer></layers></DOMTimeline></timelines>
</DOMDocument>"##;
    let tmp1 = TempDir::new();
    write_xfl_dir(
        &tmp1.0,
        &[
            ("DOMDocument.xml", dom_ns_attr),
            ("LIBRARY/Box.xml", &box_sym),
        ],
    );
    let rep1 = inspect(&tmp1.0).unwrap();
    assert!(!rep1.can_import, "{:?}", rep1.diagnostics);
    assert!(
        rep1.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.code == "unsupported_attribute_namespace"),
        "{:?}",
        rep1.diagnostics
    );

    // 2. Unknown tag `<Bogus/>` inside `<Motion xmlns="fl.motion.*">` MUST fail `unknown_namespace`.
    let dom_unknown_fl_ns = r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="550" height="400" frameRate="24" xflVersion="2.2">
  <symbols><Include href="Box.xml"/></symbols>
  <timelines><DOMTimeline name="Scene 1"><layers><DOMLayer name="L1"><frames>
    <DOMFrame index="0" duration="3" tweenType="motion object">
      <DOMMotionObject>
        <Motion duration="3" xmlns="fl.motion.*">
          <Bogus/>
          <Keyframe index="0" x="0"/>
          <Keyframe index="2" x="40"/>
        </Motion>
      </DOMMotionObject>
      <elements><DOMSymbolInstance libraryItemName="Box"/></elements>
    </DOMFrame>
  </frames></DOMLayer></layers></DOMTimeline></timelines>
</DOMDocument>"##;
    let tmp2 = TempDir::new();
    write_xfl_dir(
        &tmp2.0,
        &[
            ("DOMDocument.xml", dom_unknown_fl_ns),
            ("LIBRARY/Box.xml", &box_sym),
        ],
    );
    let rep2 = inspect(&tmp2.0).unwrap();
    assert!(!rep2.can_import, "{:?}", rep2.diagnostics);
    assert!(
        rep2.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.code == "unknown_namespace"),
        "{:?}",
        rep2.diagnostics
    );

    // 3. Unknown tag `<Bogus/>` inside unnamespaced `<Motion>` / `<Keyframe>` MUST emit `motion_object_not_supported` (`unknown motion XML element`).
    let dom_unknown_unns = r##"<DOMDocument xmlns="http://ns.adobe.com/xfl/2008/" width="550" height="400" frameRate="24" xflVersion="2.2">
  <symbols><Include href="Box.xml"/></symbols>
  <timelines><DOMTimeline name="Scene 1"><layers><DOMLayer name="L1"><frames>
    <DOMFrame index="0" duration="3" tweenType="motion object">
      <DOMMotionObject>
        <Motion duration="3">
          <Keyframe index="0" x="0"><Bogus/></Keyframe>
          <Keyframe index="2" x="40"/>
        </Motion>
      </DOMMotionObject>
      <elements><DOMSymbolInstance libraryItemName="Box"/></elements>
    </DOMFrame>
  </frames></DOMLayer></layers></DOMTimeline></timelines>
</DOMDocument>"##;
    let tmp3 = TempDir::new();
    write_xfl_dir(
        &tmp3.0,
        &[
            ("DOMDocument.xml", dom_unknown_unns),
            ("LIBRARY/Box.xml", &box_sym),
        ],
    );
    let rep3 = inspect(&tmp3.0).unwrap();
    assert!(
        rep3.diagnostics.iter().any(|d| {
            d.code == "motion_object_not_supported"
                && d.message.contains("unknown motion XML element")
        }),
        "expected motion_object_not_supported(unknown motion XML element): {:?}",
        rep3.diagnostics
    );
}
