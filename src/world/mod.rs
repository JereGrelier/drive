mod driveability;
mod mesh;

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::osm::parse::MapFeature;
use crate::osm::terrain::height_at;
use crate::osm::{chunk_size_m, generate_chunk, FeatureKind, MapData};
use crate::style::Palette;
use crate::vehicle::PlayerCar;
use driveability::spawn_drive_collider;
use mesh::{
    append_building, append_decor, append_grass, append_grass_blades, append_road,
    append_road_junction, append_road_sign, append_tree, building_collider, decor_collider,
    plan_road_signs, scatter_trees_in_polygon, tree_collider, DecorMat, MeshBuf, SignKind,
};

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TerrainSeed>()
            .add_systems(Startup, spawn_world.after(crate::style::setup_palette))
            .add_systems(
                Update,
                stream_proc_chunks.run_if(resource_exists::<ProcStream>),
            );
    }
}

/// Infinite procedural streaming (absent → bake full MapData once).
#[derive(Resource, Clone, Copy)]
pub struct ProcStream {
    pub seed: u64,
    /// Chebyshev radius in chunks around the player.
    pub radius: i32,
}

/// When `Some`, features + ground follow procedural height/biomes.
#[derive(Resource, Clone, Copy, Default)]
pub struct TerrainSeed(pub Option<u64>);

#[derive(Component)]
struct ProcChunk {
    cx: i32,
    cz: i32,
}

pub fn spawn_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    palette: Res<Palette>,
    map: Res<MapDataResource>,
    stream: Option<Res<ProcStream>>,
    terrain: Res<TerrainSeed>,
) {
    let seed = terrain.0.or(stream.as_ref().map(|s| s.seed));

    if let Some(stream) = stream.as_deref() {
        let look_at = Vec3::new(
            map.0.spawn.x,
            height_at(stream.seed, map.0.spawn.x, map.0.spawn.y),
            map.0.spawn.y,
        );
        commands.insert_resource(MapCenter(look_at));
        spawn_sun(&mut commands, look_at);
        // Eager origin chunks — Update streaming needs PlayerCar which spawns later.
        let r = stream.radius;
        for cz in -r..=r {
            for cx in -r..=r {
                spawn_proc_chunk(&mut commands, &mut meshes, &palette, stream.seed, cx, cz);
            }
        }
        return;
    }

    let data = &map.0;
    let (min, max) = bounds(data);
    let center = (min + max) * 0.5;
    let size = (max - min).abs() + Vec2::splat(120.0);
    let ground_w = size.x.max(80.0);
    let ground_d = size.y.max(80.0);

    if let Some(seed) = seed {
        // Finite procedural: one heightfield covering the map window.
        spawn_heightfield_rect(
            &mut commands,
            &mut meshes,
            &palette,
            seed,
            Vec2::new(center.x - ground_w * 0.5, center.y - ground_d * 0.5),
            ground_w,
            ground_d,
            48,
            None,
        );
    } else {
        const GROUND_THICKNESS: f32 = 4.0;
        commands.spawn((
            Mesh3d(meshes.add(Plane3d::default().mesh().size(ground_w, ground_d))),
            MeshMaterial3d(palette.ground.clone()),
            Transform::from_xyz(center.x, 0.0, center.y),
            Name::new("GroundVisual"),
        ));
        commands.spawn((
            RigidBody::Static,
            Collider::cuboid(ground_w, GROUND_THICKNESS, ground_d),
            Transform::from_xyz(center.x, -GROUND_THICKNESS * 0.5, center.y),
            Friction::new(1.4),
            Name::new("GroundCollider"),
        ));
    }

    spawn_features(
        &mut commands,
        &mut meshes,
        &palette,
        &data.features,
        None,
        seed,
    );

    let hy = seed
        .map(|s| height_at(s, center.x, center.y))
        .unwrap_or(0.0);
    let look_at = Vec3::new(center.x, hy, center.y);
    commands.insert_resource(MapCenter(look_at));
    spawn_sun(&mut commands, look_at);
}

fn spawn_proc_chunk(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    palette: &Palette,
    seed: u64,
    cx: i32,
    cz: i32,
) {
    let cs = chunk_size_m();
    let origin = Vec2::new(cx as f32 * cs, cz as f32 * cs);
    let features = generate_chunk(seed, cx, cz);
    let root = commands
        .spawn((
            ProcChunk { cx, cz },
            Transform::IDENTITY,
            Visibility::default(),
            Name::new(format!("Chunk_{cx}_{cz}")),
        ))
        .id();
    spawn_heightfield_rect(
        commands,
        meshes,
        palette,
        seed,
        origin,
        cs,
        cs,
        16,
        Some(root),
    );
    spawn_features(commands, meshes, palette, &features, Some(root), Some(seed));
}

