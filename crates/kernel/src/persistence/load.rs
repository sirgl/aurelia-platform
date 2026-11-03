//! A save file to a game in one call: the byte codec (a range of codec
//! versions), then the decode with the rules it accepts. Module UIs load through
//! it; the host picks what it accepts (`client::shell::saves::rules_match`).

use crate::module::RulesModule;
use crate::persistence::ai::SavedAiStates;
use crate::persistence::codec::{self, CodecError};
use crate::persistence::decode::{decode_with, DecodeError, RulesCheck, RulesMatch};
use crate::rules::CompiledRuleset;
use crate::world::state::GameState;

/// A loaded save: the game, the AI players' memory and the rules it was
/// saved with.
#[derive(Debug)]
pub struct Loaded<M: RulesModule> {
    pub game: GameState<M>,
    pub ai: SavedAiStates,
    pub rules: RulesCheck,
}

/// Why a save does not load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The bytes are not a save of a readable codec version.
    Codec(CodecError),
    /// The save's versions, rules or content do not load.
    Decode(DecodeError),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Codec(e) => e.fmt(f),
            Self::Decode(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for LoadError {}

pub fn load<M: RulesModule>(bytes: &[u8], rules: &CompiledRuleset<M>, accept: RulesMatch) -> Result<Loaded<M>, LoadError> {
    let save = codec::from_bytes::<M>(bytes).map_err(LoadError::Codec)?;
    let (game, rules) = decode_with(&save, rules, accept).map_err(LoadError::Decode)?;
    Ok(Loaded { game, ai: save.ai, rules })
}
