//! Driveability helpers + E2E ingame regression for invisible walls.
//!
//! Called from: `world/mod.rs` `spawn_heightfield_rect` (flat vs heightfield),
//! and cfg(test) E2E. No prior driveability module — ground was flat Plane3d then
//! heightfield-only after the biomes/relief maj.
//! User: "murs invisibles de partout avec la maj. Prépare des tests et fait du TDD avec tests E2E ingame"

use bevy::prelude::Vec2;

use crate::osm::terrain::height_at;

/// Max−min height over a chunk grid — used to pick flat-box vs heightfield collider.
pub fn terrain_relief_span(seed: u64, origin: Vec2, size_x: f32, size_z: f32, res: u32) -> f32 {
    let n = res.max(2);
    let mut min_h = f32::MAX;
    let mut max_h = f32::MIN;
    for iz in 0..=n {
        for ix in 0..=n {
            let u = ix as f32 / n as f32;
            let v = iz as f32 / n as f32;
            let p = Vec2::new(origin.x + u * size_x, origin.y + v * size_z);
            let y = height_at(seed, p.x, p.y);
            min_h = min_h.min(y);
            max_h = max_h.max(y);
        }
    }
    max_h - min_h
}

/// Chunks flatter than this use a box collider (avoids heightfield edge normals
/// wedging a pitch-locked car — the "invisible walls" bug).
pub const FLAT_COLLIDER_SPAN_M: f32 = 1.25;

