//! End-to-end tests for T-501 true-vector mesh gradients (zero bitmap rasterization):
//! - PDF / Adobe Illustrator `.ai` `/ShadingType 4` (Free-Form Gouraud), `/ShadingType 5`
//!   (Lattice Gouraud), `/ShadingType 6` (Coons 12-point patch mesh with edge sharing), and
//!   `/ShadingType 7` (Tensor-Product 16-point bicubic Bézier patch mesh) via both `sh` and
//!   `/PatternType 2` (`scn`/`SCN`)
//! - PostScript Level 3 / EPS `ShadingType 4..7` `shfill`
//! - SVG2 `<meshgradient>` (`<meshrow>`/`<meshpatch>`, shared-edge `4/3/3/2` stop layout)
//! - Runtime scene & snapshot evaluation (`DrawItem.mesh_fill`, `RuntimeDrawItem.mesh_fill`)
//! - Lossless vector round-trip across PDF (`export_project_image` + `export_clipboard_pdf`),
//!   AI/EPS (`export_project_image` + `export_clipboard_eps`), and SVG (`render_frame_svg`),
//!   asserting `0` bitmap elements and `0` raster `ResourceUnit`s at every stage.

use neoflash_core::{
    ColorTransform, ElementKind, Player, Project, VectorMeshPaint, evaluate_scene,
};
use neoflash_export::{
    ImageExportFormat, ImageExportOptions, SvgOptions, export_clipboard_eps, export_clipboard_pdf,
    export_project_image, render_authored_scene_rgba, render_frame_svg,
};
use neoflash_import::{
    VectorImportOptions, import_pdf_ai_with_options, import_svg_with_options,
    import_vector_bytes_with_options,
};
use std::collections::HashMap;

fn assert_pure_vector_mesh_project(project: &Project, label: &str, min_meshes: usize) {
    project
        .validate()
        .unwrap_or_else(|e| panic!("{label}: project failed validation: {e}"));
    assert!(
        project.resources.is_empty(),
        "{label}: vector mesh import must create ZERO raster resources, got {}",
        project.resources.len()
    );
    let mut bitmap_elements = 0usize;
    let mut mesh_effects = 0usize;
    for sym in &project.symbols {
        for layer in &sym.timeline.layers {
            for kf in &layer.keyframes {
                assert!(
                    kf.bitmap_fills.is_empty(),
                    "{label}: vector mesh import must attach ZERO bitmap_fills"
                );
                for el in &kf.elements {
                    if matches!(el.kind, ElementKind::Bitmap { .. }) {
                        bitmap_elements += 1;
                    }
                }
                for fx in &kf.vector_effects {
                    if let Some(vm) = &fx.vector_mesh {
                        assert!(
                            !vm.is_empty(),
                            "{label}: vector_mesh on {} must be non-empty",
                            fx.element_id
                        );
                        mesh_effects += 1;
                    }
                }
            }
        }
    }
    assert_eq!(
        bitmap_elements, 0,
        "{label}: vector mesh import must produce ZERO ElementKind::Bitmap elements"
    );
    assert!(
        mesh_effects >= min_meshes,
        "{label}: expected at least {min_meshes} vector_mesh entries, got {mesh_effects}"
    );
}

fn build_pdf_with_shading_stream(
    shading_dict_entries: &str,
    raw_stream: &[u8],
    content_ops: &str,
) -> Vec<u8> {
    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets = [0usize; 6];

    offsets[1] = pdf.len();
    pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    offsets[2] = pdf.len();
    pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");

    offsets[3] = pdf.len();
    pdf.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
          /Resources << /Shading << /Sh0 5 0 R >> /Pattern << /P0 << /Type /Pattern /PatternType 2 /Shading 5 0 R >> >> >> \
          /Contents 4 0 R >>\nendobj\n",
    );

    offsets[4] = pdf.len();
    let c_hdr = format!(
        "4 0 obj\n<< /Length {} >>\nstream\n{content_ops}\nendstream\nendobj\n",
        content_ops.len() + 1
    );
    pdf.extend_from_slice(c_hdr.as_bytes());

    offsets[5] = pdf.len();
    let s_hdr = format!(
        "5 0 obj\n<< {shading_dict_entries} /Length {} >>\nstream\n",
        raw_stream.len()
    );
    pdf.extend_from_slice(s_hdr.as_bytes());
    pdf.extend_from_slice(raw_stream);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");

    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for off in &offsets[1..6] {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}

