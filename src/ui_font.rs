//! Custom UI font (Noto Sans) — Bevy default lacks middot / symbols.
//!
//! Called from: `src/main.rs` (UiFontPlugin), then `hud`, `weather`, `vehicle`,
//! `minimap` text setups via `Res<UiFonts>`.
//! No prior font module — TextFont used Bevy default only.
//! User: "mets plutot en place une police custom non ?"

use bevy::prelude::*;

pub struct UiFontPlugin;

impl Plugin for UiFontPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, load_ui_fonts);
    }
}

#[derive(Resource, Clone)]
pub struct UiFonts {
    pub regular: Handle<Font>,
    pub bold: Handle<Font>,
}

impl UiFonts {
    pub fn text(&self, size: f32) -> TextFont {
        TextFont {
            font: self.regular.clone(),
            font_size: size,
            ..default()
        }
    }

    pub fn text_bold(&self, size: f32) -> TextFont {
        TextFont {
            font: self.bold.clone(),
            font_size: size,
            ..default()
        }
    }
}

fn load_ui_fonts(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(UiFonts {
        regular: asset_server.load("fonts/NotoSans-Regular.ttf"),
        bold: asset_server.load("fonts/NotoSans-SemiBold.ttf"),
    });
}
