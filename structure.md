# Module structure and contracts

> **State as of 21 October 2026, checked against the code** (`origin/main` `9230aafa`): a line-by-line check of every claim is in platform/docs/structure-conformance.md; verify with the command `sh platform/tools/check-structure.sh`. Crate names: the historical `nation-core` is now `kernel` (the 4X kernel) plus `classic-rules` (the Classic mechanics), `nation-render` is `render`, `nation-client` is `client`. Where the document's design is not yet fulfilled, there is a **TARGET (not yet true)** mark with a link to the table row; the design has not been removed. The document index is docs/README.md.

> **Folder layout (21 October 2026).** The tree is split into `platform/`, `classic/`, `lands/` (with the app) and `integration/`; a crate path such as `crates/classic-rules` in older text of this document is `classic/crates/classic-rules` now (`lands/launcher/desktop` for `apps/desktop`). The crate graph below is generated and has the real paths. Why and how: docs/split-layout.md.

## 1. Workspace

The list of crates and the dependency graph below are **generated** by `sh platform/tools/gen-crate-graph.sh` (from `cargo metadata`, without a build); `platform/tools/check-structure.sh` fails if the block has diverged from the workspace. After adding a crate or a dependency, regenerate the block.

```text
nation/
├── Cargo.toml
├── crates/                  21 crates (list and graph below)
├── apps/                    desktop, web, android
├── assets/                  rules/ models/ shaders/ audio/ fonts/ lang/ visual/ (+ PACK-URL, PACK-VERSION)
├── tools/                   check and build scripts (platform-lint, check-structure, gen-crate-graph, ...)
├── docs/                    documents by topic (docs/README.md)
└── benches/                 ai/ results/ BASELINE.md
```

The scenarios (`representative`, `stress`, `torture`) are not a directory of files but code:
`integration/crates/aurelia-qa/src/scenarios/` (the AI match bench) and the map generator
`platform/crates/kernel/src/scenario/` (`ScenarioConfig`). There is no `scenarios/` directory.

Crate roles:

```text
prof, i18n                            utility: area profiler, message catalog
kernel                                4X kernel, generic over the rules module (RulesModule)
render, client                        platform: rendering (kernel only) and the client shell (`shell`)
classic-rules / -content / -ui        Classic module: mechanics, RON schema and data, interface
lal-economy, lal-mapgen,              Lands & Laws module: rules, content, interface
  lal-rules, lal-content, lal-ui
sandbox-rules / -content / -ui        third module, the sandbox: a mini-4X on kernel contracts only
net, net-server                       multiplayer: protocol and match logic without I/O; server (tokio)
aurelia-qa, session-profile, gallery  AI match and tournament bench; session profile; art showcase
desktop, web, android                 applications: window/browser/phone, module choice
```

Dependency graph (`[dependencies]`; `dev:` in brackets means test-only; a line =
"crate `<-` what it depends on"; the order is from the lower layer to the upper one):

<!-- crate-graph:begin -->
```text
i18n             platform/crates/i18n                <- -
lal-economy      lands/crates/lal-economy            <- -
lal-mapgen       lands/crates/lal-mapgen             <- -
prof             platform/crates/prof                <- -
ui-input         platform/crates/ui-input            <- -
kernel           platform/crates/kernel              <- prof
classic-rules    classic/crates/classic-rules        <- kernel, prof  [dev: classic-content]
lal-rules        lands/crates/lal-rules              <- kernel, lal-economy, lal-mapgen, prof  [dev: lal-content]
net              platform/crates/net                 <- kernel  [dev: sandbox-content, sandbox-rules]
render           platform/crates/render              <- kernel  [dev: sandbox-content, sandbox-rules]
sandbox-rules    platform/crates/sandbox-rules       <- kernel  [dev: sandbox-content]
classic-content  classic/crates/classic-content      <- classic-rules  [dev: kernel]
client           platform/crates/client              <- i18n, kernel, render  [dev: sandbox-content, sandbox-rules]
lal-content      lands/crates/lal-content            <- kernel, lal-economy, lal-mapgen, lal-rules  [dev: i18n]
sandbox-content  platform/crates/sandbox-content     <- kernel, sandbox-rules
classic-ui       classic/crates/classic-ui           <- classic-content, classic-rules, client, i18n, render  [dev: kernel]
gallery          integration/crates/gallery          <- classic-content, classic-rules, client, kernel, lal-content, lal-rules, render, ui-input
lal-ui           lands/crates/lal-ui                 <- client, i18n, kernel, lal-content, lal-economy, lal-rules, render  [dev: lal-mapgen]
launcher-core    platform/crates/launcher-core       <- client, kernel, render, ui-input
net-server       integration/crates/net-server       <- classic-content, classic-rules, kernel, lal-content, lal-rules, net, sandbox-content, sandbox-rules
sandbox-ui       platform/crates/sandbox-ui          <- client, i18n, kernel, render, sandbox-content, sandbox-rules
session-profile  integration/crates/session-profile  <- classic-content, classic-rules, kernel, lal-content, lal-rules, prof
ui-input-diff    integration/crates/ui-input-diff    <- client, ui-input
aurelia-qa       integration/crates/aurelia-qa       <- classic-content, classic-rules, client, kernel, lal-content, lal-economy, lal-rules, lal-ui, sandbox-content, sandbox-rules  [dev: render]
classic-android  classic/launcher/android            <- classic-ui, launcher-core
classic-desktop  classic/launcher/desktop            <- classic-ui, client, kernel, launcher-core, render
classic-web      classic/launcher/web                <- classic-ui, launcher-core
dev-desktop      integration/crates/dev-desktop      <- classic-ui, gallery, kernel, lal-content, lal-ui, launcher-core, sandbox-content, sandbox-ui
lands-android    lands/launcher/android              <- lal-ui, launcher-core
lands-desktop    lands/launcher/desktop              <- lal-content, lal-ui, launcher-core
lands-web        lands/launcher/web                  <- lal-ui, launcher-core
```
<!-- crate-graph:end -->

Forbidden (checked by `platform/tools/check-structure.sh`: the graph from `cargo metadata` and
`platform/tools/platform-lint.sh`, counters `module_deps`, `sandbox_deps`, `lal_deps`):

```text
kernel | render | client → classic-*, lal-*, sandbox-*  (the rules module and its content)
kernel → render | client | any interface; kernel depends only on prof
render → client
classic-* | lal-* | sandbox-* → each other (also in dev-dependencies); core: keys in sandbox-*
simulation crates (kernel, *-rules, *-content, lal-economy, lal-mapgen, net, prof) →
    wgpu, winit, egui, web-sys, wasm-bindgen, js-sys, pollster, rand*, getrandom, chrono, tokio
a services/ directory in crates/ and apps/
```

`desktop` launches the Classic module by default, the sandbox with `AURELIA_RULES=sandbox`,
Lands & Laws with `AURELIA_RULES=lands` (window and `--screenshot`); all three modules are named in
one file, `integration/crates/dev-desktop/src/module.rs`.
A module without its own effect words takes `kernel::empty::NoVocabulary`.

The interface is split in the same way (platform/PLATFORM.md, stage 6). The shell `client::shell`
(the `App<U>` frame, the screen stack and the Empire menu, the HUD frame, settings,
presentation with sound cues, saves, the art loader; the window and the
sound player are in `desktop`) knows not a single mechanic. A module is connected
through the contract `client::shell::module::ModuleUi`: its screens (`ScreenId`,
`ScreenSpec`, `draw_screen`), the Empire menu, the top-bar values and the HUD data
(`HudFeed`), the HUD panel layers, keyboard, tile hints, the match
(turns, saves, presentation); back - only `ShellAction`.
`classic-ui::ClassicUi` is the implementation for Classic; `desktop` names it in one
file, `integration/crates/dev-desktop/src/module.rs` (counter `shell_module_uses`).

The kernel names a module only through a type parameter: `GameState<M>`,
`AuthWorld<M>`, `CompiledRuleset<M>`, `ChangeJournal<M>`, the effect vocabulary
`Modifier<M>`… The module contract is `kernel::module::RulesModule` (`kernel/src/module.rs`):
nine associated types (`World` - the module's state, `Vocabulary` - the effect words and their
evaluation, `Tables`, `Yields` - derived city incomes, `Command`, `Systems`,
`Ai`, `Step`, `CityStep`), the turn schedule `SCHEDULE`, the method `execute` (validate and
apply a command, return a journal or a rejection), the winner/outcome/end of the match
(`victor`, `outcome`, `is_over`), `end_turn`, `EARLIER_EDITIONS` (older editions of the
rules, for loading saves).

