//! Procedural city MapData — finite window or infinite chunk streaming.
//!
//! Global street lattice (`CELL_M`) stitches across chunks; each chunk owns
//! south + west edges of its cells so neighbors don't double-draw roads.

use bevy::prelude::Vec2;

use super::parse::{road_width, sanitize_spawn, DecorProp, FeatureKind, MapData, MapFeature};
use super::project::LocalFrame;
use super::terrain::{biome_at, Biome};

/// Street spacing (meters). Chunk edges snap to this lattice.
pub const CELL_M: f32 = 70.0;
/// Cells per chunk side → chunk ≈ 140 m.
pub const CHUNK_CELLS: i32 = 2;

pub fn chunk_size_m() -> f32 {
    CELL_M * CHUNK_CELLS as f32
}

#[derive(Clone, Copy, Debug)]
pub struct ProcgenOptions {
    pub seed: u64,
    /// Half-extent of a *finite* city in meters (`generate_city`). Ignored by streaming.
    pub radius_m: f32,
    /// When true, main inserts [`crate::world::ProcStream`] instead of baking one mesh.
    pub infinite: bool,
}

impl Default for ProcgenOptions {
    fn default() -> Self {
        Self {
            seed: 42,
            radius_m: 280.0,
            infinite: true,
        }
    }
}

/// Finite city = union of chunks covering `[-radius, radius]`.
pub fn generate_city(opts: ProcgenOptions) -> MapData {
    let frame = LocalFrame::from_origin(4.8357, 45.7640);
    let r = opts.radius_m.clamp(120.0, 900.0);
    let cs = chunk_size_m();
    let n = (r / cs).ceil() as i32;
    let mut features = Vec::new();
    for cz in -n..=n {
        for cx in -n..=n {
            features.extend(generate_chunk(opts.seed, cx, cz));
        }
    }
    let mut map = MapData {
        frame,
        features,
        spawn: Vec2::new(CELL_M * 0.5, CELL_M * 0.5),
    };
    sanitize_spawn(&mut map);
    map
}

/// Bootstrap MapData for infinite mode (traffic / minimap / spawn) — small window only.
pub fn generate_bootstrap(seed: u64, chunk_radius: i32) -> MapData {
    let frame = LocalFrame::from_origin(4.8357, 45.7640);
    let mut features = Vec::new();
    for cz in -chunk_radius..=chunk_radius {
        for cx in -chunk_radius..=chunk_radius {
            features.extend(generate_chunk(seed, cx, cz));
        }
    }
    let mut map = MapData {
        frame,
        features,
        spawn: Vec2::new(CELL_M * 0.5, CELL_M * 0.5),
    };
    sanitize_spawn(&mut map);
    map
}

/// Features for one chunk in world XZ meters.
pub fn generate_chunk(seed: u64, cx: i32, cz: i32) -> Vec<MapFeature> {
    let mut features = Vec::new();
    let i0 = cx * CHUNK_CELLS;
    let j0 = cz * CHUNK_CELLS;

    for dj in 0..CHUNK_CELLS {
        for di in 0..CHUNK_CELLS {
            let i = i0 + di;
            let j = j0 + dj;
            let x0 = i as f32 * CELL_M;
            let z0 = j as f32 * CELL_M;
            let x1 = x0 + CELL_M;
            let z1 = z0 + CELL_M;
            let mid = Vec2::new((x0 + x1) * 0.5, (z0 + z1) * 0.5);
            let biome = biome_at(seed, mid.x, mid.y);

            // South edge (horizontal) — owned by this cell.
            let hw_s = line_class(seed, 0, j);
            if keep_segment(seed, i, j, 0, biome) {
                let road = road_seg(
                    Vec2::new(x0, z0),
                    Vec2::new(x1, z0),
                    hw_s,
                    j,
                    true,
                    seed,
                    biome,
                );
                let pts = road.points.clone();
                features.push(road);
                scatter_roadside_decor(&mut features, &pts, hw_s, seed, i, j);
                push_verge_grass_poly(&mut features, &pts, road_width(hw_s));
            }
            // West edge (vertical).
            let hw_w = line_class(seed, 1, i);
            if keep_segment(seed, i, j, 1, biome) {
                let road = road_seg(
                    Vec2::new(x0, z0),
                    Vec2::new(x0, z1),
                    hw_w,
                    i,
                    false,
                    seed,
                    biome,
                );
                let pts = road.points.clone();
                features.push(road);
                scatter_roadside_decor(&mut features, &pts, hw_w, seed, i, j ^ 0xA5);
                push_verge_grass_poly(&mut features, &pts, road_width(hw_w));
            }

            // Interior block — biome drives density / content.
            let margin = match biome {
                Biome::Urban | Biome::Suburb => 5.5,
                _ => 4.0,
            };
            let inner = Rect {
                min: Vec2::new(x0 + margin, z0 + margin),
                max: Vec2::new(x1 - margin, z1 - margin),
            };
            if inner.width() < 12.0 || inner.depth() < 12.0 {
                continue;
            }
            let mut rng = Rng::new(mix(seed, i as u64, j as u64));
            fill_cell_interior(&mut features, inner, biome, &mut rng);
        }
    }

    features
}

