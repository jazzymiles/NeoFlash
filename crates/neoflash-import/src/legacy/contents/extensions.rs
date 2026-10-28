//! Versioned document-tail layouts, independently implemented from the format
//! evidence in JPEXS FLA Convert 0923d708 (ContentsRecordDecoder,
//! DocumentTailRecordDecoder and ColorRecordDecoder). No recovery scanning.
use super::*;

impl Reader<'_> {
    fn archive_count(&mut self, minimum: usize) -> Result<usize> {
        let small = self.u16()?;
        let count = if small == u16::MAX {
            self.u32()? as usize
        } else {
            small as usize
        };
        self.count_value(count, minimum)?;
        Ok(count)
    }
    fn properties_map(&mut self) -> Result<Vec<ContentsProperty>> {
        let count = self.archive_count(2)?;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            entries.push(ContentsProperty {
                key: self.string()?,
                value: self.string()?,
            });
        }
        Ok(entries)
    }
    fn object_properties(&mut self) -> Result<()> {
        let count = self.archive_count(3)?;
        for _ in 0..count {
            self.string()?;
            // An object value has a real MFC envelope; unknown classes stop.
            self.object()?;
        }
        Ok(())
    }
    pub(super) fn color(&mut self) -> Result<()> {
        let revision = self.revision(4)?;
        if revision < 2 {
            self.take(44)?;
        } else {
            self.take(4)?;
            let flags = self.u8()?;
            self.take(1)?;
            if flags & 0x10 != 0 {
                self.take(24)?;
                let count = self.u8()? as usize;
                if count > 15 {
                    return self.unsupported("Swatch gradient has more than 15 stops.");
                }
                if revision >= 4 {
                    self.take(8)?;
                }
                self.count_value(count, 5)?;
                self.take(count * 5)?;
            } else if flags & 0x20 != 0 {
                self.take(36)?;
            }
        }
        if revision >= 1 {
            self.take(6)?;
        }
        Ok(())
    }
    pub(super) fn color_list(&mut self, depth: usize) -> Result<()> {
        if depth >= 16 {
            return self.unsupported("Swatch nesting exceeds 16.");
        }
        let count = self.archive_count(2)?;
        for _ in 0..count {
            let id = self
                .object()?
                .ok_or_else(|| self.error("fla_contents_invalid_record", "Null swatch object."))?;
            if !matches!(
                self.refs.get(id as usize),
                Some(ContentsReference::AuxiliaryObject {
                    class: ContentsDescriptorClass::Color,
                    ..
                })
            ) {
                return self.unsupported("Swatch object is not a supported color definition.");
            }
        }
        let revision = self.revision(1)?;
        if revision >= 1 {
            let count = self.u8()? as usize;
            self.count_value(count, 3)?;
            for _ in 0..count {
                self.color_list(depth + 1)?;
            }
        }
        Ok(())
    }
    fn document_accessibility(&mut self, unicode: bool) -> Result<()> {
        let revision = self.u32()?;
        if revision > 2 {
            return self.unsupported("Document accessibility revision is unsupported.");
        }
        if revision != 0 {
            self.take(4)?;
            self.string()?;
            self.string()?;
            self.string()?;
            if unicode && revision >= 2 {
                self.string()?;
                self.string()?;
            }
            self.take(8)?;
        }
        Ok(())
    }
    fn shared_libraries(&mut self) -> Result<()> {
        self.revision(0)?;
        let count = self.count(1)?;
        for _ in 0..count {
            let revision = self.revision(2)?;
            self.string()?;
            let length = self.u32()? as usize;
            self.take(length)?;
            if revision >= 1 {
                let count = self.count(1)?;
                for _ in 0..count {
                    self.string()?;
                }
            }
            if revision == 1 {
                let count = self.count(1)?;
                for _ in 0..count {
                    self.string()?;
                }
            }
        }
        Ok(())
    }
    fn writer_identity(&mut self) -> Result<ContentsCompatibilityTail> {
        let start = self.at;
        let writer_words = [self.u16()?, self.u16()?, self.u16()?, self.u16()?];
        let length = self.u16()? as usize;
        if length > 256 {
            return self.unsupported("Writer identity exceeds the supported byte length.");
        }
        let writer_data = self.span(length)?;
        Ok(ContentsCompatibilityTail {
            encoded: ContentsByteSpan {
                start,
                end: self.at,
            },
            writer_words,
            writer_data,
        })
    }
    pub(super) fn extensions(&mut self, out: &mut BinaryContentsPrefix, version: u8) -> Result<()> {
        let start = self.at;
        // Store incremental typed evidence even if a later extension stops.
        out.extensions = Some(ContentsExtensions::default());
        if version < 0x2c {
            let properties = self.properties_map()?;
            out.extensions.as_mut().unwrap().profiles.push(properties);
        } else {
            if version >= 0x2d {
                out.extensions.as_mut().unwrap().document_properties = self.properties_map()?;
            }
            let count = self.count(2)?;
            for _ in 0..count {
                let properties = self.properties_map()?;
                out.extensions.as_mut().unwrap().profiles.push(properties);
            }
        }
        self.take(4)?;
        self.string()?;
        self.string()?;
        self.take(4)?;
        out.extensions.as_mut().unwrap().extension_properties = self.properties_map()?;
        self.color_list(0)?;
        self.color_list(0)?;
        let library_view_revision = self.u32()?;
        if library_view_revision > 3 {
            return self.unsupported("Library-view revision is unsupported.");
        }
        self.take(28)?;
        if library_view_revision >= 2 {
            self.take(8)?;
        }
        if library_view_revision >= 3 {
            self.take(4)?;
        }
        self.take(4)?; // folder collection header
        let count = self.count(13)?;
        for _ in 0..count {
            let start = self.at;
            let revision = self.u32()?;
            if revision > 4 {
                return self.unsupported("Library folder revision is unsupported.");
            }
            let name = self.string()?;
            if revision == 2 {
                self.take(4)?;
            }
            let metadata = self.library_metadata()?;
            out.extensions
                .as_mut()
                .unwrap()
                .folders
                .push(ContentsFolder {
                    name,
                    metadata,
                    encoded: ContentsByteSpan {
                        start,
                        end: self.at,
                    },
                });
        }
        if version >= 0x2d {
            self.object_properties()?;
        }
        let count = if version < 0x2c {
            1
        } else {
            out.extensions.as_ref().unwrap().profiles.len()
        };
        for _ in 0..count {
            self.object_properties()?;
        }
        self.take(2)?; // next symbol identifier
        self.take(7)?; // guides color and three flags
        self.take(4)?; // font library header
        let fonts = self.count(1)?;
        if fonts != 0 {
            return self
                .unsupported("Embedded font library records require typed font preservation.");
        }
        out.extensions.as_mut().unwrap().shared_library_url = Some(self.string()?);
        self.take(14)?; // grid flags, accuracy and Y spacing
        self.string()?;
        self.string()?; // template description and filename
        self.take(12)?; // preview frame, pixel grid and snap
        self.document_accessibility(version >= 0x2f)?;
        self.take(4)?; // next media identifier
        if version >= 0x2e {
            self.take(16)?;
        }
        if version >= 0x30 {
            self.take(1)?;
        }
        if version >= 0x32 && self.u8()? != 0 {
            self.persistent_data()?;
        }
        if version >= 0x33 {
            self.take(4)?;
        }
        if version >= 0x34 {
            out.extensions.as_mut().unwrap().document_type = Some(self.u32()?);
        }
        if version >= 0x35 && self.u32()? != 0 {
            let count = self.i32()?;
            // This is the native fixed-capacity language selector: negative
            // sentinels mean no entries and values above 101 consume 101.
            let count = count.clamp(0, 101) as usize;
            self.string()?;
            self.take(4)?;
            self.string()?;
            self.count_value(count, 1)?;
            for _ in 0..count {
                self.string()?;
            }
        }
        if version >= 0x36 {
            let present = self.u32()?;
            if present > 1 {
                return self.invalid("Invalid stage-language presence flag.");
            }
            if present == 1 {
                self.string()?;
            }
        }
        if version >= 0x38 {
            self.take(4)?;
        }
        if version >= 0x3a {
            out.extensions.as_mut().unwrap().metadata_xml = Some(self.string()?);
        }
        if version >= 0x3c {
            self.string()?;
        }
        if version >= 0x3d {
            self.take(4)?;
        }
        if version >= 0x3e && self.u32()? != 0 {
            self.take(16)?;
            self.string()?;
            let count = self.i32()?;
            if count < 0 {
                return self.invalid("Negative postprocessor byte count.");
            }
            self.take(count as usize)?;
        }
        if version >= 0x3f {
            self.take(8)?;
        }
        if version >= 0x42 {
            self.shared_libraries()?;
        }
        if version >= 0x43 {
            out.compatibility_tail = Some(self.writer_identity()?);
        }
        if version >= 0x45 {
            self.take(8)?;
        }
        if version >= 0x46 {
            self.take(8)?;
        }
        if version >= 0x47 {
            self.take(4)?;
        }
        if self.at != self.bytes.len() && version < 0x43 {
            if version < 0x42 {
                self.shared_libraries()?;
            }
            out.compatibility_tail = Some(self.writer_identity()?);
        }
        if self.at != self.bytes.len() {
            return Err(self.error(
                "fla_contents_trailing_data",
                "Bytes remain after the supported document extensions.",
            ));
        }
        out.extensions.as_mut().unwrap().encoded = ContentsByteSpan {
            start,
            end: self.at,
        };
        Ok(())
    }
}
