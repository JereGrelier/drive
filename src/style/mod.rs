//! Semi-realistic materials: procedural facade/road/ground textures + PBR.

use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

#[derive(Resource)]
pub struct Palette {
    pub ground: Handle<StandardMaterial>,
    pub road: Handle<StandardMaterial>,
    pub building: Vec<Handle<StandardMaterial>>,
    pub roof: Vec<Handle<StandardMaterial>>,
    pub grass: Handle<StandardMaterial>,
    /// Double-sided 3D blade tufts.
    pub grass_blades: Handle<StandardMaterial>,
    pub tree_trunk: Handle<StandardMaterial>,
    pub tree_foliage: Handle<StandardMaterial>,
    pub sign_pole: Handle<StandardMaterial>,
    /// Stop, Yield, Speed30, Speed50, Info — order matches `SignKind`.
    pub sign_faces: Vec<Handle<StandardMaterial>>,
    /// Metal / wood / accent — order matches `DecorMat`.
    pub decor: Vec<Handle<StandardMaterial>>,
    pub car_body: Handle<StandardMaterial>,
    /// Extra body paints for NPC traffic.
    pub traffic_bodies: Vec<Handle<StandardMaterial>>,
    pub car_cabin: Handle<StandardMaterial>,
    pub car_glass: Handle<StandardMaterial>,
    pub car_wheel: Handle<StandardMaterial>,
}

pub struct StylePlugin;

impl Plugin for StylePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_palette);
    }
}

