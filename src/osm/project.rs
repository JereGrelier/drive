//! Local tangent-plane projection (meters) from WGS84 lon/lat.

use bevy::prelude::Vec2;

#[derive(Clone, Copy, Debug)]
pub struct LocalFrame {
    pub origin_lon: f64,
    pub origin_lat: f64,
    meters_per_deg_lon: f64,
    meters_per_deg_lat: f64,
}

impl LocalFrame {
    pub fn from_origin(origin_lon: f64, origin_lat: f64) -> Self {
        let lat_rad = origin_lat.to_radians();
        let meters_per_deg_lat =
            111_132.92 - 559.82 * (2.0 * lat_rad).cos() + 1.175 * (4.0 * lat_rad).cos();
        let meters_per_deg_lon = 111_412.84 * lat_rad.cos() - 93.5 * (3.0 * lat_rad).cos();
        Self {
            origin_lon,
            origin_lat,
            meters_per_deg_lon,
            meters_per_deg_lat,
        }
    }

    pub fn project(&self, lon: f64, lat: f64) -> Vec2 {
        let x = ((lon - self.origin_lon) * self.meters_per_deg_lon) as f32;
        let z = ((lat - self.origin_lat) * self.meters_per_deg_lat) as f32;
        // Bevy XZ ground: +X east, +Z north (lat increases north)
        Vec2::new(x, z)
    }
}
