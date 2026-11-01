//! Versioned descriptor payload boundaries. All nested bytes remain in the
//! enclosing descriptor span; interpreting their authoring semantics is separate.
use super::{
    ContentsBitmapSettings, ContentsByteSpan, ContentsDescriptor, ContentsDescriptorClass as Class,
    ContentsLibraryMetadata, ContentsLinkage, ContentsSoundSettings, Reader, Result,
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
            library_metadata: None,
            linkage: None,
            bitmap: None,
            sound: None,
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
                result.library_metadata = Some(self.library_metadata()?);
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
                result.linkage = Some(self.linkage()?);
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
                result.library_metadata = Some(self.library_metadata()?);
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
                    let mut bitmap = ContentsBitmapSettings::default();
                    if v >= 1 {
                        bitmap.compression = Some(self.u8()?);
                        bitmap.quality = Some(self.u8()?);
                    }
                    if v >= 2 {
                        bitmap.smoothing = self.u8()? != 0;
                    }
                    if v >= 3 {
                        bitmap.encoded_byte_count = self.i32()?;
                    }
                    if v >= 4 {
                        bitmap.deblocking = self.u8()? != 0;
                    }
                    result.bitmap = Some(bitmap);
                }
                Class::Sound => {
                    let v = self.revision(10)?;
                    result.subtype_revision = Some(v);
                    if v == 2 {
                        self.string()?;
                    }
                    let mut sound = ContentsSoundSettings {
                        format_flags: self.u16()?,
                        sample_count: self.u32()?,
                        ..Default::default()
                    };
                    if v >= 4 {
                        sound.export_format = Some(self.i16()?);
                        sound.export_bits = Some(self.i16()?);
                    }
                    if v >= 6 {
                        sound.encoded_byte_count = Some(self.u32()?);
                        sound.imported_mp3_quality = Some(self.u8()?);
                    }
                    if v >= 7 {
                        sound.secondary_format_flags = Some(self.u16()?);
                        sound.secondary_sample_count = Some(self.u32()?);
                    }
                    if v >= 9 {
                        self.string()?;
                    }
                    if v >= 10 {
                        sound.use_8k_sample_rate = Some(self.u32()?);
                    }
                    result.sound = Some(sound);
                }
                Class::SwfPage => {
                    // Compiled-clip media: component envelope, then its own
                    // revision. The SWF payload stream itself is not read.
                    self.component()?;
                    let v = self.revision(5)?;
                    result.subtype_revision = Some(v);
                    if v >= 1 {
                        self.string()?; // source name
                    }
                    if v >= 2 {
                        self.take(16)?; // bounds words
                    }
                    if v >= 3 {
                        self.swf_metadata(0)?;
                    }
                    if v >= 4 {
                        self.take(12)?; // legacy word, player and AS versions
                    }
                    if v >= 5 {
                        if self.u32()? != 0 {
                            self.shared_library()?;
                        } else {
                            self.string()?; // export name
                        }
                    }
                }
                Class::Media | Class::Video => {}
                Class::DocumentPage
                | Class::Color
                | Class::ColorList
                | Class::QuickTimeAudioSettings => unreachable!(),
            }
        }
        Ok(result)
    }
    pub(super) fn item_id(&mut self) -> Result<Option<[u32; 2]>> {
        if self.u32()? != 0 {
            Ok(Some([self.u32()?, self.u32()?]))
        } else {
            Ok(None)
        }
    }
    pub(super) fn library_metadata(&mut self) -> Result<ContentsLibraryMetadata> {
        let v = self.u32()?;
        if v > 7 {
            return self.unsupported("Library metadata revision is unsupported.");
        }
        let mut result = ContentsLibraryMetadata {
            parent: if v <= 3 {
                let id = self.u32()?;
                (id != 0).then_some([0, id])
            } else {
                self.item_id()?
            },
            ..Default::default()
        };
        if v > 1 {
            result.item = if v <= 3 {
                let id = self.u32()?;
                (id != 0).then_some([0, id])
            } else {
                self.item_id()?
            };
        }
        if v > 2 {
            result.attributes = Some(self.u32()?);
        }
        if v >= 5 {
            result.linkage = Some(self.linkage()?);
        }
        if v >= 6 && self.u8()? != 0 {
            self.persistent_data()?;
        }
        Ok(result)
    }
    fn linkage(&mut self) -> Result<ContentsLinkage> {
        let v = self.revision(7)?;
        let flags = self.u32()?;
        let identifier = self.string()?;
        let url = self.string()?;
        let class_name = if v >= 5 { Some(self.string()?) } else { None };
        let export_flags = if v >= 1 { Some(self.u8()?) } else { None };
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
        let base_class = if v >= 7 { Some(self.string()?) } else { None };
        Ok(ContentsLinkage {
            flags,
            identifier,
            url,
            class_name,
            export_flags,
            base_class,
        })
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
    /// A component icon: an inline bitmap media record (no MFC envelope)
    /// followed by its payload. Only the boundary is read; pixels stay in the
    /// enclosing descriptor span and are neither inflated nor decoded.
    fn embedded_bitmap(&mut self) -> Result<()> {
        let media = self.descriptor(0, Class::Bitmap, 1)?;
        let encoded = media.bitmap.map_or(0, |b| b.encoded_byte_count);
        if encoded > 0 {
            self.take(encoded as usize)?;
            self.take(16)?; // bounds words
            return Ok(());
        }
        let revision = self.revision(3)?;
        let format = self.u8()?;
        let stride = usize::from(self.u16()?);
        self.take(2)?; // width
        let height = usize::from(self.u16()?);
        self.take(16)?; // bounds words
        if revision >= 3 {
            self.take(1)?; // alpha flag
        }
        if format < 4 {
            let count = usize::from(self.u16()?);
            if count > 256 {
                return self.invalid("Embedded bitmap palette exceeds 256 entries.");
            }
            let width = if revision >= 3 { 4 } else { 6 };
            self.count_value(count, width)?;
            self.take(count * width)?;
        }
        let compressed = if revision >= 2 {
            self.u8()? != 0
        } else {
            revision == 1
        };
        if !compressed {
            // stride and height are 16-bit, so the product cannot overflow.
            self.take(stride * height)?;
            return Ok(());
        }
        loop {
            let length = usize::from(self.u16()?);
            if length == 0 {
                return Ok(());
            }
            self.budget(1)?;
            self.take(length)?;
        }
    }
    /// Inline recursive compiled-clip dependency metadata (no MFC envelopes).
    fn swf_metadata(&mut self, depth: usize) -> Result<()> {
        if depth >= 32 {
            return self.unsupported("Compiled-clip metadata nesting exceeds 32.");
        }
        let v = self.revision(4)?;
        self.string()?; // name
        self.string()?; // implementation
        self.take(4)?; // version
        match v {
            1 => {
                self.take(1)?;
            }
            2.. => {
                self.take(4)?;
            }
            _ => {}
        }
        let lists: &[bool] = match v {
            2 => &[false],
            3.. => &[true, false, false],
            _ => &[],
        };
        for &named in lists {
            let count = self.count(if named { 8 } else { 7 })?;
            for _ in 0..count {
                if named {
                    self.string()?;
                }
                self.swf_metadata(depth + 1)?;
            }
        }
        if v >= 4 {
            self.string()?;
            self.string()?; // intrinsic class and script
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
            self.embedded_bitmap()?;
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
    pub(super) fn persistent_data(&mut self) -> Result<()> {
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
