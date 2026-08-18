# fixture: clean-project

A correct, ordinary TypeScript project. **overpull must stay completely
silent about it.**

Every tool in this family needs a fixture it is supposed to say nothing
about. A rule that fires on the correct example fires everywhere, and a
report nobody can act on is worse than no report.

Deliberately included, because each one is a plausible false positive:

- a small `index.ts` barrel — 2 re-exports, far below the amplification and
  cost floors; a "flag every barrel" tool would report it
- `import type` edges, which vanish at run time and must not form cycles
- a `node:` builtin import, which is not an external package
- a dynamic `import()`, whose target must not be counted as load cost
- deep-but-acyclic imports, which must not be mistaken for a cycle

## Expected

| Command | Result |
|---|---|
| `overpull cycles` | 0 cycles, exit 0 |
| `overpull barrels` | 0 barrels reported, exit 0 |
| `overpull check --fail-on any` | exit 0 |
