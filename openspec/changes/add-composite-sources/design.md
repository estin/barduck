## Context

See `proposal.md` for motivation. Relevant existing structure:

- `SourceCfg` (`src/config/source.rs`) is a `#[serde(tag = "type")]` enum with `Query`/`Stream`/`Ingest` variants, matched exhaustively (no wildcard arms) in ~20 accessor methods (`name`, `kind`, `unit`, `thresholds`, `effective_interval`, `visible_in`, ...). `health.rs`, `collector.rs`, the web renderer, and the TUI all go through these accessors and `cfg.sources.iter().find(|s| s.name() == name)` — none of them know about a "parent/child" relationship today.
- `health::assemble` resolves a source's `SourceCfg` purely by name lookup in `cfg.sources`; a name with no matching entry falls back to global defaults (used today only for the "shouldn't happen" case of a stale name). Its staleness math (`is_stale`) and its `thresholds()`/`effective_interval()` reads come entirely from that one `SourceCfg`.
- `collector.rs` runs one `loop_source` task per schedulable source, holding that source's schedule state and a `ControlReceiver`. `AppState.controls: HashMap<String, ControlSender>` maps a source name to the channel reaching its task; `Control::PollNow(oneshot::Sender<PollOutcome>)` asks that task to run its `attempt()` immediately.
- The HTTP ingest handler (`src/api.rs::ingest`) already defines the payload shape this proposal reuses: `{source, value, ts?, thresholds?}`, converted via `source::convert_value_type` and stored via `collector::store_parsed_value`.
- Web/TUI rendering already has a "table of sources" shape: a generalized pane's `table` section (`Vec<GroupItem>`, each resolved to a `SourceCfg` by name) renders one labeled, independently-colored row per member.

## Goals / Non-Goals

**Goals:**
- Reuse the existing per-source machinery (health, fetch log, readings, layout resolution, panel rendering) for children essentially unchanged, so the bulk of the codebase never needs to know "composite" exists.
- Keep the composite root and its children each independently addressable by name everywhere a source name already flows (config, CLI, HTTP API, layouts).
- One command execution per composite fetch, regardless of how many children it feeds or which name (root or child) triggered it.

**Non-Goals:**
- Composite `stream` or `ingest` sources (scope is `query` only, matching the proposal's example and the codebase's existing per-type field enforcement).
- Nested composites (a child cannot itself have children).
- Deduplicating a poll request that names both a composite root and one of its own children in the same CLI invocation — each requested name still triggers its own `PollNow`; naming both simply runs the command twice. Not required by the proposal, and de-duplicating across a repeatable `--source` list is unrelated complexity.
- Changing the ingest endpoint itself — composites reuse its payload shape and its underlying `store_parsed_value` call, not the endpoint.

## Decisions

### 1. Expand children into `cfg.sources` as a new `SourceCfg::Child` variant, at config-load time

Children are declared nested under their parent's TOML table (`[[sources.children]]`) but are **expanded into top-level entries** in `cfg.sources` during config loading, each carrying its full name (`load::1m`) and a `parent: String` back-reference. This is the one decision that makes everything else fall out of existing code:

- `health::assemble`'s `cfg.sources.iter().find(|s| s.name() == source)` finds a child exactly like any other source — no new lookup path.
- Layout validation (which already resolves every referenced id against `cfg.sources`) accepts `load::1m` with no changes.
- The TUI/web panel-building code that already turns a `SourceCfg` into a `Panel` needs no new branch for "this is secretly a child" beyond what `SourceCfg::Child`'s own accessor impls provide.

**Alternative considered:** keep children only inside the parent's `SourceCfg::Query { children: Vec<ChildCfg> }` and teach every consumer (health, layout validation, panel building) to know about composites. Rejected: it multiplies the surface area this change touches (every one of the ~10 places that currently do a flat `cfg.sources` lookup would need a second, composite-aware path) for no behavioral benefit — the flattened representation is strictly simpler and the two are observably identical from outside the process.

`SourceCfg::Child` carries no `command`/`interval`/`cron`/`timeout`/`setup`/`retry_interval` fields of its own (per spec, children can't declare them); instead it stores a **denormalized copy** of the fields those accessors need, copied from the parent at expansion time:
- `parent: String` — full name of the composite root, used to route force-polls.
- `effective_interval` / `cron` — copied so `health::is_stale` computes staleness exactly like a query source with the parent's own schedule, no lookup required.

This keeps every existing accessor (`effective_interval()`, `cron()`, `timeout()`, `visible_in()`, ...) a pure `match self { ... }` with no access to the rest of `cfg.sources` — consistent with how they're written today.

### 2. Reuse the ingest payload shape and `store_parsed_value` verbatim for each array element

The composite command's stdout is parsed as `Vec<IngestArrayItem>` where `IngestArrayItem` is structurally identical to `api::IngestBody` (`source`, `value`, `ts`, `thresholds`). Each item is validated and stored through the **same** `source::convert_value_type` + `collector::store_parsed_value` calls the ingest handler already uses, keyed on the child's `SourceCfg::Child`. This is why the proposal insists on "same as for ingest": it's not just a documentation convenience, it lets the fan-out path share code with `api::ingest` rather than reimplementing value conversion and threshold validation a third time (query's plain/jsonl path is the second).

