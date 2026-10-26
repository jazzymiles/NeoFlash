//! Sequential bounded MFC structure reader. Unknown data stops parsing;
//! neither byte-pattern scans nor guessed record boundaries are used.
use super::{BinaryStreamInspection, RecordCounts};

const MAX_RECORDS: usize = 50_000;
const MAX_DEPTH: usize = 32;
const MAX_STRING: usize = 1024 * 1024;

#[derive(Clone, Copy, PartialEq)]
enum Class {
    Page,
    Layer,
    Frame,
    Shape,
    Bitmap,
    Text,
    Sprite,
    Button,
    Morph,
    Unknown,
}
impl Class {
    fn read(name: &[u8]) -> Self {
        match name {
            b"CPicPage" => Self::Page,
            b"CPicLayer" => Self::Layer,
            b"CPicFrame" => Self::Frame,
            b"CPicShape" => Self::Shape,
            b"CPicBitmap" => Self::Bitmap,
            b"CPicText" => Self::Text,
            b"CPicSprite" => Self::Sprite,
            b"CPicButton" => Self::Button,
            b"CPicMorphShape" => Self::Morph,
            _ => Self::Unknown,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Page => "CPicPage",
            Self::Layer => "CPicLayer",
            Self::Frame => "CPicFrame",
            Self::Shape => "CPicShape",
            Self::Bitmap => "CPicBitmap",
            Self::Text => "CPicText",
            Self::Sprite => "CPicSprite",
            Self::Button => "CPicButton",
            Self::Morph => "CPicMorphShape",
            Self::Unknown => "Unknown",
        }
    }
}
#[derive(Clone, Copy)]
enum Reference {
    Null,
    Class(Class, u16),
    Object(Class),
}
struct Stop {
    code: &'static str,
    message: String,
}
type Result<T> = std::result::Result<T, Stop>;
fn stop<T>(code: &'static str, message: impl Into<String>) -> Result<T> {
    Err(Stop {
        code,
        message: message.into(),
    })
}
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    refs: Vec<Reference>,
    counts: RecordCounts,
    root_class: Option<Class>,
    root_schema: Option<u16>,
}

