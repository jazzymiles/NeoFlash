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
        let old = self.text_states.get(reference).map_or(0, |s| s.text.len());
        let total = self.text_bytes - old + state.text.len();
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
                self.text_bytes -= state.text.len();
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
