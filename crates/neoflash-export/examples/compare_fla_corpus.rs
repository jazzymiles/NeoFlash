//! Inspect editable imports and render the same authored frames captured by Animate.
use neoflash_export::{ImageExportOptions, export_project_image};
use neoflash_import::{SwfBytecodePlayer, import_project};
use serde_json::{Value, json};
use std::{env, error::Error, path::Path};

fn authored(source: &Path, destination: &Path) -> Result<Value, Box<dyn Error>> {
    let project = import_project(source)?;
    project.validate()?;
    let mut scenes = Vec::new();
    let mut render_errors = Vec::new();
    let ids = if project.scenes.is_empty() {
        std::slice::from_ref(&project.root)
    } else {
        &project.scenes
    };
    for (scene, id) in ids.iter().enumerate() {
        let timeline = &project.symbol(id).ok_or("missing scene")?.timeline;
        let keys = timeline.layers.iter().flat_map(|layer| &layer.keyframes);
        let layout = [
            timeline.frame_count as usize,
            timeline.layers.len(),
            keys.clone().count(),
            keys.filter(|key| !key.label.is_empty()).count(),
            timeline
                .scripts
                .iter()
                .filter(|script| !script.script.source.trim().is_empty())
                .count(),
        ];
        scenes.push(layout);
        let last = timeline
            .frame_count
            .checked_sub(1)
            .ok_or("empty timeline")?;
        let frames = [0, last / 2, last];
        for (i, &frame) in frames.iter().enumerate() {
            if i > 0 && frames[i - 1] == frame {
                continue;
            }
            let mut options = ImageExportOptions::for_project(&project, frame);
            options.symbol_id = Some(id.clone());
            options.runtime_capture = false;
            options.transparent_background = true;
            let path = destination.join(format!("native-s{scene}-f{frame}.png"));
            if let Err(error) = export_project_image(&project, &path, &options) {
                render_errors
                    .push(json!({"scene":scene, "frame":frame, "reason":error.to_string()}));
            }
        }
    }
    let bg = project.background;
    Ok(json!({
        "stage": [project.width, project.height, project.fps],
        "background": format!("#{:02x}{:02x}{:02x}",bg[0],bg[1],bg[2]),
        "scenes": scenes, "symbols": project.symbols.len(), "render_errors": render_errors,
    }))
}

fn compare_pixels(native: &Path, adobe: &Path) -> Result<Value, Box<dyn Error>> {
    let native = image::open(native)?.to_rgba8();
    let adobe = image::open(adobe)?.to_rgba8();
    if native.dimensions() != adobe.dimensions() {
        return Ok(
            json!({"status":"different_dimensions", "native": native.dimensions(), "adobe": adobe.dimensions()}),
        );
    }
    let mut different = 0_u64;
    let mut visible_differences = 0_u64;
    for (a, b) in native.pixels().zip(adobe.pixels()) {
        if a != b {
            different += 1;
            if a[3] != 0 || b[3] != 0 {
                visible_differences += 1;
            }
        }
    }
    Ok(
        json!({"pixels": u64::from(native.width()) * u64::from(native.height()),
              "different_rgba_pixels": different, "different_visible_pixels": visible_differences}),
    )
}

fn static_swf(source: &Path, reference: &Path, output: &Path) -> Result<Value, Box<dyn Error>> {
    let project = import_project(source)?;
    project.validate()?;
    let mut options = ImageExportOptions::for_project(&project, 0);
    options.runtime_capture = false;
    options.transparent_background = false;
    export_project_image(&project, output, &options)?;
    compare_pixels(output, reference)
}

fn playback(source: &Path, ticks: usize) -> Result<Value, Box<dyn Error>> {
    let mut player = SwfBytecodePlayer::from_path(source)?;
    let mut traces = Vec::new();
    let mut diagnostics = Vec::new();
    for _ in 0..ticks {
        let frame = player.step_frame();
        traces.extend(frame.traces);
        diagnostics.extend(frame.unsupported_diagnostics);
    }
    Ok(json!({ "traces": traces, "diagnostics": diagnostics }))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    let result = match args.as_slice() {
        [mode, file, dir] if mode == "authored" => authored(Path::new(file), Path::new(dir))?,
        [mode, native, adobe] if mode == "pixels" => {
            compare_pixels(Path::new(native), Path::new(adobe))?
        }
        [mode, file, reference, output] if mode == "static-swf" => {
            static_swf(Path::new(file), Path::new(reference), Path::new(output))?
        }
        [mode, file, ticks] if mode == "playback" => playback(Path::new(file), ticks.parse()?)?,
        _ => return Err(
            "Expected authored INPUT OUTPUT_DIR or pixels NATIVE ADOBE or playback SWF NUM_FRAMES or static-swf SWF REFERENCE OUTPUT"
                .into(),
        ),
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
