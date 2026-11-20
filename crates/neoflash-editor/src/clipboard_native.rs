//! Native clipboard ownership metadata for the macOS arm64 rich-text cache.

#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use objc2::{
    ClassType, msg_send,
    rc::{Retained, autoreleasepool},
};
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeHTML, NSPasteboardTypeString};
use objc2_foundation::NSString;

const MAX_STAGE_PASTE_FLAVOR_BYTES: usize = 64 * 1024 * 1024;
const MAX_FIGMA_HTML_FLAVOR_BYTES: usize = 512 * 1024 * 1024;

fn general_pasteboard() -> Option<Retained<NSPasteboard>> {
    // SAFETY: This is NSPasteboard's documented class method. Accept nil as well:
    // the pasteboard service can be unavailable outside a normal GUI session.
    unsafe { msg_send![NSPasteboard::class(), generalPasteboard] }
}

fn board_generation(board: &NSPasteboard) -> Option<u64> {
    u64::try_from(board.changeCount()).ok()
}

pub(crate) enum WriteResult {
    Written(u64),
    Unavailable,
    Superseded,
}

fn write_to(board: &NSPasteboard, text: &str) -> WriteResult {
    autoreleasepool(|_| {
        let Ok(generation) = u64::try_from(board.clearContents()) else {
            return WriteResult::Unavailable;
        };
        let text = NSString::from_str(text);
        // SAFETY: AppKit provides this immutable pasteboard type constant.
        let text_type = unsafe { NSPasteboardTypeString };
        let written = board.setString_forType(&text, text_type);
        // Another owner may have replaced the clipboard during the write. Never
        // attach our cached formatting to that owner's generation.
        if board_generation(board) != Some(generation) {
            WriteResult::Superseded
        } else if written {
            WriteResult::Written(generation)
        } else {
            WriteResult::Unavailable
        }
    })
}

/// Write the plain-text representation and return the generation we still own.
pub(crate) fn write_text(text: &str) -> WriteResult {
    autoreleasepool(|_| {
        general_pasteboard().map_or(WriteResult::Unavailable, |board| write_to(&board, text))
    })
}

/// Read ownership metadata only; clipboard contents are never requested here.
pub(crate) fn generation() -> Option<u64> {
    autoreleasepool(|_| {
        let board = general_pasteboard()?;
        board_generation(&board)
    })
}

fn read_string_or_utf8_data(
    board: &NSPasteboard,
    ty: &NSString,
    max_bytes: usize,
) -> Option<String> {
    if let Some(s) = board.stringForType(ty) {
        let owned = s.to_string();
        if !owned.is_empty() && owned.len() <= max_bytes {
            return Some(owned);
        }
    }
    let data = board.dataForType(ty)?;
    let len = data.len();
    if len == 0 || len > max_bytes {
        return None;
    }
    String::from_utf8(data.to_vec())
        .ok()
        .filter(|s| !s.is_empty())
}

pub(crate) fn read_stage_flavors_from(
    board: &NSPasteboard,
    expected_generation: Option<u64>,
) -> Option<super::StageClipboardFlavors> {
    autoreleasepool(|_| {
        let generation = board_generation(board);
        if expected_generation.is_some() && generation != expected_generation {
            return None;
        }
        // SAFETY: Both are immutable pasteboard type constants exported by AppKit.
        let (text_type, html_type) = unsafe { (NSPasteboardTypeString, NSPasteboardTypeHTML) };
        let plain_text = board
            .stringForType(text_type)
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty() && s.len() <= MAX_FIGMA_HTML_FLAVOR_BYTES);

        let html = read_string_or_utf8_data(board, html_type, MAX_FIGMA_HTML_FLAVOR_BYTES)
            .or_else(|| {
                read_string_or_utf8_data(
                    board,
                    &NSString::from_str("public.html"),
                    MAX_FIGMA_HTML_FLAVOR_BYTES,
                )
            })
            .or_else(|| {
                read_string_or_utf8_data(
                    board,
                    &NSString::from_str("text/html"),
                    MAX_FIGMA_HTML_FLAVOR_BYTES,
                )
            });

        let svg = [
            "public.svg-image",
            "com.adobe.svg",
            "image/svg+xml",
            "org.inkscape.svg",
        ]
        .into_iter()
        .find_map(|uti| {
            read_string_or_utf8_data(
                board,
                &NSString::from_str(uti),
                MAX_STAGE_PASTE_FLAVOR_BYTES,
            )
        });

        let wants_png = html
            .as_deref()
            .is_some_and(neoflash_import::contains_figma_clipboard_marker)
            || (plain_text.as_deref().is_none_or(|t| t.trim().is_empty()) && svg.is_none());
        let png = if wants_png {
            board
                .dataForType(&NSString::from_str("public.png"))
                .filter(|d| {
                    let len = d.len();
                    len > 0 && len <= MAX_STAGE_PASTE_FLAVOR_BYTES
                })
                .map(|d| d.to_vec())
        } else {
            None
        };

        if board_generation(board) != generation {
            return None;
        }

        Some(super::StageClipboardFlavors {
            generation,
            plain_text,
            html,
            svg,
            png,
        })
    })
}

