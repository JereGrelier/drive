mod overpass;
pub mod parse;
mod procgen;
mod project;
pub mod terrain;

pub use overpass::{fetch_map, OverpassQuery};
pub use parse::{fallback_road_name, load_map, sanitize_spawn, DecorProp, FeatureKind, MapData};
pub use procgen::{
    chunk_size_m, generate_bootstrap, generate_chunk, generate_city, ProcgenOptions,
};
pub use terrain::height_at;
