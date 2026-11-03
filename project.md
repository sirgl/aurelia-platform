# Technical specification: the architectural foundation of a 4X strategy in Rust + WASM

> **The original technical specification (12 October 2026), the law for the Classic mechanics.** The crate names in it are historical (`nation-core`, `nation-render`, `classic-content`): today's layout is in [platform/structure.md](structure.md), section 1, and platform/PLATFORM.md. Status and next steps are in ru/PLAN.en.md.

## 1. Goal

Design and implement the architectural foundation of a full turn-based 4X strategy at the level of Classic:

- Rust as the main language;
- native desktop and the browser through WASM;
- 3D rendering through `wgpu` / WebGPU;
- up to 8 active players;
- large maps and a heavy late game;
- good extensibility of gameplay mechanics;
- deterministic authoritative simulation;
- performance and memory are architectural requirements;
- incremental computation is applied where it is proven useful;
- the architecture must remain testable and maintainable.

Design the system not as a UI application with the game logic inside, but as:

```text
deterministic simulation engine
+
rules/content system
+
AI
+
render projection
+
3D client
```

---

# 2. Main target scenarios

Use several different performance scenarios.

## 2.1 Representative late game

The baseline scenario the architecture must be convenient for:

```text
8 players
20k–50k tiles
100–300 cities
500–1500 units
large explored map
roads
borders
fog of war
trade
policies
technologies
diplomacy
```

Do not treat the exact values as a contract until real gameplay exists.

---

## 2.2 Stress scenario

An artificial heavy scenario:

```text
50k+ tiles
8 players
500 cities
5,000 units
dense diplomacy
large road network
large visibility overlap
```

It is meant to detect algorithmic problems.

Do not optimize the whole architecture for this scenario alone.

---

## 2.3 Torture scenarios

Separate fixed scenarios:

```text
MassiveBorderChange
LargeWarFront
DenseRoadNetwork
NavalWorld
PathfindingMaze
PolicyAffectsAllCities
PlayerElimination
MassVisibilityChange
```

---

# 3. Non-goals of the first stage

At the first stage the following are NOT required:

- a full multiplayer;
- arbitrary runtime Rust plugins;
- dynamic libraries;
- user WASM gameplay plugins;
- complex HPA* navigation;
- a fully parallel simulation;
- a universal reactive framework;
- a universal event bus;
- a universal ECS;
- a global system of automatic dependency graphs.

Do not build an architecture that makes these things impossible, but do not pay their complexity in advance.

---

# 4. The main architectural principle

The authoritative simulation is completely independent of:

```text
renderer
wgpu
WASM
DOM
UI
animation
audio
platform APIs
```

The simulation must be runnable:

```text
cargo test
native benchmark
headless executable
WASM host
```

without a GPU.

---

# 5. Main architecture

```text
                    Source Rules
                         │
                         ▼
                  Rules Compiler
                         │
                         ▼
                  CompiledRuleset
                         │
                         ▼

 UI / Human / AI
        │
        ▼
     Command
        │
        ▼
 Command Validation
        │
        ▼
 ResolvedCommand
        │
        ▼
 WorldTransaction
        │
        ▼
      Commit
        │
        ▼
  ChangeJournal
        │
        ├───────────────┐
        ▼               ▼
 Derived State     Render Projection
        │               │
        ▼               ▼
 Simulation          RenderWorld
                            │
                            ▼
                           wgpu
```

---

# 6. Authoritative state

Authoritative state contains only the data that defines the game truth.

Example:

```rust
pub struct GameState {
    auth: AuthWorld,
    derived: DerivedWorld,
}

struct AuthWorld {
    map: MapState,
    players: PlayerStore,
    cities: CityStore,
    units: UnitStore,
    diplomacy: DiplomacyState,
    turn: TurnState,
}
```

Critically important:

```text
AuthWorld does not give free mutable access to external systems.
```

Not allowed:

