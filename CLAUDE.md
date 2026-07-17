# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A Rust AI bot for [Screeps: World](https://screeps.com/), compiled to WebAssembly and run inside the
Screeps JS sandbox. `src/` is the actual bot logic; `js_src/`/`js_tools/` are a thin JS shim that loads
the wasm module and a Rollup-based build/deploy script — treat the JS side as build plumbing, not
application code.

## Build & run

- Rust code requires the **nightly** toolchain (`lib.rs` enables unstable features:
  `variant_count`, `trait_alias`, `int_roundings`, `never_type`, `impl_trait_in_assoc_type`) plus the
  `wasm32-unknown-unknown` target, `wasm-pack`, and `wasm-opt`.
- Fast local iteration/type-checking: `cargo check`, `cargo clippy` (crate opts into
  `clippy::pedantic` as warnings — see `[lints.clippy]` in `Cargo.toml`).
- There is no test suite in this repo currently (no `#[test]`/`wasm_bindgen_test` usage) — validate
  changes with `cargo check`/`cargo clippy` and, where possible, by deploying to a sim/PTR server.
- Full build + deploy is done via npm, not `cargo build` directly:
  - `npm install` once, then `.screeps.yaml` must exist (copy from `.example-screeps.yaml` and fill in
    server credentials — this file is gitignored, don't commit real credentials).
  - `npm run deploy -- --server <name> --dryrun` — compiles via `wasm-pack` (invoked internally by
    `js_tools/deploy.js`), bundles with Rollup/Babel/Terser, but skips upload.
  - `npm run deploy -- --server <name>` — same, plus uploads via the `screeps-api` package using the
    matching entry in `.screeps.yaml`.
  - Per-server build options (crate features, wasm-pack flags, terser on/off) live under `configs:` in
    `.screeps.yaml`, keyed by server name or `*` for defaults.
- `[profile.release]` uses `panic = "abort"` and LTO; `wasm-opt` runs with `--signext-lowering`, which
  is required for the generated wasm to load on live Screeps servers (omitting it produces "Invalid
  opcode" errors at runtime).
- See `TODO.md` for the current backlog/known issues (e.g. remote mining, movement solver gaps, an
  in-progress truck bug, an in-progress `spawn.rs` split).

## Architecture

### Tick entry point (`src/lib.rs`)

`game_loop` (exported via `#[wasm_bindgen(js_name = loop)]`) is called once per Screeps tick by
`js_src/main.js`. Per-tick sequence: deserialize `Memory` from `RawMemory` → `update_coordinators` →
`do_creeps` (returns tugboat spawn requests) → `do_spawns` → `do_towers` → `do_links` → serialize
`Memory` back to `RawMemory` → draw visuals. CPU-bucket checks at the top skip/throttle ticks when the
bucket is low.

### Memory (`src/memory.rs`)

A single `Memory` struct is round-tripped through Screeps' `RawMemory` as JSON every tick (via
`serde_json_path_to_error`, aliased as `serde_json`, for better deserialize error paths). It holds
per-creep data, colony state, and the various coordinators/caches described below. Because Memory is
just JSON, any Screeps object reference stored in it (creep/structure IDs) can go stale between ticks
if the referenced object dies — see the Checked/Unchecked pattern below for how that's handled.

### Checked/Unchecked IDs (`src/check.rs`, `src/ids.rs`, `src/domain_traits.rs`)

Object references pulled out of Memory start as `Unchecked` (just deserialized, may not resolve) and
must go through `CheckFrom`/`Check` (`.check()`) to become `Checked`, which confirms the underlying
game object still exists this tick. `ObjectId<T, S>` and `CreepId<S>` are generic over this
`CheckState`. `filter_check_any_key_map` (used e.g. on `Memory.creeps`) deserializes a map while
silently dropping entries whose key/value fail the check — this is how the bot forgets creeps/structures
that died since the last tick without crashing. Expect to see `.check()`/`Checked`/`Unchecked` all over
code that touches persisted IDs.

### Colonies (`src/colony/`)

A "colony" is an owned room. `update_colonies` (`src/colony/mod.rs`) tracks the set of owned rooms and,
for each, owns a `(ColonyPlan, ColonyStep)` pair:
- `colony/planning/` computes a full base layout (`ColonyPlan`) for a room via a floodfill-based
  planner (`planner.rs`, `floodfill.rs`), independent of what's currently built. `plan.diff_with(&room)`
  compares the plan against reality; incompatible diffs block progress until a `MigrateColony` command
  is issued.
- `colony/steps.rs` (`ColonyStep`) is a per-colony state machine (driven by the same
  `statemachine::step` engine creeps use) that advances the room from bare plan toward the full build,
  placing construction sites etc. as it goes.
- `ColonyView` is the read-mostly per-tick handle passed around to creep/coordinator logic — it bundles
  the room, its plan, its current step, and its energy buffer (storage, falling back to a container).

### Creeps (`src/creeps/`)

Each live creep has a persisted `CreepRole` (in `Memory.creeps`, keyed by `CreepId`) that is one of
`Flagship`, `Excavator`, `Truck`, `ImportTruck`, `Fabricator`, `Tugboat`, or `Scrap`, each with its own
per-role state type and update logic in a matching submodule (`flagship.rs`, `excavator.rs`,
`truck/`, `fabricator/`). `do_creeps` (`src/creeps/mod.rs`) recovers `CreepData` for newly-seen creeps
(`CreepData::try_recover_from`, inferring role from name prefix) and then, per creep, calls
`statemachine::step` on that creep's role state.

`statemachine::step`/`run_transitions` (`src/statemachine.rs`) is the shared per-tick state machine
driver used by both creep roles and colony steps: a state's `update` returns
`Transition::Next(state)` or `Transition::Done(state)` (via the `next!`/`done!`/`next_if!`/`done_if!`
macros), and the driver re-invokes transitions up to `MAX_TRANSITIONS` (20) times in a single tick so a
creep can fall through several states in one call; on error it logs and resets to `T::default()`.

`VirtualCreep` (`src/creeps/virtual_creep.rs`) wraps a live `Creep` for the duration of a tick: role
logic queues intents (harvest/build/repair/attack/etc.) against it instead of calling the Screeps API
directly, `IntentType`s are grouped into priority pipelines (`PIPELINE_A`/`PIPELINE_B`) to resolve
conflicts (a creep can't harvest and attack the same tick), and `vcreep.commit()` at the end of the loop
actually issues the winning intents.

### Movement (`src/movement/`)

Movement is centralized rather than per-creep: role logic calls into `MovementRequests`
(`requests.rs`) to register where it wants to go this tick, `solver.rs` resolves all requests together
(to avoid creeps blocking/swapping with each other), and `simplifier.rs` reduces the resulting paths.
Results are cached per-creep in `MovementMemory` (`mod.rs`) across ticks. A thread-local `SELECTED` set
plus the `VisualizeMovement` command drives on-demand path visualization for debugging.

### Coordination (`src/coordination/`)

Cross-creep coordination primitives shared by the Truck and Fabricator coordinators:
`assignment.rs`/`tasks.rs`/`allocations.rs` implement a task/allocation system so multiple creeps don't
duplicate the same hauling/build work, and `expiring_map.rs` provides time-limited reservations. Per-
colony coordinator instances (`TruckCoordinator`, `FabricatorCoordinator`, plus the singleton
`FlagshipCoordinator`) live in `Memory` and are refreshed each tick in `update_coordinators`
(`src/lib.rs`) before creep logic runs.

### Spawning (`src/spawn.rs`)

Decides what to spawn (body composition, role, priority) per colony based on available/projected
energy and colony needs, and separately handles "tugboat" spawn requests (creeps that ferry other
creeps between rooms, requested by `do_creeps`'s return value). This is the largest module in the crate
and is under active refactoring.

### Commands (`src/commands.rs`) and callbacks (`src/callbacks.rs`)

`Command` is a debug/control-plane enum (e.g. `ResetMemory`, `ResetColony`, `VisualizePlan`,
`MigrateColony`) pushed externally (e.g. via the game console) and drained with `pop_command`/
`handle_commands` by the relevant subsystem each tick. `Callbacks` in `Memory` lets code schedule a
closure-like action to run on a future tick (invoked via `mem.handle_callbacks()` in `lib.rs`).

### Other

- `src/domain_traits.rs` — extension traits (`HasStore`, `EnergyStoreAccessors`, `Transferable`,
  `Withdrawable`, `Repairable`, `HasId`) that provide a uniform interface over the various
  `screeps-game-api` structure/object types, used pervasively instead of matching on concrete types.
- `src/structure.rs`, `src/tower.rs`, `src/pathfinding.rs`, `src/visuals.rs`, `src/names.rs`,
  `src/logging.rs`, `src/utils.rs` — supporting infrastructure (structure helpers, tower firing logic,
  low-level pathfinding, screen visuals, creep name generation, `fern`-based logging setup, small
  shared helpers) called into by the modules above.