/// Visual + collider heightfield for a rectangle in XZ.
#[allow(clippy::too_many_arguments)]
fn spawn_heightfield_rect(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    palette: &Palette,
    seed: u64,
    origin: Vec2,
    size_x: f32,
    size_z: f32,
    res: u32,
    parent: Option<Entity>,
) {
    let y_of = |p: Vec2| height_at(seed, p.x, p.y);
    let mut buf = MeshBuf::default();
    let n = res.max(2);
    let base = buf.positions.len() as u32;
    for iz in 0..=n {
        for ix in 0..=n {
            let u = ix as f32 / n as f32;
            let v = iz as f32 / n as f32;
            let p = Vec2::new(origin.x + u * size_x, origin.y + v * size_z);
            let y = y_of(p);
            buf.positions.push([p.x, y, p.y]);
            let e = (size_x / n as f32).max(0.5);
            let hx = y_of(Vec2::new(p.x + e, p.y)) - y_of(Vec2::new(p.x - e, p.y));
            let hz = y_of(Vec2::new(p.x, p.y + e)) - y_of(Vec2::new(p.x, p.y - e));
            let normal = Vec3::new(-hx, 2.0 * e, -hz).normalize_or_zero();
            buf.normals.push(normal.to_array());
            buf.uvs.push([u * size_x * 0.08, v * size_z * 0.08]);
        }
    }
    let stride = n + 1;
    for iz in 0..n {
        for ix in 0..n {
            let i = base + iz * stride + ix;
            buf.indices
                .extend([i, i + stride, i + 1, i + 1, i + stride, i + stride + 1]);
        }
    }
    let visual = commands
        .spawn((
            Mesh3d(meshes.add(buf.into_mesh())),
            MeshMaterial3d(palette.ground.clone()),
            Transform::IDENTITY,
            Name::new("TerrainVisual"),
        ))
        .id();
    if let Some(p) = parent {
        commands.entity(p).add_child(visual);
    }

    let collider = spawn_drive_collider(commands, seed, origin, size_x, size_z, n);
    if let Some(p) = parent {
        commands.entity(p).add_child(collider);
    }
}

fn spawn_sun(commands: &mut Commands, look_at: Vec3) {
    commands.spawn((
        DirectionalLight {
            illuminance: 22_000.0,
            shadows_enabled: true,
            shadow_depth_bias: 0.02,
            shadow_normal_bias: 0.6,
            ..default()
        },
        Transform::from_xyz(look_at.x + 60.0, 100.0, look_at.z + 40.0).looking_at(look_at, Vec3::Y),
        SunLight,
        Name::new("Sun"),
    ));
    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.72, 0.78, 0.92),
        brightness: 180.0,
    });
}

fn stream_proc_chunks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    palette: Res<Palette>,
    stream: Res<ProcStream>,
    player: Query<&Transform, With<PlayerCar>>,
    existing: Query<(Entity, &ProcChunk)>,
) {
    let Ok(tf) = player.get_single() else {
        return;
    };
    let cs = chunk_size_m();
    let pcx = (tf.translation.x / cs).floor() as i32;
    let pcz = (tf.translation.z / cs).floor() as i32;
    let r = stream.radius;

    let mut wanted = std::collections::HashSet::new();
    for cz in (pcz - r)..=(pcz + r) {
        for cx in (pcx - r)..=(pcx + r) {
            wanted.insert((cx, cz));
        }
    }

    for (entity, chunk) in &existing {
        if !wanted.contains(&(chunk.cx, chunk.cz)) {
            commands.entity(entity).despawn_recursive();
        }
    }

    let loaded: std::collections::HashSet<(i32, i32)> =
        existing.iter().map(|(_, c)| (c.cx, c.cz)).collect();

    for (cx, cz) in wanted {
        if loaded.contains(&(cx, cz)) {
            continue;
        }
        spawn_proc_chunk(&mut commands, &mut meshes, &palette, stream.seed, cx, cz);
    }
}

