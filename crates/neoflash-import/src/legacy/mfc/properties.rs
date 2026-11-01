//! Component instance parameter lists. Versioned layout facts follow the
//! pinned JPEXS reader (0923d70870d11b236755af71337da8d0110a57aa,
//! PropertyRecordDecoder.java); this is an independent bounded decoder.
//! Values are retained as archive strings; no component semantics are inferred.
use super::*;

impl Reader<'_> {
    fn property_count(&mut self, minimum_bytes: usize) -> Result<usize> {
        let count = self.dword()? as usize;
        if count > MAX_RECORDS || count > (self.data.len() - self.pos) / minimum_bytes.max(1) {
            return stop(
                "fla_record_limit",
                "Component property collection exceeds 50000 entries or the remaining input.",
            );
        }
        Ok(count)
    }
    pub(super) fn component_properties(&mut self) -> Result<TimelineComponentProperties> {
        let start = self.pos;
        let revision = self.revision(2, "instance properties")?;
        let count = self.property_count(2)?;
        let mut list = TimelineComponentProperties {
            encoded: TimelineByteSpan { start, end: start },
            revision,
            entries: Vec::with_capacity(count),
            parameters_are_new: None,
        };
        for _ in 0..count {
            self.retain_budget()?;
            let entry_start = self.pos;
            let mut property = TimelineComponentProperty::default();
            if revision == 0 {
                property.legacy_type = Some(self.dword()?);
                property.name = Some(self.string()?);
                property.legacy_value = Some(self.string()?);
            } else {
                let v = self.revision(3, "component property")?;
                property.revision = Some(v);
                property.name = Some(self.string()?);
                property.value = Some(self.property_value()?);
                if v >= 1 {
                    property.variable_name = Some(self.string()?);
                    property.identifier = Some(self.span(17)?);
                }
                if v >= 3 {
                    property.category = Some(self.string()?);
                    property.verbosity = Some(self.dword()?);
                }
            }
            property.encoded = TimelineByteSpan {
                start: entry_start,
                end: self.pos,
            };
            list.entries.push(property);
        }
        if revision >= 2 {
            list.parameters_are_new = Some(self.dword()?);
        }
        list.encoded.end = self.pos;
        Ok(list)
    }
    fn property_value(&mut self) -> Result<TimelinePropertyValue> {
        let start = self.pos;
        let revision = self.revision(8, "component property value")?;
        let value_type = self.word()?;
        let mut value = TimelinePropertyValue {
            revision,
            value_type,
            ..Default::default()
        };
        if revision >= 8 && value_type == 17 {
            value.selection_word = Some(self.word()?);
        }
        let count = self.property_count(1)?;
        value.selected_index = Some(self.dword()?);
        let named = if revision >= 1 {
            let named = self.dword()?;
            value.named_entries = Some(named);
            named != 0
        } else {
            false
        };
        value.entries.reserve(count);
        for _ in 0..count {
            self.retain_budget()?;
            value.entries.push(TimelinePropertyValueEntry {
                name: if named { Some(self.string()?) } else { None },
                value: self.string()?,
                value_type: if revision >= 6 {
                    Some(self.word()?)
                } else {
                    None
                },
            });
        }
        if value_type == 13 && revision >= 4 {
            let table_start = self.pos;
            self.string()?;
            self.string()?;
            if revision >= 5 {
                self.string()?;
            }
            let columns = self.property_count(4)?;
            for _ in 0..columns {
                self.retain_budget()?;
                self.string()?;
                self.word()?;
                self.string()?;
                self.string()?;
            }
            let rows = self.property_count(1)?;
            if rows
                .checked_mul(columns)
                .is_none_or(|cells| cells > MAX_RECORDS)
            {
                return stop(
                    "fla_record_limit",
                    "Component property table exceeds 50000 cells.",
                );
            }
            for _ in 0..rows {
                self.retain_budget()?;
                for _ in 0..columns {
                    self.string()?;
                }
            }
            value.table = Some(TimelineByteSpan {
                start: table_start,
                end: self.pos,
            });
        }
        if revision >= 8 {
            let length = self.dword()? as usize;
            value.data = Some(self.span(length)?);
        }
        value.encoded = TimelineByteSpan {
            start,
            end: self.pos,
        };
        Ok(value)
    }
}