**Where validation and the resolved command live.** The pipeline "command → validation →
`ResolvedCommand` → transaction" (section 42) is executed by the module's `RulesModule::execute`,
not by a single kernel function. The kernel provides its own part: `KernelCommand` (21 orders),
`validate_kernel`, `ResolvedKernel`, `WorldTransaction::apply_kernel`. Classic
wraps them in its own `Command`/`validate`/`ResolvedCommand`
(`classic-rules/src/command/`); Lands & Laws and the sandbox validate and apply in a single
function `command::execute` with no separate resolved type. The only external
write point is `GameState::execute`. `classic-rules` repeats
the kernel paths as aliases over `Classic` (`world::state::GameState` =
`kernel::world::state::GameState<Classic>`), so the sections below that say
"the kernel" (in the earlier names, the single simulation crate) refer to `kernel` and the module together: the generic part is in
`kernel`, the Classic mechanics in `classic-rules`.

Stage 5b moved into the kernel everything common that had been mixed with Classic: the transaction
(`transaction::WorldTransaction<M>`), the kernel commands and their validation
(`command::KernelCommand`, `validate_kernel`, `WorldTransaction::apply_kernel`),
the turn-step systems and the turn driver (`systems::*`), the incomes and citizens of cities
(`derived::yields`, `derived::citizens`), the map and start generator
(`scenario`), the byte codec of saves (`persistence::codec`) and the AI skeleton
(`ai::driver`, `ai::plan`, `ai::map`, `ai::trace`, `ai::observation`).
A module enters them through contracts: `systems::ModuleSystems` (the module's turn
steps, reactions to kernel events `KernelEvent`, formulas and command rules),
`derived::yields::YieldTerms` (income terms), `ai::mind::AiMind`
(planners), `persistence::codec::ModuleSections` and
`world::module::ModulePlayer` (the places of sections and of the player record in the bytes of
a save), `rules::tables::ModuleTables::{SourceRoles, Roles}` (roles),
`rules::vocabulary::StatWord` (module characteristics). Classic keeps
the flat `Command`: `Command::kernel` gives the kernel its orders.

Stage 5c removed the last Classic concepts from the kernel (lint: `kernel_words` and
`kernel_rows` are 0). The module's fields in the rows of kernel tables (the price in faith,
masterpiece slots, a resource's happiness, a specialist's great-person points…) are
one module value in the `module` field of each row:
`rules::rows::{ModuleRows, RowExtension}` (source and compiled
form), the kernel asks them only for its own (`UnitExtension`,
`BuildingExtension`, `SpecialistExtension`, `HandicapExtension`). Rows are
hashed by hand: kernel fields in order, module fields at their own places
(`UnitPlace`…), so the rules fingerprint did not change. The module's tables in the
source rules are `SourceRules::module` (`ModuleTables::{Source,
merge_source, source_keys}`), references to them are `ContentTable::Module(TableTag)`;
module words in content enumerations are `UnitRole::Module(RoleTag)` etc.
(hashed by tag, like the variants before). Player kinds are
`PlayerKind::{Major, Module(..)}` (`world::player::ModulePlayerKind`,
`ModuleWorld::PlayerKind`; for Classic `ClassicKind::{CityState, Barbarian}`),
the module's state in a city is `world::module::ModuleCity`
(`ModuleWorld::City`, `CityStore::module_of`; for Classic the great-person
points), the process of contributing to a shared project is `Process::Contribute`. A module
without any of this writes `NoRows`, `type Source = ()`, `NoKind`,
`type City = ()`; `ModuleSystems` and the projects of `ModuleTables` have
defaults for a module without an economy.

---

# 2. `kernel` (and the rules module)

`kernel` is the generic part of the authoritative gameplay simulation, generic over the rules
module `M: RulesModule`; the mechanics of a specific game (Classic - `classic-rules`,
Lands & Laws - `lal-rules`, the sandbox - `sandbox-rules`) live in the module crates and
together with the kernel form the simulation. `kernel`: 141 files, 36.7 thousand lines;
`classic-rules`: 347 files, 92.3 thousand (measured on 21 October 2026).

Layout of `kernel/src/` (checked against the file system; on the left are the sections below):

```text
kernel/src/
├── lib.rs, ids.rs (§3), math.rs, rng.rs (the only generator), fx.rs (deterministic hasher),
│   cow.rs, tile_set.rs, numeric.rs, metrics.rs, empty.rs (NoVocabulary and the "empty" parts of a module)
├── module.rs                the RulesModule contract (section 1)
├── rules/                   source, compiled, compile, compile_effects, compile_tables, effects,
│                            modifiers, fingerprint, key_index, rows, roles, tables, vocabulary, …
├── effects/                 reading effects over the world: city, unit, player, production, memo, module
├── systems/                 kernel mechanics: movement, city_founding, production, purchase, research,
│                            improvements, worked_tiles, borders, economy, elimination, units, turn, module
├── world/                   state (GameState, AuthWorld, GameView), map, player, city, unit, diplomacy,
│                            effects, indexes, module, citizens, facts, builds, territory, …
├── command/                 command (KernelCommand), validate (validate_kernel), resolved (ResolvedKernel), error
├── transaction/             transaction (WorldTransaction), journal (ChangeJournal), operations
├── turn/                    state (TurnState, TurnPhase), resolution (TurnSchedule, KernelStep), calendar
├── derived/                 state (DerivedWorld), visibility, yields, tile_yields, citizens, connections,
│                            connectivity, movement
├── navigation/              astar, cost, scratch, profile, terrain
├── knowledge/               state (PlayerKnowledge), view (PlayerView, KnownTile), sync
├── ai/                      AI skeleton: driver, mind (AiMind), plan, map, state, trace, observation, seats
├── persistence/             schema, encode, decode, codec, load, check, ai, economy, effects
├── replay/                  log (Replay), hash, journal (command journal), codec
├── validation/              world (validate_world)
├── scenario/                generator (ScenarioConfig), landscape, map_script, sea_resources
└── test_module.rs           the kernel's test rules module
```

The earlier layout of this document (a single simulation crate) listed files that now live in the
Classic module: `systems/{combat,growth,diplomacy,effects}`, `knowledge/observation.rs`
(AI observation is assembled in the kernel's `ai/observation.rs` and in
`classic-rules/src/knowledge/observation.rs`), `ai/planner.rs`
(`classic-rules/src/ai/`); `systems/culture.rs` is the module's `policies.rs`; there is
no `ai/evaluation.rs` file. Row-by-row discrepancies are in
platform/docs/structure-conformance.md, section 2.

---

# 3. `ids`

Contains only strongly typed identifiers.

```rust
pub struct TileId(pub u32);
pub struct PlayerId(pub u8);

pub struct UnitHandle {
    pub index: u32,       // private in the design; pub in the code (ids.rs:13-23)
    pub generation: u32,
}

pub struct CityHandle {
    pub index: u32,
    pub generation: u32,
}

pub struct EntityId(pub u64); // permanent number of a unit/city for saves and replays
```

Also content IDs:

```rust
pub struct UnitKindId(pub u16);
pub struct BuildingKindId(pub u16);
pub struct TechId(pub u16);
```

## Contract

`ids`:

- knows nothing about stores;
- knows nothing about rules;
- performs no lookups;
- contains no gameplay logic.

Runtime handles are not considered persistent IDs; the persistent identity is `EntityId`.
Besides identifiers, `ids.rs` holds a few small `Copy` values
(`ProductionItem`, `UnitWork`, `WorkTarget`), so "identifiers only" is
not fulfilled literally (rows 3.1–3.2 of the conformance table).

---

# 4. `rules`

Responsible for describing the rules of the game.

Separation:

```text
SourceRules
   ↓ compile
CompiledRuleset
```

### `source.rs`

Represents the human-readable model:

```rust
// SourceRules<M>, CompiledRuleset<M> — generic over the rules module
pub struct SourceUnit {
    pub id: StableContentKey,
    pub movement: u16,
    pub effects: Vec<SourceEffect>,
}
```

### `compiled.rs`

Runtime representation:

```rust
pub struct CompiledRuleset {
    pub units: Vec<UnitDefinition>,
    pub buildings: Vec<BuildingDefinition>,
    pub technologies: Vec<TechnologyDefinition>,
    ...
}
```

## Contract

After compilation:

```text
CompiledRuleset is immutable.
```

The simulation accepts:

```rust
&CompiledRuleset
```

and never modifies it.

No string lookups in the hot gameplay path.

---

# 5. `rules::compile`

A separate semantic compiler.

```rust
pub fn compile_rules<M: RulesModule>(
    source: SourceRules<M>
) -> Result<CompiledRuleset<M>, RulesError>; // kernel/src/rules/compile.rs:60
```

It must:

- resolve stable IDs;
- check references;
- check duplicates;
- check modifier structure;
- build runtime indexes;
- determine deterministic ordering.

This is an important boundary:

```text
invalid content must not leak into the simulation.
```

## 5.1 Effects (§20–21 platform/project.md)

An effect is an entry in the content of a source (building, technology, unit, promotion,
nation, policy, belief). The types are in `rules/effects.rs`, the compiler in
`rules/compile_effects.rs`, reading over the world in `effects/`, applying
triggers in `systems/effects.rs` (in `classic-rules`), timers in `world/effects.rs`.