```rust
world.units[i].owner = player;
```

All changes go through controlled domain operations / the transaction API.

---

# 7. Single-owner mutation

For authoritative mutable state there is one logical owner.

Forbidden:

```text
AI thread -> mutate World
renderer -> mutate World
worker -> mutate World
UI -> mutate World
```

Allowed:

```text
read immutable state
→ compute result
→ send result
→ deterministic single-owner commit
```

The basic concurrency pattern:

```text
Immutable Snapshot
       │
 ┌─────┼────────┐
 ▼     ▼        ▼
AI   path     calculations
 │     │        │
 └─────┴────────┘
       │
       ▼
isolated results
       │
       ▼
deterministic merge
       │
       ▼
single-owner commit
```

---

# 8. Transactional mutation

A command must not leave the World in a partially modified state.

Flow:

```text
Command
  ↓
validate
  ↓
ResolvedCommand
  ↓
WorldTransaction
  ↓
apply all authoritative mutations
  ↓
validate commit invariants
  ↓
commit
  ↓
ChangeJournal
```

Example of an interface:

```rust
fn execute(
    state: &mut GameState,
    command: Command,
) -> Result<CommandResult, CommandError>;
```

Inside:

```rust
let resolved = validate(state.view(), command)?;

let mut tx = state.begin_transaction();

apply_resolved(&mut tx, resolved)?;

let changes = tx.commit()?;
```

---

# 9. ChangeJournal

`ChangeJournal` describes the **actual changes of the authoritative state**.

Not downstream invalidations.

Good:

```text
UnitMoved
UnitCreated
UnitDestroyed
CityOwnerChanged
BuildingAdded
TileImprovementChanged
DiplomaticStatusChanged
TechnologyUnlocked
```

Bad:

```text
InvalidateVisibility
InvalidateAI
InvalidateRenderer
InvalidateScience
```

Each derived subsystem itself knows which authoritative changes matter to it.

---

# 10. A ban on a generic event bus as the basis of the simulation

Do not build the architecture:

```text
A emits event
→ arbitrary subscriber B
→ emits C
→ arbitrary subscriber D
```

The gameplay flow must remain explicitly readable.

ChangeJournal is data about a commit, not a universal callback system.

---

# 11. Derived state

Derived state contains computable data:

```text
city yields
empire totals
visibility
trade connectivity
movement indexes
AI caches
threat maps
render projections
```

The main invariant:

> Any derived state must be recoverable from the authoritative state and the CompiledRuleset.

Derived state must not be the only place where important game information is stored.

---

# 12. Full rebuild oracle

For substantial derived subsystems a correct full rebuild is mandatory.

For example:

```rust
fn rebuild_visibility(
    world: &AuthWorld,
    rules: &CompiledRuleset,
) -> VisibilityState;
```

and:

```rust
fn apply_visibility_changes(
    visibility: &mut VisibilityState,
    changes: &ChangeJournal,
    world: &AuthWorld,
    rules: &CompiledRuleset,
);
```

In tests:

```text
incremental update
vs
full rebuild
```

must give an identical logical result.

---

# 13. Incrementality is an optimization, not a source of truth

Do not introduce the rule:

```text
everything must be incremental
```

Use three strategies:

```text
incremental
regional rebuild
full rebuild
```

A subsystem must be free to choose the cheaper option.

Example:

```rust
if dirty_ratio < threshold {
    apply_incremental();
} else {
    rebuild();
}
```

The threshold is determined by benchmarks.

---

# 14. Do not forbid O(N)

A full pass over a dense array:

```rust
for tile in map.tiles() { ... }
```

can be cheaper than a complex system of dirty queues.

The rule:

> Avoid unnecessary work proportional to the size of the world in hot operations, but choose the algorithm on the basis of measurements.

---

# 15. Data layout

Use data-oriented design, but do not turn pure SoA into dogma.

For example, the following is acceptable:

