# How cycles are classified

Most import cycles are harmless. A few crash your process at load time. This
document explains exactly how overpull tells them apart, so you can check its
reasoning instead of trusting it.

## Why cycles are usually fine

ES modules are designed to tolerate cycles. Before any module body runs, the
whole graph is linked: every import binding is connected to the export it
points at, as a *live binding* rather than a copied value. Function
declarations are hoisted and fully initialized during that phase.

So this works, and always has:

```js
// even.mjs
import { isOdd } from './odd.mjs';
export function isEven(n) { return n === 0 ? true : isOdd(n - 1); }

// odd.mjs
import { isEven } from './even.mjs';
export function isOdd(n) { return n === 0 ? false : isEven(n - 1); }
```

`isOdd` is a hoisted function. By the time anything *calls* `isEven`, both
modules have finished evaluating. overpull reports this as `benign`.

## Why some cycles are not fine

`const`, `let` and `class` bindings are **not** initialized during linking.
They sit in the temporal dead zone until their module's body runs. Reading
one before then throws:

```js
// service.mjs
import { REGISTRY_LABEL } from './registry.mjs';
export const SERVICE_NAME = 'billing';
export function describeService() {
  return `${SERVICE_NAME} (${REGISTRY_LABEL})`;
}

// registry.mjs
import { SERVICE_NAME } from './service.mjs';
export const REGISTRY_LABEL = `registry:${SERVICE_NAME}`;  // ← runs too early
```

Load `service.mjs` and Node throws:

```
ReferenceError: Cannot access 'SERVICE_NAME' before initialization
```

Both files type-check. Both compile. The difference between this and the
mutual-recursion example is not the shape of the cycle — the shapes are
identical — it is **what is read, when, and what kind of declaration is
behind it**.

That fixture is in this repository at `tests/fixtures/crashing-cycle`, and
`node tests/fixtures/crashing-cycle/verify.mjs` makes Node produce that exact
error.

## The three questions

For every import edge inside a cycle, overpull asks:

### 1. When is the binding read?

The parser walks each reference to the imported name and looks at what
encloses it:

| Position | Classification |
|---|---|
| Top-level statement or initializer | **immediate** — runs during module evaluation |
| Class `extends` clause | **immediate** — the clearest form of this crash |
| Static class member initializer | **immediate** |
| Inside a function or arrow body | **deferred** — runs when called, long after loading |
| Instance property initializer | **deferred** — runs at construction |
| Type position only | irrelevant — erased before runtime |

Only immediate reads can be too early.

### 2. What declaration is behind the name?

The name is followed through re-export chains — `export { x } from`,
`export * from`, `export { default as x } from` — to the module that actually
declares it. Barrel files make this necessary: the import points at
`index.ts`, but the binding lives three files away, and the module that has
to have evaluated is the one at the end of the chain.

| Declaration | Before its module runs |
|---|---|
| `function` | fully initialized (hoisted) — **safe** |
| `class` | temporal dead zone — **throws** |
| `const` / `let` | temporal dead zone — **throws** |
| `var` | declared, `undefined` — **wrong value, no throw** |
| `enum` | compiles to `var` — **wrong value, no throw** |
| `interface` / `type` | erased — **safe** |

### 3. Has that module evaluated yet?

This is the part other tools skip, and the part that decides whether a real
cycle is a real bug.

overpull simulates ES module evaluation order: depth-first from an entry
point, imports in source order, each dependency fully evaluated before the
module that imports it — except an edge back into a module that is already in
progress, which returns immediately. That back edge is where cycles bite.

The simulation runs twice:

- **From the project's own entry points** — modules nothing else imports.
  A hazard found here fires when you start the app. Verdict: `crash`.
- **From each module inside the cycle**, as if it were loaded first. That
  happens with a deep import (`import '@pkg/internal/thing'`) or a test file
  importing an internal module directly. Verdict: `crash-if-loaded-first`.

The distinction is not pedantry. An early version of overpull reported a
crash in `vuejs/core`'s `runtime-core` package. The read is real; the module
order that would break it never occurs from Vue's entry points. Reporting
that as a crash would have meant calling working code broken — and a report
that does that gets switched off.

## The verdicts

| Verdict | Condition |
|---|---|
| `crash` | Immediate read of a TDZ binding, on the order your entry points produce |
| `crash-if-loaded-first` | Same read, only reachable when a cycle member is loaded first |
| `undefined-read` | Immediate read of a `var`/`enum` binding before its module runs |
| `cjs-mixed` | The loop crosses a `require()` edge; CommonJS ordering is not modeled |
| `benign` | No immediate read of an uninitialized binding from any simulated order |

## Which edge to break

Every finding names one edge, chosen by how cheap it is to remove:

1. **An edge whose bindings are all used in type positions** — `import type`
   erases it entirely and the cycle is gone at run time. No refactor.
2. **An edge whose bindings are all used inside functions** — a dynamic
   `import()` at the call site breaks the load-time loop while keeping the
   behaviour.
3. **Otherwise, the edge carrying the fewest immediately-used bindings** —
   the least code to move into a module neither side imports back.

overpull does not apply these. Breaking a cycle is a design decision, and a
codemod that guesses wrong here is worse than no codemod.

## Bounds

- At most 8 project entry points and 8 hypothetical entries are simulated per
  cycle. A graph with more distinct meaningful orders than that could in
  principle hide a hazard behind the ninth.
- Re-export chains are followed 64 levels deep.
- `require()` cycles are flagged, not resolved. Modeling
  `module.exports` re-assignment correctly is possible, and is on the roadmap
  as a definite answer rather than a guess.
