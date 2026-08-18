# fixture: iife-cycle

Two cycles containing the **same arrow function**, one of which is invoked
where it is written. The expected numbers below were written before the tool
could produce them; `fixtures.rs` asserts against these, and `verify.mjs`
proves both halves under Node.

"Is the read inside a function?" is the wrong question. The right one is
"does that function run while the module evaluates?" — and an
immediately-invoked function expression does. overpull 0.1.0 stopped at the
first function ancestor and stayed silent about a load-time `ReferenceError`
that Node throws every time.

## The shape

```
invoked-main.mjs  → invoked-b.mjs  → invoked-a.mjs  → invoked-b.mjs
                                        ↑ (() => { … NAME … })()   runs now

deferred-main.mjs → deferred-a.mjs → deferred-b.mjs → deferred-a.mjs
                                        ↑ () => { … LABEL … }      runs later
```

In both, the arrow function closes over a binding from a module that has not
finished evaluating. Only the invoked one dereferences it at that moment.

## Expected

| Property | Invoked half | Deferred half |
|---|---|---|
| hazard | `crash` | `benign` |
| reader | `invoked-a.mjs` | — |
| binding read | `NAME` | — |
| owner | `invoked-b.mjs` | — |
| declaration kind | `const/let` | — |
| runtime result | `ReferenceError: Cannot access 'NAME' before initialization` | loads cleanly |

Total: 2 cycles found, 1 `crash`, 1 `benign`, `--fail-on crash` exits 1.

Async and generator IIFEs are deliberately *not* treated as immediate: only
the part before the first `await` or `yield` runs synchronously, and overpull
does not report what it cannot prove.

## Verify both halves yourself

```sh
node tests/fixtures/iife-cycle/verify.mjs
```

Exits 0 only if the deferred half loads *and* the invoked half throws the
exact `ReferenceError`.
