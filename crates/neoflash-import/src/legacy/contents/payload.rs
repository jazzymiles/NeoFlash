//! Versioned descriptor payload boundaries. All nested bytes remain in the
//! enclosing descriptor span; interpreting their authoring semantics is separate.
use super::{
    ContentsByteSpan, ContentsDescriptor, ContentsDescriptorClass as Class, Reader, Result,
};

impl Reader<'_> {
    pub(super) fn descriptor(
        &mut self,
        object_id: u32,
        class: Class,
        schema: u16,
    ) -> Result<ContentsDescriptor> {
        let page = class == Class::DocumentPage;
        let revision = self.revision(if page { 25 } else { 7 })?;
        let stream_name =
            self.stream_name(revision >= if page { 21 } else { 6 }, page && revision < 21)?;
        let name = self.string()?;
        let mut result = ContentsDescriptor {
            object_id,
            class,
            schema,
            revision,
            subtype_revision: None,
            encoded: ContentsByteSpan::default(),
            stream_name,
            stream_index: None,
            name,
            library_id: None,
            symbol_type: None,
            media_id: None,
        };
        if page {
            if revision >= 2 {
                result.library_id = Some(self.u32()?);
            }
            if revision >= 3 {
                result.symbol_type = Some(self.u8()?);
            }
            if revision >= 6 {
                self.string()?;
            }
            if revision >= 7 {
                self.take(4)?;
            }
            if revision >= 5 {
                self.library_metadata()?;
            }
            if revision >= 8 {
                self.take(4)?;
            }
            if revision == 9 {
                return self.unsupported("Historical document-page action lists are unsupported.");
            }
            if revision >= 10 {
                self.string()?;
                self.string()?;
                self.properties()?;
            }
            if revision >= 11 {
                self.take(4)?;
            }
            if revision >= 12 {
                self.linkage()?;
            }
            if (14..=17).contains(&revision) {
                self.string()?;
            }
            if (15..=17).contains(&revision) {
                self.smart_movie()?;
                self.smart_movie()?;
            }
            if (16..=17).contains(&revision) {
                self.take(4)?;
            }
            if revision == 17 {
                self.take(4)?;
                self.string()?;
                self.take(4)?;
            }
            if revision >= 18 {
                self.component()?;
            }
            if revision >= 20 {
                self.string()?;
                self.string()?;
            }
            if revision >= 22 {
                self.take(4)?;
                self.string()?;
            }
            if revision >= 23 {
                self.take(20)?;
            }
            if revision >= 24 {
                self.take(9)?;
            }
            if revision >= 25 {
                self.take(2)?;
            }
        } else {
            result.media_id = Some(self.u16()?);
            if revision >= 2 {
                self.string()?;
            }
            if revision >= 3 {
                self.take(4)?;
            }
            if revision >= 4 {
                self.library_metadata()?;
            }
            if revision >= 5 {
                self.take(4)?;
            }
            match class {
                Class::Bitmap => {
                    let v = self.revision(4)?;
                    result.subtype_revision = Some(v);
                    if v < 2 {
                        self.string()?;
                    }
                    if v >= 1 {
                        self.take(2)?;
                    }
                    if v >= 2 {
                        self.take(1)?;
                    }
                    if v >= 3 {
                        self.take(4)?;
                    }
                    if v >= 4 {
                        self.take(1)?;
                    }
                }
                Class::Sound => {
                    let v = self.revision(10)?;
                    result.subtype_revision = Some(v);
                    if v == 2 {
                        self.string()?;
                    }
                    self.take(6)?;
                    if v >= 4 {
                        self.take(4)?;
                    }
                    if v >= 6 {
                        self.take(5)?;
                    }
                    if v >= 7 {
                        self.take(6)?;
                    }
                    if v >= 9 {
                        self.string()?;
                    }
                    if v >= 10 {
                        self.take(4)?;
                    }
                }
                Class::Media | Class::Video => {}
                Class::DocumentPage => unreachable!(),
            }
        }
        Ok(result)
    }
    fn item_id(&mut self) -> Result<()> {
        if self.u32()? != 0 {
            self.take(8)?;
        }
        Ok(())
    }
    fn library_metadata(&mut self) -> Result<()> {
        let v = self.u32()?;
        if v > 7 {
            return self.unsupported("Library metadata revision is unsupported.");
        }
        if v <= 3 {
            self.take(4)?;
        } else {
            self.item_id()?;
        }
        if v > 1 {
            if v <= 3 {
                self.take(4)?;
            } else {
                self.item_id()?;
            }
        }
        if v > 2 {
            self.take(4)?;
        }
        if v >= 5 {
            self.linkage()?;
        }
        if v >= 6 && self.u8()? != 0 {
            self.persistent_data()?;
        }
        Ok(())
    }
    fn linkage(&mut self) -> Result<()> {
        let v = self.revision(7)?;
        self.take(4)?;
        self.string()?;
        self.string()?;
        if v >= 5 {
            self.string()?;
        }
        if v >= 1 {
            self.take(1)?;
        }
        if v >= 2 {
            let aux = self.u32()?;
            if aux > 3 {
                return self.unsupported("Linkage auxiliary revision is unsupported.");
            }
            self.string()?;
            self.string()?;
            self.take(4)?;
            if aux >= 1 {
                self.take(4)?;
            }
            self.item_id()?;
        }
        if v >= 6 {
            self.take(1)?;
        }
        if v >= 7 {
            self.string()?;
        }
        Ok(())
    }
    fn smart_movie(&mut self) -> Result<()> {
        let v = self.revision(3)?;
        if v >= 1 {
            self.string()?;
            let bytes = self.u32()? as usize;
            self.take(bytes)?;
        }
        if v >= 2 {
            self.take(4)?;
        }
        Ok(())
    }
    fn component(&mut self) -> Result<()> {
        let v = self.revision(8)?;
        self.take(4)?;
        self.string()?;
        self.take(12)?;
        self.string()?;
        for _ in 0..3 {
            self.smart_movie()?;
        }
        if v >= 1 {
            self.string()?;
            self.properties()?;
        }
        if v >= 2 {
            self.string()?;
        }
        if v >= 4 {
            // Nonpositive task/persistent counts have empty native iteration.
            let count = self.i32()?.max(0) as usize;
            self.count_value(count, 2)?;
            for _ in 0..count {
                self.string()?;
                self.string()?;
            }
        }
        if v >= 5 && self.u32()? != 0 {
            return self.unsupported("Embedded component bitmap payload is unsupported.");
        }
        if v >= 6 {
            self.take(4)?;
        }
        if v >= 7 {
            self.take(4)?;
            self.string()?;
            self.take(8)?;
        }
        if v >= 8 {
            self.smart_movie()?;
        }
        Ok(())
    }
    fn persistent_data(&mut self) -> Result<()> {
        self.take(4)?;
        let count = self.i32()?.max(0) as usize;
        self.count_value(count, 1)?;
        for _ in 0..count {
            let kind = self.u8()?;
            if !matches!(kind, 0 | 1 | 3 | 4 | 5 | 7) {
                return self.unsupported("Persistent-data value type is unsupported.");
            }
            self.string()?;
            match kind {
                0 => {
                    self.take(4)?;
                }
                3 => {
                    self.take(8)?;
                }
                5 => {
                    self.string()?;
                }
                _ => {
                    let width = match kind {
                        1 => 4,
                        4 => 8,
                        _ => 1,
                    };
                    let count = self.count(width)?;
                    self.take(count * width)?;
                }
            }
        }
        Ok(())
    }
    fn properties(&mut self) -> Result<()> {
        let list_revision = self.revision(2)?;
        let count = self.count(1)?;
        for _ in 0..count {
            if list_revision == 0 {
                self.take(4)?;
                self.string()?;
                self.string()?;
                continue;
            }
            let v = self.revision(3)?;
            self.string()?;
            self.property_value()?;
            if v >= 1 {
                self.string()?;
                self.take(17)?;
            }
            if v >= 3 {
                self.string()?;
                self.take(4)?;
            }
        }
        if list_revision >= 2 {
            self.take(4)?;
        }
        Ok(())
    }
    fn property_value(&mut self) -> Result<()> {
        let v = self.revision(8)?;
        let kind = self.u16()?;
        if v >= 8 && kind == 17 {
            self.take(2)?;
        }
        let count = self.count(1)?;
        self.take(4)?;
        let named = v >= 1 && self.u32()? != 0;
        for _ in 0..count {
            if named {
                self.string()?;
            }
            self.string()?;
            if v >= 6 {
                self.take(2)?;
            }
        }
        if kind == 13 && v >= 4 {
            self.string()?;
            self.string()?;
            if v >= 5 {
                self.string()?;
            }
            let columns = self.count(5)?;
            for _ in 0..columns {
                self.string()?;
                self.take(2)?;
                self.string()?;
                self.string()?;
            }
            let rows = self.count(0)?;
            let cells = rows.checked_mul(columns).ok_or_else(|| {
                self.error("fla_contents_budget", "Property table dimensions overflow.")
            })?;
            self.count_value(cells, 1)?;
            for _ in 0..cells {
                self.string()?;
            }
        }
        if v >= 8 {
            let length = self.u32()? as usize;
            self.take(length)?;
        }
        Ok(())
    }
}
