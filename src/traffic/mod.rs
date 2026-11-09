//! NPC traffic that follows road polylines and streams around the player.
//!
//! Called from: `src/main.rs` via `TrafficPlugin`.
//! Infinite proc: roads from `generate_chunk` near the player; finite: MapData roads.
//! User: trafic qui se régénère selon la position du joueur.

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::osm::parse::MapFeature;
use crate::osm::{chunk_size_m, generate_chunk, FeatureKind, MapData};
use crate::style::Palette;
use crate::vehicle::{PlayerCar, CAR_CLEARANCE};
use crate::world::{MapDataResource, ProcStream};

pub struct TrafficPlugin;

impl Plugin for TrafficPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_traffic.after(crate::vehicle::spawn_vehicle))
            .add_systems(
                Update,
                stream_traffic
                    .after(crate::vehicle::spawn_vehicle)
                    .run_if(resource_exists::<TrafficAssets>),
            )
            .add_systems(
                FixedUpdate,
                drive_npc_traffic.before(PhysicsSet::StepSimulation),
            );
    }
}

#[derive(Component)]
struct NpcCar {
    points: Vec<Vec2>,
    seg: usize,
    /// Progress along current segment 0..1.
    t: f32,
    /// +1 forward along polyline, −1 reverse.
    dir: f32,
    cruise: f32,
    /// Lateral lane offset in meters (left of travel direction).
    lane: f32,
}

#[derive(Resource)]
struct TrafficAssets {
    body: Handle<Mesh>,
    cabin: Handle<Mesh>,
    glass: Handle<Mesh>,
    wheel: Handle<Mesh>,
}

struct RoadPath {
    points: Vec<Vec2>,
    /// Suggested cruise speed (m/s).
    speed: f32,
}

const MAX_NPCS: usize = 20;
const LANE_OFFSET: f32 = 1.55;
const SPAWN_CLEAR_PLAYER: f32 = 18.0;
const DESPAWN_M: f32 = 300.0;
const SPAWN_MIN_M: f32 = 45.0;
const SPAWN_MAX_M: f32 = 240.0;
const STREAM_INTERVAL_S: f32 = 0.8;
const PATH_CHUNK_RADIUS: i32 = 2;

fn setup_traffic(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    palette: Res<Palette>,
    map: Res<MapDataResource>,
    stream: Option<Res<ProcStream>>,
) {
    let assets = TrafficAssets {
        body: meshes.add(Cuboid::new(1.85, 0.55, 4.0)),
        cabin: meshes.add(Cuboid::new(1.55, 0.5, 1.7)),
        glass: meshes.add(Cuboid::new(1.45, 0.42, 1.55)),
        wheel: meshes.add(Cylinder::new(0.32, 0.22)),
    };

    let origin = map.0.spawn;
    let paths = if let Some(stream) = stream.as_deref() {
        paths_around(stream.seed, origin, PATH_CHUNK_RADIUS)
    } else {
        collect_road_paths(&map.0)
    };

    let slots = plan_spawns(&paths, origin, &[], MAX_NPCS, true);
    for (i, slot) in slots.into_iter().enumerate() {
        spawn_npc(
            &mut commands,
            &assets,
            &palette,
            &paths[slot.path],
            &slot,
            i,
        );
    }
    commands.insert_resource(assets);
}

