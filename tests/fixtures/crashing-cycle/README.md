# fixture: crashing-cycle

A cycle that **compiles clean and throws at run time**. The expected numbers
below were written before the tool could produce them; `cycle_fixtures.rs`
asserts against these, and `verify.mjs` proves the crash is real by running
the fixture under Node.

## The shape

```
entry.mjs → service.mjs → registry.mjs → service.mjs
                              ↑ reads SERVICE_NAME (a const) at module scope
```

`registry.mjs` is imported by `service.mjs` first, so `registry.mjs`
evaluates while `service.mjs` is still initializing. Its top-level
`SERVICE_NAME` read hits the temporal dead zone.

## Expected

| Property | Value |
|---|---|
| cycles found | 1 |
| hazard | `crash` |
| members | `registry.mjs`, `service.mjs` |
| reader | `registry.mjs` |
| binding read | `SERVICE_NAME` |
| owner | `service.mjs` |
| declaration kind | `const/let` |
| runtime result | `ReferenceError: Cannot access 'SERVICE_NAME' before initialization` |

## Verify the crash yourself

```sh
node tests/fixtures/crashing-cycle/verify.mjs
```

Exits 0 only if Node throws the exact `ReferenceError`. This is the check
that keeps the tool honest: a "crash" verdict it cannot reproduce is a
false alarm, and this fixture would catch it.