> **The list below is the original schema, and it is incomplete** (row 5.4 of the conformance table). In the kernel
> today: Scope has 5 variants (`Player, City, Unit, Tile, Module`), Selector 11, Condition 29,
> Trigger 10, Modifier 27 (`kernel/src/rules/effects.rs`); the Classic words
> (`OnAdopt`, happiness, faith, policies, tourism, spies…) are `Module(..)` variants of the module
> (`classic-rules/src/classic/effects.rs`). The extension principle at the end of the section
> is unchanged.

```text
scope      Player | City | Unit | Tile — who receives the modifier
selector   Owner, SourceCity, OwnerCities, EventCity, SourceUnit, OwnerUnits,
           EventUnit, SourceCityTiles, OwnerCityTiles, Capital — exactly whom
condition  Always, HasTechnology, LacksTechnology, HasBuilding, MinPopulation,
           TileImprovement/Resource/Feature
trigger    Passive | OnBuilt | OnTurnStart | OnTechnologyUnlocked |
           OnCityFounded | OnUnitKilled | OnUnitTrained | OnAdopt (policies)
modifier   AddYield, AddYieldPercent, MultiplyYield, AddYieldPerPopulation
           (by YieldKind: Output, Food, Production, Gold, Science, Culture,
           Faith), ModifyGrowth, ModifyCost (Units, Buildings, Technologies,
           TilePurchase, Hurry, BorderGrowth, BorderGrowthExponent, Policies,
           PolicyCityPenalty), ModifyCombat (Any, Attack,
           Defense, VsCity), ModifyMovement, AddHappiness,
           AddGreatPersonPoints, UnlockUnit/Building, FreeUnit {count},
           FreeBuilding, AddPromotion, FreePolicies {count},
           FreeBuildingInCities {building, count}
duration   WhileSourceExists (passive) | Once (instant) | Turns(n)
stacking   Stack | Unique
```

RON (`classic-content/src/effects.rs`): `scope`, `selector`, `modifier`
are required, the rest defaults to a plain passive effect; an unknown
field is a parse error.

```ron
(scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Output, amount: 2))
```

Before the game starts, the compiler rejects: an unknown reference; a selector of the wrong
scope or one unavailable to the source; a modifier of the wrong scope; a condition of the wrong
scope; a trigger unavailable to the source; an instant modifier without a trigger and
a passive unlock; a duration that does not suit the trigger (passive — only
WhileSourceExists; Once — instant or a grant into the stockpile; Turns — player or
city); a multiplier without Unique, Unique on Once, and more than 8 multipliers
on a single YieldKind.

Compilation indexes: the effects of each source are laid out in buckets
(passive ones by scope, triggered ones by trigger), `EffectIndex::by_trigger`
lists the sources of each trigger, `owner_wide_buildings` holds the buildings with
empire-wide effects. When computing, a city walks the nation,
the owner's researched techs, its own buildings and only `owner_wide_buildings`;
there is no full scan of the rules' effects.

### Evaluation order (part of the game spec)

```text
1. base            the system's value (tiles, citizens, content)
2. flat            + Σ AddYield / ModifyMovement
3. additive %      × (100 + Σ percents) / 100
                   (AddYieldPercent, ModifyCost, ModifyCombat, ModifyGrowth)
4. multiplicative  × Π (percent / 100) — MultiplyYield, Unique only,
                   in ascending order of the multiplier (independent of source order)
5. caps            the consumer's limits (for example, production ≥ 1)
6. rounding        once, toward zero
```

Stages 3–5 are computed in hundredths, rounding happens only at step 6
(`rules/modifiers.rs::StageTotals::evaluate`). Instant effects of a single
event are applied in the canonical order of sources: nation,
technologies (in order of research), buildings (in order of cities), unit kind,
promotions; within a source — in order of declaration.

Turns(n): the effect takes part in the next n resolutions of the owner's turn;
one launched inside a resolution is not counted against that resolution.

Adopted policies (`world::culture`, the `systems::policies` system) are sources
of the owner, like researched technologies: their passive effects reach the player's
cities, `OnAdopt` fires on adoption (the opening bonus — when the
branch is opened, the completion bonus — when it is completed). The `Capital` selector is the capital
(the city with a Palace). Beliefs are compiled and checked, but there are no religions yet.

### Dictionary of Classic → schema

`classic/docs/classic/effect_vocabulary.json` (354 modifier names, 33 selectors,
18 triggers, 25 condition keys) maps onto the schema by families: a name
`<family>.<parameter>.<stage>` is a single `Modifier` variant with a typed
parameter and an evaluation-order stage, not a separate variant per name.

The "Status" column below reflects the M4 plan and is out of date: the M4 words (faith, tourism, policies,
spies, great people) have long been `Module(..)` variants of the Classic module (row 5.6 of the conformance
table); the mapping scheme remains correct.

| Dictionary | Schema | Status |
|---|---|---|
| `yield.<y>.flat` / `.pct` / `.per_pop` | AddYield / AddYieldPercent / AddYieldPerPopulation (hundredths) | exists |
| `tile.yield.<y>.flat` + `selector.filter` | Tile scope, AddYield + condition TileResource/Improvement/Feature | exists |
| `combat.strength/attack/defense/attack_vs_city.pct` | ModifyCombat { context } | exists; terrain, flank, wounded, domain — new CombatContext values in M4 |
| `unit.moves.flat` | ModifyMovement | exists; sight/range/cargo/attacks — UnitStat in M4 |
| `growth.rate.pct` | ModifyGrowth | exists |
| `happiness.local/global` | AddHappiness (City / Player scope) | exists, consumer — the M4 happiness system |
| `production.<class>.pct` | AddYieldPercent(Production) + a "what is being built" condition | condition — M4 |
| `purchase.cost.pct`, `tile_purchase.cost.pct`, `border.culture_cost.pct` | ModifyCost { Hurry, TilePurchase, BorderGrowth } | exists |
| `grant.unit/building/promotion` | FreeUnit { count } / FreeBuilding / AddPromotion | exists; `grant.tech/policy/great_person` — new M4 variants |
| `unlock.unit` | UnlockUnit | exists |
| `great_people.rate.pct`, `unit.free_xp`, `heal.*`, `trade.*`, `tourism.*`, `religion.*` | new variants, one per family | M4, together with their own mechanic |
| `trait.*`, `policy.*`, `promotion.*`, `building.*` with `needs_mechanic` | flags of special mechanics | M4: a separate system, not a modifier |

Selectors: `this_city` → SourceCity, `all_cities` → OwnerCities,
`tiles_of_city` → SourceCityTiles, `tiles_of_player` → OwnerCityTiles,
`this_unit` → SourceUnit, `all_units` → OwnerUnits, `units_trained_here` →
EventUnit + OnUnitTrained, `player` → Owner, `cities_with_building` →
OwnerCities + HasBuilding; `capital`, `coastal_cities`, specialists,
religious ones (`following_cities`, `holy_city`) and auras (`adjacent_*`,
`enemy_units_in_range`) — new Selectors together with their own mechanics (the
Religion scope too). Triggers: `on_complete` → OnBuilt, `on_research` →
OnTechnologyUnlocked, `on_kill` → OnUnitKilled, `on_city_founded` →
OnCityFounded, `on_unit_trained` → OnUnitTrained; `on_adopt`, `on_attack`,
`on_great_person_expended` and the religious ones — with policies, combat and religion in M4.
Conditions: `tech_known` → HasTechnology, `until_tech` → LacksTechnology,
`min_population` → MinPopulation; `golden_age`, `city_on_river`,
`city_garrisoned` and others — one Condition variant each.

The extension is always the same: a variant in `rules/effects.rs`, a compiler
rule, one consumer; exhaustive `match`es without a wildcard
make the compiler point out every place that has to account for it.

---

# 6. `world`

Holds the authoritative state.

The main structure (`kernel/src/world/state.rs:68`):

```rust
pub struct GameState<M: RulesModule> {
    auth: AuthWorld<M>,
    derived: DerivedWorld<M>,
    path_scratch: PathScratch,   // single-owner scratch for validate
    recording: Recording<M>,     // command journal (platform/docs/saves.md)
}
```

Inside (what exists in the code):

```rust
pub struct AuthWorld<M: RulesModule> {
    pub map: MapState,
    pub players: PlayerStore<M>,
    pub cities: CityStore<M>,
    pub units: UnitStore<M>,
    pub diplomacy: DiplomacyState,
    pub turn: TurnState,
    pub indexes: WorldIndexes,
    pub knowledge: Vec<PlayerKnowledge>,
    pub effects: ActiveEffectStore<M>,
    pub rng: RngState,
    pub next_entity: u64,
    pub settings: GameSettings,
    pub module: M::World,        // rules module state
    /* memo: pub(crate) — cache of pure effect functions */
}
```

> **TARGET (not yet true): `AuthWorld` is not public outside the crate.** Today the struct and
> its 13 fields are `pub`, `GameView { pub auth, pub derived }`; reading goes both through
> methods (`map()`, `units()`, …) and directly through fields. Bringing it to the design:
> fields `pub(crate)` + reader methods (L). Rows 6.2–6.3 of the conformance table.

