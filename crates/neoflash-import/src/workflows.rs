//! Unified re-exports and unit tests for numbered image sequences, layered PSD/PSB files,
//! and high-fidelity SVG/PDF/AI vector import workflows (`T-115`).

pub use crate::figma::*;
pub use crate::psd::*;
pub use crate::sequence::*;
pub use crate::vector_fidelity::*;

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{
        BlendMode, ElementKind, InstancePlayback, LayerRole, MaskMode, Project, ResourceUnit,
        SymbolKind,
    };
    use std::io::Write as _;

    fn make_test_png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(width, height, |_, _| image::Rgba(rgba));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn image_sequence_detects_numbering_gaps_and_imports_keyframes_layers_and_symbols() {
        let files = vec![
            "walk_0001.png",
            "walk_0002.png",
            "walk_0004.png", // gap at 0003!
            "readme.txt",    // unmatched file!
        ];
        let det = detect_numbered_image_sequence(&files).expect("sequence detected");
        assert_eq!(det.prefix, "walk_");
        assert_eq!(det.extension, "png");
        assert_eq!(det.padding, 4);
        assert_eq!(det.start_number, 1);
        assert_eq!(det.end_number, 4);
        assert_eq!(det.items.len(), 3);
        assert_eq!(det.missing_numbers, vec![3]);
        assert_eq!(det.unmatched_files, vec!["readme.txt".to_string()]);

        let png1 = make_test_png(32, 24, [255, 0, 0, 255]);
        let png2 = make_test_png(32, 24, [0, 255, 0, 255]);
        let png4 = make_test_png(32, 24, [0, 0, 255, 255]);
        let entries = [
            ("walk_0001.png".to_string(), png1.clone()),
            ("walk_0002.png".to_string(), png2.clone()),
            ("walk_0004.png".to_string(), png4.clone()),
            ("corrupt_0005.png".to_string(), b"not-a-valid-png".to_vec()),
        ];

        // 1. KeyframesOnSingleLayer with frame_step = 2 and gap hold frames
        let opts_kf = ImageSequenceImportOptions {
            placement: ImageSequencePlacement::KeyframesOnSingleLayer,
            frame_step: 2,
            detect_gaps: true,
            fill_gaps_with_holds: true,
            center_on_stage: true,
            smoothing: true,
            layer_name: Some("Walk Cycle".into()),
        };
        let (proj_kf, rep_kf) =
            import_image_sequence_bytes_as_project("WalkProj", &entries[..3], &opts_kf)
                .expect("sequence keyframes import");
        proj_kf.validate().expect("project validates");
        assert!(
            rep_kf
                .diagnostics
                .iter()
                .any(|d| d.code == "sequence-missing-frame" && d.location == "walk_0003.png")
        );
        let root_kf = proj_kf.symbol(&proj_kf.root).unwrap();
        let layer_kf = &root_kf.timeline.layers[0];
        assert_eq!(layer_kf.name, "Walk Cycle");
        let frames: Vec<u32> = layer_kf.keyframes.iter().map(|k| k.frame).collect();
        // 0001 -> frame 0, 0002 -> frame 2, 0003 missing (held across 4..6), 0004 -> frame 6!
        assert_eq!(frames, vec![0, 2, 6]);

        // 2. SeparateLayers mode
        let opts_layers = ImageSequenceImportOptions {
            placement: ImageSequencePlacement::SeparateLayers,
            ..ImageSequenceImportOptions::default()
        };
        let (proj_layers, _) =
            import_image_sequence_bytes_as_project("LayersProj", &entries[..3], &opts_layers)
                .expect("separate layers import");
        proj_layers.validate().expect("project validates");
        assert_eq!(
            proj_layers
                .symbol(&proj_layers.root)
                .unwrap()
                .timeline
                .layers
                .len(),
            3
        );

        // 3. ConvertToGraphicSymbol mode + unreadable frame warning
        let bad_entries = vec![
            ("frame-1.webp".to_string(), png1),
            ("frame-2.webp".to_string(), b"corrupt-webp-bytes".to_vec()),
        ];
        let opts_sym = ImageSequenceImportOptions {
            placement: ImageSequencePlacement::ConvertToGraphicSymbol,
            ..ImageSequenceImportOptions::default()
        };
        let (proj_sym, rep_sym) =
            import_image_sequence_bytes_as_project("SymProj", &bad_entries, &opts_sym)
                .expect("graphic symbol import");
        proj_sym
            .validate()
            .expect("project with placeholder validates");
        assert!(
            rep_sym
                .diagnostics
                .iter()
                .any(|d| d.code == "sequence-unreadable-frame" && d.location == "frame-2.webp")
        );
        let graphic_sym = proj_sym
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Graphic)
            .expect("Graphic symbol created");
        assert_eq!(graphic_sym.timeline.layers[0].keyframes.len(), 2);
        let root_elem =
            &proj_sym.symbol(&proj_sym.root).unwrap().timeline.layers[0].keyframes[0].elements[0];
        assert!(matches!(
            root_elem.kind,
            ElementKind::Instance {
                playback: InstancePlayback::Loop,
                ..
            }
        ));
    }

    /// Build a synthetic binary `.psd` file with multiple layers, raw + PackBits RLE channels,
    /// blend modes, clipping masks, `TySh` text layer, `vmsk` vector mask, and hidden layer.
    fn build_synthetic_psd() -> Vec<u8> {
        let mut out = Vec::new();
        // Header (26 bytes)
        out.extend_from_slice(b"8BPS");
        out.extend_from_slice(&1u16.to_be_bytes()); // version 1
        out.extend_from_slice(&[0u8; 6]); // reserved
        out.extend_from_slice(&4u16.to_be_bytes()); // 4 channels
        out.extend_from_slice(&16u32.to_be_bytes()); // height = 16
        out.extend_from_slice(&16u32.to_be_bytes()); // width = 16
        out.extend_from_slice(&8u16.to_be_bytes()); // depth = 8
        out.extend_from_slice(&3u16.to_be_bytes()); // RGB color mode = 3

        // Color Mode Data (empty)
        out.extend_from_slice(&0u32.to_be_bytes());
        // Image Resources (empty)
        out.extend_from_slice(&0u32.to_be_bytes());

        // Prepare 4 layers (bottom-to-top):
        // Layer 0: Base mask layer (4x4, raw compression 0, visible, clipping = 0, blend = "norm")
        // Layer 1: Clipped multiply layer (4x4, RLE compression 1, visible, clipping = 1, blend = "mult", vmsk block)
        // Layer 2: Text layer (4x4, raw compression 0, visible, clipping = 0, blend = "soft" unsupported -> approximated, TySh block)
        // Layer 3: Hidden layer (4x4, raw compression 0, hidden flag 0x02, blend = "scrn")

        struct SynthLayer {
            name: &'static str,
            top: i32,
            left: i32,
            bottom: i32,
            right: i32,
            blend: &'static [u8; 4],
            opacity: u8,
            clipping: u8,
            flags: u8,
            extra_blocks: Vec<u8>,
            channel_blobs: Vec<(i16, Vec<u8>)>,
        }

        let make_raw_channel = |val: u8, count: usize| -> Vec<u8> {
            let mut b = Vec::with_capacity(2 + count);
            b.extend_from_slice(&0u16.to_be_bytes()); // raw compression = 0
            b.extend(std::iter::repeat_n(val, count));
            b
        };

        let rle_r = encode_packbits_channel(&[200u8; 16], 4, 4);
        let rle_g = encode_packbits_channel(&[100u8; 16], 4, 4);
        let rle_b = encode_packbits_channel(&[50u8; 16], 4, 4);
        let rle_a = encode_packbits_channel(&[255u8; 16], 4, 4);

        // Extra block for vmsk on Layer 1
        let mut vmsk_block = Vec::new();
        vmsk_block.extend_from_slice(b"8BIMvmsk");
        vmsk_block.extend_from_slice(&8u32.to_be_bytes());
        vmsk_block.extend_from_slice(&[0u8; 8]);

        // Extra block for TySh + luni on Layer 2
        let mut tysh_block = Vec::new();
        tysh_block.extend_from_slice(b"8BIMTySh");
        let mut tysh_payload = Vec::new();
        tysh_payload.extend_from_slice(&1u16.to_be_bytes()); // version 1
        for &v in &[1.0f64, 0.0, 0.0, 1.0, 12.0, 14.0] {
            tysh_payload.extend_from_slice(&v.to_bits().to_be_bytes());
        }
        tysh_payload.extend_from_slice(b"Txt TEXT");
        let utf16: Vec<u16> = "Hello PSD!".encode_utf16().collect();
        tysh_payload.extend_from_slice(&(utf16.len() as u32).to_be_bytes());
        for u in utf16 {
            tysh_payload.extend_from_slice(&u.to_be_bytes());
        }
        if tysh_payload.len() % 2 != 0 {
            tysh_payload.push(0);
        }
        tysh_block.extend_from_slice(&(tysh_payload.len() as u32).to_be_bytes());
        tysh_block.extend_from_slice(&tysh_payload);

        let synth_layers = vec![
            SynthLayer {
                name: "MaskBase",
                top: 0,
                left: 0,
                bottom: 4,
                right: 4,
                blend: b"norm",
                opacity: 255,
                clipping: 0,
                flags: 0,
                extra_blocks: Vec::new(),
                channel_blobs: vec![
                    (0, make_raw_channel(255, 16)),
                    (1, make_raw_channel(255, 16)),
                    (2, make_raw_channel(255, 16)),
                    (-1, make_raw_channel(255, 16)),
                ],
            },
            SynthLayer {
                name: "ClippedRLE",
                top: 2,
                left: 2,
                bottom: 6,
                right: 6,
                blend: b"mult",
                opacity: 200,
                clipping: 1,
                flags: 0,
                extra_blocks: vmsk_block,
                channel_blobs: vec![(0, rle_r), (1, rle_g), (2, rle_b), (-1, rle_a)],
            },
            SynthLayer {
                name: "TitleText",
                top: 4,
                left: 4,
                bottom: 8,
                right: 8,
                blend: b"soft",
                opacity: 255,
                clipping: 0,
                flags: 0,
                extra_blocks: tysh_block,
                channel_blobs: vec![
                    (0, make_raw_channel(20, 16)),
                    (1, make_raw_channel(40, 16)),
                    (2, make_raw_channel(60, 16)),
                    (-1, make_raw_channel(255, 16)),
                ],
            },
            SynthLayer {
                name: "HiddenDraft",
                top: 0,
                left: 0,
                bottom: 4,
                right: 4,
                blend: b"scrn",
                opacity: 128,
                clipping: 0,
                flags: 0x02, // hidden!
                extra_blocks: Vec::new(),
                channel_blobs: vec![
                    (0, make_raw_channel(10, 16)),
                    (1, make_raw_channel(10, 16)),
                    (2, make_raw_channel(10, 16)),
                    (-1, make_raw_channel(255, 16)),
                ],
            },
        ];

        let mut layer_info = Vec::new();
        layer_info.extend_from_slice(&(synth_layers.len() as i16).to_be_bytes());

        for sl in &synth_layers {
            layer_info.extend_from_slice(&sl.top.to_be_bytes());
            layer_info.extend_from_slice(&sl.left.to_be_bytes());
            layer_info.extend_from_slice(&sl.bottom.to_be_bytes());
            layer_info.extend_from_slice(&sl.right.to_be_bytes());
            layer_info.extend_from_slice(&(sl.channel_blobs.len() as u16).to_be_bytes());
            for (ch_id, blob) in &sl.channel_blobs {
                layer_info.extend_from_slice(&ch_id.to_be_bytes());
                layer_info.extend_from_slice(&(blob.len() as u32).to_be_bytes());
            }
            layer_info.extend_from_slice(b"8BIM");
            layer_info.extend_from_slice(sl.blend);
            layer_info.push(sl.opacity);
            layer_info.push(sl.clipping);
            layer_info.push(sl.flags);
            layer_info.push(0); // filler

            let mut extra = Vec::new();
            extra.extend_from_slice(&0u32.to_be_bytes()); // mask data len
            extra.extend_from_slice(&0u32.to_be_bytes()); // blending ranges len
            let name_bytes = sl.name.as_bytes();
            extra.push(name_bytes.len() as u8);
            extra.extend_from_slice(name_bytes);
            let pascal_total = 1 + name_bytes.len();
            let pad = (4 - (pascal_total % 4)) % 4;
            extra.extend(std::iter::repeat_n(0u8, pad));
            extra.extend_from_slice(&sl.extra_blocks);

            layer_info.extend_from_slice(&(extra.len() as u32).to_be_bytes());
            layer_info.extend_from_slice(&extra);
        }

        // Append Channel Image Data
        for sl in &synth_layers {
            for (_, blob) in &sl.channel_blobs {
                layer_info.extend_from_slice(blob);
            }
        }

        let mut layer_mask_section = Vec::new();
        layer_mask_section.extend_from_slice(&(layer_info.len() as u32).to_be_bytes());
        layer_mask_section.extend_from_slice(&layer_info);

        out.extend_from_slice(&(layer_mask_section.len() as u32).to_be_bytes());
        out.extend_from_slice(&layer_mask_section);

        // Merged Composite Image Data (16x16x4 raw)
        out.extend_from_slice(&0u16.to_be_bytes()); // raw compression
        out.extend(std::iter::repeat_n(128u8, 16 * 16 * 4));
        out
    }

    #[test]
    fn psd_imports_layers_clipping_masks_rle_text_and_flatten_modes_with_diagnostics() {
        let psd_bytes = build_synthetic_psd();

        // 1. TimelineLayers + EditableText + skip hidden layers
        let opts_layers = PsdImportOptions {
            layer_mapping: PsdLayerMapping::TimelineLayers,
            text_handling: PsdTextHandling::EditableText,
            import_hidden_layers: false,
            preserve_blend_modes: true,
            preserve_clipping_masks: true,
            resize_stage_to_canvas: true,
        };
        let (proj, rep) =
            import_psd_bytes(&psd_bytes, "Character.psd", &opts_layers).expect("PSD import");
        proj.validate().expect("PSD project validates");
        assert_eq!(proj.width, 16);
        assert_eq!(proj.height, 16);

        let root = proj.symbol(&proj.root).unwrap();
        // 3 visible layers (HiddenDraft skipped)
        assert_eq!(root.timeline.layers.len(), 3);
        assert!(!root.timeline.layers.iter().any(|l| l.name == "HiddenDraft"));

        // Verify TySh text layer became EditableText
        let text_layer = root
            .timeline
            .layers
            .iter()
            .find(|l| l.name == "TitleText")
            .expect("TitleText layer present");
        match &text_layer.keyframes[0].elements[0].kind {
            ElementKind::Text { text, .. } => assert_eq!(text, "Hello PSD!"),
            other => panic!("Expected ElementKind::Text, got {other:?}"),
        }

        // Verify clipping mask relationship between MaskBase and ClippedRLE
        let mask_base = root
            .timeline
            .layers
            .iter()
            .find(|l| l.name == "MaskBase")
            .expect("MaskBase layer");
        let clipped_rle = root
            .timeline
            .layers
            .iter()
            .find(|l| l.name == "ClippedRLE")
            .expect("ClippedRLE layer");
        assert_eq!(mask_base.properties.role, LayerRole::Mask);
        assert_eq!(mask_base.properties.mask_mode, MaskMode::Alpha);
        assert_eq!(
            clipped_rle.properties.mask.as_deref(),
            Some(mask_base.id.as_str())
        );
        assert_eq!(clipped_rle.properties.blend_mode, BlendMode::Multiply);

        // Verify fidelity warnings for 'vmsk' and 'soft' blend mode
        assert!(
            rep.diagnostics
                .iter()
                .any(|d| d.code == "psd-vector-mask-approximated")
        );
        assert!(
            rep.diagnostics
                .iter()
                .any(|d| d.code == "psd-unsupported-blend-mode")
        );

        // 2. MovieClipSymbols + import_hidden_layers = true + VectorOutlines
        let opts_mc = PsdImportOptions {
            layer_mapping: PsdLayerMapping::MovieClipSymbols,
            text_handling: PsdTextHandling::VectorOutlines,
            import_hidden_layers: true,
            ..PsdImportOptions::default()
        };
        let (proj_mc, rep_mc) =
            import_psd_bytes(&psd_bytes, "Character.psd", &opts_mc).expect("PSD MC import");
        proj_mc.validate().expect("PSD MC project validates");
        assert_eq!(
            proj_mc.symbol(&proj_mc.root).unwrap().timeline.layers.len(),
            4
        );
        assert!(
            rep_mc
                .diagnostics
                .iter()
                .any(|d| d.code == "psd-text-outlines-approximated")
        );

        // 3. Keyframes mode
        let opts_kf = PsdImportOptions {
            layer_mapping: PsdLayerMapping::Keyframes,
            import_hidden_layers: false,
            ..PsdImportOptions::default()
        };
        let (proj_kf, _) =
            import_psd_bytes(&psd_bytes, "Character.psd", &opts_kf).expect("PSD Keyframes import");
        proj_kf.validate().expect("PSD Keyframes project validates");
        let kf_layer = &proj_kf.symbol(&proj_kf.root).unwrap().timeline.layers[0];
        assert_eq!(kf_layer.keyframes.len(), 3);

        // 4. FlattenComposite mode
        let opts_flat = PsdImportOptions {
            layer_mapping: PsdLayerMapping::FlattenComposite,
            ..PsdImportOptions::default()
        };
        let (proj_flat, _) =
            import_psd_bytes(&psd_bytes, "Character.psd", &opts_flat).expect("PSD Flatten import");
        proj_flat.validate().expect("PSD Flatten project validates");
        assert_eq!(
            proj_flat
                .symbol(&proj_flat.root)
                .unwrap()
                .timeline
                .layers
                .len(),
            1
        );
    }

    fn minimal_flat_psd(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"8BPS");
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&4u16.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&8u16.to_be_bytes());
        bytes.extend_from_slice(&3u16.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes
    }

    fn single_layer_psd(bounds: [i32; 4], channel: Vec<u8>) -> Vec<u8> {
        let mut bytes = minimal_flat_psd(16, 16);
        bytes.truncate(bytes.len() - 4);

        let mut layer_info = Vec::new();
        layer_info.extend_from_slice(&1i16.to_be_bytes());
        for coordinate in bounds {
            layer_info.extend_from_slice(&coordinate.to_be_bytes());
        }
        layer_info.extend_from_slice(&1u16.to_be_bytes());
        layer_info.extend_from_slice(&0i16.to_be_bytes());
        layer_info.extend_from_slice(&(channel.len() as u32).to_be_bytes());
        layer_info.extend_from_slice(b"8BIMnorm");
        layer_info.extend_from_slice(&[255, 0, 0, 0]);
        layer_info.extend_from_slice(&12u32.to_be_bytes());
        layer_info.extend_from_slice(&0u32.to_be_bytes());
        layer_info.extend_from_slice(&0u32.to_be_bytes());
        layer_info.extend_from_slice(&[1, b'X', 0, 0]);
        layer_info.extend_from_slice(&channel);

        let mut layer_mask = Vec::new();
        layer_mask.extend_from_slice(&(layer_info.len() as u32).to_be_bytes());
        layer_mask.extend_from_slice(&layer_info);
        bytes.extend_from_slice(&(layer_mask.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&layer_mask);
        bytes
    }

    fn two_large_uniform_layers_psd(width: usize, height: usize) -> Vec<u8> {
        let mut bytes = minimal_flat_psd(width as u32, height as u32);
        bytes.truncate(bytes.len() - 4);
        let pixels = vec![7u8; width * height];
        let channel = encode_packbits_channel(&pixels, width, height);

        let mut layer_info = Vec::new();
        layer_info.extend_from_slice(&2i16.to_be_bytes());
        for name in *b"AB" {
            layer_info.extend_from_slice(&0i32.to_be_bytes());
            layer_info.extend_from_slice(&0i32.to_be_bytes());
            layer_info.extend_from_slice(&(height as i32).to_be_bytes());
            layer_info.extend_from_slice(&(width as i32).to_be_bytes());
            layer_info.extend_from_slice(&1u16.to_be_bytes());
            layer_info.extend_from_slice(&0i16.to_be_bytes());
            layer_info.extend_from_slice(&(channel.len() as u32).to_be_bytes());
            layer_info.extend_from_slice(b"8BIMnorm");
            layer_info.extend_from_slice(&[255, 0, 0, 0]);
            layer_info.extend_from_slice(&12u32.to_be_bytes());
            layer_info.extend_from_slice(&0u32.to_be_bytes());
            layer_info.extend_from_slice(&0u32.to_be_bytes());
            layer_info.extend_from_slice(&[1, name, 0, 0]);
        }
        layer_info.extend_from_slice(&channel);
        layer_info.extend_from_slice(&channel);

        let mut layer_mask = Vec::new();
        layer_mask.extend_from_slice(&(layer_info.len() as u32).to_be_bytes());
        layer_mask.extend_from_slice(&layer_info);
        bytes.extend_from_slice(&(layer_mask.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&layer_mask);
        bytes
    }

    #[test]
    fn psd_and_sequence_imports_bound_large_images_and_commit_atomically() {
        let oversized = minimal_flat_psd(neoflash_core::MAX_BITMAP_DIMENSION + 1, 16);
        let (project, report) =
            import_psd_bytes(&oversized, "oversized.psd", &PsdImportOptions::default())
                .expect("oversized PSD is represented safely");
        project.validate().expect("placeholder project validates");
        assert!(project.resources.is_empty());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "psd-composite-bitmap-too-large")
        );
        assert!(matches!(
            &project.symbol(&project.root).unwrap().timeline.layers[0].keyframes[0].elements[0]
                .kind,
            ElementKind::Rectangle { .. }
        ));

        let extreme = single_layer_psd(
            [i32::MIN, i32::MIN, i32::MAX, i32::MAX],
            0u16.to_be_bytes().to_vec(),
        );
        let (extreme_project, extreme_report) =
            import_psd_bytes(&extreme, "extreme.psd", &PsdImportOptions::default())
                .expect("extreme layer bounds are represented by a placeholder");
        extreme_project
            .validate()
            .expect("extreme layer placeholder validates");
        assert!(
            extreme_report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "psd-layer-bitmap-too-large")
        );

        let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        zlib.write_all(&[1, 2]).unwrap();
        let mut over_expanding_channel = 2u16.to_be_bytes().to_vec();
        over_expanding_channel.extend_from_slice(&zlib.finish().unwrap());
        let compressed = single_layer_psd([0, 0, 1, 1], over_expanding_channel);
        let (_, compressed_layers, _) =
            parse_psd_document(&compressed).expect("bounded zlib channel parses safely");
        assert_eq!(compressed_layers.len(), 1);
        assert!(
            compressed_layers[0].rgba_pixels.is_none(),
            "a channel expanding past its declared dimensions must not be retained"
        );
        let (_, compressed_report) = import_psd_bytes(
            &compressed,
            "over-expanding.psd",
            &PsdImportOptions::default(),
        )
        .expect("malformed channel receives an explicit placeholder");
        assert!(
            compressed_report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "psd-layer-pixels-unavailable")
        );

        let aggregate = two_large_uniform_layers_psd(4096, 4096);
        let (aggregate_project, aggregate_report) = import_psd_bytes(
            &aggregate,
            "aggregate-budget.psd",
            &PsdImportOptions::default(),
        )
        .expect("aggregate PSD decode budget preserves the document safely");
        aggregate_project
            .validate()
            .expect("aggregate-budget placeholders validate");
        assert_eq!(
            aggregate_project
                .symbol(&aggregate_project.root)
                .unwrap()
                .timeline
                .layers
                .len(),
            2,
            "budget exhaustion must retain both PSD layers"
        );
        assert!(
            aggregate_report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "psd-layer-decode-budget-exceeded")
        );

        let temp_root = std::env::temp_dir();
        let oversized_image_path = temp_root.join(format!(
            "neoflash-oversized-sequence-{}.png",
            std::process::id()
        ));
        let oversized_image = std::fs::File::create(&oversized_image_path).unwrap();
        oversized_image
            .set_len((neoflash_core::MAX_RESOURCE_BYTES + 1) as u64)
            .unwrap();
        drop(oversized_image);
        let mut path_target = Project::blank();
        let image_error = import_image_sequence(
            &mut path_target,
            &[&oversized_image_path],
            &ImageSequenceImportOptions::default(),
        )
        .expect_err("filesystem sequence wrapper rejects oversized input before reading it");
        std::fs::remove_file(&oversized_image_path).unwrap();
        assert!(image_error.to_string().contains("resource limit"));

        let oversized_psd_path =
            temp_root.join(format!("neoflash-oversized-psd-{}.psd", std::process::id()));
        let oversized_psd = std::fs::File::create(&oversized_psd_path).unwrap();
        oversized_psd
            .set_len((crate::psd::MAX_PSD_FILE_BYTES + 1) as u64)
            .unwrap();
        drop(oversized_psd);
        let psd_error = import_psd_path(&oversized_psd_path, &PsdImportOptions::default())
            .expect_err("filesystem PSD wrapper rejects oversized input before reading it");
        std::fs::remove_file(&oversized_psd_path).unwrap();
        assert!(psd_error.to_string().contains("file limit"));

        let png = make_test_png(2, 2, [1, 2, 3, 255]);
        let entries = vec![
            ("frame_0001.png".to_owned(), png.clone()),
            ("frame_0002.png".to_owned(), png),
        ];
        let mut sequence_target = Project::blank();
        for index in 0..(neoflash_core::MAX_RESOURCES - 1) {
            sequence_target.resources.push(
                ResourceUnit::from_bytes(
                    format!("seed-{index}"),
                    format!("seed/{index}.bin"),
                    "application/octet-stream",
                    b"x",
                )
                .unwrap(),
            );
        }
        sequence_target.format_version = 3;
        sequence_target.validate().expect("seed project validates");
        let sequence_before = serde_json::to_vec(&sequence_target).unwrap();
        assert!(
            import_image_sequence_bytes(
                &mut sequence_target,
                &entries,
                &ImageSequenceImportOptions::default(),
            )
            .is_err()
        );
        assert_eq!(
            serde_json::to_vec(&sequence_target).unwrap(),
            sequence_before,
            "the second frame crossing the resource limit must not retain the first frame"
        );

        let mut invalid_psd_target = Project::blank();
        invalid_psd_target.width = 0;
        let psd_before = serde_json::to_string(&invalid_psd_target).unwrap();
        let options = PsdImportOptions {
            resize_stage_to_canvas: false,
            ..PsdImportOptions::default()
        };
        assert!(
            import_psd_into_project(
                &mut invalid_psd_target,
                &minimal_flat_psd(16, 16),
                "valid.psd",
                &options,
            )
            .is_err()
        );
        assert_eq!(
            serde_json::to_string(&invalid_psd_target).unwrap(),
            psd_before,
            "failed PSD import must not partially mutate the destination"
        );
    }

    #[test]
    fn svg_and_pdf_ai_vector_workflows_respect_layer_symbol_text_mask_and_transform_options() {
        let svg = r##"<svg viewBox="0 0 200 100" xmlns="http://www.w3.org/2000/svg">
            <defs>
                <clipPath id="badgeClip">
                    <rect x="0" y="0" width="100" height="50" />
                </clipPath>
                <filter id="glow">
                    <feGaussianBlur stdDeviation="3" />
                </filter>
            </defs>
            <g id="BackgroundGroup" clip-path="url(#badgeClip)" filter="url(#glow)" transform="translate(10, 20)">
                <rect id="bgRect" x="0" y="0" width="80" height="40" fill="#336699" stroke="#ffffff" stroke-width="2" />
            </g>
            <g id="ForegroundGroup" transform="translate(30, 40)">
                <path id="starPath" d="M 0 0 L 20 0 L 20 20 Z" fill="#ffcc00" />
                <text id="label" x="5" y="15" font-size="18" fill="#112233">NeoFlash</text>
                <foreignObject width="10" height="10"></foreignObject>
            </g>
        </svg>"##;

        // 1. PreserveGroupsAsLayers + MaskLayers + EditableText + KeepElementTransforms
        let opts_svg = VectorImportOptions {
            layer_mapping: VectorLayerMapping::PreserveGroupsAsLayers,
            text_handling: VectorTextHandling::EditableText,
            mask_handling: VectorMaskHandling::MaskLayers,
            transform_handling: VectorTransformHandling::KeepElementTransforms,
            resize_stage_to_viewbox: true,
        };
        let (proj_svg, rep_svg) =
            import_svg_with_options(svg, "Badge.svg", &opts_svg).expect("SVG import");
        proj_svg.validate().expect("SVG project validates");
        assert_eq!(proj_svg.width, 200);
        assert_eq!(proj_svg.height, 100);
        assert!(
            rep_svg
                .diagnostics
                .iter()
                .any(|d| d.code == "svg-mask-layer-created")
        );
        assert!(
            rep_svg
                .diagnostics
                .iter()
                .any(|d| d.code == "svg-filter-approximated")
        );
        assert!(
            rep_svg
                .diagnostics
                .iter()
                .any(|d| d.code == "svg-unsupported-element")
        );

        let root_svg = proj_svg.symbol(&proj_svg.root).unwrap();
        // ForegroundGroup + BackgroundGroup Mask + BackgroundGroup = 3 layers
        assert_eq!(root_svg.timeline.layers.len(), 3);
        assert!(
            root_svg
                .timeline
                .layers
                .iter()
                .any(|l| l.properties.role == LayerRole::Mask)
        );

        // 2. ConvertGroupsToSymbols + ConvertToPaths + FlattenClip + BakeTransformsIntoPaths
        let opts_sym = VectorImportOptions {
            layer_mapping: VectorLayerMapping::ConvertGroupsToSymbols,
            text_handling: VectorTextHandling::ConvertToPaths,
            mask_handling: VectorMaskHandling::FlattenClip,
            transform_handling: VectorTransformHandling::BakeTransformsIntoPaths,
            resize_stage_to_viewbox: true,
        };
        let (proj_sym, rep_sym) =
            import_svg_with_options(svg, "Badge.svg", &opts_sym).expect("SVG symbols import");
        proj_sym.validate().expect("SVG symbols project validates");
        assert!(
            rep_sym
                .diagnostics
                .iter()
                .any(|d| d.code == "svg-text-converted-to-paths")
        );
        assert!(
            rep_sym
                .diagnostics
                .iter()
                .any(|d| d.code == "svg-clip-flattened")
        );
        assert_eq!(proj_sym.symbols.len(), 3); // root + 2 group MovieClip symbols

        // 3. PDF / Adobe Illustrator content stream import
        let pdf_ai = b"%PDF-1.4\n\
            1 0 obj << /Type /Page /MediaBox [0 0 320 240] >> endobj\n\
            2 0 obj << /Length 220 >>\n\
            stream\n\
            /Layer1 BDC\n\
            q\n\
            0 0 100 100 re W n\n\
            0.2 0.4 0.8 rg\n\
            0.9 0.1 0.1 RG\n\
            2 w\n\
            10 10 m 90 10 l 90 70 l 10 70 l h B\n\
            BT /F1 18 Tf 15 30 Td (Vector AI) Tj ET\n\
            /Sh1 sh\n\
            Q\n\
            EMC\n\
            endstream\n\
            endobj\n";

        let (proj_pdf, rep_pdf) =
            import_pdf_ai_with_options(pdf_ai, "Artwork.ai", &opts_svg).expect("PDF/AI import");
        proj_pdf.validate().expect("PDF/AI project validates");
        assert_eq!(proj_pdf.width, 320);
        assert_eq!(proj_pdf.height, 240);
        assert!(
            rep_pdf
                .diagnostics
                .iter()
                .any(|d| d.code == "pdf-content-stream-parsed")
        );
        assert!(
            rep_pdf
                .diagnostics
                .iter()
                .any(|d| d.code == "pdf-clipping-path-mapped")
        );
        assert!(
            rep_pdf
                .diagnostics
                .iter()
                .any(|d| d.code == "pdf-unsupported-operator")
        );
        let root_pdf = proj_pdf.symbol(&proj_pdf.root).unwrap();
        assert_eq!(root_pdf.timeline.layers.len(), 2); // Mask layer + Layer1
    }
}
