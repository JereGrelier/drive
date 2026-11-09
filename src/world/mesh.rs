//! Procedural meshes: extruded buildings + road ribbons.

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;

#[derive(Default)]
pub struct MeshBuf {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl MeshBuf {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn into_mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

#[cfg(test)]
fn building_mesh(points: &[Vec2], height: f32) -> Mesh {
    let mut walls = MeshBuf::default();
    let mut roof = MeshBuf::default();
    append_building(&mut walls, &mut roof, points, height, 0.0);
    walls.into_mesh()
}

/// Append wall and roof geometry for one building footprint.
/// `base_y` is ground height under the building (flat slab on relief).
pub fn append_building(
    walls: &mut MeshBuf,
    roof: &mut MeshBuf,
    points: &[Vec2],
    height: f32,
    base_y: f32,
) {
    let ring = close_ring(points);
    let n = ring.len();
    if n < 3 {
        return;
    }
    let y0 = base_y;
    let y1 = base_y + height;

    for i in 0..n {
        let a = ring[i];
        let b = ring[(i + 1) % n];
        let edge = b - a;
        let len = edge.length().max(0.001);
        let outward = Vec3::new(edge.y / len, 0.0, -edge.x / len);

        let base = walls.positions.len() as u32;
        // Facade atlas tiles 2 bays × 2 floors (~3.5m × 3.2m each).
        let u0 = 0.0;
        let u1 = len / 7.0;
        let v1 = height / 6.4;
        let corners = [
            (Vec3::new(a.x, y0, a.y), [u0, 0.0]),
            (Vec3::new(b.x, y0, b.y), [u1, 0.0]),
            (Vec3::new(b.x, y1, b.y), [u1, v1]),
            (Vec3::new(a.x, y1, a.y), [u0, v1]),
        ];
        for (p, uv) in corners {
            walls.positions.push(p.to_array());
            walls.normals.push(outward.to_array());
            walls.uvs.push(uv);
        }
        walls
            .indices
            .extend([base, base + 2, base + 1, base, base + 3, base + 2]);
    }

    let roof_tris = earcut(&ring);
    let roof_start = roof.positions.len() as u32;
    // Local UVs: ~2.5 m per texture repeat — coarse enough to read on the minimap.
    let mut rmin = ring[0];
    for p in &ring {
        rmin = rmin.min(*p);
    }
    const ROOF_UV_PER_M: f32 = 0.4;
    for p in &ring {
        roof.positions.push([p.x, y1 + 0.02, p.y]);
        roof.normals.push([0.0, 1.0, 0.0]);
        roof.uvs.push([
            (p.x - rmin.x) * ROOF_UV_PER_M,
            (p.y - rmin.y) * ROOF_UV_PER_M,
        ]);
    }
    for [i, j, k] in roof_tris {
        roof.indices
            .extend([roof_start + i, roof_start + j, roof_start + k]);
    }
}

/// Road ribbon. Ends are inset by ~half width so crossing ways don't z-fight;
/// pair with [`append_road_junction`] at shared endpoints.
/// `y_of` samples terrain height so asphalt follows relief.
pub fn append_road(buf: &mut MeshBuf, points: &[Vec2], width: f32, y_of: &dyn Fn(Vec2) -> f32) {
    if points.len() < 2 {
        return;
    }
    // Cap inset so short stubs still draw (else trim collapses to <2 pts).
    let path_len: f32 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
    let inset = (width * 0.55).min(path_len * 0.35);
    let trimmed = trim_polyline_ends(points, inset);
    let trimmed = if trimmed.len() >= 2 {
        trimmed
    } else {
        points.to_vec()
    };
    let half = width * 0.5;
    let mut left: Vec<Vec2> = Vec::new();
    let mut right: Vec<Vec2> = Vec::new();
    let mut dist = vec![0.0f32; trimmed.len()];

    for i in 0..trimmed.len() {
        if i > 0 {
            dist[i] = dist[i - 1] + trimmed[i].distance(trimmed[i - 1]);
        }
        let dir = if i == 0 {
            (trimmed[1] - trimmed[0]).normalize_or_zero()
        } else if i + 1 == trimmed.len() {
            (trimmed[i] - trimmed[i - 1]).normalize_or_zero()
        } else {
            let d0 = (trimmed[i] - trimmed[i - 1]).normalize_or_zero();
            let d1 = (trimmed[i + 1] - trimmed[i]).normalize_or_zero();
            (d0 + d1).normalize_or_zero()
        };
        let perp = Vec2::new(-dir.y, dir.x) * half;
        left.push(trimmed[i] + perp);
        right.push(trimmed[i] - perp);
    }

    let base = buf.positions.len() as u32;
    for i in 0..trimmed.len() {
        let v = dist[i] / 8.0;
        let yl = y_of(left[i]) + 0.08;
        let yr = y_of(right[i]) + 0.08;
        buf.positions.push([left[i].x, yl, left[i].y]);
        buf.normals.push([0.0, 1.0, 0.0]);
        buf.uvs.push([0.0, v]);
        buf.positions.push([right[i].x, yr, right[i].y]);
        buf.normals.push([0.0, 1.0, 0.0]);
        buf.uvs.push([1.0, v]);
    }

    for i in 0..(trimmed.len() - 1) {
        let b = base + (i * 2) as u32;
        // CCW when viewed from +Y so the asphalt faces up (was CW → culled → invisible roads).
        buf.indices.extend([b, b + 2, b + 1, b + 1, b + 2, b + 3]);
    }
}

/// Solid asphalt square at a junction (above ribbons) — no center-line UVs.
pub fn append_road_junction(
    buf: &mut MeshBuf,
    center: Vec2,
    size: f32,
    y_of: &dyn Fn(Vec2) -> f32,
) {
    // Cover ribbon insets (~0.55×width each arm) with margin.
    let half = (size * 0.5).max(1.0);
    let base = buf.positions.len() as u32;
    // Sample asphalt-only band of the road texture (between edge stripe and center dashes).
    let u0 = 0.18;
    let u1 = 0.38;
    let v0 = 0.0;
    let v1 = (size / 8.0).max(0.5);
    let corners = [
        (Vec2::new(center.x - half, center.y - half), [u0, v0]),
        (Vec2::new(center.x + half, center.y - half), [u1, v0]),
        (Vec2::new(center.x + half, center.y + half), [u1, v1]),
        (Vec2::new(center.x - half, center.y + half), [u0, v1]),
    ];
    let _ = center; // junctions stay axis-aligned; corners sample their own height
    for (p, uv) in corners {
        buf.positions.push([p.x, y_of(p) + 0.1, p.y]);
        buf.normals.push([0.0, 1.0, 0.0]);
        buf.uvs.push(uv);
    }
    // CCW from +Y (was CW → culled → green grass showing through junctions).
    buf.indices
        .extend([base, base + 2, base + 1, base, base + 3, base + 2]);
}

/// Pull both ends of a polyline inward by `inset` meters along the path.
pub fn trim_polyline_ends(points: &[Vec2], inset: f32) -> Vec<Vec2> {
    if points.len() < 2 || inset <= 0.0 {
        return points.to_vec();
    }
    let mut out = points.to_vec();
    // Start
    let mut need = inset;
    while out.len() >= 2 && need > 1e-4 {
        let d = out[0].distance(out[1]);
        if d <= need + 0.05 {
            need -= d;
            out.remove(0);
        } else {
            let dir = (out[1] - out[0]).normalize_or_zero();
            out[0] += dir * need;
            need = 0.0;
        }
    }
    // End
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
            need = 0.0;
        }
    }
    out
}

