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
    pub(super) fn color_transform(&mut self) -> Result<TimelineColorTransform> {
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
    fn historical_wide_string(
        &mut self,
        detect_byte_order: bool,
    ) -> Result<(TimelineString, Option<String>)> {
        let start = self.pos;
        let byte_len = self.dword()? as usize;
        if (byte_len & 1) != 0 || byte_len > MAX_STRING {
            return stop(
                "fla_string_limit",
                "Invalid or oversized historical wide string byte count.",
            );
        }
        let data = self.span(byte_len)?;
        let raw = &self.data[data.start..data.end];
        let swapped = detect_byte_order && byte_len >= 2 && raw[0] == 0 && raw[1] != 0;
        let mut units = Vec::with_capacity(byte_len / 2);
        for chunk in raw.as_chunks::<2>().0 {
            let u = if swapped {
                u16::from_be_bytes(*chunk)
            } else {
                u16::from_le_bytes(*chunk)
            };
            if u == 0 {
                break;
            }
            units.push(u);
        }
        let decoded = String::from_utf16(&units).map_err(|_| Stop {
            code: "fla_string_encoding".into(),
            message: "Invalid UTF-16 in historical wide string.".into(),
        })?;
        let trimmed_data = if !swapped {
            TimelineByteSpan {
                start: data.start,
                end: data.start + units.len() * 2,
            }
        } else {
            TimelineByteSpan {
                start: data.start,
                end: data.start,
            }
        };
        Ok((
            TimelineString {
                encoded: TimelineByteSpan {
                    start,
                    end: self.pos,
                },
                data: trimmed_data,
                utf16: true,
            },
            swapped.then_some(decoded),
        ))
    }
    fn timeline_effect(&mut self, symbol_revision: u8) -> Result<TimelineByteSpan> {
        let start = self.pos;
        self.string()?; // effectName
        self.string()?; // groupName
        self.string()?; // symbolType
        if symbol_revision >= 18 {
            self.string()?; // originalLayerName
        }
        if symbol_revision >= 19 {
            self.string()?; // effectId
        }
        self.string()?; // sourceScriptFilename
        let raw_count = self.dword()? as i32;
        let count = raw_count.max(0) as usize;
        if count > MAX_RECORDS || count > (self.data.len() - self.pos) / 5 {
            return stop(
                "fla_record_limit",
                "Timeline-effect parameter count exceeds 50000 or remaining input.",
            );
        }
        for _ in 0..count {
            self.retain_budget()?;
            self.string()?; // name
            let ptype = self.dword()?;
            match ptype {
                0 | 3 | 4 => {
                    self.dword()?;
                }
                1 | 6 => {
                    let _ = self.double()?;
                }
                2 => {
                    self.string()?;
                }
                5 => {
                    let list_raw = self.dword()? as i32;
                    let list_count = list_raw.max(0) as usize;
                    if list_count > MAX_RECORDS || list_count > self.data.len() - self.pos {
                        return stop(
                            "fla_record_limit",
                            "Timeline-effect list parameter exceeds budget.",
                        );
                    }
                    for _ in 0..list_count {
                        self.retain_budget()?;
                        self.string()?;
                    }
                    self.dword()?; // selectedIndex
                }
                _ => {
                    return stop(
                        "fla_timeline_effect_param_unsupported",
                        format!("Timeline-effect parameter type {ptype} is not implemented."),
                    );
                }
            }
            self.dword()?; // minimum
            self.dword()?; // maximum
            self.dword()?; // maximumLength
            self.string()?; // variableName
            self.dword()?; // displayOrder
        }
        Ok(TimelineByteSpan {
            start,
            end: self.pos,
        })
    }
    pub(super) fn accessibility(&mut self, unicode: bool) -> Result<TimelineAccessibility> {
        self.accessibility_inner(unicode, false)
    }
    fn accessibility_inner(
        &mut self,
        unicode: bool,
        historical_wide: bool,
    ) -> Result<TimelineAccessibility> {
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
            if historical_wide {
                let (n_span, n_swapped) = self.historical_wide_string(false)?;
                let (d_span, d_swapped) = self.historical_wide_string(false)?;
                let (s_span, _) = self.historical_wide_string(false)?;
                access.name = Some(n_span);
                access.description = Some(d_span);
                access.shortcut = Some(s_span);
                access.historical_wide_name = n_swapped;
                access.historical_wide_description = d_swapped;
            } else {
                access.name = Some(self.string()?);
                access.description = Some(self.string()?);
                access.shortcut = Some(self.string()?);
            }
            if unicode && revision >= 2 {
                access.tab_index = Some(self.string()?);
                access.reserved = Some(self.string()?);
            }
            access.force_simple = Some(self.dword()?);
            access.auto_labeling = Some(self.dword()?);
        }
        Ok(access)
    }
    pub(super) fn instance(&mut self, id: u32, class: Class) -> Result<()> {
        let mut value = TimelineInstance::default();
        let parsed = (|| {
            let revision = self.revision(23, "CPicSymbol")?;
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
                let properties = self.component_properties()?;
                value.property_count = Some(properties.entries.len() as u32);
                value.properties = Some(properties);
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
                let span = self.timeline_effect(revision)?;
                value.metadata.push(span);
            }
            if revision >= 12 && self.byte()? != 0 {
                let span = self.persistent_data()?;
                value.metadata.push(span);
            }
            if revision >= 14 {
                value.is_extensible = Some(self.byte()?);
            }
            if revision >= 15 {
                let (filters, span, result) = self.filters();
                value.filters = filters;
                value.filters_encoded = Some(span);
                result?;
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
            if revision >= 23 {
                value.bitmap_render_mode = Some(self.word()?);
                value.bitmap_bits_32 = Some(self.dword()?);
                value.bitmap_background_rgba = Some(self.dword()?);
                value.is_visible = Some(self.dword()?);
            }
            match class {
                Class::Sprite | Class::Screen => {
                    let rev = self.revision(8, "CPicSprite")?;
                    value.extension_revision = Some(rev);
                    if rev >= 2 {
                        let (actions, result) = self.actions(19);
                        value.actions = Some(actions);
                        result?;
                    }
                    value.instance_name = Some(self.string()?);
                    if rev >= 3 {
                        let properties = self.component_properties()?;
                        value.property_count = Some(properties.entries.len() as u32);
                        value.metadata.push(properties.encoded);
                        value.properties = Some(properties);
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
                    if class == Class::Screen {
                        let start = self.pos;
                        let srev = self.revision(5, "CPicScreen")?;
                        self.dword()?; // hidden
                        self.dword()?; // reservedScreenValue
                        if srev >= 3 {
                            self.dword()?; // autoRegUpdate
                        }
                        self.string()?; // screenName
                        value.metadata.push(TimelineByteSpan {
                            start,
                            end: self.pos,
                        });
                    }
                }
                Class::Button => {
                    let rev = self.revision(11, "CPicButton")?;
                    value.extension_revision = Some(rev);
                    if rev >= 2 {
                        // Revision 2 holds one historical command, 3-4 a list.
                        let container = match rev {
                            2 => 9,
                            3 | 4 => 18,
                            _ => 19,
                        };
                        let (actions, result) = self.actions(container);
                        value.actions = Some(actions);
                        result?;
                    }
                    if rev >= 4 {
                        value.track_as_menu = Some(self.byte()?);
                    }
                    if rev == 9 || rev == 10 {
                        let (name_span, swapped_name) = self.historical_wide_string(rev == 9)?;
                        value.instance_name = Some(name_span);
                        value.historical_wide_name = swapped_name;
                    } else if rev >= 6 {
                        value.instance_name = Some(self.string()?);
                    }
                    if rev >= 8 {
                        value.accessibility = Some(self.accessibility_inner(rev >= 11, rev == 9)?);
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
    pub(super) fn primitive(&mut self, id: u32, class: Class) -> Result<()> {
        let mut value = TimelinePrimitive::default();
        let parsed = (|| {
            value.revision = Some(self.revision(3, class.name())?);
            value.flags = Some(self.byte()?);
            if class == Class::Rect {
                value.corner_radii = Some(self.signed_words()?);
            } else {
                value.bounds_words = Some(self.signed_words()?);
                value.start_angle_degrees = Some(self.float()?);
                value.end_angle_degrees = Some(self.float()?);
                value.inner_radius_percent = Some(self.float()?);
            }
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::Primitive(Box::new(value)));
        parsed
    }
    /// CPicSwf compiled-clip placement (pinned JPEXS SwfRecordDecoder.java).
    pub(super) fn compiled_clip(&mut self, id: u32) -> Result<()> {
        let mut value = TimelineCompiledClip::default();
        let parsed = (|| {
            let revision = self.revision(7, "CPicSwf")?;
            value.revision = Some(revision);
            value.matrix_words = Some(self.signed_words()?);
            let present = self.dword()?;
            value.media_present = Some(present);
            if present != 0 {
                value.media_id = Some(self.word()?);
            }
            if revision >= 2 {
                let (actions, result) = self.actions(19);
                value.actions = Some(actions);
                result?;
            }
            if revision >= 3 {
                value.instance_name = Some(self.string()?);
            }
            if revision >= 4 {
                let properties = self.component_properties()?;
                value.property_count = Some(properties.entries.len() as u32);
                value.metadata.push(properties.encoded);
                value.properties = Some(properties);
            }
            if revision >= 5 {
                value.accessibility = Some(self.accessibility(true)?);
            }
            if revision >= 6 {
                let start = self.pos;
                self.byte()?;
                self.dword()?;
                value.component_xml = Some(self.string()?);
                value.metadata.push(TimelineByteSpan {
                    start,
                    end: self.pos,
                });
            }
            if revision >= 7 {
                value.reserved_byte = Some(self.byte()?);
            }
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::CompiledClip(Box::new(value)));
        parsed
    }
    /// CPicVideoStream placement (pinned JPEXS DefaultFlaDecoders.java).
    pub(super) fn video(&mut self, id: u32) -> Result<()> {
        let mut value = TimelineVideo::default();
        let parsed = (|| {
            let revision = self.revision(4, "CPicVideoStream")?;
            value.revision = Some(revision);
            value.matrix_words = Some(self.signed_words()?);
            value.bounds_words = Some(self.signed_words()?);
            value.reserved_byte = Some(self.byte()?);
            value.reserved_string = Some(self.string()?);
            value.instance_name = Some(self.string()?);
            let present = if revision >= 3 { self.dword()? } else { 1 };
            if revision >= 3 {
                value.media_present = Some(present);
            }
            if present != 0 {
                value.media_id = Some(self.word()?);
            }
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::Video(Box::new(value)));
        parsed
    }
    /// CPicVideo linked-video placement (pinned JPEXS AuxiliaryObjectRecordDecoder.java).
    pub(super) fn linked_video(&mut self, id: u32) -> Result<()> {
        let mut value = TimelineVideo::default();
        let parsed = (|| {
            value.revision = Some(self.revision(0, "CPicVideo")?);
            value.matrix_words = Some(self.signed_words()?);
            value.media_id = Some(self.word()?);
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::Video(Box::new(value)));
        parsed
    }
    /// CPicASObj / CPicThreeDObject / CPicTLFText auxiliary stage objects.
    pub(super) fn as_or_tlf_object(&mut self, id: u32, class: Class) -> Result<()> {
        let mut aux = TimelineAuxiliaryObject::default();
        let parsed = (|| {
            let rev = self.revision(5, class.name())?;
            aux.revision = Some(rev);
            aux.matrix_words = Some(self.signed_words()?);
            aux.bounds_words = Some(self.signed_words()?);
            aux.class_name = Some(self.string()?);
            aux.source = Some(self.string()?);
            if rev >= 2 {
                let (_filters, _span, result) = self.filters();
                result?;
                self.word()?; // selectedEffectIndex
                for _ in 0..16 {
                    let _ = self.float()?;
                }
                for _ in 0..3 {
                    let _ = self.double()?;
                }
                let _ = self.signed_words::<3>()?;
            }
            if rev >= 3 {
                self.byte()?; // blendMode
            }
            if rev >= 4 {
                let _ = self.color_transform()?;
            }
            if rev >= 5 {
                let raw_count = self.dword()? as i32;
                let count = raw_count.max(0) as usize;
                if count > MAX_RECORDS || count > (self.data.len() - self.pos) / 5 {
                    return stop(
                        "fla_record_limit",
                        "AS-object instance relation count exceeds budget.",
                    );
                }
                for _ in 0..count {
                    self.retain_budget()?;
                    self.dword()?;
                    let mut terminated = false;
                    for _ in 0..40 {
                        if self.byte()? == 0 {
                            terminated = true;
                            break;
                        }
                    }
                    if !terminated {
                        return stop(
                            "fla_identifier_limit",
                            "AS-object relation identifier exceeds 39 bytes.",
                        );
                    }
                }
            }
            if class == Class::TlfText {
                self.byte()?; // tlfRevision
                self.string()?; // localizationStringId
                self.string()?; // variableName
                let children = self.objects[self.object_index(id)].children.clone();
                let mut linked_children = 0usize;
                for child_id in children {
                    let child_obj = &self.objects[self.object_index(child_id)];
                    if let Some(span) = child_obj.base.as_ref().and_then(|b| b.identifier)
                        && span.end > span.start + 1
                    {
                        linked_children += 1;
                    }
                }
                for _ in 0..linked_children {
                    self.dword()?; // markupChildId
                }
            }
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::Auxiliary(Box::new(aux)));
        parsed
    }
    fn inline_library_metadata(&mut self) -> Result<()> {
        let v = self.dword()?;
        if v > 7 {
            return stop(
                "fla_record_revision_unsupported",
                "Inline library metadata revision exceeds 7.",
            );
        }
        if v <= 3 {
            self.dword()?;
        } else if self.dword()? != 0 {
            self.dword()?;
            self.dword()?;
        }
        if v > 1 {
            if v <= 3 {
                self.dword()?;
            } else if self.dword()? != 0 {
                self.dword()?;
                self.dword()?;
            }
        }
        if v > 2 {
            self.dword()?;
        }
        if v >= 5 {
            let lrev = self.revision(7, "inline linkage")?;
            self.dword()?;
            self.string()?;
            self.string()?;
            if lrev >= 5 {
                self.string()?;
            }
            if lrev >= 1 {
                self.byte()?;
            }
            if lrev >= 2 {
                let aux = self.dword()?;
                if aux > 3 {
                    return stop(
                        "fla_record_revision_unsupported",
                        "Inline linkage auxiliary revision exceeds 3.",
                    );
                }
                self.string()?;
                self.string()?;
                self.dword()?;
                if aux >= 1 {
                    self.dword()?;
                }
                if self.dword()? != 0 {
                    self.dword()?;
                    self.dword()?;
                }
            }
            if lrev >= 6 {
                self.byte()?;
            }
            if lrev >= 7 {
                self.string()?;
            }
        }
        if v >= 6 && self.byte()? != 0 {
            let _ = self.persistent_data()?;
        }
        Ok(())
    }
    pub(super) fn font_object(&mut self, id: u32) -> Result<()> {
        let mut aux = TimelineAuxiliaryObject::default();
        let parsed = (|| {
            let mode = self.byte()?;
            aux.revision = Some(mode);
            if mode != 0 {
                self.word()?;
            } else {
                let rev = self.revision(3, "CPicFont")?;
                self.string()?;
                self.word()?;
                self.dword()?;
                let _ = self.text_style()?;
                self.inline_library_metadata()?;
                if rev >= 3 {
                    self.dword()?;
                }
            }
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::Auxiliary(Box::new(aux)));
        parsed
    }
    pub(super) fn clipboard_object(&mut self, id: u32, depth: usize, class: Class) -> Result<()> {
        let mut aux = TimelineAuxiliaryObject::default();
        let parsed = (|| {
            aux.revision = Some(self.byte()?);
            aux.matrix_words = Some(self.signed_words()?);
            if class == Class::Ole {
                aux.bounds_words = Some(self.signed_words()?);
                let item_id = self.word()?;
                aux.ole_item_id = Some(item_id);
                if item_id == 0 {
                    self.typed_object(depth + 1, id, TimelineRelation::OleItem, Class::OleClient)?;
                }
            }
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::Auxiliary(Box::new(aux)));
        parsed
    }
}
