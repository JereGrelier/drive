//! Procedural skybox + volumetric cloud / mist FogVolumes (Bevy volumetric fog).
//!
//! Callers: `src/main.rs` registers `AtmospherePlugin` next to WeatherPlugin.
//! No existing sky/volume module — only ClearColor + DistanceFog today.
//! Schemas: camera `Skybox` + `VolumetricFog`, sun `VolumetricLight`,
//! `CloudVolume`/`MistVolume` (`FogVolume`), modulated by `WeatherState`/`DayNight`.
//! User instruction: "et une skybox avec des nuages volumétriques (pareil pour la brume si possible)"

use bevy::core_pipeline::Skybox;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{FogVolume, VolumetricFog, VolumetricLight};
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};

use crate::vehicle::ChaseCam;
use crate::weather::{DayNight, WeatherKind, WeatherState};
use crate::world::{MapCenter, SunLight};

pub struct AtmospherePlugin;

impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            setup_atmosphere
                .after(crate::vehicle::spawn_vehicle)
                .after(crate::world::spawn_world),
        )
        .add_systems(
            Update,
            (scroll_volumes, follow_volumes, apply_atmosphere_volumes).chain(),
        );
    }
}

/// Single FogVolume — Bevy 0.15 panics with 2+ volumes (`BufferTooSmall` in
/// `prepare_volumetric_fog_uniforms`, issue #17590). Mist vs clouds is simulated
/// by shifting/scaling this one slab + density from weather.
#[derive(Component)]
struct AtmosphereVolume;

const VOLUME_CENTER_Y: f32 = 48.0;
const VOLUME_HEIGHT: f32 = 110.0;
const VOLUME_XZ: f32 = 420.0;

fn setup_atmosphere(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    map_center: Option<Res<MapCenter>>,
    mut cam: Query<(Entity, &mut Camera), With<ChaseCam>>,
    sun: Query<Entity, With<SunLight>>,
) {
    let sky_handle = images.add(make_sky_cubemap(192));
    let noise = images.add(make_fbm_volume(48));

    let center = map_center.map(|c| c.0).unwrap_or(Vec3::ZERO);

    if let Ok((entity, mut camera)) = cam.get_single_mut() {
        camera.hdr = true;
        // Official volumetric examples use MSAA off; keeps the fog pass stable.
        commands.entity(entity).insert((
            Skybox {
                image: sky_handle,
                brightness: 1_200.0,
                ..default()
            },
            VolumetricFog {
                ambient_color: Color::srgb(0.65, 0.75, 0.95),
                ambient_intensity: 0.08,
                jitter: 0.35,
                step_count: 48,
            },
            Msaa::Off,
        ));
    }

    for entity in &sun {
        commands.entity(entity).insert(VolumetricLight);
    }

    // One scrolling noise volume covering low mist → high clouds.
    commands.spawn((
        Transform::from_translation(center + Vec3::Y * VOLUME_CENTER_Y).with_scale(Vec3::new(
            VOLUME_XZ,
            VOLUME_HEIGHT,
            VOLUME_XZ,
        )),
        FogVolume {
            density_texture: Some(noise),
            density_factor: 0.04,
            absorption: 0.25,
            scattering: 0.5,
            scattering_asymmetry: 0.55,
            fog_color: Color::srgb(0.9, 0.93, 0.98),
            light_tint: Color::srgb(1.0, 0.97, 0.92),
            light_intensity: 1.1,
            ..default()
        },
        AtmosphereVolume,
        Name::new("AtmosphereVolume"),
    ));
}

fn scroll_volumes(time: Res<Time>, mut volumes: Query<&mut FogVolume, With<AtmosphereVolume>>) {
    let dt = time.delta_secs();
    for mut vol in &mut volumes {
        vol.density_texture_offset += Vec3::new(0.012, 0.002, 0.018) * dt;
    }
}

fn follow_volumes(
    cam: Query<&Transform, With<ChaseCam>>,
    mut volumes: Query<&mut Transform, (With<AtmosphereVolume>, Without<ChaseCam>)>,
) {
    let Ok(cam_tf) = cam.get_single() else {
        return;
    };
    let xz = Vec3::new(cam_tf.translation.x, 0.0, cam_tf.translation.z);
    for mut tf in &mut volumes {
        tf.translation = xz + Vec3::Y * VOLUME_CENTER_Y;
    }
}

