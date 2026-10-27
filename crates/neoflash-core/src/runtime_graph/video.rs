//! Shared Video display state; decoder frames stay in the host media backend.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoState {
    pub width: f64,
    pub height: f64,
    pub smoothing: bool,
}
impl Default for VideoState {
    fn default() -> Self {
        Self {
            width: 320.,
            height: 240.,
            smoothing: false,
        }
    }
}
impl VideoState {
    pub fn from_kind(kind: &ElementKind) -> Option<Self> {
        if let ElementKind::Video { width, height } = kind {
            Some(Self {
                width: *width,
                height: *height,
                smoothing: false,
            })
        } else {
            None
        }
    }
    pub fn validate(self) -> Result<(), DisplayError> {
        if [self.width, self.height]
            .into_iter()
            .all(|value| value.is_finite() && (0. ..=8192.).contains(&value))
        {
            Ok(())
        } else {
            Err(error("Video dimensions must be finite and in 0..=8192"))
        }
    }
}
impl RuntimeGraph {
    pub(super) fn video_state(
        &self,
        reference: &ObjectRef,
        kind: Option<&ElementKind>,
    ) -> Option<VideoState> {
        if !self
            .dynamic
            .get(reference)
            .is_some_and(|node| node.kind == DynamicKind::Video)
            && !matches!(kind, Some(ElementKind::Video { .. }))
        {
            return None;
        }
        self.video_states
            .get(reference)
            .copied()
            .or_else(|| kind.and_then(VideoState::from_kind))
    }
    pub(super) fn retire_video(&mut self, retired: &BTreeSet<ObjectRef>) {
        self.video_states.retain(|id, _| !retired.contains(id));
    }
}
impl Player {
    pub fn object_video_info(
        &self,
        project: &Project,
        reference: &ObjectRef,
    ) -> Result<VideoState, DisplayError> {
        if !self.graph.alive(reference) {
            return Err(error(
                "Video reference is stale or belongs to another player",
            ));
        }
        self.runtime_snapshot(project)
            .objects
            .into_iter()
            .find(|node| &node.object_ref == reference)
            .and_then(|node| node.video)
            .ok_or_else(|| error("This display object is not a Video"))
    }
    pub fn object_video_update(
        &mut self,
        project: &Project,
        reference: &ObjectRef,
        state: VideoState,
    ) -> Result<(), DisplayError> {
        self.object_video_info(project, reference)?;
        state.validate()?;
        self.graph.video_states.insert(reference.clone(), state);
        Ok(())
    }
}