#[allow(clippy::too_many_arguments)]
fn stream_traffic(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<TrafficAssets>,
    palette: Res<Palette>,
    map: Res<MapDataResource>,
    stream: Option<Res<ProcStream>>,
    player: Query<&Transform, With<PlayerCar>>,
    npcs: Query<(Entity, &Transform), With<NpcCar>>,
    mut cooldown: Local<f32>,
    mut paint_i: Local<usize>,
) {
    *cooldown -= time.delta_secs();
    if *cooldown > 0.0 {
        return;
    }
    *cooldown = STREAM_INTERVAL_S;

    let Ok(player_tf) = player.get_single() else {
        return;
    };
    let origin = Vec2::new(player_tf.translation.x, player_tf.translation.z);

    let mut alive = 0usize;
    let mut existing: Vec<Vec2> = Vec::new();
    for (entity, tf) in &npcs {
        let p = Vec2::new(tf.translation.x, tf.translation.z);
        if p.distance(origin) > DESPAWN_M {
            commands.entity(entity).despawn_recursive();
        } else {
            alive += 1;
            existing.push(p);
        }
    }

    if alive >= MAX_NPCS {
        return;
    }

    let paths = if let Some(stream) = stream.as_deref() {
        paths_around(stream.seed, origin, PATH_CHUNK_RADIUS)
    } else {
        collect_road_paths(&map.0)
    };
    if paths.is_empty() {
        return;
    }

    let need = MAX_NPCS - alive;
    let slots = plan_spawns(&paths, origin, &existing, need, false);
    for slot in slots {
        spawn_npc(
            &mut commands,
            &assets,
            &palette,
            &paths[slot.path],
            &slot,
            *paint_i,
        );
        *paint_i = paint_i.wrapping_add(1);
    }
}

fn spawn_npc(
    commands: &mut Commands,
    assets: &TrafficAssets,
    palette: &Palette,
    path: &RoadPath,
    slot: &SpawnSlot,
    paint_i: usize,
) {
    let (pos, tangent) = sample_path(&path.points, slot.seg, slot.t, slot.dir, slot.lane);
    let yaw = tangent.x.atan2(tangent.y);
    let body_mat = palette.traffic_bodies[paint_i % palette.traffic_bodies.len()].clone();

    commands
        .spawn((
            NpcCar {
                points: path.points.clone(),
                seg: slot.seg,
                t: slot.t,
                dir: slot.dir,
                cruise: path.speed * slot.speed_mul,
                lane: slot.lane,
            },
            Transform::from_xyz(pos.x, CAR_CLEARANCE, pos.y)
                .with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            RigidBody::Dynamic,
            Collider::cuboid(1.85, 0.7, 4.0),
            SweptCcd::default(),
            LockedAxes::new().lock_rotation_x().lock_rotation_z(),
            LinearVelocity::ZERO,
            Friction::new(1.0),
            Restitution::new(0.05),
            Mass(1100.0),
            Name::new(format!("NpcCar_{paint_i}")),
        ))
        .with_children(|parent| {
            parent.spawn((
                Mesh3d(assets.body.clone()),
                MeshMaterial3d(body_mat),
                Transform::from_xyz(0.0, 0.1, 0.0),
            ));
            parent.spawn((
                Mesh3d(assets.cabin.clone()),
                MeshMaterial3d(palette.car_cabin.clone()),
                Transform::from_xyz(0.0, 0.55, -0.15),
            ));
            parent.spawn((
                Mesh3d(assets.glass.clone()),
                MeshMaterial3d(palette.car_glass.clone()),
                Transform::from_xyz(0.0, 0.58, -0.15),
            ));
            for (x, z) in [(-0.85, 1.2), (0.85, 1.2), (-0.85, -1.25), (0.85, -1.25)] {
                parent.spawn((
                    Mesh3d(assets.wheel.clone()),
                    MeshMaterial3d(palette.car_wheel.clone()),
                    Transform::from_xyz(x, -0.15, z)
                        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                ));
            }
        });
}

struct SpawnSlot {
    path: usize,
    seg: usize,
    t: f32,
    dir: f32,
    lane: f32,
    speed_mul: f32,
}

fn paths_around(seed: u64, center: Vec2, chunk_radius: i32) -> Vec<RoadPath> {
    let cs = chunk_size_m();
    let pcx = (center.x / cs).floor() as i32;
    let pcz = (center.y / cs).floor() as i32;
    let mut features: Vec<MapFeature> = Vec::new();
    for cz in (pcz - chunk_radius)..=(pcz + chunk_radius) {
        for cx in (pcx - chunk_radius)..=(pcx + chunk_radius) {
            features.extend(generate_chunk(seed, cx, cz));
        }
    }
    collect_road_paths_from_features(&features)
}

