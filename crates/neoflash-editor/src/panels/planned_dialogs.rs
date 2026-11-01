//! Planned dialog previews: Publish settings (`publish-misc-dialogs.png`), Export image / video / audio
//! (`dialogs-export.png`), Import video (`dialog-import-video.png`), Sound properties (`dialogs-sound-stroke.png`).
//! Export formats switch preview pages; every setting stays disabled. Preferences is a real dialog
//! (`preferences.rs`).
use super::planned::{
    body_text, caption, check, footer, num, segmented, select, slider, stepper, switch, text,
};
use eframe::egui::{self, Ui};
use neoflash_ui::components as c;
use neoflash_ui::components::timeline as tl;
use neoflash_ui::icons::{self, i16};
use neoflash_ui::tokens::{TextStyle, radius, space};
use neoflash_ui::typography::rich;

/// Overline caption of a dialog group ("OUTPUT", "RANGE").
pub(super) fn group(ui: &mut Ui, s: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.add_space(space::S1);
    ui.label(rich(s, TextStyle::Overline).color(nf.text_tertiary));
}

/// Stage-preview placeholder (export / import dialogs): `bg/canvas` pasteboard with a `bg/stage` page.
pub(super) fn preview_box(ui: &mut Ui, width: f32, height: f32, label: &str) {
    let nf = neoflash_ui::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let r = egui::CornerRadius::same(radius::LG as u8);
    ui.painter().rect_filled(rect, r, nf.bg_canvas);
    let stage = egui::Rect::from_center_size(rect.center(), rect.size() * 0.8);
    ui.painter().rect_filled(stage, 0.0, nf.bg_stage);
    ui.painter().text(
        stage.center(),
        egui::Align2::CENTER_CENTER,
        label,
        neoflash_ui::typography::font(TextStyle::Small),
        nf.text_tertiary,
    );
    ui.painter().rect_stroke(
        rect,
        r,
        egui::Stroke::new(1.0_f32, nf.border_subtle),
        egui::StrokeKind::Inside,
    );
}

fn disabled(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    ui.scope(|ui| {
        ui.disable();
        content(ui);
    });
}

// ── Document and publish ──

/// Dialog / Publish settings.
pub(super) fn publish_settings(ui: &mut Ui) {
    let nf = neoflash_ui::palette(ui.ctx());
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(260.0, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(260.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                select(ui, "Profile: Default", 260.0);
                for (on, icon, label) in [
                    (true, i16::GLOBE, "HTML5 · Rust / WebAssembly"),
                    (false, i16::DESKTOP, "Desktop app (macOS / Windows)"),
                    (false, i16::VIDEO, "Video (MP4)"),
                    (true, i16::GRID, "Image sequence / sprite sheet"),
                    (false, i16::CODE, "SVG animation"),
                ] {
                    ui.horizontal(|ui| {
                        ui.set_min_height(32.0);
                        check(ui, on, "");
                        icons::show(ui, icon, nf.text_secondary);
                        body_text(ui, label);
                    });
                }
            },
        );
        ui.add_space(space::S5);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = space::S2;
            ui.label(rich("HTML5 · Rust / WebAssembly", TextStyle::Title).color(nf.text_primary));
            group(ui, "Output");
            ui.horizontal(|ui| {
                text(ui, "publish/ideas.html", "File", 380.0);
                let _ = ui.add(c::Button::new("…"));
            });
            ui.horizontal(|ui| {
                check(ui, true, "Include HTML wrapper");
                select(ui, "Template: Responsive", 200.0);
            });
            group(ui, "Runtime");
            ui.horizontal(|ui| {
                select(ui, "WASM: optimised (−O3)", 220.0);
                select(ui, "Scale: Fit, keep ratio", 200.0);
            });
            ui.horizontal(|ui| {
                check(ui, true, "Loop");
                check(ui, true, "Autoplay");
                check(ui, true, "Preloader");
                check(ui, false, "Source maps");
            });
            group(ui, "Assets");
            ui.horizontal(|ui| {
                caption(ui, "JPEG quality");
                slider(ui, 80.0, "JPEG quality", 160.0);
                caption(ui, "80");
                select(ui, "Audio: MP3 128 kbps", 180.0);
            });
            ui.horizontal(|ui| {
                check(ui, true, "Sprite sheets for bitmaps");
                check(ui, true, "Embed fonts (subset)");
            });
            group(ui, "Scripts");
            ui.horizontal(|ui| {
                switch(ui, true, "Strict mode");
                switch(ui, false, "Include trace()");
                switch(ui, true, "Minify");
            });
        });
    });
    footer(
        ui,
        "Estimated size ~ 1.8 MB · 6 files",
        None,
        &["Cancel", "OK", "Publish"],
    );
}

