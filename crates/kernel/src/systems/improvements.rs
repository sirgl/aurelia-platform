//! Worker jobs (Classic builds): improvements, roads, feature removal.
//! Orders set the job; the end-turn step ticks every job of the player and
//! completes the ready ones. Journal: TileChange::ImprovementBuilt /
//! RoadBuilt / FeatureChopped. Where a build may happen: `world::builds`.

use crate::command::{CommandError, Refusal};
use crate::ids::{FeatureId, ImprovementId, PlayerId, TileId, UnitHandle, UnitWork, WorkTarget};
use crate::module::RulesModule;
use crate::rules::content::Domain;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::TileChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::builds::{chop_production, improvement_site, removal_of, work_rate, BASE_WORK_RATE};
use crate::world::state::AuthWorld;

/// Who builds: a worker (land builds; a work boat the water ones), or a
/// combat unit with builds of its own (Classic: the Legion's roads and forts,
/// the Samurai's fishing boats).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Builder<'r> {
    Worker(Domain),
    Combat { roads: bool, improvements: &'r [ImprovementId] },
}

/// A unit that builds, with no job: its owner, tile and what it builds.
pub(crate) fn idle_builder<'r, M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &'r CompiledRuleset<M>,
    unit: UnitHandle,
) -> Result<(PlayerId, TileId, Builder<'r>), CommandError> {
    use crate::rules::rows::UnitExtension;
    let view = auth.units().get(unit).ok_or(CommandError::UnknownUnit)?;
    let builder = match rules.units.get(view.core.kind.0 as usize) {
        Some(d) if d.builder => Builder::Worker(d.domain),
        Some(d) if d.module.builds_roads() || !d.module.builds().is_empty() => {
            Builder::Combat { roads: d.module.builds_roads(), improvements: d.module.builds() }
        }
        _ => return Err(CommandError::RuleViolation("not a worker".into())),
    };
    if view.core.work.is_some() {
        return Err(CommandError::Refused(Refusal::AlreadyWorking));
    }
    Ok((view.core.owner, view.core.tile, builder))
}

fn violation(reason: &'static str) -> CommandError {
    CommandError::RuleViolation(reason.into())
}