#[allow(clippy::type_complexity)]
fn apply_atmosphere_volumes(
    weather: Res<WeatherState>,
    day: Res<DayNight>,
    mut sky: Query<&mut Skybox, With<ChaseCam>>,
    mut vol_fog: Query<&mut VolumetricFog, With<ChaseCam>>,
    mut volumes: Query<
        (&mut FogVolume, &mut Transform),
        (With<AtmosphereVolume>, Without<ChaseCam>),
    >,
) {
    let dayness = day_dayness(day.time);
    // cloud_d / mist_d folded into one density; fog weather pulls the slab down.
    let (density, sky_bright, fog_amb, fog_col, low_bias) = match weather.kind {
        WeatherKind::Clear => (0.032, 1_400.0, 0.06, Color::srgb(0.95, 0.97, 1.0), 0.0),
        WeatherKind::Cloudy => (0.075, 900.0, 0.1, Color::srgb(0.72, 0.75, 0.8), 0.1),
        WeatherKind::Rain => (0.1, 650.0, 0.12, Color::srgb(0.52, 0.56, 0.6), 0.25),
        WeatherKind::Fog => (0.13, 750.0, 0.18, Color::srgb(0.8, 0.82, 0.85), 0.55),
    };

    let sky_bright = sky_bright * (0.08 + 0.92 * dayness);
    let night = 1.0 - dayness;
    let density = density * (0.55 + 0.45 * dayness) + density * 0.35 * night;

    for mut s in &mut sky {
        s.brightness = sky_bright;
    }
    for mut vf in &mut vol_fog {
        vf.ambient_intensity = fog_amb * (0.35 + 0.65 * dayness);
        vf.ambient_color = if dayness > 0.35 {
            Color::srgb(0.65, 0.75, 0.95)
        } else {
            Color::srgb(0.15, 0.18, 0.35)
        };
    }
    for (mut vol, mut tf) in &mut volumes {
        vol.density_factor = density;
        vol.fog_color = fog_col;
        vol.light_intensity = 0.45 + 0.7 * dayness;
        // Fog weather: drop & flatten the volume toward street level.
        let y = VOLUME_CENTER_Y * (1.0 - 0.55 * low_bias);
        let h = VOLUME_HEIGHT * (1.0 - 0.35 * low_bias);
        let xz = Vec3::new(tf.translation.x, 0.0, tf.translation.z);
        tf.translation = xz + Vec3::Y * y;
        tf.scale = Vec3::new(VOLUME_XZ, h, VOLUME_XZ);
    }
}

fn day_dayness(t: f32) -> f32 {
    let elev = (t.rem_euclid(1.0) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2).sin();
    let x = ((elev + 0.05) / 0.4).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Stacked cubemap (+X,-X,+Y,-Y,+Z,-Z), ready for `Skybox`.
fn make_sky_cubemap(face: u32) -> Image {
    let mut pixels = vec![0u8; (face * face * 6 * 4) as usize];
    for (layer, dir_fn) in [
        face_pos_x as fn(f32, f32) -> Vec3,
        face_neg_x as fn(f32, f32) -> Vec3,
        face_pos_y as fn(f32, f32) -> Vec3,
        face_neg_y as fn(f32, f32) -> Vec3,
        face_pos_z as fn(f32, f32) -> Vec3,
        face_neg_z as fn(f32, f32) -> Vec3,
    ]
    .into_iter()
    .enumerate()
    {
        for y in 0..face {
            for x in 0..face {
                let u = (x as f32 + 0.5) / face as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / face as f32 * 2.0 - 1.0;
                let dir = dir_fn(u, v).normalize();
                let rgb = sky_color(dir);
                let i = ((layer as u32 * face * face + y * face + x) * 4) as usize;
                pixels[i] = rgb[0];
                pixels[i + 1] = rgb[1];
                pixels[i + 2] = rgb[2];
                pixels[i + 3] = 255;
            }
        }
    }

    let mut image = Image::new(
        Extent3d {
            width: face,
            height: face * 6,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.reinterpret_stacked_2d_as_array(6);
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

fn sky_color(dir: Vec3) -> [u8; 3] {
    let up = dir.y.clamp(-1.0, 1.0);
    let t = ((up + 0.15) / 1.15).clamp(0.0, 1.0);
    let zenith = [72u8, 140, 220];
    let horizon = [190u8, 210, 235];
    let ground = [110u8, 125, 140];
    let base = if up > 0.0 {
        lerp_u8(horizon, zenith, t.sqrt())
    } else {
        lerp_u8(horizon, ground, (-up).sqrt())
    };
    // Soft painted puffs on the cubemap (complements volumetric FogVolume clouds).
    if up > 0.05 {
        let n = fbm2(dir.x * 2.2 + dir.z * 0.4, dir.z * 2.2 - dir.x * 0.3);
        let clouds = ((n - 0.45).max(0.0) * 2.2).clamp(0.0, 1.0);
        lerp_u8(base, [245, 248, 252], clouds * up.sqrt())
    } else {
        base
    }
}

fn face_pos_x(u: f32, v: f32) -> Vec3 {
    Vec3::new(1.0, -v, -u)
}
fn face_neg_x(u: f32, v: f32) -> Vec3 {
    Vec3::new(-1.0, -v, u)
}
fn face_pos_y(u: f32, v: f32) -> Vec3 {
    Vec3::new(u, 1.0, v)
}
fn face_neg_y(u: f32, v: f32) -> Vec3 {
    Vec3::new(u, -1.0, -v)
}
fn face_pos_z(u: f32, v: f32) -> Vec3 {
    Vec3::new(u, -v, 1.0)
}
fn face_neg_z(u: f32, v: f32) -> Vec3 {
    Vec3::new(-u, -v, -1.0)
}

fn lerp_u8(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t) as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t) as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t) as u8,
    ]
}

