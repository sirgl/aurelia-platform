//! Terrain style as data (not code): Source -> compile -> Compiled.
//!
//! Mirrors the gameplay rules pipeline (rules::compile): a human-readable
//! source, strict validation, a deterministic runtime. A new biome/palette/
//! height = an edit of `default_style()` + recompilation; the mesh builder and shaders
//! are untouched. RON parsing on top comes later (the classic-content loader stub waits).

/// The known set of terrain ids (matches the generator and cost).
/// The compiler requires coverage of all: a missing biome is an error, not a default.
/// 0 plains, 1 grass, 2 hills, 3 desert, 4 water, 5 snow, 6 mountain.
pub const KNOWN_TERRAINS: u16 = 8;

/// The procedural water layer in the texture array (after the file layers).
pub const WATER_LAYER: u32 = 6;

/// The single waterline (v26 review): the water surface and the shore bowl
/// are computed from the same constants, otherwise the terrain sticks through the surface.
/// surface = water_h + LIFT; underwater ground is not above surface - CLEARANCE.
pub const WATER_SURFACE_LIFT: f32 = 0.03;
pub const WATER_GROUND_CLEARANCE: f32 = 0.01;

#[derive(Debug, Clone)]
pub struct SourceTerrain {
    pub id: u16,
    pub height: f32,
    pub color: [f32; 3],
    /// Texture name from assets (plains/grass/hills/desert/snow).
    /// None = a procedural layer (water -> WATER_LAYER).
    pub texture: Option<String>,
    /// Relief raise [centre, inner ring] (v28c): 0 = flat.
    /// Water must have [0, 0] (the lake surface is a plane!).
    pub raise: [f32; 2],
}

#[derive(Debug, Clone, Default)]
pub struct SourceTerrainStyle {
    pub terrains: Vec<SourceTerrain>,
}

/// Compiled style: dense vecs, index = terrain id.
#[derive(Debug, Clone)]
pub struct CompiledTerrainStyle {
    pub heights: Vec<f32>,
    pub colors: Vec<[f32; 3]>,
    /// Texture array layer per terrain (water -> WATER_LAYER).
    pub texture_layer: Vec<u32>,
    /// Relief raise [centre, inner] per terrain.
    pub shape: Vec<[f32; 2]>,
    pub version: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StyleError {
    DuplicateId(u16),
    MissingId(u16),
    BadHeight { id: u16, height: f32 },
    BadColor { id: u16, channel: usize, value: f32 },
    UnknownTexture { id: u16, name: String },
}

impl std::fmt::Display for StyleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for StyleError {}

/// The current palette/relief (former hardcodes of buffers.rs, now data).
/// texture=None only for water (a procedural layer).
pub fn default_style() -> SourceTerrainStyle {
    SourceTerrainStyle {
        terrains: vec![
            SourceTerrain {
                id: 0,
                height: 0.35,
                raise: [0.0, 0.0],
                color: [0.66, 0.70, 0.36],
                texture: Some("plains".into()),
            },
            SourceTerrain {
                id: 1,
                height: 0.30,
                raise: [0.0, 0.0],
                color: [0.55, 0.67, 0.33],
                texture: Some("grass".into()),
            },
            SourceTerrain {
                id: 2,
                height: 0.55,
                raise: [0.40, 0.20],
                color: [0.59, 0.64, 0.39],
                texture: Some("hills".into()),
            },
            SourceTerrain {
                id: 3,
                height: 0.25,
                raise: [0.04, 0.02],
                color: [0.80, 0.70, 0.43],
                texture: Some("desert".into()),
            },
            SourceTerrain {
                id: 4,
                height: 0.05,
                raise: [0.0, 0.0],
                color: [0.08, 0.46, 0.66],
                texture: None,
            },
            SourceTerrain {
                id: 5,
                height: 0.45,
                raise: [0.12, 0.06],
                color: [0.85, 0.87, 0.90],
                texture: Some("snow".into()),
            },
            // Mountains (biome v33, silhouette towers v35): a high centre + a strong
            // ridge; a narrow dome (see MOUNTAIN_INNER_FRACTION) is a tower,
            // not a sagging dome. Impassable (cost None), yields 0 in the sim;
            // the peaks grow here.
            SourceTerrain {
                id: 6,
                height: 0.95,
                raise: [0.55, 0.42],
                color: [0.46, 0.49, 0.56],
                texture: Some("mountain".into()),
            },
            // Tundra (map code 7): low cold ground of moss and lichen.
            SourceTerrain {
                id: 7,
                height: 0.32,
                raise: [0.02, 0.01],
                color: [0.58, 0.60, 0.48],
                texture: Some("plains".into()),
            },
        ],
    }
}

/// The order of file layers in the texture array (contract with the classic-content loader:
/// names resolve to indices of this list; water is appended as the WATER_LAYER layer).
pub const TEXTURE_LAYERS: [&str; 6] = ["plains", "grass", "hills", "desert", "snow", "mountain"];

pub fn compile(source: SourceTerrainStyle) -> Result<CompiledTerrainStyle, StyleError> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    for t in &source.terrains {
        if !seen.insert(t.id) {
            return Err(StyleError::DuplicateId(t.id));
        }
        if !(0.0..=2.0).contains(&t.height) || !t.height.is_finite() {
            return Err(StyleError::BadHeight {
                id: t.id,
                height: t.height,
            });
        }
        for r in &t.raise {
            if !(0.0..=1.0).contains(r) || !r.is_finite() {
                return Err(StyleError::BadHeight {
                    id: t.id,
                    height: *r,
                });
            }
        }
        for (i, c) in t.color.iter().enumerate() {
            if !(0.0..=1.0).contains(c) || !c.is_finite() {
                return Err(StyleError::BadColor {
                    id: t.id,
                    channel: i,
                    value: *c,
                });
            }
        }
    }
    for id in 0..KNOWN_TERRAINS {
        if !seen.contains(&id) {
            return Err(StyleError::MissingId(id));
        }
    }
    let mut sorted = source.terrains;
    sorted.sort_by_key(|t| t.id);
    let mut texture_layer = Vec::with_capacity(sorted.len());
    for t in &sorted {
        match &t.texture {
            None => texture_layer.push(WATER_LAYER),
            Some(name) => match TEXTURE_LAYERS.iter().position(|l| l == name) {
                Some(idx) => texture_layer.push(idx as u32),
                None => {
                    return Err(StyleError::UnknownTexture {
                        id: t.id,
                        name: name.clone(),
                    })
                }
            },
        }
    }
    Ok(CompiledTerrainStyle {
        heights: sorted.iter().map(|t| t.height).collect(),
        colors: sorted.iter().map(|t| t.color).collect(),
        texture_layer,
        shape: sorted.iter().map(|t| t.raise).collect(),
        version: 1,
    })
}

