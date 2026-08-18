# fixture: benign-cycle

A cycle that is **legal and works**. Function declarations hoist across
module boundaries, so neither module reads an uninitialized binding while
the other is still evaluating.

This is the fixture the tool is supposed to be quiet about at `crash`
severity. A cycle detector that reports every cycle as a problem trains
people to ignore it — that failure mode is what this fixture guards.

## The shape

```
even.mjs ⇄ odd.mjs   (mutual recursion, the textbook legal cycle)
```

## Expected

| Property | Value |
|---|---|
| cycles found | 1 |
| hazard | `benign` |
| `--fail-on crash` exit code | 0 |
| runtime result | loads and runs; `isEven(10)` is `true` |

## Verify

```sh
node tests/fixtures/benign-cycle/verify.mjs
```

Exits 0 only if the modules load and produce the right answer.
