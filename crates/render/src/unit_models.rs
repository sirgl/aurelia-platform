//! Unit models baked from platform/assets/models/manifest.json by build.rs (build/units.rs).
//!
//! A unit is drawn from its stable content key (its own key or its unit class key, mapped
//! to a model by the manifest), resolved once per ruleset into a `UnitModelBook`.
//! The numeric `UnitKindId` is only an index into that book, never a mesh index.

use kernel::module::RulesModule;
use crate::gpu::buffers::UnitVertex;
use kernel::rules::CompiledRuleset;

/// What a model shows: a fighting unit by its element, or a unit that never fights
/// (the manifest's finer art categories of non-combatants all draw as civilians).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitCategory {
    Military,
    Naval,
    Air,
    Civilian,
}

impl UnitCategory {
    /// Units that never fight.
    pub fn is_civilian(self) -> bool {
        self == Self::Civilian
    }
}

/// How the model meets the map: feet on the ground, hull at the waterline, or hovering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contact {
    Ground,
    Water,
    Hover,
}

/// How a model is animated (the vertex shader's deformation, `platform/tools/blender/units/rigs.py`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Rig {
    /// A person: legs, arms, torso.
    Biped,
    /// A rider on a horse (legs of the mount, rider bob).
    Horse,
    Elephant,
    Camel,
    /// Two-wheeled chariot with a horse.
    Chariot,
    /// A wagon: it rolls.
    Cart,
    /// A crewed gun or siege engine: rolls, recoils, throws.
    Engine,
    /// A tank, a truck: tracks or wheels, a turret.
    Vehicle,
    /// A ship: bob, pitch, roll.
    Vessel,
    /// Aircraft: bank and hover.
    Plane,
    /// A missile or bomb.
    Missile,
    /// Does not animate.
    Static,
}

/// What a model fights with: picks its attack animation and the effects of its attack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Weapon {
    None,
    /// Swords, axes, sabres.
    Blade,
    Club,
    /// Spears, lances, pikes.
    Polearm,
    Bow,
    Crossbow,
    /// Slings and atlatls.
    Thrown,
    /// Muskets and rifles.
    Firearm,
    /// Machine guns, automatic rifles, aircraft guns.
    Autogun,
    /// Bazookas, rocket trucks, hwachas, surface-to-air missiles.
    Rocket,
    /// Cannons, field guns, tank guns.
    Cannon,
    Catapult,
    Ballista,
    Ram,
    /// Ship guns.
    Broadside,
    Torpedo,
    /// Aircraft bombs.
    Bomb,
    Missile,
}

impl Rig {
    /// The shader's rig index (0 is no rig).
    pub fn code(self) -> f32 {
        self as u8 as f32 + 1.0
    }
}

impl Weapon {
    /// The shader's action code of an attack with this weapon (1 + index; 0 is no action).
    pub fn action(self) -> f32 {
        self as u8 as f32 + 1.0
    }

    /// The unit sends something across to the target (the attacker does not step into it).
    pub fn is_ranged(self) -> bool {
        !matches!(self, Weapon::None | Weapon::Blade | Weapon::Club | Weapon::Polearm | Weapon::Ram)
    }
}

/// Number of detail levels: close-up, reduced, far marker.
pub const LODS: usize = 3;
/// Index of the far marker LOD: drawn once per unit, at its leader slot.
pub const MARKER_LOD: usize = 2;

/// Shader action code of a hit reaction (`unit_anim.wgsl`).
pub const ACTION_HIT: f32 = 100.0;

#[derive(Debug)]
pub struct UnitModel {
    /// Asset id from the manifest ("unit/legion").
    pub id: &'static str,
    pub category: UnitCategory,
    pub contact: Contact,
    /// Formation figures in hex units: (x, z, scale). Slot 0 is the leader.
    pub slots: &'static [[f32; 3]],
    /// Vertex range (start, count) per LOD in `vertices()`.
    pub lods: [(u32, u32); LODS],
    /// Projected model height in pixels below which LOD1 / the marker is used.
    pub lod_px: [f32; 2],
    /// Model height in hex units (LOD0).
    pub height: f32,
    pub rig: Rig,
    pub weapon: Weapon,
    /// Yaw (radians) the catalog baked into the model: its own forward is this far from +z.
    pub baked_yaw: f32,
    /// Authoring units (a person is 1.0 tall) to hex units.
    pub scale: f32,
}

