# fixture: test-entry-cycle

Reduced from a real finding in [vitejs/vite](https://github.com/vitejs/vite):
`config.ts` reads `buildEnvironmentOptionsDefaults` from `build.ts` at module
scope, and `build.ts` imports `config.ts` back. The expected numbers below
were written before the tool could produce them; `fixtures.rs` asserts against
them, and `verify.mjs` runs both orders under Node.

This fixture exists to pin down one decision: **a test file is simulated as an
entry point, but a hazard found only through one is not a `crash`.**

## The shape

```
src/index.mjs          → src/config.mjs → src/build.mjs → src/config.mjs
  (declared entry)          ↑ reads DEFAULTS, but build.mjs has already run

tests/build.test.mjs   → src/build.mjs  → src/config.mjs → src/build.mjs
  (test file)                               ↑ reads DEFAULTS too early
```

The cycle is the same in both. Only the order differs, and the order is
decided by which half the entry reaches first.

## Why not `crash`

Loaded on its own, `tests/build.test.mjs` throws — `verify.mjs` proves it in a
fresh Node process. But a test *process* is not a single file: by the time a
spec runs, vitest has usually evaluated the safe half already, which is why
vite's suite is green while this shape sits in its source.

overpull cannot know what the runner loaded first, so it says what it can
prove: the order is reachable, here is the file that produces it, whether it
fires depends on the process. Calling that a `crash` would mean calling a
green test suite broken — the one thing this tool must never do.

`--fail-on hazard` gates on it. `--fail-on crash` does not.

## Expected

| Property | Value |
|---|---|
| cycles found | 1 |
| hazard | `crash-if-loaded-first` |
| reader | `src/config.mjs` |
| binding read | `DEFAULTS` |
| owner | `src/build.mjs` |
| declaration kind | `const/let` |
| entry producing it | `tests/build.test.mjs` |
| entry kind | `test file` |
| `--fail-on crash` exit code | 0 |
| `--fail-on hazard` exit code | 1 |

Entry classification: `src/index.mjs` is `Package` (named by `exports`, and a
conventional path), `tests/build.test.mjs` is `Test`, `verify.mjs` is an
unreferenced module.

## Verify both orders yourself

```sh
node tests/fixtures/test-entry-cycle/verify.mjs
```

Exits 0 only if the package entry loads cleanly *and* a fresh process loading
the test file throws `Cannot access 'DEFAULTS' before initialization`.
