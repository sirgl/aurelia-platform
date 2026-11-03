//! Storage units. Mutation only through the transaction API.
//!
//! One cannot change `unit.owner` separately without updating indexes (§8).

use crate::ids::{EntityId, PlayerId, PromotionId, TileId, UnitHandle, UnitKindId};
use crate::module::RulesModule;
use crate::world::module::ModuleUnitOf;

/// Turns to build a road (B3): a mechanic parameter, not content
/// (a road has no bonuses/prices, only passability; a content table is redundant).
pub const ROAD_WORK_TURNS: u16 = 2;

/// Full health of a unit (own scale, 100).
/// Unciv: logic/map/mapunit/MapUnit.kt (health 100), equal.
pub const UNIT_MAX_HP: u16 = 100;

/// Move points per whole movement point (an own scale): a
/// road step costs 30, a flat step 60. Unit moves are kept in these points.
/// own value (no Unciv equivalent: an internal scale).
pub const MOVE_DENOMINATOR: u16 = 60;

#[derive(Debug, Clone, Copy)]
pub struct UnitCore {
    /// Persistent identity (survives save/load; handles do not).
    pub id: EntityId,
    pub tile: TileId,
    pub owner: PlayerId,
    pub kind: UnitKindId,
    pub hp: u16,
    /// Move points left this turn (MOVE_DENOMINATOR per movement point).
    /// Restored at the end of the owner's turn.
    pub moves_left: u16,
    /// Move points per turn (content movement × MOVE_DENOMINATOR after
    /// ModifyMovement; see MOVEMENT_DEFAULT).
    pub moves_max: u16,
    /// Standing order of the unit (fortified, set up, on interception
    /// duty). Moving, attacking and new orders end it.
    pub stance: Stance,
    /// Current construction (one at a time; movement/attack drop it).
    pub work: Option<crate::ids::UnitWork>,
    /// Multi-turn move order (MarchTo): the unit walks on towards this tile
    /// at the start of each of its owner's turns until it arrives, the way
    /// is blocked or an enemy comes into its sight. Any other order of the
    /// unit cancels it.
    pub goal: Option<TileId>,
    /// Promotions gained (AddPromotion). Copy-able bitset, no allocation.
    pub promotions: PromotionSet,
    /// Combat experience, cumulative: a promotion does not spend it
    /// (Classic levels need 10, 30, 60, ... in total).
    pub experience: u32,
    /// Level (1 for a new unit; +1 per chosen promotion).
    pub level: u8,
    /// Attacks made this turn (ranged, melee, air); reset with the moves.
    pub attacks_made: u8,
    /// Interceptions made this turn; reset with the moves.
    pub interceptions_made: u8,
}

/// A fortified unit gets +20 % per turn, up to 2 turns. Unciv: equal
/// (`BattleConstants` fortification bonus 20, `UnitTurnManager` stops at 2).
pub const MAX_FORTIFY_TURNS: u8 = 2;

/// Standing order of a unit (Classic activity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Stance {
    #[default]
    Ready,
    /// Fortified for `turns` turns (1 right after the order, then +1 per
    /// turn up to MAX_FORTIFY_TURNS): `turns × FORTIFICATION_BONUS`.
    Fortified { turns: u8 },
    /// A siege unit set up for a ranged attack this turn.
    SetUp,
    /// An air unit (or AA) waiting to intercept.
    Intercept,
}

impl Stance {
    pub fn fortify_turns(self) -> u8 {
        match self {
            Stance::Fortified { turns } => turns,
            Stance::Ready | Stance::SetUp | Stance::Intercept => 0,
        }
    }

    pub fn is_fortified(self) -> bool {
        matches!(self, Stance::Fortified { .. })
    }
}

impl UnitCore {
    /// Damage taken (Classic stores damage; HP = MAX − damage).
    pub fn damage(&self) -> u16 {
        UNIT_MAX_HP.saturating_sub(self.hp)
    }

    /// The unit holds a standing order that goes on without a new command:
    /// a march (`goal`) or a build (`work`). `CancelOrder` ends it.
    pub fn has_order(&self) -> bool {
        self.goal.is_some() || self.work.is_some()
    }

    /// Spent any move points this turn.
    pub fn has_moved(&self) -> bool {
        self.moves_left < self.moves_max
    }
}

/// Set of promotions; the compiler caps the table at MAX_PROMOTIONS (256).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PromotionSet([u64; 4]);

impl PromotionSet {
    pub fn contains(&self, p: PromotionId) -> bool {
        let (word, bit) = Self::slot(p);
        self.0[word] & bit != 0
    }

    /// Returns false when the promotion was already present.
    pub fn insert(&mut self, p: PromotionId) -> bool {
        let (word, bit) = Self::slot(p);
        let added = self.0[word] & bit == 0;
        self.0[word] |= bit;
        added
    }

    pub fn remove(&mut self, p: PromotionId) {
        let (word, bit) = Self::slot(p);
        self.0[word] &= !bit;
    }

    pub fn is_empty(&self) -> bool {
        self.0 == [0; 4]
    }

