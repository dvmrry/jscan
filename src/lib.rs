mod input;
mod paths;
mod render;
mod shape;

pub use input::{DiscoveredInput, InputFormat, InputOptions, discover_inputs};
pub use paths::{PathReport, PathsOptions, collect_paths};
pub use render::{OutputMode, write_paths, write_shape};
pub use shape::{ShapeOptions, ShapeReport, infer_shape};
