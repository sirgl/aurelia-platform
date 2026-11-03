//! Hex layout: axial (q,r) <-> world XZ. Pointy-top:
//! x = sqrt(3) * (q + r/2), z = 1.5 * r. Pure math, no GPU.

/// Hex radius in world units.
pub const HEX_SIZE: f32 = 1.0;
const SQRT3: f32 = 1.7320508;

pub fn tile_to_world(q: u32, r: u32) -> (f32, f32) {
    let (q, r) = (q as f32, r as f32);
    (SQRT3 * (q + r * 0.5) * HEX_SIZE, 1.5 * r * HEX_SIZE)
}

/// The neighbour across edge i (edge i is between corners i and i+1 of hex_corners,
/// the midpoint at 60 degrees*i in XZ). The order must match hex_corners():
/// E, SE, SW, W, NW, NE. It lives here (a leaf module without dependencies),
/// buffers re-exports it for compatibility. Mirroring breaks the pairing
/// (caught by duplicate segments; see the borders test).
pub const EDGE_NEIGHBORS: [(i32, i32); 6] = [(1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];

pub fn map_center(width: u32, height: u32) -> (f32, f32) {
    let (w, h) = (width as f32, height as f32);
    (
        SQRT3 * ((w - 1.0) * 0.5 + (h - 1.0) * 0.25) * HEX_SIZE,
        1.5 * (h - 1.0) * 0.5 * HEX_SIZE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_maps_to_origin() {
        assert_eq!(tile_to_world(0, 0), (0.0, 0.0));
    }

    #[test]
    fn neighbors_are_hex_size_apart() {
        // The eastern neighbour (1,0): the distance is exactly sqrt(3)*size.
        let (x0, z0) = tile_to_world(0, 0);
        let (x1, z1) = tile_to_world(1, 0);
        let d = ((x1 - x0).powi(2) + (z1 - z0).powi(2)).sqrt();
        assert!((d - SQRT3 * HEX_SIZE).abs() < 1e-5, "d={}", d);
        // South-east (0,1): the same distance.
        let (x2, z2) = tile_to_world(0, 1);
        let d2 = ((x2 - x0).powi(2) + (z2 - z0).powi(2)).sqrt();
        assert!((d2 - SQRT3 * HEX_SIZE).abs() < 1e-5, "d2={}", d2);
    }
}