impl UnitModel {
    /// The instance's rig constants at rest: action 0, rig, authoring units per hex
    /// unit, the yaw baked into the model.
    pub fn rig_state(&self) -> [f32; 4] {
        match self.animated() {
            true => [0.0, self.rig.code(), 1.0 / self.scale, self.baked_yaw],
            false => [0.0; 4],
        }
    }

    /// The model animates (it has a rig the shader deforms).
    pub fn animated(&self) -> bool {
        !matches!(self.rig, Rig::Static | Rig::Missile)
    }
}

include!(concat!(env!("OUT_DIR"), "/unit_models.rs"));

#[cfg(not(target_arch = "wasm32"))]
#[repr(C, align(4))]
struct Aligned<T: ?Sized>(T);

#[cfg(not(target_arch = "wasm32"))]
static VERTEX_BYTES: &Aligned<[u8]> = &Aligned(*include_bytes!(concat!(
    env!("OUT_DIR"),
    "/unit_models.bin"
)));

/// The vertex data in the browser: it is not compiled into the wasm (it would be megabytes of it);
/// the page fetches `unit_models.bin` and hands it over with `provide` before the renderer starts.
#[cfg(target_arch = "wasm32")]
static PROVIDED: std::sync::OnceLock<&'static [UnitVertex]> = std::sync::OnceLock::new();

/// Browser only: the contents of `unit_models.bin` as little-endian words (a `Uint32Array`). Call it
/// once, before the renderer is created. The vector becomes the vertex storage: no copy.
#[cfg(target_arch = "wasm32")]
pub fn provide(words: Vec<u32>) -> Result<(), String> {
    let vertices = crate::model_data::vertices_from_words(words, UNIT_VERTEX_COUNT, "unit_models.bin")?;
    PROVIDED.set(vertices).map_err(|_| "unit_models.bin was provided twice".to_string())
}

/// Every LOD of every unit model, in hex units, flat normals (one GPU buffer).
#[cfg(not(target_arch = "wasm32"))]
pub fn vertices() -> &'static [UnitVertex] {
    bytemuck::cast_slice(&VERTEX_BYTES.0)
}

/// Every LOD of every unit model, in hex units, flat normals (one GPU buffer).
#[cfg(target_arch = "wasm32")]
pub fn vertices() -> &'static [UnitVertex] {
    PROVIDED.get().copied().expect("unit_models.bin was not provided before the first frame (render::unit_models::provide)")
}

pub fn models() -> &'static [UnitModel] {
    &UNIT_MODELS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UnitModelId(pub u16);

impl UnitModelId {
    /// Explicit fallback (a team banner) for keys the manifest does not know.
    pub const FALLBACK: Self = Self(FALLBACK_MODEL);

    pub fn model(self) -> &'static UnitModel {
        &UNIT_MODELS[self.0 as usize]
    }

    /// Radius (hex units, scale 1) of the circle the model touches the ground with: the widest
    /// reach of its low vertices (LOD 0, below 6 % of its height) around the origin.
    pub fn foot_radius(self) -> f32 {
        static FEET: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
        FEET.get_or_init(|| {
            (0..models().len())
                .map(|i| {
                    let id = UnitModelId(i as u16);
                    let low = id.model().height * 0.06 + 0.01;
                    let r = id.mesh(0).iter().filter(|v| v.pos[1] <= low).map(|v| v.pos[0].hypot(v.pos[2])).fold(0.0f32, f32::max);
                    r.clamp(0.04, 0.5)
                })
                .collect()
        })[self.0 as usize]
    }

    pub fn mesh(self, lod: usize) -> &'static [UnitVertex] {
        let (start, count) = self.model().lods[lod];
        &vertices()[start as usize..(start + count) as usize]
    }
}

