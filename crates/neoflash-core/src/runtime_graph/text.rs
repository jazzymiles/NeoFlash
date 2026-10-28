use super::*;
const MAX_RUNTIME_TEXT_BYTES: usize = 16 * 1024 * 1024;
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
    /// Fit an AS2 field using Rust layout measured by the existing font host.
    pub fn as2_text_fit(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        layout: &TextLayout,
    ) -> Result<bool, TextError> {
        let mut state = self.object_text_info(project, reference)?;
        let mode = state.as2_viewport.auto_size.as_str();
        if mode == "none" {
            return Ok(false);
        }
        let old = state
            .box_size
            .ok_or_else(|| TextError("autoSize requires a boxed TextField".into()))?;
        if ![layout.text_width, layout.text_height]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.)
        {
            return Err(TextError(
                "autoSize requires finite layout dimensions".into(),
            ));
        }
        let width = if state.word_wrap {
            old.width
        } else {
            layout.text_width
                + state.format.left_margin
                + state.format.right_margin
                + state.format.block_indent
                + state.format.indent.max(0.)
                + if state.format.bullet {
                    state.format.size
                } else {
                    0.
                }
                + 4.
        };
        let height = layout.text_height + 4.;
        if (width - old.width).abs() < 1e-9 && (height - old.height).abs() < 1e-9 {
            return Ok(false);
        }
        let delta = (old.width - width)
            * match mode {
                "center" => 0.5,
                "right" => 1.,
                _ => 0.,
            };
        let mut matrix = self
            .object_matrix(project, reference)
            .map_err(|e| TextError(e.to_string()))?;
        matrix[4] += matrix[0] * delta;
        matrix[5] += matrix[1] * delta;
        let affine = transform::AffineState::new(matrix).map_err(|e| TextError(e.to_string()))?;
        state.apply(&TextPatch {
            width: Some(width),
            height: Some(height),
            ..TextPatch::default()
        })?;
        self.graph.store_text(reference, state)?;
        self.graph.affines.insert(reference.clone(), affine);
        Ok(true)
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
