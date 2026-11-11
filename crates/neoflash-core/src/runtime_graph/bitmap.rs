use super::*;
#[derive(Clone, Default)]
pub(super) struct BitmapState {
    pub pixel_ref: Option<PixelRef>,
    pub smoothing: bool,
    pub pixel_snapping: PixelSnapping,
}
impl RuntimeGraph {
    pub(crate) fn prepare_bitmaps(&mut self, project: &Project) {
        let referenced: BTreeSet<_> = project
            .symbols
            .iter()
            .flat_map(|s| &s.timeline.layers)
            .flat_map(|l| &l.keyframes)
            .flat_map(|k| &k.elements)
            .filter_map(|e| {
                if let ElementKind::Bitmap { resource_id, .. } = &e.kind {
                    Some(resource_id.as_str())
                } else {
                    None
                }
            })
            .collect();
        if referenced.len() > 256 {
            self.bitmap_error = Some(BitmapError(
                "authored images exceed the 256 buffer budget".into(),
            ));
            return;
        }
        let mut total = 0usize;
        for id in referenced {
            let Some(resource) = project.resources.iter().find(|resource| resource.id == id) else {
                self.bitmap_error = Some(BitmapError("missing image resource".into()));
                return;
            };
            if resource.data_base64.is_empty() && resource.is_image() {
                if let Some(hydrated) = self.hydrated_bitmaps.get(id).cloned() {
                    total += hydrated.width as usize * hydrated.height as usize * 4;
                    if total > MAX_BITMAP_BYTES {
                        self.bitmap_error =
                            Some(BitmapError("authored image buffers exceed 128 MiB".into()));
                        return;
                    }
                    match self.pixels.import(&hydrated) {
                        Ok(reference) => {
                            self.resource_pixels.insert(id.into(), reference);
                        }
                        Err(error) => {
                            self.bitmap_error = Some(error);
                            return;
                        }
                    }
                }
                continue;
            }
            let result = Ok(resource)
                .and_then(|resource| {
                    let (w, h) = resource
                        .image_dimensions()
                        .map_err(|e| BitmapError(e.to_string()))?;
                    total += w as usize * h as usize * 4;
                    if total > MAX_BITMAP_BYTES {
                        return Err(BitmapError("authored image buffers exceed 128 MiB".into()));
                    }
                    Ok(resource)
                })
                .and_then(|resource| {
                    resource
                        .decode_image()
                        .map_err(|e| BitmapError(e.to_string()))
                })
                .and_then(|image| self.pixels.import(&image));
            match result {
                Ok(reference) => {
                    self.resource_pixels.insert(id.into(), reference);
                }
                Err(error) => {
                    self.bitmap_error = Some(error);
                    return;
                }
            }
        }
    }

