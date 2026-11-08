use eframe::egui::Pos2;
#[cfg(test)]
use eframe::egui::Rect;
use neoflash_core::{AuthoredScene, affine_multiply, invert_affine_matrix};

const IDENTITY: [f64; 6] = [1., 0., 0., 1., 0., 0.];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StageMappingError {
    NonFinite,
    Singular,
}

impl std::fmt::Display for StageMappingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NonFinite => "the stage mapping contains a non-finite value",
            Self::Singular => "the stage mapping is not invertible",
        })
    }
}

/// One immutable conversion between the currently edited symbol's timeline
/// coordinates and the root Stage timeline coordinates.
///
/// Authored geometry remains local. Callers map evaluated scenes for display
/// and inverse-map pointer input before performing document mutations.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StageMapping {
    edit_to_stage: [f64; 6],
    stage_to_edit: [f64; 6],
    fingerprint: u64,
}

impl PartialEq for StageMapping {
    fn eq(&self, other: &Self) -> bool {
        self.fingerprint == other.fingerprint
            && self
                .edit_to_stage
                .iter()
                .zip(other.edit_to_stage)
                .all(|(a, b)| a.to_bits() == b.to_bits())
    }
}
impl Eq for StageMapping {}

impl Default for StageMapping {
    fn default() -> Self {
        Self::identity()
    }
}

impl StageMapping {
    pub(crate) fn identity() -> Self {
        Self {
            edit_to_stage: IDENTITY,
            stage_to_edit: IDENTITY,
            fingerprint: fingerprint(IDENTITY),
        }
    }

    pub(crate) fn try_new(edit_to_stage: [f64; 6]) -> Result<Self, StageMappingError> {
        if edit_to_stage.iter().any(|value| !value.is_finite()) {
            return Err(StageMappingError::NonFinite);
        }
        let stage_to_edit =
            invert_affine_matrix(edit_to_stage).ok_or(StageMappingError::Singular)?;
        if stage_to_edit.iter().any(|value| !value.is_finite()) {
            return Err(StageMappingError::NonFinite);
        }
        Ok(Self {
            edit_to_stage,
            stage_to_edit,
            fingerprint: fingerprint(edit_to_stage),
        })
    }

    pub(crate) fn matrix(self) -> [f64; 6] {
        self.edit_to_stage
    }

    #[cfg(test)]
    pub(crate) fn fingerprint(self) -> u64 {
        self.fingerprint
    }

    #[cfg(test)]
    pub(crate) fn is_identity(self) -> bool {
        self.edit_to_stage == IDENTITY
    }

    pub(crate) fn edit_to_stage_point(self, point: [f64; 2]) -> [f64; 2] {
        map_point(self.edit_to_stage, point)
    }

    pub(crate) fn stage_to_edit_point(self, point: [f64; 2]) -> [f64; 2] {
        map_point(self.stage_to_edit, point)
    }

    pub(crate) fn edit_to_stage_pos(self, point: Pos2) -> Pos2 {
        pos(self.edit_to_stage_point([f64::from(point.x), f64::from(point.y)]))
    }

    pub(crate) fn stage_to_edit_pos(self, point: Pos2) -> Pos2 {
        pos(self.stage_to_edit_point([f64::from(point.x), f64::from(point.y)]))
    }

    pub(crate) fn edit_to_screen(self, origin: Pos2, zoom: f32, point: Pos2) -> Pos2 {
        origin + self.edit_to_stage_pos(point).to_vec2() * zoom
    }

    pub(crate) fn screen_to_edit(self, origin: Pos2, zoom: f32, point: Pos2) -> Pos2 {
        let stage = Pos2::ZERO + (point - origin) / zoom;
        self.stage_to_edit_pos(stage)
    }

    pub(crate) fn map_matrix(self, local: [f64; 6]) -> [f64; 6] {
        affine_multiply(self.edit_to_stage, local)
    }

