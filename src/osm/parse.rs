//! GeoJSON (OSM export) → typed map features in local meters.

use bevy::prelude::Vec2;
use serde::Deserialize;
use std::path::Path;

use super::project::LocalFrame;

/// Street furniture / small OSM props (nodes or procedural).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorProp {
    Bench,
    Bin,
    Bollard,
    BusStop,
    Hydrant,
    PostBox,
    BikeRack,
}

#[derive(Debug, Clone)]
pub enum FeatureKind {
    Building {
        levels: f32,
        #[allow(dead_code)]
        name: Option<String>,
    },
    Road {
        width: f32,
        #[allow(dead_code)]
        highway: String,
        /// OSM `name` / `name:fr` / `ref` when present.
        name: Option<String>,
    },
    /// Park / grass / forest polygon — scatter trees + green ground.
    Green { density: f32 },
    /// Single OSM tree node.
    Tree { scale: f32 },
    /// Bench, bollard, bus stop, etc.
    Decor {
        prop: DecorProp,
        /// Yaw radians around +Y (facing).
        yaw: f32,
    },
}

#[derive(Debug, Clone)]
pub struct MapFeature {
    pub kind: FeatureKind,
    /// Ring or polyline in local XZ meters (Vec2 = x,z).
    pub points: Vec<Vec2>,
}

#[derive(Debug, Clone)]
pub struct MapData {
    #[allow(dead_code)]
    pub frame: LocalFrame,
    pub features: Vec<MapFeature>,
    pub spawn: Vec2,
}

#[derive(Deserialize)]
struct GeoJson {
    features: Vec<GeoFeature>,
}