/// Work of a job at the game speed (`SpeedValues::improvement_percent`):
/// the Standard work time × the percent, rounded to whole worker-turns, at
/// least one. Unciv: TileImprovement.getTurnsToBuild
/// (`improvementBuildLengthModifier`, `roundToInt`), equal at the base rate;
/// roads, repairs and feature removals are improvements there too.
pub fn speed_work_time(work_time: u32, percent: u32) -> u32 {
    if percent == 100 {
        return work_time;
    }
    let scaled = u64::from(work_time) * u64::from(percent) / 100;
    let whole = (scaled + u64::from(BASE_WORK_RATE) / 2) / u64::from(BASE_WORK_RATE) * u64::from(BASE_WORK_RATE);
    whole.clamp(u64::from(BASE_WORK_RATE), u64::from(u32::MAX)) as u32
}

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    /// Commit safety net: the site is checked again (another worker may have
    /// built or chopped meanwhile). A water improvement is placed at once
    /// and uses the work boat up.
    pub(crate) fn start_improvement(
        &mut self,
        unit: UnitHandle,
        improvement: ImprovementId,
        tile: TileId,
        work_time: u32,
        rules: &CompiledRuleset<M>,
    ) -> Result<(), CommandError> {
        let (owner, _, builder) = idle_builder(self.world, rules, unit)?;
        improvement_site(self.world, rules, owner, tile, improvement).map_err(violation)?;
        let water = rules
            .improvements
            .get(improvement.0 as usize)
            .map(|d| d.placement.water)
            .unwrap_or(false);
        if water {
            self.place_improvement(owner, tile, improvement, rules);
            // A work boat is used up; a combat unit that builds it is not.
            if matches!(builder, Builder::Worker(_)) {
                self.destroy_unit(unit)?;
            }
            return Ok(());
        }
        self.assign_work(unit, tile, WorkTarget::Improvement(improvement), work_time, rules)
    }

    /// The commit safety net mirrors validate (occupancy/road).
    pub(crate) fn start_road(
        &mut self,
        unit: UnitHandle,
        tile: TileId,
        rules: &CompiledRuleset<M>,
    ) -> Result<(), CommandError> {
        if self.world.map.road(tile) {
            return Err(violation("already road"));
        }
        let work = u32::from(crate::world::unit::ROAD_WORK_TURNS) * BASE_WORK_RATE;
        self.assign_work(unit, tile, WorkTarget::Road, work, rules)
    }

    /// Commit safety net: the feature may be gone (another worker).
    pub(crate) fn start_chop(
        &mut self,
        unit: UnitHandle,
        tile: TileId,
        feature: FeatureId,
        work_time: u32,
        rules: &CompiledRuleset<M>,
    ) -> Result<(), CommandError> {
        if self.world.map.feature(tile).flatten() != Some(feature) {
            return Err(violation("no feature"));
        }
        self.assign_work(unit, tile, WorkTarget::Chop(feature), work_time, rules)
    }

    fn assign_work(
        &mut self,
        unit: UnitHandle,
        tile: TileId,
        target: WorkTarget,
        work_time: u32,
        rules: &CompiledRuleset<M>,
    ) -> Result<(), CommandError> {
        let work_time = speed_work_time(work_time, self.world.settings().speed(rules).improvement_percent);
        let core = self
            .world
            .units
            .get_mut_core(unit)
            .ok_or(CommandError::UnknownUnit)?;
        if core.work.is_some() {
            return Err(CommandError::Refused(Refusal::AlreadyWorking));
        }
        // At least one turn of work: a build is never instant.
        let work_left = work_time.clamp(1, u32::from(u16::MAX)) as u16;
        core.work = Some(UnitWork {
            tile,
            target,
            work_left,
        });
        core.goal = None;
        Ok(())
    }

    /// A player's constructions per turn: each job advances at the worker's rate
    /// (`work_rate`); a finished one completes (target onto the map + journal;
    /// the worker is free for the next).
    pub(crate) fn work_progress(&mut self, owner: PlayerId, rules: &CompiledRuleset<M>) {
        let handles: Vec<UnitHandle> = self.world.indexes.units_of(owner).to_vec();
        let rate = work_rate(self.world, rules, owner);
        for h in handles {
            let job = match self.world.units.get(h) {
                Some(view) => view.core.work,
                None => continue,
            };
            let Some(job) = job else { continue };
            let next = if u32::from(job.work_left) > rate {
                Some(UnitWork {
                    work_left: job.work_left - rate as u16,
                    ..job
                })
            } else {
                self.complete_work(owner, job, rules);
                None
            };
            if let Some(core) = self.world.units.get_mut_core(h) {
                core.work = next;
            }
        }
    }

    /// Done: the target goes onto the map (improvement, road or feature removal). The site
    /// is checked again: a job whose site became invalid ends without
    /// effect.
    fn complete_work(&mut self, owner: PlayerId, job: UnitWork, rules: &CompiledRuleset<M>) {
        let tile = job.tile;
        match job.target {
            WorkTarget::Improvement(imp) => {
                if improvement_site(self.world, rules, owner, tile, imp).is_ok() {
                    self.place_improvement(owner, tile, imp, rules);
                }
            }
            WorkTarget::Road => {
                self.world.map.set_route(tile, crate::world::map::Route::Road);
                self.journal
                    .tile_changes
                    .push(TileChange::RoadBuilt { tile });
            }
            WorkTarget::Chop(feature) => self.remove_feature(owner, tile, feature, rules),
        }
    }

    /// The improvement stands on the tile; a feature it is not valid on is
    /// removed first (with its chop production).
    pub fn place_improvement(
        &mut self,
        owner: PlayerId,
        tile: TileId,
        improvement: ImprovementId,
        rules: &CompiledRuleset<M>,
    ) {
        let feature = self.world.map.feature(tile).flatten();
        let keeps = match (feature, rules.improvements.get(improvement.0 as usize)) {
            (Some(f), Some(def)) => def.valid_features.contains(&f),
            _ => true,
        };
        if let (Some(f), false) = (feature, keeps) {
            self.remove_feature(owner, tile, f, rules);
        }
        self.world.map.set_improvement(tile, Some(improvement));
        self.journal
            .tile_changes
            .push(TileChange::ImprovementBuilt { tile });
    }

    /// Remove the feature if it still stands (two workers may aim at one
    /// tile: the grant never doubles) and grant its chop production.
    fn remove_feature(
        &mut self,
        owner: PlayerId,
        tile: TileId,
        feature: FeatureId,
        rules: &CompiledRuleset<M>,
    ) {
        if self.world.map.feature(tile).flatten() != Some(feature) {
            return;
        }
        self.world.map.set_feature(tile, None);
        self.journal
            .tile_changes
            .push(TileChange::FeatureChopped { tile });
        let production = removal_of(rules, feature)
            .map(|r| r.production)
            .unwrap_or(0);
        if production == 0 {
            return;
        }
        // × the game speed's pace (Unciv: TileImprovementFunctions, chop yields × `modifier`).
        let production = crate::rules::speeds::SpeedValues::scale(production, self.world.settings().speed(rules).pace_percent);
        if let Some((city, amount)) = chop_production(self.world, owner, tile, production) {
            self.world.cities.add_production(city, amount);
        }
    }
}
