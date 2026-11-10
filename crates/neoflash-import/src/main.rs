use neoflash_core::{GlobalSwatch, Project};
use neoflash_import::{
    Diagnostic, ImportError, ImportFormat, ImportReport, PsdImportOptions, Severity, Statistics,
    VectorImportOptions, import_act_palette, import_ase_palette, import_clr_palette,
    import_fxg_str, import_numbered_image_sequence_paths, import_pdf_ai_with_options,
    import_psd_path,
};
use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn ext_lower(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
}

fn stem_string(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Imported")
        .to_owned()
}

fn is_sequence_image_ext(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
}

fn import_sequence_paths(inputs: &[PathBuf]) -> Result<(Project, ImportReport), ImportError> {
    import_numbered_image_sequence_paths(inputs, true)?
        .map(|imported| (imported.project, imported.report))
        .ok_or_else(|| {
            ImportError::Container(
                "Inputs do not form a numbered image sequence (for example name_0001.png)".into(),
            )
        })
}

fn import_palette_path(path: &Path, ext: &str) -> Result<(Project, ImportReport), ImportError> {
    let bytes =
        fs::read(path).map_err(|e| ImportError::Container(format!("{}: {e}", path.display())))?;
    let source_name = stem_string(path);
    let swatches: Vec<GlobalSwatch> = match ext {
        "act" => import_act_palette(&bytes, &source_name)?,
        "clr" => import_clr_palette(&bytes, &source_name)?,
        "ase" => import_ase_palette(&bytes)?,
        _ => unreachable!(),
    };
    if swatches.is_empty() {
        return Err(ImportError::Container(format!(
            "{}: palette contains no colour swatches",
            path.display()
        )));
    }
    let colors: Vec<_> = swatches.iter().map(|s| s.color).take(256).collect();
    let mut project = Project::blank();
    project.name = source_name.clone();
    project.set_global_swatches(&swatches);
    project.set_custom_swatches(&colors);

    let report = ImportReport {
        source: path.display().to_string(),
        format: ImportFormat::Unknown,
        can_import: true,
        version: Some(ext.to_ascii_uppercase()),
        statistics: Statistics {
            symbols: project.symbols.len(),
            layers: 1,
            keyframes: 1,
            elements: colors.len(),
            scripts: 0,
        },
        diagnostics: vec![Diagnostic {
            severity: Severity::Info,
            code: "palette-imported".to_owned(),
            location: path.display().to_string(),
            message: format!(
                "Imported {} colour swatch(es) from .{} palette.",
                colors.len(),
                ext
            ),
        }],
        binary_fla: None,
        swf: None,
        swc: None,
    };
    Ok((project, report))
}