    #[cfg(test)]
    pub(crate) fn map_rect(self, rect: Rect) -> Rect {
        let mut mapped = Rect::NOTHING;
        for corner in [
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ] {
            mapped.extend_with(self.edit_to_stage_pos(corner));
        }
        mapped
    }

    #[cfg(test)]
    pub(crate) fn map_rect_to_screen(self, origin: Pos2, zoom: f32, rect: Rect) -> Rect {
        let mut mapped = Rect::NOTHING;
        for corner in [
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ] {
            mapped.extend_with(self.edit_to_screen(origin, zoom, corner));
        }
        mapped
    }

    /// Smallest and largest length multipliers of the affine linear part.
    /// These bound a circular screen-space tolerance under skew or non-uniform scale.
    pub(crate) fn scale_bounds(self) -> (f64, f64) {
        let [a, b, c, d, _, _] = self.edit_to_stage;
        let trace = a * a + b * b + c * c + d * d;
        let determinant = a * d - b * c;
        let discriminant = (trace * trace - 4. * determinant * determinant)
            .max(0.)
            .sqrt();
        let maximum = ((trace + discriminant) * 0.5).max(0.).sqrt();
        let minimum = ((trace - discriminant) * 0.5).max(0.).sqrt();
        (minimum, maximum)
    }

    /// Clone an evaluated local scene into root Stage coordinates. Every path
    /// and every geometric/alpha clip matrix receives the same prefix so the
    /// existing renderers keep one coherent coordinate space.
    pub(crate) fn map_scene(self, scene: &AuthoredScene) -> AuthoredScene {
        let mut mapped = scene.clone();
        for draw in &mut mapped.draws {
            draw.item.transform = self.map_matrix(draw.item.transform);
        }
        for clip in &mut mapped.clips {
            for component in &mut clip.components {
                component.matrix = self.map_matrix(component.matrix);
            }
            for component in clip.alpha_components.iter_mut().flatten() {
                component.matrix = self.map_matrix(component.matrix);
            }
        }
        mapped
    }
}

fn map_point(matrix: [f64; 6], point: [f64; 2]) -> [f64; 2] {
    [
        matrix[0] * point[0] + matrix[2] * point[1] + matrix[4],
        matrix[1] * point[0] + matrix[3] * point[1] + matrix[5],
    ]
}

fn pos(point: [f64; 2]) -> Pos2 {
    Pos2::new(point[0] as f32, point[1] as f32)
}

