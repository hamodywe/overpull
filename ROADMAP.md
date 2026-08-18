# Roadmap

What is planned, what is being considered, and what is deliberately out of
scope. Nothing here is a promise of a date.

## Shipped — 0.1.0

- `cost`, `barrels`, `cycles`, `check`
- Evaluation-order simulation with five run-time verdicts
- Per-import cost attribution
- JSON output and `--fail-on` gating

## Shipped — 0.2.0

- **`--baseline`.** Report only what a branch adds, while still surfacing a
  finding that got worse than the baseline.
- **Cost budgets.** `overpull cost src/index.ts --max-modules 200
  --max-bytes 900kb` fails CI the way a bundle-size budget does.
- **SARIF output.** Findings land in GitHub code scanning with the reading
  line attached.
- **`why <module>`.** The shortest import chain from each entry point to a
  file — the question people actually ask when a number surprises them.
- **Entry-point classification and `--entry`.** Entry points come from
  `package.json` and conventional source paths, are capped at 64 with the
  overflow reported rather than silently dropped, and can be named outright.
- **Test-file entry simulation.** A test importing an internal module
  directly produces an order nothing else does, so `crash-if-loaded-first`
  findings now name the spec file that produces them rather than describing a
  hypothetical deep import.

Three verdict bugs went with them; see the [changelog](CHANGELOG.md#020--2026-08-18).

## Next

**`--diff <ref>`.** A baseline is a file someone has to keep up to date.
Comparing against a git ref directly — analyse the merge base, analyse the
working tree, report the difference — removes that step. The baseline
machinery is already in place; this is the ergonomic half.

**Workspace awareness.** Read npm/pnpm/Yarn workspace declarations and report
per-package, rather than treating a monorepo as one flat tree. Entry-point
classification already handles `packages/*/src/index.ts`; the reporting side
does not.

**Async IIFE precision.** `(async () => { … })()` is currently treated as
deferred, which is safe but incomplete: everything before the first `await`
runs synchronously. Splitting the body at the first suspension point would
promote a class of real crashes without inventing any.

## Considering

**Bundler alias config.** Reading `vite.config.ts` / `webpack.config.js`
aliases would remove a class of unresolved imports, but it means executing or
half-parsing a config file — the cost is real and the benefit needs to be
proven first.

**`export =` and `module.exports` shapes.** Would move some `cjs-mixed`
verdicts into a definite answer.

**Which member of a namespace, through a re-export.** `ns.thing` is now
resolved through re-export chains, but a namespace re-exported *as* a
namespace (`export * as ns from`) still falls back to the whole-object
answer.

## Out of scope

**Unused-export and dead-code detection.** knip and fallow cover this well.
overpull answers what an import costs, not what nothing imports.

**Drawing dependency graphs.** madge and dependency-cruiser draw; overpull
measures. A picture of a 1,500-module graph tells you nothing a number
cannot. `why` is the exception that proves it: one route, named by line.

**Type checking.** That is `tsc`/`tsgo`'s job. overpull deliberately depends
on neither, which is why it still works after TypeScript 7 removed the
compiler API.

**Automatic fixes.** Breaking a cycle is a design decision. The tool names
the edge and explains why; a codemod that guesses wrong here is worse than
no codemod.

**A configuration file.** Everything is a flag today, and it stays that way
until a real use case cannot be expressed as one.