## Contract

It must be impossible to obtain:

```rust
&mut UnitStore
&mut CityStore
&mut MapState
```

from ordinary gameplay code.

Reading is allowed through read-only views (`GameView`, `PlayerView`).

> **TARGET (not yet true).** Today `GameState::world_mut()` hands out `&mut AuthWorld`
> (outside tests it is called from: `classic-rules` 25 places, `lal-rules` 4, `lal-ui` 34,
> `sandbox-rules` 1; 264 in total with tests), and 31 `GameState::debug_*` methods change the world
> bypassing the journal. The module systems change the stores through `tx.world.<store>` inside
> a transaction — that is the intended path. Removing `world_mut`/`debug_*` from the public API
> (a typed world builder for scenarios and tests) is being done by the `world-builder` agent,
> branch `refactor/world-builder`; rows 6.4, 41.2, 52.7 of the conformance table.

---

# 7. `world::map`

Basic geometry and authoritative tile data.

```rust
pub struct MapState {
    dimensions: MapDimensions,
    terrain: Vec<TerrainId>,
    feature: Vec<Option<FeatureId>>,
    resource: Vec<Option<ResourceId>>,
    owner: Vec<Option<PlayerId>>,
    improvement: Vec<Option<ImprovementId>>,
}
```

The layout does not have to be exactly this.

## Contract

`TileId` is a cheap dense lookup key.

Map is not responsible for:

- visibility;
- UI overlays;
- AI threat maps;
- rendering.

---

# 8. `world::unit`

Storage of units and invariants.

Public:

```rust
pub struct UnitView<'a> { ... }
```

Internal:

```rust
struct UnitStore { ... }
```

Mutation only through the transaction API.

> **TARGET (not yet true):** `UnitStore::get_mut_core`, `module_mut` and
> `CityStore::get_mut`, `add_building` are `pub` today and require only `&mut` of the store
> (row 8.2 of the correspondence table; `world-builder`).

For example:

```rust
tx.move_unit(unit, destination)?;   // systems/movement.rs
tx.destroy_unit(unit)?;             // transaction/operations.rs
// there is no separate owner-transfer operation: it is part of capturing a city/unit
```

## Contract

It must not be possible to change separately:

```text
unit.owner
```

without updating the corresponding indexes.

---

# 9. `world::indexes`

Authoritative secondary indexes.

For example:

```text
units_by_player
unit_by_tile
cities_by_player
```

## Contract

Indexes:

- are part of the authoritative world invariant;
- are updated inside transaction operations;
- are never updated by downstream listeners.

That is:

```text
UnitStore + authoritative indexes
```

are committed atomically.

---

# 10. `command`

This is the public write API of the simulation. The command type is the module's
`RulesModule::Command`; the kernel provides its own part, `KernelCommand` (21 orders: movement, city founding, research
choice, production queue, purchase, worked tiles, `EndTurn`…,
`kernel/src/command/command.rs:14`). Classic keeps a flat `Command`, L&L and the sandbox
wrap `Command::Kernel(KernelCommand)`.

For example (the shape from the design):

```rust
pub enum Command {
    MoveUnit {
        unit: UnitHandle,
        destination: TileId,
    },

    FoundCity {
        settler: UnitHandle,
    },

    ChooseTechnology {
        player: PlayerId,
        technology: TechId,
    },
}
```

## Contract

`Command` expresses an **intent**, not a low-level mutation.

Bad:

```text
SetUnitTile
SetCityPopulation
SetTreasury
```

Good:

```text
MoveUnit
AttackUnit
FoundCity
SelectProduction
```

---

# 11. `command::validate`

Takes:

```text
read-only state
+
command
```

and returns:

```text
ResolvedCommand
```

or an error.

```rust
// kernel/src/command/validate.rs:23 — kernel commands
pub fn validate_kernel<M: RulesModule>(
    state: GameView<'_, M>, rules: &CompiledRuleset<M>,
    command: KernelCommand, scratch: &mut PathScratch,
) -> Result<ResolvedKernel, CommandError>;

// classic-rules/src/command/validate.rs:19 — Classic commands (kernel() → validate_kernel)
pub fn validate(
    state: GameView<'_>, rules: &CompiledRuleset,
    command: Command, scratch: &mut PathScratch,
) -> Result<ResolvedCommand, CommandError>;
```

`scratch` is the working memory for pathfinding; it belongs to `GameState` (section 19).
L&L and the sandbox have no separate `validate`: checking and applying are one function,
the module's `command::execute` (section 12).

## Contract

Validation:

- does not mutate state;
- has no side effects;
- must be deterministic.

---

# 12. `ResolvedCommand`

The internal representation of an already validated action. **It belongs to the rules module, not to the
kernel**: in the kernel it is `ResolvedKernel` (`kernel/src/command/resolved.rs:9`), in Classic
`ResolvedCommand` (`classic-rules/src/command/resolved.rs:7`) wraps it;
L&L and the sandbox do not introduce a resolved type (row 12.1 of the correspondence table).

For example, the user says:

```text
Move unit to X
```

and the resolved command may contain:

```rust
ResolvedMoveUnit {
    unit,
    from,
    to,
    movement_cost,
}
```

## Contract

It separates:

```text
"is it allowed?"
```

from:

```text
"what exactly do we apply?"
```

But commit is still obliged to protect the main invariants.

---

# 13. `transaction`

The single point of authoritative mutation.

```rust
pub struct WorldTransaction<'a, M: RulesModule> {
    pub world: &'a mut AuthWorld<M>,   // in the design it is private, pub(crate)
    pub journal: ChangeJournal<M>,
    /* scoped: the player's modifier cache */
}
```

> **TARGET (not yet true): `pub(crate)` and closed fields.** Rules modules are separate
> crates and write to the world through `WorldTransaction`, so the type stays `pub`; the goal is to
> close the fields and provide operation methods. Today the type is called from four crates and from
> `lal-ui` (row 13.1 of the correspondence table).

Operations:

```rust
tx.move_unit(...)        // systems/movement.rs
tx.spawn_unit(...)       // transaction/operations.rs (create_unit, spawn_at_city)
tx.destroy_unit(...)
// changing a tile's owner, constructing a building — methods of the module's systems and of the stores
```

## Contract

After every successful transaction:

```text
AuthWorld valid.
```

If a command is rejected:

```text
authoritative world unchanged.
```

Rollback does not have to be implemented by copying the world.

Preferred:

```text
validate everything needed
→ perform infallible mutation
```

### System registry

`transaction.rs` only maps `ResolvedCommand` to a system and closes the
commit (`refresh_alive`). Kernel mechanics live in `kernel/src/systems/`, Classic mechanics in
`classic-rules/src/systems/` (34 modules). Each mechanic is a `systems/` module with its own
functions (`impl WorldTransaction`) and its own journal entries: movement,
combat, city_founding, production, growth, culture, worked_tiles,
improvements, research, diplomacy, purchase, turn, effects (triggers),
elimination. A system that needs the work of another calls it explicitly
(production → `fire(OnBuilt)`); there is no event bus or subscriptions (§10, §56).
Mechanic tests live in `systems/tests/<mechanic>.rs`.

---

# 14. `ChangeJournal`

Records the changes of one committed transaction or resolution batch.

```rust
pub struct ChangeJournal<M: RulesModule> {     // transaction/journal.rs:8
    pub unit_changes: Vec<UnitChange>,
    pub city_changes: Vec<CityChange>,
    pub tile_changes: Vec<TileChange>,
    pub player_changes: Vec<PlayerChange>,
    pub module_changes: Vec<ModuleChangeOf<M>>, // changes to the module's state
}
```

For example:

```rust
pub enum UnitChange {
    Created(UnitHandle),
    Moved {
        unit: UnitHandle,
        from: TileId,
        to: TileId,
    },
    Destroyed(UnitHandle),
}
```

## Contract

The journal describes:

```text
what changed
```

but not:

```text
what needs to be recomputed.
```

Forbidden:

```rust
Change::InvalidateVisibility
Change::InvalidateRenderer
```

---

# 15. `derived`

Derived state exists alongside the authoritative world, but is not gameplay truth.

```rust
pub struct DerivedWorld<M: RulesModule> {      // derived/state.rs:11, fields are pub
    visibility: VisibilityState<M>,
    yields: YieldState,
    movement: MovementState,
    connectivity: ConnectivityState,
}
```

Each subsystem has roughly the same concept:

```rust
impl VisibilityState {
    pub fn rebuild(...);

    pub fn apply_changes(
        ...,
        changes: &ChangeJournal,
    );
}
```

## Contract

Required:

```text
rebuild(auth) == incremental(auth, previous, changes)
```

logically. This is checked by `GameState::validate(&rules)` (via `DerivedWorld::assert_matches_rebuild`,
`derived/state.rs:59`): it is called by the tests and benches (`ai_game`, `perf_baseline`, `round_bench`).