/// Flat grass patch from a green polygon footprint.
pub fn append_grass(buf: &mut MeshBuf, points: &[Vec2], y_of: &dyn Fn(Vec2) -> f32) {
    let ring = close_ring(points);
    if ring.len() < 3 {
        return;
    }
    let tris = earcut(&ring);
    let base = buf.positions.len() as u32;
    for p in &ring {
        buf.positions.push([p.x, y_of(*p) + 0.04, p.y]);
        buf.normals.push([0.0, 1.0, 0.0]);
        buf.uvs.push([p.x * 0.2, p.y * 0.2]);
    }
    for [i, j, k] in tris {
        buf.indices.extend([base + i, base + j, base + k]);
    }
}

/// Crossed-card 3D grass tufts inside a polygon (double-sided material).
pub fn append_grass_blades(buf: &mut MeshBuf, points: &[Vec2], y_of: &dyn Fn(Vec2) -> f32) {
    const SPACING: f32 = 1.45;
    const MAX_TUFTS: usize = 280;
    let ring = close_ring(points);
    if ring.len() < 3 {
        return;
    }
    let mut min = Vec2::splat(f32::MAX);
    let mut max = Vec2::splat(f32::MIN);
    for p in &ring {
        min = min.min(*p);
        max = max.max(*p);
    }
    let mut count = 0usize;
    let mut gy = min.y;
    let mut row = 0u32;
    while gy <= max.y && count < MAX_TUFTS {
        let mut gx = min.x
            + if row.is_multiple_of(2) {
                0.0
            } else {
                SPACING * 0.5
            };
        let mut col = 0u32;
        while gx <= max.x && count < MAX_TUFTS {
            let jx = ((hash_u32(row, col) % 1000) as f32 / 1000.0 - 0.5) * SPACING * 0.7;
            let jy = ((hash_u32(col, row) % 1000) as f32 / 1000.0 - 0.5) * SPACING * 0.7;
            let p = Vec2::new(gx + jx, gy + jy);
            if point_in_ring(p, &ring) {
                let h = hash_u32(row.wrapping_mul(17), col.wrapping_mul(23));
                let scale = 0.75 + (h % 60) as f32 * 0.01;
                let yaw = (h % 360) as f32 * (std::f32::consts::TAU / 360.0);
                append_grass_tuft(buf, p, scale, yaw, y_of(p));
                count += 1;
            }
            gx += SPACING;
            col += 1;
        }
        gy += SPACING;
        row += 1;
    }
}

/// Three vertical cards at 60° — reads as a tuft from any angle.
fn append_grass_tuft(buf: &mut MeshBuf, pos: Vec2, scale: f32, yaw: f32, base_y: f32) {
    let h = (0.28 + 0.22 * scale).clamp(0.22, 0.7);
    let half_w = 0.09 * scale;
    for i in 0..3 {
        let a = yaw + i as f32 * (std::f32::consts::TAU / 3.0);
        let (s, c) = (a.sin(), a.cos());
        let r = Vec3::new(c, 0.0, s) * half_w;
        let lean = ((i + 1) as f32 * 0.03) * scale;
        let tip = Vec3::new(pos.x + lean * c, base_y + h, pos.y + lean * s);
        let bl = Vec3::new(pos.x, base_y + 0.02, pos.y) - r;
        let br = Vec3::new(pos.x, base_y + 0.02, pos.y) + r;
        // Tip + two base corners (thin triangle blade).
        let n = Vec3::new(-s, 0.35, c).normalize_or_zero();
        let base = buf.positions.len() as u32;
        for (p, uv) in [(tip, [0.5, 0.0]), (bl, [0.0, 1.0]), (br, [1.0, 1.0])] {
            buf.positions.push(p.to_array());
            buf.normals.push(n.to_array());
            buf.uvs.push(uv);
        }
        buf.indices.extend([base, base + 1, base + 2]);
        // Back face for solidity without relying only on cull_mode.
        let base2 = buf.positions.len() as u32;
        let n2 = (-n).to_array();
        for (p, uv) in [(tip, [0.5, 0.0]), (br, [1.0, 1.0]), (bl, [0.0, 1.0])] {
            buf.positions.push(p.to_array());
            buf.normals.push(n2);
            buf.uvs.push(uv);
        }
        buf.indices.extend([base2, base2 + 1, base2 + 2]);
    }
}

/// Road sign face style — maps to a Palette material slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignKind {
    Stop = 0,
    Yield = 1,
    Speed30 = 2,
    Speed50 = 3,
    Info = 4,
}

impl SignKind {
    pub const COUNT: usize = 5;
}