/// Model for a unit or unit-class key; None when the manifest has no entry.
pub fn resolve(key: &str) -> Option<UnitModelId> {
    UNIT_KEYS
        .binary_search_by(|(k, _)| (*k).cmp(key))
        .ok()
        .map(|i| UnitModelId(UNIT_KEYS[i].1))
}

/// Model by its asset id ("unit/spearman"): a content pack's art catalog names models
/// so. None when the manifest has no such model.
pub fn model_by_id(id: &str) -> Option<UnitModelId> {
    models().iter().position(|m| m.id == id).map(|i| UnitModelId(i as u16))
}

/// Unit kind (index into the compiled ruleset's units) -> model, built once per ruleset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitModelBook {
    by_kind: Vec<UnitModelId>,
    /// The boat a land unit is drawn as while it is afloat (the module's art binding;
    /// None: its soldiers are drawn on the water all the same).
    embarked: Option<UnitModelId>,
    /// Per kind: a land unit, the kind that takes to the water (`embarking_kinds`).
    afloat: Vec<bool>,
}

/// The manifest key of the boat that the classic land units cross the water in
/// (`UnitModelBook::from_rules`).
pub const EMBARKED_KEY_CORE: &str = "core:embarked";

/// Per kind of `rules`: is it a land unit, one that is drawn as a boat on water? (Whether
/// a unit hovers is its promotions': the projection's `UnitMarker::embarked` tells that
/// of a unit; this tells the kind, for units mid-move that the projection no longer
/// holds.)
pub fn embarking_kinds<M: RulesModule>(rules: &CompiledRuleset<M>) -> Vec<bool> {
    rules.units.iter().map(|unit| unit.domain == kernel::rules::content::Domain::Land).collect()
}

impl UnitModelBook {
    pub fn from_rules<M: RulesModule>(rules: &CompiledRuleset<M>) -> Self {
        Self::from_keys(rules.units.iter().map(|unit| unit.key.as_str()))
            .with_embarked(resolve(EMBARKED_KEY_CORE), embarking_kinds(rules))
    }

    /// The book with the boat that the kinds flagged in `afloat` (in kind order) are
    /// drawn as on water; `None` keeps their own model.
    pub fn with_embarked(mut self, boat: Option<UnitModelId>, afloat: impl IntoIterator<Item = bool>) -> Self {
        self.embarked = boat;
        self.afloat = afloat.into_iter().collect();
        self
    }

    /// Keys in kind order. A key without a manifest entry draws the explicit fallback.
    pub fn from_keys<'a>(keys: impl IntoIterator<Item = &'a str>) -> Self {
        let by_kind = keys
            .into_iter()
            .map(|key| resolve(key).unwrap_or(UnitModelId::FALLBACK))
            .collect();
        Self { by_kind, ..Self::default() }
    }

    /// Models in kind order from a content pack's art catalog (None: the fallback).
    pub fn from_models(models: impl IntoIterator<Item = Option<UnitModelId>>) -> Self {
        let by_kind = models.into_iter().map(|m| m.unwrap_or(UnitModelId::FALLBACK)).collect();
        Self { by_kind, ..Self::default() }
    }

    pub fn model_for_kind(&self, kind: u16) -> UnitModelId {
        match self.by_kind.get(kind as usize) {
            Some(id) => *id,
            None => UnitModelId::FALLBACK,
        }
    }

    /// The model of a unit of `kind`: its own, or the boat while it is `embarked`.
    pub fn model_for_unit(&self, kind: u16, embarked: bool) -> UnitModelId {
        match (embarked, self.embarked) {
            (true, Some(boat)) => boat,
            _ => self.model_for_kind(kind),
        }
    }

    /// Is a unit of `kind` drawn as a boat on a water tile (it is a land unit and the
    /// module binds a boat)? For units that move: the projection's flag is of where
    /// they stand when the move is done.
    pub fn embarks(&self, kind: u16) -> bool {
        self.embarked.is_some() && self.afloat.get(kind as usize).copied().unwrap_or(false)
    }
}

/// Detail level for a model seen at `px_per_unit` screen pixels per hex unit.
/// Switching to a finer level needs 15% more pixels than the switch point, so a
/// camera resting near a threshold does not flicker between levels.
pub fn select_lod(model: &UnitModel, px_per_unit: f32, current: usize) -> usize {
    select_detail(model.height, model.lod_px, px_per_unit, current)
}