/// Read rich Stage pasteboard flavors (`public.html`, `public.svg-image`, `public.png`, plain text)
/// ONLY when an actual Stage Paste / Paste in Place command executes.
#[cfg(not(test))]
pub(crate) fn read_stage_flavors(
    expected_generation: Option<u64>,
) -> Option<super::StageClipboardFlavors> {
    general_pasteboard().and_then(|board| read_stage_flavors_from(&board, expected_generation))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PrivatePasteboard(Retained<NSPasteboard>);

    impl Drop for PrivatePasteboard {
        fn drop(&mut self) {
            // SAFETY: releaseGlobally is NSPasteboard's documented cleanup for
            // named pasteboards. objc2-app-kit does not expose this selector.
            // This guard owns only the unique board created by this test.
            unsafe {
                let _: () = msg_send![&*self.0, releaseGlobally];
            }
        }
    }

    #[test]
    fn equal_plain_text_with_replaced_rich_flavors_changes_generation() {
        autoreleasepool(|_| {
            let private = PrivatePasteboard(NSPasteboard::pasteboardWithUniqueName());
            let board = &private.0;
            let plain = "Same text — λ";
            // SAFETY: Both are immutable type constants exported by AppKit.
            let (text_type, html_type) = unsafe { (NSPasteboardTypeString, NSPasteboardTypeHTML) };

            let WriteResult::Written(owned) = write_to(board, plain) else {
                panic!("write to isolated pasteboard failed");
            };
            assert_eq!(board_generation(board), Some(owned));
            assert_eq!(board.stringForType(text_type).unwrap().to_string(), plain);

            let mut previous = owned;
            for html in ["<b>Same text — λ</b>", "<i>Same text — λ</i>"] {
                // Model a different application's complete clipboard write:
                // identical plain text, but a different rich representation.
                let replaced = u64::try_from(board.clearContents()).unwrap();
                assert!(board.setString_forType(&NSString::from_str(html), html_type));
                assert!(board.setString_forType(&NSString::from_str(plain), text_type));
                assert_ne!(replaced, previous);
                assert_ne!(replaced, owned);
                assert_eq!(board_generation(board), Some(replaced));
                assert_eq!(
                    board.stringForType(text_type).unwrap().to_string(),
                    plain,
                    "the plain-text fallback remains available"
                );
                assert_eq!(board.stringForType(html_type).unwrap().to_string(), html);
                previous = replaced;
            }

            let WriteResult::Written(recopied) = write_to(board, plain) else {
                panic!("copy again to isolated pasteboard failed");
            };
            assert_ne!(recopied, previous);
            assert_eq!(board_generation(board), Some(recopied));
            assert_eq!(board.stringForType(text_type).unwrap().to_string(), plain);
            assert!(board.stringForType(html_type).is_none());

            // Exercise the GUI bridge with this isolated board: the latest
            // Copy wins over deferred output, including an earlier bitmap.
            let ctx = eframe::egui::Context::default();
            ctx.copy_text("earlier copy".into());
            ctx.copy_image(eframe::egui::ColorImage::filled(
                [1, 1],
                eframe::egui::Color32::WHITE,
            ));
            let bridge_generation =
                super::super::copy_native_rich_text(&ctx, plain, || write_to(board, plain));
            assert!(bridge_generation.is_some());
            assert_eq!(bridge_generation, board_generation(board));
            assert!(ctx.output(|output| output.commands.is_empty()));
            assert_eq!(board.stringForType(text_type).unwrap().to_string(), plain);

            assert_eq!(
                super::super::copy_native_rich_text(&ctx, plain, || WriteResult::Unavailable),
                None
            );
            assert!(ctx.output(|output| matches!(
                output.commands.as_slice(),
                [eframe::egui::OutputCommand::CopyText(text)] if text == plain
            )));
            assert_eq!(
                super::super::copy_native_rich_text(&ctx, plain, || WriteResult::Superseded),
                None
            );
            assert!(ctx.output(|output| output.commands.is_empty()));
        });
    }

    fn build_native_test_figma_html_payload() -> (String, Vec<u8>) {
        fn deflate_stored(raw: &[u8]) -> Vec<u8> {
            let mut out = Vec::new();
            let chunks: Vec<&[u8]> = if raw.is_empty() {
                vec![&[]]
            } else {
                raw.chunks(65_535).collect()
            };
            for (idx, chunk) in chunks.iter().enumerate() {
                let is_last = idx + 1 == chunks.len();
                out.push(u8::from(is_last));
                let len = chunk.len() as u16;
                out.extend_from_slice(&len.to_le_bytes());
                out.extend_from_slice(&(!len).to_le_bytes());
                out.extend_from_slice(chunk);
            }
            out
        }
        fn b64_encode(bytes: &[u8]) -> String {
            neoflash_core::ResourceUnit::from_bytes(
                "tmp",
                "tmp.bin",
                "application/octet-stream",
                bytes,
            )
            .unwrap()
            .data_base64
            .to_string()
        }

        fn wu(out: &mut Vec<u8>, mut v: u64) {
            loop {
                let mut b = (v & 0x7F) as u8;
                v >>= 7;
                if v != 0 {
                    b |= 0x80;
                }
                out.push(b);
                if v == 0 {
                    break;
                }
            }
        }
        fn wi(out: &mut Vec<u8>, v: i64) {
            wu(out, ((v << 1) ^ (v >> 63)) as u64);
        }
        fn wf(out: &mut Vec<u8>, f: f32) {
            if f == 0.0 {
                out.push(0);
            } else {
                out.extend_from_slice(&f.to_bits().rotate_left(9).to_le_bytes());
            }
        }
        fn ws(out: &mut Vec<u8>, s: &str) {
            out.extend_from_slice(s.as_bytes());
            out.push(0);
        }

        // Schema: 0=GUID, 1=ParentIndex, 2=Vector, 3=Matrix, 4=Color, 5=Paint, 6=Path, 7=FontName, 8=TextData, 9=NodeChange, 10=Blob, 11=Message
        let mut schema = Vec::new();
        wu(&mut schema, 12);
        ws(&mut schema, "GUID");
        schema.push(1);
        wu(&mut schema, 2);
        for (i, n) in ["sessionID", "localID"].iter().enumerate() {
            ws(&mut schema, n);
            wi(&mut schema, -4);
            schema.push(0);
            wu(&mut schema, (i + 1) as u64);
        }
        ws(&mut schema, "ParentIndex");
        schema.push(1);
        wu(&mut schema, 2);
        ws(&mut schema, "guid");
        wi(&mut schema, 0);
        schema.push(0);
        wu(&mut schema, 1);
        ws(&mut schema, "position");
        wi(&mut schema, -6);
        schema.push(0);
        wu(&mut schema, 2);
        ws(&mut schema, "Vector");
        schema.push(1);
        wu(&mut schema, 2);
        for (i, n) in ["x", "y"].iter().enumerate() {
            ws(&mut schema, n);
            wi(&mut schema, -5);
            schema.push(0);
            wu(&mut schema, (i + 1) as u64);
        }
        ws(&mut schema, "Matrix");
        schema.push(1);
        wu(&mut schema, 6);
        for (i, n) in ["m00", "m01", "m02", "m10", "m11", "m12"]
            .iter()
            .enumerate()
        {
            ws(&mut schema, n);
            wi(&mut schema, -5);
            schema.push(0);
            wu(&mut schema, (i + 1) as u64);
        }
        ws(&mut schema, "Color");
        schema.push(1);
        wu(&mut schema, 4);
        for (i, n) in ["r", "g", "b", "a"].iter().enumerate() {
            ws(&mut schema, n);
            wi(&mut schema, -5);
            schema.push(0);
            wu(&mut schema, (i + 1) as u64);
        }
        ws(&mut schema, "Paint");
        schema.push(2);
        wu(&mut schema, 2);
        ws(&mut schema, "type");
        wi(&mut schema, -6);
        schema.push(0);
        wu(&mut schema, 1);
        ws(&mut schema, "color");
        wi(&mut schema, 4);
        schema.push(0);
        wu(&mut schema, 2);
        ws(&mut schema, "Path");
        schema.push(2);
        wu(&mut schema, 2);
        ws(&mut schema, "windingRule");
        wi(&mut schema, -6);
        schema.push(0);
        wu(&mut schema, 1);
        ws(&mut schema, "commandsBlob");
        wi(&mut schema, -4);
        schema.push(0);
        wu(&mut schema, 2);
        ws(&mut schema, "FontName");
        schema.push(1);
        wu(&mut schema, 2);
        for (i, n) in ["family", "style"].iter().enumerate() {
            ws(&mut schema, n);
            wi(&mut schema, -6);
            schema.push(0);
            wu(&mut schema, (i + 1) as u64);
        }
        ws(&mut schema, "TextData");
        schema.push(2);
        wu(&mut schema, 1);
        ws(&mut schema, "characters");
        wi(&mut schema, -6);
        schema.push(0);
        wu(&mut schema, 1);
        ws(&mut schema, "NodeChange");
        schema.push(2);
        let nc = [
            (1u64, "guid", 0i64, 0u8),
            (2, "parentIndex", 1, 0),
            (3, "type", -6, 0),
            (4, "name", -6, 0),
            (5, "size", 2, 0),
            (6, "transform", 3, 0),
            (7, "fillPaints", 5, 1),
            (8, "fillGeometry", 6, 1),
            (9, "textData", 8, 0),
            (10, "fontName", 7, 0),
            (11, "fontSize", -5, 0),
        ];
        wu(&mut schema, nc.len() as u64);
        for (tag, n, tid, arr) in nc {
            ws(&mut schema, n);
            wi(&mut schema, tid);
            schema.push(arr);
            wu(&mut schema, tag);
        }
        ws(&mut schema, "Blob");
        schema.push(2);
        wu(&mut schema, 1);
        ws(&mut schema, "bytes");
        wi(&mut schema, -2);
        schema.push(1);
        wu(&mut schema, 1);
        ws(&mut schema, "Message");
        schema.push(2);
        wu(&mut schema, 2);
        ws(&mut schema, "nodeChanges");
        wi(&mut schema, 9);
        schema.push(1);
        wu(&mut schema, 1);
        ws(&mut schema, "blobs");
        wi(&mut schema, 10);
        schema.push(1);
        wu(&mut schema, 2);

        let mut cmd_blob = Vec::new();
        cmd_blob.push(1);
        for f in [0.0f32, 0.0] {
            cmd_blob.extend_from_slice(&f.to_le_bytes());
        }
        cmd_blob.push(2);
        for f in [90.0f32, 0.0] {
            cmd_blob.extend_from_slice(&f.to_le_bytes());
        }
        cmd_blob.push(4);
        for f in [90.0f32, 40.0, 50.0, 60.0, 0.0, 60.0] {
            cmd_blob.extend_from_slice(&f.to_le_bytes());
        }
        cmd_blob.push(0);

        let mut msg = Vec::new();
        wu(&mut msg, 1);
        wu(&mut msg, 4);
        // DOCUMENT 0:0
        wu(&mut msg, 1);
        wu(&mut msg, 0);
        wu(&mut msg, 0);
        wu(&mut msg, 3);
        ws(&mut msg, "DOCUMENT");
        wu(&mut msg, 4);
        ws(&mut msg, "Doc");
        wu(&mut msg, 0);
        // CANVAS 0:1
        wu(&mut msg, 1);
        wu(&mut msg, 0);
        wu(&mut msg, 1);
        wu(&mut msg, 2);
        wu(&mut msg, 0);
        wu(&mut msg, 0);
        ws(&mut msg, "a");
        wu(&mut msg, 3);
        ws(&mut msg, "CANVAS");
        wu(&mut msg, 4);
        ws(&mut msg, "Page 1");
        wu(&mut msg, 0);
        // VECTOR 1:1 at (40, 50) 90x60
        wu(&mut msg, 1);
        wu(&mut msg, 1);
        wu(&mut msg, 1);
        wu(&mut msg, 2);
        wu(&mut msg, 0);
        wu(&mut msg, 1);
        ws(&mut msg, "a");
        wu(&mut msg, 3);
        ws(&mut msg, "VECTOR");
        wu(&mut msg, 4);
        ws(&mut msg, "Native Bezier");
        wu(&mut msg, 5);
        wf(&mut msg, 90.0);
        wf(&mut msg, 60.0);
        wu(&mut msg, 6);
        for v in [1.0f32, 0.0, 40.0, 0.0, 1.0, 50.0] {
            wf(&mut msg, v);
        }
        wu(&mut msg, 7);
        wu(&mut msg, 1);
        wu(&mut msg, 1);
        ws(&mut msg, "SOLID");
        wu(&mut msg, 2);
        for c in [0.15f32, 0.65, 0.95, 1.0] {
            wf(&mut msg, c);
        }
        wu(&mut msg, 0);
        wu(&mut msg, 8);
        wu(&mut msg, 1);
        wu(&mut msg, 1);
        ws(&mut msg, "NONZERO");
        wu(&mut msg, 2);
        wu(&mut msg, 0);
        wu(&mut msg, 0);
        wu(&mut msg, 0);
        // TEXT 1:2 at (40, 120) 140x32 "Figma Native"
        wu(&mut msg, 1);
        wu(&mut msg, 1);
        wu(&mut msg, 2);
        wu(&mut msg, 2);
        wu(&mut msg, 0);
        wu(&mut msg, 1);
        ws(&mut msg, "b");
        wu(&mut msg, 3);
        ws(&mut msg, "TEXT");
        wu(&mut msg, 4);
        ws(&mut msg, "Native Label");
        wu(&mut msg, 5);
        wf(&mut msg, 140.0);
        wf(&mut msg, 32.0);
        wu(&mut msg, 6);
        for v in [1.0f32, 0.0, 40.0, 0.0, 1.0, 120.0] {
            wf(&mut msg, v);
        }
        wu(&mut msg, 7);
        wu(&mut msg, 1);
        wu(&mut msg, 1);
        ws(&mut msg, "SOLID");
        wu(&mut msg, 2);
        for c in [1.0f32, 0.4, 0.2, 1.0] {
            wf(&mut msg, c);
        }
        wu(&mut msg, 0);
        wu(&mut msg, 9);
        wu(&mut msg, 1);
        ws(&mut msg, "Figma Native");
        wu(&mut msg, 0);
        wu(&mut msg, 10);
        ws(&mut msg, "Inter");
        ws(&mut msg, "Bold");
        wu(&mut msg, 11);
        wf(&mut msg, 24.0);
        wu(&mut msg, 0);
        // blobs = [cmd_blob]
        wu(&mut msg, 2);
        wu(&mut msg, 1);
        wu(&mut msg, 1);
        wu(&mut msg, cmd_blob.len() as u64);
        msg.extend_from_slice(&cmd_blob);
        wu(&mut msg, 0);
        wu(&mut msg, 0);

        let c0 = deflate_stored(&schema);
        let c1 = deflate_stored(&msg);
        let mut kiwi = Vec::new();
        kiwi.extend_from_slice(b"fig-kiwi");
        kiwi.extend_from_slice(&106u32.to_le_bytes());
        kiwi.extend_from_slice(&(c0.len() as u32).to_le_bytes());
        kiwi.extend_from_slice(&c0);
        kiwi.extend_from_slice(&(c1.len() as u32).to_le_bytes());
        kiwi.extend_from_slice(&c1);

        let meta_b64 = b64_encode(br#"{"fileKey":"MacBoard","pasteID":77,"dataType":"scene"}"#);
        let kiwi_b64 = b64_encode(&kiwi);
        let html = format!(
            "<meta charset=\"utf-8\"><span data-metadata=\"<!--(figmeta){meta_b64}(/figmeta)-->\"></span><span data-buffer=\"<!--(figma){kiwi_b64}(/figma)-->\"></span>"
        );

        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([90, 90, 90, 255]));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        (html, png)
    }

    #[test]
    fn private_pasteboard_figma_html_with_plain_text_and_svg_flavors_paste_editable_vectors() {
        use crate::app::Editor;
        use crate::clipboard::ClipboardCommand;
        use neoflash_core::{ElementKind, Project};
        use objc2_foundation::NSData;

        autoreleasepool(|_| {
            let private = PrivatePasteboard(NSPasteboard::pasteboardWithUniqueName());
            let board = &private.0;
            // SAFETY: Both are immutable pasteboard type constants exported by AppKit.
            let (text_type, html_type) = unsafe { (NSPasteboardTypeString, NSPasteboardTypeHTML) };
            let png_type = NSString::from_str("public.png");
            let svg_type = NSString::from_str("public.svg-image");
            let (figma_html, flat_png) = build_native_test_figma_html_payload();

            // 1. Write Figma HTML + plain-text fallback + flat preview PNG onto `PrivatePasteboard`.
            let gen1 = u64::try_from(board.clearContents()).unwrap();
            assert!(board.setString_forType(&NSString::from_str(&figma_html), html_type));
            assert!(board.setString_forType(&NSString::from_str("Figma Native"), text_type));
            assert!(board.setData_forType(Some(&NSData::with_bytes(&flat_png)), &png_type));

            assert!(read_stage_flavors_from(board, Some(gen1.wrapping_add(1))).is_none());
            let flavors = read_stage_flavors_from(board, Some(gen1)).expect("read matching gen");
            assert_eq!(flavors.generation, Some(gen1));
            assert_eq!(flavors.plain_text.as_deref(), Some("Figma Native"));
            assert_eq!(flavors.html.as_deref(), Some(figma_html.as_str()));
            assert_eq!(flavors.png.as_deref(), Some(flat_png.as_slice()));

            let ctx = eframe::egui::Context::default();
            let mut editor = Editor::from_project(&ctx, Project::blank());
            let before = editor.project.clone();
            editor.paste_objects_with_stage_flavors(
                &ctx,
                "Figma Native",
                true,
                Some(gen1),
                Some(&flavors),
            );
            editor.project.validate().unwrap();
            let root = editor.project.symbol(&editor.project.root).unwrap();
            let els = &root.timeline.layers[0].keyframes[0].elements;
            assert_eq!(
                els.len(),
                2,
                "must paste editable vector + styled text, NOT flat PNG"
            );
            assert!(
                els.iter()
                    .any(|e| matches!(e.kind, ElementKind::Path { .. }))
            );
            assert!(els.iter().any(
                |e| matches!(&e.kind, ElementKind::TextBox { text, format, .. } if text == "Figma Native" && format.bold && (format.size - 24.0).abs() < 1e-6)
            ));
            editor.undo();
            assert_eq!(editor.project, before);

            // 2. HTML + PNG WITHOUT `NSPasteboardTypeString` (how Figma copies pure vectors/frames) via `request_stage_clipboard_with`.
            let _gen2 = u64::try_from(board.clearContents()).unwrap();
            assert!(board.setString_forType(&NSString::from_str(&figma_html), html_type));
            assert!(board.setData_forType(Some(&NSData::with_bytes(&flat_png)), &png_type));
            editor.request_stage_clipboard_with(&ctx, ClipboardCommand::Paste, || {
                read_stage_flavors_from(board, None).ok_or_else(|| "unavailable".into())
            });
            editor.project.validate().unwrap();
            let root2 = editor.project.symbol(&editor.project.root).unwrap();
            let els2 = &root2.timeline.layers[0].keyframes[0].elements;
            assert_eq!(
                els2.len(),
                2,
                "must NOT fall through to flat PNG screenshot"
            );
            assert!(
                els2.iter()
                    .all(|e| !matches!(e.kind, ElementKind::Bitmap { .. }))
            );
            editor.undo();
            assert_eq!(editor.project, before);

            // 3. `public.svg-image` NSData flavor without plain text pastes editable vector Path.
            let _gen3 = u64::try_from(board.clearContents()).unwrap();
            let svg_bytes =
                br##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80"><path fill="#22aa66" d="M10 10H110V70H10Z"/></svg>"##;
            assert!(board.setData_forType(Some(&NSData::with_bytes(svg_bytes)), &svg_type));
            editor.request_stage_clipboard_with(&ctx, ClipboardCommand::PasteInPlace, || {
                read_stage_flavors_from(board, None).ok_or_else(|| "unavailable".into())
            });
            editor.project.validate().unwrap();
            let root3 = editor.project.symbol(&editor.project.root).unwrap();
            let els3 = &root3.timeline.layers[0].keyframes[0].elements;
            assert_eq!(els3.len(), 1);
            assert!(matches!(els3[0].kind, ElementKind::Path { .. }));
            editor.undo();
            assert_eq!(editor.project, before);
        });
    }
}