fn import_or_inspect_paths(
    inputs: &[PathBuf],
    require_importable: bool,
) -> Result<(Option<Project>, ImportReport), Box<dyn std::error::Error>> {
    if inputs.is_empty() {
        return Err("Missing input path".into());
    }
    if inputs.len() > 1 {
        let (project, report) = import_sequence_paths(inputs)?;
        return Ok((Some(project), report));
    }

    let path = &inputs[0];
    if path.is_dir() && !path.join("DOMDocument.xml").exists() {
        let (project, report) = import_sequence_paths(inputs)?;
        return Ok((Some(project), report));
    }

    if let Some(ext) = ext_lower(path) {
        match ext.as_str() {
            "psd" | "psb" => {
                let (project, report) = import_psd_path(path, &PsdImportOptions::default())?;
                return Ok((Some(project), report));
            }
            "pdf" | "ai" => {
                let bytes = fs::read(path)?;
                let source_name = stem_string(path);
                let (project, mut report) = import_pdf_ai_with_options(
                    &bytes,
                    &source_name,
                    &VectorImportOptions::default(),
                )?;
                report.statistics.symbols = project.symbols.len();
                if report.statistics.elements == 0 {
                    report.diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        code: "pdf-no-vector-content".to_owned(),
                        location: path.display().to_string(),
                        message: "PDF/AI file contains no importable vector or text operators."
                            .to_owned(),
                    });
                    report.can_import = false;
                    if require_importable {
                        return Err(ImportError::Blocked(
                            "PDF/AI file contains no importable vector or text operators.".into(),
                        )
                        .into());
                    }
                }
                return Ok((Some(project), report));
            }
            "fxg" => {
                let bytes = fs::read(path)?;
                let xml = std::str::from_utf8(&bytes).map_err(|_| {
                    ImportError::Container(format!(
                        "{}: FXG document must be valid UTF-8",
                        path.display()
                    ))
                })?;
                let source_name = stem_string(path);
                let (mut project, mut report) = import_fxg_str(xml, &source_name)?;
                if project
                    .symbols
                    .iter()
                    .any(|s| s.linkage_id.is_some() || s.class_name.is_some())
                {
                    project.format_version = project.format_version.max(2);
                }
                report.statistics.symbols = project.symbols.len();
                if report.statistics.elements == 0 && report.statistics.symbols <= 1 {
                    report.diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        code: "fxg-empty".to_owned(),
                        location: path.display().to_string(),
                        message:
                            "FXG document contains no graphic elements or library definitions."
                                .to_owned(),
                    });
                    report.can_import = false;
                    if require_importable {
                        return Err(ImportError::Blocked(
                            "FXG document contains no graphic elements or library definitions."
                                .into(),
                        )
                        .into());
                    }
                } else {
                    report.can_import = true;
                }
                return Ok((Some(project), report));
            }
            "act" | "clr" | "ase" => {
                let (project, report) = import_palette_path(path, &ext)?;
                return Ok((Some(project), report));
            }
            ext_str if is_sequence_image_ext(ext_str) => {
                if let Some(imported) = import_numbered_image_sequence_paths(inputs, true)? {
                    return Ok((Some(imported.project), imported.report));
                }
            }
            _ => {}
        }
    }

    if require_importable {
        let project = neoflash_import::import_project(path)?;
        let report = neoflash_import::inspect(path)?;
        Ok((Some(project), report))
    } else {
        let report = neoflash_import::inspect(path)?;
        Ok((None, report))
    }
}

fn load_or_import_project(inputs: &[PathBuf]) -> Result<Project, Box<dyn std::error::Error>> {
    if inputs.len() == 1 {
        let path = &inputs[0];
        if path.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("nflash") || e.eq_ignore_ascii_case("json"))
        {
            let json = fs::read_to_string(path)?;
            return Ok(Project::from_json(&json)?);
        }
    }
    let (project, _) = import_or_inspect_paths(inputs, true)?;
    project.ok_or_else(|| "Import did not produce a project".into())
}