fn fill_cell_interior(features: &mut Vec<MapFeature>, inner: Rect, biome: Biome, rng: &mut Rng) {
    match biome {
        Biome::Urban => {
            let roll = rng.f32();
            if roll < 0.14 {
                features.push(MapFeature {
                    kind: FeatureKind::Green { density: 0.014 },
                    points: rect_ring(inner),
                });
                scatter_trees(features, inner, 0.012, rng);
                scatter_park_decor(features, inner, rng);
            } else if roll < 0.20 {
                scatter_trees(features, inner, 0.004, rng);
            } else {
                fill_block_buildings(features, inner, rng);
            }
        }
        Biome::Suburb => {
            let roll = rng.f32();
            if roll < 0.35 {
                features.push(MapFeature {
                    kind: FeatureKind::Green { density: 0.01 },
                    points: rect_ring(inner),
                });
                scatter_trees(features, inner, 0.01, rng);
            } else if roll < 0.55 {
                scatter_trees(features, inner, 0.006, rng);
            } else {
                fill_block_buildings(features, inner, rng);
            }
        }
        Biome::Field => {
            features.push(MapFeature {
                kind: FeatureKind::Green { density: 0.002 },
                points: rect_ring(inner),
            });
            if rng.f32() < 0.35 {
                scatter_trees(features, inner, 0.0015, rng);
            }
        }
        Biome::Forest => {
            features.push(MapFeature {
                kind: FeatureKind::Green { density: 0.02 },
                points: rect_ring(inner),
            });
            scatter_trees(features, inner, 0.028, rng);
        }
        Biome::Hills => {
            features.push(MapFeature {
                kind: FeatureKind::Green { density: 0.012 },
                points: rect_ring(inner),
            });
            scatter_trees(features, inner, 0.01, rng);
        }
    }
}