```rust
struct UnitCore {
    tile: TileId,
    owner: PlayerId,
    kind: UnitKindId,
    hp: u16,
}
```

and:

```rust
Vec<UnitCore>
```

If profiling shows a benefit of SoA/AoSoA - move the hot fields.

Do not create long parallel arrays only for the sake of theoretical cache locality.

---

# 16. Tile storage

For map tiles dense storage is preferable.

```rust
#[repr(transparent)]
pub struct TileId(pub u32);
```

Tile data:

```text
terrain
height
feature
resource
ownership
improvement
```

store in compact structures indexed by `TileId`.

Do not use in the hot path:

```rust
HashMap<HexCoord, Tile>
```

as the main representation of the map.

---

# 17. Runtime handles and persistent IDs

Do not mix:

```text
runtime memory handle
persistent identity
content identity
```

For example runtime:

```rust
struct UnitHandle {
    index: u32,
    generation: u32,
}
```

For persistent references, if needed:

```rust
struct EntityId(u64);
```

The save/replay format must not depend on an object's position in a runtime dense array.

---

# 18. Content IDs

Source content uses stable keys:

```text
core:unit/warrior
core:building/library
mod.example:unit/foo
```

Runtime after compiling the ruleset:

```rust
UnitKindId(u16)
BuildingKindId(u16)
```

A save stores stable keys or a stable versioned mapping, not randomly assigned runtime IDs.

---

# 19. Rules compilation

Content pipeline:

```text
RON/JSON/etc.
      ↓
parse
      ↓
schema validation
      ↓
reference resolution
      ↓
stable ID validation
      ↓
effect compilation
      ↓
runtime indexes
      ↓
CompiledRuleset
```

The runtime simulation works with the `CompiledRuleset`.

The rules compiler must catch:

```text
unknown references
duplicate stable IDs
invalid modifier
invalid selector
bad trigger
dependency cycles where forbidden
invalid stacking
```

before the game starts.

---

# 20. Level of extensibility

At the first stage the following are supported:

## Level 1

Data-driven content:

```text
units
buildings
technologies
resources
nations
terrain
```

## Level 2

Declarative modifiers:

```text
AddYield
MultiplyYield
UnlockUnit
ModifyCost
ModifyCombat
ModifyMovement
```

## Level 3

Limited typed triggers:

```text
OnTurnStart
OnTechnologyUnlocked
OnUnitKilled
OnCityFounded
```

Runtime arbitrary-code mods are out of scope.

---

# 21. Effect system

Do not build a Turing-complete language at once.

Explicit ones are needed:

```text
scope
selector
condition
trigger
modifier
duration
stacking
ordering
```

Also define a single order of evaluation.

For example:

```text
base value
↓
flat modifiers
↓
additive percentage modifiers
↓
multiplicative modifiers
↓
caps
↓
rounding
```

The order is part of the gameplay specification.

---

# 22. Turn model

Introduce an explicit `TurnState` at once.

Do not let systems interpret the notion of a turn on their own.

The minimal baseline:

```text
TurnStart
PlayerActionPhase
Resolution
TurnEnd
```

The exact Nation-like model may evolve.

Any feedback loops must be resolved by a formal phase order, not by the random order of callbacks.

---

# 23. Determinism

The authoritative simulation must be deterministic given the same:

```text
initial state
ruleset
command sequence
simulation version
RNG seed/state
```

Requirements:

```text
stable iteration where result depends on ordering
explicit rounding rules
explicit command resolution order
deterministic RNG
no wall clock inside simulation
no renderer-derived authoritative values
```

`f32` is allowed in the renderer.

Do not forbid floats globally.

---

# 24. RNG

Do not use one chaotically consumed global RNG.

Prefer independent logical streams:

```text
CombatRng
WorldEventRng
MapGenerationRng
```

or a deterministic counter-based / random-key approach where it is useful.

A change to one mechanic must not, without need, change the random results of a completely different system.

