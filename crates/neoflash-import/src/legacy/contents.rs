//! Strict sequential Contents prefix reader. Layout facts are referenced in
//! docs/IMPORTS.md; this implementation and its fixtures are independently written.
//! Unknown fields never trigger a scan for a plausible later boundary.
mod extensions;
mod payload;

use super::{BinaryStreamInspection, BinaryStreamRole};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_ITEMS: usize = 50_000;
const MAX_STRING_BYTES: usize = 1024 * 1024;

type Result<T> = std::result::Result<T, BinaryContentsStop>;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContentsCounts {
    pub descriptors_encountered: usize,
    pub descriptors_complete: usize,
    pub scene_entries: usize,
    pub library_entries: usize,
    pub media_entries: usize,
    pub resolved_streams: usize,
    pub settings_reached: bool,
    pub reference_entries: usize,
}

/// Half-open offsets into BinaryContentsPrefix.original_bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentsByteSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentsDescriptorClass {
    DocumentPage,
    Media,
    Bitmap,
    Sound,
    Video,
    /// Compiled-clip (`CSwfPage`) media descriptor.
    SwfPage,
    Color,
    ColorList,
    QuickTimeAudioSettings,
}

/// This is the archive's combined class/object identity table, including index 0.
/// A pending object has no descriptor index because its body did not complete.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentsReference {
    Null,
    Class {
        class: Option<ContentsDescriptorClass>,
        schema: u16,
        name: ContentsByteSpan,
    },
    Object {
        descriptor_index: Option<usize>,
    },
    AuxiliaryObject {
        class: ContentsDescriptorClass,
        encoded: ContentsByteSpan,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentsString {
    /// Includes length/encoding header. ANSI bytes have no inferred code page.
    pub encoded: ContentsByteSpan,
    pub data: ContentsByteSpan,
    pub utf16: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentsDescriptor {
    pub object_id: u32,
    pub class: ContentsDescriptorClass,
    pub schema: u16,
    pub revision: u8,
    /// For bitmap/sound descriptors, revision above is the common media base.
    pub subtype_revision: Option<u8>,
    /// Entire envelope and payload, including nested metadata/property records.
    pub encoded: ContentsByteSpan,
    pub stream_name: String,
    /// Resolved against the immutable container inventory, never a filesystem path.
    pub stream_index: Option<usize>,
    pub name: ContentsString,
    pub library_id: Option<u32>,
    pub symbol_type: Option<u8>,
    pub media_id: Option<u16>,
    #[serde(default)]
    pub library_metadata: Option<ContentsLibraryMetadata>,
    #[serde(default)]
    pub linkage: Option<ContentsLinkage>,
    #[serde(default)]
    pub bitmap: Option<ContentsBitmapSettings>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContentsBitmapSettings {
    pub compression: Option<u8>,
    pub quality: Option<u8>,
    pub smoothing: bool,
    pub encoded_byte_count: i32,
    pub deblocking: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContentsLibraryMetadata {
    pub parent: Option<[u32; 2]>,
    pub item: Option<[u32; 2]>,
    pub attributes: Option<u32>,
    pub linkage: Option<ContentsLinkage>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentsLinkage {
    pub flags: u32,
    pub identifier: ContentsString,
    pub url: ContentsString,
    pub class_name: Option<ContentsString>,
    pub export_flags: Option<u8>,
    pub base_class: Option<ContentsString>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BinaryDocumentSettings {
    pub encoded: ContentsByteSpan,
    pub ruler_unit: u16,
    pub grid_display_mode: u8,
    /// xmin, xmax, ymin, ymax in twips. Undefined if xmin == i32::MIN.
    pub bounds_words: [i32; 4],
    pub grid_spacing_fixed16_twips: u32,
    pub preview_and_ruler_flags: [u8; 3],
    pub view_play_and_printer_flags: [u8; 2],
    pub print_margins_twips: [i32; 4],
    pub print_flags: [u8; 4],
    pub print_scale_fixed16: i32,
    pub printer_mode: ContentsByteSpan,
    pub printer_names: ContentsByteSpan,
    /// Low byte R, then G, B, A. No historical alpha normalization is inferred.
    pub background_rgba_word: u32,
    pub grid_rgba_word: u32,
    pub frame_rate_fixed16: u32,
    pub storyboard_flags: [u8; 3],
    pub storyboard_margin_twips: i32,
}
impl BinaryDocumentSettings {
    pub fn size_twips(&self) -> Option<[i64; 2]> {
        let [xmin, xmax, ymin, ymax] = self.bounds_words;
        let width = i64::from(xmax) - i64::from(xmin);
        let height = i64::from(ymax) - i64::from(ymin);
        (xmin != i32::MIN && width > 0 && height > 0).then_some([width, height])
    }
    pub fn frame_rate(&self) -> f64 {
        f64::from(self.frame_rate_fixed16) / 65536.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BinaryContentsStop {
    pub code: String,
    pub offset: usize,
    /// Static description: no source names, bytes, strings or scripts.
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentsProperty {
    pub key: ContentsString,
    pub value: ContentsString,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentsLegacyPublish {
    pub encoded: ContentsByteSpan,
    pub entries: Vec<ContentsProperty>,
    pub first_word: u32,
    pub first_string: ContentsString,
    pub second_string: ContentsString,
    pub second_word: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentsCompatibilityTail {
    pub encoded: ContentsByteSpan,
    /// Initial supported subset has an empty shared-library list, revision zero.
    pub writer_words: [u16; 4],
    pub writer_data: ContentsByteSpan,
}

/// Opt-in private preservation IR. The original stream includes its unread suffix.
/// Only descriptors with known end boundaries enter `descriptors`; a prefix can
/// stop earlier, with settings == None. No scene or source code is executed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BinaryContentsPrefix {
    pub revision: Option<u8>,
    pub compatibility_marker: Option<u32>,
    pub ole_count: Option<u32>,
    pub next_scene_id: Option<u16>,
    pub next_item_tag: Option<u16>,
    pub library_counter: Option<u16>,
    pub media_counter: Option<u16>,
    pub scenes: Vec<u32>,
    pub library: Vec<u32>,
    pub media: Vec<u32>,
    pub selected_scene: Option<u32>,
    pub references: Vec<ContentsReference>,
    pub descriptors: Vec<ContentsDescriptor>,
    pub settings: Option<BinaryDocumentSettings>,
    pub bytes_consumed: usize,
    pub stop: Option<BinaryContentsStop>,
    #[serde(default)]
    pub legacy_publish: Option<ContentsLegacyPublish>,
    #[serde(default)]
    pub compatibility_tail: Option<ContentsCompatibilityTail>,
    #[serde(default)]
    pub extensions: Option<ContentsExtensions>,
    pub original_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContentsExtensions {
    pub encoded: ContentsByteSpan,
    pub document_properties: Vec<ContentsProperty>,
    pub profiles: Vec<Vec<ContentsProperty>>,
    pub extension_properties: Vec<ContentsProperty>,
    pub folders: Vec<ContentsFolder>,
    pub document_type: Option<u32>,
    pub metadata_xml: Option<ContentsString>,
    pub shared_library_url: Option<ContentsString>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentsFolder {
    pub name: ContentsString,
    pub metadata: ContentsLibraryMetadata,
    pub encoded: ContentsByteSpan,
}
impl BinaryContentsPrefix {
    /// Decode known Unicode or ASCII metadata without guessing an ANSI codepage.
    pub fn string_text(&self, value: &ContentsString) -> Option<String> {
        let bytes = self.original_bytes.get(value.data.start..value.data.end)?;
        if value.utf16 {
            if bytes.len() % 2 != 0 {
                return None;
            }
            String::from_utf16(
                &bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect::<Vec<_>>(),
            )
            .ok()
        } else {
            bytes
                .is_ascii()
                .then(|| String::from_utf8_lossy(bytes).into_owned())
        }
    }
    /// Looks up one key in a publish profile. Before revision 0x2C the single
    /// legacy key/value list is profile 0.
    pub fn publish_property(&self, profile: usize, key: &str) -> Option<String> {
        let properties = self.extensions.as_ref()?.profiles.get(profile)?;
        let mut values = properties
            .iter()
            .filter(|p| self.string_text(&p.key).as_deref() == Some(key));
        let result = self.string_text(&values.next()?.value)?;
        values.next().is_none().then_some(result)
    }
}

pub(super) struct InventoryEntry {
    pub index: usize,
    pub role: BinaryStreamRole,
}
pub(super) type Inventory = BTreeMap<String, InventoryEntry>;

pub(super) fn inspect(
    bytes: &[u8],
    stream: &mut BinaryStreamInspection,
    inventory: &Inventory,
    retain: bool,
) -> BinaryContentsPrefix {
    let mut reader = Reader {
        bytes,
        at: 0,
        items: 0,
        object_depth: 0,
        refs: vec![ContentsReference::Null],
        descriptors: Vec::new(),
        inventory,
        claimed_streams: BTreeSet::new(),
        counts: ContentsCounts::default(),
    };
    let mut out = BinaryContentsPrefix {
        revision: None,
        compatibility_marker: None,
        ole_count: None,
        next_scene_id: None,
        next_item_tag: None,
        library_counter: None,
        media_counter: None,
        scenes: Vec::new(),
        library: Vec::new(),
        media: Vec::new(),
        selected_scene: None,
        references: Vec::new(),
        descriptors: Vec::new(),
        settings: None,
        bytes_consumed: 0,
        stop: None,
        legacy_publish: None,
        compatibility_tail: None,
        extensions: None,
        original_bytes: Vec::new(),
    };
    let parsed = reader.prefix(&mut out);
    out.stop = parsed.err();
    out.bytes_consumed = reader.at;
    reader.counts.reference_entries = reader.refs.len();
    reader.counts.settings_reached = out.settings.is_some();
    stream.bytes_consumed = out.bytes_consumed;
    stream.structure_complete = out.stop.is_none();
    stream.stop_code = out.stop.as_ref().map(|stop| stop.code.clone());
    stream.stop_message = out.stop.as_ref().map(|stop| stop.message.clone());
    stream.contents = Some(reader.counts);
    out.references = reader.refs;
    out.descriptors = reader.descriptors;
    if retain && bytes.len() <= MAX_BYTES {
        out.original_bytes = bytes.to_vec();
    }
    out
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    items: usize,
    object_depth: usize,
    refs: Vec<ContentsReference>,
    descriptors: Vec<ContentsDescriptor>,
    inventory: &'a Inventory,
    claimed_streams: BTreeSet<usize>,
    counts: ContentsCounts,
}
impl Reader<'_> {
    fn error(&self, code: &str, message: &str) -> BinaryContentsStop {
        BinaryContentsStop {
            code: code.into(),
            offset: self.at,
            message: message.into(),
        }
    }
    fn unsupported<T>(&self, message: &str) -> Result<T> {
        Err(self.error("fla_contents_record_unsupported", message))
    }
    fn invalid<T>(&self, message: &str) -> Result<T> {
        Err(self.error("fla_contents_invalid_record", message))
    }
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&end| end <= self.bytes.len())
            .ok_or_else(|| {
                self.error(
                    "fla_truncated_record",
                    "Contents field exceeds remaining bytes.",
                )
            })?;
        let value = &self.bytes[self.at..end];
        self.at = end;
        Ok(value)
    }
    fn span(&mut self, n: usize) -> Result<ContentsByteSpan> {
        let start = self.at;
        self.take(n)?;
        Ok(ContentsByteSpan {
            start,
            end: self.at,
        })
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    fn words(&mut self) -> Result<[i32; 4]> {
        Ok([self.i32()?, self.i32()?, self.i32()?, self.i32()?])
    }
    fn budget(&mut self, n: usize) -> Result<()> {
        self.items = self
            .items
            .checked_add(n)
            .filter(|&v| v <= MAX_ITEMS)
            .ok_or_else(|| {
                self.error(
                    "fla_contents_budget",
                    "Contents reference/collection work exceeds 50000 items.",
                )
            })?;
        Ok(())
    }
    fn count(&mut self, minimum_bytes: usize) -> Result<usize> {
        let n = self.u32()? as usize;
        self.count_value(n, minimum_bytes)?;
        Ok(n)
    }
    fn count_value(&mut self, n: usize, minimum_bytes: usize) -> Result<()> {
        self.budget(n)?;
        if n.checked_mul(minimum_bytes)
            .is_none_or(|size| size > self.bytes.len() - self.at)
        {
            return Err(self.error(
                "fla_truncated_record",
                "Contents collection exceeds remaining bytes.",
            ));
        }
        Ok(())
    }
    fn revision(&mut self, maximum: u8) -> Result<u8> {
        let v = self.u8()?;
        if v > maximum {
            return self.unsupported("Nested Contents record revision is unsupported.");
        }
        Ok(v)
    }
    fn prefix(&mut self, out: &mut BinaryContentsPrefix) -> Result<()> {
        if self.bytes.len() > MAX_BYTES {
            return Err(self.error("fla_contents_budget", "Contents stream exceeds 8 MiB."));
        }
        let v = self.u8()?;
        out.revision = Some(v);
        if super::version(v).is_none() {
            return Err(self.error(
                "fla_revision_unsupported",
                "Contents save-target revision is unknown.",
            ));
        }
        // Flash 1 and 2 predate the reserved-byte, library-list and settings
        // layout shared by every later save target.
        if v < 0x14 {
            return self.unsupported(
                "Contents prefix revision is unsupported; Flash 3 through CS4 are decoded.",
            );
        }
        if v >= 0x20 {
            out.compatibility_marker = Some(self.u32()?);
        }
        let ole_count = self.u32()?;
        out.ole_count = Some(ole_count);
        self.count_value(ole_count as usize, 2)?;
        // Counted OLE pointers are not a null-terminated descriptor list.
        for _ in 0..ole_count {
            if self.u16()? != 0 {
                return self.unsupported("Non-null OLE descriptor is unsupported.");
            }
        }
        self.take(1)?;
        if v >= 0x15 {
            self.take(1)?;
        }
        for threshold in [0x21, 0x2b, 0x3b, 0x40, 0x44] {
            if v >= threshold {
                self.take(4)?;
            }
        }
        self.list(BinaryStreamRole::Scene, &mut out.scenes)?;
        out.next_scene_id = Some(self.u16()?);
        out.next_item_tag = Some(self.u16()?);
        out.selected_scene = self.object()?;
        if out
            .selected_scene
            .is_some_and(|id| !out.scenes.contains(&id))
        {
            return self.invalid("Selected-scene pointer does not identify a scene-list object.");
        }
        self.list(BinaryStreamRole::Symbol, &mut out.library)?;
        out.library_counter = Some(self.u16()?);
        self.list(BinaryStreamRole::Media, &mut out.media)?;
        out.media_counter = Some(self.u16()?);
        out.settings = Some(self.settings()?);
        self.extensions(out, v)
    }
    fn list(&mut self, role: BinaryStreamRole, values: &mut Vec<u32>) -> Result<()> {
        let mut members = BTreeSet::new();
        while let Some(id) = self.object()? {
            self.budget(1)?;
            if !members.insert(id) {
                return self.invalid("Duplicate descriptor identity in Contents list.");
            }
            let Some(ContentsReference::Object {
                descriptor_index: Some(index),
            }) = self.refs.get(id as usize)
            else {
                return self.invalid("Contents list references an incomplete object.");
            };
            let descriptor = &mut self.descriptors[*index];
            let class_valid = match role {
                BinaryStreamRole::Scene | BinaryStreamRole::Symbol => {
                    descriptor.class == ContentsDescriptorClass::DocumentPage
                }
                BinaryStreamRole::Media => {
                    descriptor.class != ContentsDescriptorClass::DocumentPage
                }
                _ => false,
            };
            if !class_valid {
                return self.invalid("Descriptor class does not match its Contents list.");
            }
            let target = self
                .inventory
                .get(&descriptor.stream_name.to_ascii_uppercase())
                .ok_or_else(|| BinaryContentsStop {
                    code: "fla_contents_stream_reference".into(),
                    offset: self.at,
                    message:
                        "Descriptor target stream is absent from the root container inventory."
                            .into(),
                })?;
            if target.role != role {
                return Err(self.error(
                    "fla_contents_stream_reference",
                    "Descriptor target stream role does not match its Contents list.",
                ));
            }
            if !self.claimed_streams.insert(target.index) {
                return Err(self.error(
                    "fla_contents_stream_reference",
                    "Multiple Contents descriptors claim the same target stream.",
                ));
            }
            descriptor.stream_index = Some(target.index);
            self.counts.resolved_streams += 1;
            match role {
                BinaryStreamRole::Scene => self.counts.scene_entries += 1,
                BinaryStreamRole::Symbol => self.counts.library_entries += 1,
                BinaryStreamRole::Media => self.counts.media_entries += 1,
                _ => {}
            }
            values.push(id);
        }
        Ok(())
    }
    fn add_reference(&mut self, reference: ContentsReference) -> Result<u32> {
        self.budget(1)?;
        let id = self.refs.len() as u32;
        self.refs.push(reference);
        Ok(id)
    }
    fn object(&mut self) -> Result<Option<u32>> {
        if self.object_depth >= 32 {
            return self.unsupported("Contents object nesting exceeds 32.");
        }
        self.object_depth += 1;
        let result = self.object_inner();
        self.object_depth -= 1;
        result
    }
    fn object_inner(&mut self) -> Result<Option<u32>> {
        let start = self.at;
        let short = self.u16()?;
        let tag = if short == 0x7fff {
            self.u32()?
        } else if short & 0x8000 != 0 {
            0x80000000 | u32::from(short & 0x7fff)
        } else {
            u32::from(short)
        };
        let (class, schema) = if short == 0xffff {
            let schema = self.u16()?;
            let length = usize::from(self.u16()?);
            if !(1..=63).contains(&length) {
                return self.invalid("Invalid MFC class-name length.");
            }
            let name = self.span(length)?;
            let bytes = &self.bytes[name.start..name.end];
            if !bytes.iter().all(|&b| (33..=126).contains(&b)) {
                return self.invalid("Invalid MFC class-name encoding.");
            }
            let class = match bytes {
                b"CDocumentPage" => Some(ContentsDescriptorClass::DocumentPage),
                b"CMediaElem" => Some(ContentsDescriptorClass::Media),
                b"CMediaBits" => Some(ContentsDescriptorClass::Bitmap),
                b"CMediaSound" => Some(ContentsDescriptorClass::Sound),
                b"CMediaVideoStream" => Some(ContentsDescriptorClass::Video),
                b"CSwfPage" => Some(ContentsDescriptorClass::SwfPage),
                b"CColorDef" => Some(ContentsDescriptorClass::Color),
                b"CColorList" => Some(ContentsDescriptorClass::ColorList),
                b"CQTAudioSettings" => Some(ContentsDescriptorClass::QuickTimeAudioSettings),
                _ => None,
            };
            self.add_reference(ContentsReference::Class {
                class,
                schema,
                name,
            })?;
            (class, schema)
        } else if tag & 0x80000000 != 0 {
            let Some(ContentsReference::Class { class, schema, .. }) =
                self.refs.get((tag & 0x7fffffff) as usize)
            else {
                return self.invalid("MFC class pointer does not identify an existing class.");
            };
            (*class, *schema)
        } else {
            return match self.refs.get(tag as usize) {
                Some(ContentsReference::Null) => Ok(None),
                Some(ContentsReference::Object {
                    descriptor_index: Some(_),
                })
                | Some(ContentsReference::AuxiliaryObject { .. }) => Ok(Some(tag)),
                _ => {
                    self.invalid("MFC object pointer does not identify a complete existing object.")
                }
            };
        };
        let id = self.add_reference(ContentsReference::Object {
            descriptor_index: None,
        })?;
        let Some(class) = class else {
            return self.unsupported("MFC Contents descriptor class is unsupported.");
        };
        if class == ContentsDescriptorClass::QuickTimeAudioSettings {
            if schema != 1 {
                return self.unsupported("MFC QuickTime audio-settings schema is unsupported.");
            }
            let length = self.u32()? as usize;
            self.take(length)?;
            self.refs[id as usize] = ContentsReference::AuxiliaryObject {
                class,
                encoded: ContentsByteSpan {
                    start,
                    end: self.at,
                },
            };
            return Ok(Some(id));
        }
        if matches!(
            class,
            ContentsDescriptorClass::Color | ContentsDescriptorClass::ColorList
        ) {
            if schema != 0 {
                return self.unsupported("MFC color schema is unsupported.");
            }
            if class == ContentsDescriptorClass::Color {
                self.color()?;
            } else {
                self.color_list(0)?;
            }
            self.refs[id as usize] = ContentsReference::AuxiliaryObject {
                class,
                encoded: ContentsByteSpan {
                    start,
                    end: self.at,
                },
            };
            return Ok(Some(id));
        }
        self.counts.descriptors_encountered += 1;
        if schema != 1 {
            return self.unsupported("MFC Contents descriptor schema is unsupported.");
        }
        let mut descriptor = self.descriptor(id, class, schema)?;
        descriptor.encoded = ContentsByteSpan {
            start,
            end: self.at,
        };
        self.refs[id as usize] = ContentsReference::Object {
            descriptor_index: Some(self.descriptors.len()),
        };
        self.descriptors.push(descriptor);
        self.counts.descriptors_complete += 1;
        Ok(Some(id))
    }
    fn string_length(&mut self) -> Result<Option<u32>> {
        let first = self.u8()?;
        if first < 255 {
            return Ok(Some(u32::from(first)));
        }
        match self.u16()? {
            0xfffe => Ok(None),
            0xffff => Ok(Some(self.u32()?)),
            n => Ok(Some(u32::from(n))),
        }
    }
    fn string(&mut self) -> Result<ContentsString> {
        let start = self.at;
        let first = self.string_length()?;
        let utf16 = first.is_none();
        let length = if let Some(length) = first {
            length
        } else {
            self.string_length()?.ok_or_else(|| {
                self.error(
                    "fla_contents_invalid_record",
                    "Repeated CString Unicode marker.",
                )
            })?
        };
        // MFC's extended all-ones sentinel encodes an empty string.
        let length = if length == u32::MAX {
            0
        } else {
            length as usize
        };
        let byte_count = length
            .checked_mul(if utf16 { 2 } else { 1 })
            .filter(|&n| n <= MAX_STRING_BYTES)
            .ok_or_else(|| self.error("fla_contents_budget", "CString exceeds 1 MiB."))?;
        let data = self.span(byte_count)?;
        if utf16
            && char::decode_utf16(
                self.bytes[data.start..data.end]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes([b[0], b[1]])),
            )
            .any(|c| c.is_err())
        {
            return self.invalid("Invalid UTF-16 CString.");
        }
        Ok(ContentsString {
            encoded: ContentsByteSpan {
                start,
                end: self.at,
            },
            data,
            utf16,
        })
    }
    fn stream_name(&mut self, unicode: bool, trailing_nul: bool) -> Result<String> {
        let length = usize::from(self.u8()?);
        if !(1..=31).contains(&length) {
            return self.invalid("Invalid compact stream-name length.");
        }
        let mut units: Vec<u16> = if unicode {
            self.take(length * 2)?
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect()
        } else {
            self.take(length)?.iter().map(|&b| u16::from(b)).collect()
        };
        if trailing_nul && units.last() == Some(&0) {
            units.pop();
        }
        if units.is_empty()
            || !units
                .iter()
                .all(|&c| c <= 127 && ((c as u8).is_ascii_alphanumeric() || c == 32))
        {
            return self.invalid("Invalid compact stream-name encoding.");
        }
        Ok(units.into_iter().map(|c| c as u8 as char).collect())
    }
    fn settings(&mut self) -> Result<BinaryDocumentSettings> {
        let start = self.at;
        let ruler_unit = self.u16()?;
        let grid_display_mode = self.u8()?;
        let bounds_words = self.words()?;
        let grid_spacing_fixed16_twips = self.u32()?;
        let preview_and_ruler_flags = self.take(3)?.try_into().unwrap();
        let view_play_and_printer_flags = self.take(2)?.try_into().unwrap();
        let print_margins_twips = self.words()?;
        let print_flags = self.take(4)?.try_into().unwrap();
        let print_scale_fixed16 = self.i32()?;
        let count = usize::from(self.u16()?);
        let printer_mode = self.span(count)?;
        let count = usize::from(self.u16()?);
        let printer_names = self.span(count)?;
        let background_rgba_word = self.u32()?;
        let grid_rgba_word = self.u32()?;
        let frame_rate_fixed16 = self.u32()?;
        let storyboard_flags = self.take(3)?.try_into().unwrap();
        let storyboard_margin_twips = self.i32()?;
        Ok(BinaryDocumentSettings {
            encoded: ContentsByteSpan {
                start,
                end: self.at,
            },
            ruler_unit,
            grid_display_mode,
            bounds_words,
            grid_spacing_fixed16_twips,
            preview_and_ruler_flags,
            view_play_and_printer_flags,
            print_margins_twips,
            print_flags,
            print_scale_fixed16,
            printer_mode,
            printer_names,
            background_rgba_word,
            grid_rgba_word,
            frame_rate_fixed16,
            storyboard_flags,
            storyboard_margin_twips,
        })
    }
}
