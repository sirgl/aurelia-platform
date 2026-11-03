//! The sandbox's commands: the kernel's orders (move, found, produce, end the
//! turn, ...) and the one order of its own, a melee attack (`combat`).
//! Validation is read-only; a rejected command leaves the world unchanged.

use kernel::command::{validate_kernel, KernelCommand};
use kernel::derived::DerivedWorld;
use kernel::ids::{TileId, UnitHandle};
use kernel::module::Rejected;
use kernel::navigation::PathScratch;
use kernel::transaction::journal::ChangeJournal;
use kernel::transaction::WorldTransaction;
use kernel::world::state::{AuthWorld, GameView};

use crate::{combat, Rules, Sandbox};

/// Everything a sandbox player may order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Kernel(KernelCommand),
    /// A melee attack from `unit` on the adjacent tile `target` (its units,
    /// else its city).
    Attack { unit: UnitHandle, target: TileId },
}

// The journal's layout of the sandbox's commands (`kernel::replay::codec`).
kernel::command_field!(Command {
    0 => Kernel(order),
    1 => Attack { unit, target },
});

impl kernel::replay::codec::CommandCodec for Sandbox {
    const COMMAND_CODEC_VERSION: u32 = 1;
}

/// Validate and apply one command (`RulesModule::execute`).
pub fn execute(
    world: &mut AuthWorld<Sandbox>,
    derived: &DerivedWorld<Sandbox>,
    rules: &Rules,
    command: Command,
    scratch: &mut PathScratch,
) -> Result<ChangeJournal<Sandbox>, Rejected<Sandbox>> {
    let rejected = |error| Rejected { error, partial: ChangeJournal::default() };
    let tx = match command {
        Command::Kernel(order) => {
            let view = GameView { auth: &*world, derived };
            let resolved = validate_kernel(view, rules, order, scratch).map_err(rejected)?;
            let mut tx = WorldTransaction::new(world);
            if let Err(error) = tx.apply_kernel(&resolved, rules, scratch, derived) {
                return Err(Rejected { error, partial: tx.journal });
            }
            tx
        }
        Command::Attack { unit, target } => {
            let battle = combat::plan(world, rules, unit, target).map_err(rejected)?;
            let mut tx = WorldTransaction::new(world);
            if let Err(error) = combat::fight(&mut tx, rules, battle) {
                return Err(Rejected { error, partial: tx.journal });
            }
            tx
        }
    };
    // The commit of a game command: eliminations, and the turn passes on
    // when the active player was eliminated.
    Ok(tx.commit_command(rules))
}