/// Detail level (0 close-up, 1 reduced, 2 far marker) for a model of `size` hex units
/// whose LOD switches at `lod_px` projected pixels, with the hysteresis of `select_lod`.
pub fn select_detail(size: f32, lod_px: [f32; 2], px_per_unit: f32, current: usize) -> usize {
    const HYSTERESIS: f32 = 1.15;
    let px = size * px_per_unit;
    let mut lod = 0;
    for (i, threshold) in lod_px.iter().enumerate() {
        let finer = i < current;
        let needed = if finer {
            threshold * HYSTERESIS
        } else {
            *threshold
        };
        if px < needed {
            lod = i + 1;
        }
    }
    lod
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_resolve_by_their_asset_id() {
        // Shared models (the platform's own) are there in every build; the content keys that name
        // them are the modules' (tests of classic-ui and lands).
        let warrior = model_by_id("unit/warrior").expect("warrior");
        assert_eq!(warrior.model().id, "unit/warrior");
        assert_eq!(model_by_id("unit/no-such-unit"), None);
        assert_eq!(UnitModelId::FALLBACK.model().id, "unit/unknown");
        assert_eq!(resolve("a-module:unit/no-such-unit"), None);
    }

    #[test]
    fn baked_models_are_grounded_inside_the_hex_and_within_budget() {
        let all = vertices();
        assert_eq!(all.len(), UNIT_VERTEX_COUNT);
        for (i, model) in models().iter().enumerate() {
            let id = UnitModelId(i as u16);
            assert!(
                !model.slots.is_empty() && model.slots.len() <= 6,
                "{}",
                model.id
            );
            let lod0 = id.mesh(0);
            assert!(
                lod0.len().is_multiple_of(3) && !lod0.is_empty(),
                "{}",
                model.id
            );
            assert!(
                lod0.len() / 3 * model.slots.len() <= 3000,
                "{} formation budget",
                model.id
            );
            assert!(id.mesh(0).len() >= id.mesh(1).len() && id.mesh(1).len() >= id.mesh(2).len());
            let low = lod0.iter().map(|v| v.pos[1]).fold(f32::INFINITY, f32::min);
            match model.contact {
                Contact::Ground => {
                    assert!((-0.01..=0.02).contains(&low), "{} floats {low}", model.id)
                }
                Contact::Water => assert!(low < 0.0, "{} hull above water", model.id),
                Contact::Hover => assert!(low > 0.0, "{} hover touches ground", model.id),
            }
            for v in lod0 {
                assert!(v.pos.iter().all(|x| x.is_finite()));
                assert!(v.team == 0.0 || v.team == 1.0);
                let n = v.normal.iter().map(|x| x * x).sum::<f32>().sqrt();
                assert!((n - 1.0).abs() < 1e-3, "{} normal {:?}", model.id, v.normal);
                for [x, z, s] in model.slots {
                    let reach = (v.pos[0] * s + x).hypot(v.pos[2] * s + z);
                    assert!(reach <= 0.9, "{} leaves its hex: {reach}", model.id);
                }
            }
        }
    }

    #[test]
    fn lod_follows_projected_size_with_hysteresis() {
        let model = model_by_id("unit/warrior").unwrap().model();
        let [near, far] = model.lod_px;
        let px_per_unit = |px: f32| px / model.height;
        assert_eq!(select_lod(model, px_per_unit(near * 2.0), 2), 0);
        assert_eq!(select_lod(model, px_per_unit(near * 0.9), 0), 1);
        assert_eq!(select_lod(model, px_per_unit(far * 0.9), 1), 2);
        // Just above a switch point: the coarser level is kept until the margin is passed.
        assert_eq!(select_lod(model, px_per_unit(near * 1.05), 1), 1);
        assert_eq!(select_lod(model, px_per_unit(near * 1.05), 0), 0);
        assert_eq!(select_lod(model, px_per_unit(far * 1.05), 2), 2);
        assert_eq!(select_lod(model, px_per_unit(near * 1.2), 1), 0);
    }
}
