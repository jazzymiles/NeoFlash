//! Runtime-only affine/color overrides. Authoring TRS and document JSON stay pure.
use super::*;

/// Straight RGBA channel transform. Offsets are expressed in 0..255 channel units.
/// Local assignments are bounded; concatenation can produce larger finite values.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorTransform {
    pub multipliers: [f64; 4],
    pub offsets: [f64; 4],
}
impl Default for ColorTransform {
    fn default() -> Self {
        Self {
            multipliers: [1.; 4],
            offsets: [0.; 4],
        }
    }
}
impl ColorTransform {
    pub fn is_finite(self) -> bool {
        self.multipliers
            .into_iter()
            .chain(self.offsets)
            .all(f64::is_finite)
    }
    pub fn validate(self) -> Result<(), DisplayError> {
        if self.is_finite()
            && self.multipliers.iter().all(|v| v.abs() <= 1e4)
            && self.offsets.iter().all(|v| v.abs() <= 255.)
        {
            Ok(())
        } else {
            Err(error(
                "ColorTransform requires finite multipliers within ±10000 and offsets within ±255",
            ))
        }
    }
    /// Apply local first, then parent, without intermediate channel clamping.
    pub fn then(self, parent: Self) -> Self {
        let mut result = Self::default();
        for channel in 0..4 {
            result.multipliers[channel] = parent.multipliers[channel] * self.multipliers[channel];
            result.offsets[channel] =
                parent.multipliers[channel] * self.offsets[channel] + parent.offsets[channel];
        }
        result
    }
    pub fn apply(self, color: Color) -> Color {
        std::array::from_fn(|i| {
            (f64::from(color[i]) * self.multipliers[i] + self.offsets[i])
                .clamp(0., 255.)
                .round() as u8
        })
    }
    pub(super) fn can_be_visible(self) -> bool {
        self.apply([0; 4])[3] > 0 || self.apply([255; 4])[3] > 0
    }
    pub(super) fn from_alpha(alpha: f64) -> Self {
        Self {
            multipliers: [1., 1., 1., alpha],
            ..Self::default()
        }
    }
    pub(crate) fn transform_kind(self, kind: &mut ElementKind) {
        match kind {
            ElementKind::Rectangle { fill, stroke, .. }
            | ElementKind::Ellipse { fill, stroke, .. }
            | ElementKind::Path { fill, stroke, .. } => {
                *fill = self.apply(*fill);
                *stroke = self.apply(*stroke);
            }
            ElementKind::Text { color, .. } => *color = self.apply(*color),
            _ => {}
        }
    }
    pub(super) fn transform_text(self, field: &TextFieldState) -> TextFieldState {
        let mut result = field.clone();
        let rgb = field.format.color;
        let color = self.apply([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, field.alpha]);
        result.format.color =
            u32::from(color[0]) << 16 | u32::from(color[1]) << 8 | u32::from(color[2]);
        result.alpha = color[3];
        for value in [
            &mut result.as2_viewport.background_color,
            &mut result.as2_viewport.border_color,
        ] {
            let color = self.apply([
                (*value >> 16) as u8,
                (*value >> 8) as u8,
                *value as u8,
                field.alpha,
            ]);
            *value = (u32::from(color[0]) << 16) | (u32::from(color[1]) << 8) | u32::from(color[2]);
        }
        result
    }
}

/// A matrix assignment decodes column magnitudes and two independent axis angles.
/// Scalar mutations retain these angles through zero/negative scales, preserving
/// skew rather than repeatedly decomposing a singular or reflected matrix.
#[derive(Clone, Copy, Debug)]
pub(super) struct AffineState {
    pub matrix: [f64; 6],
    rotation: f64,
    skew: f64,
    scale_x: f64,
    scale_y: f64,
}
fn validate_matrix(matrix: [f64; 6]) -> Result<(), DisplayError> {
    if matrix
        .into_iter()
        .enumerate()
        .all(|(i, v)| v.is_finite() && v.abs() <= if i < 4 { 1e4 } else { 1e12 })
    {
        Ok(())
    } else {
        Err(error(
            "Matrix requires finite linear coefficients within ±10000 and translation within ±1e12",
        ))
    }
}
impl AffineState {
    pub(super) fn new(matrix: [f64; 6]) -> Result<Self, DisplayError> {
        validate_matrix(matrix)?;
        let [a, b, c, d, _, _] = matrix;
        let rotation = b.atan2(a);
        Ok(Self {
            matrix,
            rotation,
            skew: (-c).atan2(d) - rotation,
            scale_x: a.hypot(b),
            scale_y: c.hypot(d),
        })
    }
    pub(super) fn property(self, name: &str) -> Option<f64> {
        match name {
            "x" => Some(self.matrix[4]),
            "y" => Some(self.matrix[5]),
            "rotation" => Some(self.rotation.to_degrees()),
            "scale_x" => Some(self.scale_x),
            "scale_y" => Some(self.scale_y),
            _ => None,
        }
    }
    fn set(&mut self, name: &str, value: f64) -> bool {
        let mut next = *self;
        match name {
            "x" => next.matrix[4] = value,
            "y" => next.matrix[5] = value,
            "rotation" => {
                next.rotation = (value % 360.).to_radians();
                next.x_column();
                next.y_column();
            }
            "scale_x" => {
                next.scale_x = value;
                next.x_column();
            }
            "scale_y" => {
                next.scale_y = value;
                next.y_column();
            }
            _ => return false,
        }
        if validate_matrix(next.matrix).is_err() {
            return false;
        }
        *self = next;
        true
    }
    fn x_column(&mut self) {
        self.matrix[0] = self.rotation.cos() * self.scale_x;
        self.matrix[1] = self.rotation.sin() * self.scale_x;
    }
    fn y_column(&mut self) {
        self.matrix[2] = -(self.rotation + self.skew).sin() * self.scale_y;
        self.matrix[3] = (self.rotation + self.skew).cos() * self.scale_y;
    }
    pub(super) fn update_display(self, display: &mut DisplayNode) {
        display.transform.x = self.matrix[4];
        display.transform.y = self.matrix[5];
        display.transform.rotation = self.rotation.to_degrees();
        display.transform.scale_x = self.scale_x;
        display.transform.scale_y = self.scale_y;
    }
}

