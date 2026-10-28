//! Versioned placement field layouts are documented by the pinned JPEXS reader
//! (0923d70870d11b236755af71337da8d0110a57aa, InstanceRecordDecoder.java).
//! This bounded reader retains archive values; interpreting them is a separate step.
use super::*;

impl Reader<'_> {
    fn color_words(&mut self, alpha: bool) -> Result<TimelineColorTransform> {
        let mut color = TimelineColorTransform {
            multipliers: [256; 4],
            ..Default::default()
        };
        if alpha {
            color.multipliers[3] = self.word()? as i16;
            color.offsets[3] = self.word()? as i16;
        }
        for i in 0..3 {
            color.multipliers[i] = self.word()? as i16;
            color.offsets[i] = self.word()? as i16;
        }
        Ok(color)
    }
    fn color_transform(&mut self) -> Result<TimelineColorTransform> {
        let revision = self.byte()?;
        if revision != 1 {
            return stop(
                "fla_color_revision_unsupported",
                "Unknown color transform revision.",
            );
        }
        let mut color = self.color_words(true)?;
        color.revision = Some(revision);
        color.effect_type = Some(self.word()?);
        color.effect_percent = Some(self.word()? as i16);
        color.effect_rgba = Some(self.dword()?);
        Ok(color)
    }
    fn empty_properties(&mut self) -> Result<(u32, TimelineByteSpan)> {
        let start = self.pos;
        let revision = self.revision(2, "instance properties")?;
        let count = self.dword()?;
        if count != 0 {
            return stop(
                "fla_instance_properties_unsupported",
                "Nonempty component instance properties require semantic preservation.",
            );
        }
        if revision >= 2 {
            self.dword()?;
        }
        Ok((
            count,
            TimelineByteSpan {
                start,
                end: self.pos,
            },
        ))
    }
    pub(super) fn accessibility(&mut self, unicode: bool) -> Result<TimelineAccessibility> {
        let revision = self.dword()?;
        if revision > 2 {
            return stop(
                "fla_accessibility_revision_unsupported",
                "Unknown accessibility revision.",
            );
        }
        let mut access = TimelineAccessibility {
            revision,
            ..Default::default()
        };
        if revision != 0 {
            access.flags = Some(self.dword()?);
            access.name = Some(self.string()?);
            access.description = Some(self.string()?);
            access.shortcut = Some(self.string()?);
            if unicode && revision >= 2 {
                access.tab_index = Some(self.string()?);
                access.reserved = Some(self.string()?);
            }
            access.force_simple = Some(self.dword()?);
            access.auto_labeling = Some(self.dword()?);
        }
        Ok(access)
    }
    pub(super) fn absent_filters(&mut self) -> Result<()> {
        if self.byte()? != 0 {
            return stop(
                "fla_filters_unsupported",
                "Authored filter records require a dedicated decoder.",
            );
        }
        Ok(())
    }
    pub(super) fn instance(&mut self, id: u32, class: Class) -> Result<()> {
        let mut value = TimelineInstance::default();
        let parsed = (|| {
            let revision = self.revision(22, "CPicSymbol")?;
            value.revision = Some(revision);
            value.matrix_words = Some(self.signed_words()?);
            value.first_frame = Some(self.word()?);
            if revision >= 2 {
                value.loop_mode = Some(self.word()?);
            }
            if revision >= 7 {
                value.color_transform = Some(self.color_transform()?);
            } else {
                if revision >= 3 {
                    value.color_transform = Some(self.color_words(revision >= 5)?);
                }
                if revision >= 4 {
                    let color =
                        value
                            .color_transform
                            .get_or_insert_with(|| TimelineColorTransform {
                                multipliers: [256; 4],
                                ..Default::default()
                            });
                    color.effect_type = Some(self.word()?);
                    color.effect_percent = Some(self.word()? as i16);
                    color.effect_rgba = Some(self.dword()?);
                }
            }
            if revision >= 6 {
                value.legacy_name = Some(self.string()?);
            }
            value.library_id = Some(self.dword()?);
            if revision == 8 || revision == 9 {
                let start = self.pos;
                self.string()?;
                self.string()?;
                let (count, _) = self.empty_properties()?;
                value.property_count = Some(count);
                self.dword()?;
                if revision == 9 {
                    self.string()?;
                }
                value.metadata.push(TimelineByteSpan {
                    start,
                    end: self.pos,
                });
            }
            if revision >= 11 && self.byte()? != 0 {
                return stop(
                    "fla_timeline_effect_unsupported",
                    "Timeline effects require a dedicated decoder.",
                );
            }
            if revision >= 12 && self.byte()? != 0 {
                return stop(
                    "fla_instance_persistent_data_unsupported",
                    "Instance persistent data requires a dedicated decoder.",
                );
            }
            if revision >= 14 {
                value.is_extensible = Some(self.byte()?);
            }
            if revision >= 15 {
                self.absent_filters()?;
            }
            if revision >= 16 {
                value.blend_mode = Some(self.byte()?);
            }
            if revision >= 17 {
                value.selected_effect_index = Some(self.word()?);
            }
            if revision >= 20 {
                let mut matrix = [0.; 16];
                for n in &mut matrix {
                    *n = self.float()?;
                }
                value.matrix_3d = Some(matrix);
                let mut rotations = [0.; 3];
                for n in &mut rotations {
                    *n = self.double()?;
                }
                value.rotation_3d = Some(rotations);
            }
            if revision >= 21 {
                value.center_point_3d = Some(self.signed_words()?);
            }
            if revision >= 22 {
                let count = self.word()? as usize;
                if count > MAX_RECORDS || count > (self.data.len() - self.pos) / 4 {
                    return stop(
                        "fla_record_limit",
                        "Animation settings exceed input or item budget.",
                    );
                }
                for _ in 0..count {
                    value
                        .animation_frame_settings
                        .push([self.word()?, self.word()?]);
                }
            }
            match class {
                Class::Sprite => {
                    let rev = self.revision(8, "CPicSprite")?;
                    value.extension_revision = Some(rev);
                    if rev >= 2 {
                        let (actions, result) = self.actions(19);
                        value.actions = Some(actions);
                        result?;
                    }
                    value.instance_name = Some(self.string()?);
                    if rev >= 3 {
                        let (count, span) = self.empty_properties()?;
                        value.property_count = Some(count);
                        value.metadata.push(span);
                    }
                    if rev >= 6 {
                        value.accessibility = Some(self.accessibility(rev >= 7)?);
                    }
                    if rev >= 5 {
                        value.property_inspector_state = Some(self.dword()?);
                    }
                    if rev >= 8 {
                        let start = self.pos;
                        self.byte()?;
                        self.dword()?;
                        value.component_xml = Some(self.string()?);
                        value.metadata.push(TimelineByteSpan {
                            start,
                            end: self.pos,
                        });
                    }
                }
                Class::Button => {
                    let rev = self.revision(11, "CPicButton")?;
                    value.extension_revision = Some(rev);
                    if (2..=4).contains(&rev) {
                        return stop(
                            "fla_button_actions_unsupported",
                            "Historical button commands require a dedicated decoder.",
                        );
                    }
                    if rev >= 5 {
                        let (actions, result) = self.actions(19);
                        value.actions = Some(actions);
                        result?;
                    }
                    if rev >= 4 {
                        value.track_as_menu = Some(self.byte()?);
                    }
                    if rev == 9 || rev == 10 {
                        return stop(
                            "fla_button_name_unsupported",
                            "Historical wide button names require a dedicated decoder.",
                        );
                    } else if rev >= 6 {
                        value.instance_name = Some(self.string()?);
                    }
                    if rev >= 8 {
                        value.accessibility = Some(self.accessibility(rev >= 11)?);
                    }
                }
                _ => {}
            }
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::Instance(Box::new(value)));
        parsed
    }
}