#[derive(Deserialize)]
struct GeoFeature {
    properties: serde_json::Value,
    geometry: Geometry,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Geometry {
    Polygon {
        coordinates: Vec<Vec<[f64; 2]>>,
    },
    MultiPolygon {
        coordinates: Vec<Vec<Vec<[f64; 2]>>>,
    },
    LineString {
        coordinates: Vec<[f64; 2]>,
    },
    MultiLineString {
        coordinates: Vec<Vec<[f64; 2]>>,
    },
    Point {
        coordinates: [f64; 2],
    },
    #[serde(other)]
    Other,
}

pub fn load_map(path: impl AsRef<Path>) -> anyhow::Result<MapData> {
    let raw = std::fs::read_to_string(path)?;
    parse_geojson(&raw)
}

pub fn parse_geojson(raw: &str) -> anyhow::Result<MapData> {
    let geo: GeoJson = serde_json::from_str(raw)?;
    let origin = estimate_origin(&geo);
    let frame = LocalFrame::from_origin(origin.0, origin.1);

    let mut features = Vec::new();
    let mut spawn = None;

    for feat in &geo.features {
        let props = &feat.properties;
        if props.get("highway").is_some() {
            let highway = props
                .get("highway")
                .and_then(|v| v.as_str())
                .unwrap_or("residential")
                .to_string();
            let width = road_width(&highway);
            let osm_name = osm_label(props);
            for line in lines_of(&feat.geometry) {
                let points: Vec<Vec2> = line.iter().map(|c| frame.project(c[0], c[1])).collect();
                if points.len() >= 2 {
                    let name = osm_name
                        .clone()
                        .or_else(|| Some(fallback_road_name(&highway, &points)));
                    features.push(MapFeature {
                        kind: FeatureKind::Road {
                            width,
                            highway: highway.clone(),
                            name,
                        },
                        points,
                    });
                }
            }
        } else if props.get("building").is_some() {
            let levels = props
                .get("building:levels")
                .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
                .unwrap_or(3.0) as f32;
            let name = osm_label(props);
            for ring in polygons_of(&feat.geometry) {
                let points: Vec<Vec2> = ring.iter().map(|c| frame.project(c[0], c[1])).collect();
                if points.len() >= 3 {
                    features.push(MapFeature {
                        kind: FeatureKind::Building {
                            levels,
                            name: name.clone(),
                        },
                        points,
                    });
                }
            }
        } else if let Some(density) = green_density(props) {
            for ring in polygons_of(&feat.geometry) {
                let points: Vec<Vec2> = ring.iter().map(|c| frame.project(c[0], c[1])).collect();
                if points.len() >= 3 {
                    features.push(MapFeature {
                        kind: FeatureKind::Green { density },
                        points,
                    });
                }
            }
        } else if is_tree_node(props) {
            if let Geometry::Point { coordinates } = &feat.geometry {
                let p = frame.project(coordinates[0], coordinates[1]);
                let scale = props
                    .get("height")
                    .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
                    .map(|h| (h as f32 / 12.0).clamp(0.7, 2.4))
                    .unwrap_or(1.15);
                features.push(MapFeature {
                    kind: FeatureKind::Tree { scale },
                    points: vec![p],
                });
            }
        } else if let Some(prop) = decor_from_props(props) {
            if let Geometry::Point { coordinates } = &feat.geometry {
                let p = frame.project(coordinates[0], coordinates[1]);
                features.push(MapFeature {
                    kind: FeatureKind::Decor { prop, yaw: 0.0 },
                    points: vec![p],
                });
            }
        } else if props.get("spawn").and_then(|v| v.as_bool()) == Some(true) {
            if let Geometry::Point { coordinates } = &feat.geometry {
                spawn = Some(frame.project(coordinates[0], coordinates[1]));
            }
        }
    }

    let spawn = spawn.unwrap_or_else(|| centroid_of_roads(&features));
    let mut map = MapData {
        frame,
        features,
        spawn,
    };
    sanitize_spawn(&mut map);
    Ok(map)
}

fn estimate_origin(geo: &GeoJson) -> (f64, f64) {
    let mut sum_lon = 0.0;
    let mut sum_lat = 0.0;
    let mut n = 0.0;
    for feat in &geo.features {
        for c in all_coords(&feat.geometry) {
            sum_lon += c[0];
            sum_lat += c[1];
            n += 1.0;
        }
    }
    if n == 0.0 {
        (4.8357, 45.7640) // Lyon fallback
    } else {
        (sum_lon / n, sum_lat / n)
    }
}

fn all_coords(geom: &Geometry) -> Vec<[f64; 2]> {
    match geom {
        Geometry::Point { coordinates } => vec![*coordinates],
        Geometry::LineString { coordinates } => coordinates.clone(),
        Geometry::MultiLineString { coordinates } => {
            coordinates.iter().flatten().copied().collect()
        }
        Geometry::Polygon { coordinates } => coordinates.first().cloned().unwrap_or_default(),
        Geometry::MultiPolygon { coordinates } => coordinates
            .iter()
            .filter_map(|p| p.first())
            .flatten()
            .copied()
            .collect(),
        Geometry::Other => vec![],
    }
}

fn lines_of(geom: &Geometry) -> Vec<Vec<[f64; 2]>> {
    match geom {
        Geometry::LineString { coordinates } => vec![coordinates.clone()],
        Geometry::MultiLineString { coordinates } => coordinates.clone(),
        _ => vec![],
    }
}

fn polygons_of(geom: &Geometry) -> Vec<Vec<[f64; 2]>> {
    match geom {
        Geometry::Polygon { coordinates } => coordinates.first().cloned().into_iter().collect(),
        Geometry::MultiPolygon { coordinates } => coordinates
            .iter()
            .filter_map(|poly| poly.first().cloned())
            .collect(),
        _ => vec![],
    }
}

fn green_density(props: &serde_json::Value) -> Option<f32> {
    let leisure = props.get("leisure").and_then(|v| v.as_str());
    let landuse = props.get("landuse").and_then(|v| v.as_str());
    let natural = props.get("natural").and_then(|v| v.as_str());
    match (leisure, landuse, natural) {
        (Some("park" | "garden"), _, _) => Some(0.012),
        (_, Some("forest" | "orchard"), _) | (_, _, Some("wood")) => Some(0.035),
        (_, Some("grass" | "meadow" | "village_green" | "recreation_ground"), _)
        | (_, _, Some("grassland" | "scrub" | "heath")) => Some(0.008),
        (Some("pitch" | "playground"), _, _) => Some(0.004),
        _ => None,
    }
}

fn is_tree_node(props: &serde_json::Value) -> bool {
    props.get("natural").and_then(|v| v.as_str()) == Some("tree") || props.get("tree").is_some()
}

/// Map common OSM amenity / barrier / highway tags → decor props.
pub fn decor_from_props(props: &serde_json::Value) -> Option<DecorProp> {
    let amenity = props.get("amenity").and_then(|v| v.as_str());
    let barrier = props.get("barrier").and_then(|v| v.as_str());
    let highway = props.get("highway").and_then(|v| v.as_str());
    let emergency = props.get("emergency").and_then(|v| v.as_str());
    let leisure = props.get("leisure").and_then(|v| v.as_str());

    if amenity == Some("bench") || leisure == Some("picnic_table") {
        return Some(DecorProp::Bench);
    }
    if matches!(
        amenity,
        Some("waste_basket" | "recycling" | "waste_disposal")
    ) {
        return Some(DecorProp::Bin);
    }
    if amenity == Some("bicycle_parking") {
        return Some(DecorProp::BikeRack);
    }
    if amenity == Some("post_box") {
        return Some(DecorProp::PostBox);
    }
    if emergency == Some("fire_hydrant") || amenity == Some("fire_hydrant") {
        return Some(DecorProp::Hydrant);
    }
    if matches!(highway, Some("bus_stop"))
        || props.get("public_transport").and_then(|v| v.as_str()) == Some("platform")
        || amenity == Some("bus_station")
    {
        return Some(DecorProp::BusStop);
    }
    if matches!(barrier, Some("bollard" | "block" | "kerb")) {
        return Some(DecorProp::Bollard);
    }
    None
}

pub fn decor_from_tags(tags: &std::collections::HashMap<String, String>) -> Option<DecorProp> {
    let get = |k: &str| tags.get(k).map(String::as_str);
    if get("amenity") == Some("bench") || get("leisure") == Some("picnic_table") {
        return Some(DecorProp::Bench);
    }
    if matches!(
        get("amenity"),
        Some("waste_basket" | "recycling" | "waste_disposal")
    ) {
        return Some(DecorProp::Bin);
    }
    if get("amenity") == Some("bicycle_parking") {
        return Some(DecorProp::BikeRack);
    }
    if get("amenity") == Some("post_box") {
        return Some(DecorProp::PostBox);
    }
    if get("emergency") == Some("fire_hydrant") || get("amenity") == Some("fire_hydrant") {
        return Some(DecorProp::Hydrant);
    }
    if get("highway") == Some("bus_stop")
        || get("public_transport") == Some("platform")
        || get("amenity") == Some("bus_station")
    {
        return Some(DecorProp::BusStop);
    }
    if matches!(get("barrier"), Some("bollard" | "block" | "kerb")) {
        return Some(DecorProp::Bollard);
    }
    None
}

/// Prefer `name`, then `name:fr`, then route `ref` (e.g. D306).
pub(crate) fn osm_label(props: &serde_json::Value) -> Option<String> {
    props
        .get("name")
        .or_else(|| props.get("name:fr"))
        .or_else(|| props.get("ref"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Synthetic label when OSM has no name — every road stays labeled.
pub fn fallback_road_name(highway: &str, points: &[Vec2]) -> String {
    let mid = points
        .first()
        .copied()
        .unwrap_or(Vec2::ZERO)
        .lerp(points.last().copied().unwrap_or(Vec2::ZERO), 0.5);
    let id = ((mid.x * 0.2) as i32)
        .wrapping_mul(73856093)
        .wrapping_add((mid.y * 0.2) as i32)
        .wrapping_mul(19349663)
        .unsigned_abs();
    let kind = match highway {
        "motorway" | "trunk" | "primary" => "Avenue",
        "secondary" => "Boulevard",
        "tertiary" => "Rue",
        _ => "Rue",
    };
    format!("{kind} {id}")
}

pub(crate) fn road_width(highway: &str) -> f32 {
    match highway {
        "motorway" | "trunk" => 12.0,
        "primary" => 10.0,
        "secondary" => 8.0,
        "tertiary" => 7.0,
        "residential" | "unclassified" | "living_street" => 6.0,
        "service" | "track" => 4.0,
        "footway" | "path" | "cycleway" => 2.5,
        _ => 6.0,
    }
}

fn centroid_of_roads(features: &[MapFeature]) -> Vec2 {
    let mut sum = Vec2::ZERO;
    let mut n = 0.0;
    for f in features {
        if matches!(f.kind, FeatureKind::Road { .. }) {
            for p in &f.points {
                sum += *p;
                n += 1.0;
            }
        }
    }
    if n == 0.0 {
        Vec2::ZERO
    } else {
        sum / n
    }
}

/// Move `map.spawn` out of any building footprint (with a small car clearance).
pub fn sanitize_spawn(map: &mut MapData) {
    if !spawn_blocked(map.spawn, &map.features) {
        return;
    }
    if let Some(p) = clear_point_on_roads(&map.features) {
        map.spawn = p;
        return;
    }
    map.spawn = spiral_clear(map.spawn, &map.features);
}

fn spawn_blocked(p: Vec2, features: &[MapFeature]) -> bool {
    // Car footprint ≈ 2 m — reject if any sample sits inside a building.
    const CLEAR: f32 = 1.4;
    for d in [
        Vec2::ZERO,
        Vec2::X,
        Vec2::NEG_X,
        Vec2::Y,
        Vec2::NEG_Y,
        Vec2::new(0.7, 0.7),
        Vec2::new(-0.7, 0.7),
        Vec2::new(0.7, -0.7),
        Vec2::new(-0.7, -0.7),
    ] {
        if point_in_any_building(p + d * CLEAR, features) {
            return true;
        }
    }
    false
}

fn point_in_any_building(p: Vec2, features: &[MapFeature]) -> bool {
    for f in features {
        if !matches!(f.kind, FeatureKind::Building { .. }) {
            continue;
        }
        if f.points.len() >= 3 && point_in_polygon(p, &f.points) {
            return true;
        }
    }
    false
}

fn clear_point_on_roads(features: &[MapFeature]) -> Option<Vec2> {
    let mut best: Option<(Vec2, f32)> = None;
    for f in features {
        let FeatureKind::Road { .. } = f.kind else {
            continue;
        };
        if f.points.len() < 2 {
            continue;
        }
        // Sample mid + quarter points along each segment.
        for w in f.points.windows(2) {
            for t in [0.25, 0.5, 0.75] {
                let p = w[0].lerp(w[1], t);
                if spawn_blocked(p, features) {
                    continue;
                }
                let score = w[0].distance(w[1]);
                if best.is_none_or(|(_, s)| score > s) {
                    best = Some((p, score));
                }
            }
        }
    }
    best.map(|(p, _)| p)
}

fn spiral_clear(origin: Vec2, features: &[MapFeature]) -> Vec2 {
    for ring in 1..40 {
        let r = ring as f32 * 3.0;
        let steps = (6 + ring * 2).min(24);
        for i in 0..steps {
            let a = (i as f32 / steps as f32) * std::f32::consts::TAU;
            let p = origin + Vec2::new(a.cos(), a.sin()) * r;
            if !spawn_blocked(p, features) {
                return p;
            }
        }
    }
    origin
}

fn point_in_polygon(p: Vec2, ring: &[Vec2]) -> bool {
    // Ray cast; treat open or closed rings the same.
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let pi = ring[i];
        let pj = ring[j];
        let intersect = ((pi.y > p.y) != (pj.y > p.y))
            && (p.x < (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y + 1e-12) + pi.x);
        if intersect {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_geojson() {
        let raw = r#"{
          "type":"FeatureCollection",
          "features":[
            {"type":"Feature","properties":{"highway":"residential","name":"Rue Test"},
             "geometry":{"type":"LineString","coordinates":[[4.83,45.76],[4.831,45.76]]}},
            {"type":"Feature","properties":{"building":"yes","building:levels":"4"},
             "geometry":{"type":"Polygon","coordinates":[[[4.8302,45.7601],[4.8305,45.7601],[4.8305,45.7603],[4.8302,45.7603],[4.8302,45.7601]]]}}
          ]
        }"#;
        let map = parse_geojson(raw).unwrap();
        assert_eq!(map.features.len(), 2);
        match &map.features[0].kind {
            FeatureKind::Road { name, .. } => {
                assert_eq!(name.as_deref(), Some("Rue Test"));
            }
            other => panic!("expected road, got {other:?}"),
        }
        assert!(matches!(
            map.features[1].kind,
            FeatureKind::Building { levels: 4.0, .. }
        ));
    }

    #[test]
    fn road_label_falls_back_to_ref() {
        let props = serde_json::json!({"highway":"primary","ref":"D306"});
        assert_eq!(osm_label(&props).as_deref(), Some("D306"));
        let named = serde_json::json!({"name":"Rue A","ref":"D1"});
        assert_eq!(osm_label(&named).as_deref(), Some("Rue A"));
    }

    #[test]
    fn decor_from_amenity_tags() {
        let bench = serde_json::json!({"amenity":"bench"});
        assert_eq!(decor_from_props(&bench), Some(DecorProp::Bench));
        let bollard = serde_json::json!({"barrier":"bollard"});
        assert_eq!(decor_from_props(&bollard), Some(DecorProp::Bollard));
        let bus = serde_json::json!({"highway":"bus_stop"});
        assert_eq!(decor_from_props(&bus), Some(DecorProp::BusStop));
    }

    #[test]
    fn sanitize_moves_spawn_out_of_building() {
        let mut map = MapData {
            frame: LocalFrame::from_origin(0.0, 0.0),
            features: vec![
                MapFeature {
                    kind: FeatureKind::Building {
                        levels: 2.0,
                        name: None,
                    },
                    points: vec![
                        Vec2::new(-5.0, -5.0),
                        Vec2::new(5.0, -5.0),
                        Vec2::new(5.0, 5.0),
                        Vec2::new(-5.0, 5.0),
                    ],
                },
                MapFeature {
                    kind: FeatureKind::Road {
                        width: 6.0,
                        highway: "residential".into(),
                        name: None,
                    },
                    points: vec![Vec2::new(20.0, 0.0), Vec2::new(40.0, 0.0)],
                },
            ],
            spawn: Vec2::ZERO, // inside the building
        };
        sanitize_spawn(&mut map);
        assert!(
            !spawn_blocked(map.spawn, &map.features),
            "spawn still blocked at {:?}",
            map.spawn
        );
        assert!(map.spawn.x > 10.0);
    }

    #[test]
    fn parses_park_and_tree() {
        let raw = r#"{
          "type":"FeatureCollection",
          "features":[
            {"type":"Feature","properties":{"leisure":"park"},
             "geometry":{"type":"Polygon","coordinates":[[[4.83,45.76],[4.831,45.76],[4.831,45.761],[4.83,45.761],[4.83,45.76]]]}},
            {"type":"Feature","properties":{"natural":"tree"},
             "geometry":{"type":"Point","coordinates":[4.8305,45.7605]}}
          ]
        }"#;
        let map = parse_geojson(raw).unwrap();
        assert_eq!(map.features.len(), 2);
        assert!(matches!(map.features[0].kind, FeatureKind::Green { .. }));
        assert!(matches!(map.features[1].kind, FeatureKind::Tree { .. }));
    }
}