fn push_u16_coord(buf: &mut Vec<u8>, v: f64, lo: f64, hi: f64) {
    let q = (((v - lo) / (hi - lo)).clamp(0.0, 1.0) * 65535.0).round() as u16;
    buf.extend_from_slice(&q.to_be_bytes());
}

#[test]
fn pdf_and_ai_shading_types_4_5_6_7_import_as_pure_vector_meshes_and_render() {
    // 1. ShadingType 4: Free-Form Gouraud Triangle Mesh (flag=0 triangle + flag=1 continuation)
    let mut raw4 = Vec::new();
    let v4 = [
        (0u8, [20.0, 20.0], [255u8, 20, 40]),
        (0u8, [180.0, 20.0], [20, 240, 60]),
        (0u8, [100.0, 160.0], [30, 60, 255]),
        (1u8, [180.0, 180.0], [255, 220, 20]),
    ];
    for (flag, [x, y], rgb) in v4 {
        raw4.push(flag);
        push_u16_coord(&mut raw4, x, 0.0, 200.0);
        push_u16_coord(&mut raw4, y, 0.0, 200.0);
        raw4.extend_from_slice(&rgb);
    }
    let pdf4 = build_pdf_with_shading_stream(
        "/ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 16 /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 200 0 200 0 1 0 1 0 1]",
        &raw4,
        "q 10 10 180 180 re W n /Sh0 sh Q",
    );
    let (proj4, _) =
        import_pdf_ai_with_options(&pdf4, "GouraudTri", &VectorImportOptions::default())
            .expect("ShadingType 4 PDF import");
    assert_pure_vector_mesh_project(&proj4, "PDF ShadingType 4", 1);
    let scene4 = evaluate_scene(&proj4, &proj4.root, 0, false, true);
    assert_eq!(scene4.draws.len(), 1);
    let mf4 = scene4.draws[0]
        .item
        .mesh_fill
        .as_ref()
        .expect("DrawItem.mesh_fill populated for ShadingType 4");
    assert_eq!(mf4.triangles.len(), 2);

    // 2. ShadingType 5: Lattice-Form Gouraud Triangle Mesh (2x2 grid = 1 quad = 2 triangles)
    let mut raw5 = Vec::new();
    let v5 = [
        ([10.0, 10.0], [255u8, 0, 0]),
        ([190.0, 10.0], [0, 255, 0]),
        ([10.0, 190.0], [0, 0, 255]),
        ([190.0, 190.0], [255, 255, 0]),
    ];
    for ([x, y], rgb) in v5 {
        push_u16_coord(&mut raw5, x, 0.0, 200.0);
        push_u16_coord(&mut raw5, y, 0.0, 200.0);
        raw5.extend_from_slice(&rgb);
    }
    let pdf5 = build_pdf_with_shading_stream(
        "/ShadingType 5 /ColorSpace /DeviceRGB /BitsPerCoordinate 16 /BitsPerComponent 8 /VerticesPerRow 2 /Decode [0 200 0 200 0 1 0 1 0 1]",
        &raw5,
        "/Pattern cs /P0 scn 10 10 180 180 re f",
    );
    let (proj5, _) =
        import_pdf_ai_with_options(&pdf5, "LatticeMesh", &VectorImportOptions::default())
            .expect("ShadingType 5 PDF PatternType 2 import");
    assert_pure_vector_mesh_project(&proj5, "PDF ShadingType 5 PatternType 2", 1);
    let scene5 = evaluate_scene(&proj5, &proj5.root, 0, false, true);
    assert_eq!(
        scene5.draws[0]
            .item
            .mesh_fill
            .as_ref()
            .unwrap()
            .triangles
            .len(),
        2
    );

    // 3. ShadingType 6: Coons Patch Mesh (flag=0 12-pt patch + flag=1 shared-edge 8-pt patch)
    let mut raw6 = Vec::new();
    let patch6_0: [[f64; 2]; 12] = [
        [20.0, 20.0],
        [20.0, 60.0],
        [20.0, 100.0],
        [20.0, 140.0],
        [50.0, 145.0],
        [75.0, 145.0],
        [100.0, 140.0],
        [100.0, 100.0],
        [100.0, 60.0],
        [100.0, 20.0],
        [75.0, 15.0],
        [50.0, 15.0],
    ];
    raw6.push(0u8);
    for [x, y] in patch6_0 {
        push_u16_coord(&mut raw6, x, 0.0, 200.0);
        push_u16_coord(&mut raw6, y, 0.0, 200.0);
    }
    raw6.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0]);
    let patch6_1_rem: [[f64; 2]; 8] = [
        [50.0, 180.0],
        [75.0, 185.0],
        [100.0, 190.0],
        [140.0, 190.0],
        [160.0, 190.0],
        [180.0, 140.0],
        [150.0, 142.0],
        [125.0, 142.0],
    ];
    raw6.push(1u8);
    for [x, y] in patch6_1_rem {
        push_u16_coord(&mut raw6, x, 0.0, 200.0);
        push_u16_coord(&mut raw6, y, 0.0, 200.0);
    }
    raw6.extend_from_slice(&[255, 0, 255, 0, 255, 255]);
    let pdf6 = build_pdf_with_shading_stream(
        "/ShadingType 6 /ColorSpace /DeviceRGB /BitsPerCoordinate 16 /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 200 0 200 0 1 0 1 0 1]",
        &raw6,
        "q 15 15 175 180 re W n /Sh0 sh Q",
    );
    let (proj6, _) =
        import_pdf_ai_with_options(&pdf6, "CoonsMesh", &VectorImportOptions::default())
            .expect("ShadingType 6 Coons PDF import");
    assert_pure_vector_mesh_project(&proj6, "PDF ShadingType 6", 1);
    let scene6 = evaluate_scene(&proj6, &proj6.root, 0, false, true);
    assert_eq!(
        scene6.draws[0]
            .item
            .mesh_fill
            .as_ref()
            .unwrap()
            .patches
            .len(),
        2
    );

    // 4. ShadingType 7: Illustrator Tensor-Product 16-point Bicubic Patch Mesh + Video RGBA + RuntimeSnapshot
    let mut raw7 = Vec::new();
    let patch7 = neoflash_core::VectorMeshPatch {
        points: [
            [20.0, 20.0],
            [70.0, 10.0],
            [130.0, 30.0],
            [180.0, 20.0],
            [15.0, 70.0],
            [75.0, 65.0],
            [125.0, 75.0],
            [185.0, 70.0],
            [25.0, 130.0],
            [70.0, 135.0],
            [130.0, 125.0],
            [175.0, 130.0],
            [20.0, 180.0],
            [70.0, 190.0],
            [130.0, 170.0],
            [180.0, 180.0],
        ],
        colors: [
            [240, 30, 50, 255],
            [30, 220, 90, 255],
            [40, 80, 250, 255],
            [250, 210, 30, 255],
        ],
    };
    let (pdf_pts, pdf_cols) = patch7.to_pdf_tensor_16();
    raw7.push(0u8);
    for [x, y] in pdf_pts {
        push_u16_coord(&mut raw7, x, 0.0, 200.0);
        push_u16_coord(&mut raw7, y, 0.0, 200.0);
    }
    for c in pdf_cols {
        raw7.extend_from_slice(&[c[0], c[1], c[2]]);
    }
    let pdf7 = build_pdf_with_shading_stream(
        "/ShadingType 7 /ColorSpace /DeviceRGB /BitsPerCoordinate 16 /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 200 0 200 0 1 0 1 0 1]",
        &raw7,
        "q 20 20 160 160 re W n /Sh0 sh Q",
    );
    let (proj7, _) =
        import_pdf_ai_with_options(&pdf7, "TensorMesh", &VectorImportOptions::default())
            .expect("ShadingType 7 Tensor PDF/AI import");
    assert_pure_vector_mesh_project(&proj7, "PDF ShadingType 7", 1);

    // Runtime snapshot carries `mesh_fill` for HTML5 Canvas/WASM
    let player = Player::new(&proj7);
    let snap = player.runtime_snapshot(&proj7);
    assert_eq!(snap.draws.len(), 1);
    let rt_mesh = snap.draws[0]
        .item
        .mesh_fill
        .as_ref()
        .expect("RuntimeSnapshot draw item must carry mesh_fill");
    assert_eq!(rt_mesh.patches.len(), 1);

    // Video/frame renderer rasterizes smooth Gouraud colors only at final pixel output
    let scene7 = evaluate_scene(&proj7, &proj7.root, 0, false, true);
    let mut cache = HashMap::new();
    let rgba =
        render_authored_scene_rgba(&proj7, &proj7.root, 0, &scene7, 200, 200, true, &mut cache)
            .expect("render vector mesh scene to RGBA");
    let px = |x: u32, y: u32| -> [u8; 4] {
        let idx = ((y * 200 + x) * 4) as usize;
        [rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3]]
    };
    assert_eq!(
        px(5, 5)[3],
        0,
        "pixels outside clip contour must remain transparent"
    );
    let center_px = px(100, 100);
    assert_eq!(center_px[3], 255, "interior mesh pixel must be opaque");
    assert!(
        center_px[0] > 60 && center_px[1] > 60 && center_px[2] > 40,
        "center pixel must be smoothly interpolated across all 4 corner colours, got {center_px:?}"
    );
}

