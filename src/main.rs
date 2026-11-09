mod atmosphere;
mod hud;
mod lights;
mod minimap;
mod osm;
mod style;
mod traffic;
mod ui_font;
mod vehicle;
mod weather;
mod world;

use atmosphere::AtmospherePlugin;
use avian3d::prelude::*;
use bevy::prelude::*;
use hud::HudPlugin;
use lights::LightsPlugin;
use minimap::MinimapPlugin;
use osm::{
    fetch_map, generate_bootstrap, generate_city, load_map, sanitize_spawn, OverpassQuery,
    ProcgenOptions,
};
use style::StylePlugin;
use traffic::TrafficPlugin;
use ui_font::UiFontPlugin;
use vehicle::VehiclePlugin;
use weather::WeatherPlugin;
use world::{MapDataResource, ProcStream, TerrainSeed, WorldPlugin};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut resolved = resolve_map(&args);
    sanitize_spawn(&mut resolved.map);

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Drive — OSM semi-realistic".into(),
            resolution: (1280.0_f32, 720.0_f32).into(),
            ..default()
        }),
        ..default()
    }))
    .add_plugins(PhysicsPlugins::default())
    .insert_resource(SubstepCount(12))
    .insert_resource(ClearColor(Color::srgb(0.55, 0.70, 0.88)))
    .insert_resource(MapDataResource(resolved.map))
    .insert_resource(TerrainSeed(resolved.terrain_seed));

    if let Some(stream) = resolved.stream {
        app.insert_resource(stream);
    }

    app.add_plugins((
        UiFontPlugin,
        StylePlugin,
        WorldPlugin,
        VehiclePlugin,
        TrafficPlugin,
        LightsPlugin,
        AtmospherePlugin,
        MinimapPlugin,
        WeatherPlugin,
        HudPlugin,
    ))
    .run();
}

struct ResolvedMap {
    map: osm::MapData,
    stream: Option<ProcStream>,
    /// Procedural height/biomes; `None` for flat OSM/GeoJSON.
    terrain_seed: Option<u64>,
}

fn resolve_map(args: &[String]) -> ResolvedMap {
    // Usage:
    //   drive                         → infinite procedural
    //   drive --seed 7
    //   drive --size 320              → finite procedural window
    //   drive path.geojson
    //   drive --overpass [--lat …] [--lon …] [--radius …]
    let mut overpass = args.iter().any(|a| a == "--overpass");
    let mut q = OverpassQuery::default();
    let mut geojson_path: Option<&str> = None;
    let mut proc = ProcgenOptions::default();
    let mut force_proc = args.iter().any(|a| a == "--proc");
    let mut size_set = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--overpass" => {}
            "--proc" => force_proc = true,
            "--seed" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                    proc.seed = v;
                    i += 1;
                }
            }
            "--size" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                    proc.radius_m = v;
                    proc.infinite = false;
                    size_set = true;
                    i += 1;
                }
            }
            "--lat" => {
                overpass = true;
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                    q.lat = v;
                    i += 1;
                }
            }
            "--lon" => {
                overpass = true;
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                    q.lon = v;
                    i += 1;
                }
            }
            "--radius" => {
                overpass = true;
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                    q.radius_m = v;
                    i += 1;
                }
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other if !other.starts_with('-') => geojson_path = Some(other),
            other => eprintln!("Unknown arg: {other}"),
        }
        i += 1;
    }

    if overpass {
        match fetch_map(q) {
            Ok(map) => {
                eprintln!("Loaded Overpass map: {} features", map.features.len());
                return ResolvedMap {
                    map,
                    stream: None,
                    terrain_seed: None,
                };
            }
            Err(err) => {
                eprintln!("Overpass failed ({err}); falling back to procedural city.");
                force_proc = true;
            }
        }
    }

    if let Some(path) = geojson_path {
        return match load_map(path) {
            Ok(map) => ResolvedMap {
                map,
                stream: None,
                terrain_seed: None,
            },
            Err(err) => {
                eprintln!("Failed to load map '{path}': {err}");
                eprintln!("Falling back to procedural city.");
                procedural_resolved(proc, size_set)
            }
        };
    }

    let _ = force_proc;
    procedural_resolved(proc, size_set)
}

fn procedural_resolved(proc: ProcgenOptions, finite: bool) -> ResolvedMap {
    if finite || !proc.infinite {
        let map = generate_city(ProcgenOptions {
            infinite: false,
            ..proc
        });
        eprintln!(
            "Procedural city (finite): {} features (seed={}, size={:.0}m)",
            map.features.len(),
            proc.seed,
            proc.radius_m
        );
        ResolvedMap {
            map,
            stream: None,
            terrain_seed: Some(proc.seed),
        }
    } else {
        // Logical window for traffic / minimap / street lamps; visuals stream forever.
        let map = generate_bootstrap(proc.seed, 2);
        eprintln!(
            "Procedural city (infinite): seed={} · chunks~140m · stream radius 3 · biomes+relief",
            proc.seed
        );
        ResolvedMap {
            map,
            stream: Some(ProcStream {
                seed: proc.seed,
                radius: 3,
            }),
            terrain_seed: Some(proc.seed),
        }
    }
}

fn print_usage() {
    eprintln!(
        "\
Drive — native OSM / procedural driving

  cargo run --release
  cargo run --release -- --seed 7
  cargo run --release -- --size 320
  cargo run --release -- --proc
  cargo run --release -- assets/maps/sample.geojson
  cargo run --release -- --overpass
  cargo run --release -- --lat 45.764 --lon 4.8357 --radius 400

Map: default = infinite procedural · --size N = finite half-extent · GeoJSON · --overpass
Proc: --seed N · --size meters (finite window)

Controls: WASD / arrows · minimap: scroll/+/- zoom, Reset or 0
Camera: C cycle (Shift+C back) · Chase · Hood · Cockpit · Top-down · Cinematic
Sky: procedural skybox + volumetric clouds/mist (follow weather)
Weather: M cycle · 1 clear · 2 cloudy · 3 rain · 4 fog
Day/night: N pause · [ ] scrub (Shift faster) · F5 morning · F6 noon · F7 dusk · F8 night
Night lights: headlights + street lamps + lit windows (auto at dusk)
Traffic: NPC cars follow OSM roads (collide with player)"
    );
}
