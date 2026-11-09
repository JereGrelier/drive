//! Deterministic biomes + height for procedural maps.
//!
//! Called from: `procgen` (chunk fill / road bend), `world` (heightfield + mesh Y),
//! `vehicle` (spawn height). Flat when seed is unused (OSM / GeoJSON path).
//! No prior terrain/biome module — ground was a flat Plane3d only.
//! User: "/goal ... routes pas forcément droites ... biomes hors ville ... relief en 3d"

/// Core stays dense city; beyond edge → countryside biomes.
pub const CITY_CORE_M: f32 = 220.0;
pub const CITY_EDGE_M: f32 = 420.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Biome {
    Urban,
    Suburb,
    Field,
    Forest,
    Hills,
}

impl Biome {
    /// Lateral road bend amplitude (meters) for mid-segment control points.
    pub fn curve_amp(self) -> f32 {
        match self {
            Biome::Urban => 2.5,
            Biome::Suburb => 5.0,
            Biome::Field => 10.0,
            Biome::Forest => 12.0,
            Biome::Hills => 14.0,
        }
    }

    /// Terrain vertical scale. Urban stays flat — pitch-locked car + heightfield
    /// edges caused "invisible walls" on even mild city relief.
    pub fn relief_amp(self) -> f32 {
        match self {
            Biome::Urban => 0.0,
            Biome::Suburb => 1.2,
            Biome::Field => 4.5,
            Biome::Forest => 7.0,
            Biome::Hills => 16.0,
        }
    }
}

/// Biome at world XZ (meters). Distance from origin + low-frequency noise.
pub fn biome_at(seed: u64, x: f32, z: f32) -> Biome {
    let dist = (x * x + z * z).sqrt();
    let n = value_noise(seed ^ 0xB10E_0001, x * 0.0018, z * 0.0018);
    if dist < CITY_CORE_M {
        return Biome::Urban;
    }
    if dist < CITY_EDGE_M {
        // Soft fringe: noise picks suburb vs lingering urban pockets.
        return if n > 0.15 {
            Biome::Suburb
        } else {
            Biome::Urban
        };
    }
    // Countryside ring.
    let t = (n + 1.0) * 0.5; // 0..1
    if t < 0.34 {
        Biome::Field
    } else if t < 0.62 {
        Biome::Forest
    } else {
        Biome::Hills
    }
}

/// Ground height (meters) at XZ. Urban is nearly flat; hills roll harder.
pub fn height_at(seed: u64, x: f32, z: f32) -> f32 {
    let biome = biome_at(seed, x, z);
    let amp = biome.relief_amp();
    let broad = value_noise(seed, x * 0.0025, z * 0.0025);
    let mid = value_noise(seed ^ 0xA11, x * 0.012, z * 0.012) * 0.45;
    let fine = value_noise(seed ^ 0xF1E, x * 0.04, z * 0.04) * 0.12;
    // Flatten toward city core so downtown stays driveable.
    let dist = (x * x + z * z).sqrt();
    let city_flat = (1.0 - (dist / CITY_EDGE_M).clamp(0.0, 1.0)).powi(2);
    let damp = 1.0 - city_flat * 0.85;
    (broad + mid + fine) * 0.5 * amp * damp
}

/// Value noise in ≈[-1, 1] via hashed lattice + smoothstep.
fn value_noise(seed: u64, x: f32, z: f32) -> f32 {
    let x0 = x.floor() as i32;
    let z0 = z.floor() as i32;
    let fx = x - x0 as f32;
    let fz = z - z0 as f32;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sz = fz * fz * (3.0 - 2.0 * fz);
    let n00 = hash_corner(seed, x0, z0);
    let n10 = hash_corner(seed, x0 + 1, z0);
    let n01 = hash_corner(seed, x0, z0 + 1);
    let n11 = hash_corner(seed, x0 + 1, z0 + 1);
    let a = n00 + (n10 - n00) * sx;
    let b = n01 + (n11 - n01) * sx;
    a + (b - a) * sz
}

fn hash_corner(seed: u64, x: i32, z: i32) -> f32 {
    let mut n = seed
        .wrapping_add((x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add((z as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9));
    n = (n ^ (n >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    n ^= n >> 31;
    // Map to [-1, 1]
    (n as f32 / u64::MAX as f32) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn city_core_is_urban() {
        assert_eq!(biome_at(42, 10.0, 10.0), Biome::Urban);
    }

    #[test]
    fn far_out_is_countryside() {
        let b = biome_at(42, 900.0, 50.0);
        assert!(matches!(b, Biome::Field | Biome::Forest | Biome::Hills));
    }

    #[test]
    fn height_is_deterministic() {
        // Countryside sample — urban amp is 0 so seed would not matter there.
        let a = height_at(7, 900.0, 50.0);
        let b = height_at(7, 900.0, 50.0);
        assert!((a - b).abs() < 1e-6);
        assert!(height_at(7, 900.0, 50.0) != height_at(8, 900.0, 50.0));
    }

    #[test]
    fn hills_taller_than_urban() {
        let urban = height_at(99, 20.0, 20.0).abs();
        let mut hill_peak = 0.0f32;
        for i in 0..40 {
            let x = 800.0 + i as f32 * 17.0;
            let z = 600.0 + i as f32 * 13.0;
            if biome_at(99, x, z) == Biome::Hills {
                hill_peak = hill_peak.max(height_at(99, x, z).abs());
            }
        }
        assert!(
            hill_peak > urban + 1.0,
            "hill_peak={hill_peak} urban={urban}"
        );
    }

    /// Pitch-locked arcade car wedges if the road rises more than ~car half-height
    /// over one car length. City core must stay nearly flat.
    #[test]
    fn urban_slope_is_driveable_for_flat_car() {
        const CAR_LEN: f32 = 4.0;
        const MAX_RISE: f32 = 0.12; // meters over one car length
        let seed = 42u64;
        let mut worst = 0.0f32;
        for ix in -8..8 {
            for iz in -8..8 {
                let x = ix as f32 * 12.0;
                let z = iz as f32 * 12.0;
                if biome_at(seed, x, z) != Biome::Urban {
                    continue;
                }
                let h0 = height_at(seed, x, z);
                for (dx, dz) in [(CAR_LEN, 0.0), (0.0, CAR_LEN), (CAR_LEN, CAR_LEN)] {
                    let h1 = height_at(seed, x + dx, z + dz);
                    worst = worst.max((h1 - h0).abs());
                }
            }
        }
        assert!(
            worst <= MAX_RISE,
            "urban rise {worst}m over {CAR_LEN}m exceeds {MAX_RISE}m — causes invisible-wall wedging"
        );
    }
}
