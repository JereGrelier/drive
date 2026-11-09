//! Night lighting: headlights, street lamps (streamed), facade window glow.
//!
//! Callers: `src/main.rs` registers `LightsPlugin`; `vehicle::spawn_vehicle`
//! calls `attach_car_lights`. Lamps follow the player like traffic/chunks.
//! User: toutes les rues n'ont pas de nom / de lampes

use bevy::prelude::*;

use crate::osm::{chunk_size_m, generate_chunk, FeatureKind};
use crate::style::Palette;
use crate::vehicle::PlayerCar;
use crate::weather::DayNight;
use crate::world::{MapDataResource, ProcStream};

pub struct LightsPlugin;

impl Plugin for LightsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            setup_lamp_assets
                .after(crate::world::spawn_world)
                .after(crate::style::setup_palette),
        )
        .add_systems(
            Update,
            (
                stream_street_lamps.run_if(resource_exists::<LampAssets>),
                sync_night_lights,
            ),
        );
    }
}

#[derive(Component)]
pub struct Headlight;

#[derive(Component)]
struct StreetLamp;

#[derive(Component)]
struct StreetLampLight;

#[derive(Component)]
struct CarFillLight;

#[derive(Resource)]
struct LampAssets {
    pole: Handle<Mesh>,
    bulb: Handle<Mesh>,
    pole_mat: Handle<StandardMaterial>,
    bulb_mat: Handle<StandardMaterial>,
}

const MAX_STREET_LAMPS: usize = 96;
const LAMP_SPACING_M: f32 = 28.0;
const LAMP_MIN_SEP_M: f32 = 18.0;
const LAMP_DESPAWN_M: f32 = 320.0;
const LAMP_SPAWN_MAX_M: f32 = 260.0;
const LAMP_STREAM_S: f32 = 0.9;
const LAMP_CHUNK_RADIUS: i32 = 2;

fn setup_lamp_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(LampAssets {
        pole: meshes.add(Cylinder::new(0.08, 5.2)),
        bulb: meshes.add(Sphere::new(0.22)),
        pole_mat: materials.add(StandardMaterial {
            base_color: Color::srgb(0.18, 0.18, 0.2),
            perceptual_roughness: 0.7,
            metallic: 0.4,
            ..default()
        }),
        bulb_mat: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.92, 0.7),
            emissive: LinearRgba::rgb(4.0, 3.2, 1.6),
            perceptual_roughness: 1.0,
            ..default()
        }),
    });
}

#[allow(clippy::too_many_arguments)]
fn stream_street_lamps(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<LampAssets>,
    map: Res<MapDataResource>,
    stream: Option<Res<ProcStream>>,
    player: Query<&Transform, With<PlayerCar>>,
    lamps: Query<(Entity, &Transform), With<StreetLamp>>,
    mut cooldown: Local<f32>,
) {
    *cooldown -= time.delta_secs();
    if *cooldown > 0.0 {
        return;
    }
    *cooldown = LAMP_STREAM_S;

    let Ok(player_tf) = player.get_single() else {
        return;
    };
    let origin = Vec2::new(player_tf.translation.x, player_tf.translation.z);

    let mut alive = 0usize;
    let mut placed: Vec<Vec2> = Vec::new();
    for (entity, tf) in &lamps {
        let p = Vec2::new(tf.translation.x, tf.translation.z);
        if p.distance(origin) > LAMP_DESPAWN_M {
            commands.entity(entity).despawn_recursive();
        } else {
            alive += 1;
            placed.push(p);
        }
    }

    if alive >= MAX_STREET_LAMPS {
        return;
    }

    let features = local_road_features(&map, stream.as_deref(), origin);
    let mut next_budget = MAX_STREET_LAMPS - alive;

    for feature in &features {
        let FeatureKind::Road { .. } = feature.kind else {
            continue;
        };
        if feature.points.len() < 2 || next_budget == 0 {
            continue;
        }
        let mut dist = 0.0f32;
        // Offset phase per road so lamps aren't grid-aligned.
        let mut next_at = 6.0 + (feature.points[0].x.abs() + feature.points[0].y.abs()) % 17.0;
        for w in feature.points.windows(2) {
            let a = w[0];
            let b = w[1];
            let seg = (b - a).length();
            if seg < 1e-3 {
                continue;
            }
            let dir = (b - a) / seg;
            let mut t = 0.0;
            while t < seg {
                if dist + t >= next_at {
                    let p = a + dir * t;
                    let side = Vec2::new(-dir.y, dir.x) * 3.4;
                    let foot = p + side;
                    let d = foot.distance(origin);
                    if d <= LAMP_SPAWN_MAX_M
                        && d > 5.0
                        && placed.iter().all(|q| q.distance(foot) >= LAMP_MIN_SEP_M)
                    {
                        spawn_one_lamp(&mut commands, foot, &assets);
                        placed.push(foot);
                        next_budget -= 1;
                        if next_budget == 0 {
                            return;
                        }
                    }
                    next_at += LAMP_SPACING_M;
                }
                t += 2.0;
            }
            dist += seg;
        }
    }
}