/// Repeating 3D FBM density texture (R channel) for FogVolume clouds/mist.
fn make_fbm_volume(size: u32) -> Image {
    let mut data = vec![0u8; (size * size * size * 4) as usize];
    let inv = 1.0 / size as f32;
    for z in 0..size {
        for y in 0..size {
            for x in 0..size {
                let fx = x as f32 * inv;
                let fy = y as f32 * inv;
                let fz = z as f32 * inv;
                let n = fbm3_tile(fx, fy, fz, 4);
                let d = ((n - 0.38).max(0.0) * 1.9).clamp(0.0, 1.0);
                let v = (d * 255.0) as u8;
                let i = ((z * size * size + y * size + x) * 4) as usize;
                data[i] = v;
                data[i + 1] = v;
                data[i + 2] = v;
                data[i + 3] = 255;
            }
        }
    }

    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: size,
        },
        TextureDimension::D3,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

fn hash3(x: i32, y: i32, z: i32) -> f32 {
    let mut n = x
        .wrapping_mul(374_761_393)
        .wrapping_add(y.wrapping_mul(668_265_263))
        .wrapping_add(z.wrapping_mul(1_274_126_177));
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    (n & 0xffff) as f32 / 65535.0
}

fn value_noise3(p: Vec3) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let x0 = i.x as i32;
    let y0 = i.y as i32;
    let z0 = i.z as i32;
    let c000 = hash3(x0, y0, z0);
    let c100 = hash3(x0 + 1, y0, z0);
    let c010 = hash3(x0, y0 + 1, z0);
    let c110 = hash3(x0 + 1, y0 + 1, z0);
    let c001 = hash3(x0, y0, z0 + 1);
    let c101 = hash3(x0 + 1, y0, z0 + 1);
    let c011 = hash3(x0, y0 + 1, z0 + 1);
    let c111 = hash3(x0 + 1, y0 + 1, z0 + 1);
    let x00 = c000 + (c100 - c000) * u.x;
    let x10 = c010 + (c110 - c010) * u.x;
    let x01 = c001 + (c101 - c001) * u.x;
    let x11 = c011 + (c111 - c011) * u.x;
    let y0v = x00 + (x10 - x00) * u.y;
    let y1v = x01 + (x11 - x01) * u.y;
    y0v + (y1v - y0v) * u.z
}

fn fbm3_tile(fx: f32, fy: f32, fz: f32, octaves: u32) -> f32 {
    let mut amp = 0.5;
    let mut freq = 4.0;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for _ in 0..octaves {
        sum += amp * value_noise3(Vec3::new(fx, fy, fz) * freq);
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    (sum / norm).clamp(0.0, 1.0)
}

fn fbm2(x: f32, y: f32) -> f32 {
    let mut amp = 0.5;
    let mut freq = 1.0;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for _ in 0..4 {
        sum += amp * value_noise3(Vec3::new(x * freq, y * freq, 0.17 * freq));
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sky_cubemap_is_six_faces() {
        let img = make_sky_cubemap(16);
        assert_eq!(img.texture_descriptor.array_layer_count(), 6);
        assert_eq!(
            img.texture_view_descriptor
                .as_ref()
                .and_then(|d| d.dimension),
            Some(TextureViewDimension::Cube)
        );
    }

    #[test]
    fn volume_noise_is_3d() {
        let img = make_fbm_volume(8);
        assert_eq!(img.texture_descriptor.dimension, TextureDimension::D3);
        assert_eq!(img.texture_descriptor.size.depth_or_array_layers, 8);
        assert!(!img.data.iter().all(|&b| b == 0));
    }

    #[test]
    fn dayness_peaks_at_noon() {
        assert!(day_dayness(0.5) > day_dayness(0.0));
        assert!(day_dayness(0.5) > 0.85);
    }
}
