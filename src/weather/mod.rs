//! Weather + day/night cycle (shared atmosphere).

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;

use crate::style::Palette;
use crate::vehicle::{ChaseCam, PlayerCar};
use crate::world::{MapCenter, SunLight};

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WeatherState>()
            .init_resource::<DayNight>()
            .add_systems(
                Startup,
                setup_weather_ui.after(crate::vehicle::spawn_vehicle),
            )
            .add_systems(
                Update,
                (
                    weather_input,
                    day_night_input,
                    apply_atmosphere,
                    update_weather_label,
                    rain_system,
                )
                    .chain(),
            );
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WeatherKind {
    #[default]
    Clear,
    Cloudy,
    Rain,
    Fog,
}

impl WeatherKind {
    const ALL: [WeatherKind; 4] = [
        WeatherKind::Clear,
        WeatherKind::Cloudy,
        WeatherKind::Rain,
        WeatherKind::Fog,
    ];

    fn next(self) -> Self {
        let i = Self::ALL.iter().position(|w| *w == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    fn label(self) -> &'static str {
        match self {
            WeatherKind::Clear => "Clear",
            WeatherKind::Cloudy => "Cloudy",
            WeatherKind::Rain => "Rain",
            WeatherKind::Fog => "Fog",
        }
    }
}

#[derive(Resource)]
pub struct WeatherState {
    pub kind: WeatherKind,
    auto_timer: f32,
}

impl Default for WeatherState {
    fn default() -> Self {
        Self {
            kind: WeatherKind::Clear,
            auto_timer: 120.0,
        }
    }
}

/// 0 = midnight, 0.25 = sunrise, 0.5 = noon, 0.75 = sunset.
#[derive(Resource)]
pub struct DayNight {
    pub time: f32,
    pub paused: bool,
    /// Real seconds for a full 24h loop.
    pub day_length_secs: f32,
}

impl Default for DayNight {
    fn default() -> Self {
        Self {
            time: 0.35, // mid-morning
            paused: false,
            day_length_secs: 240.0,
        }
    }
}

impl DayNight {
    fn clock_label(&self) -> String {
        let hours = self.time.rem_euclid(1.0) * 24.0;
        let h = hours as u32;
        let m = ((hours - h as f32) * 60.0) as u32;
        let phase = day_phase_name(self.time);
        let pause = if self.paused { " · paused" } else { "" };
        format!("{h:02}:{m:02} {phase}{pause}")
    }
}

#[derive(Component)]
struct WeatherLabel;

#[derive(Component)]
struct RainDrop {
    vel: Vec3,
}

const RAIN_COUNT: usize = 280;

fn setup_weather_ui(mut commands: Commands, fonts: Res<crate::ui_font::UiFonts>) {
    commands.spawn((
        Text::new("Clear · --:--"),
        fonts.text(18.0),
        TextColor(Color::srgba(0.95, 0.96, 0.98, 0.9)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(16.0),
            top: Val::Px(16.0),
            ..default()
        },
        WeatherLabel,
        Name::new("WeatherLabel"),
    ));
}

fn weather_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut weather: ResMut<WeatherState>,
) {
    if keys.just_pressed(KeyCode::KeyM) {
        weather.kind = weather.kind.next();
        weather.auto_timer = 120.0;
    }
    if keys.just_pressed(KeyCode::Digit1) {
        weather.kind = WeatherKind::Clear;
        weather.auto_timer = 120.0;
    }
    if keys.just_pressed(KeyCode::Digit2) {
        weather.kind = WeatherKind::Cloudy;
        weather.auto_timer = 120.0;
    }
    if keys.just_pressed(KeyCode::Digit3) {
        weather.kind = WeatherKind::Rain;
        weather.auto_timer = 120.0;
    }
    if keys.just_pressed(KeyCode::Digit4) {
        weather.kind = WeatherKind::Fog;
        weather.auto_timer = 120.0;
    }

    weather.auto_timer -= time.delta_secs();
    if weather.auto_timer <= 0.0 {
        weather.kind = weather.kind.next();
        weather.auto_timer = 120.0;
    }
}