fn fingerprint(matrix: [f64; 6]) -> u64 {
    matrix
        .into_iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, value| {
            (hash ^ value.to_bits()).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoflash_core::{
        AlphaClipComponent, AuthoredDraw, AuthoredMaskGroup, BlendMode, ClipComponent,
        ColorTransform, DrawItem, ElementKind, FillRule, PathCommand, RuntimeClip,
    };

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
    }

    #[test]
    fn affine_points_matrices_and_rectangles_round_trip() {
        let mapping = StageMapping::try_new([2., 0.5, -0.25, 3., 40., -20.]).unwrap();
        let local = [12., -7.];
        let stage = mapping.edit_to_stage_point(local);
        let round_trip = mapping.stage_to_edit_point(stage);
        near(round_trip[0], local[0]);
        near(round_trip[1], local[1]);

        let origin = Pos2::new(100., 200.);
        let screen = mapping.edit_to_screen(origin, 2., Pos2::new(12., -7.));
        let edit = mapping.screen_to_edit(origin, 2., screen);
        near(f64::from(edit.x), 12.);
        near(f64::from(edit.y), -7.);

        let child = [0., 1., -1., 0., 5., 6.];
        assert_eq!(
            mapping.map_matrix(child),
            affine_multiply(mapping.matrix(), child)
        );
        let rect = mapping.map_rect(Rect::from_min_max(Pos2::ZERO, Pos2::new(10., 20.)));
        assert!(rect.contains(mapping.edit_to_stage_pos(Pos2::new(10., 20.))));
        assert_eq!(mapping, StageMapping::try_new(mapping.matrix()).unwrap());
        assert_eq!(
            mapping.fingerprint(),
            StageMapping::try_new(mapping.matrix())
                .unwrap()
                .fingerprint()
        );
        assert!(!mapping.is_identity());
        assert!(StageMapping::identity().is_identity());
        let (minimum, maximum) = mapping.scale_bounds();
        assert!(minimum > 0. && maximum >= minimum);
    }

    #[test]
    fn invalid_matrices_are_rejected() {
        assert_eq!(
            StageMapping::try_new([1., 0., 0., 0., 0., 0.]),
            Err(StageMappingError::Singular)
        );
        assert_eq!(
            StageMapping::try_new([1., 0., 0., 1., f64::NAN, 0.]),
            Err(StageMappingError::NonFinite)
        );
    }

    #[test]
    fn scene_mapping_prefixes_draw_geometric_and_alpha_clip_matrices() {
        let draw_matrix = [1., 0., 0., 1., 2., 3.];
        let clip_matrix = [2., 0., 0., 2., 4., 5.];
        let alpha_matrix = [1., 0.5, 0., 1., 6., 7.];
        let scene = AuthoredScene {
            draws: vec![AuthoredDraw {
                item: DrawItem {
                    element_id: "shape".into(),
                    name: String::new(),
                    instance_path: vec!["shape".into()],
                    layer_id: "layer".into(),
                    transform: draw_matrix,
                    alpha: 1.,
                    color_transform: ColorTransform::default(),
                    path_fingerprint: None,
                    kind: ElementKind::Rectangle {
                        width: 10.,
                        height: 20.,
                        fill: [0, 0, 0, 255],
                        stroke: [0; 4],
                        stroke_width: 0.,
                    },
                },
                clip_ids: vec!["clip".into()],
                outline_color: None,
                mask_group: Some("group".into()),
                blend_mode: BlendMode::Normal,
            }],
            clips: vec![RuntimeClip {
                id: "clip".into(),
                components: vec![ClipComponent {
                    commands: vec![PathCommand::MoveTo { x: 0., y: 0. }],
                    fill_rule: FillRule::NonZero,
                    matrix: clip_matrix,
                    clip_ids: vec![],
                }],
                alpha_components: Some(vec![AlphaClipComponent {
                    commands: vec![PathCommand::MoveTo { x: 0., y: 0. }],
                    fill_rule: FillRule::NonZero,
                    matrix: alpha_matrix,
                    alpha: 0.5,
                    clip_ids: vec![],
                    resource_bitmap: None,
                    fill_gradient: None,
                    fill_bitmap: None,
                    luminance: false,
                }]),
                invert: false,
            }],
            mask_groups: vec![AuthoredMaskGroup {
                id: "group".into(),
                parent_id: None,
                clip_ids: vec!["clip".into()],
                blend_mode: BlendMode::Normal,
            }],
        };
        let mapping = StageMapping::try_new([0., 2., -3., 0., 10., 20.]).unwrap();
        let mapped = mapping.map_scene(&scene);

        assert_eq!(
            mapped.draws[0].item.transform,
            mapping.map_matrix(draw_matrix)
        );
        assert_eq!(
            mapped.clips[0].components[0].matrix,
            mapping.map_matrix(clip_matrix)
        );
        assert_eq!(
            mapped.clips[0].alpha_components.as_ref().unwrap()[0].matrix,
            mapping.map_matrix(alpha_matrix)
        );
        assert_eq!(mapped.draws[0].clip_ids, scene.draws[0].clip_ids);
        assert_eq!(mapped.draws[0].mask_group, scene.draws[0].mask_group);
        assert_eq!(mapped.mask_groups[0].id, scene.mask_groups[0].id);
    }
}
