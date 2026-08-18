# JSON output

Every command accepts `--json`. The shape is stable within a minor version;
additive fields may appear in a patch release.

All paths are project-relative with forward slashes, on every platform.

## `cost --json`

```json
{
  "tool": "overpull",
  "version": "0.2.0",
  "command": "cost",
  "budget": { "maxModules": 200, "maxBytes": null },
  "entries": [
    {
      "entry": "src/app.ts",
      "modules": 26,
      "bytes": 3402,
      "overBudget": false,
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
| `budget` | The `--max-modules` / `--max-bytes` values, `null` when not given |
| `overBudget` | Whether this entry exceeded either budget; any `true` makes the process exit 1 |

## `barrels --json`

```json
{
  "tool": "overpull",
  "version": "0.2.0",
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
  "version": "0.2.0",
  "command": "cycles",
  "entriesSimulated": 3,
  "entriesSkipped": 0,
  "cycles": [
    {
      "hazard": "crash",
      "members": ["registry.mjs", "service.mjs"],
      "path": ["registry.mjs", "service.mjs", "registry.mjs"],
      "evidence": {
        "reader": "registry.mjs",
        "line": 5,
        "binding": "SERVICE_NAME",
        "member": null,
        "importedName": "SERVICE_NAME",
        "owner": "service.mjs",
        "importPointsAt": "service.mjs",
        "declarationKind": "const/let",
        "inExtendsClause": false,
        "entry": "entry.mjs",
        "entryKind": "entry point"
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

`member` is set when the read went through a namespace binding: `binding` is
the local name (`b`) and `member` is what was read off it (`PREFIX`).
`importedName` is the name whose declaration decides the verdict, so for a
namespace read it is the member.

`entryKind` is `entry point`, `test file`, or `unreferenced module` — where
the entry that produces the failure came from. `entriesSimulated` and
`entriesSkipped` describe the entry set the verdicts were computed against;
a non-zero `entriesSkipped` means the 64-entry cap bit, and a hazard
reachable only through one of those appears one severity down.

## `why --json`

```json
{
  "tool": "overpull",
  "version": "0.2.0",
  "command": "why",
  "module": "src/internal/h07.ts",
  "loadCost": { "modules": 1, "bytes": 83 },
  "unreachableEntries": 0,
  "paths": [
    {
      "entry": "src/index.ts",
      "entryKind": "entry point",
      "hops": ["src/index.ts", "src/components/c07.ts", "src/internal/h07.ts"],
      "lines": [7, 1]
    }
  ],
  "directImporters": [
    { "file": "src/components/c07.ts", "line": 1, "specifier": "../internal/h07.js" }
  ],
  "dynamicImporters": []
}
```

`lines[i]` is the import line in `hops[i]` that leads to `hops[i + 1]`, so it
is always one shorter than `hops`. `dynamicImporters` are places the module is
reached only through `import()`, where it loads on demand rather than at
startup.

`why` has no SARIF form: it answers a question rather than reporting findings.

## `check --json`

Wraps the analyses that ran:

```json
{
  "tool": "overpull",
  "version": "0.2.0",
  "command": "check",
  "results": [ { "command": "barrels", "barrels": [] },
               { "command": "cycles",  "cycles": []  } ]
}
```

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Nothing at or above `--fail-on`, and no budget exceeded |
| 1 | Findings at or above `--fail-on`, or a `cost` budget exceeded |
| 2 | Usage error, or nothing to analyse |

JSON is written to stdout on codes 0 and 1. Usage errors go to stderr as
plain text — parse stdout only when the exit code is 0 or 1.

## `--sarif`

SARIF 2.1.0, for GitHub code scanning and other dashboards:

```json
{
  "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
  "version": "2.1.0",
  "runs": [
    {
      "tool": { "driver": { "name": "overpull", "version": "0.2.0", "rules": [ … ] } },
      "results": [
        {
          "ruleId": "crash",
          "level": "error",
          "message": { "text": "Import cycle (crash): `SERVICE_NAME` is read at registry.mjs:5 …" },
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": { "uri": "registry.mjs" },
                "region": { "startLine": 5 }
              }
            }
          ]
        }
      ]
    }
  ]
}
```

Rule ids are the hazard labels plus `barrel-amplification`. Levels map as
`crash` → `error`, `crash-if-loaded-first` and `undefined-read` → `warning`,
`cjs-mixed` → `note`.

Benign cycles are omitted on purpose: a dashboard that shows every legal
cycle teaches people to dismiss the whole run.

`--json` and `--sarif` together are a usage error, not a silent preference.