fn collect_road_paths(map: &MapData) -> Vec<RoadPath> {
    collect_road_paths_from_features(&map.features)
}

fn collect_road_paths_from_features(features: &[MapFeature]) -> Vec<RoadPath> {
    let mut paths = Vec::new();
    for f in features {
        let FeatureKind::Road {
            width: _, highway, ..
        } = &f.kind
        else {
            continue;
        };
        if f.points.len() < 2 {
            continue;
        }
        let len: f32 = f.points.windows(2).map(|w| (w[1] - w[0]).length()).sum();
        if len < 18.0 {
            continue;
        }
        paths.push(RoadPath {
            points: f.points.clone(),
            speed: highway_cruise(highway.as_str()),
        });
    }
    paths.sort_by(|a, b| {
        let la: f32 = a.points.windows(2).map(|w| (w[1] - w[0]).length()).sum();
        let lb: f32 = b.points.windows(2).map(|w| (w[1] - w[0]).length()).sum();
        lb.total_cmp(&la)
    });
    paths
}

fn highway_cruise(highway: &str) -> f32 {
    match highway {
        "motorway" | "trunk" => 16.0,
        "primary" => 13.0,
        "secondary" => 11.0,
        "tertiary" => 9.5,
        "residential" | "unclassified" | "living_street" => 7.5,
        "service" | "track" => 5.0,
        _ => 8.0,
    }
}

/// `near_spawn`: allow close-ish pack at start; streaming uses a ring around the player.
fn plan_spawns(
    paths: &[RoadPath],
    origin: Vec2,
    existing: &[Vec2],
    max: usize,
    near_spawn: bool,
) -> Vec<SpawnSlot> {
    let mut slots = Vec::new();
    if max == 0 || paths.is_empty() {
        return slots;
    }
    let mut path_i = 0usize;
    let mut along = 25.0f32;
    let mut side = 0usize;
    let mut attempts = 0usize;

    while slots.len() < max && attempts < paths.len() * 24 {
        attempts += 1;
        let path = &paths[path_i % paths.len()];
        let total: f32 = path.points.windows(2).map(|w| (w[1] - w[0]).length()).sum();
        if along > total - 8.0 {
            path_i += 1;
            along = 20.0 + (path_i % 5) as f32 * 7.0;
            continue;
        }

        let (seg, t) = distance_to_seg_t(&path.points, along);
        let dir = if side.is_multiple_of(2) { 1.0 } else { -1.0 };
        let lane = LANE_OFFSET * dir;
        let (pos, _) = sample_path(&path.points, seg, t, dir, lane);
        let d = pos.distance(origin);
        let in_band = if near_spawn {
            d >= SPAWN_CLEAR_PLAYER
        } else {
            (SPAWN_MIN_M..=SPAWN_MAX_M).contains(&d)
        };
        let clear_npc = slots.iter().all(|s: &SpawnSlot| {
            let (op, _) = sample_path(&paths[s.path].points, s.seg, s.t, s.dir, s.lane);
            op.distance(pos) >= 12.0
        }) && existing.iter().all(|op| op.distance(pos) >= 12.0);

        if in_band && clear_npc {
            slots.push(SpawnSlot {
                path: path_i % paths.len(),
                seg,
                t,
                dir,
                lane,
                speed_mul: 0.85 + (slots.len() % 4) as f32 * 0.08,
            });
        }

        along += 35.0 + (slots.len() % 3) as f32 * 10.0;
        side += 1;
        if side.is_multiple_of(2) {
            path_i += 1;
            along = 15.0 + (path_i % 7) as f32 * 9.0;
        }
    }
    slots
}

fn distance_to_seg_t(points: &[Vec2], mut dist: f32) -> (usize, f32) {
    for i in 0..points.len().saturating_sub(1) {
        let len = (points[i + 1] - points[i]).length().max(1e-4);
        if dist <= len {
            return (i, (dist / len).clamp(0.0, 1.0));
        }
        dist -= len;
    }
    let last = points.len().saturating_sub(2);
    (last, 1.0)
}

