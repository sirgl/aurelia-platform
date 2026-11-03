use std::sync::Arc;

use kernel::command::CommandError;
use kernel::ids::PlayerId;
use kernel::module::RulesModule;
use kernel::rules::CompiledRuleset;
use kernel::transaction::journal::{AppliedCommand, CommandResult};
use kernel::world::state::GameState;
use render::world::projection;
use render::{renderer::Renderer, RenderWorld};

use crate::controller::Controller;
use crate::overlays::Overlays;
use crate::selection::Selection;

/// input -> Command -> simulation.execute() -> Journal -> RenderWorld -> render (§35).
pub struct GameClient<M: RulesModule> {
    pub game: GameState<M>,
    pub rules: Arc<CompiledRuleset<M>>,
    pub renderer: Renderer,
    pub render_world: RenderWorld,
    pub controller: Controller,
    pub overlays: Overlays,
    /// What the player selected (pure UI state, does not touch the simulation).
    pub selection: Selection,
    /// Whose eyes we render with (fog of war): the player the host plays.
    pub observer: PlayerId,
    /// Applied commands (the host's and, through the driver, the AI's) with
    /// their journals, while the host records them for its presentation.
    journal: Option<Vec<AppliedCommand<M>>>,
    /// Commands the game accepted through this client (`CommandJournal::commands_played`).
    pub commands: u64,
}

impl<M: RulesModule> GameClient<M> {
    /// A client that shows the world through `observer`'s eyes.
    pub fn new(game: GameState<M>, rules: Arc<CompiledRuleset<M>>, renderer: Renderer, observer: PlayerId) -> Self {
        let render_world = projection::rebuild_render_world(game.view(), observer, &rules);
        Self {
            game,
            rules,
            renderer,
            render_world,
            controller: Controller,
            overlays: Overlays::default(),
            selection: Selection::default(),
            observer,
            journal: None,
            commands: 0,
        }
    }

    /// Look through another player's eyes (hot seat, spectator, tests).
    pub fn set_observer(&mut self, observer: PlayerId) {
        self.observer = observer;
        self.selection = Selection::default();
        self.refresh_projection();
    }

    pub fn submit(&mut self, command: M::Command) -> Result<(), CommandError> {
        let result = self.game.execute(&self.rules, command.clone())?;
        self.commands += 1;
        projection::apply_changes(
            &mut self.render_world,
            self.game.view(),
            result.changes(),
            &self.rules,
        );
        self.keep(command, result);
        Ok(())
    }

    /// Keep every applied command with its journal until `take_journal`.
    pub fn record_journal(&mut self) {
        self.journal.get_or_insert_with(Vec::new);
    }

    pub fn records_journal(&self) -> bool {
        self.journal.is_some()
    }

    /// The commands applied since the last `take_journal`, in order, without
    /// taking them (empty when no journal is kept).
    pub fn journal(&self) -> &[AppliedCommand<M>] {
        self.journal.as_deref().unwrap_or(&[])
    }

    /// The commands applied since the last call, in order.
    pub fn take_journal(&mut self) -> Vec<AppliedCommand<M>> {
        match self.journal.as_mut() {
            Some(journal) => std::mem::take(journal),
            None => Vec::new(),
        }
    }

    /// Commands applied elsewhere (the AI driver) join the journal in order.
    pub fn extend_journal(&mut self, applied: Vec<AppliedCommand<M>>) {
        if let Some(journal) = self.journal.as_mut() {
            journal.extend(applied);
        }
    }

    fn keep(&mut self, command: M::Command, result: CommandResult<M>) {
        if let Some(journal) = self.journal.as_mut() {
            journal.push(AppliedCommand {
                command,
                changes: result.into_journal(),
            });
        }
    }

    /// A quiet command: the projection is NOT updated (accumulated until refresh).
    /// Interactive orders go through submit (immediate redraw).
    pub fn submit_quiet(&mut self, command: M::Command) -> Result<(), CommandError> {
        let result = self.game.execute(&self.rules, command.clone())?;
        self.commands += 1;
        self.keep(command, result);
        Ok(())
    }

    /// One projection refresh after a batch of quiet commands: rebuilds and bumps only
    /// the layers that changed, so the renderer re-uploads only what differs.
    pub fn refresh_projection(&mut self) {
        projection::refresh(
            &mut self.render_world,
            self.game.view(),
            self.observer,
            &self.rules,
        );
    }

    /// Replacing the whole game (new game / load): the projection and selection
    /// are rebuilt, renderer/rules/observer remain (textures are not loaded).
    pub fn replace_game(&mut self, game: GameState<M>) {
        self.game = game;
        // Records of the old game name entities that no longer exist.
        if let Some(journal) = self.journal.as_mut() {
            journal.clear();
        }
        self.selection = Selection::default();
        self.refresh_projection();
    }
}