pub fn setup_palette(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let road_tex = images.add(make_road_texture(256, 256));
    let ground_tex = images.add(make_ground_texture(256, 256));
    let grass_tex = images.add(make_grass_texture(256, 256));
    let bark_tex = images.add(make_bark_texture(128, 256));
    let leaf_tex = images.add(make_leaf_texture(256, 256));
    let stop_tex = images.add(make_stop_sign_texture(128));
    let yield_tex = images.add(make_yield_sign_texture(128));
    let speed30_tex = images.add(make_speed_sign_texture(128, 30));
    let speed50_tex = images.add(make_speed_sign_texture(128, 50));
    let info_tex = images.add(make_info_sign_texture(192, 64));

    // High-contrast roofs so top-down (minimap) still reads as tiled, not flat paint.
    let roof_styles = [
        RoofStyle::Clay {
            tile: [168, 72, 48],
            grout: [72, 42, 32],
        },
        RoofStyle::Slate {
            tile: [72, 82, 98],
            grout: [36, 40, 48],
        },
        RoofStyle::Zinc {
            panel: [150, 156, 162],
            seam: [88, 92, 98],
        },
        RoofStyle::Tar {
            base: [52, 50, 48],
            grit: [90, 86, 80],
        },
        RoofStyle::Clay {
            tile: [128, 58, 42],
            grout: [58, 34, 28],
        },
        RoofStyle::Slate {
            tile: [58, 62, 68],
            grout: [28, 30, 34],
        },
    ];
    let roofs: Vec<_> = roof_styles
        .into_iter()
        .map(|style| {
            let tex = images.add(make_roof_texture(256, 256, style));
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: Some(tex),
                perceptual_roughness: match style {
                    RoofStyle::Zinc { .. } => 0.35,
                    RoofStyle::Tar { .. } => 0.95,
                    _ => 0.88,
                },
                metallic: match style {
                    RoofStyle::Zinc { .. } => 0.55,
                    _ => 0.04,
                },
                reflectance: match style {
                    RoofStyle::Zinc { .. } => 0.55,
                    _ => 0.12,
                },
                ..default()
            })
        })
        .collect();

    // Distinct facade styles (tileable floor × bay) so the city doesn't look stamped.
    let facade_styles = [
        FacadeStyle::Plaster {
            wall: [196, 188, 176],
            frame: [92, 86, 80],
        },
        FacadeStyle::Brick {
            brick: [148, 78, 62],
            mortar: [188, 178, 168],
            frame: [70, 60, 55],
        },
        FacadeStyle::Plaster {
            wall: [168, 178, 186],
            frame: [64, 72, 82],
        },
        FacadeStyle::Modern {
            panel: [120, 128, 136],
            frame: [40, 44, 48],
        },
        FacadeStyle::Plaster {
            wall: [186, 172, 158],
            frame: [88, 78, 70],
        },
        FacadeStyle::Brick {
            brick: [112, 96, 88],
            mortar: [170, 162, 152],
            frame: [55, 50, 48],
        },
    ];

    let buildings = facade_styles
        .into_iter()
        .map(|style| {
            let (albedo, emissive_map) = make_facade_textures(512, 512, style);
            let tex = images.add(albedo);
            let emit_tex = images.add(emissive_map);
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: Some(tex),
                emissive: LinearRgba::BLACK, // scaled at night by LightsPlugin
                emissive_texture: Some(emit_tex),
                perceptual_roughness: match style {
                    FacadeStyle::Modern { .. } => 0.45,
                    FacadeStyle::Brick { .. } => 0.82,
                    FacadeStyle::Plaster { .. } => 0.7,
                },
                metallic: match style {
                    FacadeStyle::Modern { .. } => 0.18,
                    _ => 0.02,
                },
                reflectance: match style {
                    FacadeStyle::Modern { .. } => 0.45,
                    _ => 0.18,
                },
                ..default()
            })
        })
        .collect();

    commands.insert_resource(Palette {
        ground: materials.add(StandardMaterial {
            base_color: Color::srgb(0.45, 0.55, 0.38),
            base_color_texture: Some(ground_tex),
            perceptual_roughness: 0.95,
            metallic: 0.0,
            ..default()
        }),
        road: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(road_tex),
            perceptual_roughness: 0.88,
            metallic: 0.0,
            ..default()
        }),
        building: buildings,
        roof: roofs,
        grass: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(grass_tex.clone()),
            perceptual_roughness: 0.95,
            metallic: 0.0,
            ..default()
        }),
        grass_blades: materials.add(StandardMaterial {
            base_color: Color::srgb(0.35, 0.62, 0.28),
            base_color_texture: Some(grass_tex),
            perceptual_roughness: 0.92,
            metallic: 0.0,
            reflectance: 0.08,
            cull_mode: None,
            ..default()
        }),
        tree_trunk: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(bark_tex),
            perceptual_roughness: 0.92,
            metallic: 0.0,
            ..default()
        }),
        tree_foliage: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(leaf_tex),
            perceptual_roughness: 0.88,
            metallic: 0.0,
            reflectance: 0.12,
            ..default()
        }),
        sign_pole: materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.55, 0.58),
            perceptual_roughness: 0.45,
            metallic: 0.75,
            reflectance: 0.4,
            ..default()
        }),
        sign_faces: [
            (stop_tex, 0.55),
            (yield_tex, 0.55),
            (speed30_tex, 0.5),
            (speed50_tex, 0.5),
            (info_tex, 0.5),
        ]
        .into_iter()
        .map(|(tex, rough)| {
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: Some(tex),
                perceptual_roughness: rough,
                metallic: 0.05,
                reflectance: 0.35,
                cull_mode: None,
                ..default()
            })
        })
        .collect(),
        decor: [
            // Metal
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.45, 0.46, 0.48),
                perceptual_roughness: 0.4,
                metallic: 0.85,
                reflectance: 0.45,
                ..default()
            }),
            // Wood
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.42, 0.28, 0.16),
                perceptual_roughness: 0.85,
                metallic: 0.0,
                ..default()
            }),
            // Accent (bins / hydrants / post / shelter panel)
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.72, 0.12, 0.14),
                perceptual_roughness: 0.55,
                metallic: 0.15,
                ..default()
            }),
        ]
        .into_iter()
        .collect(),
        car_body: materials.add(StandardMaterial {
            base_color: Color::srgb(0.75, 0.08, 0.08),
            perceptual_roughness: 0.28,
            metallic: 0.55,
            reflectance: 0.5,
            ..default()
        }),
        traffic_bodies: [
            [0.15, 0.35, 0.75],
            [0.92, 0.88, 0.82],
            [0.12, 0.12, 0.14],
            [0.18, 0.55, 0.28],
            [0.85, 0.55, 0.12],
            [0.55, 0.15, 0.55],
        ]
        .into_iter()
        .map(|rgb| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(rgb[0], rgb[1], rgb[2]),
                perceptual_roughness: 0.32,
                metallic: 0.5,
                reflectance: 0.45,
                ..default()
            })
        })
        .collect(),
        car_cabin: materials.add(StandardMaterial {
            base_color: Color::srgb(0.12, 0.12, 0.14),
            perceptual_roughness: 0.45,
            metallic: 0.2,
            ..default()
        }),
        car_glass: materials.add(StandardMaterial {
            base_color: Color::srgba(0.45, 0.55, 0.65, 0.55),
            perceptual_roughness: 0.05,
            metallic: 0.1,
            alpha_mode: AlphaMode::Blend,
            reflectance: 0.8,
            ..default()
        }),
        car_wheel: materials.add(StandardMaterial {
            base_color: Color::srgb(0.08, 0.08, 0.08),
            perceptual_roughness: 0.85,
            metallic: 0.1,
            ..default()
        }),
    });
}