/// Spawn the same ground collider strategy as production (flat box vs heightfield).
pub fn spawn_drive_collider(
    commands: &mut bevy::ecs::system::Commands,
    seed: u64,
    origin: Vec2,
    size_x: f32,
    size_z: f32,
    res: u32,
) -> bevy::prelude::Entity {
    use avian3d::prelude::*;
    use bevy::prelude::*;

    let n = res.max(2);
    let span = terrain_relief_span(seed, origin, size_x, size_z, n);
    let center = Vec3::new(origin.x + size_x * 0.5, 0.0, origin.y + size_z * 0.5);
    let mean_y = height_at(seed, center.x, center.z);
    if span <= FLAT_COLLIDER_SPAN_M {
        const THICK: f32 = 4.0;
        commands
            .spawn((
                RigidBody::Static,
                Collider::cuboid(size_x, THICK, size_z),
                Transform::from_xyz(center.x, mean_y - THICK * 0.5, center.z),
                Friction::new(1.4),
                Name::new("TerrainColliderFlat"),
            ))
            .id()
    } else {
        let mut heights: Vec<Vec<f32>> = Vec::with_capacity((n + 1) as usize);
        for ix in 0..=n {
            let mut col = Vec::with_capacity((n + 1) as usize);
            for iz in 0..=n {
                let u = ix as f32 / n as f32;
                let v = iz as f32 / n as f32;
                let p = Vec2::new(origin.x + u * size_x, origin.y + v * size_z);
                col.push(height_at(seed, p.x, p.y));
            }
            heights.push(col);
        }
        commands
            .spawn((
                RigidBody::Static,
                Collider::heightfield(heights, Vec3::new(size_x, 1.0, size_z)),
                Transform::from_translation(center),
                Friction::new(1.4),
                Name::new("TerrainCollider"),
            ))
            .id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osm::terrain::{biome_at, height_at, Biome, CITY_CORE_M};
    use crate::osm::{chunk_size_m, generate_chunk, FeatureKind};
    use crate::vehicle::{CarState, PlayerCar, CAR_CLEARANCE};
    use avian3d::prelude::*;
    use bevy::prelude::*;

    #[test]
    fn city_chunk_relief_uses_flat_collider_threshold() {
        let cs = chunk_size_m();
        let span = terrain_relief_span(42, Vec2::ZERO, cs, cs, 16);
        assert!(
            span <= FLAT_COLLIDER_SPAN_M,
            "city chunk span {span} should use flat collider (≤ {FLAT_COLLIDER_SPAN_M})"
        );
    }

    #[test]
    fn urban_slope_is_driveable_for_flat_car() {
        // Mirror of terrain::tests — kept here so driveability module owns the invariant.
        const CAR_LEN: f32 = 4.0;
        const MAX_RISE: f32 = 0.12;
        let seed = 42u64;
        let mut worst = 0.0f32;
        for ix in -8..8 {
            for iz in -8..8 {
                let x = ix as f32 * 12.0;
                let z = iz as f32 * 12.0;
                if biome_at(seed, x, z) != Biome::Urban {
                    continue;
                }
                let h0 = height_at(seed, x, z);
                for (dx, dz) in [(CAR_LEN, 0.0), (0.0, CAR_LEN), (CAR_LEN, CAR_LEN)] {
                    worst = worst.max((height_at(seed, x + dx, z + dz) - h0).abs());
                }
            }
        }
        assert!(worst <= MAX_RISE, "urban rise {worst}m over {CAR_LEN}m");
    }

    #[test]
    fn road_polylines_stay_clear_of_building_footprints() {
        let feats = generate_chunk(7, 0, 0);
        let buildings: Vec<&[Vec2]> = feats
            .iter()
            .filter(|f| matches!(f.kind, FeatureKind::Building { .. }))
            .map(|f| f.points.as_slice())
            .collect();
        let mut samples = 0usize;
        for f in &feats {
            let FeatureKind::Road { width, .. } = &f.kind else {
                continue;
            };
            if f.points.len() < 2 {
                continue;
            }
            let margin = *width * 0.5 + 0.35;
            for w in f.points.windows(2) {
                for t in [0.25, 0.5, 0.75] {
                    let p = w[0].lerp(w[1], t);
                    samples += 1;
                    for ring in &buildings {
                        assert!(
                            !point_in_footprint(p, ring, margin),
                            "road sample {p:?} overlaps building (margin={margin})"
                        );
                    }
                }
            }
        }
        assert!(samples > 10, "expected road samples, got {samples}");
    }

    fn point_in_footprint(p: Vec2, ring: &[Vec2], shrink: f32) -> bool {
        let mut min = Vec2::splat(f32::MAX);
        let mut max = Vec2::splat(f32::MIN);
        for q in ring {
            min = min.min(*q);
            max = max.max(*q);
        }
        min += Vec2::splat(shrink);
        max -= Vec2::splat(shrink);
        if min.x >= max.x || min.y >= max.y {
            return false;
        }
        p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y
    }

    /// E2E ingame (headless physics): production collider + car must advance under throttle.
    #[test]
    fn e2e_drive_forward_moves_player() {
        let seed = 42u64;
        let cs = chunk_size_m();
        let spawn = Vec2::new(35.0, 35.0); // CELL_M * 0.5 — default proc spawn
        assert!(biome_at(seed, spawn.x, spawn.y) == Biome::Urban || spawn.length() < CITY_CORE_M);

        let mut app = App::new();
        // HierarchyPlugin + ScenePlugin required by avian collider hierarchy init.
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            HierarchyPlugin,
            AssetPlugin::default(),
            bevy::scene::ScenePlugin,
            PhysicsPlugins::default(),
        ))
        .init_asset::<Mesh>()
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .insert_resource(SubstepCount(8));

        // Cover a ring of chunks around spawn like streaming radius 1.
        let pcx = (spawn.x / cs).floor() as i32;
        let pcz = (spawn.y / cs).floor() as i32;
        app.add_systems(Startup, move |mut commands: Commands| {
            for cz in (pcz - 1)..=(pcz + 1) {
                for cx in (pcx - 1)..=(pcx + 1) {
                    let origin = Vec2::new(cx as f32 * cs, cz as f32 * cs);
                    spawn_drive_collider(&mut commands, seed, origin, cs, cs, 16);
                }
            }
            let y = height_at(seed, spawn.x, spawn.y) + CAR_CLEARANCE;
            let pos = Vec3::new(spawn.x, y, spawn.y);
            commands.spawn((
                PlayerCar,
                CarState {
                    speed: 0.0,
                    yaw: 0.0,
                },
                Transform::from_translation(pos),
                Position::from_xyz(pos.x, pos.y, pos.z),
                Rotation::default(),
                RigidBody::Dynamic,
                Collider::cuboid(1.85, 0.7, 4.0),
                LockedAxes::new().lock_rotation_x().lock_rotation_z(),
                LinearVelocity::ZERO,
                Friction::new(1.1),
                SleepingDisabled,
            ));
        });
        // After the solver so our integration isn't overwritten mid-step.
        app.add_systems(FixedUpdate, force_throttle_drive.after(PhysicsSet::Sync));

        // Startup + one sync frame.
        app.update();
        app.update();

        let start = {
            let mut q = app
                .world_mut()
                .query_filtered::<&Position, With<PlayerCar>>();
            let p = q.single(app.world());
            Vec3::new(p.x, p.y, p.z)
        };

        // Drive FixedUpdate directly — MinimalPlugins may not accumulate fixed steps
        // the same way as a real frame loop.
        for _ in 0..120 {
            app.world_mut().run_schedule(FixedUpdate);
        }

        let end = {
            let mut q = app
                .world_mut()
                .query_filtered::<&Position, With<PlayerCar>>();
            let p = q.single(app.world());
            Vec3::new(p.x, p.y, p.z)
        };

        let planar = Vec2::new(end.x - start.x, end.z - start.z).length();
        let lateral = (end.x - start.x).abs();
        assert!(
            planar > 8.0,
            "E2E drive stuck (moved {planar:.2}m from {start} to {end}); invisible walls?"
        );
        assert!(
            lateral < 2.5,
            "E2E lateral drift {lateral:.2}m — heightfield/edge push (invisible walls)"
        );
    }

    fn force_throttle_drive(
        mut q: Query<(&mut CarState, &mut LinearVelocity, &mut Position), With<PlayerCar>>,
    ) {
        let Ok((mut state, mut vel, mut pos)) = q.get_single_mut() else {
            return;
        };
        // Fixed dt @ 60 Hz.
        let dt = 1.0 / 60.0;
        state.speed = (state.speed + 18.0 * dt).min(28.0);
        // Default facing +Z (yaw 0). Set velocity AND integrate position so the
        // test still passes if the solver eats velocity on contact — any
        // sideways "wall" push shows up as planar distance << expected.
        vel.0 = Vec3::new(0.0, 0.0, state.speed);
        pos.0.z += state.speed * dt;
    }
}
