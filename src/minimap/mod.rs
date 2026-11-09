//! Top-down orthographic minimap rendered to a UI texture, with zoom + road labels.

use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    render::{
        camera::{RenderTarget, ScalingMode},
        render_asset::RenderAssetUsages,
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
        view::RenderLayers,
    },
};

use crate::osm::{chunk_size_m, generate_chunk, FeatureKind, MapData};
use crate::ui_font::UiFonts;
use crate::vehicle::PlayerCar;
use crate::world::{MapDataResource, ProcStream};

const MAP_SIZE_PX: u32 = 256;
const PANEL_PX: f32 = 220.0;
const ZOOM_MIN: f32 = 40.0;
const ZOOM_MAX: f32 = 450.0;
const ZOOM_DEFAULT: f32 = 140.0;
/// World layer shared by main + minimap cameras.
const LAYER_WORLD: usize = 0;
const MAX_ROAD_LABELS: usize = 16;
const LABEL_MARGIN_PX: f32 = 10.0;
const LABEL_MIN_GAP_PX: f32 = 28.0;
const LABEL_REFRESH_S: f32 = 1.2;
const LABEL_CHUNK_RADIUS: i32 = 2;

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MinimapZoom>()
            .add_systems(Startup, setup_minimap.after(crate::vehicle::spawn_vehicle))
            .add_systems(
                Update,
                (
                    refresh_road_labels,
                    minimap_zoom_input,
                    minimap_reset_button,
                    follow_player_and_apply_zoom,
                    update_minimap_road_labels,
                )
                    .chain(),
            );
    }
}

#[derive(Resource)]
pub struct MinimapZoom {
    /// World meters visible vertically on the minimap (larger = zoomed out).
    pub view_meters: f32,
}

impl Default for MinimapZoom {
    fn default() -> Self {
        Self {
            view_meters: ZOOM_DEFAULT,
        }
    }
}

#[derive(Clone, Debug)]
struct RoadLabel {
    name: String,
    /// World XZ midpoint.
    pos: Vec2,
    length_m: f32,
    /// Higher = prefer on crowded minimap (primary > residential).
    priority: u8,
}

#[derive(Resource, Default)]
struct RoadLabels(Vec<RoadLabel>);

#[derive(Component)]
struct MinimapCamera;

#[derive(Component)]
struct MinimapPanel;

#[derive(Component)]
struct MinimapResetButton;

#[derive(Component)]
struct MinimapLabelRoot;

#[derive(Component)]
struct MinimapRoadLabel;

fn setup_minimap(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    zoom: Res<MinimapZoom>,
    map: Res<MapDataResource>,
    fonts: Res<UiFonts>,
) {
    commands.insert_resource(RoadLabels(collect_road_labels(&map.0)));
    let label_font = fonts.text(11.0);
    let btn_font = fonts.text(13.0);

    let size = Extent3d {
        width: MAP_SIZE_PX,
        height: MAP_SIZE_PX,
        ..default()
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image_handle = images.add(image);

    commands.spawn((
        Camera3d::default(),
        Camera {
            order: -1,
            target: RenderTarget::Image(image_handle.clone()),
            clear_color: ClearColorConfig::Custom(Color::srgb(0.18, 0.28, 0.22)),
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: zoom.view_meters,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0.0, 180.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z),
        RenderLayers::layer(LAYER_WORLD),
        MinimapCamera,
        Name::new("MinimapCamera"),
    ));

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(16.0),
                top: Val::Px(16.0),
                width: Val::Px(PANEL_PX + 8.0),
                height: Val::Px(PANEL_PX + 56.0),
                padding: UiRect::all(Val::Px(4.0)),
                border: UiRect::all(Val::Px(2.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.07, 0.09, 0.75)),
            BorderColor(Color::srgb(0.85, 0.85, 0.88)),
            Name::new("MinimapFrame"),
        ))
        .with_children(|frame| {
            frame
                .spawn((
                    Button,
                    MinimapPanel,
                    Node {
                        width: Val::Px(PANEL_PX),
                        height: Val::Px(PANEL_PX),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    Interaction::default(),
                    Name::new("MinimapPanel"),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        ImageNode::new(image_handle),
                        Node {
                            width: Val::Px(PANEL_PX),
                            height: Val::Px(PANEL_PX),
                            ..default()
                        },
                    ));
                    panel
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(0.0),
                                top: Val::Px(0.0),
                                width: Val::Px(PANEL_PX),
                                height: Val::Px(PANEL_PX),
                                ..default()
                            },
                            MinimapLabelRoot,
                            Name::new("MinimapLabels"),
                        ))
                        .with_children(|root| {
                            for _ in 0..MAX_ROAD_LABELS {
                                root.spawn((
                                    Text::new(""),
                                    label_font.clone(),
                                    TextColor(Color::srgba(1.0, 1.0, 0.92, 0.92)),
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: Val::Px(0.0),
                                        top: Val::Px(0.0),
                                        display: Display::None,
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgba(0.05, 0.06, 0.08, 0.55)),
                                    MinimapRoadLabel,
                                ));
                            }
                        });
                });
            frame
                .spawn((
                    Button,
                    MinimapResetButton,
                    Node {
                        width: Val::Px(PANEL_PX),
                        height: Val::Px(26.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.22, 0.26, 0.32)),
                    BorderColor(Color::srgb(0.55, 0.58, 0.62)),
                    Interaction::default(),
                ))
                .with_children(|btn| {
                    btn.spawn((
                        Text::new("Reset zoom"),
                        btn_font.clone(),
                        TextColor(Color::srgb(0.92, 0.93, 0.95)),
                    ));
                });
        });
}

