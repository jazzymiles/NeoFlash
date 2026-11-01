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
            let result = project
                .resources
                .iter()
                .find(|resource| resource.id == id)
                .ok_or_else(|| BitmapError("missing image resource".into()))
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
    pub fn as2_bitmap_load(
        &mut self,
        project: &Project,
        id: &str,
    ) -> Result<Option<PixelRef>, BitmapError> {
        if id.is_empty() || id.len() > 1024 {
            return Err(BitmapError(
                "loadBitmap requires a resource ID of 1 through 1024 bytes".into(),
            ));
        }
        let Some(resource) = project.resources.iter().find(|resource| resource.id == id) else {
            return Ok(None);
        };
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