fn scatter_roadside_decor(
    features: &mut Vec<MapFeature>,
    pts: &[Vec2],
    highway: &str,
    seed: u64,
    i: i32,
    j: i32,
) {
    if pts.len() < 2 {
        return;
    }
    let a = pts[0];
    let b = pts[pts.len() - 1];
    let len = polyline_len(pts);
    if len < 12.0 {
        return;
    }
    let dir = (b - a).normalize_or_zero();
    let perp = Vec2::new(-dir.y, dir.x);
    let mut rng = Rng::new(mix(seed ^ 0xDEC0, i as u64, j as u64));
    let side = if rng.bool() { 1.0 } else { -1.0 };
    let offset = road_width(highway) * 0.5 + 1.8;

    // Bollard pair near the start of the segment (junction feel).
    if rng.f32() < 0.35 {
        let p = a + dir * 3.5 + perp * offset * side;
        features.push(MapFeature {
            kind: FeatureKind::Decor {
                prop: DecorProp::Bollard,
                yaw: dir.x.atan2(dir.y),
            },
            points: vec![p],
        });
        features.push(MapFeature {
            kind: FeatureKind::Decor {
                prop: DecorProp::Bollard,
                yaw: dir.x.atan2(dir.y),
            },
            points: vec![p + perp * 0.85 * side],
        });
    }

    // Mid-segment street furniture — sample along the bent polyline.
    let mid = sample_polyline(pts, 0.45 + rng.f32() * 0.15);
    let foot = mid + perp * offset * side;
    let yaw = dir.x.atan2(dir.y);
    let roll = rng.f32();
    let prop = if matches!(highway, "primary" | "secondary") && roll < 0.22 {
        DecorProp::BusStop
    } else if roll < 0.35 {
        DecorProp::Bench
    } else if roll < 0.55 {
        DecorProp::Bin
    } else if roll < 0.68 {
        DecorProp::PostBox
    } else if roll < 0.80 {
        DecorProp::BikeRack
    } else if roll < 0.90 {
        DecorProp::Hydrant
    } else {
        return;
    };
    features.push(MapFeature {
        kind: FeatureKind::Decor { prop, yaw },
        points: vec![foot],
    });
}

/// Thin green strips along a (possibly curved) road — feeds 3D grass blades.
/// Ends inset so strips stay clear of square junction pads (`width * 1.25`).
fn push_verge_grass_poly(features: &mut Vec<MapFeature>, pts: &[Vec2], width: f32) {
    if pts.len() < 2 {
        return;
    }
    let len = polyline_len(pts);
    let pad_clear = road_width("primary") * 0.625 + 1.0;
    let inset = (width * 0.75 + 1.5).max(pad_clear).min(len * 0.4);
    if len < inset * 2.0 + 4.0 {
        return;
    }
    let trimmed = trim_polyline(pts, inset);
    if trimmed.len() < 2 {
        return;
    }
    let half = width * 0.5;
    let strip = 1.6;
    for side in [-1.0f32, 1.0] {
        let mut inner = Vec::with_capacity(trimmed.len());
        let mut outer = Vec::with_capacity(trimmed.len());
        for i in 0..trimmed.len() {
            let dir = if i + 1 < trimmed.len() {
                (trimmed[i + 1] - trimmed[i]).normalize_or_zero()
            } else {
                (trimmed[i] - trimmed[i - 1]).normalize_or_zero()
            };
            let perp = Vec2::new(-dir.y, dir.x);
            inner.push(trimmed[i] + perp * ((half + 0.35) * side));
            outer.push(trimmed[i] + perp * ((half + strip) * side));
        }
        let mut ring = inner;
        ring.extend(outer.into_iter().rev());
        features.push(MapFeature {
            kind: FeatureKind::Green { density: 0.0 },
            points: ring,
        });
    }
}

fn polyline_len(pts: &[Vec2]) -> f32 {
    pts.windows(2).map(|w| w[0].distance(w[1])).sum()
}

fn sample_polyline(pts: &[Vec2], t: f32) -> Vec2 {
    let total = polyline_len(pts);
    if total < 1e-4 {
        return pts[0];
    }
    let mut need = t.clamp(0.0, 1.0) * total;
    for w in pts.windows(2) {
        let d = w[0].distance(w[1]);
        if need <= d {
            return w[0].lerp(w[1], need / d.max(1e-6));
        }
        need -= d;
    }
    pts[pts.len() - 1]
}