fn day_night_input(time: Res<Time>, keys: Res<ButtonInput<KeyCode>>, mut day: ResMut<DayNight>) {
    if keys.just_pressed(KeyCode::KeyN) {
        day.paused = !day.paused;
    }
    // Scrub: [ ] hold = slow, Shift+[ ] = fast jump
    let scrub = if keys.pressed(KeyCode::BracketRight) {
        1.0
    } else if keys.pressed(KeyCode::BracketLeft) {
        -1.0
    } else {
        0.0
    };
    if scrub != 0.0 {
        let speed = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
            0.08
        } else {
            0.02
        };
        day.time = (day.time + scrub * speed * time.delta_secs() * 60.0).rem_euclid(1.0);
    } else if !day.paused {
        day.time = (day.time + time.delta_secs() / day.day_length_secs).rem_euclid(1.0);
    }

    // Quick presets
    if keys.just_pressed(KeyCode::F5) {
        day.time = 0.35; // morning
    }
    if keys.just_pressed(KeyCode::F6) {
        day.time = 0.5; // noon
    }
    if keys.just_pressed(KeyCode::F7) {
        day.time = 0.78; // dusk
    }
    if keys.just_pressed(KeyCode::F8) {
        day.time = 0.0; // midnight
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_atmosphere(
    weather: Res<WeatherState>,
    day: Res<DayNight>,
    map_center: Option<Res<MapCenter>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<AmbientLight>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<SunLight>>,
    mut fog: Query<&mut DistanceFog, With<ChaseCam>>,
    palette: Res<Palette>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let base = weather_params(weather.kind);
    let dn = day_factors(day.time);
    let params = blend_day_night(base, dn);

    clear.0 = params.clear;
    ambient.color = params.ambient;
    ambient.brightness = params.ambient_brightness;

    let look_at = map_center.map(|c| c.0).unwrap_or(Vec3::ZERO);
    for (mut light, mut tf) in &mut sun {
        light.illuminance = params.sun_illuminance;
        light.color = params.sun_color;
        *tf = sun_transform(day.time, look_at);
    }

    for mut f in &mut fog {
        f.color = params.fog_color;
        f.falloff = FogFalloff::Linear {
            start: params.fog_start,
            end: params.fog_end,
        };
    }

    if let Some(mat) = materials.get_mut(&palette.road) {
        mat.base_color = params.road_tint;
        mat.perceptual_roughness = params.road_roughness;
        mat.reflectance = params.road_reflectance;
    }
}

struct WeatherParams {
    clear: Color,
    ambient: Color,
    ambient_brightness: f32,
    sun_illuminance: f32,
    sun_color: Color,
    fog_color: Color,
    fog_start: f32,
    fog_end: f32,
    road_tint: Color,
    road_roughness: f32,
    road_reflectance: f32,
}

struct DayFactors {
    /// 0 night → 1 full day.
    dayness: f32,
    /// Golden-hour warmth 0..1 near sunrise/sunset.
    warmth: f32,
}

fn day_factors(t: f32) -> DayFactors {
    let t = t.rem_euclid(1.0);
    // elev: −1 midnight, 0 horizon, +1 noon
    let elev = (t * TAU - FRAC_PI_2).sin();
    let dayness = smoothstep(-0.05, 0.35, elev);
    // Peaks near sunrise (~0.25) and sunset (~0.75)
    let rise = 1.0 - ((t - 0.25).abs() * 12.0).min(1.0);
    let set = 1.0 - ((t - 0.75).abs() * 12.0).min(1.0);
    let warmth = rise.max(set) * (1.0 - (dayness - 0.5).abs() * 0.5);
    DayFactors { dayness, warmth }
}

fn day_phase_name(t: f32) -> &'static str {
    let t = t.rem_euclid(1.0);
    match t {
        x if x < 0.20 || x >= 0.90 => "Night",
        x if x < 0.30 => "Dawn",
        x if x < 0.70 => "Day",
        x if x < 0.82 => "Dusk",
        _ => "Night",
    }
}