---

# 16. `derived::visibility`

Responsible only for:

```text
who currently sees what
what has already been explored
```

Not responsible for:

```text
what the AI knows about units seen long ago.
```

That is the job of `knowledge`.

---

# 17. `derived::yields`

Computes runtime derived values. The generic part (`tile_yields`, `citizens`, `yields`,
`connections`) is in `kernel/src/derived/`; the Classic mechanics from the descriptions below
(`derived::happiness`, `maintenance`, `income`, `resources`, `great_people`,
`culture`, `religion`, `tourism`, `trade`) are in `classic-rules/src/derived/`.

```text
tile yields
city totals
player aggregates
```

But does not store:

```text
city population
buildings
tile ownership
```

because that is authoritative state.

Yields per Classic (M4-A, mechanic 1):

```text
derived::tile_yields  tile yield: map-code terrain → feature (replaces
                      or adds) → lake +1 food → revealed resource →
                      improvement (+ its yield on the resource, + technologies on fresh
                      water, + per adjacent mountain) → city center at least 2/1 → 0
derived::citizens     citizens: the center; assigned specialists; locked tiles;
                      the rest one at a time — the best own tile within 3 rings,
                      a free specialist slot, or unemployed (by focus weights,
                      food ×2 while the city is not growing)
derived::yields       a city by channels: tiles + specialists + science per citizen →
                      city modifiers by stages (+20 % production in a golden
                      age); stores city rates and player totals without the unhappiness
                      penalty (derived::income applies it)
```

The tile and build-site rules (`derived::tile_yields`, `world::builds`)
read `world::facts::TileFacts`: the truth for the game (`TruthFacts`) and the player's
knowledge (`PlayerView`) for the AI and UI — one rule for both sources.
Citizen orders (focus, locked tiles, specialists) are the authoritative
`CityCore::citizens`, saved in the economy section (`persistence::economy`).

Growth and great people (M4-A, mechanic 2):

```text
systems::growth          threshold 15 + 8(p−1) + ⌊(p−1)^1.5⌋; food stock in hundredths
                         (CityCore::food_times100); starvation −1 citizen (not below 1);
                         kept food (Stat::FoodKept, aqueduct 40 %);
                         settler at the head of the queue: surplus → production
derived::great_people    great person (GP) actions (GreatPersonAction), thresholds, city points,
                         action check — read-only for the UI and AI
systems::great_people    city step GreatPeople (first): points, birth;
                         actions: improvement (academy, manufactory, customs house,
                         citadel capturing adjacent tiles), scientist's lightbulb
                         (science of the last 8 turns), engineer's rush,
                         artist's golden age
systems::golden_age      turn step GoldenAge (before cities): golden age (GA) countdown
```

The growth stock, kept food and GP points are in `CityCore::economy`
(`world::city_economy`); the GP and GA counters and the science history are in
`PlayerCore::economy` (`world::player_economy`); all of it is in the economy section
of the save.

Happiness and golden ages (M4-A, mechanic 3):

```text
derived::resources       player resources: the known and connected (city center
                         or resource improvement) tiles of its cities
derived::happiness       happiness report: difficulty (handicap), luxuries (each kind
                         once), local (cap = population), global, natural
                         wonders, Stat::HappinessPerCity; unhappiness: cities,
                         citizens, occupied (in hundredths, Stat percentages and
                         handicap, AIUnhappinessPercent); mood and penalties
systems::happiness       turn step Happiness: a snapshot of the surplus
                         (PlayerEconomy::excess_happiness) — it is read by growth
                         (−75/−100 %), production and gold (−2 %/point, ≤ −40 %,
                         via EmpireContext), combat (effects::unit), and the ban on
                         settlers and founding at ≤ −10
systems::golden_age      step GoldenAge: countdown or gauge (surplus accumulates, threshold
                         500 + 250·GA + 1 %/city, multiple of 5), length 10 ×
                         Stat::GoldenAgeLengthPercent
world::game_settings     game difficulty and AI players (handicap rows: human — the
                         game level, AI — AI_DEFAULT with the level's ai_*); set by
                         GameState::configure_players or AiDriver on the first step
```

Gold (M4-A, mechanic 4):

```text
derived::maintenance     upkeep of units (Classic: 0.5 per unit × (1 + 8p)^(1 + p/7),
                         p = turn/500, in fixed point), buildings, roads; gold
                         for the connection to the capital (−1 + 0.15·capital + 1.1·city)
derived::connections     cities connected to the capital by roads or by sea (harbors)
systems::economy         step Gold: balance; the treasury not below 0, a shortfall is taken from
                         the turn's science, at ≤ −5 with more units than cities — disbanding 1/2
world::building_rules    where building is allowed: unbuildable, unique buildings,
                         national wonders (one, a building in all cities),
                         coast, river, mountains, fresh water, resources nearby,
                         mutually exclusive, capital only
systems::processes       Wealth and Research: 25 % of the production of an empty
                         queue into gold or science (Command::SetProcess)
world::city::hurry_price purchase: ⌊(30·P)^0.75⌋ × (100 + HurryCostModifier) %, /10;
                         wonders cannot be purchased; tile: 50 + 5·bought, × distance
```

AI: `ai::economy::EconomyAdvisor` in the `ai::system` registry (great people,
focus, happiness buildings); AI expansion is limited by happiness
(`planner_strategy::happiness_allows_city`) instead of the limit of 12 cities.

---

# 18. `navigation`

Navigation stands apart from `derived` because most of pathfinding is a **query service**, not persistent derived state.

Main API:

```rust
pub fn find_path(
    world: GameView<'_>,
    rules: &CompiledRuleset,
    request: PathRequest,
    scratch: &mut PathScratch,
) -> Option<Path>;
```

## Contract

Navigation:

- does not mutate the world;
- does not own units;
- does not cache arbitrary results globally without need;
- uses plain A* first.

Persistent navigation caches may live in `DerivedWorld` later.

---

# 19. `PathScratch`

Working memory:

```rust
pub struct PathScratch {
    open: BinaryHeap<Node>,
    cost: Vec<u32>,
    parent: Vec<TileId>,
}
```

Reused between queries.

This is an optimization implementation detail.

Public gameplay code must not know about it.

---

# 20. `turn`

Sets the simulation progression.

```rust
pub enum TurnPhase {
    Start,
    PlayerActions,
    Resolution,
    End,
}
```

And:

```rust
pub struct TurnState {                   // turn/state.rs:20
    pub round: u32,              // global round ("turn N" in the UI and match progress)
    pub active_player: PlayerId, // whose turn it is now
    pub phase: TurnPhase,        // phase of the active player's turn
    /* window, acting: pub(crate) — simultaneous turn window, execute_as */
}
```

Turns are sequential (Classic single player, `classic/docs/classic/rules.md` §1):
live players move in id order; only the active one acts
(`validate` rejects commands from the others: "not your turn"). The active
player's `EndTurn` runs their sequence and passes the turn to the next live
player; after the last one the round grows by 1. An eliminated active player
passes the turn in the commit of the command.

## Contract

Only `turn` determines:

```text
when a new turn begins
when delayed effects are applied
when resolution happens
```

No subsystem does the following on its own:

```rust
if world.turn_changed() { ... }
```

with implicit semantics.

## Order of systems in a player's turn

The end of a player's turn (`EndTurn`) is the rules module's schedule
(`RulesModule::SCHEDULE`; for Classic, `classic/turn.rs::SCHEDULE`, whose
documentation is the order spec with the reason for each place): kernel steps
(`turn/resolution.rs::KernelStep`) interleaved with module steps. The kernel
checks the schedule at compile time (each of its own steps exactly once, in its
own order and in its own phase) and executes `systems/turn.rs`. The table below describes the
owner systems; the order follows `classic/turn.rs`.
The `TurnState` phase goes Start → Resolution → End and returns to
PlayerActions. The number in the list is part of the spec: reordering changes the game.

> **The table below is partly outdated** (row 20.3 of the conformance table). The source
> of truth is `classic-rules/src/classic/turn.rs::SCHEDULE`. Its order today:
>
> ```text
> Start:      HealUnits, CitadelDamage, MountainDamage, RestoreMoves, CapitalConnections,
>             TurnStartEffects, NaturalWonderFinds
> Resolution: Happiness, GoldenAge, Trade, Barbarians, CityCombat, Cities, Gold, Culture,
>             Tourism, Science, Research, Espionage, Diplomacy, CityStates, Congress,
>             Faith, WorkerJobs
> End:        ConquestDecisions, TimedEffects, Victory
> city:       GreatPeople, Growth, Production, Borders
> ```
>
> The table below has no steps `MountainDamage`, `CapitalConnections`, `NaturalWonderFinds`,
> `Happiness`, `GoldenAge`, `Trade`, and it has `UnitUpkeep` and `TurnEnded`, which are not
> in the schedule; the order of `Happiness`/`GoldenAge` (before `Barbarians`) and `Faith` (after `Congress`)
> differs in the table. The step descriptions are correct in meaning.