fn trim_polyline(pts: &[Vec2], inset: f32) -> Vec<Vec2> {
    if pts.len() < 2 || inset <= 0.0 {
        return pts.to_vec();
    }
    let mut out = pts.to_vec();
    let mut need = inset;
    while out.len() >= 2 && need > 1e-4 {
        let d = out[0].distance(out[1]);
        if d <= need + 0.05 {
            need -= d;
            out.remove(0);
        } else {
            let dir = (out[1] - out[0]).normalize_or_zero();
            out[0] += dir * need;
            break;
        }
    }
    need = inset;
    while out.len() >= 2 && need > 1e-4 {
        let n = out.len();
        let d = out[n - 2].distance(out[n - 1]);
        if d <= need + 0.05 {
            need -= d;
            out.pop();
        } else {
            let dir = (out[n - 2] - out[n - 1]).normalize_or_zero();
            let last = out.len() - 1;
            out[last] += dir * need;
            break;
        }
    }
    out
}

fn scatter_park_decor(features: &mut Vec<MapFeature>, area: Rect, rng: &mut Rng) {
    let n = 1 + rng.usize(3);
    for _ in 0..n {
        let p = Vec2::new(
            area.min.x + rng.f32() * area.width(),
            area.min.y + rng.f32() * area.depth(),
        );
        let prop = if rng.f32() < 0.55 {
            DecorProp::Bench
        } else {
            DecorProp::Bin
        };
        features.push(MapFeature {
            kind: FeatureKind::Decor {
                prop,
                yaw: rng.f32() * std::f32::consts::TAU,
            },
            points: vec![p],
        });
    }
}

fn road_seg(
    a: Vec2,
    b: Vec2,
    highway: &'static str,
    line_index: i32,
    east_west: bool,
    seed: u64,
    biome: Biome,
) -> MapFeature {
    let mut rng = Rng::new(mix(seed, line_index as u64, if east_west { 3 } else { 7 }));
    // Arterials bend less; countryside roads snake more. Endpoints stay on the
    // lattice so junctions still meet across chunk borders.
    let mut amp = biome.curve_amp();
    if matches!(highway, "primary") {
        amp *= 0.35;
    } else if matches!(highway, "secondary") {
        amp *= 0.55;
    }
    let points = curved_road_points(a, b, amp, &mut rng);
    MapFeature {
        kind: FeatureKind::Road {
            width: road_width(highway),
            highway: highway.to_string(),
            name: Some(street_name(
                highway,
                line_index.unsigned_abs() as usize,
                east_west,
                &mut rng,
            )),
        },
        points,
    }
}

/// Endpoints fixed; 1–2 mid control points offset on the perpendicular.
fn curved_road_points(a: Vec2, b: Vec2, amp: f32, rng: &mut Rng) -> Vec<Vec2> {
    let dir = b - a;
    let len = dir.length();
    if amp < 0.4 || len < 18.0 {
        return vec![a, b];
    }
    let perp = Vec2::new(-dir.y, dir.x).normalize_or_zero();
    let sign = if rng.bool() { 1.0 } else { -1.0 };
    let bend = (0.45 + rng.f32() * 0.55) * amp * sign;
    if amp > 7.0 && len > 40.0 {
        let p1 = a.lerp(b, 0.28) + perp * (bend * 0.55);
        let p2 = a.lerp(b, 0.55) + perp * bend;
        let p3 = a.lerp(b, 0.78) + perp * (bend * 0.4);
        vec![a, p1, p2, p3, b]
    } else {
        let p1 = a.lerp(b, 0.35) + perp * (bend * 0.65);
        let p2 = a.lerp(b, 0.68) + perp * bend;
        vec![a, p1, p2, b]
    }
}

fn line_class(seed: u64, axis: u8, index: i32) -> &'static str {
    let h = mix(seed, axis as u64, index as u64);
    if index.rem_euclid(6) == 0 {
        "primary"
    } else if index.rem_euclid(3) == 0 {
        "secondary"
    } else if h & 1 == 0 {
        "tertiary"
    } else {
        "residential"
    }
}

