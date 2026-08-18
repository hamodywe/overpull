# JSON output

Every command accepts `--json`. The shape is stable within a minor version;
additive fields may appear in a patch release.

All paths are project-relative with forward slashes, on every platform.

## `cost --json`

```json
{
  "tool": "overpull",
  "version": "0.1.0",
  "command": "cost",
  "entries": [
    {
      "entry": "src/app.ts",
      "modules": 26,
      "bytes": 3402,
      "externalPackages": ["react"],
      "dynamicTargets": 1,
      "opaqueDynamicImports": 0,
      "unresolvedImports": 0,
      "contributors": [
        {
          "specifier": "./index.js",
          "line": 1,
          "target": "src/index.ts",
          "exclusiveModules": 25
        }
      ]
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `modules` | Project modules evaluated at load, including the entry |
| `bytes` | Total source bytes of those modules |
| `externalPackages` | npm packages reached at load time, counted at the boundary |
| `dynamicTargets` | Modules behind `import()`, **not** included in `modules` |
| `opaqueDynamicImports` | `import(expr)` calls whose specifier could not be read — the counts above are a floor when this is non-zero |
| `unresolvedImports` | Imports that did not resolve; real cost is higher |
| `contributors[].exclusiveModules` | Modules that leave the graph entirely if this one import line goes |

## `barrels --json`

```json
{
  "tool": "overpull",
  "version": "0.1.0",
  "command": "barrels",
  "barrels": [
    {
      "file": "src/index.ts",
      "reexports": 12,
      "starReexports": 0,
      "localExports": 0,
      "costModules": 25,
      "costBytes": 3174,
      "medianMemberCost": 2,
      "amplification": 12.5,
      "importers": 1,
      "externalPackages": 0
    }
  ]
}
```

`amplification` is `costModules / medianMemberCost`: what importing the
barrel costs against what importing a member would have cost.

## `cycles --json`

```json
{
  "tool": "overpull",
  "version": "0.1.0",
  "command": "cycles",
  "cycles": [
    {
      "hazard": "crash",
      "members": ["registry.mjs", "service.mjs"],
      "path": ["registry.mjs", "service.mjs", "registry.mjs"],
      "evidence": {
        "reader": "registry.mjs",
        "line": 5,
        "binding": "SERVICE_NAME",
        "importedName": "SERVICE_NAME",
        "owner": "service.mjs",
        "importPointsAt": "service.mjs",
        "declarationKind": "const/let",
        "inExtendsClause": false,
        "entry": "entry.mjs"
      },
      "suggestion": {
        "kind": "defer-import",
        "from": "service.mjs",
        "to": "registry.mjs",
        "line": 1
      }
    }
  ]
}
```

`hazard` is one of `crash`, `crash-if-loaded-first`, `undefined-read`,
`cjs-mixed`, `benign`. See
[how-cycles-are-classified.md](how-cycles-are-classified.md).

`evidence` is absent for `benign` and `cjs-mixed` findings. `owner` is the
module that declares the binding; `importPointsAt` is where the import
statement points, which differs when a barrel sits between them. `entry` is
the entry point whose evaluation order produces the failure — for
`crash-if-loaded-first`, this is the module that has to be loaded first.

`suggestion.kind` is `import-type`, `defer-import`, `extract-shared`, or
`null`.

## `check --json`

Wraps the analyses that ran:

```json
{
  "tool": "overpull",
  "version": "0.1.0",
  "command": "check",
  "results": [ { "command": "barrels", "barrels": [] },
               { "command": "cycles",  "cycles": []  } ]
}
```

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Nothing at or above `--fail-on` |
| 1 | Findings at or above `--fail-on` |
| 2 | Usage error, or nothing to analyse |

JSON is written to stdout on codes 0 and 1. Usage errors go to stderr as
plain text — parse stdout only when the exit code is 0 or 1.
