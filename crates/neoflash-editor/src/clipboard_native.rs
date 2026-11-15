//! Native clipboard ownership metadata for the macOS arm64 rich-text cache.

#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use objc2::{
    ClassType, msg_send,
    rc::{Retained, autoreleasepool},
};
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;

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

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_app_kit::NSPasteboardTypeHTML;

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
}
