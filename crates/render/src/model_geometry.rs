//! Deterministic low-poly art for the visual sandbox. Geometry is baked at compile time.
use crate::gpu::buffers::UnitVertex;

const EMPTY: UnitVertex = UnitVertex {
    pos: [0.0; 3],
    normal: [0.0, 1.0, 0.0],
    color: [0.0; 3],
    team: 0.0,
};
const STONE: [f32; 3] = [0.88, 0.82, 0.65];
const LIGHT_STONE: [f32; 3] = [0.97, 0.92, 0.77];
const ROOF: [f32; 3] = [0.71, 0.25, 0.12];
const WOOD: [f32; 3] = [0.30, 0.20, 0.12];
const DARK: [f32; 3] = [0.13, 0.16, 0.18];
const METAL: [f32; 3] = [0.71, 0.77, 0.79];
const GOLD: [f32; 3] = [0.95, 0.72, 0.24];
const SKIN: [f32; 3] = [0.77, 0.56, 0.37];
const WHITE: [f32; 3] = [0.96, 0.91, 0.77];

pub struct Mesh {
    pub vertices: [UnitVertex; 12000],
    pub len: usize,
}
impl Mesh {
    const fn new() -> Self {
        Self {
            vertices: [EMPTY; 12000],
            len: 0,
        }
    }
    const fn triangle(
        &mut self,
        a: [f32; 3],
        b: [f32; 3],
        c: [f32; 3],
        color: [f32; 3],
        team: f32,
        outward: [f32; 3],
    ) {
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let mut n = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        let reverse = n[0] * outward[0] + n[1] * outward[1] + n[2] * outward[2] < 0.0;
        let norm = sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2]);
        let sign = if reverse { -1.0 } else { 1.0 };
        n = [n[0] / norm * sign, n[1] / norm * sign, n[2] / norm * sign];
        let pts = if reverse { [a, c, b] } else { [a, b, c] };
        let mut i = 0;
        while i < 3 {
            self.vertices[self.len] = UnitVertex {
                pos: pts[i],
                normal: n,
                color,
                team,
            };
            self.len += 1;
            i += 1;
        }
    }
    const fn quad(&mut self, p: [[f32; 3]; 4], color: [f32; 3], team: f32, outward: [f32; 3]) {
        self.triangle(p[0], p[1], p[2], color, team, outward);
        self.triangle(p[0], p[2], p[3], color, team, outward);
    }
    const fn block(&mut self, c: [f32; 3], size: [f32; 3], color: [f32; 3], team: f32) {
        let x = c[0] - size[0] * 0.5;
        let x_max = c[0] + size[0] * 0.5;
        let y = c[1] - size[1] * 0.5;
        let y_max = c[1] + size[1] * 0.5;
        let z = c[2] - size[2] * 0.5;
        let z_max = c[2] + size[2] * 0.5;
        self.quad(
            [[x, y, z], [x_max, y, z], [x_max, y_max, z], [x, y_max, z]],
            color,
            team,
            [0.0, 0.0, -1.0],
        );
        self.quad(
            [
                [x, y, z_max],
                [x_max, y, z_max],
                [x_max, y_max, z_max],
                [x, y_max, z_max],
            ],
            color,
            team,
            [0.0, 0.0, 1.0],
        );
        self.quad(
            [[x, y, z], [x, y, z_max], [x, y_max, z_max], [x, y_max, z]],
            color,
            team,
            [-1.0, 0.0, 0.0],
        );
        self.quad(
            [
                [x_max, y, z],
                [x_max, y, z_max],
                [x_max, y_max, z_max],
                [x_max, y_max, z],
            ],
            color,
            team,
            [1.0, 0.0, 0.0],
        );
        self.quad(
            [
                [x, y_max, z],
                [x_max, y_max, z],
                [x_max, y_max, z_max],
                [x, y_max, z_max],
            ],
            color,
            team,
            [0.0, 1.0, 0.0],
        );
        self.quad(
            [[x, y, z], [x_max, y, z], [x_max, y, z_max], [x, y, z_max]],
            color,
            team,
            [0.0, -1.0, 0.0],
        );
    }
    const fn roof(&mut self, x: f32, z: f32, y: f32, w: f32, d: f32, h: f32, color: [f32; 3]) {
        let a = [x - w * 0.5, y, z - d * 0.5];
        let b = [x + w * 0.5, y, z - d * 0.5];
        let c = [x + w * 0.5, y, z + d * 0.5];
        let d = [x - w * 0.5, y, z + d * 0.5];
        let e = [x, y + h, a[2]];
        let f = [x, y + h, c[2]];
        self.quad([a, d, f, e], color, 0.0, [-1.0, 1.0, 0.0]);
        self.quad([b, c, f, e], color, 0.0, [1.0, 1.0, 0.0]);
        // Thin, real roof edges leave the recessed plaster gable visible below.
        let edges = [[a, e], [e, b], [d, f], [f, c]];
        let mut i = 0;
        while i < 4 {
            let u = edges[i][0];
            let v = edges[i][1];
            self.quad(
                [u, v, [v[0], v[1] - 0.012, v[2]], [u[0], u[1] - 0.012, u[2]]],
                color,
                0.0,
                [0.0, 0.0, if i < 2 { -1.0 } else { 1.0 }],
            );
            i += 1;
        }
    }
    const fn pyramid(&mut self, x: f32, z: f32, y: f32, w: f32, h: f32, color: [f32; 3]) {
        let p = [
            [x - w * 0.5, y, z - w * 0.5],
            [x + w * 0.5, y, z - w * 0.5],
            [x + w * 0.5, y, z + w * 0.5],
            [x - w * 0.5, y, z + w * 0.5],
        ];
        let top = [x, y + h, z];
        let mut i = 0;
        while i < 4 {
            let a = p[i];
            let b = p[(i + 1) % 4];
            self.triangle(
                a,
                b,
                top,
                color,
                0.0,
                [a[0] + b[0] - 2.0 * x, 1.0, a[2] + b[2] - 2.0 * z],
            );
            i += 1;
        }
    }
    const fn panel(&mut self, c: [f32; 3], size: [f32; 3], color: [f32; 3], _team: f32) {
        let x = c[0];
        let y = c[1];
        let z = c[2];
        let w = size[0] * 0.5;
        let h = size[1] * 0.5;
        self.quad(
            [
                [x - w, y - h, z],
                [x + w, y - h, z],
                [x + w, y + h, z],
                [x - w, y + h, z],
            ],
            color,
            0.0,
            [0.0, 0.0, 1.0],
        );
    }
    const fn rotate_y(&mut self, first: usize, x: f32, z: f32, cosine: f32, sine: f32) {
        let mut i = first;
        while i < self.len {
            let p = self.vertices[i].pos;
            let dx = p[0] - x;
            let dz = p[2] - z;
            self.vertices[i].pos = [
                x + dx * cosine - dz * sine,
                p[1],
                z + dx * sine + dz * cosine,
            ];
            let n = self.vertices[i].normal;
            self.vertices[i].normal = [
                n[0] * cosine - n[2] * sine,
                n[1],
                n[0] * sine + n[2] * cosine,
            ];
            i += 1;
        }
    }
    const fn segment(&mut self, a: [f32; 2], b: [f32; 2], y: f32, h: f32, w: f32, color: [f32; 3]) {
        let dx = b[0] - a[0];
        let dz = b[1] - a[1];
        let len = sqrt(dx * dx + dz * dz);
        let x = (a[0] + b[0]) * 0.5;
        let z = (a[1] + b[1]) * 0.5;
        let first = self.len;
        self.block([x, y, z], [len, h, w], color, 0.0);
        self.rotate_y(first, x, z, dx / len, dz / len);
    }
    const fn curtain(&mut self, a: [f32; 2], b: [f32; 2], height: f32) {
        self.segment(a, b, 0.04 + height * 0.5, height, 0.058, STONE);
        self.segment(a, b, height + 0.045, 0.025, 0.073, LIGHT_STONE);
        let dx = b[0] - a[0];
        let dz = b[1] - a[1];
        let len = sqrt(dx * dx + dz * dz);
        let count = (len / 0.115) as usize;
        let mut i = 0;
        while i < count {
            let t = (i as f32 + 0.5) / count as f32;
            let x = a[0] + dx * t;
            let z = a[1] + dz * t;
            let first = self.len;
            self.block(
                [x, height + 0.083, z],
                [0.053, 0.055, 0.074],
                LIGHT_STONE,
                0.0,
            );
            self.rotate_y(first, x, z, dx / len, dz / len);
            i += 1;
        }
    }
    const fn oriented_house(&mut self, h: [f32; 5], variant: usize, cosine: f32, sine: f32) {
        let first = self.len;
        self.house(h[0], h[1], h[2], h[3], h[4], variant);
        self.rotate_y(first, h[0], h[1], cosine, sine);
    }
    const fn octagonal(
        &mut self,
        bottom: [f32; 3],
        top: [f32; 3],
        r0: [f32; 2],
        r1: [f32; 2],
        color: [f32; 3],
        team: f32,
    ) {
        let ring = [
            [1.0, 0.0],
            [0.70710677, 0.70710677],
            [0.0, 1.0],
            [-0.70710677, 0.70710677],
            [-1.0, 0.0],
            [-0.70710677, -0.70710677],
            [0.0, -1.0],
            [0.70710677, -0.70710677],
        ];
        let mut i = 0;
        while i < 8 {
            let a = ring[i];
            let b = ring[(i + 1) % 8];
            let p = [
                bottom[0] + a[0] * r0[0],
                bottom[1],
                bottom[2] + a[1] * r0[1],
            ];
            let q = [
                bottom[0] + b[0] * r0[0],
                bottom[1],
                bottom[2] + b[1] * r0[1],
            ];
            let r = [top[0] + b[0] * r1[0], top[1], top[2] + b[1] * r1[1]];
            let t = [top[0] + a[0] * r1[0], top[1], top[2] + a[1] * r1[1]];
            self.quad([p, q, r, t], color, team, [a[0] + b[0], 0.0, a[1] + b[1]]);
            self.triangle(bottom, q, p, color, team, [0.0, -1.0, 0.0]);
            self.triangle(top, t, r, color, team, [0.0, 1.0, 0.0]);
            i += 1;
        }
    }
    // Six-sided limbs with independent end centers give bent elbows/knees, not cuboids.
    const fn limb(
        &mut self,
        a: [f32; 3],
        b: [f32; 3],
        ra: f32,
        rb: f32,
        color: [f32; 3],
        team: f32,
    ) {
        let ring = [
            [1.0, 0.0],
            [0.5, 0.8660254],
            [-0.5, 0.8660254],
            [-1.0, 0.0],
            [-0.5, -0.8660254],
            [0.5, -0.8660254],
        ];
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let len = sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
        let axis = [d[0] / len, d[1] / len, d[2] / len];
        let side_len = sqrt(axis[0] * axis[0] + axis[1] * axis[1]);
        let side = [axis[1] / side_len, -axis[0] / side_len, 0.0];
        let front = [
            -axis[2] * side[1],
            axis[2] * side[0],
            axis[0] * side[1] - axis[1] * side[0],
        ];
        let mut i = 0;
        while i < 6 {
            let u = ring[i];
            let v = ring[(i + 1) % 6];
            let du = [
                side[0] * u[0] + front[0] * u[1],
                side[1] * u[0] + front[1] * u[1],
                side[2] * u[0] + front[2] * u[1],
            ];
            let dv = [
                side[0] * v[0] + front[0] * v[1],
                side[1] * v[0] + front[1] * v[1],
                side[2] * v[0] + front[2] * v[1],
            ];
            let p = [a[0] + du[0] * ra, a[1] + du[1] * ra, a[2] + du[2] * ra];
            let q = [a[0] + dv[0] * ra, a[1] + dv[1] * ra, a[2] + dv[2] * ra];
            let r = [b[0] + dv[0] * rb, b[1] + dv[1] * rb, b[2] + dv[2] * rb];
            let t = [b[0] + du[0] * rb, b[1] + du[1] * rb, b[2] + du[2] * rb];
            self.quad(
                [p, q, r, t],
                color,
                team,
                [du[0] + dv[0], du[1] + dv[1], du[2] + dv[2]],
            );
            self.triangle(a, q, p, color, team, [-axis[0], -axis[1], -axis[2]]);
            self.triangle(b, t, r, color, team, axis);
            i += 1;
        }
    }
    const fn kite_shield(&mut self, x: f32, y: f32, z: f32) {
        let outline = [
            [-0.135, 0.15],
            [-0.09, 0.22],
            [0.09, 0.22],
            [0.135, 0.15],
            [0.10, -0.09],
            [0.0, -0.27],
            [-0.10, -0.09],
        ];
        let mut i = 0;
        while i < 7 {
            let a = outline[i];
            let b = outline[(i + 1) % 7];
            self.triangle(
                [x, y, z + 0.048],
                [x + a[0], y + a[1], z],
                [x + b[0], y + b[1], z],
                WHITE,
                1.0,
                [0.0, 0.0, 1.0],
            );
            self.triangle(
                [x, y, z - 0.018],
                [x + a[0], y + a[1], z - 0.018],
                [x + b[0], y + b[1], z - 0.018],
                WOOD,
                0.0,
                [0.0, 0.0, -1.0],
            );
            self.quad(
                [
                    [x + a[0], y + a[1], z],
                    [x + b[0], y + b[1], z],
                    [x + b[0], y + b[1], z - 0.018],
                    [x + a[0], y + a[1], z - 0.018],
                ],
                GOLD,
                0.0,
                [a[0] + b[0], a[1] + b[1], 0.0],
            );
            i += 1;
        }
        self.quad(
            [
                [x - 0.013, y - 0.18, z + 0.051],
                [x + 0.013, y - 0.18, z + 0.051],
                [x + 0.013, y + 0.15, z + 0.051],
                [x - 0.013, y + 0.15, z + 0.051],
            ],
            WHITE,
            0.0,
            [0.0, 0.0, 1.0],
        );
    }
    // Buildings stay deliberately broad: roof mass, wall, doorway, then a few recesses.
    const fn house(&mut self, x: f32, z: f32, w: f32, d: f32, h: f32, variant: usize) {
        let first = self.len;
        let wall = match variant % 4 {
            0 => LIGHT_STONE,
            1 => [0.87, 0.79, 0.64],
            2 => [0.94, 0.87, 0.72],
            _ => [0.82, 0.79, 0.68],
        };
        let roof = match variant % 3 {
            0 => [0.72, 0.29, 0.16],
            1 => [0.80, 0.35, 0.20],
            _ => [0.64, 0.25, 0.15],
        };
        self.block([x, 0.04 + h * 0.5, z], [w, h, d], wall, 0.0);
        self.block([x, 0.055, z], [w + 0.015, 0.03, d + 0.015], STONE, 0.0);
        let ridge = w * 0.58;
        let eave = 0.04 + h;
        // A dark fascia below the overhang separates the plaster wall from its roof.
        self.block([x, eave, z], [w + 0.040, 0.018, d + 0.045], WOOD, 0.0);
        self.roof(x, z, eave + 0.012, w + 0.045, d + 0.054, ridge, roof);
        self.block(
            [x, eave + ridge + 0.01, z],
            [0.021, 0.021, d + 0.058],
            roof,
            0.0,
        );
        // Plaster gables are inset beneath the terracotta roof edges.
        plaster_gables(self, x, z, w, d, eave, ridge, wall);
        self.window(
            x - w * 0.24,
            z + d * 0.5 + 0.003,
            h * 0.69 + 0.04,
            0.030,
            0.047,
        );
        self.window(
            x + w * 0.24,
            z + d * 0.5 + 0.003,
            h * 0.69 + 0.04,
            0.030,
            0.047,
        );
        self.panel(
            [x, 0.092, z + d * 0.5 + 0.004],
            [0.042, 0.096, 0.0],
            DARK,
            0.0,
        );
        self.block(
            [x, 0.043, z + d * 0.5 + 0.012],
            [0.064, 0.017, 0.042],
            LIGHT_STONE,
            0.0,
        );
        // Side-wall window; unlike the previous front-only painting it survives camera changes.
        self.quad(
            [
                [x + w * 0.5 + 0.002, h * 0.62, z - 0.019],
                [x + w * 0.5 + 0.002, h * 0.62, z + 0.019],
                [x + w * 0.5 + 0.002, h * 0.62 + 0.054, z + 0.019],
                [x + w * 0.5 + 0.002, h * 0.62 + 0.054, z - 0.019],
            ],
            DARK,
            0.0,
            [1.0, 0.0, 0.0],
        );
        if variant % 3 != 1 {
            let cx = x + w * 0.24;
            let cz = z - d * 0.22;
            self.block(
                [cx, eave + ridge * 0.72, cz],
                [0.031, ridge * 0.92, 0.038],
                wall,
                0.0,
            );
            self.block(
                [cx, eave + ridge * 1.20, cz],
                [0.043, 0.023, 0.049],
                STONE,
                0.0,
            );
            self.quad(
                [
                    [cx - 0.010, eave + ridge * 1.20 + 0.012, cz - 0.014],
                    [cx + 0.010, eave + ridge * 1.20 + 0.012, cz - 0.014],
                    [cx + 0.010, eave + ridge * 1.20 + 0.012, cz + 0.014],
                    [cx - 0.010, eave + ridge * 1.20 + 0.012, cz + 0.014],
                ],
                DARK,
                0.0,
                [0.0, 1.0, 0.0],
            );
        }
        if variant % 4 == 2 {
            self.block(
                [x - w * 0.46, h * 0.5 + 0.04, z + d * 0.5 + 0.003],
                [0.016, h, 0.008],
                WOOD,
                0.0,
            );
            self.block(
                [x + w * 0.46, h * 0.5 + 0.04, z + d * 0.5 + 0.003],
                [0.016, h, 0.008],
                WOOD,
                0.0,
            );
            self.block(
                [x, h * 0.45 + 0.04, z + d * 0.5 + 0.006],
                [w, 0.017, 0.009],
                WOOD,
                0.0,
            );
        }
        // Alternating actual ridge directions prevent a repeated toy-house grid.
        if variant % 2 == 1 {
            self.rotate_quarter(first, x, z);
        }
    }
    const fn rotate_quarter(&mut self, first: usize, x: f32, z: f32) {
        let mut i = first;
        while i < self.len {
            let p = self.vertices[i].pos;
            self.vertices[i].pos = [x - (p[2] - z), p[1], z + (p[0] - x)];
            let n = self.vertices[i].normal;
            self.vertices[i].normal = [-n[2], n[1], n[0]];
            i += 1;
        }
    }
    const fn window(&mut self, x: f32, z: f32, y: f32, w: f32, h: f32) {
        self.panel([x, y, z], [w, h, 0.0], DARK, 0.0);
        self.block(
            [x, y - h * 0.5 - 0.004, z + 0.006],
            [w + 0.012, 0.010, 0.020],
            LIGHT_STONE,
            0.0,
        );
    }
    const fn city_tower(
        &mut self,
        x: f32,
        z: f32,
        height: f32,
        width: f32,
        roof: bool,
        blue: bool,
    ) {
        self.octagonal(
            [x, 0.04, z],
            [x, height + 0.04, z],
            [width * 0.57, width * 0.57],
            [width * 0.50, width * 0.50],
            LIGHT_STONE,
            0.0,
        );
        self.block(
            [x, 0.069, z],
            [width + 0.026, 0.058, width + 0.026],
            STONE,
            0.0,
        );
        self.block(
            [x, height + 0.035, z],
            [width + 0.030, 0.045, width + 0.030],
            STONE,
            0.0,
        );
        self.window(x, z + width * 0.5 + 0.003, height * 0.73, 0.026, 0.072);
        if height > 0.5 {
            self.window(x, z + width * 0.5 + 0.003, height * 0.38, 0.026, 0.065);
        }
        if roof {
            self.pyramid(
                x,
                z,
                height + 0.062,
                width + 0.058,
                width * 0.90,
                if blue { [0.18, 0.35, 0.56] } else { ROOF },
            );
        } else {
            let mut j = 0;
            while j < 4 {
                let dx = if j % 2 == 0 {
                    -width * 0.40
                } else {
                    width * 0.40
                };
                let dz = if j < 2 { -width * 0.40 } else { width * 0.40 };
                self.block(
                    [x + dx, height + 0.082, z + dz],
                    [0.043, 0.062, 0.043],
                    LIGHT_STONE,
                    0.0,
                );
                j += 1;
            }
        }
    }
    const fn round_tower(&mut self, x: f32, z: f32, h: f32) {
        let ring = [
            [-1.0, 0.0],
            [-0.7071, -0.7071],
            [0.0, -1.0],
            [0.7071, -0.7071],
            [1.0, 0.0],
            [0.7071, 0.7071],
            [0.0, 1.0],
            [-0.7071, 0.7071],
        ];
        let mut i = 0;
        while i < 8 {
            let a = ring[i];
            let b = ring[(i + 1) % 8];
            self.quad(
                [
                    [x + a[0] * 0.092, 0.04, z + a[1] * 0.092],
                    [x + b[0] * 0.092, 0.04, z + b[1] * 0.092],
                    [x + b[0] * 0.092, h, z + b[1] * 0.092],
                    [x + a[0] * 0.092, h, z + a[1] * 0.092],
                ],
                LIGHT_STONE,
                0.0,
                [a[0] + b[0], 0.0, a[1] + b[1]],
            );
            self.triangle(
                [x + a[0] * 0.115, h, z + a[1] * 0.115],
                [x + b[0] * 0.115, h, z + b[1] * 0.115],
                [x, h + 0.20, z],
                [0.78, 0.36, 0.16],
                0.0,
                [a[0] + b[0], 1.0, a[1] + b[1]],
            );
            i += 1;
        }
        self.window(x, z + 0.093, h * 0.70, 0.026, 0.062);
    }
    const fn gate(&mut self, x: f32, z: f32) {
        // Stone voussoirs surround an actual tunnel: there is no black plane across it.
        let inside = [
            [-0.13, 0.04],
            [-0.13, 0.17],
            [-0.11, 0.225],
            [-0.064, 0.262],
            [0.0, 0.275],
            [0.064, 0.262],
            [0.11, 0.225],
            [0.13, 0.17],
            [0.13, 0.04],
        ];
        let mut i = 0;
        while i < 8 {
            let a = inside[i];
            let b = inside[i + 1];
            let oa = [a[0] * 1.36, if i == 0 { 0.04 } else { a[1] + 0.044 }];
            let ob = [b[0] * 1.36, if i == 7 { 0.04 } else { b[1] + 0.044 }];
            let color = if i % 2 == 0 { LIGHT_STONE } else { STONE };
            self.quad(
                [
                    [x + a[0], a[1], z + 0.06],
                    [x + b[0], b[1], z + 0.06],
                    [x + ob[0], ob[1], z + 0.06],
                    [x + oa[0], oa[1], z + 0.06],
                ],
                color,
                0.0,
                [0.0, 0.0, 1.0],
            );
            self.quad(
                [
                    [x + a[0], a[1], z - 0.06],
                    [x + b[0], b[1], z - 0.06],
                    [x + ob[0], ob[1], z - 0.06],
                    [x + oa[0], oa[1], z - 0.06],
                ],
                color,
                0.0,
                [0.0, 0.0, -1.0],
            );
            self.quad(
                [
                    [x + a[0], a[1], z - 0.06],
                    [x + b[0], b[1], z - 0.06],
                    [x + b[0], b[1], z + 0.06],
                    [x + a[0], a[1], z + 0.06],
                ],
                STONE,
                0.0,
                [-(a[0] + b[0]), -0.2, 0.0],
            );
            self.quad(
                [
                    [x + oa[0], oa[1], z - 0.06],
                    [x + ob[0], ob[1], z - 0.06],
                    [x + ob[0], ob[1], z + 0.06],
                    [x + oa[0], oa[1], z + 0.06],
                ],
                color,
                0.0,
                [a[0] + b[0], 0.2, 0.0],
            );
            i += 1;
        }
        self.block([x, 0.334, z], [0.37, 0.035, 0.135], STONE, 0.0);
    }
    const fn market(&mut self, x: f32, z: f32) {
        let mut i = 0;
        while i < 4 {
            let tint = if i % 2 == 0 {
                [0.93, 0.65, 0.26]
            } else {
                WHITE
            };
            let a = x - 0.12 + i as f32 * 0.06;
            self.quad(
                [
                    [a, 0.20, z - 0.09],
                    [a + 0.06, 0.20, z - 0.09],
                    [a + 0.06, 0.15, z + 0.10],
                    [a, 0.15, z + 0.10],
                ],
                tint,
                0.0,
                [0.0, 1.0, 0.0],
            );
            self.quad(
                [
                    [a, 0.15, z + 0.10],
                    [a + 0.06, 0.15, z + 0.10],
                    [a + 0.06, 0.125, z + 0.10],
                    [a, 0.125, z + 0.10],
                ],
                tint,
                0.0,
                [0.0, 0.0, 1.0],
            );
            i += 1;
        }
        self.block(
            [x - 0.115, 0.10, z + 0.095],
            [0.012, 0.18, 0.012],
            WOOD,
            0.0,
        );
        self.block(
            [x + 0.115, 0.10, z + 0.095],
            [0.012, 0.18, 0.012],
            WOOD,
            0.0,
        );
        self.block(
            [x, 0.072, z + 0.04],
            [0.20, 0.07, 0.055],
            [0.48, 0.32, 0.17],
            0.0,
        );
    }
    const fn banner(&mut self, x: f32, z: f32, y: f32, height: f32, large: bool) {
        self.block([x, y + height * 0.5, z], [0.022, height, 0.022], WOOD, 0.0);
        self.pyramid(x, z, y + height, 0.055, 0.07, GOLD);
        let w = if large { 0.88 } else { 0.15 };
        let h = if large { 0.95 } else { 0.19 };
        self.block(
            [x + w * 0.5, y + height - h * 0.55, z],
            [w, h, 0.014],
            WHITE,
            1.0,
        );
        self.block(
            [x + w * 0.5, y + height - h * 0.55, z + 0.009],
            [w * 0.18, h * 0.6, 0.006],
            WHITE,
            0.0,
        );
        self.block(
            [x + w * 0.5, y + height - h * 0.55, z + 0.009],
            [w * 0.6, h * 0.16, 0.006],
            WHITE,
            0.0,
        );
    }
}
const fn sqrt(v: f32) -> f32 {
    let mut x = 1.0;
    let mut i = 0;
    while i < 24 {
        x = (x + v / x) * 0.5;
        i += 1;
    }
    x
}