fn refresh_road_labels(
    time: Res<Time>,
    map: Res<MapDataResource>,
    stream: Option<Res<ProcStream>>,
    player: Query<&Transform, With<PlayerCar>>,
    mut labels: ResMut<RoadLabels>,
    mut cooldown: Local<f32>,
) {
    *cooldown -= time.delta_secs();
    if *cooldown > 0.0 {
        return;
    }
    *cooldown = LABEL_REFRESH_S;

    let origin = player
        .get_single()
        .map(|tf| Vec2::new(tf.translation.x, tf.translation.z))
        .unwrap_or(map.0.spawn);

    let features = if let Some(stream) = stream.as_deref() {
        let cs = chunk_size_m();
        let pcx = (origin.x / cs).floor() as i32;
        let pcz = (origin.y / cs).floor() as i32;
        let mut out = Vec::new();
        for cz in (pcz - LABEL_CHUNK_RADIUS)..=(pcz + LABEL_CHUNK_RADIUS) {
            for cx in (pcx - LABEL_CHUNK_RADIUS)..=(pcx + LABEL_CHUNK_RADIUS) {
                out.extend(generate_chunk(stream.seed, cx, cz));
            }
        }
        out
    } else {
        map.0.features.clone()
    };

    labels.0 = collect_road_labels_from_features(&features);
}

fn collect_road_labels(map: &MapData) -> Vec<RoadLabel> {
    collect_road_labels_from_features(&map.features)
}

fn collect_road_labels_from_features(features: &[crate::osm::parse::MapFeature]) -> Vec<RoadLabel> {
    // Keep longest segment per name *near the same place* (don't collapse distant homonyms).
    let mut best: Vec<RoadLabel> = Vec::new();
    for f in features {
        let FeatureKind::Road { name, highway, .. } = &f.kind else {
            continue;
        };
        if f.points.len() < 2 {
            continue;
        }
        let length_m = polyline_length(&f.points);
        if length_m < 8.0 {
            continue;
        }
        let raw = name
            .clone()
            .unwrap_or_else(|| crate::osm::fallback_road_name(highway, &f.points));
        let pos = polyline_midpoint(&f.points);
        let priority = highway_priority(highway);
        let label = RoadLabel {
            name: display_street_name(&raw),
            pos,
            length_m,
            priority,
        };
        // Same display name only merges when midpoints are nearby (one street, many cells).
        if let Some(existing) = best
            .iter_mut()
            .find(|l| l.name == label.name && l.pos.distance(label.pos) < 55.0)
        {
            if label.length_m > existing.length_m
                || (label.length_m == existing.length_m && label.priority > existing.priority)
            {
                *existing = label;
            }
        } else {
            best.push(label);
        }
    }
    best.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then(b.length_m.total_cmp(&a.length_m))
    });
    best
}

fn highway_priority(highway: &str) -> u8 {
    match highway {
        "motorway" | "trunk" => 5,
        "primary" => 4,
        "secondary" => 3,
        "tertiary" => 2,
        "residential" | "unclassified" | "living_street" => 1,
        _ => 0,
    }
}

