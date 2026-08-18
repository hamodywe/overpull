# fixture: namespace-cycle

Two cycles with the **same shape** and opposite verdicts, both built around
`import * as ns`. The expected numbers below were written before the tool
could produce them; `fixtures.rs` asserts against these, and `verify.mjs`
proves both halves under Node.

An ES module namespace object is created during *instantiation*, before any
module body runs. Holding it is therefore always safe. What can throw is
reading a member whose declaration is still in the temporal dead zone — and
a hoisted function never is.

overpull 0.1.0 treated every top-level namespace read as a `const` read and
called the safe half a crash. That is the failure mode this fixture exists
to prevent: reporting working code as broken is how a gate gets switched off.

## The shape

```
safe-main.mjs   → safe-b.mjs   → safe-a.mjs   → safe-b.mjs
                                    ↑ reads b.greet()  — a hoisted function

unsafe-main.mjs → unsafe-b.mjs → unsafe-a.mjs → unsafe-b.mjs
                                    ↑ reads b.PREFIX   — a const
```

In both, the `-a` module evaluates while the `-b` module is still
initializing. Only the second read hits the dead zone.

## Expected

| Property | Safe half | Unsafe half |
|---|---|---|
| hazard | `benign` | `crash` |
| reader | — | `unsafe-a.mjs` |
| member read | `greet` | `PREFIX` |
| owner | — | `unsafe-b.mjs` |
| declaration kind | `function` | `const/let` |
| runtime result | loads; `OUTPUT` is `greeting:a` | `ReferenceError: Cannot access 'PREFIX' before initialization` |

Total: 2 cycles found, 1 `crash`, 1 `benign`, `--fail-on crash` exits 1.

## Verify both halves yourself

```sh
node tests/fixtures/namespace-cycle/verify.mjs
```

Exits 0 only if the safe half loads *and* the unsafe half throws the exact
`ReferenceError`. One assertion without the other would let the tool pass by
being uniformly wrong.