---

# 25. World hash

For debug/replay compute a canonical hash of the authoritative state.

Do not include:

```text
derived caches
renderer
temporary AI state
scratch buffers
```

The hash traversal must have a stable order.

---

# 26. Save format

Do not serialize internal Rust structs directly as a long-term format.

Use a separate versioned save schema.

A save must contain at least:

```text
save schema version
simulation version
ruleset hash
mod/content manifest
authoritative state
RNG state
```

A migration strategy may appear later.

---

# 27. Replay

The replay of the first stage:

```text
initial snapshot
+
command log
+
periodic authoritative hashes
```

Do not guarantee replay reproducibility between incompatible versions of the simulation.

This must be an explicit limitation.

---

# 28. AI access

The AI must not automatically see the full authoritative world.

A layer is needed:

```text
AuthWorld
  ↓
KnowledgeState[player]
  ↓
AIObservation
  ↓
AI Planner
```

AI output:

```text
normal Commands
```

like a human.

The AI planning model may differ from the authoritative simulation.

Do not require cloning the full World for speculative planning.

---

# 29. Map indexes

Provide specialized secondary indexes:

```text
units_by_player
units_by_tile
cities_by_player
observers_by_region
trade_routes_by_city
```

But the mutation API is obliged to maintain their consistency centrally.

Forbid direct writes into indexed fields.

---

# 30. WorldValidator

Implement a debug/test validator.

It must be able to check:

```text
valid entity references
secondary index consistency
ownership consistency
tile occupancy invariants
city/player relations
trade route references
no dangling IDs
```

Validation can be expensive and run only in test/debug modes.

---

# 31. Property testing / fuzzing

Create tests of the kind:

```text
generate valid world
generate valid random commands
execute thousands of commands
validate state after every commit
```

Also:

```text
incremental derived
==
full rebuild derived
```

after random sequences of changes.

---

# 32. Visibility

Baseline:

```text
visibility_count[player][tile]
seen[player][tile]
```

When a unit moves, update the local area.

But the architecture must support changes:

```text
LOS blocker
terrain
elevation
shared vision
promotion
observer destroyed
```

Do not assume that visibility depends only on a unit's position.

If needed, introduce a spatial index of observers.

---

# 33. Navigation

The first implementation:

```text
A*
dense tile arrays
good heuristic
reusable scratch memory
```

Do not implement HPA* in advance.

Separate conceptually:

```text
static traversal constraints
slow-changing traversal data
fast dynamic occupancy/ZOC
```

After benchmarks, add if needed:

```text
region graph
portal graph
hierarchical navigation
```

---

# 34. Scratch memory

Reusable scratch memory is allowed.

But such techniques as:

```text
generation counters instead of fill()
```

introduce only after a benchmark.

Do not treat them as an architectural requirement.

---

# 35. Parallelism

The base simulation must be fast enough single-threaded.

Parallelism is an optimization layer.

Do not create an abstraction:

```text
SequentialExecutor / RayonExecutor / WebWorkerExecutor
```

as if they had the same semantics.

Prefer:

```text
pure computation kernels
+
explicit orchestration
```

---

# 36. WASM threading

The game must work correctly without a mandatory:

```text
SharedArrayBuffer
cross-origin isolation
WASM threads
```

Threads can be an additional speed-up.

Do not build the correctness of the simulation around browser thread support.

---

# 37. Browser responsiveness

Throughput is insufficient.

Measure:

```text
main-thread stall
frame latency
p95/p99 frame time
simulation chunk latency
```

Do not allow large synchronous blocks of the browser UI.

If the simulation stays on the main thread, heavy computations must be able to yield cooperatively or later move to a worker.

---

# 38. Renderer architecture

The renderer does not read the mutable `AuthWorld`.

Use:

```text
ChangeJournal
   ↓
RenderProjection
   ↓
RenderWorld
```

`RenderWorld` is a disposable projection, not gameplay truth.

