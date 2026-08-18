# Examples

Runnable examples of the questions overpull answers. Every command below
works against the fixtures in this repository — clone it and try them.

## Find out what an entry point really loads

```sh
overpull cost src/index.ts
```

Use it when a test suite got slow, a serverless cold start got worse, or a
"small" import turned out not to be. The interesting number is not the total,
it is the attribution underneath it: if one line owns 200 of 240 modules, you
know exactly what to look at.

Multiple entries at once:

```sh
overpull cost src/index.ts src/worker.ts src/cli.ts
```

## Decide whether a barrel is worth removing

```sh
overpull barrels --root packages/ui
```

Removing a barrel is a wide, annoying refactor. Do it when the number
justifies it:

```
  src/index.ts
    importing it loads 99 modules (1.2 MB); a member costs 8 — 12.4x amplification
    57 re-exports (0 via export *), 3 local · imported by 14 modules
```

Fourteen files pay 12.4x. That is an argument. Lower the floors to see
everything:

```sh
overpull barrels --min-amplification 1.5 --min-cost 5
```

## Find the cycle that will actually break

```sh
overpull cycles
```

Then read the verdict, not just the count. `benign` cycles are legal and
working code; `crash` is a live bug. See
[docs/how-cycles-are-classified.md](../docs/how-cycles-are-classified.md).

Prove one to yourself:

```sh
node tests/fixtures/crashing-cycle/verify.mjs   # Node throws, exit 0
node tests/fixtures/benign-cycle/verify.mjs     # loads fine, exit 0
node tests/fixtures/namespace-cycle/verify.mjs  # one half throws, one does not
node tests/fixtures/iife-cycle/verify.mjs       # the same arrow, invoked and not
```

If a verdict looks wrong, check the entry set first — it is printed with the
findings, and everything order-dependent rests on it:

```
  simulated from 12 entry points (3 declared, 9 test)
```

Name them yourself when the guess is wrong:

```sh
overpull cycles --entry src/server.ts --entry src/worker.ts
```

## Find out why a file is in the graph at all

```sh
overpull why src/legacy/config.ts
```

The answer is a route, not a count — every hop names the import line to open:

```
    entry point src/index.ts
      src/index.ts:7 → src/components/c07.ts
      src/components/c07.ts:1 → src/internal/h07.ts
```

Useful the moment `cost` or `barrels` shows you a module you did not expect
to be paying for.

## Gate a pull request

```sh
overpull check --fail-on crash
```

Fails only on a cycle that throws on your own entry order. Tighten once the
existing findings are cleared:

```sh
overpull check --fail-on hazard   # also undefined reads and deep-import crashes
```

### GitHub Actions

```yaml
name: overpull
on: [pull_request]

jobs:
  imports:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install --git https://github.com/hamodywe/overpull
      - run: overpull check --fail-on crash
```

### GitHub code scanning

```yaml
      - run: overpull check --sarif --fail-on never > overpull.sarif
      - uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: overpull.sarif
```

Findings appear in the Security tab with the reading line attached, rather
than as a line in a build log.

### Adopting on a codebase that already has findings

Record what is there today, commit the file, and gate on what a branch adds:

```sh
overpull check --json > overpull-baseline.json
git add overpull-baseline.json && git commit -m "chore: record overpull baseline"
```

```yaml
      - run: overpull check --baseline overpull-baseline.json --fail-on hazard
```

A finding that got *worse* than the baseline is still reported — a cycle that
was benign and now crashes is new information about an old cycle — and the
number suppressed is printed, so a quiet run is never mistaken for a clean
one.

### Pre-commit hook

```sh
#!/bin/sh
# .git/hooks/pre-commit
overpull cycles --fail-on crash || {
  echo "A cycle in this commit throws at load time. See the report above."
  exit 1
}
```

## Track cost over time

`--json` gives you the numbers to store:

```sh
overpull cost src/index.ts --json \
  | jq '{date: now | todate, modules: .entries[0].modules, bytes: .entries[0].bytes}' \
  >> cost-history.jsonl
```

Or enforce a budget, the way you would a bundle size:

```sh
overpull cost src/index.ts --max-modules 200 --max-bytes 900kb
```

Exits 1 when either budget is exceeded and names the overshoot. A budget that
passes still says `within`, so a silent run means the check ran and was
happy — not that it never ran.

## Monorepos

Point `--root` at one package to analyse it on its own:

```sh
overpull check --root packages/ui
```

Or run from the repository root to see cycles that cross package boundaries —
the ones a per-package tool cannot see at all:

```sh
overpull cycles --root .
```

## When imports do not resolve

```
  3 imports did not resolve — the real cost is higher than shown
```

Usually a bundler alias that is not in tsconfig. Point overpull at the
tsconfig that has the `paths`:

```sh
overpull check --tsconfig tsconfig.app.json
```
