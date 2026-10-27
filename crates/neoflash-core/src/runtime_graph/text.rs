use super::*;
const MAX_RUNTIME_TEXT_BYTES: usize = 16 * 1024 * 1024;
#[derive(Clone, Debug, Serialize)]
pub struct TextReflow {
    pub field: TextFieldState,
    pub layout: TextLayout,
    pub scroll_changed: bool,
}
impl RuntimeGraph {
    pub(super) fn text_state(
        &self,
        reference: &ObjectRef,
        kind: Option<&ElementKind>,
    ) -> Option<TextFieldState> {
        self.text_states
            .get(reference)
            .cloned()
            .or_else(|| kind.and_then(TextFieldState::from_kind))
    }
    pub(crate) fn store_text(
        &mut self,
        reference: &ObjectRef,
        state: TextFieldState,
    ) -> Result<(), TextError> {
        state.validate()?;
        let old = self
            .text_states
            .get(reference)
            .map_or(0, TextFieldState::storage_bytes);
        let total = self.text_bytes - old + state.storage_bytes();
        if total > MAX_RUNTIME_TEXT_BYTES {
            return Err(TextError(
                "runtime text overrides exceed the 16 MiB UTF-8 budget".into(),
            ));
        }
        self.text_bytes = total;
        self.text_states.insert(reference.clone(), state);
        Ok(())
    }
    pub(super) fn retire_text(&mut self, references: &BTreeSet<ObjectRef>) {
        for reference in references {
            if let Some(state) = self.text_states.remove(reference) {
                self.text_bytes -= state.storage_bytes();
            }
        }
    }
    pub(crate) fn patch_path_text(
        &mut self,
        reference: &ObjectRef,
        kind: &ElementKind,
        patch: &TextPatch,
    ) -> Result<(), TextError> {
        let mut state = self
            .text_state(reference, Some(kind))
            .ok_or_else(|| TextError("requires a TextField".into()))?;
        state.apply(patch)?;
        self.store_text(reference, state)
    }
}
impl Player {
    fn commit_text_layout<M>(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        original: TextFieldState,
        mut next: TextFieldState,
        measure: M,
    ) -> Result<TextReflow, TextError>
    where
        M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
    {
        let layout = layout_text(&next, measure)?;
        let old_width = next.box_size.map_or(layout.text_width, |b| b.width);
        let shift = if next.word_wrap {
            0.
        } else {
            match next.auto_size {
                TextAutoSize::Right => 1.,
                TextAutoSize::Center => 0.5,
                _ => 0.,
            }
        } * (old_width - layout.viewport.map_or(old_width, |b| b.width));
        next.box_size = layout.viewport;
        next.scroll_h = layout.scroll_h;
        next.scroll_v = layout.scroll_v;
        let scroll_changed =
            original.scroll_h != next.scroll_h || original.scroll_v != next.scroll_v;
        let matrix = if shift != 0. {
            let mut matrix = self
                .object_matrix(project, reference)
                .map_err(|e| TextError(e.to_string()))?;
            matrix[4] += matrix[0] * shift;
            matrix[5] += matrix[1] * shift;
            transform::validate_matrix(matrix).map_err(|e| TextError(e.to_string()))?;
            Some(matrix)
        } else {
            None
        };
        next.revision = original.revision.clone();
        if next != original {
            next.revision = original
                .revision
                .parse::<u64>()
                .map_err(|_| TextError("invalid text revision".into()))?
                .checked_add(1)
                .ok_or_else(|| TextError("text revision exhausted".into()))?
                .to_string();
            self.graph.store_text(reference, next.clone())?;
        }
        if let Some(matrix) = matrix {
            // All validation and memory reservations preceded this commit; the
            // object cannot retire while the synchronous Rust call is active.
            self.object_set_matrix(reference, matrix)
                .map_err(|e| TextError(e.to_string()))?;
        }
        Ok(TextReflow {
            field: next,
            layout,
            scroll_changed,
        })
    }
    pub fn object_text_reflow<M>(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        measure: M,
    ) -> Result<TextReflow, TextError>
    where
        M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
    {
        let original = self.object_text_info(project, reference)?;
        self.commit_text_layout(project, reference, original.clone(), original, measure)
    }
    pub fn object_text_update_layout<M>(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        patch: &TextPatch,
        measure: M,
    ) -> Result<TextReflow, TextError>
    where
        M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
    {
        let original = self.object_text_info(project, reference)?;
        let mut next = original.clone();
        next.apply(patch)?;
        self.commit_text_layout(project, reference, original, next, measure)
    }
    pub fn object_text_input_layout<M>(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        edit: &TextInputEdit,
        measure: M,
    ) -> Result<TextReflow, TextError>
    where
        M: FnMut(&[String], &TextFormat) -> Result<Vec<f64>, TextError>,
    {
        let original = self
            .runtime_snapshot(project)
            .objects
            .into_iter()
            .find(|node| {
                &node.object_ref == reference && node.on_stage && node.display.effective_visible
            })
            .and_then(|node| node.text)
            .ok_or_else(|| {
                TextError("user editing requires an attached visible TextField".into())
            })?;
        let mut next = original.clone();
        next.apply_input(edit)?;
        self.commit_text_layout(project, reference, original, next, measure)
    }
    pub fn object_text_info(
        &self,
        project: &Project,
        reference: &ObjectRef,
    ) -> Result<TextFieldState, TextError> {
        self.runtime_snapshot(project)
            .objects
            .into_iter()
            .find(|node| &node.object_ref == reference)
            .and_then(|node| node.text)
            .ok_or_else(|| TextError("requires a live TextField display object".into()))
    }
    pub fn object_text_update(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        patch: &TextPatch,
    ) -> Result<bool, TextError> {
        let mut state = self.object_text_info(project, reference)?;
        state.apply(patch)?;
        self.graph.store_text(reference, state)?;
        Ok(true)
    }
    pub fn object_text_input(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        edit: &TextInputEdit,
    ) -> Result<TextFieldState, TextError> {
        let mut state = self
            .runtime_snapshot(project)
            .objects
            .into_iter()
            .find(|node| {
                &node.object_ref == reference && node.on_stage && node.display.effective_visible
            })
            .and_then(|node| node.text)
            .ok_or_else(|| {
                TextError("user editing requires an attached visible TextField".into())
            })?;
        state.apply_input(edit)?;
        self.graph.store_text(reference, state.clone())?;
        Ok(state)
    }
    pub fn object_set_text(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        text: &str,
    ) -> bool {
        self.object_text_update(
            project,
            reference,
            &TextPatch {
                text: Some(text.into()),
                ..TextPatch::default()
            },
        )
        .is_ok()
    }
    pub fn object_set_text_color(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        color: u32,
    ) -> bool {
        self.object_text_update(
            project,
            reference,
            &TextPatch {
                format: Some(TextFormatPatch {
                    color: Some(color),
                    ..TextFormatPatch::default()
                }),
                ..TextPatch::default()
            },
        )
        .is_ok()
    }
}