#[derive(Clone, Copy)]
enum FacadeStyle {
    Plaster {
        wall: [u8; 3],
        frame: [u8; 3],
    },
    Brick {
        brick: [u8; 3],
        mortar: [u8; 3],
        frame: [u8; 3],
    },
    Modern {
        panel: [u8; 3],
        frame: [u8; 3],
    },
}

fn make_image(w: u32, h: u32, pixels: Vec<u8>, repeat: bool) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    if repeat {
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            ..default()
        });
    }
    image
}

fn hash2(x: u32, y: u32) -> u32 {
    let mut n = x
        .wrapping_mul(374761393)
        .wrapping_add(y.wrapping_mul(668265263));
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    n ^ (n >> 16)
}

fn noise_u8(x: u32, y: u32, amp: u8) -> i16 {
    if amp == 0 {
        return 0;
    }
    ((hash2(x, y) % (amp as u32 * 2 + 1)) as i16) - amp as i16
}

fn put(pixels: &mut [u8], w: u32, x: u32, y: u32, rgb: [u8; 3]) {
    let i = ((y * w + x) * 4) as usize;
    pixels[i] = rgb[0];
    pixels[i + 1] = rgb[1];
    pixels[i + 2] = rgb[2];
    pixels[i + 3] = 255;
}

fn shade(rgb: [u8; 3], delta: i16) -> [u8; 3] {
    [
        (rgb[0] as i16 + delta).clamp(0, 255) as u8,
        (rgb[1] as i16 + delta).clamp(0, 255) as u8,
        (rgb[2] as i16 + delta).clamp(0, 255) as u8,
    ]
}

fn lerp_rgb(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t) as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t) as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t) as u8,
    ]
}

