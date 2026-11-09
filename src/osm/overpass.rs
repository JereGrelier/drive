//! Fetch roads + buildings from the Overpass API at runtime.

use bevy::prelude::Vec2;
use serde::Deserialize;

use super::parse::{
    decor_from_tags, fallback_road_name, road_width, sanitize_spawn, FeatureKind, MapData,
    MapFeature,
};
use super::project::LocalFrame;

const OVERPASS_URL: &str = "https://overpass-api.de/api/interpreter";

#[derive(Clone, Copy, Debug)]
pub struct OverpassQuery {
    pub lat: f64,
    pub lon: f64,
    pub radius_m: f64,
}

impl Default for OverpassQuery {
    fn default() -> Self {
        Self {
            lat: 45.7640,
            lon: 4.8357,
            radius_m: 350.0,
        }
    }
}

pub fn fetch_map(q: OverpassQuery) -> anyhow::Result<MapData> {
    let south = q.lat - meters_to_lat(q.radius_m);
    let north = q.lat + meters_to_lat(q.radius_m);
    let west = q.lon - meters_to_lon(q.radius_m, q.lat);
    let east = q.lon + meters_to_lon(q.radius_m, q.lat);

    let ql = format!(
        r#"[out:json][timeout:40];
(
  way["highway"]({south},{west},{north},{east});
  way["building"]({south},{west},{north},{east});
  way["leisure"~"park|garden|pitch|playground"]({south},{west},{north},{east});
  way["landuse"~"grass|forest|meadow|orchard|village_green|recreation_ground"]({south},{west},{north},{east});
  way["natural"~"wood|grassland|scrub|heath"]({south},{west},{north},{east});
  node["natural"="tree"]({south},{west},{north},{east});
  node["amenity"~"bench|waste_basket|recycling|bicycle_parking|post_box|bus_station|fire_hydrant"]({south},{west},{north},{east});
  node["emergency"="fire_hydrant"]({south},{west},{north},{east});
  node["highway"="bus_stop"]({south},{west},{north},{east});
  node["public_transport"="platform"]({south},{west},{north},{east});
  node["barrier"~"bollard|block"]({south},{west},{north},{east});
  node["leisure"="picnic_table"]({south},{west},{north},{east});
);
out geom;"#
    );

    eprintln!(
        "Overpass: lat={:.5} lon={:.5} r={:.0}m …",
        q.lat, q.lon, q.radius_m
    );

    let mut response = ureq::post(OVERPASS_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(&format!("data={}", urlencoding_lite(&ql)))?;
    let body: OverpassResponse = response.body_mut().read_json()?;

    parse_overpass(body, q.lon, q.lat)
}

fn parse_overpass(
    body: OverpassResponse,
    origin_lon: f64,
    origin_lat: f64,
) -> anyhow::Result<MapData> {
    let frame = LocalFrame::from_origin(origin_lon, origin_lat);
    let mut features = Vec::new();

    for el in body.elements {
        let tags = el.tags.unwrap_or_default();

        if el.r#type == "node" {
            if let (Some(lat), Some(lon)) = (el.lat, el.lon) {
                let p = frame.project(lon, lat);
                if tags.get("natural").map(String::as_str) == Some("tree") {
                    let scale = tags
                        .get("height")
                        .and_then(|s| s.parse::<f32>().ok())
                        .map(|h| (h / 12.0).clamp(0.7, 2.4))
                        .unwrap_or(1.15);
                    features.push(MapFeature {
                        kind: FeatureKind::Tree { scale },
                        points: vec![p],
                    });
                } else if let Some(prop) = decor_from_tags(&tags) {
                    features.push(MapFeature {
                        kind: FeatureKind::Decor { prop, yaw: 0.0 },
                        points: vec![p],
                    });
                }
            }
            continue;
        }

        if el.r#type != "way" {
            continue;
        }
        let Some(geom) = el.geometry.as_ref().filter(|g| g.len() >= 2) else {
            continue;
        };

        if let Some(highway) = tags.get("highway").cloned() {
            let points: Vec<Vec2> = geom.iter().map(|n| frame.project(n.lon, n.lat)).collect();
            let name =
                osm_tags_label(&tags).or_else(|| Some(fallback_road_name(&highway, &points)));
            features.push(MapFeature {
                kind: FeatureKind::Road {
                    width: road_width(&highway),
                    highway,
                    name,
                },
                points,
            });
        } else if tags.contains_key("building") {
            let levels = tags
                .get("building:levels")
                .and_then(|s| s.parse().ok())
                .unwrap_or(3.0);
            let name = osm_tags_label(&tags);
            let mut points: Vec<Vec2> = geom.iter().map(|n| frame.project(n.lon, n.lat)).collect();
            if points.len() >= 3 {
                if points.first() == points.last() {
                    points.pop();
                }
                if points.len() >= 3 {
                    features.push(MapFeature {
                        kind: FeatureKind::Building { levels, name },
                        points,
                    });
                }
            }
        } else if let Some(density) = overpass_green_density(&tags) {
            let mut points: Vec<Vec2> = geom.iter().map(|n| frame.project(n.lon, n.lat)).collect();
            if points.first() == points.last() {
                points.pop();
            }
            if points.len() >= 3 {
                features.push(MapFeature {
                    kind: FeatureKind::Green { density },
                    points,
                });
            }
        }
    }

    if features.is_empty() {
        anyhow::bail!("Overpass returned no roads/buildings in bbox");
    }

    let mut map = MapData {
        frame,
        features,
        spawn: Vec2::ZERO,
    };
    sanitize_spawn(&mut map);
    Ok(map)
}

