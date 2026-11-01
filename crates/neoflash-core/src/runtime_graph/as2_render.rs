//! AS2-only raster isolation, blend selection and opaque backgrounds.
//! Rust validates state and publishes compositor commands; the browser supplies
//! Canvas's standardized blend operation, not an additional script engine.
use super::*;
use serde_json::{Value, json};
#[derive(Clone, Debug, Default)]
pub(super) struct As2Render {
    pub blend_mode: String,
    pub cache_as_bitmap: bool,
    pub opaque_background: Option<u32>,
}
impl As2Render {
    pub(super) fn blend(&self) -> &str {
        if self.blend_mode.is_empty() {
            "normal"
        } else {
            &self.blend_mode
        }
    }
    pub(super) fn isolated(&self) -> bool {
        self.blend() != "normal" || self.cache_as_bitmap || self.opaque_background.is_some()
    }
    pub(super) fn operation(&self) -> &str {
        match self.blend() {
            "normal" | "layer" => "source-over",
            "add" => "lighter",
            "hardlight" => "hard-light",
            value => value,
        }
    }
}
impl Player {
    pub fn as2_render_command(
        &mut self,
        project: &Project,
        target: &ObjectRef,
        request: Value,
    ) -> Result<Value, DisplayError> {
        if !self.graph.alive(target) {
            return Err(error("AS2 render target is stale or foreign"));
        }
        let as3 = request["as3"].as_bool().unwrap_or(false);
        let scene = self.runtime_snapshot(project);
        let node = scene
            .objects
            .iter()
            .find(|node| &node.object_ref == target)
            .ok_or_else(|| error("Render target is absent"))?;
        if !as3 && !matches!(node.display.owner.as_str(), "movie_clip" | "button") {
            return Err(error(
                "AS2 blend/cache properties require MovieClip or Button",
            ));
        }
        let mut state = self
            .graph
            .as2_render_states
            .get(target)
            .cloned()
            .unwrap_or_default();
        if request["op"] == "set" || request["op"] == "prepare" {
            let value = &request["value"];
            match request["name"].as_str().unwrap_or("") {
                "blendMode" => {
                    let name = if let Some(name) = value.as_str() {
                        name
                    } else if as3 {
                        return Err(error("DisplayObject.blendMode requires a BlendMode String"));
                    } else {
                        match value.as_u64() {
                            Some(0 | 1) => "normal",
                            Some(2) => "layer",
                            Some(3) => "multiply",
                            Some(4) => "screen",
                            Some(5) => "lighten",
                            Some(6) => "darken",
                            Some(7) => "difference",
                            Some(8) => "add",
                            Some(13) => "overlay",
                            Some(14) => "hardlight",
                            _ => {
                                return Err(error(
                                    "Unsupported AS2 numeric blendMode; supported modes are normal/layer/multiply/screen/lighten/darken/difference/add/overlay/hardlight",
                                ));
                            }
                        }
                    };
                    if !matches!(
                        name,
                        "normal"
                            | "layer"
                            | "multiply"
                            | "screen"
                            | "lighten"
                            | "darken"
                            | "difference"
                            | "add"
                            | "overlay"
                            | "hardlight"
                    ) {
                        return Err(error(
                            "Unsupported blendMode; shader/subtract/invert/alpha/erase need a separate compositing profile",
                        ));
                    }
                    state.blend_mode = name.into();
                }
                "cacheAsBitmap" => {
                    state.cache_as_bitmap = value
                        .as_bool()
                        .ok_or_else(|| error("cacheAsBitmap requires Boolean"))?
                }
                "opaqueBackground" if as3 || node.display.owner == "movie_clip" => {
                    state.opaque_background = if value.is_null() {
                        None
                    } else {
                        Some(
                            value
                                .as_u64()
                                .filter(|value| *value <= 0xffffff)
                                .ok_or_else(|| {
                                    error("opaqueBackground requires RGB integer or null")
                                })? as u32,
                        )
                    }
                }
                _ => return Err(error("Unknown AS2 render property")),
            }
            let existing: BTreeSet<_> = self
                .graph
                .filters
                .keys()
                .chain(self.graph.as2_render_states.keys())
                .filter(|id| *id != target)
                .collect();
            if existing.len() + usize::from(state.isolated() || !node.filters.is_empty()) > 128 {
                return Err(error("AS2 raster/filter groups exceed128 objects"));
            }
            if request["op"] == "set" {
                if state.isolated() {
                    self.graph
                        .as2_render_states
                        .insert(target.clone(), state.clone());
                } else {
                    self.graph.as2_render_states.remove(target);
                }
            }
        } else if request["op"] != "get" {
            return Err(error("Unknown AS2 render operation"));
        }
        Ok(
            json!({"blendMode":state.blend(),"cacheAsBitmap":state.cache_as_bitmap||!node.filters.is_empty(),"opaqueBackground":state.opaque_background,"isolated":state.isolated()||!node.filters.is_empty()}),
        )
    }
}
