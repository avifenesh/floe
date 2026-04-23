# Hunk types

A **hunk** is one semantic delta between base and head. Hunks are the
atoms that flows group. Each hunk has a kind, a file, and
sometimes a subject entity.

| Kind | Subject | What it catches |
|---|---|---|
| `call` | call edges | A call-graph edge appeared, disappeared, or moved. |
| `state` | state machine | A string-union state gained or lost variants, or transitions changed. |
| `api` | exported API | An exported function / route handler's signature changed shape. |
| `lock` | (file, primitive) | A sync primitive appeared / disappeared / changed class. |
| `data` | (file, type) | A serde-serializable / `interface` / `z.object` / Rust struct gained, lost, or renamed fields. |
| `docs` | (file, target) | JSDoc `@param` drift from the documented function's signature. |
| `deletion` | entity | A Function / Type / State present in base is absent in head, with no remaining references. |

## `call`

Triggered by: any non-trivial delta in the call graph.

- `added_edges: Vec<EdgeId>` — new edges in head.
- `removed_edges: Vec<EdgeId>` — edges present in base that are gone in head.

Produced by: `floe-hunks/call.rs`.

## `state`

Triggered by: a string-union literal type (`type S = "a" | "b"`)
whose variant set differs between base and head.

- `node` — head-side `NodeId` of the state machine (falls back to
  base when removed).
- `added_variants`, `removed_variants`.

Produced by: `floe-hunks/state.rs`.

## `api`

Triggered by: an exported function whose signature string shape
changes. Same entity identity on both sides, different shape.

- `before_signature`, `after_signature` — one-line signature
  strings (e.g. `function enqueue(item: Item): Promise<void>`).

Produced by: `floe-hunks/api.rs`.

## `lock`

Triggered by: a sync primitive appearing, disappearing, or changing
class between base and head.

- TS primitives detected: `Mutex` / `Semaphore` from `async-mutex`,
  `pLimit` from `p-limit`, `PQueue` from `p-queue`, `AsyncLock`
  from `async-lock`, `Atomics.`.
- Rust primitives detected: `Mutex::new`, `RwLock::new`,
  `parking_lot::Mutex`, `AtomicBool/UsizeU32/I32::new`, `OnceCell::`,
  `OnceLock::`.

- `file`, `primitive` — identity.
- `before: Option<String>`, `after: Option<String>` — the primitive
  class name on each side (`None` = not present).

Produced by: `floe-hunks/lock.rs`. Pattern-matched via substring,
not AST — occasional false positives on comments mentioning a
primitive name.

## `data`

Triggered by: a struct / `interface` / `z.object` whose field set
differs between base and head.

- `file`, `type_name` — identity.
- `added_fields`, `removed_fields` — field names.
- `renamed_fields: Vec<(before, after)>` — single-rename heuristic
  fires when exactly one field is added and one removed.

Produced by: `floe-hunks/data.rs`. TypeScript covers
`interface NAME { }`, `type NAME = { }`, and `const NAME = z.object({ })`;
Rust covers `struct NAME { }`. Destructured / anonymous shapes are
not tracked.

## `docs`

Triggered by: JSDoc `@param` names disagreeing with the function's
actual parameter list on the head side.

- `file`, `target` — function identity.
- `drift_kind` — `"param-count"` (doc lists N params, signature has
  M ≠ N) or `"param-names"` (same count, different names).

Produced by: `floe-hunks/docs.rs`. Head-only — we don't care how
the drift appeared, only that the docs currently lie.

## `deletion`

Triggered by: an entity present in base but absent from head, with
no remaining references in head.

- `file`, `entity_name`, `was_exported` — `was_exported: bool` is
  read from the base signature's `export ` / `pub ` prefix;
  exported deletions are higher-weight.

Produced by: `floe-hunks/deletion.rs`.

## Adding a new hunk type

See [CONTRIBUTING.md](../CONTRIBUTING.md#adding-a-hunk-type).