GPU resources are also considered disposable.

After a GPU device loss the renderer must have the architectural ability to recreate the GPU state from the CPU-side RenderWorld/assets.

---

# 39. Renderer layers

Minimal independent layers:

```text
Terrain
Water
Features
Improvements
Roads
Borders
Units
Fog
Selection
Strategic overlays
Labels
UI
```

The UI can switch the display of a layer on and off without mutating the simulation state.

---

# 40. Rendering performance metrics

Measure not only FPS.

Also:

```text
CPU render preparation
GPU frame time
draw calls
visible instances
GPU uploads/frame
pipeline switches
visible labels
shadow draw calls
temporary CPU allocations
```

---

# 41. LOD

The RenderWorld architecture must allow at least three presentation modes:

```text
close tactical
medium
strategic
```

It is not necessary to implement a full LOD at once.

But a gameplay entity must not be rigidly bound to a single render mesh representation.

---

# 42. UI boundary

Do not define the DOM/Rust UI in advance as an architectural dogma.

But if JS/DOM is used:

- do not make per-tile WASM->JS calls;
- pass data in batches;
- minimize boundary crossings.

---

# 43. Performance philosophy

Performance is a mandatory part of development.

But do not use rules of the kind:

```text
0 allocations everywhere
never O(N)
all operations <1 ms
```

Instead measure:

```text
time
algorithmic work
memory
allocations
latency distribution
```

---

# 44. Performance scenario harness

Create an own reusable scenario harness, independent of Criterion.

For example:

```rust
Scenario::RepresentativeLateGame
Scenario::StressLateGame
Scenario::MassiveBorderChange
```

The benchmark framework must run the same scenarios.

---

# 45. Performance metrics

For a subsystem collect:

```text
wall time
p50 / p95 / p99 where applicable
entities visited
tiles visited
nodes expanded
cities recomputed
dirty items processed
bytes allocated
peak temporary memory
```

---

# 46. Allocation rule

Do not require an absolute zero of allocations.

The hard rule:

> There must be no hidden unbounded allocation or allocation-per-element in hot loops.

Steady-state zero-allocation can be used where it is proven useful.

---

# 47. Memory model

Memory usage is a first-class benchmark.

Measure separately:

```text
AuthWorld memory
DerivedWorld memory
AI memory/player
RenderWorld memory
temporary memory
GPU memory estimate
```

Especially control maps of the kind:

```text
tiles × players × metric
```

which easily create tens of megabytes.

---

# 48. Benchmark levels

## Micro

Only for truly hot primitives.

For example:

```text
tile lookup
LOS calculation
A* inner loop
modifier evaluation
```

## Subsystem

For example:

```text
visibility unit move
city recomputation
policy change
road removal
```

## Scenario

```text
representative late game
large war
mass border update
```

---

# 49. CI policy

Do not fail on a random wall-clock regression of `+3%` on ordinary shared CI.

Separate:

```text
correctness CI
algorithmic regression CI
performance measurement
```

For example, work counters can have hard bounds:

```text
single-unit move must not scan all 50k units
```

Use wall-clock regression:

```text
dedicated runner
or informational comparison
```

---

# 50. Workspace

Do not split the project into 15 crates in advance.

The initial variant:

```text
crates/
    nation-core/
        world/
        transaction/
        commands/
        rules/
        derived/
        navigation/
        ai/

    nation-render/
        model/
        wgpu/

    classic-content/

    nation-platform/

apps/
    desktop/
    web/

benches/
scenarios/
assets/
```

Extract a new crate only when the boundary is really stable and useful.

---

# 51. The first vertical slice

Implement a minimal game that verifies the architecture.

Composition:

```text
hex map
terrain
8 players
units
cities
ownership
movement
basic A*
visibility/fog
one resource
one building
one technology
one policy modifier
basic combat
turn state
3D renderer
selection
basic overlay
save
replay
```

