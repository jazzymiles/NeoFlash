//! Document model and deterministic authoring/runtime evaluation shared by desktop and web.
mod demo;
mod evaluate;
mod history;
pub mod model;
mod runtime;
mod validation;

pub use evaluate::{DrawItem, affine_multiply, easing, evaluate, sample_layer, transform_matrix};
pub use history::History;
pub use model::*;
pub use runtime::{PlaybackCommand, Player, ScriptDispatch, ScriptEvent};
pub use validation::{ProjectError, ValidationIssue, validate};