fn sun_transform(t: f32, look_at: Vec3) -> Transform {
    let t = t.rem_euclid(1.0);
    let elev = (t * TAU - FRAC_PI_2).sin();
    let azim = t * TAU;
    // Keep a slight height at night so the moon-like light still casts soft shadows.
    let height = elev.abs().max(0.08);
    let dist = 140.0;
    let pos = look_at
        + Vec3::new(
            azim.sin() * dist * (0.35 + 0.65 * elev.max(0.0).sqrt()),
            height * dist,
            azim.cos() * dist * (0.35 + 0.65 * elev.max(0.0).sqrt()),
        );
    Transform::from_translation(pos).looking_at(look_at, Vec3::Y)
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let aa = a.to_srgba();
    let bb = b.to_srgba();
    Color::srgba(
        aa.red + (bb.red - aa.red) * t,
        aa.green + (bb.green - aa.green) * t,
        aa.blue + (bb.blue - aa.blue) * t,
        aa.alpha + (bb.alpha - aa.alpha) * t,
    )
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn blend_day_night(base: WeatherParams, dn: DayFactors) -> WeatherParams {
    let night_clear = Color::srgb(0.02, 0.03, 0.08);
    let night_ambient = Color::srgb(0.25, 0.30, 0.55);
    let night_fog = Color::srgba(0.04, 0.05, 0.10, 1.0);
    let moon = Color::srgb(0.55, 0.62, 0.85);
    let warm_sun = Color::srgb(1.0, 0.55, 0.28);

    let clear = lerp_color(night_clear, base.clear, dn.dayness);
    let ambient = lerp_color(night_ambient, base.ambient, dn.dayness);
    let fog_color = lerp_color(night_fog, base.fog_color, dn.dayness);

    let sun_day = base.sun_illuminance * dn.dayness.powf(1.4);
    let sun_night = 280.0 * (1.0 - dn.dayness);
    let sun_illuminance = sun_day + sun_night;

    let sun_color = if dn.dayness > 0.15 {
        lerp_color(warm_sun, base.sun_color, 1.0 - dn.warmth)
    } else {
        moon
    };

    // Keep a usable night floor so streets stay readable with local lights.
    let ambient_brightness =
        base.ambient_brightness * (0.12 + 0.88 * dn.dayness) + 70.0 * (1.0 - dn.dayness);

    // Night roads a bit darker; wetness unchanged from weather.
    let road_tint = lerp_color(
        Color::srgb(0.35, 0.38, 0.45),
        base.road_tint,
        0.35 + 0.65 * dn.dayness,
    );

    let fog_start = base.fog_start * (0.55 + 0.45 * dn.dayness);
    let fog_end = base.fog_end * (0.65 + 0.35 * dn.dayness);

    WeatherParams {
        clear,
        ambient,
        ambient_brightness,
        sun_illuminance,
        sun_color,
        fog_color,
        fog_start,
        fog_end,
        road_tint,
        road_roughness: base.road_roughness,
        road_reflectance: base.road_reflectance,
    }
}

fn weather_params(kind: WeatherKind) -> WeatherParams {
    match kind {
        WeatherKind::Clear => WeatherParams {
            clear: Color::srgb(0.55, 0.70, 0.88),
            ambient: Color::srgb(0.72, 0.78, 0.92),
            ambient_brightness: 180.0,
            sun_illuminance: 22_000.0,
            sun_color: Color::srgb(1.0, 0.97, 0.92),
            fog_color: Color::srgba(0.62, 0.74, 0.88, 1.0),
            fog_start: 90.0,
            fog_end: 320.0,
            road_tint: Color::WHITE,
            road_roughness: 0.88,
            road_reflectance: 0.1,
        },
        WeatherKind::Cloudy => WeatherParams {
            clear: Color::srgb(0.55, 0.60, 0.66),
            ambient: Color::srgb(0.65, 0.68, 0.72),
            ambient_brightness: 140.0,
            sun_illuminance: 9_000.0,
            sun_color: Color::srgb(0.85, 0.88, 0.92),
            fog_color: Color::srgba(0.58, 0.62, 0.66, 1.0),
            fog_start: 60.0,
            fog_end: 220.0,
            road_tint: Color::srgb(0.9, 0.9, 0.92),
            road_roughness: 0.9,
            road_reflectance: 0.08,
        },
        WeatherKind::Rain => WeatherParams {
            clear: Color::srgb(0.38, 0.42, 0.48),
            ambient: Color::srgb(0.55, 0.58, 0.62),
            ambient_brightness: 110.0,
            sun_illuminance: 4_500.0,
            sun_color: Color::srgb(0.7, 0.75, 0.82),
            fog_color: Color::srgba(0.45, 0.50, 0.55, 1.0),
            fog_start: 35.0,
            fog_end: 160.0,
            road_tint: Color::srgb(0.55, 0.58, 0.62),
            road_roughness: 0.35,
            road_reflectance: 0.55,
        },
        WeatherKind::Fog => WeatherParams {
            clear: Color::srgb(0.70, 0.72, 0.74),
            ambient: Color::srgb(0.78, 0.80, 0.82),
            ambient_brightness: 160.0,
            sun_illuminance: 3_000.0,
            sun_color: Color::srgb(0.9, 0.9, 0.92),
            fog_color: Color::srgba(0.78, 0.80, 0.82, 1.0),
            fog_start: 8.0,
            fog_end: 55.0,
            road_tint: Color::srgb(0.85, 0.85, 0.88),
            road_roughness: 0.85,
            road_reflectance: 0.12,
        },
    }
}

fn update_weather_label(
    weather: Res<WeatherState>,
    day: Res<DayNight>,
    mut q: Query<&mut Text, With<WeatherLabel>>,
) {
    let Ok(mut text) = q.get_single_mut() else {
        return;
    };
    let label = format!(
        "{} · {}  (M wx · N pause · [ ] time)",
        weather.kind.label(),
        day.clock_label()
    );
    if text.0 != label {
        text.0 = label;
    }
}

#[allow(clippy::too_many_arguments)]
fn rain_system(
    mut commands: Commands,
    weather: Res<WeatherState>,
    time: Res<Time>,
    car: Query<&Transform, With<PlayerCar>>,
    mut drops: Query<(Entity, &mut Transform, &mut RainDrop), Without<PlayerCar>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rain_mesh: Local<Option<Handle<Mesh>>>,
    mut rain_mat: Local<Option<Handle<StandardMaterial>>>,
) {
    let raining = weather.kind == WeatherKind::Rain;
    let Ok(car_tf) = car.get_single() else {
        return;
    };
    let origin = car_tf.translation;

    if !raining {
        for (e, _, _) in &drops {
            commands.entity(e).despawn();
        }
        return;
    }

    let mesh = rain_mesh.get_or_insert_with(|| meshes.add(Cuboid::new(0.03, 0.55, 0.03)));
    let mat = rain_mat.get_or_insert_with(|| {
        materials.add(StandardMaterial {
            base_color: Color::srgba(0.75, 0.82, 0.95, 0.55),
            perceptual_roughness: 0.2,
            metallic: 0.0,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        })
    });

    let count = drops.iter().count();
    for i in count..RAIN_COUNT {
        let (x, z) = rain_spawn_xz(origin, i as u32, time.elapsed_secs());
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(x, origin.y + 12.0 + (i % 7) as f32, z),
            RainDrop {
                vel: Vec3::new(1.5, -28.0, 0.8),
            },
            Name::new("RainDrop"),
        ));
    }

    let dt = time.delta_secs();
    for (i, (_entity, mut tf, mut drop)) in drops.iter_mut().enumerate() {
        tf.translation += drop.vel * dt;
        if tf.translation.y < origin.y - 0.5 {
            let (x, z) = rain_spawn_xz(origin, i as u32, time.elapsed_secs() + i as f32);
            tf.translation = Vec3::new(x, origin.y + 14.0, z);
            drop.vel.y = -24.0 - (i % 9) as f32;
        }
        let dx = tf.translation.x - origin.x;
        let dz = tf.translation.z - origin.z;
        if dx * dx + dz * dz > 45.0 * 45.0 {
            let (x, z) = rain_spawn_xz(origin, i as u32 + 17, time.elapsed_secs());
            tf.translation = Vec3::new(x, origin.y + 12.0, z);
        }
    }
}

