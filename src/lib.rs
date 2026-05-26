mod grep;
mod input;
mod paths;
mod profile;
mod render;
mod shape;

pub use grep::{
    GrepMatchValue, GrepOptions, GrepPredicate, GrepReport, GrepSomeConstraint, MatchMode,
    PathExpr, collect_grep, parse_path_expr,
};
pub use input::{DiscoveredInput, InputFormat, InputOptions, discover_inputs};
pub use paths::{PathReport, PathsOptions, collect_paths};
pub use profile::{ProfileOptions, ProfileReport, build_profile};
pub use render::{
    OutputMode, write_grep_matches, write_grep_report, write_path_list, write_paths, write_profile,
    write_shape,
};
pub use shape::{ShapeOptions, ShapeReport, infer_shape};
