//! # neoflash-ui — the NeoFlash design system (NF2) for egui
//!
//! This crate is the code form of `interface/design-system` (Figma file `BUz3pseopiuOg2Uiy3Fog7`).
//! Panels in `neoflash-editor` are built **only** from what this crate exports:
//!
//! | Module | Figma source | Contents |
//! |---|---|---|
//! | [`tokens`] | `NF2 · Tokens` (Dark / Light) | [`Palette`], [`DARK`], [`LIGHT`], `space::S1..S6`, `radius::SM..FULL`, [`TextStyle`] |
//! | [`metrics`] | §3.2, §4, §5 of the reference | fixed control heights, window regions, timeline geometry |
//! | [`theme`] | — | [`Theme`], [`theme::install`], [`palette`] (current colours for a `Ui`) |
//! | [`typography`] | `NF2/*` text styles | bundled Inter + Roboto Mono, [`typography::rich`] |
//! | [`icons`] | `NF2 Icon/16/*`, `NF2 Icon/20/*` | [`Icon`] constants (`icons::i16::EYE`), tinted SVG rendering |
//! | [`cursors`] | `NF2 Cursor/*` | [`cursors::Cursor`] constants with hotspots |
//! | [`components`] | `NF2/*` component sets | one Rust type per Figma component set |
//!
//! Generated parts (`tokens`, `icons`, `cursors`) come from `build.rs`; never hand-edit them.
//! The crate has no dependency on the engine (core, script, export, import).

pub mod components;
pub mod cursors;
pub mod icons;
pub mod metrics;
pub mod theme;
pub mod tokens;
pub mod typography;

pub use icons::Icon;
pub use theme::{Theme, palette};
pub use tokens::{DARK, LIGHT, Palette, TextStyle};
