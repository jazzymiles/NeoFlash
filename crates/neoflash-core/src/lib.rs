//! Document model and deterministic authoring/runtime evaluation shared by desktop and web.
mod demo;
mod evaluate;
mod geometry;
mod history;
pub mod model;
mod runtime;
mod validation;

pub use evaluate::{DrawItem, affine_multiply, easing, evaluate, sample_layer, transform_matrix};
pub use geometry::{
    FlattenOptions, FlattenedContour, GeometryError, MAX_PATH_COMMANDS, PathBounds, flatten_path,
    hit_test_path, path_bounds, paths_have_same_topology, validate_path_commands,
};
pub use history::History;
pub use model::*;
pub use runtime::{AccessibleNode, PlaybackCommand, Player, ScriptDispatch, ScriptEvent};
pub use validation::{ProjectError, ValidationIssue, validate};