/// Sample centerline, then offset by `lane` to the left of travel (`dir`).
fn sample_path(points: &[Vec2], seg: usize, t: f32, dir: f32, lane: f32) -> (Vec2, Vec2) {
    let i = seg.min(points.len().saturating_sub(2));
    let a = points[i];
    let b = points[i + 1];
    let center = a.lerp(b, t.clamp(0.0, 1.0));
    let mut tangent = (b - a).normalize_or_zero();
    if dir < 0.0 {
        tangent = -tangent;
    }
    let left = Vec2::new(-tangent.y, tangent.x);
    (center + left * lane, tangent)
}

fn drive_npc_traffic(
    time: Res<Time>,
    mut q: Query<(
        Entity,
        &mut NpcCar,
        &mut Rotation,
        &mut LinearVelocity,
        &mut Position,
    )>,
) {
    let dt = time.delta_secs().max(1e-4);

    let others: Vec<(u64, Vec2, Vec2)> = q
        .iter()
        .map(|(e, npc, _, _, pos)| {
            let (_, tan) = sample_path(&npc.points, npc.seg, npc.t, npc.dir, npc.lane);
            (e.to_bits(), Vec2::new(pos.x, pos.z), tan)
        })
        .collect();

    for (entity, mut npc, mut rotation, mut lin_vel, mut pos) in &mut q {
        if npc.points.len() < 2 {
            continue;
        }
        let self_id = entity.to_bits();
        let self_pos = Vec2::new(pos.x, pos.z);
        let (_, self_fwd) = sample_path(&npc.points, npc.seg, npc.t, npc.dir, npc.lane);
        let speed = cruise_with_gaps(self_pos, self_fwd, npc.cruise, self_id, &others);

        let dist = speed * dt;
        let mut seg = npc.seg;
        let mut t = npc.t;
        let mut dir = npc.dir;
        advance_along_path(&npc.points, &mut seg, &mut t, &mut dir, dist);
        npc.seg = seg;
        npc.t = t;
        npc.dir = dir;
        npc.lane = LANE_OFFSET * dir.signum();

        let (xz, tangent) = sample_path(&npc.points, seg, t, dir, npc.lane);
        let blocked = others.iter().any(|(oid, op, _)| {
            *oid != self_id
                && op.distance(xz) < 4.2
                && op.distance(self_pos) > 0.05
                && (*op - self_pos).normalize_or_zero().dot(tangent) > 0.1
        });
        let yaw = tangent.x.atan2(tangent.y);
        *rotation = Rotation::from(Quat::from_rotation_y(yaw));

        if !blocked {
            pos.x = xz.x;
            pos.z = xz.y;
        }
        if pos.y < 0.2 {
            pos.y = CAR_CLEARANCE;
        }

        let applied = if blocked { 0.0 } else { speed };
        let desired = Vec3::new(tangent.x, 0.0, tangent.y) * applied;
        let y = lin_vel.y;
        lin_vel.0 = desired;
        lin_vel.y = y;
    }
}

fn cruise_with_gaps(
    self_pos: Vec2,
    self_fwd: Vec2,
    cruise: f32,
    self_id: u64,
    others: &[(u64, Vec2, Vec2)],
) -> f32 {
    let mut speed = cruise;
    for &(oid, op, ofwd) in others {
        if oid == self_id {
            continue;
        }
        let delta = op - self_pos;
        let dist = delta.length();
        if dist < 0.4 {
            return 0.0;
        }
        let ahead = delta.normalize_or_zero().dot(self_fwd);
        let align = ofwd.dot(self_fwd).abs();

        if ahead > 0.3 && dist < 14.0 && align > 0.55 {
            const STOP: f32 = 5.5;
            const SLOW: f32 = 12.0;
            if dist <= STOP {
                speed = 0.0;
            } else {
                speed = speed.min(cruise * ((dist - STOP) / (SLOW - STOP)).clamp(0.0, 1.0));
            }
        }

        if align < 0.55 && dist < 9.0 && ahead > -0.25 && self_id > oid {
            speed = speed.min(cruise * (dist / 9.0).clamp(0.0, 0.4));
        }
    }
    speed
}