fn is_fla_destination(dest: &str) -> bool {
    Path::new(dest)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("fla"))
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.as_slice() {
        [command, inputs @ ..] if command == "inspect" && !inputs.is_empty() => {
            let input_paths: Vec<PathBuf> = inputs.iter().map(PathBuf::from).collect();
            let (_, report) = import_or_inspect_paths(&input_paths, false)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        [command, rest @ ..] if command == "import" && rest.len() >= 2 => {
            let (inputs, dest_slice) = rest.split_at(rest.len() - 1);
            let destination = &dest_slice[0];
            let input_paths: Vec<PathBuf> = inputs.iter().map(PathBuf::from).collect();
            let (project_opt, _) = import_or_inspect_paths(&input_paths, true)?;
            let project = project_opt.ok_or("Import did not produce a project")?;
            let bytes = if is_fla_destination(destination) {
                neoflash_import::export_fla_bytes(&project)?
            } else {
                project.to_json()?.into_bytes()
            };
            // Creating a new output never overwrites a source or an existing project.
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(destination)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            println!(
                "Imported {} symbols into {destination}",
                project.symbols.len()
            );
        }
        [command, rest @ ..] if command == "export-fla" && rest.len() >= 2 => {
            let (inputs, dest_slice) = rest.split_at(rest.len() - 1);
            let destination = &dest_slice[0];
            let input_paths: Vec<PathBuf> = inputs.iter().map(PathBuf::from).collect();
            let project = load_or_import_project(&input_paths)?;
            let bytes = neoflash_import::export_fla_bytes(&project)?;
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(destination)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            println!(
                "Exported {} symbols into {destination}",
                project.symbols.len()
            );
        }
        _ => {
            return Err(
                "Usage: neoflash-import inspect PATH... | neoflash-import import PATH... NEW_OUTPUT.{nflash|json|fla} | neoflash-import export-fla INPUT... NEW_OUTPUT.fla"
                    .into(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageEncoder, codecs::png::PngEncoder};
    use neoflash_core::{BlendMode, ElementKind, LayerRole, MaskMode};

    fn make_png_bytes(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut raw = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            raw.extend_from_slice(&rgba);
        }
        let mut out = Vec::new();
        PngEncoder::new(&mut out)
            .write_image(&raw, w, h, image::ExtendedColorType::Rgba8)
            .unwrap();
        out
    }

    fn make_hierarchical_psd_bytes(canvas_w: u32, canvas_h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"8BPS");
        out.extend_from_slice(&1u16.to_be_bytes()); // PSD v1
        out.extend_from_slice(&[0u8; 6]);
        out.extend_from_slice(&4u16.to_be_bytes()); // 4 channels
        out.extend_from_slice(&canvas_h.to_be_bytes());
        out.extend_from_slice(&canvas_w.to_be_bytes());
        out.extend_from_slice(&8u16.to_be_bytes()); // 8-bit depth
        out.extend_from_slice(&3u16.to_be_bytes()); // RGB mode
        out.extend_from_slice(&0u32.to_be_bytes()); // color mode data
        out.extend_from_slice(&0u32.to_be_bytes()); // image resources

        struct LRec<'a> {
            name: &'a str,
            bounds: [i32; 4],
            blend: &'a [u8; 4],
            clipping: u8,
            flags: u8,
            extra: Vec<u8>,
            channels: Vec<(i16, Vec<u8>)>,
        }

        let raw_ch = |val: u8, n: usize| -> Vec<u8> {
            let mut b = Vec::with_capacity(2 + n);
            b.extend_from_slice(&0u16.to_be_bytes());
            b.extend(std::iter::repeat_n(val, n));
            b
        };

        let lsct = |kind: u32| -> Vec<u8> {
            let mut b = Vec::new();
            b.extend_from_slice(b"8BIMlsct");
            b.extend_from_slice(&4u32.to_be_bytes());
            b.extend_from_slice(&kind.to_be_bytes());
            b
        };

        let mut tysh_extra = Vec::new();
        tysh_extra.extend_from_slice(b"8BIMTySh");
        let mut tysh = Vec::new();
        tysh.extend_from_slice(&1u16.to_be_bytes());
        for &v in &[1.0f64, 0.0, 0.0, 1.0, 10.0, 14.0] {
            tysh.extend_from_slice(&v.to_bits().to_be_bytes());
        }
        tysh.extend_from_slice(b"Txt TEXT");
        let utf16: Vec<u16> = "Hero PSD".encode_utf16().collect();
        tysh.extend_from_slice(&(utf16.len() as u32).to_be_bytes());
        for u in utf16 {
            tysh.extend_from_slice(&u.to_be_bytes());
        }
        if tysh.len() % 2 != 0 {
            tysh.push(0);
        }
        tysh_extra.extend_from_slice(&(tysh.len() as u32).to_be_bytes());
        tysh_extra.extend_from_slice(&tysh);
        // Add an adjustment signature to trigger `psd-adjustment-approximated`
        tysh_extra.extend_from_slice(b"8BIMcurv");
        tysh_extra.extend_from_slice(&4u32.to_be_bytes());
        tysh_extra.extend_from_slice(&0u32.to_be_bytes());

        // Bottom-to-top PSD order:
        // 0: "</Layer group>" (section divider 3 = close folder)
        // 1: "MaskBase" (4x4 bitmap)
        // 2: "ClippedMul" (4x4 bitmap clipped to MaskBase, blend = mult)
        // 3: "TitleText" (TySh text + adjustment warning)
        // 4: "UI Group" (section divider 1 = open folder)
        let layers = vec![
            LRec {
                name: "</Layer group>",
                bounds: [0, 0, 0, 0],
                blend: b"norm",
                clipping: 0,
                flags: 0,
                extra: lsct(3),
                channels: vec![
                    (0, raw_ch(0, 0)),
                    (1, raw_ch(0, 0)),
                    (2, raw_ch(0, 0)),
                    (-1, raw_ch(0, 0)),
                ],
            },
            LRec {
                name: "MaskBase",
                bounds: [0, 0, 4, 4],
                blend: b"norm",
                clipping: 0,
                flags: 0,
                extra: Vec::new(),
                channels: vec![
                    (0, raw_ch(200, 16)),
                    (1, raw_ch(100, 16)),
                    (2, raw_ch(50, 16)),
                    (-1, raw_ch(255, 16)),
                ],
            },
            LRec {
                name: "ClippedMul",
                bounds: [0, 0, 4, 4],
                blend: b"mult",
                clipping: 1,
                flags: 0,
                extra: Vec::new(),
                channels: vec![
                    (0, raw_ch(40, 16)),
                    (1, raw_ch(180, 16)),
                    (2, raw_ch(240, 16)),
                    (-1, raw_ch(255, 16)),
                ],
            },
            LRec {
                name: "TitleText",
                bounds: [2, 2, 6, 6],
                blend: b"norm",
                clipping: 0,
                flags: 0,
                extra: tysh_extra,
                channels: vec![
                    (0, raw_ch(20, 16)),
                    (1, raw_ch(20, 16)),
                    (2, raw_ch(20, 16)),
                    (-1, raw_ch(255, 16)),
                ],
            },
            LRec {
                name: "UI Group",
                bounds: [0, 0, 0, 0],
                blend: b"pass",
                clipping: 0,
                flags: 0,
                extra: lsct(1),
                channels: vec![
                    (0, raw_ch(0, 0)),
                    (1, raw_ch(0, 0)),
                    (2, raw_ch(0, 0)),
                    (-1, raw_ch(0, 0)),
                ],
            },
        ];

        let mut linfo = Vec::new();
        linfo.extend_from_slice(&(layers.len() as i16).to_be_bytes());
        for l in &layers {
            for b in l.bounds {
                linfo.extend_from_slice(&b.to_be_bytes());
            }
            linfo.extend_from_slice(&(l.channels.len() as u16).to_be_bytes());
            for (cid, blob) in &l.channels {
                linfo.extend_from_slice(&cid.to_be_bytes());
                linfo.extend_from_slice(&(blob.len() as u32).to_be_bytes());
            }
            linfo.extend_from_slice(b"8BIM");
            linfo.extend_from_slice(l.blend);
            linfo.push(255);
            linfo.push(l.clipping);
            linfo.push(l.flags);
            linfo.push(0);

            let mut extra = Vec::new();
            extra.extend_from_slice(&0u32.to_be_bytes());
            extra.extend_from_slice(&0u32.to_be_bytes());
            let nb = l.name.as_bytes();
            extra.push(nb.len() as u8);
            extra.extend_from_slice(nb);
            let pad = (4 - ((1 + nb.len()) % 4)) % 4;
            extra.extend(std::iter::repeat_n(0u8, pad));
            extra.extend_from_slice(&l.extra);

            linfo.extend_from_slice(&(extra.len() as u32).to_be_bytes());
            linfo.extend_from_slice(&extra);
        }
        for l in &layers {
            for (_, blob) in &l.channels {
                linfo.extend_from_slice(blob);
            }
        }
        let mut lsection = Vec::new();
        lsection.extend_from_slice(&(linfo.len() as u32).to_be_bytes());
        lsection.extend_from_slice(&linfo);
        out.extend_from_slice(&(lsection.len() as u32).to_be_bytes());
        out.extend_from_slice(&lsection);
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend(std::iter::repeat_n(
            128u8,
            (canvas_w * canvas_h * 4) as usize,
        ));
        out
    }

    #[test]
    fn cli_entry_points_import_and_inspect_psd_sequence_pdf_fxg_and_palettes_and_reject_malformed()
    {
        let temp = std::env::temp_dir().join(format!(
            "neoflash-cli-professional-import-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        // 1. PSD with folder hierarchy, clipping mask, blend mode, TySh text, and adjustment warning
        let psd_path = temp.join("CharacterRig.psd");
        fs::write(&psd_path, make_hierarchical_psd_bytes(32, 24)).unwrap();
        let (psd_proj_opt, psd_report) =
            import_or_inspect_paths(std::slice::from_ref(&psd_path), true).unwrap();
        assert!(psd_report.can_import);
        assert!(
            psd_report
                .diagnostics
                .iter()
                .any(|d| d.code == "psd-adjustment-layer-approximated")
        );
        let psd_proj = psd_proj_opt.unwrap();
        psd_proj.validate().unwrap();
        assert_eq!(psd_proj.name, "CharacterRig");
        assert_eq!(psd_proj.width, 32);
        assert_eq!(psd_proj.height, 24);
        let psd_root = psd_proj.symbol(&psd_proj.root).unwrap();
        let folder = psd_root
            .timeline
            .layers
            .iter()
            .find(|l| l.name == "UI Group")
            .unwrap();
        assert_eq!(folder.properties.role, LayerRole::Folder);
        let mask_base = psd_root
            .timeline
            .layers
            .iter()
            .find(|l| l.name == "MaskBase")
            .unwrap();
        assert_eq!(mask_base.properties.role, LayerRole::Mask);
        assert_eq!(mask_base.properties.mask_mode, MaskMode::Alpha);
        assert_eq!(
            mask_base.properties.parent.as_deref(),
            Some(folder.id.as_str())
        );
        let clipped = psd_root
            .timeline
            .layers
            .iter()
            .find(|l| l.name == "ClippedMul")
            .unwrap();
        assert_eq!(
            clipped.properties.mask.as_deref(),
            Some(mask_base.id.as_str())
        );
        assert_eq!(clipped.properties.blend_mode, BlendMode::Multiply);
        let text_layer = psd_root
            .timeline
            .layers
            .iter()
            .find(|l| l.name == "TitleText")
            .unwrap();
        assert!(matches!(
            &text_layer.keyframes[0].elements[0].kind,
            ElementKind::Text { text, .. } if text == "Hero PSD"
        ));

        // 2. Numbered PNG sequence with gap hold (walk_0001.png, walk_0002.png, walk_0004.png)
        let seq_dir = temp.join("seq");
        fs::create_dir_all(&seq_dir).unwrap();
        let f1 = seq_dir.join("walk_0001.png");
        let f2 = seq_dir.join("walk_0002.png");
        let f4 = seq_dir.join("walk_0004.png");
        fs::write(&f1, make_png_bytes(48, 36, [255, 0, 0, 255])).unwrap();
        fs::write(&f2, make_png_bytes(48, 36, [0, 255, 0, 255])).unwrap();
        fs::write(&f4, make_png_bytes(48, 36, [0, 0, 255, 255])).unwrap();

        // Single numbered frame expands sibling frames automatically
        let (seq_proj_opt, seq_report) =
            import_or_inspect_paths(std::slice::from_ref(&f1), true).unwrap();
        assert!(seq_report.can_import);
        assert!(
            seq_report
                .diagnostics
                .iter()
                .any(|d| d.code == "sequence-missing-frame")
        );
        let seq_proj = seq_proj_opt.unwrap();
        seq_proj.validate().unwrap();
        assert_eq!(seq_proj.name, "walk");
        assert_eq!(seq_proj.width, 48);
        assert_eq!(seq_proj.height, 36);
        let seq_root = seq_proj.symbol(&seq_proj.root).unwrap();
        assert_eq!(seq_root.timeline.frame_count, 4);
        assert_eq!(seq_root.timeline.layers[0].keyframes.len(), 3);
        assert_eq!(seq_root.timeline.layers[0].keyframes[0].frame, 0);
        assert_eq!(seq_root.timeline.layers[0].keyframes[1].frame, 1);
        assert_eq!(seq_root.timeline.layers[0].keyframes[2].frame, 3);

        // A normal non-numbered image keeps the prior generic inspection route instead of being
        // misclassified as an invalid sequence seed.
        let poster = seq_dir.join("poster.png");
        fs::write(&poster, make_png_bytes(8, 6, [80, 90, 100, 255])).unwrap();
        let (ordinary_project, ordinary_report) =
            import_or_inspect_paths(std::slice::from_ref(&poster), false).unwrap();
        assert!(ordinary_project.is_none());
        assert_eq!(ordinary_report.format, ImportFormat::Unknown);
        assert!(
            ordinary_report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "unknown_format")
        );

        // 3. PDF/AI vector artwork
        let pdf_path = temp.join("Poster.ai");
        let pdf_bytes = b"%PDF-1.4\n\
            1 0 obj << /Type /Page /MediaBox [0 0 400 300] >> endobj\n\
            2 0 obj << /Length 180 >>\n\
            stream\n\
            /ArtworkLayer BDC\n\
            q 0 0 100 100 re W n 0.1 0.5 0.9 rg 10 10 m 80 10 l 80 60 l h f BT /F1 16 Tf 12 24 Td (Hello AI) Tj ET /Sh1 sh Q\n\
            EMC\n\
            endstream\n\
            endobj\n";
        fs::write(&pdf_path, pdf_bytes).unwrap();
        let (pdf_proj_opt, pdf_report) =
            import_or_inspect_paths(std::slice::from_ref(&pdf_path), true).unwrap();
        assert!(pdf_report.can_import);
        assert!(
            pdf_report
                .diagnostics
                .iter()
                .any(|d| d.code == "pdf-unsupported-operator")
        );
        let pdf_proj = pdf_proj_opt.unwrap();
        pdf_proj.validate().unwrap();
        assert_eq!(pdf_proj.name, "Poster");
        assert_eq!(pdf_proj.width, 400);
        assert_eq!(pdf_proj.height, 300);

        // 4. FXG 2.0 document
        let fxg_path = temp.join("HudBadge.fxg");
        let fxg_xml = r##"<?xml version="1.0" encoding="utf-8"?>
<Graphic version="2.0" viewWidth="512" viewHeight="256" xmlns="http://ns.adobe.com/fxg/2008">
  <Library>
    <Definition name="GemDef">
      <Ellipse width="32" height="32"><fill><SolidColor color="#FF3366"/></fill></Ellipse>
    </Definition>
  </Library>
  <Group id="MainGroup">
    <Rect x="10" y="20" width="120" height="60"><fill><SolidColor color="#224488"/></fill></Rect>
    <GemDef x="80" y="40"/>
  </Group>
</Graphic>"##;
        fs::write(&fxg_path, fxg_xml).unwrap();
        let (fxg_proj_opt, fxg_report) =
            import_or_inspect_paths(std::slice::from_ref(&fxg_path), true).unwrap();
        assert!(fxg_report.can_import);
        let fxg_proj = fxg_proj_opt.unwrap();
        fxg_proj.validate().unwrap();
        assert_eq!(fxg_proj.name, "HudBadge");
        assert_eq!(fxg_proj.width, 512);
        assert_eq!(fxg_proj.height, 256);
        assert!(fxg_proj.symbols.iter().any(|s| s.name == "GemDef"));

        // 5. ACT, CLR, and ASE palettes
        let act_path = temp.join("Studio.act");
        let act_bytes = neoflash_import::export_act_palette(&[
            GlobalSwatch {
                id: "s1".into(),
                name: "Ruby".into(),
                color: [220, 20, 60, 255],
            },
            GlobalSwatch {
                id: "s2".into(),
                name: "Mint".into(),
                color: [40, 200, 120, 255],
            },
        ]);
        fs::write(&act_path, &act_bytes).unwrap();
        let (act_proj_opt, act_report) =
            import_or_inspect_paths(std::slice::from_ref(&act_path), true).unwrap();
        assert!(act_report.can_import);
        let act_proj = act_proj_opt.unwrap();
        assert_eq!(act_proj.name, "Studio");
        assert_eq!(act_proj.custom_swatches().len(), 2);

        // 6. Malformed inputs fail cleanly with located diagnostics
        let bad_psd = temp.join("broken.psd");
        fs::write(&bad_psd, b"8BPS\x00\x01truncated").unwrap();
        assert!(import_or_inspect_paths(&[bad_psd], true).is_err());

        let bad_pdf = temp.join("empty.pdf");
        fs::write(&bad_pdf, b"%PDF-1.4\n1 0 obj << >> endobj\n").unwrap();
        assert!(import_or_inspect_paths(&[bad_pdf], true).is_err());

        let bad_fxg = temp.join("empty.fxg");
        fs::write(
            &bad_fxg,
            r#"<Graphic version="2.0" xmlns="http://ns.adobe.com/fxg/2008"/>"#,
        )
        .unwrap();
        assert!(import_or_inspect_paths(&[bad_fxg], true).is_err());

        let bad_act = temp.join("short.act");
        fs::write(&bad_act, [1u8, 2, 3]).unwrap();
        assert!(import_or_inspect_paths(&[bad_act], true).is_err());

        let _ = fs::remove_dir_all(&temp);
    }
}