fn keep_segment(seed: u64, i: i32, j: i32, edge: u8, biome: Biome) -> bool {
    let h = mix(seed ^ 0xC0FFEE, ((i as u64) << 32) | j as u64, edge as u64);
    let class = if edge == 0 {
        line_class(seed, 0, j)
    } else {
        line_class(seed, 1, i)
    };
    // Primary always kept (country routes). Secondary kept in town.
    match biome {
        Biome::Urban => {
            if class == "primary" || class == "secondary" {
                return true;
            }
            (h % 100) > 10
        }
        Biome::Suburb => {
            if class == "primary" || class == "secondary" {
                return true;
            }
            (h % 100) > 35
        }
        Biome::Field | Biome::Hills => {
            if class == "primary" {
                return true;
            }
            if class == "secondary" {
                return (h % 100) > 40;
            }
            (h % 100) > 78
        }
        Biome::Forest => {
            if class == "primary" {
                return true;
            }
            (h % 100) > 85
        }
    }
}

fn mix(seed: u64, a: u64, b: u64) -> u64 {
    let mut x = seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(a.wrapping_mul(0xBF58_476D_1CE4_E5B9))
        .wrapping_add(b.wrapping_mul(0x94D0_49BB_1331_11EB));
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x.wrapping_mul(0x94D0_49BB_1331_11EB)
}

#[derive(Clone, Copy)]
struct Rect {
    min: Vec2,
    max: Vec2,
}

impl Rect {
    fn width(self) -> f32 {
        self.max.x - self.min.x
    }
    fn depth(self) -> f32 {
        self.max.y - self.min.y
    }
}

fn rect_ring(r: Rect) -> Vec<Vec2> {
    vec![
        Vec2::new(r.min.x, r.min.y),
        Vec2::new(r.max.x, r.min.y),
        Vec2::new(r.max.x, r.max.y),
        Vec2::new(r.min.x, r.max.y),
    ]
}

fn street_name(highway: &str, index: usize, east_west: bool, _rng: &mut Rng) -> String {
    // Deterministic + unique per lattice line so every street has a stable label.
    let prefixes = match highway {
        "primary" => ["Avenue", "Cours", "Grande Rue", "Quai"],
        "secondary" => ["Boulevard", "Route", "Allée", "Cours"],
        "tertiary" => ["Rue", "Chemin", "Passage", "Venelle"],
        _ => ["Rue", "Impasse", "Venelle", "Ruelle"],
    };
    let names = [
        "Lafayette",
        "Victor Hugo",
        "des Lilas",
        "du Commerce",
        "Saint-Jean",
        "de la Gare",
        "des Écoles",
        "Pasteur",
        "du Parc",
        "Neuve",
        "de l'Église",
        "des Acacias",
        "Jean Jaurès",
        "de la République",
        "des Tilleuls",
        "Carnot",
        "des Fleurs",
        "du Marché",
        "Sainte-Claire",
        "des Artisans",
        "de la Mairie",
        "Gambetta",
        "des Peupliers",
        "du Stade",
    ];
    let pref = prefixes[index % prefixes.len()];
    let name = names[index % names.len()];
    let axis = if east_west { "E" } else { "N" };
    // Suffix keeps parallel streets distinct when the name pool wraps.
    format!("{pref} {name} ({axis}{index})")
}

fn fill_block_buildings(features: &mut Vec<MapFeature>, block: Rect, rng: &mut Rng) {
    let along_x = block.width() >= block.depth();
    let long = if along_x {
        block.width()
    } else {
        block.depth()
    };
    let short = if along_x {
        block.depth()
    } else {
        block.width()
    };

    let mut cursor = 0.0;
    while cursor < long - 8.0 {
        let lot = 10.0 + rng.f32() * 14.0;
        let lot_end = (cursor + lot).min(long);
        let lot_len = lot_end - cursor;
        if lot_len < 7.0 {
            break;
        }

        let depth = (short * (0.55 + rng.f32() * 0.3)).clamp(6.0, short - 2.0);
        let gap = 1.2;
        let (min, max) = if along_x {
            let x0 = block.min.x + cursor + gap * 0.5;
            let x1 = block.min.x + lot_end - gap * 0.5;
            let toward_min = rng.bool();
            let z0 = if toward_min {
                block.min.y
            } else {
                block.max.y - depth
            };
            (Vec2::new(x0, z0), Vec2::new(x1, z0 + depth))
        } else {
            let z0 = block.min.y + cursor + gap * 0.5;
            let z1 = block.min.y + lot_end - gap * 0.5;
            let toward_min = rng.bool();
            let x0 = if toward_min {
                block.min.x
            } else {
                block.max.x - depth
            };
            (Vec2::new(x0, z0), Vec2::new(x0 + depth, z1))
        };

        if max.x - min.x > 4.0 && max.y - min.y > 4.0 {
            let levels = 2.0 + rng.f32() * 5.0;
            if rng.f32() < 0.18 && (max.x - min.x) > 12.0 && (max.y - min.y) > 10.0 {
                push_l_building(features, min, max, levels, rng);
            } else {
                features.push(MapFeature {
                    kind: FeatureKind::Building { levels, name: None },
                    points: rect_ring(Rect { min, max }),
                });
            }
        }

        cursor = lot_end;
    }
}

