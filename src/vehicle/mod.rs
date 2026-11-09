//! Arcade vehicle with avian3d collisions + multi-mode camera.

use std::f32::consts::FRAC_PI_3;

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::lights::attach_car_lights;
use crate::style::Palette;
use crate::world::MapDataResource;

pub struct VehiclePlugin;

impl Plugin for VehiclePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraShake>()
            .init_resource::<CameraMode>()
            .add_systems(
                Startup,
                (spawn_vehicle, setup_camera_mode_ui)
                    .chain()
                    .after(crate::world::spawn_world),
            )
            // Drive before the solver so we don't stomp resolved collision velocities.
            .add_systems(
                FixedUpdate,
                drive_vehicle.before(PhysicsSet::StepSimulation),
            )
            .add_systems(
                Update,
                (
                    camera_mode_input,
                    rescue_under_map,
                    update_drive_camera,
                    update_camera_mode_label,
                )
                    .chain()
                    .after(PhysicsSet::Sync),
            );
    }
}

#[derive(Component)]
pub struct PlayerCar;

#[derive(Component)]
pub struct CarState {
    pub speed: f32,
    pub yaw: f32,
}

/// Marker for the main drive camera (name kept for weather fog queries).
#[derive(Component)]
pub struct ChaseCam;

#[derive(Component)]
struct CameraModeLabel;

/// Drive camera presets — cycle with `C` (Shift+C reverse).
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CameraMode {
    #[default]
    Chase,
    Hood,
    Cockpit,
    TopDown,
    Cinematic,
}

impl CameraMode {
    const ALL: [CameraMode; 5] = [
        CameraMode::Chase,
        CameraMode::Hood,
        CameraMode::Cockpit,
        CameraMode::TopDown,
        CameraMode::Cinematic,
    ];

