# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] — 2026-08-18

Three verdict bugs and a set of features that make the tool adoptable on a
codebase that already has findings.

### Fixed

Each fix ships with a fixture whose `verify.mjs` runs the case under Node, so
the verdict is checked against what actually happens rather than against
itself.

- **`import * as ns` no longer reports working code as a crash.** An ES module
  namespace object is created during instantiation, before any module body
  runs, so holding one is always safe — only reading a member whose
  declaration is in the temporal dead zone can throw. 0.1.0 treated every
  top-level namespace read as a `const` read, which made
  `ns.someHoistedFunction()` inside a cycle a `crash` and failed builds that
  work. Member reads are now judged by the member's own declaration, and the
  report names it (`b.PREFIX`, not `b`). Fixture: `tests/fixtures/namespace-cycle`.
- **An immediately-invoked function expression is no longer treated as
  deferred.** `(() => { … })()` runs where it is written, so a binding it
  reads from a not-yet-evaluated module throws at load time. 0.1.0 stopped at
  the first function ancestor and stayed silent about a real `ReferenceError`,
  which `--fail-on crash` then let through. `(f)()`, `(f).call(…)` and
  `new (f)()` are recognised; async and generator IIFEs stay deferred, because
  only the part before the first `await` runs synchronously. Fixture:
  `tests/fixtures/iife-cycle`.
- **A real entry point can no longer fall off the end of the entry list.**
  Entry points were the first eight modules nothing imported, in discovery
  order — so a project with more than eight root files could have its actual
  entry point never simulated, and a genuine `crash` was reported as
  `crash-if-loaded-first` and exited 0. Entry points now come from
  `package.json` and conventional source paths first, the cap is 64, and the
  report says how many entries were simulated and warns when the cap bites.
  Fixture: `tests/fixtures/entry-points`.
- Barrel amplification prints one decimal consistently: `10.0x`, not `10x`
  between `10.8x` and `22.5x`.
- **`rust-version` was wrong.** The manifest and the README claimed Rust 1.85;
  `oxc_syntax 0.144` requires 1.95, so 1.85 could never have built this. The
  GitHub Actions `msrv` job exists to catch exactly that and has never been
  able to run — `scripts/ci.sh` caught it on the first pass.
- **Five `collapsible_if` lints** that the workflow's `RUSTFLAGS: -D warnings`
  would have failed on. They never surfaced locally because a plain
  `cargo clippy` re-run reports a cached "Finished" without re-linting;
  `scripts/ci.sh` sets the same `RUSTFLAGS` the workflow does.

### Added

- **`why <module>`** — the shortest chain of import lines from each entry
  point to one file, plus who imports it directly and what sits behind a
  dynamic `import()`. The question people actually ask when a number
  surprises them is which line to open.
- **`--entry <file>`** (repeatable) — name the program entry points instead
  of letting them be inferred. Every order-dependent verdict rests on this
  set, so it is now both printed and overridable.
- **Test files are simulated as entry points.** Nothing imports a test file,
  so it produces evaluation orders nothing else does. Findings reached only
  through one stay `crash-if-loaded-first` — a test *process* has usually
  evaluated the safe half before the spec runs — but the verdict now names the
  spec file that produces the order instead of describing a hypothetical deep
  import. Found on `vitejs/vite` and pinned as
  `tests/fixtures/test-entry-cycle`.
- **Cost budgets** — `overpull cost --max-modules 200 --max-bytes 900kb`
  exits 1 when an entry loads more than the budget, the way a bundle-size
  budget does. A budget that passes still prints `within`.
- **`--baseline <file>`** — suppress findings already recorded in a previous
  `--json` run, so a pull request is judged on what it adds. A finding that
  got worse than the baseline is still reported, and the number suppressed is
  printed so a quiet run is never mistaken for a clean one.
- **`--sarif`** — SARIF 2.1.0 output for GitHub code scanning and other
  dashboards, with the reading line as the result location. Benign cycles are
  omitted deliberately.
- **`scripts/ci.sh`** — every check CI runs, offline, on Linux, macOS, or
  Windows through Git Bash, with a summary that distinguishes skipped from
  passed. `scripts/install-hooks.sh` wires it to a pre-push hook.
- **`.cirrus.yml`** — the same checks on a provider that does not draw on
  GitHub Actions minutes. See [docs/ci.md](docs/ci.md).

### Changed

- `--fail-on` is rejected on `cost` and `why` instead of silently doing
  nothing; `cost` gates on `--max-modules` / `--max-bytes`.
- `--json` and `--sarif` together are a usage error rather than one silently
  winning.
- The `cycles` JSON document gained `entriesSimulated` and `entriesSkipped`;
  each cycle's `evidence` gained `member` and `entryKind`.
- `cost` JSON gained a `budget` object and a per-entry `overBudget` flag.
- Naming a module that is not in the analyzed graph now says so, and names
  the directories the walker skips, instead of reporting nothing to analyze.

## [0.1.0] — 2026-08-18

First release.

### Added

- `cost <entries…>` — transitive load cost of an entry (modules, bytes,
  external packages) with per-import attribution: how many modules leave the
  graph entirely if a given import line goes.
- `barrels` — re-export files ranked by measured amplification: the barrel's
  load cost against the median load cost of its own re-export targets.
- `cycles` — import cycles classified by run-time behaviour through ES module
  evaluation-order simulation: `crash`, `crash-if-loaded-first`,
  `undefined-read`, `cjs-mixed`, `benign`. Each verdict names the reading
  line, the binding, the declaration kind behind it, and the module that had
  not evaluated yet, plus the edge to break.
- `check` — `barrels` and `cycles` over the whole project, for CI.
- `--json` output for every command.
- `--fail-on never|crash|hazard|any` exit-code gating, defaulting to `crash`
  for `cycles` and `check`.
- Resolution through `oxc_resolver`: tsconfig `paths`, package.json
  `exports`/`imports` maps, and the `./x.js` → `x.ts` mapping TypeScript
  requires.
- Runtime-accurate edge classification: `import type` produces no runtime
  edge, `import()` is recorded as a boundary rather than crossed, `require()`
  edges are kept and marked.

### Notes

The distinction between `crash` and `crash-if-loaded-first` came out of
running the tool against [vuejs/core](https://github.com/vuejs/core), where
an earlier version reported a crash in `runtime-core` that cannot happen from
Vue's own entry points. Reporting working code as broken is how a gate gets
switched off, so the verdict was split rather than softened.

[0.2.0]: https://github.com/hamodywe/overpull/releases/tag/v0.2.0
[0.1.0]: https://github.com/hamodywe/overpull/releases/tag/v0.1.0