fn spawn_features(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    palette: &Palette,
    features: &[MapFeature],
    parent: Option<Entity>,
    seed: Option<u64>,
) {
    let y_of = |p: Vec2| -> f32 { seed.map(|s| height_at(s, p.x, p.y)).unwrap_or(0.0) };
    let n_building_mats = palette.building.len();
    let n_roof_mats = palette.roof.len().max(1);
    let mut wall_bufs: Vec<MeshBuf> = (0..n_building_mats).map(|_| MeshBuf::default()).collect();
    let mut roof_bufs: Vec<MeshBuf> = (0..n_roof_mats).map(|_| MeshBuf::default()).collect();
    let mut road_buf = MeshBuf::default();
    let mut grass_buf = MeshBuf::default();
    let mut grass_blade_buf = MeshBuf::default();
    let mut trunk_buf = MeshBuf::default();
    let mut foliage_buf = MeshBuf::default();
    let mut sign_pole_buf = MeshBuf::default();
    let mut sign_face_bufs: Vec<MeshBuf> =
        (0..SignKind::COUNT).map(|_| MeshBuf::default()).collect();
    let mut decor_bufs: Vec<MeshBuf> = (0..DecorMat::COUNT).map(|_| MeshBuf::default()).collect();
    let mut junctions: Vec<(Vec2, f32)> = Vec::new();
    let mut tree_count = 0usize;
    let mut sign_count = 0usize;
    let mut decor_count = 0usize;

    for feature in features {
        match &feature.kind {
            FeatureKind::Building { levels, .. } => {
                let height = (levels * 3.2).clamp(4.0, 60.0);
                let jitter = (feature.points[0].x * 17.0 + feature.points[0].y * 31.0)
                    .sin()
                    .abs()
                    * 1.5;
                let h = height + jitter;
                let base_y = y_of(feature.points[0]);
                let bucket = hash_bucket(feature.points[0], n_building_mats);
                let roof_bucket = hash_bucket(feature.points[0] + Vec2::splat(3.1), n_roof_mats);
                append_building(
                    &mut wall_bufs[bucket],
                    &mut roof_bufs[roof_bucket],
                    &feature.points,
                    h,
                    base_y,
                );

                if let Some(collider) = building_collider(&feature.points, h) {
                    let id = commands
                        .spawn((
                            RigidBody::Static,
                            collider,
                            Transform::from_xyz(0.0, base_y, 0.0),
                            Friction::new(0.8),
                            Name::new("BuildingCollider"),
                        ))
                        .id();
                    if let Some(p) = parent {
                        commands.entity(p).add_child(id);
                    }
                }
            }
            FeatureKind::Road { width, highway, .. } => {
                append_road(&mut road_buf, &feature.points, *width, &y_of);
                if let (Some(&a), Some(&b)) = (feature.points.first(), feature.points.last()) {
                    push_junction(&mut junctions, a, *width);
                    if a.distance(b) > 0.5 {
                        push_junction(&mut junctions, b, *width);
                    }
                }
                for (foot, dir, kind) in plan_road_signs(&feature.points, *width, highway) {
                    if sign_count >= 48 {
                        break;
                    }
                    append_road_sign(
                        &mut sign_pole_buf,
                        &mut sign_face_bufs,
                        foot,
                        dir,
                        kind,
                        y_of(foot),
                    );
                    sign_count += 1;
                }
            }
            FeatureKind::Green { density } => {
                append_grass(&mut grass_buf, &feature.points, &y_of);
                append_grass_blades(&mut grass_blade_buf, &feature.points, &y_of);
                for (pos, scale) in scatter_trees_in_polygon(&feature.points, *density) {
                    if tree_count >= 80 {
                        break;
                    }
                    let by = y_of(pos);
                    append_tree(&mut trunk_buf, &mut foliage_buf, pos, scale, by);
                    spawn_tree_collider(commands, parent, pos, scale, by);
                    tree_count += 1;
                }
            }
            FeatureKind::Tree { scale } => {
                if tree_count < 80 {
                    if let Some(pos) = feature.points.first() {
                        let by = y_of(*pos);
                        append_tree(&mut trunk_buf, &mut foliage_buf, *pos, *scale, by);
                        spawn_tree_collider(commands, parent, *pos, *scale, by);
                        tree_count += 1;
                    }
                }
            }
            FeatureKind::Decor { prop, yaw } => {
                if decor_count >= 120 {
                    continue;
                }
                if let Some(&pos) = feature.points.first() {
                    let by = y_of(pos);
                    append_decor(&mut decor_bufs, *prop, pos, *yaw, by);
                    if let Some((tf, col)) = decor_collider(*prop, pos, *yaw, by) {
                        let id = commands
                            .spawn((
                                RigidBody::Static,
                                col,
                                tf,
                                Friction::new(0.7),
                                Name::new("DecorCollider"),
                            ))
                            .id();
                        if let Some(p) = parent {
                            commands.entity(p).add_child(id);
                        }
                    }
                    decor_count += 1;
                }
            }
        }
    }

    for (pos, width) in junctions {
        // ≥ 1.2×width so inset ribbon ends (~0.55w) still meet under the pad.
        append_road_junction(&mut road_buf, pos, width * 1.25, &y_of);
    }

    let mut spawn_mesh = |buf: MeshBuf, mat: Handle<StandardMaterial>, name: &str| {
        if buf.is_empty() {
            return;
        }
        let id = commands
            .spawn((
                Mesh3d(meshes.add(buf.into_mesh())),
                MeshMaterial3d(mat),
                Transform::IDENTITY,
                Name::new(name.to_string()),
            ))
            .id();
        if let Some(p) = parent {
            commands.entity(p).add_child(id);
        }
    };

    for (i, buf) in wall_bufs.into_iter().enumerate() {
        spawn_mesh(
            buf,
            palette.building[i].clone(),
            &format!("BuildingWalls_{i}"),
        );
    }
    for (i, buf) in roof_bufs.into_iter().enumerate() {
        spawn_mesh(buf, palette.roof[i].clone(), &format!("Roofs_{i}"));
    }
    spawn_mesh(road_buf, palette.road.clone(), "Roads");
    spawn_mesh(grass_buf, palette.grass.clone(), "Grass");
    spawn_mesh(grass_blade_buf, palette.grass_blades.clone(), "GrassBlades");
    spawn_mesh(trunk_buf, palette.tree_trunk.clone(), "TreeTrunks");
    spawn_mesh(foliage_buf, palette.tree_foliage.clone(), "TreeFoliage");
    spawn_mesh(sign_pole_buf, palette.sign_pole.clone(), "SignPoles");
    for (i, buf) in sign_face_bufs.into_iter().enumerate() {
        if let Some(mat) = palette.sign_faces.get(i) {
            spawn_mesh(buf, mat.clone(), &format!("SignFaces_{i}"));
        }
    }
    for (i, buf) in decor_bufs.into_iter().enumerate() {
        if let Some(mat) = palette.decor.get(i) {
            spawn_mesh(buf, mat.clone(), &format!("Decor_{i}"));
        }
    }
}