#[test]
fn svg2_meshgradient_and_eps_mesh_import_and_roundtrip_across_pdf_ai_eps_and_svg() {
    let svg_src = r##"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="160" viewBox="0 0 240 160">
  <defs>
    <meshgradient id="aurora" gradientUnits="userSpaceOnUse" x="20" y="20" color-interpolation="linearRGB">
      <meshrow>
        <meshpatch>
          <stop path="c 30,-10 60,10 90,0" stop-color="#ff2040"/>
          <stop path="c 5,40 -5,80 0,120" stop-color="#20e070"/>
          <stop path="c -30,10 -60,-10 -90,0" stop-color="#3060ff"/>
          <stop path="c -5,-40 5,-80 0,-120" stop-color="#ffd020"/>
        </meshpatch>
        <meshpatch>
          <stop path="l 100,0" stop-color="#ff40e0"/>
          <stop path="l 0,120" stop-color="#20f0f0"/>
          <stop path="l -100,0"/>
        </meshpatch>
      </meshrow>
    </meshgradient>
  </defs>
  <path d="M 20 20 L 210 20 L 210 140 L 20 140 Z" fill="url(#aurora)" stroke="#112233" stroke-width="2"/>
</svg>"##;

    let (svg_proj, _) =
        import_svg_with_options(svg_src, "AuroraMesh", &VectorImportOptions::default())
            .expect("import SVG2 meshgradient");
    assert_pure_vector_mesh_project(&svg_proj, "SVG2 <meshgradient>", 1);
    let svg_scene = evaluate_scene(&svg_proj, &svg_proj.root, 0, false, true);
    let orig_mesh: &VectorMeshPaint = svg_scene.draws[0]
        .item
        .mesh_fill
        .as_ref()
        .expect("SVG2 mesh_fill evaluated");
    assert_eq!(orig_mesh.patches.len(), 2);
    assert!(orig_mesh.linear_rgb);

    // 1. Export SVG -> verify `<meshgradient>` in `<defs>` and NO `<image` tag -> Re-import SVG
    let (exported_svg, _, _) = render_frame_svg(
        &svg_proj,
        &SvgOptions {
            transparent_background: true,
            ..SvgOptions::default()
        },
    )
    .expect("export vector mesh to SVG");
    assert!(
        exported_svg.contains("<meshgradient "),
        "SVG export must emit vector <meshgradient>: {exported_svg}"
    );
    assert!(
        !exported_svg.contains("<image"),
        "SVG export must NEVER rasterize vector mesh to <image>: {exported_svg}"
    );
    let (reimported_svg, _) = import_svg_with_options(
        &exported_svg,
        "RoundTripSvg",
        &VectorImportOptions::default(),
    )
    .expect("re-import exported mesh SVG");
    assert_pure_vector_mesh_project(&reimported_svg, "Round-trip SVG", 1);
    let rt_svg_scene = evaluate_scene(&reimported_svg, &reimported_svg.root, 0, false, true);
    let rt_svg_mesh = rt_svg_scene
        .draws
        .iter()
        .find_map(|d| d.item.mesh_fill.as_ref())
        .expect("re-imported SVG scene must contain mesh_fill draw");
    assert_eq!(rt_svg_mesh.patches.len(), 2);

    // 2. Export PDF (both `export_project_image` AND `export_clipboard_pdf`) -> Re-import PDF
    let temp = std::env::temp_dir().join(format!("nf-t501-mesh-rt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).unwrap();

    let pdf_file = temp.join("aurora.pdf");
    export_project_image(
        &svg_proj,
        &pdf_file,
        &ImageExportOptions {
            format: ImageExportFormat::Pdf,
            transparent_background: true,
            ..ImageExportOptions::default()
        },
    )
    .expect("export vector mesh PDF");
    let pdf_bytes = std::fs::read(&pdf_file).unwrap();
    let pdf_ascii = String::from_utf8_lossy(&pdf_bytes);
    assert!(
        pdf_ascii.contains("/ShadingType 7"),
        "exported PDF must contain vector /ShadingType 7, not raster /Subtype /Image"
    );
    assert!(
        !pdf_ascii.contains("/Subtype /Image"),
        "exported PDF must NOT fall back to a bitmap /Subtype /Image"
    );
    let (reimported_pdf, _) =
        import_pdf_ai_with_options(&pdf_bytes, "RoundTripPdf", &VectorImportOptions::default())
            .expect("re-import exported PDF");
    assert_pure_vector_mesh_project(&reimported_pdf, "Round-trip PDF", 1);

    let clip_pdf = export_clipboard_pdf(&svg_proj, None, 0, None).expect("clipboard PDF");
    let (reimported_clip_pdf, _) = import_pdf_ai_with_options(
        &clip_pdf,
        "RoundTripClipPdf",
        &VectorImportOptions::default(),
    )
    .expect("re-import clipboard PDF");
    assert_pure_vector_mesh_project(&reimported_clip_pdf, "Round-trip Clipboard PDF", 1);

    // 3. Export AI & Clipboard EPS -> verify Level 3 `/ShadingType 7 ... shfill` & no `colorimage` -> Re-import EPS
    let ai_file = temp.join("aurora.ai");
    export_project_image(
        &svg_proj,
        &ai_file,
        &ImageExportOptions {
            format: ImageExportFormat::Ai,
            transparent_background: true,
            ..ImageExportOptions::default()
        },
    )
    .expect("export vector mesh AI");
    let ai_bytes = std::fs::read(&ai_file).unwrap();
    let ai_str = std::str::from_utf8(&ai_bytes).expect("AI is ASCII PostScript");
    assert!(ai_str.contains("/ShadingType 7"));
    assert!(!ai_str.contains("colorimage"));
    let (reimported_ai, _) =
        import_vector_bytes_with_options(&ai_bytes, "RoundTripAi", &VectorImportOptions::default())
            .expect("re-import AI as PostScript Level 3");
    assert_pure_vector_mesh_project(&reimported_ai, "Round-trip AI", 1);

    let clip_eps = export_clipboard_eps(&svg_proj, None, 0, None).expect("clipboard EPS");
    let (reimported_eps, _) = import_vector_bytes_with_options(
        &clip_eps,
        "RoundTripEps",
        &VectorImportOptions::default(),
    )
    .expect("re-import clipboard EPS");
    assert_pure_vector_mesh_project(&reimported_eps, "Round-trip Clipboard EPS", 1);

    let _ = std::fs::remove_dir_all(&temp);

    let dimmed = orig_mesh.tessellate(
        Some(4),
        ColorTransform {
            multipliers: [0.5, 0.5, 0.5, 0.5],
            offsets: [0.0; 4],
        },
    );
    assert!(dimmed.vertices.iter().all(|(_, c)| c[3] <= 128));
}

#[test]
fn masked_translucent_mesh_pdf_ai_and_iso32000_subbyte_alignment_stay_pure_vector() {
    // 1. Masked (`clipPath` -> `LayerRole::Mask`) + translucent (`alpha = 0.65`) SVG2 `<meshgradient>`:
    //    `export_project_image(..., Pdf)` and `export_project_image(..., Ai)` must stay 100% vector
    //    (`/ShadingType 7`, ZERO `/Subtype /Image`, ZERO `colorimage`).
    let masked_svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120" viewBox="0 0 120 120">
      <defs>
        <clipPath id="circleClip"><circle cx="60" cy="60" r="45"/></clipPath>
        <meshgradient id="mgClip" x="10" y="10" gradientUnits="userSpaceOnUse">
          <meshrow>
            <meshpatch>
              <stop path="c 30,0 70,0 100,0" stop-color="#ff2244"/>
              <stop path="c 0,30 0,70 0,100" stop-color="#22ee88"/>
              <stop path="c -30,0 -70,0 -100,0" stop-color="#2266ff"/>
              <stop path="c 0,-30 0,-70 0,-100" stop-color="#ffcc00"/>
            </meshpatch>
          </meshrow>
        </meshgradient>
      </defs>
      <g clip-path="url(#circleClip)" opacity="0.65">
        <rect x="10" y="10" width="100" height="100" fill="url(#mgClip)"/>
      </g>
    </svg>"##;
    let (masked_proj, _) =
        import_svg_with_options(masked_svg, "MaskedMesh", &VectorImportOptions::default())
            .expect("import masked mesh SVG");
    assert_pure_vector_mesh_project(&masked_proj, "Masked SVG2 mesh", 1);

    let temp = std::env::temp_dir().join(format!("nf-t501-masked-mesh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).unwrap();

    let masked_pdf_path = temp.join("masked.pdf");
    export_project_image(
        &masked_proj,
        &masked_pdf_path,
        &ImageExportOptions {
            format: ImageExportFormat::Pdf,
            transparent_background: true,
            ..ImageExportOptions::default()
        },
    )
    .expect("export masked translucent mesh PDF");
    let masked_pdf = std::fs::read(&masked_pdf_path).unwrap();
    let masked_pdf_str = String::from_utf8_lossy(&masked_pdf);
    assert!(masked_pdf_str.contains("/ShadingType 7"));
    assert!(masked_pdf_str.contains("/ExtGState"));
    assert!(!masked_pdf_str.contains("/Subtype /Image"));

    let masked_ai_path = temp.join("masked.ai");
    export_project_image(
        &masked_proj,
        &masked_ai_path,
        &ImageExportOptions {
            format: ImageExportFormat::Ai,
            transparent_background: true,
            ..ImageExportOptions::default()
        },
    )
    .expect("export masked mesh AI");
    let masked_ai = std::fs::read(&masked_ai_path).unwrap();
    let masked_ai_str = std::str::from_utf8(&masked_ai).unwrap();
    assert!(masked_ai_str.contains("/ShadingType 7"));
    assert!(masked_ai_str.contains("clip"));
    assert!(!masked_ai_str.contains("colorimage"));
    let _ = std::fs::remove_dir_all(&temp);

    // 2. ISO 32000-1 §8.7.4.5.5 per-vertex byte alignment (`BitsPerFlag 2`, `BitsPerCoordinate 12`,
    //    `BitsPerComponent 8` -> 50 bits/vertex + 6 zero pad bits = 56 bits = 7 bytes/vertex):
    let pack_type4_vtx = |flag: u8, x12: u16, y12: u16, rgb: [u8; 3]| -> [u8; 7] {
        let mut bits: u64 = 0;
        bits = (bits << 2) | u64::from(flag & 0b11);
        bits = (bits << 12) | u64::from(x12 & 0xFFF);
        bits = (bits << 12) | u64::from(y12 & 0xFFF);
        bits = (bits << 8) | u64::from(rgb[0]);
        bits = (bits << 8) | u64::from(rgb[1]);
        bits = (bits << 8) | u64::from(rgb[2]);
        bits <<= 6; // 6-bit pad to 56-bit (7-byte) boundary per ISO 32000-1!
        let be = bits.to_be_bytes();
        [be[1], be[2], be[3], be[4], be[5], be[6], be[7]]
    };
    let mut t4_stream = Vec::new();
    t4_stream.extend_from_slice(&pack_type4_vtx(0, 0, 0, [255, 0, 0]));
    t4_stream.extend_from_slice(&pack_type4_vtx(0, 4095, 0, [0, 255, 0]));
    t4_stream.extend_from_slice(&pack_type4_vtx(0, 4095, 4095, [0, 0, 255]));
    t4_stream.extend_from_slice(&pack_type4_vtx(2, 0, 4095, [255, 255, 0])); // 2nd triangle sharing v0,v2!

    let pdf_subbyte = build_pdf_with_shading_stream(
        "/ShadingType 4 /ColorSpace /DeviceRGB /BitsPerFlag 2 /BitsPerCoordinate 12 \
         /BitsPerComponent 8 /Decode [0 100 0 100 0 1 0 1 0 1]",
        &t4_stream,
        "q 0 0 100 100 re W n /Sh0 sh Q",
    );
    let (subbyte_proj, _) = import_pdf_ai_with_options(
        &pdf_subbyte,
        "Iso32000SubByte",
        &VectorImportOptions::default(),
    )
    .expect("import ISO 32000-1 sub-byte ShadingType 4");
    assert_pure_vector_mesh_project(&subbyte_proj, "ISO 32000-1 SubByte Type 4", 1);
    let subbyte_root = subbyte_proj.symbol(&subbyte_proj.root).unwrap();
    let subbyte_mesh = subbyte_root.timeline.layers[0].keyframes[0].vector_effects[0]
        .vector_mesh
        .as_ref()
        .unwrap();
    assert_eq!(subbyte_mesh.triangles.len(), 2);
    assert_eq!(subbyte_mesh.triangles[0].colors[1], [0, 255, 0, 255]);
    assert_eq!(subbyte_mesh.triangles[0].colors[2], [0, 0, 255, 255]);
    assert_eq!(subbyte_mesh.triangles[1].colors[2], [255, 255, 0, 255]);
}