// ── Export / import ──

const IMAGE_FORMATS: [&str; 5] = ["PNG", "SVG", "PSD", "AI", "PDF"];

/// Dialog / Export image. The format segments switch preview pages.
pub(super) fn export_image(ui: &mut Ui, format: &mut usize) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(300.0);
            preview_box(ui, 300.0, 188.0, "Stage preview");
            caption(ui, "Frame 30 of 120 · 960 × 600");
        });
        ui.add_space(space::S5);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = space::S2;
            let opts: Vec<(usize, &str)> = IMAGE_FORMATS.iter().copied().enumerate().collect();
            c::Segmented::show(ui, format, &opts, false);
            disabled(ui, |ui| {
                group(ui, "Range");
                ui.horizontal(|ui| {
                    select(ui, "Current frame", 200.0);
                    num(ui, 30.0, "Frames", "Frames", "", 0, 160.0);
                });
                group(ui, "Size");
                ui.horizontal(|ui| {
                    num(ui, 960.0, "Width", "W", "", 0, 120.0);
                    num(ui, 600.0, "Height", "H", "", 0, 120.0);
                    let _ = ui
                        .add(c::IconButton::new(i16::LINK, "Constrain proportions").selected(true));
                    select(ui, "1×", 80.0);
                });
                group(ui, "Background");
                ui.horizontal(|ui| {
                    ui.add(c::Radio::new(*format != 1, "Stage colour"));
                    ui.add(c::Radio::new(*format == 1, "Transparent"));
                });
                match *format {
                    1 => {
                        group(ui, "SVG options");
                        ui.horizontal(|ui| {
                            select(ui, "Images: Embed", 180.0);
                            select(ui, "Text: Convert to outlines", 200.0);
                        });
                        switch(ui, true, "Minify output");
                        switch(ui, false, "Use CSS classes for styles");
                        check(ui, false, "Include hidden & guide layers");
                        ui.horizontal(|ui| {
                            num(ui, 2.0, "Precision", "Precision", "", 0, 120.0);
                            text(ui, "nf-", "ID prefix", 180.0);
                        });
                    }
                    2 => {
                        group(ui, "PSD options");
                        ui.add(c::Radio::new(
                            true,
                            "Keep timeline layers as Photoshop layers",
                        ));
                        ui.add(c::Radio::new(false, "Flatten to one layer"));
                        check(ui, true, "Folders → layer groups");
                        check(ui, true, "Symbols → smart objects");
                        check(ui, true, "Keep text editable");
                        ui.horizontal(|ui| {
                            select(ui, "Colour: sRGB 8-bit", 200.0);
                            select(ui, "Resolution: 144 ppi", 180.0);
                        });
                    }
                    3 => {
                        group(ui, "AI options");
                        select(ui, "Version: Illustrator CS6+", 240.0);
                        ui.add(c::Radio::new(true, "Each layer → AI layer"));
                        ui.add(c::Radio::new(false, "Each keyframe → artboard"));
                        check(ui, true, "Strokes as strokes (not outlines)");
                        check(ui, false, "Convert text to outlines");
                    }
                    _ => caption(ui, "No format-specific options in the reference design."),
                }
            });
        });
    });
    let (size, button) = match *format {
        1 => ("~ 18 kB · 42 paths", "Export SVG"),
        2 => ("~ 4.2 MB · 6 layers", "Export PSD"),
        3 => ("~ 260 kB · 6 layers", "Export AI"),
        4 => ("", "Export PDF"),
        _ => ("", "Export PNG"),
    };
    disabled(ui, |ui| footer(ui, size, None, &["Cancel", button]));
}