#[derive(Deserialize)]
struct OverpassResponse {
    elements: Vec<OverpassElement>,
}

#[derive(Deserialize)]
struct OverpassElement {
    #[serde(rename = "type")]
    r#type: String,
    tags: Option<std::collections::HashMap<String, String>>,
    geometry: Option<Vec<OverpassNode>>,
    /// Present on node elements (Overpass `out geom`).
    lat: Option<f64>,
    lon: Option<f64>,
}

#[derive(Deserialize)]
struct OverpassNode {
    lat: f64,
    lon: f64,
}

fn osm_tags_label(tags: &std::collections::HashMap<String, String>) -> Option<String> {
    tags.get("name")
        .or_else(|| tags.get("name:fr"))
        .or_else(|| tags.get("ref"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn overpass_green_density(tags: &std::collections::HashMap<String, String>) -> Option<f32> {
    let leisure = tags.get("leisure").map(String::as_str);
    let landuse = tags.get("landuse").map(String::as_str);
    let natural = tags.get("natural").map(String::as_str);
    match (leisure, landuse, natural) {
        (Some("park" | "garden"), _, _) => Some(0.012),
        (_, Some("forest" | "orchard"), _) | (_, _, Some("wood")) => Some(0.035),
        (_, Some("grass" | "meadow" | "village_green" | "recreation_ground"), _)
        | (_, _, Some("grassland" | "scrub" | "heath")) => Some(0.008),
        (Some("pitch" | "playground"), _, _) => Some(0.004),
        _ => None,
    }
}

fn meters_to_lat(m: f64) -> f64 {
    m / 111_320.0
}

fn meters_to_lon(m: f64, lat: f64) -> f64 {
    m / (111_320.0 * lat.to_radians().cos().max(0.2))
}

fn urlencoding_lite(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_overpass_ways() {
        let raw = r#"{
          "elements":[
            {"type":"way","tags":{"highway":"residential","name":"Rue Overpass"},
             "geometry":[{"lat":45.76,"lon":4.83},{"lat":45.761,"lon":4.831}]},
            {"type":"way","tags":{"building":"yes","building:levels":"4"},
             "geometry":[
               {"lat":45.7601,"lon":4.8302},{"lat":45.7601,"lon":4.8305},
               {"lat":45.7603,"lon":4.8305},{"lat":45.7603,"lon":4.8302},
               {"lat":45.7601,"lon":4.8302}
             ]}
          ]
        }"#;
        let body: OverpassResponse = serde_json::from_str(raw).unwrap();
        let map = parse_overpass(body, 4.83, 45.76).unwrap();
        assert_eq!(map.features.len(), 2);
        match &map.features[0].kind {
            FeatureKind::Road { name, .. } => {
                assert_eq!(name.as_deref(), Some("Rue Overpass"));
            }
            other => panic!("expected road, got {other:?}"),
        }
    }
}
