//! Bytes of the economy and society section (`crate::persistence::economy`).

use crate::module::RulesModule;
use crate::persistence::economy::{SavedCityEconomy, SavedEconomy};

use super::{CodecError, Field, ModuleSections, Reader};
use crate::world::module::SavedModule;

/// The module's layout holds the game's speed (`ModuleSections::SPEED_SINCE`).
fn speed_written<M: RulesModule>() -> bool {
    <SavedModule<M> as ModuleSections>::SPEED_SINCE <= <SavedModule<M> as ModuleSections>::CODEC_VERSION
}

impl<M: RulesModule> Field for SavedCityEconomy<M> {
    fn put(&self, out: &mut Vec<u8>) {
        self.focus.put(out);
        self.locked.put(out);
        self.specialists.put(out);
        self.food_kept.put(out);
        self.module.put(out);
        self.process.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            focus: Field::take(r)?,
            locked: Field::take(r)?,
            specialists: Field::take(r)?,
            food_kept: Field::take(r)?,
            module: Field::take(r)?,
            process: Field::take(r)?,
        })
    }
}

impl<M: RulesModule> Field for SavedEconomy<M> {
    fn put(&self, out: &mut Vec<u8>) {
        self.cities.put(out);
        self.players.put(out);
        self.difficulty.put(out);
        if speed_written::<M>() {
            self.speed.put(out);
        }
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            cities: Field::take(r)?,
            players: Field::take(r)?,
            difficulty: Field::take(r)?,
            speed: r.since(<SavedModule<M> as ModuleSections>::SPEED_SINCE)?,
        })
    }
}
