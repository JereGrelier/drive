//! Standard driving HUD: speedometer, gear, control strip.
//!
//! Called from: `src/main.rs` via `HudPlugin`.
//! No prior HUD module — weather/camera/minimap only cover their own labels.
//! User: "et un hud standard"

use bevy::prelude::*;

use crate::ui_font::UiFonts;
use crate::vehicle::{CarState, PlayerCar};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_hud.after(crate::vehicle::spawn_vehicle))
            .add_systems(Update, update_hud);
    }
}

#[derive(Component)]
struct SpeedText;

#[derive(Component)]
struct GearText;

fn setup_hud(mut commands: Commands, fonts: Res<UiFonts>) {
    // Speed cluster — bottom center.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(28.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-70.0)),
                width: Val::Px(140.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(2.0),
                ..default()
            },
            Name::new("HudSpeedCluster"),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("0"),
                fonts.text_bold(56.0),
                TextColor(Color::srgba(0.95, 0.97, 1.0, 0.95)),
                SpeedText,
                Name::new("HudSpeed"),
            ));
            root.spawn((
                Text::new("km/h"),
                fonts.text(14.0),
                TextColor(Color::srgba(0.75, 0.82, 0.9, 0.85)),
            ));
            root.spawn((
                Text::new("N"),
                fonts.text_bold(22.0),
                TextColor(Color::srgba(0.55, 0.95, 0.65, 0.95)),
                GearText,
                Name::new("HudGear"),
            ));
        });

    // Control strip — bottom left.
    commands.spawn((
        Text::new("WASD steer · C cam · M weather · N day/night · [ ] time"),
        fonts.text(13.0),
        TextColor(Color::srgba(0.85, 0.88, 0.92, 0.72)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(16.0),
            bottom: Val::Px(16.0),
            ..default()
        },
        Name::new("HudControls"),
    ));
}

fn update_hud(
    car: Query<&CarState, With<PlayerCar>>,
    mut speed_q: Query<&mut Text, (With<SpeedText>, Without<GearText>)>,
    mut gear_q: Query<&mut Text, (With<GearText>, Without<SpeedText>)>,
    mut gear_color: Query<&mut TextColor, With<GearText>>,
) {
    let Ok(state) = car.get_single() else {
        return;
    };
    let kmh = (state.speed.abs() * 3.6).round() as i32;
    if let Ok(mut text) = speed_q.get_single_mut() {
        *text = Text::new(format!("{kmh}"));
    }

    let (gear, color) = if state.speed > 0.4 {
        ("D", Color::srgba(0.55, 0.95, 0.65, 0.95))
    } else if state.speed < -0.4 {
        ("R", Color::srgba(0.95, 0.55, 0.45, 0.95))
    } else {
        ("N", Color::srgba(0.85, 0.88, 0.7, 0.9))
    };
    if let Ok(mut text) = gear_q.get_single_mut() {
        *text = Text::new(gear);
    }
    if let Ok(mut c) = gear_color.get_single_mut() {
        *c = TextColor(color);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ms_to_kmh() {
        let kmh = (28.0_f32 * 3.6).round() as i32;
        assert_eq!(kmh, 101);
    }
}
