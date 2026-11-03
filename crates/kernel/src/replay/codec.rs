//! Commands as bytes, for the command journal (platform/docs/saves.md §3).
//!
//! A command is the tag of its variant (u8) and its fields in order. Units
//! and cities are written as their entity ids, not their handles: a handle
//! depends on the history of its store (freed slots are reused, a loaded
//! game numbers them anew, §17), an id does not. Reading looks the id up in
//! the world the command is replayed in, so a command reads correctly only
//! at its place in the journal. Content ids are written as they are: a
//! journal replays only on the rules it was recorded with.
//!
//! A rules module declares the layout of its command type with
//! [`command_field!`](crate::command_field) and marks itself
//! [`CommandCodec`].
//!
//! The same declaration gives the command as a tree of named values,
//! [`Value`] (`CommandField::value`, `from_value`): the bytes' layout with
//! the names of the variant and its fields, what the AI's datasets and
//! tools read and write.

use crate::ids::{
    BuildingKindId, CityHandle, EntityId, ImprovementId, PlayerId, ProductionItem, PromotionId, ResourceId, TechId,
    TileId, UnitHandle, UnitKindId,
};
use crate::module::RulesModule;
use crate::persistence::codec::{CodecError, Field, Reader};
use crate::rules::content::{ProjectId, SpecialistId};
use crate::world::citizens::CityFocus;
use crate::world::city_economy::Process;
use crate::world::state::AuthWorld;

/// One value of a command, written and read symmetrically. `world` is the
/// world before the command: it turns handles into ids and back.
pub trait CommandField: Sized {
    fn put<M: RulesModule>(&self, world: &AuthWorld<M>, out: &mut Vec<u8>);
    fn take<M: RulesModule>(r: &mut Reader<'_>, world: &AuthWorld<M>) -> Result<Self, CodecError>;
    /// The value as a tree: the layout of `put` with names.
    fn value<M: RulesModule>(&self, world: &AuthWorld<M>) -> Value;
    /// The value a tree names (`value` backwards): an unknown variant, a
    /// missing field, a wrong kind of value or an unknown entity is an error.
    fn from_value<M: RulesModule>(value: &Value, world: &AuthWorld<M>) -> Result<Self, CodecError>;
}

/// A command as a tree of plain values: the layout of its bytes with the
/// names of its variant and fields (what the AI's datasets and tools read
/// and write). Handles are entity ids, a gone handle is `Null`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Uint(u64),
    List(Vec<Value>),
    Record { name: &'static str, fields: Vec<(&'static str, Value)> },
}

