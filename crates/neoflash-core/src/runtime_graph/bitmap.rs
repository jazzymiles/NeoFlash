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