    pub(crate) fn hydrate_sprite_sheet(
        &mut self,
        sheet_png_bytes: &[u8],
        manifest_json: &str,
    ) -> Result<usize, BitmapError> {
        if manifest_json.len() > 4 * 1024 * 1024 {
            return Err(BitmapError("sprite sheet manifest exceeds 4 MiB".into()));
        }
        let sheet = crate::resource_image::decode_encoded_image(sheet_png_bytes)
            .map_err(|e| BitmapError(format!("sprite sheet decode: {e}")))?;
        let parsed: serde_json::Value = serde_json::from_str(manifest_json)
            .map_err(|e| BitmapError(format!("sprite sheet manifest JSON: {e}")))?;
        let Some(bitmaps) = parsed.get("bitmaps").and_then(|v| v.as_array()) else {
            return Ok(0);
        };
        if bitmaps.len() > 256 {
            return Err(BitmapError(
                "sprite sheet bitmaps exceed the 256 buffer budget".into(),
            ));
        }
        let mut hydrated_count = 0usize;
        for entry in bitmaps {
            let Some(id) = entry.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(frame) = entry.get("frame") else {
                continue;
            };
            let x = frame.get("x").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let y = frame.get("y").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let w = frame.get("w").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let h = frame.get("h").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if w == 0
                || h == 0
                || x.saturating_add(w) > sheet.width
                || y.saturating_add(h) > sheet.height
            {
                return Err(BitmapError(format!(
                    "sprite sheet frame for '{id}' ({x},{y},{w}x{h}) exceeds sheet {}x{}",
                    sheet.width, sheet.height
                )));
            }
            let row_bytes = (w as usize) * 4;
            let sheet_stride = (sheet.width as usize) * 4;
            let mut cropped = vec![0u8; row_bytes * (h as usize)];
            for row in 0..(h as usize) {
                let src_start = (y as usize + row) * sheet_stride + (x as usize) * 4;
                let dst_start = row * row_bytes;
                cropped[dst_start..dst_start + row_bytes]
                    .copy_from_slice(&sheet.rgba[src_start..src_start + row_bytes]);
            }
            let decoded = crate::resource_image::DecodedImage {
                width: w,
                height: h,
                rgba: cropped.into(),
            };
            self.hydrated_bitmaps.insert(id.to_owned(), decoded.clone());
            if !self.resource_pixels.contains_key(id) {
                self.pixels.reserve_import(w, h)?;
                let pixel_ref = self.pixels.import(&decoded)?;
                self.resource_pixels.insert(id.to_owned(), pixel_ref);
            }
            hydrated_count += 1;
        }
        Ok(hydrated_count)
    }
    pub(super) fn bitmap_state(
        &self,
        reference: &ObjectRef,
        kind: Option<&ElementKind>,
    ) -> BitmapState {
        if let Some(state) = self.bitmap_states.get(reference) {
            return state.clone();
        }
        if let Some(ElementKind::Bitmap {
            resource_id,
            smoothing,
            ..
        }) = kind
        {
            return BitmapState {
                pixel_ref: self.resource_pixels.get(resource_id).cloned(),
                smoothing: *smoothing,
                pixel_snapping: PixelSnapping::Auto,
            };
        }
        BitmapState::default()
    }
}
fn symbol_first_bitmap_resource<'a>(
    project: &'a Project,
    symbol: &crate::Symbol,
) -> Option<&'a crate::ResourceUnit> {
    for layer in &symbol.timeline.layers {
        for keyframe in &layer.keyframes {
            for element in &keyframe.elements {
                if let crate::ElementKind::Bitmap { resource_id, .. } = &element.kind
                    && let Some(resource) = project
                        .resources
                        .iter()
                        .find(|resource| resource.id == *resource_id)
                {
                    return Some(resource);
                }
            }
        }
    }
    None
}

fn is_image_resource(resource: &crate::ResourceUnit) -> bool {
    resource
        .media_type
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("image/"))
}

fn strip_extension(name: &str) -> Option<&str> {
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty() && !ext.contains('/')).then_some(stem)
}

fn resolve_as2_bitmap_resource<'a>(
    project: &'a Project,
    id: &str,
) -> Option<&'a crate::ResourceUnit> {
    if let Some(resource) = project.resources.iter().find(|resource| resource.id == id) {
        return Some(resource);
    }
    if let Some(resource) = project
        .symbols
        .iter()
        .filter(|symbol| {
            symbol.linkage_id.as_deref() == Some(id) || symbol.class_name.as_deref() == Some(id)
        })
        .find_map(|symbol| symbol_first_bitmap_resource(project, symbol))
    {
        return Some(resource);
    }
    if let Some(resource) = project
        .resources
        .iter()
        .filter(|resource| is_image_resource(resource))
        .find(|resource| {
            let normalized = resource
                .path
                .trim_start_matches("./")
                .trim_start_matches('/');
            let basename = normalized.rsplit('/').next().unwrap_or(normalized);
            normalized == id
                || basename == id
                || strip_extension(normalized) == Some(id)
                || strip_extension(basename) == Some(id)
        })
    {
        return Some(resource);
    }
    project
        .symbols
        .iter()
        .filter(|symbol| symbol.name == id)
        .find_map(|symbol| symbol_first_bitmap_resource(project, symbol))
}