const fn plaster_gables(
    m: &mut Mesh,
    x: f32,
    z: f32,
    w: f32,
    d: f32,
    y: f32,
    h: f32,
    color: [f32; 3],
) {
    m.triangle(
        [x - w * 0.46, y, z + d * 0.5 + 0.001],
        [x + w * 0.46, y, z + d * 0.5 + 0.001],
        [x, y + h * 0.90, z + d * 0.5 + 0.001],
        color,
        0.0,
        [0.0, 0.0, 1.0],
    );
    m.triangle(
        [x - w * 0.46, y, z - d * 0.5 - 0.001],
        [x + w * 0.46, y, z - d * 0.5 - 0.001],
        [x, y + h * 0.90, z - d * 0.5 - 0.001],
        color,
        0.0,
        [0.0, 0.0, -1.0],
    );
}
const fn city(tier: usize) -> Mesh {
    let mut m = Mesh::new();
    let paving = [0.75, 0.70, 0.56];
    // Bent lanes and a small open plaza replace the rectilinear cross and square fortress.
    let lane = [
        [0.02, 0.69],
        [0.025, 0.40],
        [-0.08, 0.14],
        [-0.05, -0.13],
        [0.15, -0.38],
    ];
    let mut i = 0;
    while i < 4 {
        m.segment(lane[i], lane[i + 1], 0.026, 0.014, 0.105, paving);
        i += 1;
    }
    m.segment([-0.46, 0.20], [0.35, 0.025], 0.027, 0.012, 0.095, paving);
    m.octagonal(
        [-0.065, 0.025, 0.13],
        [-0.065, 0.035, 0.13],
        [0.19, 0.145],
        [0.19, 0.145],
        paving,
        0.0,
    );
    let homes = [
        [-0.36, -0.18, 0.18, 0.21, 0.28],
        [0.21, -0.31, 0.21, 0.18, 0.23],
        [-0.33, 0.15, 0.18, 0.20, 0.20],
        [0.31, -0.04, 0.19, 0.17, 0.25],
        [-0.23, 0.37, 0.19, 0.17, 0.17],
        [0.30, 0.26, 0.17, 0.21, 0.20],
        [0.01, -0.39, 0.16, 0.18, 0.28],
    ];
    let angles = [
        [0.9396926, 0.34202015],
        [0.8660254, -0.5],
        [0.9063078, -0.42261827],
        [0.819152, 0.57357645],
        [0.9659258, 0.25881904],
        [0.76604444, -0.64278764],
        [0.9396926, -0.34202015],
    ];
    i = 0;
    while i < if tier == 0 { 5 } else { 7 } {
        m.oriented_house(homes[i], i, angles[i][0], angles[i][1]);
        i += 1;
    }
    m.oriented_house([-0.095, -0.15, 0.23, 0.25, 0.34], 0, 0.9659258, -0.25881904);
    m.city_tower(
        -0.235,
        -0.265,
        if tier > 1 { 0.75 } else { 0.57 },
        0.225,
        true,
        true,
    );
    m.banner(
        -0.235,
        -0.265,
        if tier > 1 { 1.02 } else { 0.84 },
        0.19,
        false,
    );
    if tier > 0 {
        // An eleven-point irregular enceinte follows the settlement's perimeter.
        // The final edge stays open for the gate. No pair of long walls defines a square.
        let perimeter = [
            [-0.15, 0.58],
            [-0.40, 0.47],
            [-0.59, 0.22],
            [-0.56, -0.10],
            [-0.42, -0.40],
            [-0.13, -0.54],
            [0.20, -0.50],
            [0.46, -0.26],
            [0.55, 0.07],
            [0.42, 0.38],
            [0.17, 0.58],
        ];
        i = 0;
        while i < 10 {
            m.curtain(
                perimeter[i],
                perimeter[i + 1],
                0.17 + (i % 3) as f32 * 0.025,
            );
            i += 1;
        }
        m.gate(0.01, 0.58);
        m.round_tower(-0.17, 0.575, 0.30);
        m.city_tower(0.19, 0.565, 0.36, 0.135, true, false);
        m.round_tower(-0.56, 0.20, 0.31);
        m.city_tower(-0.13, -0.54, 0.36, 0.145, false, false);
        m.round_tower(0.45, -0.255, 0.42);
        m.market(0.095, 0.14);
        m.oriented_house([-0.68, 0.30, 0.18, 0.16, 0.17], 3, 0.8660254, -0.5);
        m.oriented_house([-0.64, -0.24, 0.15, 0.19, 0.22], 4, 0.819152, 0.57357645);
        m.oriented_house([0.38, 0.67, 0.19, 0.17, 0.18], 6, 0.9063078, -0.42261827);
        m.oriented_house([-0.23, 0.72, 0.17, 0.18, 0.23], 1, 0.9396926, 0.34202015);
    }
    i = 0;
    while i < m.len {
        m.vertices[i].pos[0] *= 0.75;
        m.vertices[i].pos[2] *= 0.75;
        let n = m.vertices[i].normal;
        let x = n[0] / 0.75;
        let y = n[1];
        let z = n[2] / 0.75;
        let length = sqrt(x * x + y * y + z * z);
        m.vertices[i].normal = [x / length, y / length, z / length];
        i += 1;
    }
    m
}