    fn next(self) -> Self {
        let i = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    fn prev(self) -> Self {
        let i = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    fn label(self) -> &'static str {
        match self {
            CameraMode::Chase => "Chase",
            CameraMode::Hood => "Hood",
            CameraMode::Cockpit => "Cockpit",
            CameraMode::TopDown => "Top-down",
            CameraMode::Cinematic => "Cinematic",
        }
    }

    fn shake_scale(self) -> f32 {
        match self {
            CameraMode::Chase => 1.0,
            CameraMode::Hood => 0.55,
            CameraMode::Cockpit => 0.35,
            CameraMode::TopDown => 0.15,
            CameraMode::Cinematic => 0.7,
        }
    }

    fn fov_radians(self) -> f32 {
        match self {
            CameraMode::Chase => FRAC_PI_3, // 60°
            CameraMode::Hood => 1.22,       // ~70°
            CameraMode::Cockpit => 1.35,    // ~77°
            CameraMode::TopDown => 0.95,    // ~54°
            CameraMode::Cinematic => 0.85,  // ~49°
        }
    }

    /// Position lerp rate (higher = snappier). Locked modes use a huge rate.
    fn follow_rate(self) -> f32 {
        match self {
            CameraMode::Chase => 12.0,
            CameraMode::Hood => 22.0,
            CameraMode::Cockpit => 40.0,
            CameraMode::TopDown => 10.0,
            CameraMode::Cinematic => 4.5,
        }
    }
}

/// Trauma-based cam shake (road rumble + collision punch).
#[derive(Resource, Default)]
struct CameraShake {
    /// 0..=1, amplitude uses trauma².
    trauma: f32,
}

const ACCEL: f32 = 18.0;
const BRAKE: f32 = 28.0;
const DRAG: f32 = 2.5;
const MAX_SPEED: f32 = 28.0;
const STEER_RATE: f32 = 2.2;
/// Minimum center height (matches spawn / collider half-height).
pub(crate) const CAR_CLEARANCE: f32 = 0.55;
const SHAKE_DECAY: f32 = 1.6;
const SHAKE_MAX_OFFSET: f32 = 0.55;
const SHAKE_HIT: f32 = 0.45;
/// Continuous road rumble at full speed (trauma floor before²). Was 0.22 — too twitchy.
const SHAKE_RUMBLE: f32 = 0.09;

pub(crate) fn spawn_vehicle(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    palette: Res<Palette>,
    map: Res<MapDataResource>,
    terrain: Res<crate::world::TerrainSeed>,
) {
    let spawn = map.0.spawn;
    let ground_y = terrain
        .0
        .map(|s| crate::osm::height_at(s, spawn.x, spawn.y))
        .unwrap_or(0.0);
    let body = meshes.add(Cuboid::new(1.85, 0.55, 4.0));
    let cabin = meshes.add(Cuboid::new(1.55, 0.5, 1.7));
    let glass = meshes.add(Cuboid::new(1.45, 0.42, 1.55));
    let wheel = meshes.add(Cylinder::new(0.32, 0.22));

    commands
        .spawn((
            PlayerCar,
            CarState {
                speed: 0.0,
                yaw: 0.0,
            },
            Transform::from_xyz(spawn.x, ground_y + CAR_CLEARANCE, spawn.y),
            Visibility::default(),
            RigidBody::Dynamic,
            Collider::cuboid(1.85, 0.7, 4.0),
            // Continuous collision — stops tunneling through building AABBs at speed.
            SweptCcd::default(),
            LockedAxes::new().lock_rotation_x().lock_rotation_z(),
            LinearVelocity::ZERO,
            Friction::new(1.1),
            Restitution::new(0.02),
            Mass(1200.0),
            Name::new("PlayerCar"),
        ))
        .with_children(|parent| {
            parent.spawn((
                Mesh3d(body),
                MeshMaterial3d(palette.car_body.clone()),
                Transform::from_xyz(0.0, 0.1, 0.0),
            ));
            parent.spawn((
                Mesh3d(cabin),
                MeshMaterial3d(palette.car_cabin.clone()),
                Transform::from_xyz(0.0, 0.55, -0.15),
            ));
            parent.spawn((
                Mesh3d(glass),
                MeshMaterial3d(palette.car_glass.clone()),
                Transform::from_xyz(0.0, 0.58, -0.15),
            ));
            for (x, z) in [(-0.85, 1.2), (0.85, 1.2), (-0.85, -1.25), (0.85, -1.25)] {
                parent.spawn((
                    Mesh3d(wheel.clone()),
                    MeshMaterial3d(palette.car_wheel.clone()),
                    Transform::from_xyz(x, -0.15, z)
                        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                ));
            }
            attach_car_lights(parent);
        });

    commands.spawn((
        Camera3d::default(),
        ChaseCam,
        DistanceFog {
            color: Color::srgba(0.62, 0.74, 0.88, 1.0),
            falloff: FogFalloff::Linear {
                start: 80.0,
                end: 280.0,
            },
            ..default()
        },
        // AtmospherePlugin switches to Msaa::Off for volumetric fog stability.
        Msaa::Sample4,
        Transform::from_xyz(spawn.x, 8.0, spawn.y + 12.0)
            .looking_at(Vec3::new(spawn.x, 0.5, spawn.y), Vec3::Y),
        Name::new("DriveCam"),
    ));
}

fn setup_camera_mode_ui(mut commands: Commands, fonts: Res<crate::ui_font::UiFonts>) {
    commands.spawn((
        Text::new(format!("Cam: {}  (C)", CameraMode::Chase.label())),
        fonts.text(16.0),
        TextColor(Color::srgba(0.92, 0.94, 0.96, 0.88)),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(16.0),
            bottom: Val::Px(52.0),
            ..default()
        },
        CameraModeLabel,
        Name::new("CameraModeLabel"),
    ));
}

