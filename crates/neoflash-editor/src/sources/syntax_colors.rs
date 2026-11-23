use eframe::egui;
use serde::{Deserialize, Serialize};

/// Per-token syntax and editor colour overrides (`Preferences ▸ Code ▸ Colour scheme`, `T-376`),
/// stored as unmultiplied sRGBA `[r, g, b, a]`. `None` uses the active NF2 theme's default token.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct SyntaxColorOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_class: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attr: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breakpoint: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_line: Option<[u8; 4]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SyntaxColorSlot {
    Keyword,
    Identifier,
    TypeClass,
    Function,
    String,
    Number,
    Comment,
    Tag,
    Attr,
    Error,
    Warning,
    Breakpoint,
    Selection,
    CurrentLine,
}

impl SyntaxColorSlot {
    pub const ALL: [Self; 14] = [
        Self::Keyword,
        Self::Identifier,
        Self::TypeClass,
        Self::Function,
        Self::String,
        Self::Number,
        Self::Comment,
        Self::Tag,
        Self::Attr,
        Self::Error,
        Self::Warning,
        Self::Breakpoint,
        Self::Selection,
        Self::CurrentLine,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Keyword => "Keyword",
            Self::Identifier => "Identifier",
            Self::TypeClass => "Type / class",
            Self::Function => "Function",
            Self::String => "String",
            Self::Number => "Number",
            Self::Comment => "Comment",
            Self::Tag => "XML tag",
            Self::Attr => "Attribute",
            Self::Error => "Error",
            Self::Warning => "Warning",
            Self::Breakpoint => "Breakpoint",
            Self::Selection => "Selection",
            Self::CurrentLine => "Current line",
        }
    }

    pub const fn default_color(self, p: &neoflash_ui::Palette) -> egui::Color32 {
        match self {
            Self::Keyword => p.syntax_keyword,
            Self::Identifier => p.syntax_identifier,
            Self::TypeClass => p.syntax_type,
            Self::Function => p.syntax_function,
            Self::String => p.syntax_string,
            Self::Number => p.syntax_number,
            Self::Comment => p.syntax_comment,
            Self::Tag => p.syntax_tag,
            Self::Attr => p.syntax_attr,
            Self::Error => p.syntax_error,
            Self::Warning => p.syntax_warning,
            Self::Breakpoint => p.syntax_breakpoint,
            Self::Selection => p.syntax_selection,
            Self::CurrentLine => p.syntax_current_line,
        }
    }
}

impl SyntaxColorOverrides {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub const fn get(&self, slot: SyntaxColorSlot) -> Option<[u8; 4]> {
        match slot {
            SyntaxColorSlot::Keyword => self.keyword,
            SyntaxColorSlot::Identifier => self.identifier,
            SyntaxColorSlot::TypeClass => self.type_class,
            SyntaxColorSlot::Function => self.function,
            SyntaxColorSlot::String => self.string,
            SyntaxColorSlot::Number => self.number,
            SyntaxColorSlot::Comment => self.comment,
            SyntaxColorSlot::Tag => self.tag,
            SyntaxColorSlot::Attr => self.attr,
            SyntaxColorSlot::Error => self.error,
            SyntaxColorSlot::Warning => self.warning,
            SyntaxColorSlot::Breakpoint => self.breakpoint,
            SyntaxColorSlot::Selection => self.selection,
            SyntaxColorSlot::CurrentLine => self.current_line,
        }
    }

    pub fn set(&mut self, slot: SyntaxColorSlot, value: Option<[u8; 4]>) {
        match slot {
            SyntaxColorSlot::Keyword => self.keyword = value,
            SyntaxColorSlot::Identifier => self.identifier = value,
            SyntaxColorSlot::TypeClass => self.type_class = value,
            SyntaxColorSlot::Function => self.function = value,
            SyntaxColorSlot::String => self.string = value,
            SyntaxColorSlot::Number => self.number = value,
            SyntaxColorSlot::Comment => self.comment = value,
            SyntaxColorSlot::Tag => self.tag = value,
            SyntaxColorSlot::Attr => self.attr = value,
            SyntaxColorSlot::Error => self.error = value,
            SyntaxColorSlot::Warning => self.warning = value,
            SyntaxColorSlot::Breakpoint => self.breakpoint = value,
            SyntaxColorSlot::Selection => self.selection = value,
            SyntaxColorSlot::CurrentLine => self.current_line = value,
        }
    }

    pub fn effective_color(
        &self,
        slot: SyntaxColorSlot,
        base: &neoflash_ui::Palette,
    ) -> egui::Color32 {
        match self.get(slot) {
            Some([r, g, b, a]) => egui::Color32::from_rgba_unmultiplied(r, g, b, a),
            None => slot.default_color(base),
        }
    }

    pub fn resolve_palette(&self, base: &neoflash_ui::Palette) -> neoflash_ui::Palette {
        let mut p = *base;
        let to_c32 = |[r, g, b, a]: [u8; 4]| egui::Color32::from_rgba_unmultiplied(r, g, b, a);
        if let Some(c) = self.keyword {
            p.syntax_keyword = to_c32(c);
        }
        if let Some(c) = self.identifier {
            p.syntax_identifier = to_c32(c);
        }
        if let Some(c) = self.type_class {
            p.syntax_type = to_c32(c);
        }
        if let Some(c) = self.function {
            p.syntax_function = to_c32(c);
        }
        if let Some(c) = self.string {
            p.syntax_string = to_c32(c);
        }
        if let Some(c) = self.number {
            p.syntax_number = to_c32(c);
        }
        if let Some(c) = self.comment {
            p.syntax_comment = to_c32(c);
        }
        if let Some(c) = self.tag {
            p.syntax_tag = to_c32(c);
        }
        if let Some(c) = self.attr {
            p.syntax_attr = to_c32(c);
        }
        if let Some(c) = self.error {
            p.syntax_error = to_c32(c);
        }
        if let Some(c) = self.warning {
            p.syntax_warning = to_c32(c);
        }
        if let Some(c) = self.breakpoint {
            p.syntax_breakpoint = to_c32(c);
        }
        if let Some(c) = self.selection {
            p.syntax_selection = to_c32(c);
        }
        if let Some(c) = self.current_line {
            p.syntax_current_line = to_c32(c);
        }
        p
    }
}
