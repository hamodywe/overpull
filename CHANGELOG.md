# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.1.0]: https://github.com/hamodywe/overpull/releases/tag/v0.1.0