| # | Phase | Step | System | What it does |
|---|---|---|---|---|
| 1 | Start | HealUnits | combat | units that did not spend their turn: city 25, own land 20, neutral and enemy 10, healing promotions (Classic, `classic/docs/classic/rules.md` "Combat" §5) |
| 1a | Start | CitadelDamage | combat | citadels hit adjacent enemies (30) |
| 2 | Start | RestoreMoves | turn | movement points = base + ModifyMovement + promotions; attack and intercept counters are zeroed |
| 3 | Start | TurnStartEffects | effects | OnTurnStart trigger |
| 4 | Resolution | UnitUpkeep | economy | 1 gold per unit (before income); the congress's Standing Army Tax +25 % |
| 4a | Resolution | Barbarians | barbarians | barbarian turn only: technologies of half the nations, camps in the fog, spawn from camps |
| 4b | Resolution | CityCombat | combat | cities heal and can fire again; resistance and burning continue |
| 5 | Resolution | Cities | growth, production, culture | for each city in list order — the city steps below |
| 6 | Resolution | Gold | economy | sum of city gold → player modifiers → treasury |
| 6a | Resolution | Culture | policies | sum of city culture → player pool (policies), 8-turn history, maximum of cities |
| 6b | Resolution | Tourism | tourism | influence on each nation met: city tourism (great works, theming, tourist buildings) × capital modifier; then the ideology's public opinion (`classic/docs/classic/rules.md`) |
| 7 | Resolution | Science | research | sum of city beakers → player modifiers → stockpile |
| 8 | Resolution | Research | research | completion of queued techs, OnTechnologyUnlocked trigger |
| 8b | Resolution | Espionage | espionage | era spies (Renaissance for everyone, Industrial and later — for the one who entered it), each spy's turn: path, surveillance, theft, elections, introducing a diplomat, replacing a dead one |
| 8c | Resolution | Diplomacy | diplomacy | gold per turn of deals, research agreements, expiry of borders and offers, decay of the memory of belligerence |
| 8d | Resolution | CityStates | city_states | city-state turn only: first-contact gold, influence decay/recovery, ally, elections every 15 rounds, units of militaristic city-states, invasion, quests |
| 8e | Resolution | Congress | congress | once per round: founding (Printing Press and all met), special sessions by era, timer, session opening and result |
| 8a | Resolution | Faith | religion | sum of city faith → player modifiers → faith pool; Great Prophet roll (before the Industrial era, `classic/docs/classic/rules.md`) |
| 9 | Resolution | WorkerJobs | improvements | worker job tick, completed ones go onto the map |
| 9a | End | ConquestDecisions | combat | the unchosen fate of a captured city — puppet |
| 10 | End | TimedEffects | effects | minus one turn for timers that existed before the start |
| 10a | End | Victory | victory | after round 10: domination (all original capitals), science (spaceship parts), culture (influential over everyone), diplomacy (World Leader), time (best score in the last round) |
| 11 | End | TurnEnded | turn | journal entry |

After the turn of the round's last live player, before the round number grows,
the module's end of round runs (`RulesModule::end_round`; Classic, `classic/docs/classic/rules.md` §1.1):

| # | Step | System | What it does |
|---|---|---|---|
| 1 | ReligiousPressure | religion | holy cities press with their religion (5 × 60), each city with a religious majority — on cities within radius 10 (60, belief modifiers) |

Then the turn passes to the next live player, and their turn begins
with the `TURN_START_SEQUENCE` list:

| # | Phase | Step | System | What it does |
|---|---|---|---|---|
| 1 | Start | Fortification | combat | fortified units gain a fortification turn (up to 2) |
| 2 | Start | MoveOrders | movement | units with a multi-turn order (`goal`) continue; an enemy in the unit's sight, an occupied path or a lost route cancels the order |

City steps (`SCHEDULE.city`: the module's GreatPeople, then the kernel's Growth, Production, Borders; the city turn order is from
`classic/docs/classic/rules.md`): snapshot of the city's outputs (base and modifiers — before
any changes; the city's gold and beakers come from it as well) → Growth (food
surplus, growth) → Production (production stockpile, queue, OnBuilt/OnUnitTrained) →
Borders (city culture: tiles and modifiers, without culture per citizen —
into the city stockpile and the player pool; no more than one border level per turn,
the level grows even without a free tile, `classic/docs/classic/rules.md`).

Places of M4 in this list (checked against `classic/docs/classic/rules.md`): happiness —
before cities; golden age — before cities; great people — at the start of the city
steps; player culture and policies — after Gold; faith and religion — after
Research. A divergence from Classic kept for the sake of v1 behavior: unit
upkeep is deducted before income (in Classic — inside `DoGold`), healing and
movement are at the start of our sequence (in Classic — `doTurnUnits` after the
economy).

---

# 21. `knowledge`

This is the player's subjective knowledge of the world.

```rust
pub struct PlayerKnowledge {                 // knowledge/state.rs:174
    explored: TileSet,                       // the tile has been visible at least once
    memory: Chunked<TileMemory>,             // what the player saw last and in which round
    sightings: BTreeMap<u32, CitySighting>,  // cities seen last time (by tile)
    contacts: Vec<Contact>,                  // meeting with each player and the last time they were seen
}

pub enum KnownTile {
    Unknown,
    Remembered { snapshot: TileSnapshot, last_seen_round: u32 },
    Visible(TileSnapshot),
}
```

`TileSnapshot`: terrain, feature, resource (only if the player knows the
revealing technology), improvement, road, river, owner, city. Memory
stores the mutable layers (owner, improvement, road, feature, city);
terrain, rivers and resource placement do not change during a match and are read from
the map. `PlayerView` (`knowledge/view.rs`) is the filter between the truth
and the AI, the render projection and the HUD: the fields are private, it answers only with the player's knowledge.