impl Player {
    pub(crate) fn transform_property_override(
        &self,
        reference: &ObjectRef,
        name: &str,
    ) -> Option<f64> {
        if name == "alpha" {
            self.graph
                .color_transforms
                .get(reference)
                .map(|value| value.multipliers[3])
        } else {
            self.graph.affines.get(reference)?.property(name)
        }
    }
    pub(crate) fn set_transform_property_override(
        &mut self,
        reference: &ObjectRef,
        name: &str,
        value: f64,
    ) -> Option<bool> {
        if name == "alpha" {
            let color = self.graph.color_transforms.get_mut(reference)?;
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Some(false);
            }
            color.multipliers[3] = value;
            Some(true)
        } else {
            let affine = self.graph.affines.get_mut(reference)?;
            if !value.is_finite()
                || match name {
                    "x" | "y" => value.abs() > 1e12,
                    "scale_x" | "scale_y" => value.abs() > 1e4,
                    "rotation" => value.abs() > 1e9,
                    _ => true,
                }
            {
                return Some(false);
            }
            Some(affine.set(name, value))
        }
    }
    fn source_transform(&self, project: &Project, reference: &ObjectRef) -> Option<Transform> {
        if let Some(dynamic) = self.graph.dynamic.get(reference) {
            return Some(dynamic.transform);
        }
        let (scope, path) = if let Some(address) = self.graph.linked_paths.get(reference) {
            (
                &*self.graph.namespaces.get(&address.namespace)?.player,
                &address.path,
            )
        } else {
            (self, self.graph.paths.get(reference)?)
        };
        scope
            .display_nodes(project)
            .into_iter()
            .find(|node| &node.path == path)
            .map(|node| node.transform)
    }
    pub fn object_matrix(
        &self,
        project: &Project,
        reference: &ObjectRef,
    ) -> Result<[f64; 6], DisplayError> {
        if let Some(value) = self.graph.affines.get(reference) {
            return Ok(value.matrix);
        }
        self.source_transform(project, reference)
            .map(transform_matrix)
            .ok_or_else(|| error("Matrix requires a live display object"))
    }
    pub fn object_set_matrix(
        &mut self,
        reference: &ObjectRef,
        matrix: [f64; 6],
    ) -> Result<bool, DisplayError> {
        if !self.graph.alive(reference) {
            return Err(error("Matrix target is stale or foreign"));
        }
        let affine = AffineState::new(matrix)?;
        self.graph.affines.insert(reference.clone(), affine);
        Ok(true)
    }
    pub fn object_color_transform(
        &self,
        project: &Project,
        reference: &ObjectRef,
    ) -> Result<ColorTransform, DisplayError> {
        if let Some(value) = self.graph.color_transforms.get(reference) {
            return Ok(*value);
        }
        self.source_transform(project, reference)
            .map(|value| ColorTransform::from_alpha(value.alpha))
            .ok_or_else(|| error("ColorTransform requires a live display object"))
    }
    pub fn object_set_color_transform(
        &mut self,
        reference: &ObjectRef,
        value: ColorTransform,
    ) -> Result<bool, DisplayError> {
        if !self.graph.alive(reference) {
            return Err(error("ColorTransform target is stale or foreign"));
        }
        value.validate()?;
        self.graph.color_transforms.insert(reference.clone(), value);
        Ok(true)
    }
    /// Bounded render copy, never a mutation/revision of shared BitmapData.
    pub fn bitmap_pixels_transformed(
        &self,
        reference: &PixelRef,
        color: ColorTransform,
    ) -> Result<Vec<u8>, BitmapError> {
        if !color.is_finite() {
            return Err(BitmapError("composed ColorTransform must be finite".into()));
        }
        let mut pixels = self.bitmap_pixels(reference)?;
        for pixel in pixels.as_chunks_mut::<4>().0 {
            *pixel = color.apply(*pixel);
        }
        Ok(pixels)
    }
}
