# fixture: entry-points

Not a cycle fixture — an **entry-set** fixture. Every order-dependent verdict
overpull makes rests on which modules it believes the project starts from, so
that classification needs a test of its own.

## The shape

```
package.json   exports "." → ./src/index.ts
               bin  "tool" → ./src/cli.ts

src/index.ts   ← declared
src/cli.ts     ← declared
src/core.ts    ← imported by everything; not an entry
src/legacy.ts  ← nothing imports it, nothing declares it
tests/core.test.ts ← nothing imports it, but the test runner loads it
```

## Expected

| Module | Kind | Why |
|---|---|---|
| `src/index.ts` | `Package` | named by `exports`, and a conventional path |
| `src/cli.ts` | `Package` | named by `bin`, and a conventional path |
| `src/legacy.ts` | `Orphan` | a root, but the project declares real entries |
| `tests/core.test.ts` | `Test` | a root the test runner loads; simulated, but never enough on its own to claim a `crash` — see `test-entry-cycle` |
| `src/core.ts` | — | has importers, so not an entry at all |

The distinction that matters: `legacy.ts` is a root file. Before 0.2.0 every
root was treated as a program start, which let a stray script decide whether
a cycle was called a crash — and let a real entry point fall off the end of
a fixed-size list. A hazard reachable only from `legacy.ts` is real, but it
is `crash-if-loaded-first`, not `crash`.