fn camera_mode_input(keys: Res<ButtonInput<KeyCode>>, mut mode: ResMut<CameraMode>) {
    if !keys.just_pressed(KeyCode::KeyC) {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    *mode = if shift { mode.prev() } else { mode.next() };
}

fn drive_vehicle(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    collisions: Res<Collisions>,
    mut shake: ResMut<CameraShake>,
    mut q: Query<(Entity, &mut Rotation, &mut CarState, &mut LinearVelocity), With<PlayerCar>>,
) {
    let Ok((entity, mut rotation, mut state, mut lin_vel)) = q.get_single_mut() else {
        return;
    };
    let dt = time.delta_secs().max(1e-4);

    let throttle = if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        1.0
    } else {
        0.0
    };
    let brake = if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        1.0
    } else {
        0.0
    };
    let steer = if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        1.0
    } else if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        -1.0
    } else {
        0.0
    };

    state.speed += throttle * ACCEL * dt;
    state.speed -= brake * BRAKE * dt;
    if throttle == 0.0 && brake == 0.0 {
        let sign = state.speed.signum();
        state.speed -= sign * DRAG * dt;
        if state.speed.signum() != sign {
            state.speed = 0.0;
        }
    }
    state.speed = state.speed.clamp(-MAX_SPEED * 0.4, MAX_SPEED);

    let steer_scale = (state.speed.abs() / MAX_SPEED).clamp(0.15, 1.0);
    state.yaw += steer * STEER_RATE * steer_scale * dt;
    *rotation = Rotation::from(Quat::from_rotation_y(state.yaw));

    let forward = Vec3::new(state.yaw.sin(), 0.0, state.yaw.cos());
    let mut desired = forward * state.speed;

    // Strip velocity that pushes into walls. Without this, absolute velocity writes
    // every frame defeat the solver → tunneling → fall under the map.
    let mut hit_wall = false;
    for contacts in collisions.collisions_with_entity(entity) {
        if !contacts.during_current_frame {
            continue;
        }
        for manifold in &contacts.manifolds {
            let n_world = separating_normal_for(entity, contacts, manifold, *rotation);
            let n_h = Vec3::new(n_world.x, 0.0, n_world.z);
            if n_h.length_squared() < 0.25 {
                continue; // mostly ground contact
            }
            let n = n_h.normalize();
            desired = reject_into_wall(desired, n);
            hit_wall = true;
        }
    }
    if hit_wall {
        state.speed *= 0.92;
        let impact = (state.speed.abs() / MAX_SPEED).clamp(0.2, 1.0);
        shake.trauma = (shake.trauma + SHAKE_HIT * impact).min(1.0);
    }

    let y = lin_vel.y;
    lin_vel.0 = desired;
    lin_vel.y = y;
}

/// World normal pointing from obstacle toward the car (separating direction).
fn separating_normal_for(
    car: Entity,
    contacts: &Contacts,
    manifold: &ContactManifold,
    car_rotation: Rotation,
) -> Vec3 {
    if contacts.entity1 == car {
        // Exterior of the car points into the obstacle → flip to get wall→car.
        -(car_rotation * manifold.normal1)
    } else {
        // Exterior of the other body (static building / ground) points toward the car.
        // Static colliders are unrotated; normal1 is already world-aligned enough.
        manifold.normal1
    }
}

/// Remove the component of `v` that moves into the wall (opposite separating normal).
fn reject_into_wall(v: Vec3, separating_normal: Vec3) -> Vec3 {
    let into = v.dot(-separating_normal).max(0.0);
    v + separating_normal * into
}

/// Last-resort: if CCD/solver still fails, yank the car back onto the road surface.
fn rescue_under_map(
    terrain: Res<crate::world::TerrainSeed>,
    mut q: Query<(&mut Position, &mut LinearVelocity), With<PlayerCar>>,
) {
    let Ok((mut pos, mut vel)) = q.get_single_mut() else {
        return;
    };
    let ground = terrain
        .0
        .map(|s| crate::osm::height_at(s, pos.x, pos.z))
        .unwrap_or(0.0);
    if pos.y < ground + 0.15 {
        pos.y = ground + CAR_CLEARANCE;
        vel.y = vel.y.max(0.0);
    }
}

fn update_drive_camera(
    time: Res<Time>,
    mode: Res<CameraMode>,
    mut shake: ResMut<CameraShake>,
    car: Query<&Transform, (With<PlayerCar>, Without<ChaseCam>)>,
    mut cam_tf: Query<&mut Transform, With<ChaseCam>>,
    mut cam_proj: Query<&mut Projection, With<ChaseCam>>,
    car_state: Query<&CarState>,
) {
    let Ok(car_tf) = car.get_single() else {
        return;
    };
    let Ok(mut cam_tf) = cam_tf.get_single_mut() else {
        return;
    };
    let state = car_state.get_single().ok();
    let yaw = state.map(|s| s.yaw).unwrap_or(0.0);
    let speed = state.map(|s| s.speed.abs()).unwrap_or(0.0);

    let dt = time.delta_secs();
    let rumble = speed_rumble(speed / MAX_SPEED, mode.shake_scale());
    shake.trauma = (shake.trauma - SHAKE_DECAY * dt).max(rumble);

    let forward = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    let right = Vec3::new(forward.z, 0.0, -forward.x);
    let car_pos = car_tf.translation;

    let (desired_pos, look_at, up) = match *mode {
        CameraMode::Chase => {
            let back = forward * -10.0;
            (
                car_pos + back + Vec3::Y * 5.5,
                car_pos + Vec3::Y * 1.2,
                Vec3::Y,
            )
        }
        CameraMode::Hood => (
            car_pos + forward * 1.35 + Vec3::Y * 1.05,
            car_pos + forward * 18.0 + Vec3::Y * 0.9,
            Vec3::Y,
        ),
        CameraMode::Cockpit => (
            car_pos + forward * 0.15 + Vec3::Y * 1.15,
            car_pos + forward * 20.0 + Vec3::Y * 1.05,
            Vec3::Y,
        ),
        CameraMode::TopDown => (
            car_pos + Vec3::Y * 55.0 - forward * 2.0,
            car_pos,
            -forward, // keep heading readable on the map
        ),
        CameraMode::Cinematic => {
            let side = right * 9.5;
            (
                car_pos + side + Vec3::Y * 3.2 - forward * 2.5,
                car_pos + forward * 4.0 + Vec3::Y * 1.0,
                Vec3::Y,
            )
        }
    };

    let t = 1.0 - (-mode.follow_rate() * dt).exp();
    cam_tf.translation = cam_tf.translation.lerp(desired_pos, t);

    let shake_offset = camera_shake_offset(time.elapsed_secs(), shake.trauma * mode.shake_scale());
    cam_tf.translation += shake_offset;
    // Mild look coupling — full offset on the target reads as nervous rotation.
    cam_tf.look_at(look_at + shake_offset * 0.15, up);

    if let Ok(mut projection) = cam_proj.get_single_mut() {
        if let Projection::Perspective(persp) = projection.as_mut() {
            let target_fov = mode.fov_radians();
            persp.fov += (target_fov - persp.fov) * (1.0 - (-8.0 * dt).exp());
        }
    }
}

