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
```

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
      - run: cargo install overpull
      - run: overpull check --fail-on crash
```

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

Or enforce a budget until the built-in one lands (see
[ROADMAP.md](../ROADMAP.md)):

```sh
MODULES=$(overpull cost src/index.ts --json | jq '.entries[0].modules')
if [ "$MODULES" -gt 200 ]; then
  echo "entry loads $MODULES modules, budget is 200"
  exit 1
fi
```

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
