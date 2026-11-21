//! Native clipboard ownership metadata for the macOS arm64 rich-text cache.

#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use objc2::{
    ClassType, msg_send,
    rc::{Retained, autoreleasepool},
};
use objc2_app_kit::{
    NSPasteboard, NSPasteboardTypeHTML, NSPasteboardTypePDF, NSPasteboardTypeString,
};
use objc2_foundation::{NSData, NSString};

pub(crate) const PASTEBOARD_TYPE_NEOFLASH_STAGE: &str = "app.neoflash.stage-clipboard";
const MAX_NEOFLASH_STAGE_FLAVOR_BYTES: usize = neoflash_core::MAX_CLIPBOARD_BYTES;
const MAX_STAGE_PASTE_FLAVOR_BYTES: usize = 64 * 1024 * 1024;
const MAX_FIGMA_HTML_FLAVOR_BYTES: usize = neoflash_import::figma::MAX_STAGE_HTML_CLIPBOARD_BYTES;

pub(crate) struct StageClipboardWritePayload<'a> {
    pub neoflash_text: &'a str,
    pub pdf_bytes: Option<&'a [u8]>,
    pub eps_bytes: Option<&'a [u8]>,
    pub svg_text: Option<&'a str>,
}

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

/// Write all 5 Stage clipboard flavors in a single `clearContents()` pasteboard generation:
/// 1. `"app.neoflash.stage-clipboard"` (`NSString` — raw `NEOFLASH-CLIPBOARD\n...`)
/// 2. `"com.adobe.pdf"` / `NSPasteboardTypePDF` (`NSData` — selection-cropped vector PDF 1.4)
/// 3. `"com.adobe.encapsulated-postscript"` (`NSData` — selection-cropped Level 3 EPS)
/// 4. `"public.svg-image"` & `"com.adobe.svg"` (`NSData` — UTF-8 SVG with `<!--(neoflash)...-->`)
/// 5. `NSPasteboardTypeString` (`NSString` — `<svg>` text when available, else raw `NEOFLASH-CLIPBOARD\n...`)
pub(crate) fn write_stage_clipboard_to(
    board: &NSPasteboard,
    payload: &StageClipboardWritePayload<'_>,
) -> WriteResult {
    autoreleasepool(|_| {
        let Ok(generation) = u64::try_from(board.clearContents()) else {
            return WriteResult::Unavailable;
        };

        // 1. Lossless NeoFlash private UTI
        let nf_ty = NSString::from_str(PASTEBOARD_TYPE_NEOFLASH_STAGE);
        let nf_str = NSString::from_str(payload.neoflash_text);
        let mut ok = board.setString_forType(&nf_str, &nf_ty);

        // 2. Adobe Illustrator native vector PDF (`com.adobe.pdf` / `NSPasteboardTypePDF`)
        if let Some(pdf) = payload
            .pdf_bytes
            .filter(|b| !b.is_empty() && b.len() <= MAX_STAGE_PASTE_FLAVOR_BYTES)
        {
            // SAFETY: AppKit provides this immutable pasteboard type constant (`"com.adobe.pdf"`).
            let pdf_ty = unsafe { NSPasteboardTypePDF };
            let pdf_data = NSData::with_bytes(pdf);
            let _ = board.setData_forType(Some(&pdf_data), pdf_ty);
        }

        // 3. Adobe Illustrator native PostScript Level 3 EPS (`com.adobe.encapsulated-postscript`)
        if let Some(eps) = payload
            .eps_bytes
            .filter(|b| !b.is_empty() && b.len() <= MAX_STAGE_PASTE_FLAVOR_BYTES)
        {
            let eps_ty = NSString::from_str("com.adobe.encapsulated-postscript");
            let eps_data = NSData::with_bytes(eps);
            let _ = board.setData_forType(Some(&eps_data), &eps_ty);
        }

        // 4. Clean/tagged SVG (`public.svg-image` & `com.adobe.svg`)
        if let Some(svg) = payload
            .svg_text
            .filter(|s| !s.is_empty() && s.len() <= MAX_STAGE_PASTE_FLAVOR_BYTES)
        {
            let svg_data = NSData::with_bytes(svg.as_bytes());
            for uti in ["public.svg-image", "com.adobe.svg"] {
                let svg_ty = NSString::from_str(uti);
                let _ = board.setData_forType(Some(&svg_data), &svg_ty);
            }
        }

        // 5. Plain-text fallback (`<svg>` text for Figma/SVG tools when available, else `NEOFLASH-CLIPBOARD\n...`)
        let plain_str = payload
            .svg_text
            .filter(|s| !s.is_empty())
            .unwrap_or(payload.neoflash_text);
        // SAFETY: AppKit provides this immutable pasteboard type constant.
        let text_type = unsafe { NSPasteboardTypeString };
        ok &= board.setString_forType(&NSString::from_str(plain_str), text_type);

        if board_generation(board) != Some(generation) {
            WriteResult::Superseded
        } else if ok {
            WriteResult::Written(generation)
        } else {
            WriteResult::Unavailable
        }
    })
}

