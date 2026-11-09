# Drive

Native stylized 3D driving game built with [Bevy](https://bevyengine.org/) and [Avian](https://github.com/Jondolf/avian). Worlds come from procedural city generation, GeoJSON, or live [OpenStreetMap](https://www.openstreetmap.org/) data via Overpass.

## Requirements

- Rust 1.85+ (edition 2021)
- GPU with Vulkan / Metal / DX12
- Network access only if you use `--overpass`

## Quick start

```bash
cargo run --release
```

Default mode: infinite procedural city.

```bash
cargo run --release -- --seed 7
cargo run --release -- --size 320
cargo run --release -- assets/maps/sample.geojson
cargo run --release -- --overpass
cargo run --release -- --lat 45.764 --lon 4.8357 --radius 400
```

| Flag | Effect |
|------|--------|
| *(none)* | Infinite procedural map |
| `--seed N` | Deterministic procedural seed |
| `--size N` | Finite procedural window (meters half-extent) |
| `--proc` | Force procedural (ignore GeoJSON path) |
| `path.geojson` | Load local GeoJSON map |
| `--overpass` | Fetch OSM around default Lyon bbox |
| `--lat` / `--lon` / `--radius` | Overpass query (implies `--overpass`) |

## Controls

| Input | Action |
|-------|--------|
| WASD / arrows | Drive |
| C / Shift+C | Cycle camera (Chase → Hood → Cockpit → Top-down → Cinematic) |
| M / 1–4 | Weather cycle / clear / cloudy / rain / fog |
| N / `[` `]` | Pause day-night / scrub time (Shift = faster) |
| F5–F8 | Morning / noon / dusk / night |
| Minimap scroll / `+` `-` / `0` | Zoom / reset |

## Releases

GitHub Releases ship prebuilt archives (`Linux x86_64`, `macOS aarch64`, `Windows x86_64`). Extract, then run from the extracted folder so `assets/` stays next to the binary:

```bash
./drive                 # Linux / macOS
./drive.exe             # Windows
```

Tag `vX.Y.Z` must match `version` in `Cargo.toml`; CI builds and attaches the archives automatically.

## Development

```bash
cargo test
cargo run --release
```

CI runs `cargo fmt --check`, `cargo clippy`, and `cargo test` on every push and PR.

## Assets

- UI font: [Noto Sans](https://github.com/googlefonts/noto-fonts) (SIL Open Font License 1.1) — see `assets/fonts/README.md`
- Map sample: `assets/maps/sample.geojson`

## License

MIT — see [LICENSE](LICENSE).

Map data from OpenStreetMap is © OpenStreetMap contributors ([ODbL](https://www.openstreetmap.org/copyright)).