---

# 52. Synthetic late game from the first stage

Even while the mechanics are simple, create a world generator:

```text
50k tiles
8 players
hundreds of cities
thousands of units
```

so that the architecture is constantly checked at a size close to the target.

---

# 53. Mandatory benchmark cases of the first vertical slice

## Map

```text
dense tile iteration
coordinate -> TileId
TileId -> coordinate
```

## Units

```text
create
move
destroy
bulk iteration
```

## Visibility

```text
full rebuild
single unit move
100 moves
terrain LOS change
```

## Navigation

```text
short path
long path
blocked path
100 path requests
stress map
```

## City

```text
full yield rebuild
single tile changed
building added
policy affecting one city
policy affecting all cities
```

### A city as in Classic (target slice, after territory/overlay)

```text
[x] city name (named at founding: the owner's City N)
[x] city screen (window: name, citizens, food, production, queue, orders)
[x] population growth (food surplus -> +1, growth_cost = 10+4*pop)
[x] production queue (Enqueue/Clear, overflow, buildings with unlocks)
[ ] citizen management (assigning citizens to ring tiles)
[ ] territorial expansion (expanding the rings by culture)
[ ] resource contribution (the city's contribution to the empire: yields/tax/science)
```

The queue replaces the warrior auto-spam (`produce_city` currently hardcodes
WARRIOR_COST): `Command::SetProduction` + validate + journal + UI buttons.
The founding territory (ring = work area) already exists - the foundation for
citizen management and expansion.

## Renderer

```text
visible map
units
fog update
large camera movement
overlay toggle
```

---

# 54. Mandatory correctness tests

For every incremental derived subsystem:

```text
incremental update
==
full rebuild
```

For a transaction:

```text
invalid command changes nothing
```

For deterministic simulation:

```text
same snapshot
+
same commands
=
same authoritative hash
```

For save/load:

```text
save
load
=
same authoritative state
```

For the renderer projection:

```text
incremental RenderWorld
==
full RenderWorld rebuild
```

where the comparison is possible at the logical level.

---

# 55. Definition of Done for the architectural stage

The stage is considered complete when:

1. The desktop build works.
2. The Web/WASM build works without mandatory threads.
3. The simulation does not depend on rendering/platform crates.
4. Authoritative mutation is closed by the transaction API.
5. There is a `ChangeJournal`.
6. At least visibility and city yields have:
   - full rebuild;
   - incremental update;
   - differential test.
7. There is a deterministic replay test.
8. There is a canonical authoritative hash.
9. There is a versioned save representation.
10. There is a CompiledRuleset.
11. There is a synthetic late-game generator.
12. There is a scenario benchmark harness.
13. There are memory/work counters.
14. There is a 3D map on `wgpu`.
15. The renderer uses a separate RenderWorld.
16. The AI receives a filtered `AIObservation`.
17. The stress scenario does not reveal an architectural scan of the world on every small action.
18. There is no mandatory shared mutable multithreading model.

---

# 56. Forbidden shortcuts

Do not allow:

```text
public mutable stores
```

```text
UI directly modifying World
```

```text
renderer querying arbitrary gameplay state every frame
```

```text
AI reading hidden authoritative information
```

```text
serde-dump internal structs as permanent save format
```

```text
runtime IDs persisted blindly into saves
```

```text
manual downstream invalidation inside every gameplay command
```

```text
generic callback/event bus controlling gameplay semantics
```

```text
premature complex pathfinding hierarchy
```

```text
mandatory browser threads
```

```text
large implicit global scans hidden behind convenience APIs
```

---

# 57. Engineering decision rule

When choosing between:

```text
simple full calculation
```

and:

```text
complex incremental algorithm
```

the order of decision is as follows:

```text
1. Implement simple correct version.
2. Measure representative scenario.
3. Identify actual hot path.
4. Add incremental/optimized version.
5. Keep simple rebuild implementation as oracle where practical.
6. Differential-test optimized implementation.
7. Measure again.
```