fn rain_spawn_xz(origin: Vec3, seed: u32, t: f32) -> (f32, f32) {
    let a = (seed as f32 * 12.9898 + t * 3.1).sin() * 43_758.547;
    let b = (seed as f32 * 78.233 + t * 1.7).sin() * 43_758.547;
    let rx = a.fract() * 2.0 - 1.0;
    let rz = b.fract() * 2.0 - 1.0;
    (origin.x + rx * 28.0, origin.z + rz * 28.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noon_is_bright_midnight_is_dark() {
        let noon = day_factors(0.5);
        let midnight = day_factors(0.0);
        assert!(noon.dayness > 0.9);
        assert!(midnight.dayness < 0.1);
        let noon_elev = (0.5_f32 * TAU - FRAC_PI_2).sin();
        let mid_elev = (0.0_f32 * TAU - FRAC_PI_2).sin();
        assert!(noon_elev > mid_elev);
    }

    #[test]
    fn dawn_and_dusk_are_warm() {
        let dawn = day_factors(0.25);
        let noon = day_factors(0.5);
        assert!(dawn.warmth > noon.warmth);
    }

    #[test]
    fn clock_wraps() {
        let mut d = DayNight {
            time: 0.999,
            paused: true,
            day_length_secs: 240.0,
        };
        d.time = (d.time + 0.01).rem_euclid(1.0);
        assert!(d.time < 0.02);
        let label = d.clock_label();
        assert!(label.contains(':'));
    }
}
