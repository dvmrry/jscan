mod input;
mod paths;
mod profile;
mod render;
mod shape;

pub use input::{DiscoveredInput, InputFormat, InputOptions, discover_inputs};
pub use paths::{PathReport, PathsOptions, collect_paths};
pub use profile::{ProfileOptions, ProfileReport, build_profile};
pub use render::{OutputMode, write_path_list, write_paths, write_profile, write_shape};
pub use shape::{ShapeOptions, ShapeReport, infer_shape};
