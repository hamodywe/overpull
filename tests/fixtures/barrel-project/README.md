# fixture: barrel-project

A design-system shaped project: one barrel re-exporting 12 components, each
component pulling its own helper. This is the shape behind Atlassian's
"75% faster builds after removing barrel files" — importing one button
loads the whole library.

Expected numbers were written before the tool could produce them.

## The shape

```
src/index.ts          barrel: export * / export { … } from 12 components
src/components/c01…c12.ts   each imports its own helper
src/internal/h01…h12.ts     helpers, no further imports
src/app.ts            imports ONE component through the barrel
```

## Expected

| Property | Value |
|---|---|
| modules in project | 26 (barrel + 12 components + 12 helpers + app) |
| `cost src/app.ts` | 26 modules — it needed 3 |
| barrel reported | `src/index.ts` |
| barrel re-exports | 12 |
| barrel load cost | 25 modules |
| median member cost | 2 modules (component + helper) |
| amplification | 12.5x |

The point of the fixture: `app.ts` uses one component, and the tool must
show that importing it through the barrel loads 26 modules instead of 3.