fn local_road_features(
    map: &MapDataResource,
    stream: Option<&ProcStream>,
    origin: Vec2,
) -> Vec<crate::osm::parse::MapFeature> {
    if let Some(stream) = stream {
        let cs = chunk_size_m();
        let pcx = (origin.x / cs).floor() as i32;
        let pcz = (origin.y / cs).floor() as i32;
        let mut out = Vec::new();
        for cz in (pcz - LAMP_CHUNK_RADIUS)..=(pcz + LAMP_CHUNK_RADIUS) {
            for cx in (pcx - LAMP_CHUNK_RADIUS)..=(pcx + LAMP_CHUNK_RADIUS) {
                out.extend(
                    generate_chunk(stream.seed, cx, cz)
                        .into_iter()
                        .filter(|f| matches!(f.kind, FeatureKind::Road { .. })),
                );
            }
        }
        out
    } else {
        map.0
            .features
            .iter()
            .filter(|f| matches!(f.kind, FeatureKind::Road { .. }))
            .cloned()
            .collect()
    }
}

fn spawn_one_lamp(commands: &mut Commands, foot: Vec2, assets: &LampAssets) {
    let base = Vec3::new(foot.x, 0.0, foot.y);
    commands
        .spawn((
            Transform::from_translation(base),
            Visibility::default(),
            StreetLamp,
            Name::new("StreetLamp"),
        ))
        .with_children(|parent| {
            parent.spawn((
                Mesh3d(assets.pole.clone()),
                MeshMaterial3d(assets.pole_mat.clone()),
                Transform::from_xyz(0.0, 2.6, 0.0),
            ));
            parent.spawn((
                Mesh3d(assets.bulb.clone()),
                MeshMaterial3d(assets.bulb_mat.clone()),
                Transform::from_xyz(0.0, 5.15, 0.0),
            ));
            parent.spawn((
                PointLight {
                    color: Color::srgb(1.0, 0.82, 0.55),
                    intensity: 0.0,
                    range: 24.0,
                    radius: 0.25,
                    shadows_enabled: false,
                    ..default()
                },
                Transform::from_xyz(0.0, 5.0, 0.0),
                StreetLampLight,
            ));
        });
}

fn sync_night_lights(
    day: Res<DayNight>,
    palette: Res<Palette>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut headlights: Query<&mut SpotLight, With<Headlight>>,
    mut fill: Query<&mut PointLight, (With<CarFillLight>, Without<StreetLampLight>)>,
    mut lamps: Query<&mut PointLight, (With<StreetLampLight>, Without<CarFillLight>)>,
) {
    let night = night_factor(day.time);
    let lamps_on = night > 0.08;

    let emit = LinearRgba::rgb(6.5, 5.0, 2.4) * night.powf(1.15);
    for handle in &palette.building {
        if let Some(mat) = materials.get_mut(handle) {
            mat.emissive = emit;
        }
    }

    let head_i = if lamps_on {
        420_000.0 * night.max(0.45)
    } else {
        0.0
    };
    for mut spot in &mut headlights {
        spot.intensity = head_i;
        spot.color = Color::srgb(1.0, 0.97, 0.90);
    }

    let fill_i = if lamps_on { 18_000.0 * night } else { 0.0 };
    for mut light in &mut fill {
        light.intensity = fill_i;
        light.color = Color::srgb(1.0, 0.9, 0.75);
    }

    let lamp_i = if lamps_on {
        35_000.0 * night.clamp(0.4, 1.0)
    } else {
        0.0
    };
    for mut light in &mut lamps {
        light.intensity = lamp_i;
    }
}

/// 0 = full day, 1 = deep night.
pub fn night_factor(time_of_day: f32) -> f32 {
    let elev =
        (time_of_day.rem_euclid(1.0) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2).sin();
    let t = ((0.25 - elev) / 0.45).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Attach headlights + fill light to the player car entity.
pub fn attach_car_lights(parent: &mut ChildBuilder) {
    let aim = Vec3::new(0.0, -0.08, 1.0).normalize();
    for x in [-0.55, 0.55] {
        parent.spawn((
            SpotLight {
                intensity: 0.0,
                range: 85.0,
                radius: 0.22,
                shadows_enabled: false,
                outer_angle: 0.48,
                inner_angle: 0.22,
                color: Color::srgb(1.0, 0.97, 0.90),
                ..default()
            },
            Transform::from_xyz(x, 0.42, 1.85).looking_to(aim, Vec3::Y),
            Headlight,
            Name::new("Headlight"),
        ));
    }
    parent.spawn((
        PointLight {
            intensity: 0.0,
            range: 16.0,
            shadows_enabled: false,
            color: Color::srgb(1.0, 0.9, 0.75),
            ..default()
        },
        Transform::from_xyz(0.0, 0.6, 0.5),
        CarFillLight,
        Name::new("CarFillLight"),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn night_factor_extremes() {
        assert!(night_factor(0.5) < 0.15, "noon should be day");
        assert!(night_factor(0.0) > 0.7, "midnight should be night");
    }
}