    /// Ascending ids (deterministic order for effects, hash and saves).
    /// Empty words are skipped: a unit without promotions costs 4 checks.
    pub fn iter(&self) -> impl Iterator<Item = PromotionId> + '_ {
        let words = self.0.iter().enumerate().filter(|(_, word)| **word != 0);
        words.flat_map(|(w, word)| {
            let word = *word;
            let bits = (0..64u16).filter(move |b| word & (1u64 << b) != 0);
            bits.map(move |b| PromotionId(w as u16 * 64 + b))
        })
    }

    fn slot(p: PromotionId) -> (usize, u64) {
        let index = p.0 as usize;
        assert!(index < 256, "promotion id beyond the compiled cap");
        (index / 64, 1u64 << (index % 64))
    }
}

/// Read-only view for gameplay/AI/render.
#[derive(Debug, Clone, Copy)]
pub struct UnitView<'a> {
    pub handle: UnitHandle,
    pub core: &'a UnitCore,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct UnitSlot<M: RulesModule> {
    core: Option<UnitCore>,
    /// The rules module's state of the unit (`ModuleUnit`): default in a
    /// free slot.
    module: ModuleUnitOf<M>,
    generation: u32,
}

/// Unit slots with generations (runtime handles) plus the live units in
/// entity order. Slot positions depend on history (freed slots are
/// reused); every iteration goes through `order`, so behaviour, hash and
/// save depend only on entity ids, never on slot positions.
#[derive(Debug, Clone, Default)]
pub struct UnitStore<M: RulesModule> {
    slots: Vec<UnitSlot<M>>,
    free: Vec<u32>,
    /// Slots of live units, ascending entity id.
    order: Vec<u32>,
}

impl<M: RulesModule> UnitStore<M> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn get(&self, h: UnitHandle) -> Option<UnitView<'_>> {
        self.slots.get(h.index as usize).and_then(|s| {
            if s.generation == h.generation {
                s.core.as_ref().map(|core| UnitView { handle: h, core })
            } else {
                None
            }
        })
    }

    /// The rules module's state of a unit (None: no such unit).
    pub fn module_of(&self, h: UnitHandle) -> Option<&ModuleUnitOf<M>> {
        let slot = self.slots.get(h.index as usize)?;
        (slot.generation == h.generation && slot.core.is_some()).then_some(&slot.module)
    }

    /// The rules module's state of a unit, for its systems.
    pub fn module_mut(&mut self, h: UnitHandle) -> Option<&mut ModuleUnitOf<M>> {
        let slot = self.slots.get_mut(h.index as usize)?;
        (slot.generation == h.generation && slot.core.is_some()).then_some(&mut slot.module)
    }

    /// Insert a unit whose id is newer than every live unit (ids come from
    /// one ascending counter; decode inserts in id order).
    pub fn insert(&mut self, core: UnitCore) -> UnitHandle {
        debug_assert!(self.last_id().map(|last| last < core.id).unwrap_or(true));
        let handle = match self.free.pop() {
            Some(idx) => {
                let slot = &mut self.slots[idx as usize];
                slot.generation += 1;
                slot.core = Some(core);
                slot.module = ModuleUnitOf::<M>::default();
                UnitHandle {
                    index: idx,
                    generation: slot.generation,
                }
            }
            None => {
                let index = self.slots.len() as u32;
                self.slots.push(UnitSlot {
                    core: Some(core),
                    module: ModuleUnitOf::<M>::default(),
                    generation: 0,
                });
                UnitHandle {
                    index,
                    generation: 0,
                }
            }
        };
        self.order.push(handle.index);
        handle
    }

    pub fn remove(&mut self, h: UnitHandle) -> Option<UnitCore> {
        let id = self.get(h)?.core.id;
        let position = self.position_of(id)?;
        let slot = self.slots.get_mut(h.index as usize)?;
        let core = slot.core.take()?;
        slot.module = ModuleUnitOf::<M>::default();
        slot.generation += 1;
        self.free.push(h.index);
        self.order.remove(position);
        Some(core)
    }

    /// The live unit of persistent id `id` (a command journal names units by id).
    pub fn handle_of(&self, id: EntityId) -> Option<UnitHandle> {
        let index = self.order[self.position_of(id)?];
        Some(UnitHandle { index, generation: self.slots[index as usize].generation })
    }

    pub fn get_mut_core(&mut self, h: UnitHandle) -> Option<&mut UnitCore> {
        let slot = self.slots.get_mut(h.index as usize)?;
        if slot.generation != h.generation {
            return None;
        }
        slot.core.as_mut()
    }

    /// Live units in entity order (read-only).
    pub fn iter_cores(&self) -> impl Iterator<Item = &UnitCore> {
        self.iter_handles().map(|(_, core)| core)
    }

    /// Live units with their handles, ascending entity id: AIObservation,
    /// hash and save all see this one canonical order.
    pub fn iter_handles(&self) -> impl Iterator<Item = (UnitHandle, &UnitCore)> {
        self.order.iter().filter_map(move |&index| {
            let slot = &self.slots[index as usize];
            let core = slot.core.as_ref()?;
            let handle = UnitHandle {
                index,
                generation: slot.generation,
            };
            Some((handle, core))
        })
    }

    fn core_at(&self, index: u32) -> Option<&UnitCore> {
        self.slots.get(index as usize)?.core.as_ref()
    }

    fn last_id(&self) -> Option<EntityId> {
        let index = *self.order.last()?;
        Some(self.core_at(index)?.id)
    }

    fn position_of(&self, id: EntityId) -> Option<usize> {
        let found = self.order.binary_search_by_key(&id, |&index| {
            self.core_at(index).map(|core| core.id).unwrap_or(EntityId(u64::MAX))
        });
        found.ok()
    }
}
