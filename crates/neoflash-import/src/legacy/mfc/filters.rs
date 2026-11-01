//! Instance/text graphic filter lists. Versioned layout facts follow the pinned
//! JPEXS reader (0923d70870d11b236755af71337da8d0110a57aa,
//! FilterRecordDecoder.java); this is an independent bounded decoder. Each type
//! and nested block (shared shadow settings, enabled state, gradient) carries its
//! own revision. Unproven revisions and type codes stop explicitly.
use super::*;

/// Native filter lists are short; this bounds allocation before the remaining
/// input check.
const MAX_FILTERS: u32 = 1024;

impl Reader<'_> {
    /// Presence byte, then (when nonzero) a counted inline filter list.
    pub(super) fn filters(&mut self) -> (Vec<TimelineFilter>, TimelineByteSpan, Result<()>) {
        let start = self.pos;
        let mut filters = Vec::new();
        let parsed = (|| {
            if self.byte()? == 0 {
                return Ok(());
            }
            let count = self.dword()?;
            if count > MAX_FILTERS || count as usize > self.data.len() - self.pos {
                return stop(
                    "fla_record_limit",
                    "Filter count exceeds 1024 or the remaining input.",
                );
            }
            for _ in 0..count {
                self.retain_budget()?;
                let filter_start = self.pos;
                let type_code = self.byte()?;
                let kind = match type_code {
                    0 => TimelineFilterKind::DropShadow,
                    1 => TimelineFilterKind::Blur,
                    2 => TimelineFilterKind::Glow,
                    3 => TimelineFilterKind::Bevel,
                    4 => TimelineFilterKind::GradientGlow,
                    6 => TimelineFilterKind::AdjustColor,
                    7 => TimelineFilterKind::GradientBevel,
                    _ => {
                        return stop(
                            "fla_filter_type_unsupported",
                            format!("Filter type {type_code} has no known authoring layout."),
                        );
                    }
                };
                let mut params = TimelineFilterParams::default();
                let revision = match kind {
                    TimelineFilterKind::DropShadow => self.shadow(&mut params)?,
                    TimelineFilterKind::Blur => self.blur(&mut params)?,
                    TimelineFilterKind::Glow => self.glow(&mut params)?,
                    TimelineFilterKind::Bevel => self.bevel(&mut params)?,
                    TimelineFilterKind::GradientGlow => self.filter_gradient(&mut params)?,
                    TimelineFilterKind::AdjustColor => {
                        let revision = self.revision(1, "adjust-color filter")?;
                        if revision != 0 {
                            self.filter_state(&mut params)?;
                            params.brightness = Some(self.float()?);
                            params.contrast = Some(self.float()?);
                            params.saturation = Some(self.float()?);
                            params.hue = Some(self.float()?);
                        }
                        revision
                    }
                    TimelineFilterKind::GradientBevel => {
                        let revision = self.revision(1, "gradient-bevel filter")?;
                        if revision != 0 {
                            self.filter_gradient(&mut params)?;
                        }
                        revision
                    }
                };
                filters.push(TimelineFilter {
                    encoded: TimelineByteSpan {
                        start: filter_start,
                        end: self.pos,
                    },
                    type_code,
                    kind,
                    revision,
                    enabled: params.enabled_word.map(|word| word != 0),
                    params,
                });
            }
            Ok(())
        })();
        let span = TimelineByteSpan {
            start,
            end: self.pos,
        };
        (filters, span, parsed)
    }
    fn filter_state(&mut self, params: &mut TimelineFilterParams) -> Result<()> {
        let revision = self.revision(1, "filter state")?;
        params.state_revision = Some(revision);
        if revision != 0 {
            params.enabled_word = Some(self.dword()?);
        }
        Ok(())
    }
    /// Shared drop-shadow settings block, also nested in later blur, glow,
    /// bevel and gradient records with its own revision.
    fn shadow(&mut self, p: &mut TimelineFilterParams) -> Result<u8> {
        let v = self.revision(4, "shadow filter settings")?;
        p.settings_revision = Some(v);
        if v == 1 || v == 2 {
            p.angle_radians = Some(self.float()?);
            p.color_rgba = Some(self.dword()?);
            p.blur_x = Some(self.float()?);
            p.distance = Some(self.float()?);
            p.inner = Some(self.dword()?);
            p.knockout = Some(self.dword()?);
            if v == 2 {
                self.filter_state(p)?;
                p.quality = Some(self.dword()?);
            }
        } else if v >= 3 {
            self.filter_state(p)?;
            p.color_rgba = Some(self.dword()?);
            p.distance = Some(self.float()?);
            p.blur_x = Some(self.float()?);
            p.blur_y = Some(self.float()?);
            p.angle_radians = Some(self.float()?);
            p.inner = Some(self.dword()?);
            p.knockout = Some(self.dword()?);
            p.quality = Some(self.dword()?);
            p.strength_percent = Some(self.dword()?);
            if v >= 4 {
                p.hide_object = Some(self.dword()?);
            }
        }
        Ok(v)
    }
    fn blur(&mut self, p: &mut TimelineFilterParams) -> Result<u8> {
        let v = self.revision(3, "blur filter")?;
        match v {
            1 => {
                p.legacy_color_rgba = Some(self.dword()?);
                p.blur_x = Some(self.float()?);
                p.blur_y = Some(self.float()?);
            }
            2 => {
                p.blur_x = Some(self.float()?);
                p.blur_y = Some(self.float()?);
                self.filter_state(p)?;
                p.quality = Some(self.dword()?);
            }
            3 => {
                self.shadow(p)?;
            }
            _ => {}
        }
        Ok(v)
    }
    fn glow(&mut self, p: &mut TimelineFilterParams) -> Result<u8> {
        let v = self.revision(3, "glow filter")?;
        match v {
            1 | 2 => {
                p.color_rgba = Some(self.dword()?);
                p.blur_x = Some(self.float()?);
                p.inner = Some(self.dword()?);
                p.knockout = Some(self.dword()?);
                if v == 2 {
                    self.filter_state(p)?;
                    p.quality = Some(self.dword()?);
                }
            }
            3 => {
                self.shadow(p)?;
            }
            _ => {}
        }
        Ok(v)
    }
    fn bevel(&mut self, p: &mut TimelineFilterParams) -> Result<u8> {
        let v = self.revision(3, "bevel filter")?;
        match v {
            1 => {
                return stop(
                    "fla_record_revision_unsupported",
                    "Bevel filter revision 1 has no proven layout.",
                );
            }
            2 => {
                p.angle_radians = Some(self.float()?);
                p.color_rgba = Some(self.dword()?);
                p.blur_x = Some(self.float()?);
                p.distance = Some(self.float()?);
                p.inner = Some(self.dword()?);
                p.knockout = Some(self.dword()?);
                self.filter_state(p)?;
                p.quality = Some(self.dword()?);
                p.legacy_byte = Some(self.byte()?);
                p.highlight_color_rgba = Some(self.dword()?);
                p.legacy_word = Some(self.dword()?);
                p.full = Some(self.dword()?);
            }
            3 => {
                self.shadow(p)?;
                p.highlight_color_rgba = Some(self.dword()?);
                p.full = Some(self.dword()?);
            }
            _ => {}
        }
        Ok(v)
    }
    /// Gradient glow body; also the nested gradient block of gradient bevel.
    fn filter_gradient(&mut self, p: &mut TimelineFilterParams) -> Result<u8> {
        let v = self.revision(1, "gradient filter")?;
        p.gradient_revision = Some(v);
        if v == 0 {
            return Ok(v);
        }
        self.shadow(p)?;
        let count = self.dword()?;
        // The native structure has fifteen ratio and fifteen color slots.
        if count > 15 {
            return stop(
                "fla_gradient_stop_limit",
                "Gradient filter exceeds native capacity of 15 stops.",
            );
        }
        p.gradient_value = Some(self.dword()?);
        p.full = Some(self.dword()?);
        if count as usize > (self.data.len() - self.pos) / 8 {
            return stop(
                "fla_truncated_record",
                "Gradient filter stops exceed the remaining input.",
            );
        }
        for _ in 0..count {
            p.gradient_stops.push(TimelineFilterGradientStop {
                ratio: self.dword()?,
                color_rgba: self.dword()?,
            });
        }
        Ok(v)
    }
}