fn push_l_building(
    features: &mut Vec<MapFeature>,
    min: Vec2,
    max: Vec2,
    levels: f32,
    rng: &mut Rng,
) {
    let mid_x = min.x + (max.x - min.x) * (0.45 + rng.f32() * 0.2);
    let mid_z = min.y + (max.y - min.y) * (0.45 + rng.f32() * 0.2);
    features.push(MapFeature {
        kind: FeatureKind::Building { levels, name: None },
        points: rect_ring(Rect {
            min,
            max: Vec2::new(max.x, mid_z),
        }),
    });
    features.push(MapFeature {
        kind: FeatureKind::Building {
            levels: (levels - 0.5).max(2.0),
            name: None,
        },
        points: rect_ring(Rect {
            min: Vec2::new(min.x, mid_z - 0.5),
            max: Vec2::new(mid_x, max.y),
        }),
    });
}

fn scatter_trees(features: &mut Vec<MapFeature>, area: Rect, density: f32, rng: &mut Rng) {
    let area_m2 = area.width() * area.depth();
    let count = ((area_m2 * density) as usize).clamp(1, 24);
    for _ in 0..count {
        let p = Vec2::new(
            area.min.x + rng.f32() * area.width(),
            area.min.y + rng.f32() * area.depth(),
        );
        features.push(MapFeature {
            kind: FeatureKind::Tree {
                scale: 1.0 + rng.f32() * 1.1,
            },
            points: vec![p],
        });
    }
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn usize(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() as usize) % n
    }

    fn bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verge_grass_clears_junction_pads() {
        // Narrow arm: must still clear a primary-sized merged pad.
        let width = road_width("residential");
        let mut feats = Vec::new();
        push_verge_grass_poly(&mut feats, &[Vec2::ZERO, Vec2::new(70.0, 0.0)], width);
        assert_eq!(feats.len(), 2, "both verges");
        let pad_half = road_width("primary") * 1.25 * 0.5;
        for f in &feats {
            assert!(matches!(f.kind, FeatureKind::Green { density: 0.0 }));
            for p in &f.points {
                assert!(
                    p.x >= pad_half - 0.05,
                    "verge enters start junction: {p:?} pad_half={pad_half}"
                );
                assert!(
                    p.x <= 70.0 - pad_half + 0.05,
                    "verge enters end junction: {p:?} pad_half={pad_half}"
                );
            }
        }
    }

    #[test]
    fn roads_can_bend() {
        let mut rng = Rng::new(123);
        let pts = curved_road_points(Vec2::ZERO, Vec2::new(70.0, 0.0), 10.0, &mut rng);
        assert!(pts.len() > 2, "expected mid control points");
        assert_eq!(pts[0], Vec2::ZERO);
        assert_eq!(*pts.last().unwrap(), Vec2::new(70.0, 0.0));
        assert!(
            pts.iter().any(|p| p.y.abs() > 0.5),
            "bend leaves lattice line"
        );
    }

    #[test]
    fn countryside_chunk_has_green_not_only_buildings() {
        // Far chunk should be Field/Forest/Hills.
        let feats = generate_chunk(42, 12, 0);
        let greens = feats
            .iter()
            .filter(|f| matches!(f.kind, FeatureKind::Green { .. }))
            .count();
        let buildings = feats
            .iter()
            .filter(|f| matches!(f.kind, FeatureKind::Building { .. }))
            .count();
        assert!(greens >= 1, "greens={greens}");
        assert!(
            buildings < greens + 5,
            "buildings={buildings} greens={greens}"
        );
    }

    #[test]
    fn generates_roads_and_buildings() {
        let map = generate_city(ProcgenOptions {
            seed: 7,
            radius_m: 200.0,
            infinite: false,
        });
        let roads = map
            .features
            .iter()
            .filter(|f| matches!(f.kind, FeatureKind::Road { .. }))
            .count();
        let buildings = map
            .features
            .iter()
            .filter(|f| matches!(f.kind, FeatureKind::Building { .. }))
            .count();
        let greens = map
            .features
            .iter()
            .filter(|f| matches!(f.kind, FeatureKind::Green { .. }))
            .count();
        assert!(roads > 20, "roads={roads}");
        assert!(buildings > 10, "buildings={buildings}");
        assert!(greens >= 1, "greens={greens}");
    }

    #[test]
    fn same_seed_is_deterministic() {
        let a = generate_city(ProcgenOptions {
            seed: 99,
            radius_m: 180.0,
            infinite: false,
        });
        let b = generate_city(ProcgenOptions {
            seed: 99,
            radius_m: 180.0,
            infinite: false,
        });
        assert_eq!(a.features.len(), b.features.len());
        assert_eq!(a.features[0].points, b.features[0].points);
    }

    #[test]
    fn named_roads_present() {
        let map = generate_city(ProcgenOptions {
            seed: 42,
            radius_m: 200.0,
            infinite: false,
        });
        let named = map
            .features
            .iter()
            .any(|f| matches!(&f.kind, FeatureKind::Road { name: Some(_), .. }));
        assert!(named);
    }

    #[test]
    fn adjacent_chunks_share_edge_without_duplicate_south() {
        let a = generate_chunk(1, 0, 0);
        let b = generate_chunk(1, 0, 1); // north of a
        let z_edge = CELL_M * 2.0;
        // Curved roads leave the lattice mid-span — match on endpoints only.
        let ends_on_edge = |f: &MapFeature| {
            matches!(f.kind, FeatureKind::Road { .. })
                && f.points.len() >= 2
                && (f.points[0].y - z_edge).abs() < 0.01
                && (f.points[f.points.len() - 1].y - z_edge).abs() < 0.01
        };
        let a_on_north: Vec<_> = a.iter().filter(|f| ends_on_edge(f)).collect();
        let b_on_south: Vec<_> = b.iter().filter(|f| ends_on_edge(f)).collect();
        assert!(
            a_on_north.is_empty(),
            "south-owner rule: chunk below must not draw north edge"
        );
        assert!(!b_on_south.is_empty());
    }

    #[test]
    fn chunk_coords_stable() {
        let f = generate_chunk(5, -2, 3);
        assert!(!f.is_empty());
        let cs = chunk_size_m();
        let x0 = -2.0 * cs;
        let z0 = 3.0 * cs;
        // Curves (hills amp≤14) + verge/decor offset — keep features near the chunk.
        let pad = 22.0;
        for feat in &f {
            for p in &feat.points {
                assert!(p.x >= x0 - pad && p.x <= x0 + cs + pad, "x={}", p.x);
                assert!(p.y >= z0 - pad && p.y <= z0 + cs + pad, "z={}", p.y);
            }
        }
    }

    #[test]
    fn urban_chunk_has_street_furniture() {
        let f = generate_chunk(5, 0, 0);
        assert!(
            f.iter()
                .any(|f| matches!(f.kind, FeatureKind::Decor { .. })),
            "urban chunks should scatter street furniture"
        );
    }
}