/// One seamless tile = 2 window bays × 2 floors. Returns (albedo, emissive).
fn make_facade_textures(w: u32, h: u32, style: FacadeStyle) -> (Image, Image) {
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    let mut emissive = vec![0u8; (w * h * 4) as usize];
    let bays_x = 2u32;
    let floors_y = 2u32;
    let bay_w = w / bays_x;
    let floor_h = h / floors_y;

    for y in 0..h {
        for x in 0..w {
            let bay = x / bay_w;
            let floor = y / floor_h;
            let lx = x % bay_w;
            let ly = y % floor_h;

            let wall_rgb = match style {
                FacadeStyle::Plaster { wall, .. } => {
                    shade(wall, noise_u8(x, y, 8) + ((ly as i16) / 18) - 4)
                }
                FacadeStyle::Brick { brick, mortar, .. } => {
                    let brick_w = 18u32;
                    let brick_h = 10u32;
                    let row = ly / brick_h;
                    let offset = if row.is_multiple_of(2) {
                        0
                    } else {
                        brick_w / 2
                    };
                    let bx = (lx + offset) % brick_w;
                    let by = ly % brick_h;
                    if bx == 0 || by == 0 {
                        mortar
                    } else {
                        shade(brick, noise_u8(x + bay * 17, y + floor * 9, 14))
                    }
                }
                FacadeStyle::Modern { panel, .. } => {
                    let seam = lx < 2 || ly < 2;
                    if seam {
                        shade(panel, -28)
                    } else {
                        shade(panel, noise_u8(x, y, 6))
                    }
                }
            };

            let frame = match style {
                FacadeStyle::Plaster { frame, .. }
                | FacadeStyle::Brick { frame, .. }
                | FacadeStyle::Modern { frame, .. } => frame,
            };

            let margin_x = bay_w / 6;
            let margin_top = floor_h / 5;
            let margin_bot = floor_h / 4;
            let in_window = lx > margin_x
                && lx + margin_x < bay_w
                && ly > margin_top
                && ly + margin_bot < floor_h;

            let mut emit_rgb = [0u8, 0, 0];
            let rgb = if in_window {
                let wx = lx - margin_x;
                let wy = ly - margin_top;
                let ww = bay_w - 2 * margin_x;
                let wh = floor_h - margin_top - margin_bot;
                let frame_t = 3u32;

                if wx < frame_t
                    || wy < frame_t
                    || wx + frame_t >= ww
                    || wy + frame_t >= wh
                    || (wx.abs_diff(ww / 2) < 2)
                    || (wy.abs_diff(wh / 2) < 2 && matches!(style, FacadeStyle::Plaster { .. }))
                {
                    shade(frame, noise_u8(x, y, 4))
                } else {
                    let lit = !hash2(bay * 31 + floor * 17, 7).is_multiple_of(5);
                    let u = wx as f32 / ww as f32;
                    let v = wy as f32 / wh as f32;
                    let glass_dark = [28, 38, 52];
                    let glass_lit = [210, 195, 140];
                    let sky_reflect = [90, 130, 170];
                    let base = if lit { glass_lit } else { glass_dark };
                    let mut g = lerp_rgb(base, sky_reflect, 0.15 + 0.35 * (1.0 - v));
                    if (u - 0.28).abs() < 0.08 {
                        g = shade(g, 35);
                    }
                    if lit {
                        // Warm window glow mask (multiplied by material.emissive at night).
                        emit_rgb = [255, 210, 120];
                    }
                    shade(g, noise_u8(x, y, 5))
                }
            } else if floor == 0 && ly + 8 > floor_h {
                shade(wall_rgb, -18)
            } else {
                wall_rgb
            };

            put(&mut pixels, w, x, y, rgb);
            put(&mut emissive, w, x, y, emit_rgb);
        }
    }

    for y in 0..h {
        let ly = y % floor_h;
        if ly < 3 {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                pixels[i] = pixels[i].saturating_add(18);
                pixels[i + 1] = pixels[i + 1].saturating_add(16);
                pixels[i + 2] = pixels[i + 2].saturating_add(14);
            }
        }
    }

    (
        make_image(w, h, pixels, true),
        make_image(w, h, emissive, true),
    )
}

fn make_road_texture(w: u32, h: u32) -> Image {
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    let mid = w / 2;
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let noise = ((x.wrapping_mul(17) ^ y.wrapping_mul(31)) % 28) as u8;
            let mut v = 42u8.saturating_add(noise);
            if x < 6 || x + 6 >= w {
                v = 210;
            }
            if x.abs_diff(mid) <= 3 && (y % 32) < 18 {
                v = 235;
            }
            pixels[i] = v;
            pixels[i + 1] = v;
            pixels[i + 2] = v.saturating_sub(6);
            pixels[i + 3] = 255;
        }
    }
    make_image(w, h, pixels, true)
}

fn make_ground_texture(w: u32, h: u32) -> Image {
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let n = ((x.wrapping_mul(13) ^ y.wrapping_mul(29)) % 40) as u8;
            pixels[i] = 70 + n / 2;
            pixels[i + 1] = 95 + n;
            pixels[i + 2] = 55 + n / 3;
            pixels[i + 3] = 255;
        }
    }
    make_image(w, h, pixels, true)
}

fn make_grass_texture(w: u32, h: u32) -> Image {
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let n = ((x.wrapping_mul(19) ^ y.wrapping_mul(37)) % 50) as u8;
            let blade = ((x.wrapping_mul(7) ^ y.wrapping_mul(11)) % 17) < 3;
            let rgb = if blade {
                [48u8.saturating_add(n / 2), 120u8.saturating_add(n), 42u8]
            } else {
                [
                    55u8.saturating_add(n / 3),
                    100u8.saturating_add(n / 2),
                    48u8,
                ]
            };
            put(&mut pixels, w, x, y, rgb);
        }
    }
    make_image(w, h, pixels, true)
}