pub(super) fn inspect(data: &[u8], stream: &mut BinaryStreamInspection) {
    let mut reader = Reader {
        data,
        pos: 0,
        refs: vec![Reference::Null],
        counts: RecordCounts::default(),
        root_class: None,
        root_schema: None,
    };
    let result = reader.root();
    stream.bytes_consumed = reader.pos;
    stream.records = reader.counts;
    stream.root_class = reader.root_class.map(|c| c.name().to_owned());
    stream.root_schema = reader.root_schema;
    match result {
        Ok(()) => stream.structure_complete = true,
        Err(e) => {
            stream.stop_code = Some(e.code.into());
            stream.stop_message = Some(e.message);
        }
    }
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.data.len() - self.pos {
            return stop(
                "fla_truncated_record",
                "Truncated MFC record; no boundary recovery attempted.",
            );
        }
        let value = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn word(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    fn dword(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn skip(&mut self, n: usize) -> Result<()> {
        self.bytes(n).map(|_| ())
    }
    fn revision(&mut self, max: u8, what: &str) -> Result<u8> {
        let value = self.byte()?;
        if value > max {
            return stop(
                "fla_record_revision_unsupported",
                format!("Unsupported {what} revision {value}; maximum decoded revision is {max}."),
            );
        }
        Ok(value)
    }
    fn count_bytes(&mut self, count: usize, width: usize) -> Result<()> {
        if count > MAX_RECORDS {
            return stop("fla_record_limit", "Collection count exceeds 50000.");
        }
        self.skip(count * width)
    }
    fn string_length(&mut self) -> Result<u32> {
        let short = self.byte()?;
        if short != 255 {
            return Ok(u32::from(short));
        }
        let medium = self.word()?;
        match medium {
            0xfffe => Ok(u32::MAX),
            0xffff => self.dword(),
            value => Ok(u32::from(value)),
        }
    }
    fn string(&mut self) -> Result<()> {
        let first = self.string_length()?;
        let unicode = first == u32::MAX;
        let len = if unicode {
            self.string_length()?
        } else {
            first
        };
        if len as usize > MAX_STRING / if unicode { 2 } else { 1 } {
            return stop(
                "fla_string_limit",
                "String exceeds 1 MiB or has a repeated Unicode marker.",
            );
        }
        let bytes = self.bytes(len as usize * if unicode { 2 } else { 1 })?;
        if unicode
            && char::decode_utf16(
                bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes(*c)),
            )
            .any(|c| c.is_err())
        {
            return stop("fla_string_encoding", "Invalid UTF-16 string.");
        }
        // ANSI strings are inspected without guessing their source code page.
        Ok(())
    }
    fn root(&mut self) -> Result<()> {
        let kind = self.byte()?;
        if kind > 1 {
            return stop(
                "fla_page_kind_unsupported",
                "Only shape (0) and timeline (1) stream roots are decoded.",
            );
        }
        let class = self.object(0, false)?;
        let expected = if kind == 0 { Class::Shape } else { Class::Page };
        if class != Some(expected) {
            return stop(
                "fla_invalid_root",
                "MFC root class disagrees with stream kind.",
            );
        }
        if self.pos != self.data.len() {
            return stop(
                "fla_trailing_data",
                "Trailing bytes remain after the MFC root; no content is silently dropped.",
            );
        }
        Ok(())
    }
    fn object(&mut self, depth: usize, allow_reference: bool) -> Result<Option<Class>> {
        if depth > MAX_DEPTH {
            return stop("fla_depth_limit", "MFC nesting exceeds 32.");
        }
        let small = self.word()?;
        let tag = if small == 0x7fff {
            self.dword()?
        } else if small & 0x8000 != 0 {
            0x80000000 | u32::from(small & 0x7fff)
        } else {
            u32::from(small)
        };
        if tag == 0 {
            return Ok(None);
        }
        let (class, schema) = if small == 0xffff {
            let schema = self.word()?;
            let len = usize::from(self.word()?);
            if !(1..=63).contains(&len) {
                return stop("fla_mfc_class", "Invalid MFC class-name length.");
            }
            let name = self.bytes(len)?;
            if !name.iter().all(|b| (33..=126).contains(b)) {
                return stop("fla_mfc_class", "MFC class name is not ASCII.");
            }
            let class = Class::read(name);
            self.add_reference(Reference::Class(class, schema))?;
            (class, schema)
        } else {
            let is_class = tag & 0x80000000 != 0;
            let index = (tag & 0x7fffffff) as usize;
            match self.refs.get(index).copied() {
                Some(Reference::Class(class, schema)) if is_class => (class, schema),
                Some(Reference::Object(class)) if !is_class && allow_reference => {
                    return Ok(Some(class));
                }
                Some(Reference::Object(_)) if !is_class => {
                    return stop(
                        "fla_mfc_object_reference_unsupported",
                        "Shared or cyclic child object references require graph decoding.",
                    );
                }
                _ => {
                    return stop(
                        "fla_mfc_reference",
                        "Invalid MFC reference index or reference kind.",
                    );
                }
            }
        };
        self.add_reference(Reference::Object(class))?;
        self.counts.encountered += 1;
        if depth == 0 {
            self.root_class = Some(class);
            self.root_schema = Some(schema);
        }
        if schema != 1 {
            return stop(
                "fla_mfc_schema_unsupported",
                format!("MFC schema {schema} is not implemented."),
            );
        }
        match class {
            Class::Page | Class::Layer | Class::Frame | Class::Shape | Class::Bitmap => {
                self.base(depth)?
            }
            _ => {
                return stop(
                    "fla_mfc_class_unsupported",
                    format!(
                        "{} payload is not implemented; parsing stops at its header.",
                        class.name()
                    ),
                );
            }
        }
        match class {
            Class::Page => self.page()?,
            Class::Layer => self.layer(depth)?,
            Class::Shape => self.shape()?,
            Class::Frame => {
                self.shape()?;
                self.frame(depth)?;
            }
            Class::Bitmap => self.bitmap()?,
            _ => unreachable!(),
        }
        self.counts.complete += 1;
        match class {
            Class::Page => self.counts.pages += 1,
            Class::Layer => self.counts.layers += 1,
            Class::Frame => self.counts.frames += 1,
            Class::Shape => self.counts.shapes += 1,
            _ => {}
        }
        Ok(Some(class))
    }
    fn add_reference(&mut self, r: Reference) -> Result<()> {
        if self.refs.len() >= MAX_RECORDS {
            return stop(
                "fla_record_limit",
                "MFC class/object table exceeds 50000 entries.",
            );
        }
        self.refs.push(r);
        Ok(())
    }
    fn base(&mut self, depth: usize) -> Result<()> {
        let rev = self.revision(5, "CPicObj")?;
        self.byte()?;
        while self.object(depth + 1, false)?.is_some() {}
        if rev >= 1 {
            self.skip(8)?;
        }
        if rev >= 3 {
            self.byte()?;
        }
        if rev >= 4 {
            self.byte()?;
        }
        Ok(())
    }
    fn page(&mut self) -> Result<()> {
        let rev = self.revision(7, "CPicPage")?;
        if rev != 4 {
            self.word()?;
        }
        if rev >= 5 {
            self.word()?;
        }
        if rev >= 7 {
            self.dword()?;
        }
        if rev >= 3 {
            let count = self.dword()? as usize;
            self.count_bytes(count, 8)?;
        }
        Ok(())
    }
    fn layer(&mut self, depth: usize) -> Result<()> {
        let rev = self.revision(11, "CPicLayer")?;
        self.string()?;
        self.skip(if rev <= 3 { 1 } else { 3 })?;
        if rev >= 5 {
            self.dword()?;
        }
        if rev >= 6 {
            self.skip(8)?;
        }
        if rev >= 8 {
            self.dword()?;
        }
        self.byte()?;
        self.object(depth + 1, true)?;
        if (7..=8).contains(&rev) {
            self.object(depth + 1, true)?;
        }
        if (2..=5).contains(&rev) {
            self.byte()?;
        }
        if rev >= 3 {
            self.byte()?;
        }
        if rev >= 10 {
            self.byte()?;
        }
        Ok(())
    }
    fn shape(&mut self) -> Result<()> {
        self.revision(6, "CPicShape")?;
        self.skip(24)?;
        let rev = self.revision(5, "shape data")?;
        self.dword()?;
        if self.word()? != 0 {
            return stop(
                "fla_shape_styles_unsupported",
                "Shape fill styles need a versioned geometry decoder.",
            );
        }
        if self.word()? != 0 {
            return stop(
                "fla_shape_styles_unsupported",
                "Shape stroke styles need a versioned geometry decoder.",
            );
        }
        if rev >= 2 && self.byte()? != 0 {
            return stop(
                "fla_shape_edges_unsupported",
                "Nonempty delta-coded edge data is not implemented.",
            );
        }
        if rev >= 5 && self.dword()? != 0 {
            return stop(
                "fla_shape_auxiliary_unsupported",
                "Auxiliary shape geometry is not implemented.",
            );
        }
        Ok(())
    }
    fn frame(&mut self, depth: usize) -> Result<()> {
        let rev = self.revision(26, "CPicFrame")?;
        if self.word()? == 0 {
            return stop("fla_frame_duration", "Frame duration must be nonzero.");
        }
        self.skip(if rev <= 2 { 1 } else { 2 })?;
        if rev >= 2 {
            self.word()?;
        }
        if rev >= 5 {
            self.word()?;
        }
        if rev >= 6 {
            let count = usize::from(self.word()?);
            self.count_bytes(count, 8)?;
        }
        if rev >= 7 {
            self.skip(11)?;
        }
        if rev >= 8 {
            self.word()?;
        }
        if rev >= 9 {
            self.string()?;
        }
        if (4..=18).contains(&rev) {
            return stop(
                "fla_historical_actions_unsupported",
                "Historical frame action records are not implemented.",
            );
        }
        if rev >= 19 {
            self.actions()?;
        }
        if rev >= 11 {
            self.skip(8)?;
        }
        if rev >= 12 {
            self.dword()?;
        }
        if rev >= 13 {
            self.object(depth + 1, true)?;
        }
        if rev >= 14 {
            self.dword()?;
        }
        if rev >= 15 {
            self.object(depth + 1, true)?;
        }
        if rev >= 16 {
            self.string()?;
        }
        if rev >= 20 {
            self.dword()?;
        }
        if rev >= 21 {
            self.dword()?;
        }
        if rev >= 22 {
            self.dword()?;
        }
        if rev >= 24 {
            self.dword()?;
            if self.dword()? != 0 {
                return stop(
                    "fla_custom_ease_unsupported",
                    "Custom easing records are not implemented.",
                );
            }
        }
        Ok(())
    }
    fn actions(&mut self) -> Result<()> {
        let rev = self.dword()?;
        if rev > 5 {
            return stop(
                "fla_action_revision_unsupported",
                "Unknown action source revision.",
            );
        }
        let mode = self.dword()?;
        if rev >= 1 {
            self.dword()?;
            let count = self.dword()? as usize;
            self.count_bytes(count, 4)?;
        }
        if rev >= 5 {
            let count = self.dword()? as usize;
            self.count_bytes(count, 4)?;
        }
        match mode {
            1 if rev != 3 => self.string(),
            2 => Ok(()),
            0 => {
                self.dword()?;
                if self.dword()? != 0 {
                    stop(
                        "fla_compiled_actions_unsupported",
                        "Nonempty compiled action lists require a dedicated decoder.",
                    )
                } else {
                    Ok(())
                }
            }
            _ => stop(
                "fla_action_mode_unsupported",
                "Action source encoding/mode is not implemented.",
            ),
        }
    }
    fn bitmap(&mut self) -> Result<()> {
        let rev = self.revision(2, "CPicBitmap")?;
        if rev == 0 {
            return stop(
                "fla_bitmap_revision_unsupported",
                "Embedded historical raster records are not implemented.",
            );
        }
        self.skip(24)?;
        self.word()?;
        if rev == 2 && self.byte()? != 0 {
            return stop(
                "fla_bitmap_extension_unsupported",
                "Bitmap instance extension is not implemented.",
            );
        }
        Ok(())
    }
}