impl CompiledTerrainStyle {
    pub fn height(&self, terrain: u16) -> Option<f32> {
        self.heights.get(terrain as usize).copied()
    }

    pub fn color(&self, terrain: u16) -> Option<[f32; 3]> {
        self.colors.get(terrain as usize).copied()
    }

    pub fn layer(&self, terrain: u16) -> Option<u32> {
        self.texture_layer.get(terrain as usize).copied()
    }

    /// Relief raise [centre, inner] (v28c). Water is always [0, 0].
    pub fn raise(&self, terrain: u16) -> Option<[f32; 2]> {
        self.shape.get(terrain as usize).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_style_compiles() {
        let compiled = compile(default_style()).unwrap();
        assert_eq!(compiled.heights.len(), KNOWN_TERRAINS as usize);
        assert!(compiled.height(2).unwrap() > compiled.height(0).unwrap());
    }

    #[test]
    fn compiler_rejects_garbage() {
        let mut dup = default_style();
        dup.terrains.push(SourceTerrain {
            id: 0,
            height: 0.5,
            color: [0.0, 0.0, 0.0],
            texture: Some("plains".into()),
            raise: [0.0, 0.0],
        });
        assert_eq!(compile(dup).unwrap_err(), StyleError::DuplicateId(0));

        let mut missing = default_style();
        missing.terrains.retain(|t| t.id != 3);
        assert_eq!(compile(missing).unwrap_err(), StyleError::MissingId(3));

        let mut bad_h = default_style();
        bad_h.terrains[1].height = f32::NAN;
        assert!(matches!(
            compile(bad_h).unwrap_err(),
            StyleError::BadHeight { .. }
        ));

        let mut bad_c = default_style();
        bad_c.terrains[1].color[0] = 2.0;
        assert!(matches!(
            compile(bad_c).unwrap_err(),
            StyleError::BadColor { .. }
        ));

        let mut bad_t = default_style();
        bad_t.terrains[1].texture = Some("lava".into());
        assert_eq!(
            compile(bad_t).unwrap_err(),
            StyleError::UnknownTexture {
                id: 1,
                name: "lava".into()
            }
        );
    }

    #[test]
    fn water_resolves_to_proc_layer() {
        let compiled = compile(default_style()).unwrap();
        assert_eq!(compiled.layer(4), Some(super::WATER_LAYER));
        assert_eq!(compiled.layer(0), Some(0));
        assert_eq!(compiled.layer(5), Some(4));
    }
}