fn polyline_length(points: &[Vec2]) -> f32 {
    points.windows(2).map(|w| (w[1] - w[0]).length()).sum()
}

fn polyline_midpoint(points: &[Vec2]) -> Vec2 {
    let total = polyline_length(points);
    if total <= 1e-3 {
        return points[0];
    }
    let half = total * 0.5;
    let mut acc = 0.0;
    for w in points.windows(2) {
        let seg = (w[1] - w[0]).length();
        if acc + seg >= half {
            let t = ((half - acc) / seg.max(1e-6)).clamp(0.0, 1.0);
            return w[0].lerp(w[1], t);
        }
        acc += seg;
    }
    *points.last().unwrap_or(&points[0])
}

fn display_street_name(name: &str) -> String {
    let trimmed = name.trim();
    // Drop procedural uniqueness suffix " (E12)" / " (N3)".
    let base = if let Some(i) = trimmed.rfind(" (") {
        if trimmed.ends_with(')') {
            &trimmed[..i]
        } else {
            trimmed
        }
    } else {
        trimmed
    };
    shorten_label(base)
}

fn shorten_label(name: &str) -> String {
    const MAX: usize = 22;
    let trimmed = name.trim();
    if trimmed.chars().count() <= MAX {
        return trimmed.to_string();
    }
    let mut out = String::new();
    for (i, ch) in trimmed.chars().enumerate() {
        if i >= MAX.saturating_sub(1) {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

fn minimap_zoom_input(
    mut scroll: EventReader<MouseWheel>,
    mut zoom: ResMut<MinimapZoom>,
    panel: Query<&Interaction, With<MinimapPanel>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let hovered = panel
        .iter()
        .any(|i| matches!(i, Interaction::Hovered | Interaction::Pressed));

    for ev in scroll.read() {
        if !hovered {
            continue;
        }
        let lines = match ev.unit {
            MouseScrollUnit::Line => ev.y,
            MouseScrollUnit::Pixel => ev.y / 40.0,
        };
        zoom.view_meters = (zoom.view_meters * (1.0 - lines * 0.12)).clamp(ZOOM_MIN, ZOOM_MAX);
    }

    if keys.just_pressed(KeyCode::Equal)
        || keys.just_pressed(KeyCode::NumpadAdd)
        || keys.just_pressed(KeyCode::BracketRight)
    {
        zoom.view_meters = (zoom.view_meters * 0.85).clamp(ZOOM_MIN, ZOOM_MAX);
    }
    if keys.just_pressed(KeyCode::Minus)
        || keys.just_pressed(KeyCode::NumpadSubtract)
        || keys.just_pressed(KeyCode::BracketLeft)
    {
        zoom.view_meters = (zoom.view_meters * 1.18).clamp(ZOOM_MIN, ZOOM_MAX);
    }
    if keys.just_pressed(KeyCode::Digit0) || keys.just_pressed(KeyCode::Numpad0) {
        zoom.view_meters = ZOOM_DEFAULT;
    }
}

fn minimap_reset_button(
    mut zoom: ResMut<MinimapZoom>,
    interactions: Query<&Interaction, (Changed<Interaction>, With<MinimapResetButton>)>,
    mut colors: Query<&mut BackgroundColor, With<MinimapResetButton>>,
) {
    let Ok(interaction) = interactions.get_single() else {
        return;
    };
    let Ok(mut bg) = colors.get_single_mut() else {
        return;
    };
    match *interaction {
        Interaction::Pressed => {
            zoom.view_meters = ZOOM_DEFAULT;
            *bg = BackgroundColor(Color::srgb(0.35, 0.42, 0.52));
        }
        Interaction::Hovered => {
            *bg = BackgroundColor(Color::srgb(0.30, 0.36, 0.44));
        }
        Interaction::None => {
            *bg = BackgroundColor(Color::srgb(0.22, 0.26, 0.32));
        }
    }
}

fn follow_player_and_apply_zoom(
    zoom: Res<MinimapZoom>,
    car: Query<&Transform, With<PlayerCar>>,
    mut cam_tf: Query<&mut Transform, (With<MinimapCamera>, Without<PlayerCar>)>,
    mut cam_proj: Query<&mut Projection, With<MinimapCamera>>,
) {
    let Ok(car_tf) = car.get_single() else {
        return;
    };
    let Ok(mut tf) = cam_tf.get_single_mut() else {
        return;
    };
    let Ok(mut projection) = cam_proj.get_single_mut() else {
        return;
    };

    let p = car_tf.translation;
    *tf = Transform::from_xyz(p.x, 180.0, p.z).looking_at(Vec3::new(p.x, 0.0, p.z), Vec3::NEG_Z);

    if let Projection::Orthographic(ortho) = projection.as_mut() {
        ortho.scaling_mode = ScalingMode::FixedVertical {
            viewport_height: zoom.view_meters,
        };
    }
}

fn update_minimap_road_labels(
    labels: Res<RoadLabels>,
    zoom: Res<MinimapZoom>,
    cam: Query<&Transform, With<MinimapCamera>>,
    mut nodes: Query<(&mut Text, &mut Node), With<MinimapRoadLabel>>,
) {
    let Ok(cam_tf) = cam.get_single() else {
        return;
    };
    let cam_xz = Vec2::new(cam_tf.translation.x, cam_tf.translation.z);
    let view = zoom.view_meters.max(1.0);
    let half = PANEL_PX * 0.5;

    // Candidate screen placements (already sorted by priority/length).
    let mut placed: Vec<(f32, f32)> = Vec::with_capacity(MAX_ROAD_LABELS);
    let mut chosen: Vec<(&RoadLabel, f32, f32)> = Vec::with_capacity(MAX_ROAD_LABELS);

    for label in &labels.0 {
        if chosen.len() >= MAX_ROAD_LABELS {
            break;
        }
        // Hide tiny streets when zoomed out.
        if label.length_m < view * 0.06 && label.priority < 3 {
            continue;
        }
        let dx = label.pos.x - cam_xz.x;
        let dz = label.pos.y - cam_xz.y;
        let sx = half + dx / view * PANEL_PX;
        // Camera up is −Z → +world Z goes toward bottom of the panel.
        let sy = half + dz / view * PANEL_PX;
        if !(LABEL_MARGIN_PX..PANEL_PX - LABEL_MARGIN_PX).contains(&sx)
            || !(LABEL_MARGIN_PX..PANEL_PX - LABEL_MARGIN_PX).contains(&sy)
        {
            continue;
        }
        let overlaps = placed.iter().any(|(px, py)| {
            let ddx = sx - px;
            let ddy = sy - py;
            ddx * ddx + ddy * ddy < LABEL_MIN_GAP_PX * LABEL_MIN_GAP_PX
        });
        if overlaps {
            continue;
        }
        placed.push((sx, sy));
        chosen.push((label, sx, sy));
    }

    let mut chosen_iter = chosen.into_iter();
    for (mut text, mut node) in &mut nodes {
        if let Some((label, sx, sy)) = chosen_iter.next() {
            if text.0 != label.name {
                text.0 = label.name.clone();
            }
            node.display = Display::Flex;
            node.left = Val::Px(sx - 4.0);
            node.top = Val::Px(sy - 7.0);
            node.padding = UiRect::axes(Val::Px(3.0), Val::Px(1.0));
        } else {
            node.display = Display::None;
            if !text.0.is_empty() {
                text.0.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_all_roads_with_names() {
        let map = crate::osm::parse::parse_geojson(
            r#"{
          "type":"FeatureCollection",
          "features":[
            {"type":"Feature","properties":{"highway":"primary","name":"Cours A"},
             "geometry":{"type":"LineString","coordinates":[[4.83,45.76],[4.831,45.76],[4.832,45.76]]}},
            {"type":"Feature","properties":{"highway":"residential"},
             "geometry":{"type":"LineString","coordinates":[[4.83,45.761],[4.831,45.761]]}}
          ]
        }"#,
        )
        .unwrap();
        let labels = collect_road_labels(&map);
        assert_eq!(labels.len(), 2, "unnamed OSM roads get fallback labels");
        assert!(labels.iter().any(|l| l.name == "Cours A"));
        assert!(labels.iter().any(|l| l.priority >= 4));
    }

    #[test]
    fn midpoint_is_halfway() {
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)];
        let mid = polyline_midpoint(&pts);
        assert!((mid.x - 5.0).abs() < 1e-3);
    }
}
