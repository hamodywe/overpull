# Roadmap

What is planned, what is being considered, and what is deliberately out of
scope. Nothing here is a promise of a date.

## Shipped — 0.1.0

- `cost`, `barrels`, `cycles`, `check`
- Evaluation-order simulation with five run-time verdicts
- Per-import cost attribution
- JSON output and `--fail-on` gating

## Next

**`--diff <ref>` and `--baseline`.** Report only what a pull request *adds*:
new cycles, new barrel amplification, cost that grew. Adoption on an existing
codebase is the hard part for any analyser, and a baseline is what makes the
first run useful instead of overwhelming.

**A cost budget.** `overpull cost src/index.ts --max-modules 200` so a
regression in what an entry point loads fails CI the way a bundle-size budget
does.

**SARIF output**, so findings appear in GitHub code scanning with the
evidence attached.

**Workspace awareness.** Read npm/pnpm/Yarn workspace declarations and report
per-package, rather than treating a monorepo as one flat tree.

## Considering

**A `why <module>` query** — the shortest import path from an entry point to
a given file, which is the question people actually ask when they see an
unexpected module in a graph.

**Test-file entry simulation.** A test that imports an internal module
directly is exactly the deep import behind a `crash-if-loaded-first` verdict.
Detecting test files and simulating them as entry points would promote those
findings to certain rather than conditional.

**Bundler alias config.** Reading `vite.config.ts` / `webpack.config.js`
aliases would remove a class of unresolved imports, but it means executing or
half-parsing a config file — the cost is real and the benefit needs to be
proven first.

**`export =` and `module.exports` shapes.** Would move some `cjs-mixed`
verdicts into a definite answer.

## Out of scope

**Unused-export and dead-code detection.** knip and fallow cover this well.
overpull answers what an import costs, not what nothing imports.

**Drawing dependency graphs.** madge and dependency-cruiser draw; overpull
measures. A picture of a 1,500-module graph tells you nothing a number
cannot.

**Type checking.** That is `tsc`/`tsgo`'s job. overpull deliberately depends
on neither, which is why it still works after TypeScript 7 removed the
compiler API.

**Automatic fixes.** Breaking a cycle is a design decision. The tool names
the edge and explains why; a codemod that guesses wrong here is worse than
no codemod.

**A configuration file.** Everything is a flag today, and it stays that way
until a real use case cannot be expressed as one.