pub(crate) fn write_stage_clipboard(payload: &StageClipboardWritePayload<'_>) -> WriteResult {
    autoreleasepool(|_| {
        general_pasteboard().map_or(WriteResult::Unavailable, |board| {
            write_stage_clipboard_to(&board, payload)
        })
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

fn is_valid_pdf_clipboard_bytes(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(1024)]
        .windows(5)
        .any(|w| w == b"%PDF-")
}

fn is_valid_eps_clipboard_bytes(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xC5, 0xD0, 0xD3, 0xC6])
        || bytes[..bytes.len().min(1024)]
            .windows(10)
            .any(|w| w == b"%!PS-Adobe")
}

fn read_validated_binary_flavor(
    board: &NSPasteboard,
    utis: &[&str],
    max_bytes: usize,
    validate: fn(&[u8]) -> bool,
) -> (Option<Vec<u8>>, bool) {
    let mut saw_any_flavor = false;
    for &uti in utis {
        let ty = NSString::from_str(uti);
        let Some(data) = board.dataForType(&ty) else {
            continue;
        };
        let len = data.len();
        if len == 0 {
            continue;
        }
        saw_any_flavor = true;
        if len > max_bytes {
            continue;
        }
        let vec = data.to_vec();
        if validate(&vec) {
            return (Some(vec), true);
        }
    }
    (None, saw_any_flavor)
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
        let neoflash = read_string_or_utf8_data(
            board,
            &NSString::from_str(PASTEBOARD_TYPE_NEOFLASH_STAGE),
            MAX_NEOFLASH_STAGE_FLAVOR_BYTES,
        );

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

        let (pdf, saw_pdf_uti) = read_validated_binary_flavor(
            board,
            &["com.adobe.pdf", "Apple PDF pasteboard type", "public.pdf"],
            MAX_STAGE_PASTE_FLAVOR_BYTES,
            is_valid_pdf_clipboard_bytes,
        );

        let (eps, saw_eps_uti) = read_validated_binary_flavor(
            board,
            &[
                "com.adobe.encapsulated-postscript",
                "NeXT Encapsulated PostScript v1.2 pasteboard type",
                "AICB",
                "image/x-eps",
            ],
            MAX_STAGE_PASTE_FLAVOR_BYTES,
            is_valid_eps_clipboard_bytes,
        );

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

        // NEVER read or fall back to flat PNG screenshot when structured NeoFlash, PDF, EPS, or SVG
        // vector flavors exist on the pasteboard (unless Figma `fig-kiwi` HTML needs its raster fallback)!
        let has_vector_flavor = neoflash.is_some()
            || pdf.is_some()
            || eps.is_some()
            || svg.is_some()
            || saw_pdf_uti
            || saw_eps_uti;
        let wants_png = html
            .as_deref()
            .is_some_and(neoflash_import::contains_figma_clipboard_marker)
            || (!has_vector_flavor && plain_text.as_deref().is_none_or(|t| t.trim().is_empty()));
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
            neoflash,
            plain_text,
            html,
            pdf,
            eps,
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

    #[test]
    fn t415_private_pasteboard_five_flavor_single_generation_write_and_four_way_1_to_1_roundtrips()
    {
        use crate::app::Editor;
        use crate::clipboard::{ClipboardCommand, copy_native_stage_clipboard};
        use crate::selection::SelectionTarget;
        use neoflash_core::{
            ClipboardPayload, Element, ElementKind, FillRule, GradientKind, GradientPaint,
            GradientSpread, GradientStop, InstancePlayback, Keyframe, Layer, LayerProperties,
            LayerRole, PathCommand, Project, ResourceUnit, Symbol, SymbolKind, TextFormat,
            Timeline, Transform, new_id,
        };

        autoreleasepool(|_| {
            let ctx = eframe::egui::Context::default();
            let mut src_project = Project::blank(); // 960 x 600

            // 1. Add a 2x2 RGBA PNG resource with semi-transparent pixel (alpha = 128).
            let mut rgba_img = image::RgbaImage::new(2, 2);
            rgba_img.put_pixel(0, 0, image::Rgba([240, 60, 40, 255]));
            rgba_img.put_pixel(1, 0, image::Rgba([40, 200, 100, 128]));
            rgba_img.put_pixel(0, 1, image::Rgba([50, 90, 240, 255]));
            rgba_img.put_pixel(1, 1, image::Rgba([255, 220, 0, 128]));
            let mut png_buf = Vec::new();
            rgba_img
                .write_to(
                    &mut std::io::Cursor::new(&mut png_buf),
                    image::ImageFormat::Png,
                )
                .unwrap();
            let bmp_res =
                ResourceUnit::from_bytes("res-alpha", "bitmaps/alpha.png", "image/png", &png_buf)
                    .unwrap();
            src_project.resources.push(bmp_res.clone());

            // 2. Add a nested MovieClip symbol (`BadgeSym`).
            let badge_sym_id = "sym-badge".to_owned();
            src_project.symbols.push(Symbol {
                id: badge_sym_id.clone(),
                name: "BadgeSym".into(),
                folder: String::new(),
                kind: SymbolKind::MovieClip,
                class_name: None,
                linkage_id: None,
                timeline: Timeline {
                    frame_count: 1,
                    scripts: Vec::new(),
                    guides: Vec::new(),
                    layers: vec![Layer {
                        id: "badge-layer".into(),
                        name: "Shape".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: vec![Element {
                                id: "badge-el".into(),
                                name: "BadgeStar".into(),
                                transform: Transform::default(),
                                kind: ElementKind::Path {
                                    commands: vec![
                                        PathCommand::MoveTo { x: -12.0, y: -10.0 },
                                        PathCommand::LineTo { x: 18.0, y: -10.0 },
                                        PathCommand::CubicTo {
                                            cx1: 22.0,
                                            cy1: 0.0,
                                            cx2: 22.0,
                                            cy2: 12.0,
                                            x: 18.0,
                                            y: 16.0,
                                        },
                                        PathCommand::LineTo { x: -12.0, y: 16.0 },
                                        PathCommand::Close,
                                    ],
                                    fill_rule: FillRule::NonZero,
                                    fill: [255, 160, 30, 255],
                                    stroke: [30, 30, 40, 255],
                                    stroke_width: 1.5,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        }],
                    }],
                },
            });

            // 3. Author Stage layers on `src_project.root`:
            //    - `mask_layer` + `masked_layer` (clipped shape inside `(120, 90)..(200, 170)`)
            //    - `main_layer` with:
            //      * Selected: EvenOdd hole cubic/quadratic Path, 3-stop gradient PaintedPath,
            //        Cyrillic TextBox ("Привет, NeoFlash!"), BadgeSym Instance, RGBA Bitmap
            //        all within `(120, 90)..(420, 270)`!
            //      * Unselected distractor at `(820, 500)` (`80x60`) that MUST NOT be exported!
            let mask_layer_id = "stage-mask".to_owned();
            let masked_layer_id = "stage-masked".to_owned();
            let main_layer_id = "stage-main".to_owned();

            let id_clipped = new_id("el");
            let id_hole = new_id("el");
            let id_grad = new_id("el");
            let id_text = new_id("el");
            let id_inst = new_id("el");
            let id_bmp = new_id("el");
            let id_distractor = new_id("el");

            {
                let root_sym = src_project.symbol_mut(&src_project.root.clone()).unwrap();
                root_sym.timeline.layers = vec![
                    Layer {
                        id: mask_layer_id.clone(),
                        name: "ClipMask".into(),
                        visible: true,
                        locked: true,
                        properties: LayerProperties {
                            role: LayerRole::Mask,
                            ..LayerProperties::default()
                        },
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: vec![Element {
                                id: new_id("mask"),
                                name: "WindowMask".into(),
                                transform: Transform::default(),
                                kind: ElementKind::Path {
                                    commands: vec![
                                        PathCommand::MoveTo { x: 120.0, y: 90.0 },
                                        PathCommand::LineTo { x: 190.0, y: 90.0 },
                                        PathCommand::LineTo { x: 190.0, y: 160.0 },
                                        PathCommand::LineTo { x: 120.0, y: 160.0 },
                                        PathCommand::Close,
                                    ],
                                    fill_rule: FillRule::NonZero,
                                    fill: [0, 0, 0, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        }],
                    },
                    Layer {
                        id: masked_layer_id.clone(),
                        name: "ClippedFill".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties {
                            role: LayerRole::Normal,
                            mask: Some(mask_layer_id),
                            ..LayerProperties::default()
                        },
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: vec![Element {
                                id: id_clipped.clone(),
                                name: "ClippedCard".into(),
                                transform: Transform::default(),
                                kind: ElementKind::Path {
                                    commands: vec![
                                        PathCommand::MoveTo { x: 120.0, y: 90.0 },
                                        PathCommand::LineTo { x: 185.0, y: 90.0 },
                                        PathCommand::LineTo { x: 185.0, y: 155.0 },
                                        PathCommand::LineTo { x: 120.0, y: 155.0 },
                                        PathCommand::Close,
                                    ],
                                    fill_rule: FillRule::NonZero,
                                    fill: [40, 180, 120, 255],
                                    stroke: [0, 0, 0, 0],
                                    stroke_width: 0.0,
                                },
                                ..Element::default()
                            }],
                            ..Keyframe::default()
                        }],
                    },
                    Layer {
                        id: main_layer_id.clone(),
                        name: "MainArtwork".into(),
                        visible: true,
                        locked: false,
                        properties: LayerProperties::default(),
                        keyframes: vec![Keyframe {
                            frame: 0,
                            elements: vec![
                                Element {
                                    id: id_hole.clone(),
                                    name: "DonutHolePath".into(),
                                    transform: Transform::default(),
                                    kind: ElementKind::Path {
                                        commands: vec![
                                            PathCommand::MoveTo { x: 200.0, y: 95.0 },
                                            PathCommand::CubicTo {
                                                cx1: 230.0,
                                                cy1: 90.0,
                                                cx2: 260.0,
                                                cy2: 90.0,
                                                x: 280.0,
                                                y: 95.0,
                                            },
                                            PathCommand::QuadraticTo {
                                                cx: 285.0,
                                                cy: 135.0,
                                                x: 280.0,
                                                y: 175.0,
                                            },
                                            PathCommand::LineTo { x: 200.0, y: 175.0 },
                                            PathCommand::Close,
                                            PathCommand::MoveTo { x: 220.0, y: 115.0 },
                                            PathCommand::LineTo { x: 260.0, y: 115.0 },
                                            PathCommand::LineTo { x: 260.0, y: 155.0 },
                                            PathCommand::LineTo { x: 220.0, y: 155.0 },
                                            PathCommand::Close,
                                        ],
                                        fill_rule: FillRule::EvenOdd,
                                        fill: [220, 50, 80, 255],
                                        stroke: [20, 30, 50, 255],
                                        stroke_width: 2.0,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: id_grad.clone(),
                                    name: "MultiStopGrad".into(),
                                    transform: Transform::default(),
                                    kind: ElementKind::PaintedPath {
                                        commands: vec![
                                            PathCommand::MoveTo { x: 295.0, y: 95.0 },
                                            PathCommand::LineTo { x: 420.0, y: 95.0 },
                                            PathCommand::LineTo { x: 420.0, y: 165.0 },
                                            PathCommand::LineTo { x: 295.0, y: 165.0 },
                                            PathCommand::Close,
                                        ],
                                        fill_rule: FillRule::NonZero,
                                        fill: [0, 0, 0, 0],
                                        stroke: [0, 0, 0, 0],
                                        fill_gradient: Some(GradientPaint {
                                            kind: GradientKind::Linear,
                                            stops: vec![
                                                GradientStop {
                                                    ratio: 0,
                                                    color: [255, 40, 40, 255],
                                                },
                                                GradientStop {
                                                    ratio: 128,
                                                    color: [255, 200, 20, 255],
                                                },
                                                GradientStop {
                                                    ratio: 255,
                                                    color: [30, 110, 255, 255],
                                                },
                                            ],
                                            matrix: [
                                                125.0 / 1638.4,
                                                0.0,
                                                0.0,
                                                70.0 / 1638.4,
                                                357.5,
                                                130.0,
                                            ],
                                            spread: GradientSpread::default(),
                                            focal_point_ratio: 0.0,
                                            linear_rgb: false,
                                        }),
                                        stroke_gradient: None,
                                        stroke_width: 0.0,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: id_text.clone(),
                                    name: "CyrillicTitle".into(),
                                    transform: Transform {
                                        x: 125.0,
                                        y: 195.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::TextBox {
                                        text: "Привет, NeoFlash!".into(),
                                        width: 260.0,
                                        height: 36.0,
                                        format: TextFormat {
                                            font: "Inter".into(),
                                            size: 18.0,
                                            bold: true,
                                            color: 0x191E2D,
                                            ..TextFormat::default()
                                        },
                                        word_wrap: false,
                                        multiline: false,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: id_inst.clone(),
                                    name: "BadgePlacement".into(),
                                    transform: Transform {
                                        x: 345.0,
                                        y: 215.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Instance {
                                        symbol_id: badge_sym_id,
                                        playback: InstancePlayback::Independent,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: id_bmp.clone(),
                                    name: "AlphaTile".into(),
                                    transform: Transform {
                                        x: 385.0,
                                        y: 205.0,
                                        scale_x: 12.0,
                                        scale_y: 12.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Bitmap {
                                        resource_id: bmp_res.id,
                                        width: 2,
                                        height: 2,
                                        smoothing: true,
                                    },
                                    ..Element::default()
                                },
                                Element {
                                    id: id_distractor,
                                    name: "UnselectedDistractor".into(),
                                    transform: Transform {
                                        x: 820.0,
                                        y: 500.0,
                                        ..Transform::default()
                                    },
                                    kind: ElementKind::Rectangle {
                                        width: 80.0,
                                        height: 60.0,
                                        fill: [123, 234, 45, 255],
                                        stroke: [0, 0, 0, 0],
                                        stroke_width: 0.0,
                                    },
                                    ..Element::default()
                                },
                            ],
                            ..Keyframe::default()
                        }],
                    },
                ];
            }
            crate::timeline_commands::upgrade_all_format_versions(&mut src_project);
            src_project.validate().unwrap();

            let mut src_editor = Editor::from_project(&ctx, src_project);
            src_editor.set_selection(vec![
                SelectionTarget {
                    layer: masked_layer_id,
                    element: id_clipped,
                },
                SelectionTarget {
                    layer: main_layer_id.clone(),
                    element: id_hole,
                },
                SelectionTarget {
                    layer: main_layer_id.clone(),
                    element: id_grad,
                },
                SelectionTarget {
                    layer: main_layer_id.clone(),
                    element: id_text,
                },
                SelectionTarget {
                    layer: main_layer_id.clone(),
                    element: id_inst,
                },
                SelectionTarget {
                    layer: main_layer_id,
                    element: id_bmp,
                },
            ]);

            let (bundle, copied_count) = src_editor
                .capture_stage_clipboard_bundle(&ctx)
                .expect("capture Stage clipboard bundle");
            assert_eq!(copied_count, 6);
            assert!(bundle.pdf_bytes.is_some());
            assert!(bundle.eps_bytes.is_some());
            assert!(bundle.svg_text.is_some());

            // Verify `copy_native_stage_clipboard` clears deferred `OutputCommand::CopyText` AND writes
            // all 5 flavors to `PrivatePasteboard` in ONE single `clearContents()` generation!
            let private = PrivatePasteboard(NSPasteboard::pasteboardWithUniqueName());
            let board = &private.0;
            let gen_before = board_generation(board).unwrap();

            ctx.output_mut(|out| {
                out.commands.push(eframe::egui::OutputCommand::CopyText(
                    "STALE_DEFERRED".into(),
                ));
            });
            let written_gen = copy_native_stage_clipboard(&ctx, &bundle.neoflash_text, || {
                write_stage_clipboard_to(
                    board,
                    &StageClipboardWritePayload {
                        neoflash_text: &bundle.neoflash_text,
                        pdf_bytes: bundle.pdf_bytes.as_deref(),
                        eps_bytes: bundle.eps_bytes.as_deref(),
                        svg_text: bundle.svg_text.as_deref(),
                    },
                )
            })
            .expect("single-generation write must succeed");

            let has_deferred_copy = ctx.output(|out| {
                out.commands.iter().any(|cmd| {
                    matches!(
                        cmd,
                        eframe::egui::OutputCommand::CopyText(_)
                            | eframe::egui::OutputCommand::CopyImage(_)
                    )
                })
            });
            assert!(
                !has_deferred_copy,
                "copy_native_stage_clipboard must clear deferred egui CopyText/CopyImage commands"
            );
            assert_eq!(
                written_gen,
                gen_before + 1,
                "write_stage_clipboard_to must call clearContents() exactly once!"
            );

            // Also verify Unavailable fallback restores `ctx.copy_text`.
            assert!(
                copy_native_stage_clipboard(&ctx, "FALLBACK_PAYLOAD", || WriteResult::Unavailable)
                    .is_none()
            );
            let fallback_queued = ctx.output_mut(|out| {
                let found = out.commands.iter().any(|c| {
                    matches!(c, eframe::egui::OutputCommand::CopyText(s) if s == "FALLBACK_PAYLOAD")
                });
                out.commands.clear();
                found
            });
            assert!(
                fallback_queued,
                "Unavailable native write must fall back to ctx.copy_text"
            );

            // Inspect all 5 flavors on `board` at `written_gen`:
            let text_type = unsafe { NSPasteboardTypeString };
            let pdf_type = unsafe { NSPasteboardTypePDF };
            assert_eq!(
                board
                    .stringForType(&NSString::from_str(PASTEBOARD_TYPE_NEOFLASH_STAGE))
                    .unwrap()
                    .to_string(),
                bundle.neoflash_text
            );
            assert!(board.dataForType(pdf_type).is_some());
            assert!(
                board
                    .dataForType(&NSString::from_str("com.adobe.encapsulated-postscript"))
                    .is_some()
            );
            assert!(
                board
                    .dataForType(&NSString::from_str("public.svg-image"))
                    .is_some()
            );
            assert!(
                board
                    .dataForType(&NSString::from_str("com.adobe.svg"))
                    .is_some()
            );
            let plain_on_board = board.stringForType(text_type).unwrap().to_string();
            assert!(plain_on_board.starts_with("<svg"));
            assert!(plain_on_board.contains(ClipboardPayload::SVG_COMMENT_OPEN));
            // Distractor color `#7bea2d` (`[123, 234, 45]`) must NOT appear in exported SVG!
            assert!(!plain_on_board.to_ascii_lowercase().contains("7bea2d"));

            // =========================================================================
            // Round-trip (a): Lossless NeoFlash round-trip (`app.neoflash.stage-clipboard`
            //                 AND `<svg><!--(neoflash)...(/neoflash)-->` plain text!)
            // =========================================================================
            {
                let mut dst = Editor::from_project(&ctx, Project::blank());
                let baseline = dst.project.clone();
                dst.request_stage_clipboard_with(&ctx, ClipboardCommand::PasteInPlace, || {
                    read_stage_flavors_from(board, Some(written_gen))
                        .ok_or_else(|| "missing".into())
                });
                dst.project.validate().unwrap();
                assert_eq!(dst.selection.targets.len(), 6);
                assert_eq!(dst.history.undo_len(), 1);
                dst.undo();
                assert_eq!(dst.project, baseline);

                // Also test pasting ONLY the `<svg><!--(neoflash)...-->` string when private UTI is absent!
                dst.paste_objects(&ctx, &plain_on_board, true);
                dst.project.validate().unwrap();
                assert_eq!(dst.selection.targets.len(), 6);
                assert!(dst.symbol().timeline.layers[0].keyframes[0].elements.iter().any(
                    |e| matches!(&e.kind, ElementKind::TextBox { text, .. } if text == "Привет, NeoFlash!")
                ));
            }

            // Flat screenshot PNG distractor (1x1 magenta) to prove vector flavors ALWAYS win!
            let mut screenshot_png = Vec::new();
            image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 255, 255]))
                .write_to(
                    &mut std::io::Cursor::new(&mut screenshot_png),
                    image::ImageFormat::Png,
                )
                .unwrap();
            let png_type = NSString::from_str("public.png");

            // =========================================================================
            // Round-trip (b): Pure `"com.adobe.pdf"` (`NSPasteboardTypePDF`) round-trip
            //                 (`⌘V` centered AND `⌘⇧V` exact `/ArtBox` Stage coordinates!)
            // =========================================================================
            {
                let pdf_bytes = bundle.pdf_bytes.as_ref().unwrap();
                let pdf_ascii = String::from_utf8_lossy(pdf_bytes);
                assert!(
                    !pdf_ascii.contains("-4096"),
                    "Form XObject /BBox must be tight, never a [-4096 -4096 ...] phantom box"
                );
                let gen_pdf = u64::try_from(board.clearContents()).unwrap();
                assert!(board.setData_forType(Some(&NSData::with_bytes(pdf_bytes)), pdf_type));
                assert!(
                    board.setData_forType(Some(&NSData::with_bytes(&screenshot_png)), &png_type)
                );

                let flavors_pdf = read_stage_flavors_from(board, Some(gen_pdf)).unwrap();
                assert!(flavors_pdf.pdf.is_some());
                assert!(
                    flavors_pdf.png.is_none(),
                    "read_stage_flavors_from must suppress flat PNG when com.adobe.pdf exists"
                );

                // `⌘⇧V` (`Paste in Place`) -> preserves exact `/ArtBox` Stage coordinates!
                let mut dst_in_place = Editor::from_project(&ctx, Project::blank());
                let baseline = dst_in_place.project.clone();
                dst_in_place.request_stage_clipboard_with(
                    &ctx,
                    ClipboardCommand::PasteInPlace,
                    || Ok(flavors_pdf.clone()),
                );
                dst_in_place.project.validate().unwrap();
                assert_eq!(dst_in_place.history.undo_len(), 1);
                let els = &dst_in_place.symbol().timeline.layers[0].keyframes[0].elements;
                // Must include: EvenOdd hole Path, 3-stop PaintedPath, Clipped Group Instance,
                // BadgeSym Form Instance, Cyrillic TextBox, and RGBA SMask Bitmap!
                assert!(els.iter().any(|e| matches!(
                    e.kind,
                    ElementKind::Path {
                        fill_rule: FillRule::EvenOdd,
                        ..
                    }
                )));
                assert!(els.iter().any(|e| matches!(
                    &e.kind,
                    ElementKind::PaintedPath {
                        fill_gradient: Some(g),
                        ..
                    } if g.stops.len() >= 3
                )));
                assert!(els.iter().any(|e| matches!(
                    &e.kind,
                    ElementKind::TextBox { text, .. } if text == "Привет, NeoFlash!"
                )));
                let inst_count = els
                    .iter()
                    .filter(|e| matches!(e.kind, ElementKind::Instance { .. }))
                    .count();
                assert!(
                    inst_count >= 2,
                    "expected both Clipped Group mask instance and /Fm0 BadgeSym Form instance, got {inst_count}"
                );
                let form_inst = els
                    .iter()
                    .find(|e| {
                        e.name.contains("Fm0") && matches!(e.kind, ElementKind::Instance { .. })
                    })
                    .expect("expected /Fm0 BadgeSym Form Instance");
                assert!(
                    (form_inst.transform.x - 332.0).abs() < 4.0
                        && (form_inst.transform.y - 204.0).abs() < 4.0,
                    "Form XObject Instance must sit at tight Stage bounds ~(332, 204), got {:?}",
                    form_inst.transform
                );
                let bmp_el = els
                    .iter()
                    .find(|e| matches!(e.kind, ElementKind::Bitmap { .. }))
                    .expect("expected /SMask RGBA Bitmap from PDF");
                // Original AlphaTile was at Stage `(385, 205)` -> Paste in Place must land right at `(385, 205)`!
                assert!(
                    (bmp_el.transform.x - 385.0).abs() < 2.0
                        && (bmp_el.transform.y - 205.0).abs() < 2.0,
                    "PDF Paste in Place must preserve exact Stage (385, 205) via /ArtBox, got {:?}",
                    bmp_el.transform
                );
                assert_eq!(
                    dst_in_place.project.resources.len(),
                    1,
                    "must import 1 RGBA /SMask image and NOT the 1x1 magenta screenshot_png"
                );
                let decoded_alpha = image::load_from_memory(
                    &dst_in_place.project.resources[0].decoded_bytes().unwrap(),
                )
                .unwrap()
                .to_rgba8();
                assert_eq!(decoded_alpha.dimensions(), (2, 2));
                assert_eq!(decoded_alpha.get_pixel(1, 0).0[3], 128);

                // `⌘V` (`Paste`) -> centers selection around Stage center `(480, 300)`!
                dst_in_place.undo();
                assert_eq!(dst_in_place.project, baseline);
                dst_in_place.request_stage_clipboard_with(&ctx, ClipboardCommand::Paste, || {
                    Ok(flavors_pdf)
                });
                dst_in_place.project.validate().unwrap();
                let bmp_centered = dst_in_place.symbol().timeline.layers[0].keyframes[0]
                    .elements
                    .iter()
                    .find(|e| matches!(e.kind, ElementKind::Bitmap { .. }))
                    .unwrap();
                assert!(
                    (bmp_centered.transform.x - 385.0).abs() > 50.0,
                    "plain Paste must center selection on Stage rather than keeping top-left ArtBox position"
                );
            }

            // =========================================================================
            // Round-trip (c): Pure `"com.adobe.encapsulated-postscript"` EPS round-trip
            // =========================================================================
            {
                let eps_bytes = bundle.eps_bytes.as_ref().unwrap();
                let gen_eps = u64::try_from(board.clearContents()).unwrap();
                assert!(board.setData_forType(
                    Some(&NSData::with_bytes(eps_bytes)),
                    &NSString::from_str("com.adobe.encapsulated-postscript"),
                ));
                assert!(
                    board.setData_forType(Some(&NSData::with_bytes(&screenshot_png)), &png_type)
                );
                let flavors_eps = read_stage_flavors_from(board, Some(gen_eps)).unwrap();
                assert!(flavors_eps.eps.is_some());
                assert!(flavors_eps.png.is_none());

                let mut dst_eps = Editor::from_project(&ctx, Project::blank());
                let baseline = dst_eps.project.clone();
                dst_eps.request_stage_clipboard_with(&ctx, ClipboardCommand::PasteInPlace, || {
                    Ok(flavors_eps)
                });
                dst_eps.project.validate().unwrap();
                assert_eq!(dst_eps.history.undo_len(), 1);
                let els = &dst_eps.symbol().timeline.layers[0].keyframes[0].elements;
                assert!(els.iter().any(|e| matches!(
                    e.kind,
                    ElementKind::Path {
                        fill_rule: FillRule::EvenOdd,
                        ..
                    }
                )));
                assert!(els.iter().any(|e| matches!(
                    e.kind,
                    ElementKind::PaintedPath {
                        fill_gradient: Some(_),
                        ..
                    }
                )));
                let eps_bmp = els
                    .iter()
                    .find(|e| matches!(e.kind, ElementKind::Bitmap { .. }))
                    .expect("EPS colorimage hex stream must round-trip as ElementKind::Bitmap");
                assert!(
                    (eps_bmp.transform.x - 385.0).abs() < 2.0
                        && (eps_bmp.transform.y - 205.0).abs() < 2.0,
                    "EPS Paste in Place must preserve Bitmap at (385, 205), got {:?}",
                    eps_bmp.transform
                );
                dst_eps.undo();
                assert_eq!(dst_eps.project, baseline);
            }

            // =========================================================================
            // Round-trip (d): Comment-stripped `"com.adobe.svg"` / `"public.svg-image"`
            // =========================================================================
            {
                let raw_svg = bundle.svg_text.as_ref().unwrap();
                let start_c = raw_svg.find(ClipboardPayload::SVG_COMMENT_OPEN).unwrap();
                let end_c = raw_svg.find(ClipboardPayload::SVG_COMMENT_CLOSE).unwrap()
                    + ClipboardPayload::SVG_COMMENT_CLOSE.len();
                let mut clean_svg = String::with_capacity(raw_svg.len());
                clean_svg.push_str(&raw_svg[..start_c]);
                clean_svg.push_str(&raw_svg[end_c..]);
                assert!(ClipboardPayload::from_text(&clean_svg).unwrap().is_none());

                let gen_svg = u64::try_from(board.clearContents()).unwrap();
                assert!(board.setData_forType(
                    Some(&NSData::with_bytes(clean_svg.as_bytes())),
                    &NSString::from_str("com.adobe.svg"),
                ));
                let flavors_svg = read_stage_flavors_from(board, Some(gen_svg)).unwrap();
                assert!(flavors_svg.svg.is_some());

                let mut dst_svg = Editor::from_project(&ctx, Project::blank());
                let baseline = dst_svg.project.clone();
                dst_svg.request_stage_clipboard_with(&ctx, ClipboardCommand::Paste, || {
                    Ok(flavors_svg)
                });
                dst_svg.project.validate().unwrap();
                assert_eq!(dst_svg.history.undo_len(), 1);
                assert!(
                    !dst_svg.symbol().timeline.layers[0].keyframes[0]
                        .elements
                        .is_empty()
                );
                dst_svg.undo();
                assert_eq!(dst_svg.project, baseline);
            }
        });
    }
}