**Alternative considered:** define a bespoke array-row schema closer to the existing `jsonl` row (`value`/`ts`/`threshold`, singular). Rejected: the proposal explicitly asks for ingest's shape, and reusing ingest's exact field names (`thresholds`, plural) lets one conversion function serve both call sites with zero branching on which caller it is.

### 3. One collector task per composite family; `Control::PollNow` carries the requested name

Only the **root** gets a `loop_source` task and a schedule; children get no task of their own. `AppState.controls` gets an entry for the root's name **and** for every child's full name, all cloned `UnboundedSender`s pointing at the same root task — cheap, since the sender is just a channel handle.

`Control::PollNow` gains the requested name: `PollNow(String, oneshot::Sender<PollOutcome>)`. For a plain (non-composite) source this is a no-op change (the name it carries is always that source's own name, already known to the caller). For a composite family, the root's task uses the carried name to shape its reply: run the command once, fan out to every child, then reply with either the root's own `PollOutcome` (command ran / stdout parsed, no `value`) or the specific child's `PollOutcome` (its stored value), depending on which name was requested. `Db::mark_polling` is called for the root and every declared child before running the command, and all guards are held (a small `Vec<PollingGuard>`) until it returns — so the shared "polling" signal covers the whole family for the duration of one command, regardless of which name triggered it.

**Alternative considered:** give every child its own `Control` channel and task that, on `PollNow`, asks the parent's task to run the command and awaits its result. Rejected: adds a second hop and a second kind of task for no benefit — the parent task can already read `cfg` to find its children, so it can shape a child-scoped reply directly.

### 4. Fan-out error policy (per the resolved proposal question)

- Stdout that isn't valid JSON, isn't an array, or fails to run at all → the whole attempt fails: one fetch-log entry for the root records the failure, no child is touched.
- An array element whose `source` doesn't name one of this root's declared children → the whole attempt fails the same way (a misconfigured or drifted script should be loud, not silently drop one row).
- A declared child simply absent from an otherwise-valid array → only that child's fetch log records a failure (`"missing from composite output"`); the root and every other present child succeed normally.

This means "the whole array parses and every `source` in it resolves to a declared child" is checked **before** any child is written, so a fetch is never partially applied because of a typo — only a legitimately absent entry degrades gracefully.

### 5. UI auto-rendering reuses the generalized pane's `table` section renderer

When a layout cell resolves to a composite root's `SourceCfg::Child`-parent (i.e., `cfg.sources` has children whose `parent == this name`), both the web and TUI panel builders synthesize a `table`-shaped set of `GroupItem`s from the root's declared children (in declared order) instead of trying to render the root as a single-value panel — going through the exact same rendering function an author-written `{ table = [...] }` cell already uses. The pane's title comes from the root's own `title`/name, matching how a `{ id, title }` cell already resolves a title today.

**Alternative considered:** require the user to spell out `{ table = ["load::1m", "load::5m", "load::15m"] }` by hand instead of auto-expanding a bare `"load"` reference. Rejected: it's exactly the boilerplate the proposal is trying to remove — the whole point of declaring children under the parent is that the layout doesn't need to enumerate them again.

## Risks / Trade-offs

- **[Risk]** Expanding children into `cfg.sources` means every future exhaustive `match SourceCfg { Query, Stream, Ingest }` in the codebase (and any new one added later) must also handle `Child` — a missed arm is a compile error today (good — the exhaustiveness the codebase already relies on catches it), but a careless `_ => ...` wildcard added under time pressure could silently mis-handle children. → Mitigation: none needed beyond keeping the existing no-wildcard convention; the compiler enforces it.
- **[Risk]** A composite root with many children turns one fetch-log/health query per child into effectively-simultaneous writes, increasing write volume per schedule tick proportional to child count. → Mitigation: acceptable — this replaces what would otherwise be that many *separate scheduled commands*, so total write volume is unchanged or lower.
- **[Trade-off]** Denormalizing `effective_interval`/`cron` onto each `Child` at expansion time means a config reload must re-expand every child (not just re-parse the root) if the root's schedule changes. This is already true of the whole config today (no live reload exists — a schedule change requires a restart), so it adds no new constraint.

## Migration Plan

Purely additive: no existing config, storage schema, or API contract changes shape. Existing single-value `query` sources are unaffected (no `children` field declared, `SourceCfg::Query` behaves exactly as before). No data migration; new `SourceCfg::Child` entries only appear for configs that opt in by declaring `children`.