/// Place roadside signs along a polyline. Returns (foot, facing_dir, kind).
pub fn plan_road_signs(points: &[Vec2], width: f32, highway: &str) -> Vec<(Vec2, Vec2, SignKind)> {
    if points.len() < 2 {
        return vec![];
    }
    let arterial = matches!(
        highway,
        "motorway" | "trunk" | "primary" | "secondary" | "tertiary"
    );
    let spacing = if arterial { 70.0 } else { 95.0 };
    let side = width * 0.5 + 1.35;
    let mut out = Vec::new();
    let mut dist = 0.0f32;
    // First candidate a bit after the start (near junctions → stop/yield).
    let mut next =
        6.0 + (hash_u32((points[0].x * 4.0) as u32, (points[0].y * 4.0) as u32) % 11) as f32;

    for w in points.windows(2) {
        let a = w[0];
        let b = w[1];
        let seg = (b - a).length();
        if seg < 1e-3 {
            continue;
        }
        let dir = (b - a) / seg;
        let perp = Vec2::new(-dir.y, dir.x);
        let mut t = 0.0;
        while t < seg {
            if dist + t >= next {
                let p = a + dir * t;
                let h = hash_u32((p.x * 10.0) as u32, (p.y * 10.0) as u32);
                // Skip some so roads aren't a forest of signs.
                let keep = if arterial {
                    !h.is_multiple_of(5)
                } else {
                    !h.is_multiple_of(3)
                };
                if keep {
                    let kind = if next < 18.0 {
                        // Near junction: stop / yield / priority feel.
                        match h % 3 {
                            0 => SignKind::Stop,
                            1 => SignKind::Yield,
                            _ => SignKind::Speed30,
                        }
                    } else if arterial {
                        match h % 4 {
                            0 => SignKind::Speed50,
                            1 => SignKind::Info,
                            2 => SignKind::Speed30,
                            _ => SignKind::Yield,
                        }
                    } else {
                        match h % 3 {
                            0 => SignKind::Speed30,
                            1 => SignKind::Info,
                            _ => SignKind::Stop,
                        }
                    };
                    let side_sign = if h.is_multiple_of(2) { 1.0 } else { -1.0 };
                    out.push((p + perp * side * side_sign, dir, kind));
                }
                next += spacing + (h % 17) as f32;
            }
            t += 1.5;
        }
        dist += seg;
        if out.len() >= 12 {
            break;
        }
    }

    // Every street gets at least one blue name plate at mid-segment.
    if !out.iter().any(|(_, _, k)| *k == SignKind::Info) {
        let total: f32 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
        if total > 4.0 {
            let mut acc = 0.0;
            let half = total * 0.5;
            for w in points.windows(2) {
                let a = w[0];
                let b = w[1];
                let seg = a.distance(b);
                if acc + seg >= half {
                    let t = ((half - acc) / seg.max(1e-4)).clamp(0.0, 1.0);
                    let p = a.lerp(b, t);
                    let dir = (b - a).normalize_or_zero();
                    let perp = Vec2::new(-dir.y, dir.x);
                    out.push((p + perp * side, dir, SignKind::Info));
                    break;
                }
                acc += seg;
            }
        }
    }
    out
}

/// Metal pole + sign face facing `dir` (traffic travel direction).
pub fn append_road_sign(
    pole: &mut MeshBuf,
    faces: &mut [MeshBuf],
    foot: Vec2,
    dir: Vec2,
    kind: SignKind,
    base_y: f32,
) {
    let d = dir.normalize_or_zero();
    let forward = if d.length_squared() < 1e-6 {
        Vec2::Y
    } else {
        d
    };
    // Face oncoming traffic.
    let face_n = -forward;
    let right = Vec2::new(-face_n.y, face_n.x);

    let pole_h = 2.85;
    append_taper_cylinder(
        pole,
        Vec3::new(foot.x, base_y, foot.y),
        0.045,
        0.035,
        pole_h,
    );

    let center =
        Vec3::new(foot.x, base_y + 2.35, foot.y) + Vec3::new(face_n.x, 0.0, face_n.y) * 0.06;
    let face_buf = &mut faces[kind as usize];
    match kind {
        SignKind::Stop => append_sign_octagon(face_buf, center, face_n, right, 0.38),
        SignKind::Yield => append_sign_triangle(face_buf, center, face_n, right, 0.42, true),
        SignKind::Speed30 | SignKind::Speed50 => {
            append_sign_disc(face_buf, center, face_n, right, 0.34);
        }
        SignKind::Info => append_sign_rect(face_buf, center, face_n, right, 0.72, 0.28),
    }
}