const fn soldier(banner: bool, civilian: bool) -> Mesh {
    let mut m = Mesh::new();
    let lean = if civilian { 0.035 } else { -0.015 };
    // Asymmetric planted/forward feet, bent knees and a tapered tunic form the silhouette.
    m.octagonal(
        [-0.11, 0.035, 0.095],
        [-0.11, 0.15, 0.065],
        [0.075, 0.13],
        [0.061, 0.085],
        WOOD,
        0.0,
    );
    m.octagonal(
        [0.105, 0.035, -0.025],
        [0.105, 0.15, -0.04],
        [0.07, 0.115],
        [0.060, 0.078],
        WOOD,
        0.0,
    );
    m.limb(
        [-0.11, 0.13, 0.045],
        [-0.125, 0.34, 0.0],
        0.052,
        0.065,
        DARK,
        0.0,
    );
    m.limb(
        [-0.125, 0.34, 0.0],
        [-0.085, 0.55, -0.015],
        0.065,
        0.072,
        DARK,
        0.0,
    );
    m.limb(
        [0.105, 0.13, -0.04],
        [0.09, 0.34, -0.06],
        0.050,
        0.060,
        DARK,
        0.0,
    );
    m.limb(
        [0.09, 0.34, -0.06],
        [0.082, 0.55, -0.02],
        0.060,
        0.07,
        DARK,
        0.0,
    );
    m.octagonal(
        [0.0, 0.51, 0.0],
        [0.0, 0.70, lean],
        [0.19, 0.12],
        [0.145, 0.10],
        WHITE,
        1.0,
    );
    m.octagonal(
        [0.0, 0.70, lean],
        [0.0, 0.93, lean],
        [0.145, 0.10],
        [0.20, 0.115],
        WHITE,
        1.0,
    );
    m.octagonal(
        [0.0, 0.675, lean],
        [0.0, 0.715, lean],
        [0.152, 0.108],
        [0.152, 0.108],
        WOOD,
        0.0,
    );
    // Skin is a chamfered jaw/head, helmet has a domed eight-face crown, never a cube.
    m.octagonal(
        [0.0, 0.93, lean],
        [0.0, 1.035, lean],
        [0.060, 0.064],
        [0.09, 0.083],
        SKIN,
        0.0,
    );
    m.octagonal(
        [0.0, 1.015, lean],
        [0.0, 1.145, lean],
        [0.095, 0.090],
        [0.11, 0.105],
        SKIN,
        0.0,
    );
    let helmet = if civilian { [0.46, 0.30, 0.16] } else { METAL };
    m.octagonal(
        [0.0, 1.105, lean],
        [0.0, 1.19, lean],
        [0.124, 0.119],
        [0.102, 0.095],
        helmet,
        0.0,
    );
    // Nonzero small top ring keeps cap triangles well-conditioned.
    m.octagonal(
        [0.0, 1.19, lean],
        [0.0, 1.265, lean],
        [0.102, 0.095],
        [0.025, 0.025],
        helmet,
        0.0,
    );
    // Eyes/visor are a single narrow front recess; the lower face stays visible.
    m.panel([0.0, 1.095, lean + 0.094], [0.13, 0.024, 0.0], DARK, 0.0);
    m.limb(
        [-0.175, 0.90, lean],
        [-0.24, 0.75, 0.025],
        0.080,
        0.058,
        WHITE,
        1.0,
    );
    m.limb(
        [-0.24, 0.75, 0.025],
        [-0.255, 0.66, 0.15],
        0.054,
        0.042,
        SKIN,
        0.0,
    );
    m.limb(
        [0.175, 0.90, lean],
        [0.25, 0.75, -0.015],
        0.080,
        0.056,
        WHITE,
        1.0,
    );
    m.limb(
        [0.25, 0.75, -0.015],
        [0.305, 0.79, 0.075],
        0.053,
        0.040,
        SKIN,
        0.0,
    );
    if civilian {
        m.octagonal(
            [0.0, 0.60, -0.175],
            [0.0, 0.87, -0.165],
            [0.16, 0.10],
            [0.135, 0.088],
            [0.61, 0.43, 0.24],
            0.0,
        );
        m.limb(
            [0.315, 0.03, 0.085],
            [0.315, 1.27, 0.085],
            0.016,
            0.018,
            WOOD,
            0.0,
        );
    } else {
        m.kite_shield(-0.275, 0.67, 0.175);
        if banner {
            m.banner(-0.43, 0.0, 0.10, 2.80, true);
        } else {
            m.limb(
                [0.315, 0.03, 0.085],
                [0.315, 1.42, 0.085],
                0.015,
                0.012,
                WOOD,
                0.0,
            );
            m.octagonal(
                [0.315, 1.40, 0.085],
                [0.315, 1.57, 0.085],
                [0.041, 0.025],
                [0.002, 0.002],
                METAL,
                0.0,
            );
        }
    }
    m
}

pub const SOLDIER: Mesh = soldier(false, false);
pub const STANDARD: Mesh = soldier(true, false);
pub const CIVILIAN: Mesh = soldier(false, true);
pub const HAMLET: Mesh = city(0);
pub const TOWN: Mesh = city(1);
pub const CITY: Mesh = city(2);