impl Value {
    /// The field of a record by its name; None for another value or a name
    /// the record has not.
    pub fn field(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Record { fields, .. } => fields.iter().find(|(n, _)| *n == name).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// A rules module whose commands go into a command journal.
pub trait CommandCodec: RulesModule<Command: CommandField> {
    /// The layout of the module's commands: a change of any command's
    /// fields or tags bumps it, and a journal of another layout does not
    /// replay.
    const COMMAND_CODEC_VERSION: u32;

    /// The name of the variant of the module's rules that `rules` are, written
    /// in the journal's header so that a reader of the journal can build the
    /// rules before it replays (Lands & Laws: its mode). Empty: the module has
    /// one set of rules.
    fn rules_label(_rules: &crate::rules::CompiledRuleset<Self>) -> String {
        String::new()
    }
}

/// The layout of an enum as a command field: each variant by its tag, its
/// fields in the listed order. Struct variants list their fields
/// (`0 => MoveUnit { unit, destination }`), tuple variants name theirs
/// (`0 => Kernel(order)`), unit variants have none (`3 => Local {}`). The
/// tags are the file's: a variant keeps its tag for good. The tree form is
/// a `Record` named after the variant with the fields under their names.
#[macro_export]
macro_rules! command_field {
    ($ty:ident { $($tag:literal => $variant:ident $fields:tt),* $(,)? }) => {
        impl $crate::replay::codec::CommandField for $ty {
            fn put<M: $crate::module::RulesModule>(
                &self,
                world: &$crate::world::state::AuthWorld<M>,
                out: &mut Vec<u8>,
            ) {
                let _ = world;
                match self {
                    $($crate::command_field!(@pattern $ty $variant $fields) => {
                        out.push($tag);
                        $crate::command_field!(@put world out $fields);
                    })*
                }
            }

            fn take<M: $crate::module::RulesModule>(
                r: &mut $crate::persistence::codec::Reader<'_>,
                world: &$crate::world::state::AuthWorld<M>,
            ) -> Result<Self, $crate::persistence::codec::CodecError> {
                let _ = world;
                match <u8 as $crate::persistence::codec::Field>::take(r)? {
                    $($tag => Ok($crate::command_field!(@build r world $ty $variant $fields)),)*
                    _ => Err($crate::persistence::codec::CodecError::Invalid(concat!("tag of ", stringify!($ty)))),
                }
            }

            fn value<M: $crate::module::RulesModule>(
                &self,
                world: &$crate::world::state::AuthWorld<M>,
            ) -> $crate::replay::codec::Value {
                let _ = world;
                match self {
                    $($crate::command_field!(@pattern $ty $variant $fields) => $crate::replay::codec::Value::Record {
                        name: stringify!($variant),
                        fields: $crate::command_field!(@value world $fields),
                    },)*
                }
            }

            fn from_value<M: $crate::module::RulesModule>(
                value: &$crate::replay::codec::Value,
                world: &$crate::world::state::AuthWorld<M>,
            ) -> Result<Self, $crate::persistence::codec::CodecError> {
                let _ = world;
                let $crate::replay::codec::Value::Record { name, .. } = value else {
                    return Err($crate::persistence::codec::CodecError::Invalid(concat!("record of ", stringify!($ty))));
                };
                match *name {
                    $(stringify!($variant) => Ok($crate::command_field!(@from_value value world $ty $variant $fields)),)*
                    _ => Err($crate::persistence::codec::CodecError::Invalid(concat!("variant of ", stringify!($ty)))),
                }
            }
        }
    };
    (@pattern $ty:ident $variant:ident { $($field:ident),* }) => { $ty::$variant { $($field),* } };
    (@pattern $ty:ident $variant:ident ( $($field:ident),* )) => { $ty::$variant ( $($field),* ) };
    (@put $world:ident $out:ident { $($field:ident),* }) => {
        $($crate::replay::codec::CommandField::put($field, $world, $out);)*
    };
    (@put $world:ident $out:ident ( $($field:ident),* )) => {
        $($crate::replay::codec::CommandField::put($field, $world, $out);)*
    };
    (@build $r:ident $world:ident $ty:ident $variant:ident { $($field:ident),* }) => {
        $ty::$variant { $($field: $crate::replay::codec::CommandField::take($r, $world)?),* }
    };
    (@build $r:ident $world:ident $ty:ident $variant:ident ( $($field:ident),* )) => {
        $ty::$variant ( $($crate::command_field!(@take $r $world $field)),* )
    };
    (@take $r:ident $world:ident $field:ident) => { $crate::replay::codec::CommandField::take($r, $world)? };
    (@value $world:ident { $($field:ident),* }) => {
        vec![$((stringify!($field), $crate::replay::codec::CommandField::value($field, $world))),*]
    };
    (@value $world:ident ( $($field:ident),* )) => {
        vec![$((stringify!($field), $crate::replay::codec::CommandField::value($field, $world))),*]
    };
    (@from_value $value:ident $world:ident $ty:ident $variant:ident { $($field:ident),* }) => {
        $ty::$variant { $($field: $crate::command_field!(@field $value $world $field)),* }
    };
    (@from_value $value:ident $world:ident $ty:ident $variant:ident ( $($field:ident),* )) => {
        $ty::$variant ( $($crate::command_field!(@field $value $world $field)),* )
    };
    (@field $value:ident $world:ident $field:ident) => {
        $crate::replay::codec::CommandField::from_value(
            $value
                .field(stringify!($field))
                .ok_or($crate::persistence::codec::CodecError::Invalid(concat!("missing field ", stringify!($field))))?,
            $world,
        )?
    };
}

/// Values written as the save's fields; in the tree, the `Value` variant
/// of their kind.
macro_rules! plain_fields {
    ($($t:ty => $kind:ident),*) => {$(
        impl CommandField for $t {
            fn put<M: RulesModule>(&self, _: &AuthWorld<M>, out: &mut Vec<u8>) {
                Field::put(self, out);
            }
            fn take<M: RulesModule>(r: &mut Reader<'_>, _: &AuthWorld<M>) -> Result<Self, CodecError> {
                Field::take(r)
            }
            fn value<M: RulesModule>(&self, _: &AuthWorld<M>) -> Value {
                Value::$kind((*self).into())
            }
            fn from_value<M: RulesModule>(value: &Value, _: &AuthWorld<M>) -> Result<Self, CodecError> {
                match value {
                    Value::$kind(v) => (*v).try_into().map_err(|_| CodecError::Invalid(concat!("range of ", stringify!($t)))),
                    _ => Err(CodecError::Invalid(concat!("not a ", stringify!($t)))),
                }
            }
        }
    )*};
}
plain_fields!(u8 => Uint, u16 => Uint, u32 => Uint, u64 => Uint, i32 => Int, i64 => Int, bool => Bool);

/// Command fields of ids that are a number (players, tiles, content rows):
/// written as the number, `Uint` in the tree.
#[macro_export]
macro_rules! command_ids {
    ($($id:ident),* $(,)?) => {$(
        impl $crate::replay::codec::CommandField for $id {
            fn put<M: $crate::module::RulesModule>(
                &self,
                _: &$crate::world::state::AuthWorld<M>,
                out: &mut Vec<u8>,
            ) {
                $crate::persistence::codec::Field::put(&self.0, out);
            }
            fn take<M: $crate::module::RulesModule>(
                r: &mut $crate::persistence::codec::Reader<'_>,
                _: &$crate::world::state::AuthWorld<M>,
            ) -> Result<Self, $crate::persistence::codec::CodecError> {
                Ok($id($crate::persistence::codec::Field::take(r)?))
            }
            fn value<M: $crate::module::RulesModule>(
                &self,
                _: &$crate::world::state::AuthWorld<M>,
            ) -> $crate::replay::codec::Value {
                $crate::replay::codec::Value::Uint(u64::from(self.0))
            }
            fn from_value<M: $crate::module::RulesModule>(
                value: &$crate::replay::codec::Value,
                _: &$crate::world::state::AuthWorld<M>,
            ) -> Result<Self, $crate::persistence::codec::CodecError> {
                match value {
                    $crate::replay::codec::Value::Uint(v) => (*v)
                        .try_into()
                        .map($id)
                        .map_err(|_| $crate::persistence::codec::CodecError::Invalid(concat!("range of ", stringify!($id)))),
                    _ => Err($crate::persistence::codec::CodecError::Invalid(concat!("not a ", stringify!($id)))),
                }
            }
        }
    )*};
}
crate::command_ids!(
    PlayerId, TileId, TechId, UnitKindId, BuildingKindId, ImprovementId, ResourceId, PromotionId, ProjectId,
    SpecialistId,
);

impl<T: CommandField> CommandField for Option<T> {
    fn put<M: RulesModule>(&self, world: &AuthWorld<M>, out: &mut Vec<u8>) {
        match self {
            None => out.push(0),
            Some(value) => {
                out.push(1);
                value.put(world, out);
            }
        }
    }
    fn take<M: RulesModule>(r: &mut Reader<'_>, world: &AuthWorld<M>) -> Result<Self, CodecError> {
        match <u8 as Field>::take(r)? {
            0 => Ok(None),
            1 => Ok(Some(T::take(r, world)?)),
            _ => Err(CodecError::Invalid("option tag")),
        }
    }
    fn value<M: RulesModule>(&self, world: &AuthWorld<M>) -> Value {
        self.as_ref().map_or(Value::Null, |value| value.value(world))
    }
    fn from_value<M: RulesModule>(value: &Value, world: &AuthWorld<M>) -> Result<Self, CodecError> {
        match value {
            Value::Null => Ok(None),
            _ => Ok(Some(T::from_value(value, world)?)),
        }
    }
}

impl<T: CommandField> CommandField for Vec<T> {
    fn put<M: RulesModule>(&self, world: &AuthWorld<M>, out: &mut Vec<u8>) {
        Field::put(&(self.len() as u32), out);
        for item in self {
            item.put(world, out);
        }
    }
    fn take<M: RulesModule>(r: &mut Reader<'_>, world: &AuthWorld<M>) -> Result<Self, CodecError> {
        let len = <u32 as Field>::take(r)?;
        (0..len).map(|_| T::take(r, world)).collect()
    }
    fn value<M: RulesModule>(&self, world: &AuthWorld<M>) -> Value {
        Value::List(self.iter().map(|item| item.value(world)).collect())
    }
    fn from_value<M: RulesModule>(value: &Value, world: &AuthWorld<M>) -> Result<Self, CodecError> {
        match value {
            Value::List(items) => items.iter().map(|item| T::from_value(item, world)).collect(),
            _ => Err(CodecError::Invalid("not a list")),
        }
    }
}

impl<A: CommandField, B: CommandField> CommandField for (A, B) {
    fn put<M: RulesModule>(&self, world: &AuthWorld<M>, out: &mut Vec<u8>) {
        self.0.put(world, out);
        self.1.put(world, out);
    }
    fn take<M: RulesModule>(r: &mut Reader<'_>, world: &AuthWorld<M>) -> Result<Self, CodecError> {
        Ok((A::take(r, world)?, B::take(r, world)?))
    }
    fn value<M: RulesModule>(&self, world: &AuthWorld<M>) -> Value {
        Value::List(vec![self.0.value(world), self.1.value(world)])
    }
    fn from_value<M: RulesModule>(value: &Value, world: &AuthWorld<M>) -> Result<Self, CodecError> {
        match value {
            Value::List(items) if items.len() == 2 => Ok((A::from_value(&items[0], world)?, B::from_value(&items[1], world)?)),
            _ => Err(CodecError::Invalid("not a pair")),
        }
    }
}

/// An id no live entity has: what a handle of a unit or city that is gone
/// is written as (it reads back as unknown).
const GONE: EntityId = EntityId(u64::MAX);

impl CommandField for UnitHandle {
    fn put<M: RulesModule>(&self, world: &AuthWorld<M>, out: &mut Vec<u8>) {
        let id = world.units().get(*self).map_or(GONE, |unit| unit.core.id);
        Field::put(&id.0, out);
    }
    fn take<M: RulesModule>(r: &mut Reader<'_>, world: &AuthWorld<M>) -> Result<Self, CodecError> {
        let id = EntityId(Field::take(r)?);
        world.units().handle_of(id).ok_or(CodecError::Invalid("unknown unit"))
    }
    fn value<M: RulesModule>(&self, world: &AuthWorld<M>) -> Value {
        world.units().get(*self).map_or(Value::Null, |unit| Value::Uint(unit.core.id.0))
    }
    fn from_value<M: RulesModule>(value: &Value, world: &AuthWorld<M>) -> Result<Self, CodecError> {
        match value {
            Value::Uint(id) => world.units().handle_of(EntityId(*id)).ok_or(CodecError::Invalid("unknown unit")),
            Value::Null => Err(CodecError::Invalid("unknown unit")),
            _ => Err(CodecError::Invalid("not a unit id")),
        }
    }
}

impl CommandField for CityHandle {
    fn put<M: RulesModule>(&self, world: &AuthWorld<M>, out: &mut Vec<u8>) {
        let id = world.cities().get(*self).map_or(GONE, |city| city.id);
        Field::put(&id.0, out);
    }
    fn take<M: RulesModule>(r: &mut Reader<'_>, world: &AuthWorld<M>) -> Result<Self, CodecError> {
        let id = EntityId(Field::take(r)?);
        world.cities().handle_of(id).ok_or(CodecError::Invalid("unknown city"))
    }
    fn value<M: RulesModule>(&self, world: &AuthWorld<M>) -> Value {
        world.cities().get(*self).map_or(Value::Null, |city| Value::Uint(city.id.0))
    }
    fn from_value<M: RulesModule>(value: &Value, world: &AuthWorld<M>) -> Result<Self, CodecError> {
        match value {
            Value::Uint(id) => world.cities().handle_of(EntityId(*id)).ok_or(CodecError::Invalid("unknown city")),
            Value::Null => Err(CodecError::Invalid("unknown city")),
            _ => Err(CodecError::Invalid("not a city id")),
        }
    }
}

impl CommandField for CityFocus {
    fn put<M: RulesModule>(&self, _: &AuthWorld<M>, out: &mut Vec<u8>) {
        out.push(self.code());
    }
    fn take<M: RulesModule>(r: &mut Reader<'_>, _: &AuthWorld<M>) -> Result<Self, CodecError> {
        CityFocus::from_code(Field::take(r)?).ok_or(CodecError::Invalid("city focus"))
    }
    fn value<M: RulesModule>(&self, _: &AuthWorld<M>) -> Value {
        Value::Uint(u64::from(self.code()))
    }
    fn from_value<M: RulesModule>(value: &Value, world: &AuthWorld<M>) -> Result<Self, CodecError> {
        CityFocus::from_code(u8::from_value(value, world)?).ok_or(CodecError::Invalid("city focus"))
    }
}

crate::command_field!(ProductionItem {
    0 => Unit(kind),
    1 => Building(kind),
    2 => Project(project),
});

crate::command_field!(Process {
    0 => Wealth {},
    1 => Research {},
    2 => Contribute(project),
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::KernelCommand;
    use crate::rules::compile::compile_rules;
    use crate::rules::source::SourceRules;
    use crate::rules::CompiledRuleset;
    use crate::test_module::{Order, TestModule};
    use crate::world::state::{GameConfig, GameState};

    const P0: PlayerId = PlayerId(0);

    fn rules() -> CompiledRuleset<TestModule> {
        compile_rules(SourceRules::default()).expect("empty rules compile")
    }

    /// A game with one unit and one city of player 0.
    fn game(rules: &CompiledRuleset<TestModule>) -> (GameState<TestModule>, UnitHandle, CityHandle) {
        let mut game = GameState::new(rules, GameConfig { players: 2, width: 12, height: 10, rng_seed: 7 });
        let unit = game.debug_spawn_unit(rules, P0, TileId(13), UnitKindId(0));
        let city = game.debug_found_city(rules, P0, TileId(50));
        (game, unit, city)
    }

    fn bytes<T: CommandField>(field: &T, world: &AuthWorld<TestModule>) -> Vec<u8> {
        let mut out = Vec::new();
        field.put(world, &mut out);
        out
    }

    /// The field through the tree and back has the same bytes.
    fn round_trips<T: CommandField>(field: &T, world: &AuthWorld<TestModule>) {
        let value = field.value(world);
        let back = T::from_value(&value, world).unwrap_or_else(|e| panic!("{value:?}: {e}"));
        assert_eq!(bytes(&back, world), bytes(field, world), "{value:?}");
    }

    #[test]
    fn a_command_is_a_record_of_its_variant_and_fields() {
        let rules = rules();
        let (game, unit, city) = game(&rules);
        let auth = game.view().auth;
        let id = auth.units().get(unit).expect("spawned").core.id.0;
        let order = Order::Kernel(KernelCommand::MoveUnit { unit, destination: TileId(14) });
        assert_eq!(
            order.value(auth),
            Value::Record {
                name: "Kernel",
                fields: vec![(
                    "order",
                    Value::Record {
                        name: "MoveUnit",
                        fields: vec![("unit", Value::Uint(id)), ("destination", Value::Uint(14))],
                    },
                )],
            }
        );
        assert_eq!(Order::Pass.value(auth), Value::Record { name: "Pass", fields: Vec::new() });
        let city_id = auth.cities().get(city).expect("founded").id.0;
        let enqueue = KernelCommand::EnqueueProduction { player: P0, city, item: ProductionItem::Unit(UnitKindId(3)) };
        assert_eq!(
            enqueue.value(auth),
            Value::Record {
                name: "EnqueueProduction",
                fields: vec![
                    ("player", Value::Uint(0)),
                    ("city", Value::Uint(city_id)),
                    ("item", Value::Record { name: "Unit", fields: vec![("kind", Value::Uint(3))] }),
                ],
            }
        );
    }

    #[test]
    fn every_order_and_field_kind_reads_back_from_its_tree() {
        let rules = rules();
        let (game, unit, city) = game(&rules);
        let auth = game.view().auth;
        use KernelCommand as K;
        let orders = [
            Order::Pass,
            Order::Fight { unit, damage: 7 },
            Order::Kernel(K::MoveUnit { unit, destination: TileId(14) }),
            Order::Kernel(K::CancelOrder { unit }),
            Order::Kernel(K::EnqueueProduction { player: P0, city, item: ProductionItem::Project(ProjectId(2)) }),
            Order::Kernel(K::SetCityFocus { player: P0, city, focus: CityFocus::Science }),
            Order::Kernel(K::SetSpecialists { player: P0, city, specialist: SpecialistId(1), count: 2 }),
            Order::Kernel(K::SetAvoidGrowth { player: P0, city, avoid: true }),
            Order::Kernel(K::ChooseCityName { player: P0, city, name: 300 }),
            Order::Kernel(K::EndTurn { player: P0 }),
        ];
        for order in &orders {
            round_trips(order, auth);
        }
        round_trips(&Some(unit), auth);
        round_trips(&None::<CityHandle>, auth);
        round_trips(&vec![(P0, Process::Wealth), (PlayerId(1), Process::Contribute(ProjectId(1)))], auth);
        round_trips(&(-5i32, 9u64), auth);
        assert_eq!(Some(TileId(3)).value(auth), Value::Uint(3));
        assert_eq!(None::<TileId>.value(auth), Value::Null);
        assert_eq!((-5i32, true).value(auth), Value::List(vec![Value::Int(-5), Value::Bool(true)]));
    }

    #[test]
    fn a_wrong_tree_is_an_error() {
        let rules = rules();
        let (mut game, unit, _) = game(&rules);
        let gone = game.debug_spawn_unit(&rules, P0, TileId(15), UnitKindId(0));
        game.execute(&rules, Order::Fight { unit: gone, damage: u16::MAX }).expect("the unit dies");
        let auth = game.view().auth;
        let invalid = |value: &Value| Order::from_value(value, auth).expect_err("refused");
        let unit_id = auth.units().get(unit).expect("spawned").core.id.0;
        let record = |name, fields| Value::Record { name, fields };
        let kernel = |order| record("Kernel", vec![("order", order)]);
        assert_eq!(invalid(&record("Nothing", Vec::new())), CodecError::Invalid("variant of Order"));
        assert_eq!(invalid(&kernel(record("Nothing", Vec::new()))), CodecError::Invalid("variant of KernelCommand"));
        assert_eq!(invalid(&record("Fight", vec![("unit", Value::Uint(unit_id))])), CodecError::Invalid("missing field damage"));
        let fight = |unit, damage| record("Fight", vec![("unit", unit), ("damage", damage)]);
        assert_eq!(invalid(&fight(Value::Uint(unit_id), Value::Int(3))), CodecError::Invalid("not a u16"));
        assert_eq!(invalid(&fight(Value::Uint(unit_id), Value::Uint(70_000))), CodecError::Invalid("range of u16"));
        assert_eq!(invalid(&fight(Value::Uint(999), Value::Uint(3))), CodecError::Invalid("unknown unit"));
        assert_eq!(invalid(&fight(Value::Null, Value::Uint(3))), CodecError::Invalid("unknown unit"));
        assert_eq!(invalid(&Value::Uint(1)), CodecError::Invalid("record of Order"));
        assert_eq!(Order::Fight { unit: gone, damage: 1 }.value(auth), fight(Value::Null, Value::Uint(1)));
    }
}