fn spawn_tree_collider(
    commands: &mut Commands,
    parent: Option<Entity>,
    pos: Vec2,
    scale: f32,
    base_y: f32,
) {
    let (tf, collider) = tree_collider(pos, scale, base_y);
    let id = commands
        .spawn((
            RigidBody::Static,
            collider,
            tf,
            Friction::new(0.9),
            Name::new("TreeCollider"),
        ))
        .id();
    if let Some(p) = parent {
        commands.entity(p).add_child(id);
    }
}

fn hash_bucket(p: Vec2, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let h = (p.x * 17.0 + p.y * 31.0).abs() as usize;
    h % n
}

/// World-space point the sun looks at (map center).
#[derive(Resource, Clone, Copy)]
pub struct MapCenter(pub Vec3);

/// Marker for the primary directional sun/moon light.
#[derive(Component)]
pub struct SunLight;

#[derive(Resource)]
pub struct MapDataResource(pub MapData);

fn push_junction(junctions: &mut Vec<(Vec2, f32)>, point: Vec2, width: f32) {
    let merge = (width * 0.55).max(1.25);
    if let Some((_, w)) = junctions
        .iter_mut()
        .find(|(p, existing_w)| p.distance(point) <= merge.max(*existing_w * 0.55))
    {
        *w = w.max(width);
    } else {
        junctions.push((point, width));
    }
}

fn bounds(data: &MapData) -> (Vec2, Vec2) {
    let mut min = Vec2::splat(f32::MAX);
    let mut max = Vec2::splat(f32::MIN);
    for f in &data.features {
        for p in &f.points {
            min = min.min(*p);
            max = max.max(*p);
        }
    }
    if !min.is_finite() {
        (Vec2::splat(-50.0), Vec2::splat(50.0))
    } else {
        (min, max)
    }
}