/// Dialog / Export video.
pub(super) fn export_video(ui: &mut Ui) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(300.0);
            preview_box(ui, 300.0, 188.0, "Stage preview");
            c::progress(ui, "Exporting frames 72 / 120", Some(0.6), 300.0);
            caption(ui, "Preview while rendering · ⏸ to pause");
        });
        ui.add_space(space::S5);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = space::S2;
            segmented(ui, &["MP4", "WebM", "MOV", "GIF", "PNG seq."], 0);
            group(ui, "Range");
            ui.horizontal(|ui| {
                select(ui, "Whole movie (all scenes)", 220.0);
                num(ui, 1.0, "From frame", "From", "", 0, 90.0);
                num(ui, 120.0, "To frame", "To", "", 0, 90.0);
            });
            group(ui, "Video");
            ui.horizontal(|ui| {
                num(ui, 1920.0, "Width", "W", "", 0, 110.0);
                num(ui, 1200.0, "Height", "H", "", 0, 110.0);
                let _ =
                    ui.add(c::IconButton::new(i16::LINK, "Constrain proportions").selected(true));
                select(ui, "2×", 76.0);
            });
            ui.horizontal(|ui| {
                select(ui, "24 fps (document)", 160.0);
                select(ui, "Codec: H.264", 160.0);
            });
            ui.horizontal(|ui| {
                caption(ui, "Quality");
                slider(ui, 75.0, "Quality", 180.0);
                caption(ui, "High · ~8 Mbps");
            });
            group(ui, "Content");
            check(
                ui,
                true,
                "Render nested movie clip animation and scripts (runtime capture)",
            );
            ui.horizontal(|ui| {
                ui.add(c::Radio::new(true, "Stop at last frame"));
                ui.add(c::Radio::new(false, "Stop after"));
                num(ui, 10.0, "Stop after seconds", "", "s", 0, 72.0);
            });
            ui.horizontal(|ui| {
                switch(ui, true, "Include audio");
                select(ui, "AAC 192 kbps", 150.0);
                check(ui, false, "Transparent bg (MOV/WebM)");
            });
        });
    });
    footer(
        ui,
        "~ 12.1 MB · 00:05 · 1920×1200",
        None,
        &["Cancel", "Export"],
    );
}

/// Dialog / Export audio.
pub(super) fn export_audio(ui: &mut Ui) {
    let w = ui.available_width();
    segmented(ui, &["WAV", "MP3", "OGG", "AAC"], 1);
    let (span, _) = ui.allocate_exact_size(egui::vec2(w, 40.0), egui::Sense::hover());
    tl::paint_span(ui.painter(), ui.ctx(), span, tl::SpanType::Sound, true);
    group(ui, "Source");
    ui.horizontal(|ui| {
        select(ui, "Timeline mix (all sound layers)", 300.0);
        select(ui, "Range: whole movie", 280.0);
    });
    group(ui, "Format");
    ui.horizontal(|ui| {
        select(ui, "44.1 kHz", 140.0);
        select(ui, "16-bit", 120.0);
        select(ui, "Stereo", 120.0);
        select(ui, "192 kbps CBR", 180.0);
    });
    group(ui, "Processing");
    ui.horizontal(|ui| {
        switch(ui, true, "Normalize to −1 dB");
        stepper(ui, 1.0, "Fade out", "Fade out", "s", 1, 160.0);
        check(ui, false, "Trim silence");
    });
    footer(
        ui,
        "music.mp3 + sfx · 00:32.4 · ~ 760 kB",
        None,
        &["Cancel", "Export"],
    );
}

/// Dialog / Import video.
pub(super) fn import_video(ui: &mut Ui) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(300.0);
            preview_box(ui, 300.0, 169.0, "Video preview");
            let (span, _) = ui.allocate_exact_size(egui::vec2(300.0, 27.0), egui::Sense::hover());
            tl::paint_span(ui.painter(), ui.ctx(), span, tl::SpanType::Video, true);
            ui.horizontal(|ui| {
                text(ui, "00:00.00", "In", 146.0);
                text(ui, "00:24.00", "Out", 146.0);
            });
            caption(ui, "Drag the in / out handles on the strip to trim.");
        });
        ui.add_space(space::S5);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = space::S2;
            ui.horizontal(|ui| {
                text(ui, "~/Movies/intro.mp4", "File", 260.0);
                let _ = ui.add(c::Button::new("Browse…"));
            });
            group(ui, "Deployment");
            for (on, label, hint) in [
                (
                    true,
                    "Embed in timeline",
                    "Frames sync with the timeline; best for short clips",
                ),
                (
                    false,
                    "Load external video",
                    "Streams with a player component; keeps .nfl small",
                ),
                (
                    false,
                    "Progressive download",
                    "Plays while downloading from a URL",
                ),
            ] {
                ui.add(c::Radio::new(on, label));
                caption(ui, hint);
            }
            group(ui, "Embedding");
            ui.horizontal(|ui| {
                select(ui, "Symbol: Movie clip", 172.0);
                select(ui, "Codec: H.264", 172.0);
            });
            ui.horizontal(|ui| {
                check(ui, true, "Place on stage");
                check(ui, true, "Expand timeline if needed");
            });
            ui.horizontal(|ui| {
                check(ui, true, "Include audio");
                check(ui, true, "Match document frame rate");
            });
        });
    });
    footer(
        ui,
        "1280×720 · 30 fps → 24 fps · 24.0 s · ~6.1 MB",
        None,
        &["Cancel", "Import"],
    );
}