fn append_sign_rect(
    buf: &mut MeshBuf,
    c: Vec3,
    forward: Vec2,
    right: Vec2,
    half_w: f32,
    half_h: f32,
) {
    let n = Vec3::new(forward.x, 0.0, forward.y);
    let r = Vec3::new(right.x, 0.0, right.y);
    let u = Vec3::Y;
    let depth = 0.03;
    // Front quad + thin back (double-sided read).
    for sign in [1.0f32, -1.0] {
        let o = c + n * (depth * 0.5 * sign);
        let nn = n * sign;
        let corners = [
            o - r * half_w - u * half_h,
            o + r * half_w - u * half_h,
            o + r * half_w + u * half_h,
            o - r * half_w + u * half_h,
        ];
        let base = buf.positions.len() as u32;
        let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        for (i, p) in corners.iter().enumerate() {
            buf.positions.push(p.to_array());
            buf.normals.push(nn.to_array());
            buf.uvs.push(uvs[i]);
        }
        if sign > 0.0 {
            buf.indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        } else {
            buf.indices
                .extend([base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
}

fn append_sign_disc(buf: &mut MeshBuf, c: Vec3, forward: Vec2, right: Vec2, radius: f32) {
    const SEGS: usize = 12;
    let n = Vec3::new(forward.x, 0.0, forward.y);
    let r = Vec3::new(right.x, 0.0, right.y);
    let u = Vec3::Y;
    let o = c + n * 0.02;
    let center_i = buf.positions.len() as u32;
    buf.positions.push(o.to_array());
    buf.normals.push(n.to_array());
    buf.uvs.push([0.5, 0.5]);
    let ring = buf.positions.len() as u32;
    for s in 0..SEGS {
        let a = (s as f32 / SEGS as f32) * std::f32::consts::TAU;
        let p = o + r * (a.cos() * radius) + u * (a.sin() * radius);
        buf.positions.push(p.to_array());
        buf.normals.push(n.to_array());
        buf.uvs.push([0.5 + a.cos() * 0.5, 0.5 - a.sin() * 0.5]);
    }
    for s in 0..SEGS {
        let i0 = ring + s as u32;
        let i1 = ring + ((s + 1) % SEGS) as u32;
        buf.indices.extend([center_i, i0, i1]);
    }
}

fn append_sign_triangle(
    buf: &mut MeshBuf,
    c: Vec3,
    forward: Vec2,
    right: Vec2,
    size: f32,
    point_down: bool,
) {
    let n = Vec3::new(forward.x, 0.0, forward.y);
    let r = Vec3::new(right.x, 0.0, right.y);
    let u = Vec3::Y;
    let o = c + n * 0.02;
    let (p0, p1, p2) = if point_down {
        (
            o + u * size * 0.55,
            o - r * size * 0.55 - u * size * 0.45,
            o + r * size * 0.55 - u * size * 0.45,
        )
    } else {
        (
            o - u * size * 0.55,
            o - r * size * 0.55 + u * size * 0.45,
            o + r * size * 0.55 + u * size * 0.45,
        )
    };
    let base = buf.positions.len() as u32;
    for (p, uv) in [(p0, [0.5, 0.0]), (p1, [0.0, 1.0]), (p2, [1.0, 1.0])] {
        buf.positions.push(p.to_array());
        buf.normals.push(n.to_array());
        buf.uvs.push(uv);
    }
    buf.indices.extend([base, base + 1, base + 2]);
}

fn append_sign_octagon(buf: &mut MeshBuf, c: Vec3, forward: Vec2, right: Vec2, radius: f32) {
    const SEGS: usize = 8;
    let n = Vec3::new(forward.x, 0.0, forward.y);
    let r = Vec3::new(right.x, 0.0, right.y);
    let u = Vec3::Y;
    let o = c + n * 0.02;
    let center_i = buf.positions.len() as u32;
    buf.positions.push(o.to_array());
    buf.normals.push(n.to_array());
    buf.uvs.push([0.5, 0.5]);
    let ring = buf.positions.len() as u32;
    // Flat-top octagon.
    let rot = std::f32::consts::PI / 8.0;
    for s in 0..SEGS {
        let a = rot + (s as f32 / SEGS as f32) * std::f32::consts::TAU;
        let p = o + r * (a.cos() * radius) + u * (a.sin() * radius);
        buf.positions.push(p.to_array());
        buf.normals.push(n.to_array());
        buf.uvs.push([0.5 + a.cos() * 0.5, 0.5 - a.sin() * 0.5]);
    }
    for s in 0..SEGS {
        let i0 = ring + s as u32;
        let i1 = ring + ((s + 1) % SEGS) as u32;
        buf.indices.extend([center_i, i0, i1]);
    }
}

/// Material bucket for batched decor meshes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorMat {
    Metal = 0,
    Wood = 1,
    Accent = 2,
}

impl DecorMat {
    pub const COUNT: usize = 3;
}

/// Low-poly OSM street furniture at `pos` (XZ), yaw around +Y.
pub fn append_decor(
    bufs: &mut [MeshBuf],
    prop: crate::osm::DecorProp,
    pos: Vec2,
    yaw: f32,
    base_y: f32,
) {
    use crate::osm::DecorProp;
    let m = DecorMat::Metal as usize;
    let w = DecorMat::Wood as usize;
    let a = DecorMat::Accent as usize;
    let (s, c) = (yaw.sin(), yaw.cos());
    let local = |lx: f32, ly: f32, lz: f32| -> Vec3 {
        Vec3::new(
            pos.x + lx * c + lz * s,
            base_y + ly,
            pos.y - lx * s + lz * c,
        )
    };

    match prop {
        DecorProp::Bench => {
            append_box(
                &mut bufs[w],
                local(0.0, 0.42, 0.0),
                Vec3::new(1.4, 0.08, 0.45),
            );
            append_box(
                &mut bufs[w],
                local(0.0, 0.72, -0.18),
                Vec3::new(1.4, 0.45, 0.08),
            );
            append_box(
                &mut bufs[m],
                local(-0.55, 0.2, 0.1),
                Vec3::new(0.08, 0.4, 0.08),
            );
            append_box(
                &mut bufs[m],
                local(0.55, 0.2, 0.1),
                Vec3::new(0.08, 0.4, 0.08),
            );
        }
        DecorProp::Bin => {
            append_taper_cylinder(&mut bufs[m], local(0.0, 0.0, 0.0), 0.22, 0.2, 0.95);
            append_box(
                &mut bufs[a],
                local(0.0, 0.98, 0.0),
                Vec3::new(0.42, 0.05, 0.42),
            );
        }
        DecorProp::Bollard => {
            append_taper_cylinder(&mut bufs[m], local(0.0, 0.0, 0.0), 0.12, 0.1, 0.85);
            append_box(
                &mut bufs[a],
                local(0.0, 0.88, 0.0),
                Vec3::new(0.16, 0.06, 0.16),
            );
        }
        DecorProp::BusStop => {
            append_taper_cylinder(&mut bufs[m], local(-0.7, 0.0, 0.0), 0.06, 0.05, 2.6);
            append_taper_cylinder(&mut bufs[m], local(0.7, 0.0, 0.0), 0.06, 0.05, 2.6);
            append_box(
                &mut bufs[m],
                local(0.0, 2.55, 0.0),
                Vec3::new(1.7, 0.06, 0.9),
            );
            append_box(
                &mut bufs[a],
                local(0.0, 1.4, -0.35),
                Vec3::new(1.5, 0.9, 0.05),
            );
            append_box(
                &mut bufs[w],
                local(0.0, 0.55, 0.15),
                Vec3::new(1.3, 0.06, 0.35),
            );
        }
        DecorProp::Hydrant => {
            append_taper_cylinder(&mut bufs[a], local(0.0, 0.0, 0.0), 0.16, 0.14, 0.55);
            append_box(
                &mut bufs[a],
                local(0.0, 0.62, 0.0),
                Vec3::new(0.28, 0.18, 0.28),
            );
            append_box(
                &mut bufs[m],
                local(0.22, 0.45, 0.0),
                Vec3::new(0.2, 0.1, 0.1),
            );
        }
        DecorProp::PostBox => {
            append_box(
                &mut bufs[a],
                local(0.0, 0.55, 0.0),
                Vec3::new(0.45, 1.1, 0.35),
            );
            append_box(
                &mut bufs[m],
                local(0.0, 1.15, 0.05),
                Vec3::new(0.35, 0.08, 0.12),
            );
        }
        DecorProp::BikeRack => {
            for x in [-0.45f32, 0.0, 0.45] {
                append_taper_cylinder(&mut bufs[m], local(x, 0.0, 0.0), 0.04, 0.04, 0.75);
                append_box(
                    &mut bufs[m],
                    local(x, 0.78, 0.0),
                    Vec3::new(0.08, 0.08, 0.55),
                );
            }
        }
    }
}

/// Optional static collider for solid props (bollards / hydrants / post boxes).
pub fn decor_collider(
    prop: crate::osm::DecorProp,
    pos: Vec2,
    yaw: f32,
    base_y: f32,
) -> Option<(Transform, Collider)> {
    use crate::osm::DecorProp;
    let (s, c) = (yaw.sin(), yaw.cos());
    let place = |lx: f32, ly: f32, lz: f32, col: Collider| {
        Some((
            Transform::from_xyz(
                pos.x + lx * c + lz * s,
                base_y + ly,
                pos.y - lx * s + lz * c,
            )
            .with_rotation(Quat::from_rotation_y(yaw)),
            col,
        ))
    };
    match prop {
        DecorProp::Bollard => place(0.0, 0.42, 0.0, Collider::cylinder(0.14, 0.85)),
        DecorProp::Hydrant => place(0.0, 0.35, 0.0, Collider::cylinder(0.2, 0.7)),
        DecorProp::PostBox => place(0.0, 0.55, 0.0, Collider::cuboid(0.45, 1.1, 0.35)),
        DecorProp::BusStop => place(0.0, 1.3, 0.0, Collider::cuboid(1.6, 2.6, 0.7)),
        DecorProp::Bin => place(0.0, 0.48, 0.0, Collider::cylinder(0.24, 0.95)),
        _ => None,
    }
}

fn append_box(buf: &mut MeshBuf, center: Vec3, size: Vec3) {
    let hx = size.x * 0.5;
    let hy = size.y * 0.5;
    let hz = size.z * 0.5;
    let corners = [
        [center.x - hx, center.y - hy, center.z - hz],
        [center.x + hx, center.y - hy, center.z - hz],
        [center.x + hx, center.y + hy, center.z - hz],
        [center.x - hx, center.y + hy, center.z - hz],
        [center.x - hx, center.y - hy, center.z + hz],
        [center.x + hx, center.y - hy, center.z + hz],
        [center.x + hx, center.y + hy, center.z + hz],
        [center.x - hx, center.y + hy, center.z + hz],
    ];
    let faces: [([u32; 4], [f32; 3]); 6] = [
        ([0, 1, 2, 3], [0.0, 0.0, -1.0]),
        ([5, 4, 7, 6], [0.0, 0.0, 1.0]),
        ([4, 0, 3, 7], [-1.0, 0.0, 0.0]),
        ([1, 5, 6, 2], [1.0, 0.0, 0.0]),
        ([3, 2, 6, 7], [0.0, 1.0, 0.0]),
        ([4, 5, 1, 0], [0.0, -1.0, 0.0]),
    ];
    for (idx, normal) in faces {
        let base = buf.positions.len() as u32;
        for &i in &idx {
            buf.positions.push(corners[i as usize]);
            buf.normals.push(normal);
            buf.uvs.push([0.0, 0.0]);
        }
        buf.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Street/park tree. `scale` ≈ 1.0 → ~10–13 m tall.
/// Hash of position picks conifer (stacked cones) vs broadleaf (canopy blobs).
pub fn append_tree(trunk: &mut MeshBuf, foliage: &mut MeshBuf, pos: Vec2, scale: f32, base_y: f32) {
    let s = scale.clamp(0.65, 2.6);
    let h = hash_u32((pos.x * 10.0) as u32, (pos.y * 10.0) as u32);
    let conifer = h % 5 < 2;

    let trunk_h = if conifer { 8.0 * s } else { 6.5 * s };
    let r_bot = if conifer { 0.32 * s } else { 0.40 * s };
    let r_top = r_bot * 0.55;
    append_taper_cylinder(
        trunk,
        Vec3::new(pos.x, base_y, pos.y),
        r_bot,
        r_top,
        trunk_h,
    );

    if conifer {
        // Three nested cones — classic pine silhouette (~12 m tip at scale 1).
        let tiers = [
            (trunk_h * 0.32, 3.4 * s, 4.4 * s),
            (trunk_h * 0.52, 2.6 * s, 3.8 * s),
            (trunk_h * 0.72, 1.7 * s, 3.6 * s),
        ];
        for (y, r, fh) in tiers {
            append_cone(foliage, Vec3::new(pos.x, base_y + y, pos.y), r, fh);
        }
    } else {
        // Broadleaf: overlapping ellipsoids for a rounded crown.
        let crown_y = base_y + trunk_h * 0.78;
        let blobs = [
            (0.0, 0.0, 3.6 * s, 3.2 * s),
            (1.35 * s, 0.45 * s, 2.6 * s, 2.3 * s),
            (-1.15 * s, -0.55 * s, 2.5 * s, 2.2 * s),
            (0.4 * s, -s, 2.1 * s, 1.9 * s),
            (-0.25 * s, 1.1 * s, 2.2 * s, 2.0 * s),
        ];
        for (ox, oz, r, fh) in blobs {
            let y_off = (h % 7) as f32 * 0.05 * s;
            append_ellipsoid(
                foliage,
                Vec3::new(pos.x + ox, crown_y + y_off + fh * 0.28, pos.y + oz),
                r,
                fh * 0.58,
                r,
            );
        }
    }
}

fn append_taper_cylinder(buf: &mut MeshBuf, base: Vec3, r_bot: f32, r_top: f32, height: f32) {
    const SIDES: usize = 8;
    let bot = buf.positions.len() as u32;
    for s in 0..SIDES {
        let a = (s as f32 / SIDES as f32) * std::f32::consts::TAU;
        let n = [a.cos(), 0.0, a.sin()];
        buf.positions
            .push([base.x + n[0] * r_bot, base.y, base.z + n[2] * r_bot]);
        buf.normals.push(n);
        buf.uvs.push([s as f32 / SIDES as f32, 0.0]);
    }
    let top = buf.positions.len() as u32;
    for s in 0..SIDES {
        let a = (s as f32 / SIDES as f32) * std::f32::consts::TAU;
        let n = [a.cos(), 0.0, a.sin()];
        buf.positions.push([
            base.x + n[0] * r_top,
            base.y + height,
            base.z + n[2] * r_top,
        ]);
        buf.normals.push(n);
        buf.uvs.push([s as f32 / SIDES as f32, height / 4.0]);
    }
    for s in 0..SIDES {
        let i0 = bot + s as u32;
        let i1 = bot + ((s + 1) % SIDES) as u32;
        let i2 = top + ((s + 1) % SIDES) as u32;
        let i3 = top + s as u32;
        buf.indices.extend([i0, i1, i2, i0, i2, i3]);
    }
}

fn append_cone(buf: &mut MeshBuf, base: Vec3, radius: f32, height: f32) {
    const SIDES: usize = 8;
    let tip = Vec3::new(base.x, base.y + height, base.z);
    let tip_i = buf.positions.len() as u32;
    buf.positions.push(tip.to_array());
    buf.normals.push([0.0, 1.0, 0.0]);
    buf.uvs.push([0.5, 1.0]);

    let ring_start = buf.positions.len() as u32;
    for s in 0..SIDES {
        let a = (s as f32 / SIDES as f32) * std::f32::consts::TAU;
        let p = Vec3::new(base.x + a.cos() * radius, base.y, base.z + a.sin() * radius);
        let n = Vec3::new(a.cos(), 0.45, a.sin()).normalize_or_zero();
        buf.positions.push(p.to_array());
        buf.normals.push(n.to_array());
        buf.uvs.push([s as f32 / SIDES as f32, 0.0]);
    }
    for s in 0..SIDES {
        let i0 = ring_start + s as u32;
        let i1 = ring_start + ((s + 1) % SIDES) as u32;
        buf.indices.extend([tip_i, i0, i1]);
    }
}

/// Low-poly ellipsoid (lat/long) for broadleaf crowns.
fn append_ellipsoid(buf: &mut MeshBuf, center: Vec3, rx: f32, ry: f32, rz: f32) {
    const RINGS: usize = 5;
    const SEGS: usize = 8;
    let start = buf.positions.len() as u32;
    for ring in 0..=RINGS {
        let v = ring as f32 / RINGS as f32;
        let phi = v * std::f32::consts::PI;
        let y = center.y + (-phi.cos()) * ry;
        let band_r = phi.sin();
        for seg in 0..SEGS {
            let u = seg as f32 / SEGS as f32;
            let theta = u * std::f32::consts::TAU;
            let x = center.x + theta.cos() * band_r * rx;
            let z = center.z + theta.sin() * band_r * rz;
            let n = Vec3::new(
                theta.cos() * band_r / rx.max(1e-3),
                -phi.cos() / ry.max(1e-3),
                theta.sin() * band_r / rz.max(1e-3),
            )
            .normalize_or_zero();
            buf.positions.push([x, y, z]);
            buf.normals.push(n.to_array());
            buf.uvs.push([u, v]);
        }
    }
    for ring in 0..RINGS {
        for seg in 0..SEGS {
            let i0 = start + (ring * SEGS + seg) as u32;
            let i1 = start + (ring * SEGS + (seg + 1) % SEGS) as u32;
            let i2 = start + ((ring + 1) * SEGS + (seg + 1) % SEGS) as u32;
            let i3 = start + ((ring + 1) * SEGS + seg) as u32;
            buf.indices.extend([i0, i1, i2, i0, i2, i3]);
        }
    }
}

/// Deterministic tree scatter inside a polygon. `density` ≈ trees per m².
pub fn scatter_trees_in_polygon(points: &[Vec2], density: f32) -> Vec<(Vec2, f32)> {
    let ring = close_ring(points);
    if ring.len() < 3 {
        return vec![];
    }
    let mut min = Vec2::splat(f32::MAX);
    let mut max = Vec2::splat(f32::MIN);
    for p in &ring {
        min = min.min(*p);
        max = max.max(*p);
    }
    let spacing = (1.0 / density.max(0.001)).sqrt().clamp(4.0, 28.0);
    let mut out = Vec::new();
    let mut gy = min.y;
    let mut row = 0u32;
    while gy <= max.y {
        let mut gx = min.x
            + if row.is_multiple_of(2) {
                0.0
            } else {
                spacing * 0.5
            };
        let mut col = 0u32;
        while gx <= max.x {
            let jx = ((hash_u32(row, col) % 1000) as f32 / 1000.0 - 0.5) * spacing * 0.55;
            let jy = ((hash_u32(col, row) % 1000) as f32 / 1000.0 - 0.5) * spacing * 0.55;
            let p = Vec2::new(gx + jx, gy + jy);
            if point_in_ring(p, &ring) {
                let scale = 0.95
                    + (hash_u32(row.wrapping_mul(13), col.wrapping_mul(7)) % 90) as f32 * 0.012;
                out.push((p, scale));
            }
            gx += spacing;
            col += 1;
        }
        gy += spacing;
        row += 1;
        if out.len() > 400 {
            break;
        }
    }
    out
}

fn hash_u32(a: u32, b: u32) -> u32 {
    let mut n = a
        .wrapping_mul(374761393)
        .wrapping_add(b.wrapping_mul(668265263));
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    n ^ (n >> 16)
}

fn point_in_ring(p: Vec2, ring: &[Vec2]) -> bool {
    let mut inside = false;
    let n = ring.len();
    let mut j = n - 1;
    for i in 0..n {
        let pi = ring[i];
        let pj = ring[j];
        if ((pi.y > p.y) != (pj.y > p.y))
            && (p.x < (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y + 1e-8) + pi.x)
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Trunk collider for a tree at `pos` (XZ). Height covers the lower trunk only.
pub fn tree_collider(pos: Vec2, scale: f32, base_y: f32) -> (Transform, Collider) {
    let s = scale.clamp(0.65, 2.6);
    let h = hash_u32((pos.x * 10.0) as u32, (pos.y * 10.0) as u32);
    let conifer = h % 5 < 2;
    // Match visual trunk radius; slightly thicker so the car can't clip the mesh.
    let radius = if conifer { 0.40 * s } else { 0.50 * s };
    let height = if conifer { 4.5 * s } else { 3.8 * s };
    (
        Transform::from_xyz(pos.x, base_y + height * 0.5, pos.y),
        Collider::cylinder(radius, height),
    )
}

/// Collider matching the visible footprint (not an AABB — avoids invisible walls).
/// Concave footprints are split into convex triangular prisms via earcut.
pub fn building_collider(points: &[Vec2], height: f32) -> Option<Collider> {
    let ring = close_ring(points);
    if ring.len() < 3 {
        return None;
    }
    let tris = earcut(&ring);
    if tris.is_empty() {
        return None;
    }

    let mut parts: Vec<(Vec3, Quat, Collider)> = Vec::with_capacity(tris.len());
    for [i, j, k] in tris {
        let a = ring[i as usize];
        let b = ring[j as usize];
        let c = ring[k as usize];
        // Degenerate ear → skip
        let area = (b - a).perp_dot(c - a).abs();
        if area < 1e-4 {
            continue;
        }
        let hull_pts = vec![
            Vec3::new(a.x, 0.0, a.y),
            Vec3::new(b.x, 0.0, b.y),
            Vec3::new(c.x, 0.0, c.y),
            Vec3::new(a.x, height, a.y),
            Vec3::new(b.x, height, b.y),
            Vec3::new(c.x, height, c.y),
        ];
        if let Some(col) = Collider::convex_hull(hull_pts) {
            parts.push((Vec3::ZERO, Quat::IDENTITY, col));
        }
    }

    match parts.len() {
        0 => None,
        1 => Some(parts.pop().unwrap().2),
        _ => Some(Collider::compound(parts)),
    }
}

fn close_ring(points: &[Vec2]) -> Vec<Vec2> {
    let mut ring: Vec<Vec2> = points.iter().copied().filter(|p| p.is_finite()).collect();
    if ring.len() >= 2 && ring.first() == ring.last() {
        ring.pop();
    }
    if signed_area(&ring) < 0.0 {
        ring.reverse();
    }
    ring
}

fn signed_area(ring: &[Vec2]) -> f32 {
    let mut a = 0.0;
    for i in 0..ring.len() {
        let p = ring[i];
        let q = ring[(i + 1) % ring.len()];
        a += p.x * q.y - q.x * p.y;
    }
    a * 0.5
}

fn earcut(ring: &[Vec2]) -> Vec<[u32; 3]> {
    let n = ring.len();
    if n < 3 {
        return vec![];
    }
    if n == 3 {
        return vec![[0, 1, 2]];
    }

    let mut idx: Vec<usize> = (0..n).collect();
    let mut tris = Vec::with_capacity(n - 2);
    let mut guard = n * n;

    while idx.len() > 3 && guard > 0 {
        guard -= 1;
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let prev = idx[(i + m - 1) % m];
            let curr = idx[i];
            let next = idx[(i + 1) % m];
            if !is_ear(ring, &idx, prev, curr, next) {
                continue;
            }
            tris.push([prev as u32, curr as u32, next as u32]);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            let anchor = idx[0] as u32;
            for w in idx.windows(2).skip(1) {
                tris.push([anchor, w[0] as u32, w[1] as u32]);
            }
            return tris;
        }
    }
    if idx.len() == 3 {
        tris.push([idx[0] as u32, idx[1] as u32, idx[2] as u32]);
    }
    tris
}

fn is_ear(ring: &[Vec2], idx: &[usize], prev: usize, curr: usize, next: usize) -> bool {
    let a = ring[prev];
    let b = ring[curr];
    let c = ring[next];
    let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    if cross <= 1e-6 {
        return false;
    }
    for &i in idx {
        if i == prev || i == curr || i == next {
            continue;
        }
        if point_in_triangle(ring[i], a, b, c) {
            return false;
        }
    }
    true
}

fn point_in_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let d1 = sign(p, a, b);
    let d2 = sign(p, b, c);
    let d3 = sign(p, c, a);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

fn sign(p1: Vec2, p2: Vec2, p3: Vec2) -> f32 {
    (p1.x - p3.x) * (p2.y - p3.y) - (p2.x - p3.x) * (p1.y - p3.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::mesh::VertexAttributeValues;

    #[test]
    fn building_has_vertices() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 8.0),
            Vec2::new(0.0, 8.0),
        ];
        let mesh = building_mesh(&pts, 12.0);
        let pos = mesh.attribute(Mesh::ATTRIBUTE_POSITION).expect("positions");
        assert!(pos.len() > 8);
    }

    #[test]
    fn trim_shortens_both_ends() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0)];
        let t = trim_polyline_ends(&pts, 3.0);
        assert_eq!(t.len(), 2);
        assert!((t[0].x - 3.0).abs() < 1e-3);
        assert!((t[1].x - 17.0).abs() < 1e-3);
    }

    #[test]
    fn short_road_still_draws() {
        let mut buf = MeshBuf::default();
        // ~4 m stub; uncapped inset (0.55*6) would wipe it.
        append_road(
            &mut buf,
            &[Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0)],
            6.0,
            &|_| 0.0,
        );
        assert!(!buf.positions.is_empty());
    }

    #[test]
    fn junction_pad_faces_up() {
        let mut buf = MeshBuf::default();
        append_road_junction(&mut buf, Vec2::ZERO, 8.0, &|_| 0.0);
        let mesh = buf.into_mesh();
        let VertexAttributeValues::Float32x3(positions) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
        else {
            panic!("bad positions");
        };
        let Indices::U32(indices) = mesh.indices().unwrap() else {
            panic!("bad indices");
        };
        let p0 = Vec3::from_array(positions[indices[0] as usize]);
        let p1 = Vec3::from_array(positions[indices[1] as usize]);
        let p2 = Vec3::from_array(positions[indices[2] as usize]);
        let n = (p1 - p0).cross(p2 - p0);
        assert!(n.y > 0.0, "junction normal y={}", n.y);
    }

    #[test]
    fn road_signs_are_planned_and_meshed() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(120.0, 0.0)];
        let planned = plan_road_signs(&pts, 7.0, "primary");
        assert!(!planned.is_empty());
        let mut pole = MeshBuf::default();
        let mut faces: Vec<MeshBuf> = (0..SignKind::COUNT).map(|_| MeshBuf::default()).collect();
        for (foot, dir, kind) in planned {
            append_road_sign(&mut pole, &mut faces, foot, dir, kind, 0.0);
        }
        assert!(!pole.positions.is_empty());
        assert!(faces.iter().any(|f| !f.positions.is_empty()));
    }

    #[test]
    fn grass_blades_fill_polygon() {
        let mut buf = MeshBuf::default();
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(20.0, 0.0),
            Vec2::new(20.0, 20.0),
            Vec2::new(0.0, 20.0),
        ];
        append_grass_blades(&mut buf, &pts, &|_| 0.0);
        assert!(buf.positions.len() > 30);
    }

    #[test]
    fn decor_bench_emits_geometry() {
        let mut bufs: Vec<MeshBuf> = (0..DecorMat::COUNT).map(|_| MeshBuf::default()).collect();
        append_decor(
            &mut bufs,
            crate::osm::DecorProp::Bench,
            Vec2::ZERO,
            0.0,
            0.0,
        );
        assert!(bufs.iter().any(|b| !b.positions.is_empty()));
    }

    #[test]
    fn tree_collider_is_upright_cylinder() {
        let (tf, _col) = tree_collider(Vec2::new(3.0, -2.0), 1.0, 0.0);
        assert!((tf.translation.x - 3.0).abs() < 1e-4);
        assert!((tf.translation.z + 2.0).abs() < 1e-4);
        assert!(tf.translation.y > 1.0);
    }

    #[test]
    fn tree_is_tall_enough() {
        let mut trunk = MeshBuf::default();
        let mut foliage = MeshBuf::default();
        append_tree(&mut trunk, &mut foliage, Vec2::ZERO, 1.0, 0.0);
        let max_y = trunk
            .positions
            .iter()
            .chain(foliage.positions.iter())
            .map(|p| p[1])
            .fold(0.0f32, f32::max);
        assert!(max_y > 8.0, "tree height {max_y}");
        assert!(!trunk.positions.is_empty());
        assert!(!foliage.positions.is_empty());
    }

    #[test]
    fn road_triangles_face_up() {
        let mut buf = MeshBuf::default();
        // Long enough that inset still leaves a ribbon.
        append_road(
            &mut buf,
            &[Vec2::new(0.0, 0.0), Vec2::new(30.0, 0.0)],
            6.0,
            &|_| 0.0,
        );
        let mesh = buf.into_mesh();
        let VertexAttributeValues::Float32x3(positions) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
        else {
            panic!("bad positions");
        };
        let Indices::U32(indices) = mesh.indices().unwrap() else {
            panic!("bad indices");
        };
        let i0 = indices[0] as usize;
        let i1 = indices[1] as usize;
        let i2 = indices[2] as usize;
        let p0 = Vec3::from_array(positions[i0]);
        let p1 = Vec3::from_array(positions[i1]);
        let p2 = Vec3::from_array(positions[i2]);
        let n = (p1 - p0).cross(p2 - p0).normalize();
        assert!(
            n.y > 0.5,
            "road must face +Y (was culled when facing down); got {n:?}"
        );
    }

    #[test]
    fn roof_uvs_span_footprint() {
        let pts = [
            Vec2::new(100.0, 200.0),
            Vec2::new(110.0, 200.0),
            Vec2::new(110.0, 208.0),
            Vec2::new(100.0, 208.0),
        ];
        let mut walls = MeshBuf::default();
        let mut roof = MeshBuf::default();
        append_building(&mut walls, &mut roof, &pts, 10.0, 0.0);
        assert!(!roof.uvs.is_empty());
        let max_u = roof.uvs.iter().map(|uv| uv[0]).fold(0.0f32, f32::max);
        let max_v = roof.uvs.iter().map(|uv| uv[1]).fold(0.0f32, f32::max);
        // 10m × 0.4 = 4 UV repeats in X; 8m × 0.4 = 3.2 in Z
        assert!(max_u > 3.5, "max_u={max_u}");
        assert!(max_v > 2.5, "max_v={max_v}");
        assert!(roof.uvs.iter().all(|uv| uv[0] >= 0.0 && uv[1] >= 0.0));
    }

    #[test]
    fn wall_winding_matches_outward_normal() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 8.0),
            Vec2::new(0.0, 8.0),
        ];
        let mesh = building_mesh(&pts, 5.0);
        let VertexAttributeValues::Float32x3(positions) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
        else {
            panic!("bad positions");
        };
        let VertexAttributeValues::Float32x3(normals) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL).unwrap()
        else {
            panic!("bad normals");
        };
        let Indices::U32(indices) = mesh.indices().unwrap() else {
            panic!("bad indices");
        };

        let i0 = indices[0] as usize;
        let i1 = indices[1] as usize;
        let i2 = indices[2] as usize;
        let p0 = Vec3::from_array(positions[i0]);
        let p1 = Vec3::from_array(positions[i1]);
        let p2 = Vec3::from_array(positions[i2]);
        let geom = (p1 - p0).cross(p2 - p0).normalize();
        let attrib = Vec3::from_array(normals[i0]);
        assert!(geom.dot(attrib) > 0.5, "geom {geom:?} vs attrib {attrib:?}");
        assert!(attrib.z < -0.5, "expected south outward, got {attrib:?}");
    }

    #[test]
    fn earcut_handles_l_shape() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(6.0, 0.0),
            Vec2::new(6.0, 2.0),
            Vec2::new(2.0, 2.0),
            Vec2::new(2.0, 6.0),
            Vec2::new(0.0, 6.0),
        ];
        let tris = earcut(&close_ring(&pts));
        assert_eq!(tris.len(), pts.len() - 2);
    }

    #[test]
    fn building_collider_matches_rectangle() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 4.0),
            Vec2::new(0.0, 4.0),
        ];
        assert!(building_collider(&pts, 12.0).is_some());
    }

    #[test]
    fn building_collider_handles_l_shape() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(6.0, 0.0),
            Vec2::new(6.0, 2.0),
            Vec2::new(2.0, 2.0),
            Vec2::new(2.0, 6.0),
            Vec2::new(0.0, 6.0),
        ];
        assert!(building_collider(&pts, 8.0).is_some());
    }
}
