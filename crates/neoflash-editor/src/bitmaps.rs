//! Bounded native textures for the pure authoring scene. Pixels remain resources.
use eframe::egui::{self, TextureHandle, TextureId, TextureOptions};
use neoflash_core::{ElementKind, Project};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

const MAX_TEXTURE_BYTES: usize = 64 * 1024 * 1024;
type Key = (String, bool);

struct Texture {
    source: Arc<str>,
    handle: TextureHandle,
    bytes: usize,
}

#[derive(Default)]
pub(crate) struct BitmapTextures {
    textures: HashMap<Key, Texture>,
    used: HashSet<Key>,
    reported: HashMap<String, Arc<str>>,
    warnings: Vec<String>,
}

impl BitmapTextures {
    pub fn begin_frame(&mut self, project: &Project) {
        self.used.clear();
        self.textures.retain(|(id, _), texture| {
            project.resources.iter().any(|resource| {
                resource.id == *id && Arc::ptr_eq(&resource.data_base64, &texture.source)
            })
        });
        self.reported.retain(|id, source| {
            project
                .resources
                .iter()
                .any(|resource| resource.id == *id && Arc::ptr_eq(&resource.data_base64, source))
        });
    }

    pub fn texture(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        kind: &ElementKind,
    ) -> Option<TextureId> {
        let ElementKind::Bitmap {
            resource_id,
            smoothing,
            ..
        } = kind
        else {
            return None;
        };
        let resource = project
            .resources
            .iter()
            .find(|resource| resource.id == *resource_id)?;
        let key = (resource_id.clone(), *smoothing);
        self.used.insert(key.clone());
        if let Some(texture) = self.textures.get(&key) {
            return Some(texture.handle.id());
        }
        let decoded = match resource.decode_image() {
            Ok(image) => image,
            Err(error) => {
                self.warn(
                    resource_id,
                    &resource.data_base64,
                    format!("{}: {error}", resource.path),
                );
                return None;
            }
        };
        let bytes = decoded.rgba.len();
        let mut used_bytes: usize = self.textures.values().map(|texture| texture.bytes).sum();
        if used_bytes + bytes > MAX_TEXTURE_BYTES {
            self.textures.retain(|key, texture| {
                if !self.used.contains(key) && used_bytes + bytes > MAX_TEXTURE_BYTES {
                    used_bytes -= texture.bytes;
                    false
                } else {
                    true
                }
            });
        }
        if used_bytes + bytes > MAX_TEXTURE_BYTES {
            self.warn(
                resource_id,
                &resource.data_base64,
                "Native image preview exceeds its 64 MiB texture budget.".into(),
            );
            return None;
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [decoded.width as usize, decoded.height as usize],
            &decoded.rgba,
        );
        let handle = ctx.load_texture(
            format!("bitmap-{resource_id}-{smoothing}"),
            image,
            if *smoothing {
                TextureOptions::LINEAR
            } else {
                TextureOptions::NEAREST
            },
        );
        let id = handle.id();
        self.textures.insert(
            key,
            Texture {
                source: resource.data_base64.clone(),
                handle,
                bytes,
            },
        );
        Some(id)
    }

    fn warn(&mut self, id: &str, source: &Arc<str>, message: String) {
        if !self.reported.contains_key(id) {
            self.reported.insert(id.to_owned(), source.clone());
            self.warnings.push(message);
        }
    }

    pub fn take_warning(&mut self) -> Option<String> {
        let warning = self.warnings.first().cloned();
        self.warnings.clear();
        warning
    }
}
