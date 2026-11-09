# AGENTS.md — Drive

Native Bevy 0.15 + avian3d 0.2 arcade driving game. Maps: procedural (default, infinite chunks), GeoJSON, or Overpass OSM.

## Commands

```bash
cargo run --release                    # infinite procedural
cargo run --release -- --seed 7
cargo run --release -- --size 320      # finite window
cargo run --release -- assets/maps/sample.geojson
cargo run --release -- --overpass --lat 45.764 --lon 4.8357 --radius 400

cargo test                             # unit + driveability E2E (headless physics)
cargo clippy -- -D warnings            # required green before done
cargo fmt --check
```

Prefer `cargo test -q <filter>` while iterating. Full suite ~60 tests.

## Layout

| Path | Role |
|------|------|
| `src/main.rs` | CLI map resolve, plugin wiring |
| `src/osm/` | parse / Overpass / **procgen** / **terrain** (biomes + height) |
| `src/world/` | chunk stream, meshes, colliders, `driveability` |
| `src/vehicle/` | player + camera modes + shake |
| `src/traffic/` | NPC paths from roads |
| `src/style/` | Palette + procedural textures |
| `src/hud/`, `minimap/`, `weather/`, `lights/`, `atmosphere/` | UI / env |
| `assets/fonts/` | Noto Sans (UI middots/symbols) |

Human docs: `README.md`. Changelog: `CHANGELOG.md`. Keep this file short; link out for depth.

## Stack invariants

- **Bevy 0.15** ECS; UI text via `Res<UiFonts>` (`src/ui_font.rs`), not default font.
- **avian3d** physics. Player: pitch/roll locked. Nearly-flat terrain → **box collider**, not heightfield (`spawn_drive_collider` in `world/driveability.rs`). Heightfield only when relief span > `FLAT_COLLIDER_SPAN_M`.
- Procgen lattice: `CELL_M` / `CHUNK_CELLS` in `osm/procgen.rs`. Chunk owns **south + west** edges only.
- Road polylines may curve mid-segment; **endpoints stay on lattice** so junctions meet.
- Biomes (`osm/terrain.rs`): Urban (flat) → Suburb → Field / Forest / Hills. Urban `relief_amp = 0`.
- Features elevated with `height_at(seed, x, z)`. `TerrainSeed(Option<u64>)` — `None` for flat OSM/GeoJSON.

## Code style

- Small diffs; reuse existing helpers (`MeshBuf`, `height_at`, `generate_chunk`).
- No drive-by refactors; immutability preferred.
- New Rust modules: state callers + why no existing file serves the purpose (GATEGUARD).
- Conventional commits when asked: `feat|fix|refactor|docs|test|chore: …` — no co-author trailer.

```rust
// Terrain collider pick (do not regress to heightfield-everywhere)
if span <= FLAT_COLLIDER_SPAN_M {
    Collider::cuboid(size_x, THICK, size_z) // flat arcade drive
} else {
    Collider::heightfield(heights, Vec3::new(size_x, 1.0, size_z))
}
```

## Testing

- Unit tests live next to code (`#[cfg(test)]`).
- Driveability / invisible-wall regressions: `src/world/driveability.rs` (`e2e_drive_forward_moves_player`).
- Junction / verge / curve / biome: `osm/procgen.rs`, `osm/terrain.rs`, `world/mesh.rs`.
- After physics/terrain/collider changes: `cargo test e2e_drive` plus relevant unit filters.
- Do not weaken driveability assertions without root-cause evidence.

## Gotchas

1. **Invisible walls**: pitch-locked car + city heightfield edge normals → flat box collider + urban amp 0.
2. **Green holes at junctions**: ribbon inset + junction pad CCW, size ≥ `width * 1.25`.
3. **Grass on asphalt**: verge strips inset clear of junction pads (clear against primary width).
4. **UI tofu**: always `fonts.text` / `text_bold`; middot `·` needs Noto in `assets/fonts/`.
5. **`rescue_under_map`**: lift relative to `height_at`, not raw `y = CAR_CLEARANCE`.
6. Curved roads: match **endpoints** on lattice in tests, not every polyline vertex.

## Boundaries

**Always**

- Run `cargo test` + `clippy -D warnings` after behavior changes.
- Keep procgen deterministic for a given seed.
- Preserve south/west chunk ownership for roads.

**Ask first**

- New Cargo dependencies.
- Bumping Bevy / avian major versions.
- Committing or pushing.
- Large asset downloads / aggressive Overpass usage.

**Never**

- Commit secrets, `.env`, or credentials.
- Force-push `main` / skip hooks (`--no-verify`).
- Reintroduce default Bevy UI font for HUD text.
- Use heightfield colliders on urban / near-flat chunks.

## PR / commit

Only when the user asks. Message: conventional, why over what, no `Co-Authored-By`.