fn advance_along_path(points: &[Vec2], seg: &mut usize, t: &mut f32, dir: &mut f32, mut dist: f32) {
    if points.len() < 2 || dist <= 0.0 {
        return;
    }
    let last = points.len() - 2;
    let mut guard = 0;
    while dist > 0.0 && guard < 64 {
        guard += 1;
        *seg = (*seg).min(last);
        let a = points[*seg];
        let b = points[*seg + 1];
        let seg_len = (b - a).length().max(1e-4);

        if *dir > 0.0 {
            let remain = (1.0 - *t) * seg_len;
            if dist < remain {
                *t += dist / seg_len;
                return;
            }
            dist -= remain;
            if *seg >= last {
                *dir = -1.0;
                *t = 1.0;
            } else {
                *seg += 1;
                *t = 0.0;
            }
        } else {
            let remain = *t * seg_len;
            if dist < remain {
                *t -= dist / seg_len;
                return;
            }
            dist -= remain;
            if *seg == 0 {
                *dir = 1.0;
                *t = 0.0;
            } else {
                *seg -= 1;
                *t = 1.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advance_reaches_end_and_reverses() {
        let pts = vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)];
        let mut seg = 0usize;
        let mut t = 0.0f32;
        let mut dir = 1.0f32;
        advance_along_path(&pts, &mut seg, &mut t, &mut dir, 12.0);
        assert!(dir < 0.0, "should reverse after overshooting end");
    }

    #[test]
    fn sample_lane_offsets_left_of_travel() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)];
        let (pos, tan) = sample_path(&pts, 0, 0.5, 1.0, 2.0);
        assert!((pos.x - 5.0).abs() < 1e-3);
        assert!(pos.y > 0.0);
        assert!(tan.x > 0.9);
    }

    #[test]
    fn gaps_stop_for_car_ahead() {
        let me = Vec2::ZERO;
        let fwd = Vec2::Y;
        let others = [(2, Vec2::new(0.0, 4.0), Vec2::Y)];
        assert_eq!(cruise_with_gaps(me, fwd, 10.0, 1, &others), 0.0);
    }

    #[test]
    fn crossing_higher_id_yields() {
        let me = Vec2::ZERO;
        let fwd = Vec2::Y;
        let others = [(1, Vec2::new(3.0, 3.0), Vec2::X)];
        let slow = cruise_with_gaps(me, fwd, 10.0, 9, &others);
        let prio = cruise_with_gaps(me, fwd, 10.0, 0, &others);
        assert!(slow < prio);
        assert!(slow < 10.0);
    }

    #[test]
    fn collects_paths_from_geojson() {
        let map = crate::osm::parse::parse_geojson(
            r#"{
          "type":"FeatureCollection",
          "features":[
            {"type":"Feature","properties":{"highway":"primary","name":"A"},
             "geometry":{"type":"LineString","coordinates":[
               [4.83,45.76],[4.831,45.76],[4.832,45.76],[4.833,45.76]
             ]}}
          ]
        }"#,
        )
        .unwrap();
        let paths = collect_road_paths(&map);
        assert_eq!(paths.len(), 1);
        assert!(paths[0].speed > 10.0);
    }

    #[test]
    fn paths_around_player_are_local() {
        let origin = Vec2::new(500.0, -200.0);
        let paths = paths_around(7, origin, 1);
        assert!(!paths.is_empty());
        for p in &paths {
            let near = p.points.iter().any(|pt| pt.distance(origin) < 400.0);
            assert!(near, "expected path near player");
        }
    }

    #[test]
    fn streaming_spawns_in_ring() {
        let paths = paths_around(3, Vec2::ZERO, 2);
        let slots = plan_spawns(&paths, Vec2::ZERO, &[], 8, false);
        for s in &slots {
            let (pos, _) = sample_path(&paths[s.path].points, s.seg, s.t, s.dir, s.lane);
            let d = pos.length();
            assert!(
                (SPAWN_MIN_M - 1.0..=SPAWN_MAX_M + 1.0).contains(&d),
                "d={d}"
            );
        }
    }
}