> **TARGET (not yet true): the "single" filter.** `GameView.auth` is public, and the AI code
> reads the truth directly 128 times (`view().auth`: L&L 101 in 21 files, sandbox 10,
> Classic 9, the kernel's AI framework 8) against 73 mentions of `PlayerView` in the AI code. The ban
> on hidden information is held by non-interference tests, not by types. Being eliminated by the agent
> `ai-truth`, branch `ai/no-truth-reads`; row 21.2 of the conformance table.

Knowledge is written by a single module — `knowledge/sync.rs`, after each journal (after
derived visibility): an arriving observer memorizes the covered tiles,
a departing one sets the round of the last glance on tiles that stopped being visible;
a tile change is re-memorized by everyone who sees it; another player's unit or city
in view is a meeting (mutual) and "seen this round"; the end of
a player's turn updates "seen" for everything they see. Knowledge is saved
(schema 26) and is included in the hash.

## Contract

Separate:

```text
World truth
Visibility
Player knowledge
```

These are three different concepts.

---

# 22. `AIObservation`

The AI does not get `AuthWorld`.

> **TARGET (not yet true).** Today there is no unified `AIObservation`: `AiMind::start_turn` and
> `decide_unit` take `&GameState<M>` (`kernel/src/ai/mind.rs:40-69`). The kernel provides
> parts of the observation (`kernel/src/ai/observation.rs`: `OwnUnit`, `ObservedUnit`,
> `ObservedCity` and assembly functions), the Classic module assembles its own in
> `classic-rules/src/knowledge/observation.rs`. A single type instead of `&GameState` —
> rows 22.1, 53.3 of the conformance table, L, after `ai-truth`.

It gets (the shape from the design):

```rust
pub struct AIObservation<'a> {
    player: PlayerId,
    round: u32,
    view: PlayerView<'a>,                    // Unknown / Remembered / Visible
    own_units: Vec<OwnUnit>,                 // moves, work, fortification, movement order
    own_cities: Vec<OwnCity>,                // population, stockpiles, queue, buildings, worked tiles, income
    economy: OwnEconomy,                     // treasury, science, income, technologies, queue
    visible_foreign_units: Vec<ObservedUnit>,
    visible_foreign_cities: Vec<ObservedCity>,
    relations: Vec<Relation>,                // public diplomacy with those met
}
```

## Contract

The AI must not accidentally obtain hidden information. The non-interference test
(`tests/knowledge.rs`): two worlds with the same observable history and a different
hidden part give the same observations and commands.

---

# 23. `ai`

The AI stays inside the simulation: the framework (`driver`, `plan`, `map`, `trace`, `state`) is in
`kernel/src/ai/`, the planners are in the rules modules (`classic-rules/src/ai/`: 48 files,
`lal-rules/src/ai/`: 28, `sandbox-rules/src/ai.rs`). There is no separate AI crate.

API (today, `kernel/src/ai/mind.rs:40`; in the design — `plan(AIObservation, rules)`):

```rust
pub trait AiMind<M: RulesModule> {
    type Brief;
    fn start_turn(&self, game: &GameState<M>, rules: &CompiledRuleset<M>,
                  player: PlayerId, state: &mut AiState) -> TurnStart<M, Self::Brief>;
    fn plan_strategic(&self, brief: &Self::Brief, state: &mut AiState,
                      rules: &CompiledRuleset<M>) -> Vec<Decision<M>>;
    fn decide_unit(&self, game: &GameState<M>, player: PlayerId, unit: UnitHandle,
                   brief: &Self::Brief, state: &mut AiState,
                   tools: &mut PlanTools<'_, M>) -> Option<Decision<M>>;
    /* … */
}
```

The AI returns decisions that unfold into ordinary module commands (`Decision<M>`).

## Contract

The AI:

- does not mutate the simulation;
- does not get `&mut GameState`;
- speculative models belong to the AI;
- AI caches are not authoritative.

---

# 24. `persistence`

This is the **simulation save schema**, not a filesystem API.

For example:

```rust
pub struct SaveGameV1<M: RulesModule> {    // persistence/schema.rs:176
    header: SaveHeader,
    world: SavedWorldV1<M>,
}
```

## Contract

The kernel (`kernel`, `persistence`) can do:

```text
GameState → Save representation
Save representation → GameState
```

But it does not know:

```text
where to write the file.
```

This is the job of the host/client.

The save header stores `ruleset_hash = CompiledRuleset::hash()` — a
canonical fingerprint of the ruleset content (`rules/fingerprint.rs`): all
tables in key order, all fields, all effects, enums — with explicit
tags. Loading under a different ruleset gives `DecodeError::RulesetMismatch`.
A new definition field will not compile until it enters the fingerprint.

---

# 25. `replay`

Contains:

```text
initial state
commands
hash checkpoints
```

For example:

```rust
pub struct Replay<M: RulesModule> {         // replay/log.rs:12
    pub initial: SaveGameV1<M>,
    pub entries: Vec<ReplayEntry<M>>,
}
```

## Contract

Replay works only with a compatible:

```text
simulation version
ruleset
```

---

# 26. `validation`

A debug/test subsystem.

```rust
pub fn validate_world(
    state: &GameState,
    rules: &CompiledRuleset,
) -> Result<(), ValidationError>;
```

Checks:

```text
indexes
handles
ownership
occupancy
foreign keys
turn invariants
```

It may be expensive.

---

# 27. `scenario`

Creates reproducible artificial worlds.

```rust
ScenarioConfig {                            // scenario/generator.rs:16
    seed, players, width, height, cities, units, water_ratio,
}
```

Used at the same time for:

```text
tests
benchmarks
profiling
manual debug
```

This is an important module, not benchmark-only code.

---

# 28. `classic-content`

Structure:

```text
classic-content/src/
├── lib.rs, manifest.rs, loader.rs, error.rs
├── ron.rs, ron_types.rs, ron_roles.rs     RON schema
├── core.rs, classic.rs, nations.rs, effects.rs, stats.rs, natural_wonders.rs   data and ruleset assembly
├── import/, bin/                          importer and utilities
└── legacy.rs
```

(Art loading is not here: `client/src/shell/art*.rs`. Other modules have their own
content crates: `lal-content`, `sandbox-content`.)

Purpose:

```text
disk / HTTP / embedded assets
          ↓
     source files
          ↓
    SourceRules
```

Then:

```text
SourceRules
    ↓ kernel::rules::compile
CompiledRuleset
```

## Contract

`classic-content` knows about:

```text
files
paths
RON/JSON
asset manifests
```

The kernel and the rules crates (`kernel`, `*-rules`) do not: there is no `std::fs` in them.

---

# 29. Why the rules compiler stays in core

Because:

```text
"what counts as a correct ruleset"
```

is part of the simulation semantics.

While:

```text
"how to read RON from disk"
```

is infrastructure.

Therefore:

```text
parse RON          → classic-content
semantic validation → kernel
```

---

# 30. `render`

Structure:

```text
render/src/                      111 files, 37.7k lines; depends only on kernel
├── lib.rs, renderer.rs
├── world/        state.rs (RenderWorld), projection.rs (+ projection/)
├── camera/       mod.rs, controller.rs
├── scene/        terrain/, water, units, cities, borders, fog, overlays, picking, heightfield, layout, style
├── gpu/          context, buffers, textures, pipelines, device_recovery, + 35 files
│                 (terrain, coasts, rivers, shadows, fog, unit batches, memory, budget, measurements)
└── models.rs, unit_models.rs, world_models.rs, model_geometry.rs, ground.rs, debug_view.rs
```

---

# 31. `RenderWorld`

CPU-side renderer projection.

```rust
pub struct RenderWorld {
    terrain: TerrainRenderState,
    units: UnitRenderState,
    cities: CityRenderState,
    fog: FogRenderState,
    ...
}
```

## Contract

RenderWorld:

```text
is not authoritative
can be destroyed
can be rebuilt
```

The renderer does not write back to GameState.

---

# 32. `projection`

Receives:

```text
Game read model
+
ChangeJournal
```

and updates RenderWorld.

```rust
// render/src/world/projection.rs:400
pub fn apply_changes<M: RulesModule>(
    render_world: &mut RenderWorld,
    game: GameView<'_, M>,
    changes: &ChangeJournal<M>,
    rules: &CompiledRuleset<M>,
) -> LayerSet;     // which layers changed
```

Also required:

```rust
// projection.rs:326
pub fn rebuild_render_world<M: RulesModule>(
    game: GameView<'_, M>, observer: PlayerId, rules: &CompiledRuleset<M>,
) -> RenderWorld;
```

For correctness tests.

---

# 33. GPU layer

`gpu/` works only with RenderWorld/assets.

It must not import:

```text
WorldTransaction
Command
AI
Diplomacy logic
```

GPU state is fully reconstructable.

---

# 34. `client`

This is the orchestration layer.

```text
client/src/                      96 files, 23.2k lines; depends on kernel, render, i18n
├── lib.rs, game.rs (GameClient), controller.rs, input.rs, selection.rs, overlays.rs,
│   ui_model.rs, simulation_driver.rs, ai_round.rs (AI worker thread), perf.rs
└── shell/        application shell without mechanics: app, screens, HUD, settings, saves,
                  presentation, observer, snapshot, art_loader, module.rs (ModuleUi), …
```

Here the following come together:

```text
input
simulation
AI
render projection
renderer
UI
```

---

# 35. `GameClient`

Roughly:

```rust
pub struct GameClient<M: RulesModule> {     // client/src/game.rs:17
    game: GameState<M>,
    rules: Arc<CompiledRuleset<M>>,
    renderer: Renderer,
    render_world: RenderWorld,
    controller: Controller,
    /* overlays, selection, observer, journal */
}
```

## Contract

It is the client that has the right to orchestration:

```text
input
 ↓
Command
 ↓
simulation.execute()
 ↓
ChangeJournal
 ↓
RenderWorld update
 ↓
render
```

---

# 36. Very important: `client` is not a simulation layer

Functions like these must not appear there:

```text
calculate_city_yield()
resolve_combat()
can_unit_enter_tile()
```

That is the kernel and the rules modules (`kernel`, `*-rules`).

The client is responsible only for:

```text
what to run
when to show it
how to pass it on
```

---

# 37. `simulation_driver`

Especially important for WASM.

It manages:

```text
simulation step
AI scheduling
time slicing
background computation
```

But the core stays synchronous.

For example:

```text
client tick
 ├─ process input
 ├─ advance pending simulation work
 ├─ apply completed AI result
 ├─ update render projection
 └─ render
```

---

# 38. Desktop app

```text
lands/launcher/desktop/src/    main.rs, lib.rs, module.rs (rules module selection), audio.rs, visibility*.rs
```

Responsible only for:

```text
window
event loop
filesystem
native threads
startup
shutdown
```

It may use:

```text
winit
wgpu
```

But there is no gameplay logic here.

---

# 39. Web app

```text
lands/launcher/web/src/        lib.rs, page.rs, assets.rs
lands/launcher/android/src/    lib.rs, host.rs, platform.rs   (third app; depends on desktop and the interface crates)
```

Responsible for:

```text
canvas
browser event loop
WASM bindings
browser storage
optional workers
```

## Contract

Browser-specific types do not leak below `client` (`web-sys`, `wasm-bindgen`, `js-sys` exist only in `lands/launcher/web`).

---

# 40. The main read API

It is very important not to hand out to everyone:

```rust
&GameState
```

I would gradually introduce restricted read views.

For example:

```rust
pub struct GameView<'a, M: RulesModule> {   // world/state.rs:80
    pub auth: &'a AuthWorld<M>,             // pub — see TARGET in section 6
    pub derived: &'a DerivedWorld<M>,
}
```

Beyond the original design there is `PlayerView` — reading filtered by the player's knowledge (section 21).

And narrower ones:

```rust
UnitQuery<'a>
CityQuery<'a>
MapQuery<'a>
```

But **do not build an abstract query framework in advance**. (They are not built; `AuthWorld`
has reader methods `map()`, `units()`, `cities()`…; capability traits
are proposed by `platform/docs/kernel-audit.md`, C.3.)

---

# 41. The main write API

For the outside world there is one write boundary:

```rust
game.execute(command)
```

Inside the simulation, these are allowed:

```text
WorldTransaction operations
```

But not public mutable fields.

> **TARGET (not yet true).** Next to `execute` there is today a second write point —
> `GameState::world_mut()` and 31 `debug_*` methods (for scenarios, tests and
> development tools), plus the `pub` fields of `AuthWorld`. Removed by `world-builder`
> (`refactor/world-builder`); row 41.2 of the conformance table. For rules modules
> the write path is `WorldTransaction`, not `world_mut`.

---

# 42. The full command pipeline

```text
UI / AI
   │
   ▼
Command
   │
   ▼
command::validate
   │
   ▼
ResolvedCommand
   │
   ▼
WorldTransaction
   │
   ├── update primary storage
   ├── update authoritative indexes
   │
   ▼
commit
   │
   ▼
ChangeJournal
   │
   ├── derived.apply()
   ├── knowledge.apply()
   └── return CommandResult
             │
             ▼
          client
             │
             ▼
      render projection
```

The "validate → ResolvedCommand → WorldTransaction" frame is executed inside the module's
`RulesModule::execute` (section 1); `GameState::execute` calls it, then
`commit_journal` (derived, then knowledge), and returns `CommandResult`.

---

# 43. Where the transaction boundary lies

Do not make a transaction for every primitive mutation.

For example:

```text
CaptureCity
```

may internally:

```text
change city owner
change tiles
cancel trade routes
relocate units
change diplomacy-derived state
```

All authoritative changes go into **one logical commit**.

Downstream systems then see an already consistent world.

---

# 44. Where the derived boundary lies

Authoritative:

```text
unit.position
city.population
city.buildings
player.technology
tile.owner
```

Derived:

```text
city.production_per_turn
player.total_science
tile.visible_to_player
trade connectivity
movement caches
```

The main question:

> Can the value be fully reconstructed from authoritative state + rules alone?

If yes, it is a candidate for derived.

---

# 45. Exception: historical knowledge

For example:

```text
player remembers enemy city
```

cannot be reconstructed from the current world.

So this is not a derived cache.

This is real state:

```text
PlayerKnowledge
```

and it must be saved.

---

# 46. Dependency direction contracts inside core

Preferably:

```text
ids
 ↑
rules
 ↑
world
 ↑
transaction / command
 ↑
derived / navigation / knowledge
 ↑
AI
```

But do not make this a rigid crate-level dependency graph.

Rust modules may use each other's domain types through careful APIs.

The main thing is to avoid:

```text
world imports AI
world imports renderer
rules imports world state
```

Cross-check against the code (row 46 of the conformance table): `world` does not import AI or the renderer;
but `world/{territory,builds,facts,building_rules}.rs` import `derived::tile_yields`,
`knowledge::view` and `systems::ModuleSystems` (upward references), and `rules` in two places
reaches into `world` (a constant and the `ModuleWorld` trait, not state).

---

# 47. `rules` must not know the current World

Bad:

```rust
rules.calculate_bonus(&GameState)
```

Better, rules are data:

```rust
CompiledModifier
```

And the gameplay subsystem interprets them in a concrete context.

Otherwise the rules engine quickly turns into a second simulation engine.

---

# 48. Do not create `services/`

Do not introduce a directory:

```text
services/
    UnitService
    CityService
    CombatService
```

if it is just a set of arbitrary methods.

Better to use domain modules:

```text
combat/
city/
navigation/
```

with clear input/output.

---

# 49. When to extract a new crate

Only if at least one of the following holds:

1. the module is needed independently by several binaries;
2. it has its own heavy dependency tree;
3. it has a stable API boundary;
4. the compile blast radius needs to be reduced significantly;
5. there is an independent lifecycle/platform boundary.

Do not extract a crate merely because a file has grown.

---

# 50. Likely evolution

Later it may make sense to extract:

```text
nation-ai
```

if the AI becomes large. (Not done: the AI is 48 + 28 files in the module crates plus a skeleton in the
kernel; `platform/docs/kernel-audit.md` proposes the "player agent" shape, PlayerAgent.)

Possibly:

```text
nation-sim
```

if the kernel starts to include too much persistence/scenario infrastructure.
(Not done; `platform/docs/kernel-audit.md`, section C proposes `core` + `std4x` with `kernel`
as a facade; `docs/split-plan.md` describes four repositories.)

Possibly:

```text
nation-protocol
```

if multiplayer appears. (Done: `net` is the protocol and match logic without I/O,
`net-server` is the tokio-based server; `platform/docs/multiplayer.md`.)

---

# 51. Summary of contracts

| Module | Owns | May change World | May depend on GPU/platform |
|---|---|---:|---:|
| `rules` | rules | no | no |
| `world` | authoritative data | only via transaction | no |
| `command` | intent + validation | no | no |
| `transaction` | mutation | **yes** | no |
| `derived` | rebuildable caches | no authoritative | no |
| `navigation` | path queries | no | no |
| `knowledge` | player knowledge | via controlled resolution | no |
| `ai` | planning | no | no |
| `persistence` | save representation | no | no |
| `render` | RenderWorld + GPU | no | yes, wgpu |
| `content` | source/assets loading | no | filesystem/web fetch abstraction |
| `client` | orchestration | via `Command` | yes |
| desktop/web/android | platform runtime | via client | yes |

Cross-check (platform/docs/structure-conformance.md, row 51): the table is correct for the modules; the exceptions are
the UI development tools, which change the world through `debug_*`/`world_mut` (TARGET in section 41).

---

# 52. The most important architectural prohibitions

If during implementation you see this appear:

```rust
renderer.game_state_mut()
```

the architecture is violated.

If this appears:

```rust
ai.world_mut()
```

the architecture is violated.

If a command does:

```rust
changes.invalidate_visibility = true;
```

the boundary is violated.

If gameplay does:

```rust
wgpu::...
```

the boundary is violated.

If the renderer computes:

```text
whether a unit can attack
```

the boundary is violated.

If save serializes:

```rust
UnitStore
```

directly as a permanent format, the boundary is violated.

If the UI changes:

```rust
city.production
```

bypassing `Command`, the boundary is violated.

### Checking the prohibitions against the code (21 October 2026)

Each prohibition was checked with a command (the list and the commands are in
platform/docs/structure-conformance.md, sections 52.1–52.14;
some are checked by `sh platform/tools/check-structure.sh`):

| Prohibition | Result |
|---|---|
| `renderer.game_state_mut()` | no: `render` has no `&mut GameState` |
| `ai.world_mut()` | no outside tests; **but** the AI reads the truth through `view().auth` 128 times (TARGET, `ai-truth`) |
| `invalidate_visibility` in the journal | no: the kernel has no `Invalidate*` |
| `wgpu::` in gameplay | no: the simulation crates have no `wgpu`, `winit`, `egui`, `web-sys` (script) |
| the renderer decides "can the unit attack" | no |
| save writes `UnitStore` as the format | no: `SavedWorldV1` records and a byte codec |
| the UI changes `city.production` bypassing `Command` | **violated in the development tools** (`debug_*`, `world_mut`); TARGET, `world-builder` |
| public mutable fields | **violated**: `AuthWorld` (13 `pub` fields), `WorldTransaction` |
| global state | no `static mut`; only the effects cache (`OnceLock`, `AtomicU64`) and AI handles for test benches |
| time (`Instant`, `SystemTime`) in the simulation | no |
| `rand` outside `kernel/src/rng.rs` | no |
| hash-table iteration order affects the result | no: tables with a fixed hasher, iteration only in checks |
| string lookups in the hot path | no |
| a `services/` directory | no |

---

# 53. Practical starting API

I would aim for the first usable API to look roughly like this:

```rust
let rules = compile_rules(source_rules)?;

let mut game = GameState::new(
    &rules,
    GameConfig::default(),
);

let result = game.execute(
    &rules,
    Command::MoveUnit {
        unit,
        destination,
    },
)?;

// in the code: a free function, not a RenderWorld method
projection::apply_changes(
    &mut render_world,
    game.view(),
    result.changes(),
    &rules,
);
```

For the AI:

> **TARGET (not yet true):** `game.ai_observation(player)` and `ai.plan(observation, &rules)`
> do not exist; today the AI driver (`kernel::ai::driver::AiDriver`) calls
> `AiMind` with `&GameState` (section 22, row 53.3 of the correspondence table).

```rust
let observation =
    game.ai_observation(player);

let commands =
    ai.plan(observation, &rules);

for command in commands {
    game.execute(&rules, command)?;
}
```

For correctness:

```rust
// in the code there is a single check: world invariants and derived == rebuild
game.validate(&rules)?;
// DerivedWorld::assert_matches_rebuild — pub(crate), called by validate
```

This is the main shape of the architecture: a small public API with hard write boundaries, rather than hundreds of service interfaces.