fn make_bark_texture(w: u32, h: u32) -> Image {
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let ridge = ((x.wrapping_mul(3) + noise_u8(x, y, 20) as u32) % 11) < 3;
            let n = noise_u8(x, y.wrapping_mul(2), 28);
            let base = if ridge {
                [62i16, 42, 28]
            } else {
                [88i16, 58, 36]
            };
            put(
                &mut pixels,
                w,
                x,
                y,
                [
                    shade_chan(base[0], n),
                    shade_chan(base[1], n - 4),
                    shade_chan(base[2], n - 6),
                ],
            );
        }
    }
    make_image(w, h, pixels, true)
}

fn make_leaf_texture(w: u32, h: u32) -> Image {
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let n = noise_u8(x, y, 36);
            let mottled = ((x.wrapping_mul(13) ^ y.wrapping_mul(29)) % 23) < 6;
            let deep = ((x + y * 3) % 17) < 5;
            let rgb = if deep {
                [28i16, 72, 30]
            } else if mottled {
                [48i16, 110, 42]
            } else {
                [38i16, 92, 36]
            };
            put(
                &mut pixels,
                w,
                x,
                y,
                [
                    shade_chan(rgb[0], n / 2),
                    shade_chan(rgb[1], n),
                    shade_chan(rgb[2], n / 3),
                ],
            );
        }
    }
    make_image(w, h, pixels, true)
}

fn shade_chan(v: i16, d: i16) -> u8 {
    (v + d).clamp(0, 255) as u8
}

fn make_stop_sign_texture(size: u32) -> Image {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let cx = (size / 2) as i32;
    let cy = (size / 2) as i32;
    let r = (size / 2 - 2) as i32;
    for y in 0..size {
        for x in 0..size {
            let dx = x as i32 - cx;
            let dy = y as i32 - cy;
            // Approximate octagon via max-norm mix.
            let d = dx
                .abs()
                .max(dy.abs())
                .max(((dx.abs() + dy.abs()) as f32 * 0.707) as i32);
            let rgb = if d > r {
                [0, 0, 0]
            } else if d > r - 6 {
                [240, 240, 240]
            } else {
                [200, 32, 32]
            };
            put(&mut pixels, size, x, y, rgb);
        }
    }
    // Crude "STOP" bar across the middle.
    let mid = size / 2;
    for y in (mid - 6)..(mid + 6) {
        for x in (size / 5)..(size * 4 / 5) {
            put(&mut pixels, size, x, y, [250, 250, 250]);
        }
    }
    make_image(size, size, pixels, false)
}

fn make_yield_sign_texture(size: u32) -> Image {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let u = x as f32 / size as f32;
            let v = y as f32 / size as f32;
            // Inverted triangle in UV (point down at v=1).
            let half = v * 0.5 + 0.02;
            let inside = v > 0.08 && v < 0.95 && (u - 0.5).abs() < half;
            let border = inside && (v < 0.16 || v > 0.88 || (u - 0.5).abs() > half - 0.08);
            let rgb = if !inside {
                [0, 0, 0]
            } else if border {
                [220, 40, 40]
            } else {
                [245, 245, 245]
            };
            put(&mut pixels, size, x, y, rgb);
        }
    }
    make_image(size, size, pixels, false)
}

fn make_speed_sign_texture(size: u32, speed: u32) -> Image {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let cx = size as f32 * 0.5;
    let cy = size as f32 * 0.5;
    let r = size as f32 * 0.48;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            let rgb = if d > r {
                [0, 0, 0]
            } else if d > r - 8.0 {
                [210, 35, 35]
            } else {
                [250, 250, 250]
            };
            put(&mut pixels, size, x, y, rgb);
        }
    }
    // Block-digit approximation (30 / 50).
    let tens = speed / 10;
    blit_digit(&mut pixels, size, size / 5, size / 3, tens);
    blit_digit(&mut pixels, size, size / 2, size / 3, 0);
    make_image(size, size, pixels, false)
}

