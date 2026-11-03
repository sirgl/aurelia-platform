//! What tile rules read about tiles (`TileFacts`), from two sources: the
//! truth as one player sees it on the map it rules (`TruthFacts`: game
//! systems, validation) and a player's knowledge (`PlayerView`: AI and UI
//! estimates). One rule — tile yields, improvement sites — works on both,
//! so the AI evaluates exactly the rule the game applies, without reading
//! hidden state (platform/structure.md §21–22).

use crate::module::RulesModule;
use crate::derived::tile_yields::TechSet;
use crate::ids::{FeatureId, ImprovementId, PlayerId, ResourceId, TerrainId, TileId};
use crate::knowledge::view::PlayerView;
use crate::math::MapDimensions;
use crate::rules::CompiledRuleset;
use crate::world::state::AuthWorld;

pub trait TileFacts {
    fn dimensions(&self) -> MapDimensions;
    fn terrain(&self, tile: TileId) -> Option<TerrainId>;
    fn feature(&self, tile: TileId) -> Option<FeatureId>;
    fn natural_wonder(&self, tile: TileId) -> Option<crate::rules::content::NaturalWonderId>;
    /// The resource as the viewer sees it (its reveal technology known).
    fn resource(&self, tile: TileId) -> Option<ResourceId>;
    fn improvement(&self, tile: TileId) -> Option<ImprovementId>;
    /// The tile's improvement is pillaged (as far as the viewer sees).
    fn pillaged(&self, tile: TileId) -> bool;
    fn owner(&self, tile: TileId) -> Option<PlayerId>;
    fn city(&self, tile: TileId) -> bool;
    /// A river runs along an edge of the tile.
    fn river(&self, tile: TileId) -> bool;
    fn lake(&self, tile: TileId) -> bool;
    fn coastal_land(&self, tile: TileId) -> bool;
    fn is_water(&self, tile: TileId) -> bool;
    /// The player's land hosts other players' host-land improvements
    /// (`ModulePlayerKind::hosts_improvements`; public knowledge).
    fn is_host(&self, player: PlayerId) -> bool;
}

/// The truth, with resources filtered by the technologies of one player.
#[derive(Clone, Copy)]
pub struct TruthFacts<'a, M: RulesModule> {
    pub auth: &'a AuthWorld<M>,
    pub rules: &'a CompiledRuleset<M>,
    pub techs: TechSet,
}

impl<'a, M: RulesModule> TruthFacts<'a, M> {
    pub fn new(auth: &'a AuthWorld<M>, rules: &'a CompiledRuleset<M>, viewer: PlayerId) -> Self {
        TruthFacts {
            auth,
            rules,
            techs: TechSet::of_player(auth, viewer),
        }
    }
}

impl<M: RulesModule> TileFacts for TruthFacts<'_, M> {
    fn dimensions(&self) -> MapDimensions {
        self.auth.map().dimensions
    }
    fn terrain(&self, tile: TileId) -> Option<TerrainId> {
        self.auth.map().terrain(tile)
    }
    fn feature(&self, tile: TileId) -> Option<FeatureId> {
        self.auth.map().feature(tile).flatten()
    }
    fn natural_wonder(&self, tile: TileId) -> Option<crate::rules::content::NaturalWonderId> {
        self.auth.map().natural_wonder(tile)
    }
    fn resource(&self, tile: TileId) -> Option<ResourceId> {
        let resource = self.auth.map().resource(tile).flatten()?;
        let def = self.rules.resources.get(resource.0 as usize)?;
        self.techs.allows(def.required_tech).then_some(resource)
    }
    fn improvement(&self, tile: TileId) -> Option<ImprovementId> {
        self.auth.map().improvement(tile).flatten()
    }
    fn pillaged(&self, tile: TileId) -> bool {
        self.auth.map().pillaged(tile)
    }
    fn owner(&self, tile: TileId) -> Option<PlayerId> {
        self.auth.map().owner(tile).flatten()
    }
    fn city(&self, tile: TileId) -> bool {
        self.auth.indexes().city_on_tile(tile).is_some()
    }
    fn river(&self, tile: TileId) -> bool {
        self.auth.map().river(tile)
    }
    fn lake(&self, tile: TileId) -> bool {
        self.auth.map().lake(tile)
    }
    fn coastal_land(&self, tile: TileId) -> bool {
        self.auth.map().coastal_land(tile)
    }
    fn is_water(&self, tile: TileId) -> bool {
        self.auth.map().is_water(tile)
    }
    fn is_host(&self, player: PlayerId) -> bool {
        is_host(self.auth, player)
    }
}

/// A player's knowledge: unknown tiles have no facts; the geography of a
/// known tile (terrain, river, lake, coast) is static and read from the map.
impl<M: RulesModule> TileFacts for PlayerView<'_, M> {
    fn dimensions(&self) -> MapDimensions {
        self.map_dimensions()
    }
    fn terrain(&self, tile: TileId) -> Option<TerrainId> {
        self.tile(tile).snapshot().map(|s| s.terrain)
    }
    fn feature(&self, tile: TileId) -> Option<FeatureId> {
        self.tile(tile).snapshot().and_then(|s| s.feature)
    }
    fn natural_wonder(&self, tile: TileId) -> Option<crate::rules::content::NaturalWonderId> {
        self.tile(tile).snapshot().and_then(|s| s.natural_wonder)
    }
    fn resource(&self, tile: TileId) -> Option<ResourceId> {
        self.tile(tile).snapshot().and_then(|s| s.resource)
    }
    fn improvement(&self, tile: TileId) -> Option<ImprovementId> {
        self.tile(tile).snapshot().and_then(|s| s.improvement)
    }
    fn pillaged(&self, tile: TileId) -> bool {
        self.known_pillaged(tile)
    }
    fn owner(&self, tile: TileId) -> Option<PlayerId> {
        self.tile(tile).snapshot().and_then(|s| s.owner)
    }
    fn city(&self, tile: TileId) -> bool {
        self.tile(tile).snapshot().map(|s| s.city).unwrap_or(false)
    }
    fn river(&self, tile: TileId) -> bool {
        self.tile(tile).snapshot().map(|s| s.river).unwrap_or(false)
    }
    fn lake(&self, tile: TileId) -> bool {
        self.known_geography(tile).map(|g| g.lake).unwrap_or(false)
    }
    fn coastal_land(&self, tile: TileId) -> bool {
        self.known_geography(tile)
            .map(|g| g.coastal_land)
            .unwrap_or(false)
    }
    fn is_water(&self, tile: TileId) -> bool {
        self.known_geography(tile).map(|g| g.water).unwrap_or(false)
    }
    fn is_host(&self, player: PlayerId) -> bool {
        is_host(&self.game_view().auth, player)
    }
}

/// The player's kind hosts other players' improvements.
fn is_host<M: RulesModule>(auth: &AuthWorld<M>, player: PlayerId) -> bool {
    use crate::world::player::ModulePlayerKind;
    auth.players().kind_of(player).module().is_some_and(|kind| kind.hosts_improvements())
}