impl Player {
    pub fn bitmap_error(&self) -> Option<&BitmapError> {
        self.graph.bitmap_error.as_ref()
    }
    pub fn as2_bitmap_command(
        &mut self,
        command: crate::pixels::As2BitmapCommand,
    ) -> Result<serde_json::Value, BitmapError> {
        self.graph.pixels.as2_command(command)
    }
    pub fn hydrate_sprite_sheet(
        &mut self,
        sheet_png_bytes: &[u8],
        manifest_json: &str,
    ) -> Result<usize, BitmapError> {
        self.graph
            .hydrate_sprite_sheet(sheet_png_bytes, manifest_json)
    }
    pub fn as2_bitmap_load(
        &mut self,
        project: &Project,
        id: &str,
    ) -> Result<Option<PixelRef>, BitmapError> {
        if id.is_empty() || id.len() > 1024 {
            return Err(BitmapError(
                "loadBitmap requires a resource or linkage ID of 1 through 1024 bytes".into(),
            ));
        }
        let Some(resource) = resolve_as2_bitmap_resource(project, id) else {
            return Ok(None);
        };
        if resource.data_base64.is_empty()
            && let Some(hydrated) = self.graph.hydrated_bitmaps.get(&resource.id).cloned()
        {
            self.graph
                .pixels
                .reserve_import(hydrated.width, hydrated.height)?;
            return self.graph.pixels.import(&hydrated).map(Some);
        }
        let (width, height) = resource
            .image_dimensions()
            .map_err(|e| BitmapError(e.to_string()))?;
        self.graph.pixels.reserve_import(width, height)?;
        let image = resource
            .decode_image()
            .map_err(|e| BitmapError(e.to_string()))?;
        self.graph.pixels.import(&image).map(Some)
    }
    pub fn bitmap_create(
        &mut self,
        w: u32,
        h: u32,
        transparent: bool,
        fill_argb: u32,
    ) -> Result<PixelRef, BitmapError> {
        self.graph.pixels.create(w, h, transparent, fill_argb)
    }
    pub fn bitmap_info(&self, reference: &PixelRef) -> Result<PixelDescriptor, BitmapError> {
        self.graph.pixels.info(reference)
    }
    pub fn bitmap_pixels(&self, reference: &PixelRef) -> Result<Vec<u8>, BitmapError> {
        self.graph.pixels.pixels(reference)
    }
    pub fn bitmap_histogram(
        &self,
        reference: &PixelRef,
        rect: Option<crate::BitmapRect>,
    ) -> Result<Vec<Vec<u32>>, BitmapError> {
        self.graph.pixels.histogram(reference, rect)
    }
    pub fn bitmap_get_pixels(
        &self,
        reference: &PixelRef,
        rect: crate::BitmapRect,
        little: bool,
    ) -> Result<Vec<u8>, BitmapError> {
        self.graph.pixels.get_pixels(reference, rect, little)
    }
    pub fn bitmap_set_pixels(
        &mut self,
        reference: &PixelRef,
        rect: crate::BitmapRect,
        bytes: &[u8],
        little: bool,
    ) -> Result<crate::PixelWrite, BitmapError> {
        self.graph.pixels.set_pixels(reference, rect, bytes, little)
    }
    pub fn bitmap_color_transform(
        &mut self,
        reference: &PixelRef,
        rect: crate::BitmapRect,
        color: ColorTransform,
    ) -> Result<(), BitmapError> {
        self.graph.pixels.color_transform(reference, rect, color)
    }
    pub fn bitmap_generate_filter_rect(
        &self,
        reference: &PixelRef,
        rect: crate::BitmapRect,
        filter: &FilterSpec,
    ) -> Result<crate::BitmapRect, BitmapError> {
        self.graph
            .pixels
            .generate_filter_rect(reference, rect, filter)
    }
    pub fn bitmap_apply_filter(
        &mut self,
        dst: &PixelRef,
        src: &PixelRef,
        rect: crate::BitmapRect,
        dx: i32,
        dy: i32,
        filter: &FilterSpec,
    ) -> Result<(), BitmapError> {
        self.graph
            .pixels
            .apply_filter(dst, src, rect, dx, dy, filter)
    }
    pub fn bitmap_encode_png(
        &self,
        reference: &PixelRef,
        rect: crate::BitmapRect,
        fast: bool,
    ) -> Result<Vec<u8>, BitmapError> {
        self.graph.pixels.encode_png(reference, rect, fast)
    }
    pub fn bitmap_encode_jpeg(
        &self,
        reference: &PixelRef,
        rect: crate::BitmapRect,
        quality: u8,
    ) -> Result<Vec<u8>, BitmapError> {
        self.graph.pixels.encode_jpeg(reference, rect, quality)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bitmap_draw_rgba(
        &mut self,
        reference: &PixelRef,
        width: u32,
        height: u32,
        rgba: &[u8],
        clip_rect: Option<crate::BitmapRect>,
        expected_revision: &str,
    ) -> Result<(), BitmapError> {
        self.graph
            .pixels
            .draw_rgba(reference, width, height, rgba, clip_rect, expected_revision)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bitmap_draw_rgba_blended(
        &mut self,
        reference: &PixelRef,
        width: u32,
        height: u32,
        rgba: &[u8],
        clip_rect: Option<crate::BitmapRect>,
        expected_revision: &str,
        blend_mode: &str,
    ) -> Result<(), BitmapError> {
        self.graph.pixels.draw_rgba_blended(
            reference,
            width,
            height,
            rgba,
            clip_rect,
            expected_revision,
            blend_mode,
        )
    }
    pub fn bitmap_get_pixel(
        &self,
        reference: &PixelRef,
        x: i32,
        y: i32,
        with_alpha: bool,
    ) -> Result<u32, BitmapError> {
        self.graph.pixels.get(reference, x, y, with_alpha)
    }
    pub fn bitmap_set_pixel(
        &mut self,
        reference: &PixelRef,
        x: i32,
        y: i32,
        color: u32,
        with_alpha: bool,
    ) -> Result<(), BitmapError> {
        self.graph.pixels.set(reference, x, y, color, with_alpha)
    }
    pub fn bitmap_fill_rect(
        &mut self,
        reference: &PixelRef,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        color: u32,
    ) -> Result<(), BitmapError> {
        self.graph.pixels.fill(reference, x, y, w, h, color)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn bitmap_copy_pixels(
        &mut self,
        dst: &PixelRef,
        src: &PixelRef,
        sx: i32,
        sy: i32,
        w: i32,
        h: i32,
        dx: i32,
        dy: i32,
        merge_alpha: bool,
    ) -> Result<(), BitmapError> {
        self.graph
            .pixels
            .copy(dst, src, sx, sy, w, h, dx, dy, merge_alpha)
    }
    pub fn bitmap_clone(&mut self, reference: &PixelRef) -> Result<PixelRef, BitmapError> {
        self.graph.pixels.clone_buffer(reference)
    }
    pub fn bitmap_scroll(
        &mut self,
        reference: &PixelRef,
        dx: i32,
        dy: i32,
    ) -> Result<(), BitmapError> {
        self.graph.pixels.scroll(reference, dx, dy)
    }
    pub fn bitmap_dispose(&mut self, reference: &PixelRef) -> Result<(), BitmapError> {
        self.graph.pixels.dispose(reference)
    }
    fn bitmap_object(
        &self,
        project: &Project,
        reference: &ObjectRef,
    ) -> Result<RuntimeObject, BitmapError> {
        self.runtime_snapshot(project)
            .objects
            .into_iter()
            .find(|node| &node.object_ref == reference && node.display.owner == "bitmap")
            .ok_or_else(|| BitmapError("requires a live Bitmap display object".into()))
    }
    pub fn object_bitmap(
        &self,
        project: &Project,
        reference: &ObjectRef,
    ) -> Result<Option<PixelRef>, BitmapError> {
        Ok(self
            .bitmap_object(project, reference)?
            .bitmap
            .map(|b| b.pixel_ref))
    }
    pub fn object_set_bitmap(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        value: Option<PixelRef>,
    ) -> Result<bool, BitmapError> {
        let object = self.bitmap_object(project, reference)?;
        if let Some(r) = &value {
            let info = self.graph.pixels.info(r)?;
            if info.disposed {
                return Err(BitmapError("cannot assign disposed BitmapData".into()));
            }
        }
        let mut state = self
            .graph
            .bitmap_state(reference, object.display.kind.as_ref());
        state.pixel_ref = value;
        self.graph.bitmap_states.insert(reference.clone(), state);
        Ok(true)
    }
    pub fn object_smoothing(
        &self,
        project: &Project,
        reference: &ObjectRef,
    ) -> Result<bool, BitmapError> {
        Ok(self.bitmap_object(project, reference)?.smoothing)
    }
    pub fn object_set_smoothing(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        value: bool,
    ) -> Result<bool, BitmapError> {
        let object = self.bitmap_object(project, reference)?;
        let mut state = self
            .graph
            .bitmap_state(reference, object.display.kind.as_ref());
        state.smoothing = value;
        self.graph.bitmap_states.insert(reference.clone(), state);
        Ok(true)
    }
    pub fn object_pixel_snapping(
        &self,
        project: &Project,
        reference: &ObjectRef,
    ) -> Result<PixelSnapping, BitmapError> {
        Ok(self.bitmap_object(project, reference)?.pixel_snapping)
    }
    pub fn object_set_pixel_snapping(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        value: PixelSnapping,
    ) -> Result<bool, BitmapError> {
        let object = self.bitmap_object(project, reference)?;
        let mut state = self
            .graph
            .bitmap_state(reference, object.display.kind.as_ref());
        state.pixel_snapping = value;
        self.graph.bitmap_states.insert(reference.clone(), state);
        Ok(true)
    }
}