fn update_camera_mode_label(mode: Res<CameraMode>, mut q: Query<&mut Text, With<CameraModeLabel>>) {
    let Ok(mut text) = q.get_single_mut() else {
        return;
    };
    let label = format!("Cam: {}  (C)", mode.label());
    if text.0 != label {
        text.0 = label;
    }
}

/// Speed rumble: quadratic so mid speeds stay calm, only high speed buzzes lightly.
fn speed_rumble(speed_ratio: f32, shake_scale: f32) -> f32 {
    let n = speed_ratio.clamp(0.0, 1.0);
    n * n * SHAKE_RUMBLE * shake_scale
}

fn camera_shake_offset(elapsed: f32, trauma: f32) -> Vec3 {
    let amount = (trauma.clamp(0.0, 1.0).powi(2)) * SHAKE_MAX_OFFSET;
    if amount < 1e-4 {
        return Vec3::ZERO;
    }
    // Lower frequencies — high Hz sin products felt like vibration, not rumble.
    let nx = (elapsed * 17.0).sin() * (elapsed * 9.3 + 1.3).cos();
    let ny = (elapsed * 19.5 + 2.1).sin() * (elapsed * 11.1).cos();
    let nz = (elapsed * 13.7 + 0.7).cos() * (elapsed * 8.4).sin();
    Vec3::new(nx, ny * 0.55, nz) * amount
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_into_wall_stops_head_on() {
        // Separating normal points from wall toward car (+Z ⇒ car is on +Z side).
        let n = Vec3::Z;
        let v = Vec3::new(0.0, 0.0, -10.0); // driving into the wall (−Z)
        let out = reject_into_wall(v, n);
        assert!(out.z.abs() < 1e-4, "expected ~0 z, got {out:?}");
    }

    #[test]
    fn reject_into_wall_keeps_parallel() {
        let n = Vec3::Z;
        let v = Vec3::new(5.0, 0.0, 0.0);
        let out = reject_into_wall(v, n);
        assert!((out.x - 5.0).abs() < 1e-4);
    }

    #[test]
    fn camera_mode_cycles() {
        let mut m = CameraMode::Chase;
        for _ in 0..CameraMode::ALL.len() {
            m = m.next();
        }
        assert_eq!(m, CameraMode::Chase);
        assert_eq!(CameraMode::Chase.prev(), CameraMode::Cinematic);
        assert!(CameraMode::Cockpit.shake_scale() < CameraMode::Chase.shake_scale());
    }

    #[test]
    fn speed_rumble_stays_soft() {
        assert_eq!(speed_rumble(0.0, 1.0), 0.0);
        assert!(speed_rumble(0.5, 1.0) < speed_rumble(1.0, 1.0));
        assert!(speed_rumble(1.0, 1.0) <= SHAKE_RUMBLE + 1e-5);
        // Half speed should be ~¼ of full (quadratic).
        assert!((speed_rumble(0.5, 1.0) - SHAKE_RUMBLE * 0.25).abs() < 1e-5);
    }
}