fn blit_digit(pixels: &mut [u8], w: u32, ox: u32, oy: u32, digit: u32) {
    // 3×5 pixel-art digits scaled ×6.
    let glyphs: [[u8; 15]; 6] = [
        // 0
        [1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1],
        // 1
        [0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 1, 1],
        // 2
        [1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1],
        // 3
        [1, 1, 1, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 1],
        // 4
        [1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1],
        // 5
        [1, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1],
    ];
    let g = glyphs[digit.min(5) as usize];
    let scale = 6u32;
    for gy in 0..5u32 {
        for gx in 0..3u32 {
            if g[(gy * 3 + gx) as usize] == 0 {
                continue;
            }
            for sy in 0..scale {
                for sx in 0..scale {
                    let x = ox + gx * scale + sx;
                    let y = oy + gy * scale + sy;
                    if x < w && y < w {
                        put(pixels, w, x, y, [20, 20, 22]);
                    }
                }
            }
        }
    }
}

fn make_info_sign_texture(w: u32, h: u32) -> Image {
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let border = x < 3 || y < 3 || x + 3 >= w || y + 3 >= h;
            let rgb = if border {
                [240, 240, 245]
            } else {
                [35, 75, 170]
            };
            put(&mut pixels, w, x, y, rgb);
        }
    }
    // Fake street label bars.
    for y in (h / 3)..(h / 3 + 4) {
        for x in (w / 8)..(w * 7 / 8) {
            put(&mut pixels, w, x, y, [230, 230, 235]);
        }
    }
    for y in (h * 3 / 5)..(h * 3 / 5 + 3) {
        for x in (w / 6)..(w * 5 / 6) {
            put(&mut pixels, w, x, y, [200, 210, 230]);
        }
    }
    make_image(w, h, pixels, false)
}

#[derive(Clone, Copy)]
enum RoofStyle {
    Clay { tile: [u8; 3], grout: [u8; 3] },
    Slate { tile: [u8; 3], grout: [u8; 3] },
    Zinc { panel: [u8; 3], seam: [u8; 3] },
    Tar { base: [u8; 3], grit: [u8; 3] },
}

fn make_roof_texture(w: u32, h: u32, style: RoofStyle) -> Image {
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    match style {
        RoofStyle::Clay { tile, grout } | RoofStyle::Slate { tile, grout } => {
            // Large tiles: readable from minimap (~8×4 cells across 256).
            let tile_w = 32u32;
            let tile_h = 18u32;
            for y in 0..h {
                for x in 0..w {
                    let row = y / tile_h;
                    let offset = if row.is_multiple_of(2) { 0 } else { tile_w / 2 };
                    let tx = (x + offset) % tile_w;
                    let ty = y % tile_h;
                    let on_grout = tx < 2 || ty < 2;
                    let n = noise_u8(x, y, 18);
                    let mut rgb = if on_grout { grout } else { shade(tile, n) };
                    if ty == 2 && !on_grout {
                        rgb = shade(rgb, 28);
                    }
                    if tx == tile_w - 1 {
                        rgb = shade(rgb, -22);
                    }
                    put(&mut pixels, w, x, y, rgb);
                }
            }
        }
        RoofStyle::Zinc { panel, seam } => {
            let seam_every = 28u32;
            for y in 0..h {
                for x in 0..w {
                    let on_seam = x % seam_every < 3;
                    let n = noise_u8(x, y, 12);
                    let mut rgb = if on_seam { seam } else { shade(panel, n) };
                    // Standing-seam highlight
                    if x % seam_every == 3 {
                        rgb = shade(rgb, 30);
                    }
                    put(&mut pixels, w, x, y, rgb);
                }
            }
        }
        RoofStyle::Tar { base, grit } => {
            for y in 0..h {
                for x in 0..w {
                    let n = noise_u8(x, y, 28);
                    let speck = hash2(x, y).is_multiple_of(7);
                    let rgb = if speck {
                        shade(grit, n / 2)
                    } else {
                        shade(base, n)
                    };
                    put(&mut pixels, w, x, y, rgb);
                }
            }
        }
    }
    make_image(w, h, pixels, true)
}
