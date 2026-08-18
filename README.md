# overpull

**Measures what your imports really load.** Import-graph cost, barrel-file
amplification, and the import cycles that compile clean and throw at run
time — in Rust, on the [oxc](https://oxc.rs) parser and resolver.

```
overpull cost src/app.ts
overpull barrels
overpull cycles
```

---

## The problem

Three things are true about a modern TypeScript codebase, and no tool tells
you any of them:

**One import line can load a thousand modules.** Atlassian removed barrel
files and got [75% faster builds](https://www.atlassian.com/blog/atlassian-engineering/faster-builds-when-removing-barrel-files).
Marvin Hagemeister traced a test runner evaluating thousands of modules per
test file back to a single barrel ([the barrel file debacle](https://marvinh.dev/blog/speeding-up-javascript-ecosystem-part-7/)).
Existing tools flag that a barrel *exists* — `oxlint` warns above 100
re-exports, ESLint plugins ban them outright. None of them measure what one
costs you, which is the number that decides whether it is worth removing.

**Some import cycles crash and most do not.** A cycle that only passes
hoisted functions around is legal and works — mutual recursion across two
modules has always been fine. A cycle where a module reads a `const`, `let`,
or `class` from a module that has not evaluated yet throws
`ReferenceError: Cannot access 'X' before initialization`, at load time, in
production, having compiled and type-checked cleanly. Cycle detectors report
both identically. A report where most entries are fine is a report people
learn to scroll past.

**"Which import brought this in?" has no answer.** You can see that your
entry loads 300 modules. Nothing tells you that 250 of them arrive through
one line.

## What overpull does

| Command | Answers |
|---|---|
| `cost <entries…>` | What does importing this load? Modules, bytes, external packages — and for every direct import, how many modules vanish if that line goes. |
| `barrels` | Which re-export files amplify, and by how much: the measured cost of importing the barrel against the measured cost of importing a member. |
| `cycles` | Which cycles misbehave at run time, what exactly throws, and which edge to break. |
| `check` | `barrels` + `cycles` over the whole project, for CI. |
| `why <module>` | Why is this file in the graph at all? The shortest chain of import lines from each entry point to it. |

### What makes the cycle analysis different

overpull simulates ES module evaluation order from your project's real entry
points, then checks three things together:

1. **When** the imported binding is read — at module evaluation time, or
   deferred inside a function body that runs later.
2. **What kind of declaration** stands behind it, following re-export chains
   to the module that actually declares it. A hoisted `function` is safe. A
   `const`, `let` or `class` is in the temporal dead zone. An `enum` compiles
   to `var` and reads as `undefined` instead of throwing.
3. **Whether that module has evaluated yet** on the order your entry points
   produce.

Only the combination is reported as a crash. The verdicts:

| Verdict | Meaning |
|---|---|
| `crash` | Throws when your own entry point loads it. This is a live bug. |
| `crash-if-loaded-first` | Would throw, but only if a module inside the cycle is loaded first — a deep import, or a test file importing an internal module directly. Safe from your entry points. |
| `undefined-read` | An `enum`/`var` binding reads as `undefined` at load time. No throw; a wrong value. |
| `cjs-mixed` | The loop crosses a `require()` edge. CommonJS may observe a partial exports object, and overpull will not pretend to know which half. |
| `benign` | Hoisted functions or deferred use only. Legal, works, left alone. |

Every crash verdict names the reading line, the binding, the declaration
kind, and the module that had not run yet — enough to check the claim
yourself in about thirty seconds.

### Where "your entry points" comes from

The difference between `crash` and `crash-if-loaded-first` is entirely a
question of which module loads first, so overpull says out loud which modules
it treated as program starts:

| Source | Treated as |
|---|---|
| `package.json` — `main`, `module`, `browser`, `bin`, every leaf of `exports` | a program start |
| Conventional source paths — `src/index.*`, `src/main.*`, `cli`, `app`, `server` at the root or under `src` | a program start |
| Test files — `*.test.*`, `*.spec.*`, anything under `test/`, `tests/`, `__tests__/`, `e2e/` | simulated, because they produce real orders nothing else does — but findings reached only through one stay `crash-if-loaded-first`, and name the spec file |
| Everything else nothing imports | *not* a program start; hazards through it are `crash-if-loaded-first` |

A test file is deliberately not enough to claim a `crash`. Loaded on its own
a spec file really does produce that order — but a test *process* has usually
evaluated the safe half long before the spec runs, which is why a suite can
be green with the shape sitting in its source. This is not hypothetical:
`tests/fixtures/test-entry-cycle` is that exact case reduced from
[vitejs/vite](https://github.com/vitejs/vite), with a `verify.mjs` that runs
both orders under Node. `--fail-on hazard` gates on it; `--fail-on crash`
does not.

If the project declares nothing and has no conventional entry, every root is
treated as a program start — with nothing to go on, `node whatever.js` is as
plausible as any other guess.

Override all of it with `--entry`:

```sh
overpull cycles --entry src/server.ts --entry src/worker.ts
```

The report prints how many entry points it simulated, and warns if the cap
(64) left any out, rather than silently downgrading what it did not check.

## Install

```sh
cargo install --git https://github.com/hamodywe/overpull
```

Or build from source:

```sh
git clone https://github.com/hamodywe/overpull
cd overpull
cargo build --release      # ./target/release/overpull
```

Requires Rust 1.95 or newer. No Node.js, no `node_modules`, no config file.

> Not on crates.io yet, so `cargo install overpull` does not work — install
> from git with the command above. Prebuilt binaries wait on the same thing
> holding up CI; see [docs/ci.md](docs/ci.md).

## Quick start

```sh
# What does my entry point actually load?
overpull cost src/index.ts

# Which barrels are expensive?
overpull barrels

# Which cycles are dangerous?
overpull cycles

# Why on earth is this file in my graph?
overpull why src/legacy/config.ts

# Gate a pull request
overpull check --fail-on crash

# Gate a load-cost budget the way you would a bundle size
overpull cost src/index.ts --max-modules 200

# Report only what this branch adds
overpull check --baseline overpull-baseline.json
```

### `cost` — attribution per import

```
cost  src/app.ts
  loads 26 modules · 3.3 KB of source · 0 external packages

  brought in by, and only by:
       25  ./index.js  line 1
    (modules that leave the graph entirely if that import goes)
```

Twenty-five of twenty-six modules arrive through one line. The number is
measured, not estimated: overpull walks the graph again with that edge
removed and reports the difference.

### `barrels` — amplification, measured

```
barrels  1 amplifying

  src/index.ts
    importing it loads 25 modules (3.1 KB); a member costs 2 — 12.5x amplification
    12 re-exports (0 via export *), 0 local · imported by 1 module
```

Amplification is the barrel's load cost divided by the median load cost of
its own re-export targets: what you pay against what you needed.

### `cycles` — with the consequence spelled out

```
cycles  1 found
  1 crash · 0 crash-if-loaded-first · 0 undefined-read · 0 cjs-mixed · 0 benign

  crash  registry.mjs → service.mjs → registry.mjs
    registry.mjs:5 reads `SERVICE_NAME` while the module evaluates, but service.mjs has not run yet
    at run time — it is a const/let, so: ReferenceError: Cannot access 'SERVICE_NAME' before initialization
    fix service.mjs:1 uses registry.mjs only inside functions — a dynamic import() there
         breaks the load-time loop
```

That fixture is in this repository, and `node tests/fixtures/crashing-cycle/verify.mjs`
makes Node throw the exact error the report predicts.

### `why` — the route, not the count

```
why  src/internal/h07.ts
  once loaded it pulls 1 module (83 B)

  shortest chain from each entry point:

    entry point src/index.ts
      src/index.ts:7 → src/components/c07.ts
      src/components/c07.ts:1 → src/internal/h07.ts

  imported directly by 1 module
    src/components/c07.ts:1  ../internal/h07.js
```

"Who dragged that in" is the question people actually ask when a number
surprises them. Every hop names the import line to open.

### Budgets — a gate for `cost`

```sh
overpull cost src/index.ts --max-modules 200 --max-bytes 900kb
```

```
  over 26 modules, budget 10 — 16 over
```

Exits 1 when a budget is exceeded, the way a bundle-size budget does. A
budget that passes still prints `within`, because a gate that goes quiet when
it succeeds is indistinguishable from one that is not running.

### Baselines — adoption on a codebase that already has findings

```sh
overpull check --json > overpull-baseline.json   # record today
overpull check --baseline overpull-baseline.json # report only what is new
```

The first run on a large codebase is the hard part of adopting any analyser:
two hundred findings, none of them today's problem, and the gate is off
before lunch. A baseline judges a pull request on what it adds — while still
reporting a finding that got *worse*, since a cycle that was benign and now
crashes is new information about an old cycle. Suppressed counts are printed,
so a quiet run is never mistaken for a clean one.

## CI

```yaml
- uses: actions/checkout@v5
- uses: dtolnay/rust-toolchain@stable
- run: cargo install --git https://github.com/hamodywe/overpull
- run: overpull check --fail-on crash
```

Exit codes: `0` nothing at or above the threshold and no budget exceeded,
`1` findings or a budget exceeded, `2` usage error.

`--json` emits the same findings as machine-readable output for a dashboard
or a custom gate. `--sarif` emits SARIF 2.1.0, so findings land in GitHub
code scanning with the evidence attached:

```yaml
- run: overpull check --sarif --fail-on never > overpull.sarif
- uses: github/codeql-action/upload-sarif@v3
  with:
    sarif_file: overpull.sarif
```

Benign cycles are left out of SARIF on purpose: a dashboard that shows every
legal cycle teaches people to dismiss the whole run.

**This repository's own CI** is described in [docs/ci.md](docs/ci.md) —
including `scripts/ci.sh`, which runs every check offline on a laptop.

## Options

```
--root <dir>              Project root (default: current directory)
--entry <file>            Treat this file as a program entry point (repeatable)
--tsconfig <file>         tsconfig to read `paths` from (default: auto-discover)
--json                    Machine-readable output
--sarif                   SARIF 2.1.0, for code-scanning dashboards
--baseline <file>         Suppress findings already recorded in this file
--fail-on <level>         never | crash | hazard | any
--max-modules <n>         `cost` budget: exit 1 above this many modules
--max-bytes <n>           `cost` budget in bytes; accepts 900kb, 2mb
--top <n>                 Rows per section (default: 10)
--min-amplification <n>   Barrel amplification floor (default: 4)
--min-cost <n>            Barrel load-cost floor in modules (default: 20)
--no-color                Disable ANSI color (NO_COLOR is honoured too)
```

`--fail-on` levels, narrowest first:

- **`crash`** — throws on your own entry order. The default for `cycles` and
  `check`, and the only level that claims a live bug.
- **`hazard`** — also silent `undefined` reads and crashes that need a deep
  import to trigger.
- **`any`** — every cycle and every reported barrel.

## How it works

```
discover ──► parse ──► resolve ──► graph ──► analyse ──► report
  walk       oxc        oxc        edges     cost
  sources    parser     resolver   split     barrels
                                   by kind   cycles
```

- **Parsing** uses `oxc_parser` and `oxc_semantic` — the same engine behind
  oxlint and Rolldown. Nothing here touches the TypeScript compiler API,
  which TypeScript 7 removed and will not restore until 7.1.
- **Resolution** uses `oxc_resolver`: package.json `exports`/`imports` maps,
  tsconfig `paths`, extension probing, and the `./x.js` → `x.ts` mapping
  TypeScript requires. What overpull follows is what a bundler or Node
  actually loads.
- **The graph** separates edges by what they mean at run time. `import type`
  vanishes. `import()` is recorded as a boundary and not crossed. `require()`
  is kept but marked, because CommonJS evaluation order differs.
- **Cycles** come from an iterative Tarjan pass, then each component is
  simulated as described above.
- Files are parsed in parallel (`rayon`); everything else is deterministic
  and single-pass.

## Performance

Measured on this machine (Windows 11, warm file cache), whole-project
`check`:

| Project | Source files | Time |
|---|---|---|
| [vitejs/vite](https://github.com/vitejs/vite) | 1,519 | ~280 ms |
| [vuejs/core](https://github.com/vuejs/core) | 517 | ~180 ms |
| [microsoft/vscode-eslint](https://github.com/microsoft/vscode-eslint) | 61 | ~20 ms |

First run after a fresh clone is several times slower — that is disk cache,
not analysis. These are wall-clock numbers from one machine, not a
benchmark suite; reproduce them with `overpull check --root <repo> --json`.

### What it found on those repositories

| Project | Cycles | Barrels |
|---|---|---|
| vite | 27 — 0 crash, 10 crash-if-loaded-first, 1 undefined-read, 16 benign | 2 (`node/index.ts` at 12.4x, loading 99 modules) |
| vuejs/core | 7 — 0 crash, 1 undefined-read, 6 benign | 1 (`compiler-core/src/index.ts` at 4.3x) |
| vscode-eslint | 0 | 0 |

Zero false crash claims across all three. The `undefined-read` in vite is in
`__tests__/fixtures/cyclic2/` — a cycle the Vite team maintains deliberately,
to test exactly this behaviour.

## Limitations

Stated plainly, because a tool that overstates what it knows is worse than
no tool.

- **No type checker.** overpull reads syntax and module structure. It cannot
  tell a type-only value re-export from a runtime one when the code does not
  say so, which is one reason `import type` and `export type` matter.
- **Computed dynamic specifiers are opaque.** `import(someVariable)` cannot
  be followed; the count is reported so you know the number is a floor.
- **`node_modules` is a boundary, not a graph.** External packages are
  counted by name at the edge; overpull does not walk into them. The
  module counts are your code.
- **CommonJS is partially modeled.** `require()` calls and
  `import x = require()` become edges, but `module.exports` re-assignment
  patterns are not analysed. Cycles crossing them are reported as
  `cjs-mixed` rather than guessed at.
- **Entry-order simulation is bounded.** At most 64 real entry points and 8
  hypothetical entries per cycle are simulated. When the cap bites, the report
  says so and names the number left out — narrow the set with `--entry`.
- **Async IIFEs are treated as deferred.** `(() => { … })()` is correctly
  recognised as running at module-evaluation time, but
  `(async () => { … })()` is not: only the part before the first `await` runs
  synchronously, and overpull does not report what it cannot prove.
- **Whole-namespace reads are pessimistic.** `ns.member` is judged by that
  member's declaration, but `{...ns}` or `console.log(ns)` can observe every
  export at once, so every export becomes a candidate.
- **Re-export chains are followed 64 levels deep.** Past that the search
  stops, because source is untrusted input and an unbounded chain is a stack
  hazard, not a codebase.
- **Bundler-specific aliases are invisible** unless they are in tsconfig
  `paths`. A Vite `resolve.alias` or webpack `resolve.alias` entry will show
  up as an unresolved import — which is reported, not silently dropped.
- **Conditional exports are resolved for one condition set**
  (`types`, `import`, `node`, `default`). A package that resolves differently
  under `browser` or `react-native` is followed along the default branch.

## FAQ

**Is this a knip or madge replacement?**
No. knip finds unused code; madge draws dependency graphs. overpull answers
what an import costs and whether a cycle breaks. If you already run
`oxlint`'s `import/no-cycle`, overpull tells you which of those cycles
actually matters.

**Why not just ban barrel files?**
Because they are useful, and blanket bans lose to convenience. A measured
12.4x amplification on one file is an argument; "barrels are bad" is not.

**Why Rust?**
Because the analysis has to re-walk the graph once per import to attribute
cost, and that is only affordable at native speed. It also means no
dependency on the TypeScript compiler API, which TypeScript 7 removed —
tools built on it are stranded until 7.1.

**Does it need `node_modules` installed?**
No, and it does not need a build. Resolution degrades gracefully: an
unresolved bare specifier is still counted as an external package, and
unresolved relative imports are reported.

**Why does `check` show fewer findings than I expected?**
If you passed `--baseline`, the run prints how many findings it hid and says
"no *new* import cycles" rather than "clean". If you did not, check the
`simulated from N entry points` line — the verdicts are only as good as the
entry set, and `--entry` overrides it.

**Why is my cycle `crash-if-loaded-first` when I know it crashes?**
Because from your project's entry points, the module that declares the
binding evaluates first. If your test suite imports the internal module
directly, that is the deep import the verdict is describing — and the crash
is real in that context. Use `--fail-on hazard` to gate on it.

## Contributing

Issues and pull requests are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).
The fastest useful contribution is a repository where overpull says something
wrong: a false crash verdict is a bug of the highest priority here.

## License

MIT — see [LICENSE](LICENSE).