---

# 58. The main invariant of the project

Not:

> Everything must be incremental.

But:

> The authoritative state must remain simple and correct; the derived state must be recoverable; optimizations must be measurable and verifiable against a simpler correct implementation.

---

# 59. The expected result of the first implementation stage

The repository must contain a working architectural skeleton, not a set of interfaces without implementation.

The minimum expected:

```text
compiling Rust workspace
native application
browser WASM application
wgpu hex rendering
basic camera
GameState
AuthWorld
WorldTransaction
Command
ResolvedCommand
ChangeJournal
CompiledRuleset
UnitStore
CityStore
MapState
VisibilityState
Navigation baseline
RenderWorld
basic AIObservation
save/load
replay
scenario generator
bench harness
validators
property tests
```

Do not create architectural abstractions if there are not yet at least two real use cases.

---

# 60. Implementation priorities

The order:

```text
P0 correctness
P1 deterministic state model
P2 testability
P3 maintainability
P4 representative performance
P5 incremental optimisation
P6 parallelism
P7 advanced modding
```

P0-P3 must not be sacrificed for a hypothetical micro-optimization.

---

# 61. The final criterion of the architecture

The architecture is considered successful if three properties hold at once:

### Correctness

It can be proven that the optimized/incremental state matches the rebuild state.

### Performance

The late-game workload is measured, profiled and contains no obvious accidental `O(world)` operations in frequent local actions.

### Evolvability

Adding a new mechanic usually requires changing the mechanic itself, rules/content and the explicitly connected derived systems, not a cascade of manual dirty flags across the whole project.

---

# 62. The oracle rule: the fast path proves its correspondence to the slow one

Wherever a fast path deviates from a reliable one, **both** exist and
are maintained; their equivalence is proven by tests,
not asserted.

```text
reliable path: full rebuild / naive implementation / replay of the log
fast path:     increment / cache / heuristic
```

The rules:

```text
1. The fast path always has a named slow twin
   (rebuild, oracle, naive algorithm, replay).
2. A differential test compares them on random inputs,
   not only on hand-made scenarios.
3. Fuzz validation: the oracle is checked after EVERY commit
   of a random command sequence.
4. A violation of equivalence is a bug of the fast path by definition.
   The fast path is fixed, not the test and not the oracle.
5. Where an oracle is impossible in principle (historical state such as
   explored - it cannot be recovered from the current world, section 45 of platform/structure.md),
   the replay acts as the pair: the state after replaying the log must
   match the live one. The semantics are additionally covered by spot tests.
6. The arithmetic of increments is checked in debug (fails loudly),
   saturating only in release as a safety belt.
   A silent saturating without a checked twin is forbidden: it hides bugs.
7. A match on journal enums is exhaustive, without a wildcard:
   a new journal variant must make the compiler demand a branch
   in all derived applications.
8. The apply of derived systems does not read another derived: only AuthWorld +
   ChangeJournal + CompiledRuleset. The signatures guarantee this -
   the order of calls in apply_changes is semantically irrelevant.
   This is the constructive difference from an event bus (section 10): there the order
   of subscriptions is part of the semantics, here it is not. Reading derived is allowed
   only to one-way downstream consumers (AIObservation, render projection):
   they read but never write back and never trigger recomputation.
```

Examples of existing pairs:

```text
visibility rebuild / incremental (+ LOS predicate as the spec, section 32)
yields rebuild / incremental
movement occupancy rebuild / incremental
A* / Dijkstra oracle (in tests only)
render projection rebuild / apply
encode / decode (inversion, roundtrip)
knowledge sync / replay of explored vectors
```

Not pairs (a single implementation, behaviour tests):

```text
planner greedy (no slow twin - optimality is not required)
chunk build (determinism byte for byte instead of a pair)
hash (cross-check through roundtrip + replay)
```
