//! Classic text/run layout facts: pinned JPEXS TextRecordDecoder.java at
//! 0923d70870d11b236755af71337da8d0110a57aa. Strings retain exact archive spans;
//! font availability has no bearing on whether a record is readable.
use super::*;

impl Reader<'_> {
    fn raw_text_string(
        &mut self,
        count: usize,
        utf16: bool,
        start: usize,
    ) -> Result<TimelineString> {
        let width = if utf16 { 2 } else { 1 };
        if count > MAX_STRING / width {
            return stop("fla_string_limit", "Text string exceeds 1 MiB.");
        }
        let data = self.span(count * width)?;
        if utf16
            && char::decode_utf16(
                self.data[data.start..data.end]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|v| u16::from_le_bytes(*v)),
            )
            .any(|v| v.is_err())
        {
            return stop("fla_string_encoding", "Invalid UTF-16 text run.");
        }
        Ok(TimelineString {
            encoded: TimelineByteSpan {
                start,
                end: self.pos,
            },
            data,
            utf16,
        })
    }
    fn style_string(&mut self, revision: u8) -> Result<TimelineString> {
        if revision >= 15 {
            return self.string();
        }
        let start = self.pos;
        let count = self.byte()? as usize;
        let mut string = self.raw_text_string(count, revision >= 10, start)?;
        if !string.utf16
            && string.data.end > string.data.start
            && self.data[string.data.end - 1] == 0
        {
            string.data.end -= 1;
        }
        let raw = &self.data[string.data.start..string.data.end];
        if (string.utf16 && raw.as_chunks::<2>().0.contains(&[0, 0]))
            || (!string.utf16 && raw.contains(&0))
        {
            return stop(
                "fla_string_encoding",
                "Embedded NUL in a compact text-style string.",
            );
        }
        Ok(string)
    }
    fn text_style(&mut self) -> Result<TimelineTextStyle> {
        let revision = self.revision(15, "text style")?;
        let mut style = TimelineTextStyle {
            revision,
            ..Default::default()
        };
        if revision < 2 {
            return Ok(style);
        }
        style.font_size_twips = Some(self.word()?);
        style.font_family = Some(self.style_string(revision)?);
        if revision >= 14 {
            style.font_face = Some(self.style_string(revision)?);
            style.font_flags = Some(self.dword()?);
        }
        style.color_rgba = Some(self.dword()?);
        style.pitch_and_family = Some(self.byte()?);
        style.char_set = Some(self.byte()?);
        style.bold = Some(self.byte()?);
        if revision == 2 {
            let flags = self.byte()?;
            style.italic = Some(flags & 1);
            style.use_screen_spacing = Some((flags >> 1) & 1);
            style.auto_kern = Some(u8::from(flags & 4 == 0));
            style.character_position = Some((flags >> 3) & 3);
        } else {
            style.italic = Some(self.byte()?);
            style.use_screen_spacing = Some(self.byte()?);
            style.auto_kern = Some(self.byte()?);
            style.character_position = Some(self.byte()?);
        }
        style.alignment = Some(self.byte()?);
        style.line_spacing = Some(self.word()? as i16);
        style.indent = Some(self.word()? as i16);
        style.left_margin = Some(self.word()? as i16);
        style.right_margin = Some(self.word()? as i16);
        style.letter_spacing = Some(self.word()? as i16);
        if revision >= 5 {
            style.url = Some(self.style_string(revision)?);
        }
        if revision >= 6 {
            style.vertical = Some(self.byte()?);
            style.right_to_left = Some(self.byte()?);
            style.rotation = Some(self.byte()?);
        }
        if revision >= 8 {
            style.bitmap_rendering = Some(self.byte()?);
        }
        if revision >= 7 {
            style.target = Some(self.style_string(revision)?);
        }
        if revision >= 11 {
            let start = self.pos;
            let antialias_revision = self.revision(2, "text antialias")?;
            self.byte()?;
            if antialias_revision == 1 {
                self.signed_words::<3>()?;
            }
            self.float()?;
            self.float()?;
            style.anti_alias = Some(TimelineByteSpan {
                start,
                end: self.pos,
            });
        }
        if revision >= 12 {
            style.url_override = Some(self.style_string(revision)?);
        }
        if revision == 13 {
            style.legacy_font_name = Some(self.style_string(revision)?);
            style.legacy_font_value = Some(self.dword()?);
        }
        Ok(style)
    }
    pub(super) fn text(&mut self, id: u32) -> Result<()> {
        let mut text = TimelineText::default();
        let parsed = (|| {
            let revision = self.revision(14, "CPicText")?;
            text.revision = Some(revision);
            text.matrix_words = Some(self.signed_words()?);
            text.bounds_words = Some(self.signed_words()?);
            text.auto_expand = Some(self.byte()?);
            if revision > 2 {
                text.reserved = Some(self.byte()?);
            }
            let flags = if revision >= 4 {
                let flags = if revision == 4 {
                    u32::from(self.word()?)
                } else {
                    self.dword()?
                };
                text.flags = Some(flags);
                text.max_characters = Some(self.word()?);
                text.variable_name = Some(self.string()?);
                if flags & 0x2000 != 0 {
                    text.font_library_name = Some(self.string()?);
                }
                flags
            } else {
                0
            };
            if revision >= 14 {
                text.embed_variant_glyphs = Some(self.byte()?);
            }
            if flags & 0x4000 != 0 {
                text.default_style = Some(self.text_style()?);
            }
            loop {
                let count = self.word()? as usize;
                if count == 0 {
                    break;
                }
                if count > 514 || text.runs.len() >= MAX_RECORDS {
                    return stop(
                        "fla_text_run_limit",
                        "Text run/count exceeds archive limits.",
                    );
                }
                let style = self.text_style()?;
                let start = self.pos;
                let characters = self.raw_text_string(count, style.revision >= 10, start)?;
                self.retain_budget()?;
                text.runs.push(TimelineTextRun { characters, style });
            }
            if revision >= 6 {
                text.instance_name = Some(self.string()?);
            }
            if revision >= 9 {
                text.accessibility = Some(self.accessibility(revision >= 10)?);
            }
            if revision >= 8 {
                text.scrollable = Some(self.dword()?);
            }
            if revision >= 11 {
                text.localization_id = Some(self.string()?);
            }
            if revision >= 12 {
                text.embed_ranges = Some(self.string()?);
            }
            if revision >= 13 {
                let (filters, span, result) = self.filters();
                text.filters = filters;
                text.filters_encoded = Some(span);
                result?;
                text.selected_effect_index = Some(self.word()?);
            }
            Ok(())
        })();
        let index = self.object_index(id);
        self.objects[index].payload = Some(TimelinePayload::Text(Box::new(text)));
        parsed
    }
}
