# Contributing to overpull

Thanks for looking. This project has one quality bar above all others:
**overpull must not be wrong about working code.** A false `crash` verdict
costs more trust than a missed finding, because it teaches people to ignore
the report.

## The most valuable contribution

A repository where overpull says something wrong. Open an issue with:

- the repository and commit,
- the command you ran,
- what overpull reported,
- why it is wrong (ideally: the code runs, here is the proof).

That is the highest-priority class of bug here.

## Setup

```sh
git clone https://github.com/hamodywe/overpull
cd overpull
cargo build
cargo test
```

Rust 1.85 or newer. No other toolchain is needed to build, though Node.js is
needed to run the fixture verification scripts.

## Before opening a pull request

```sh
cargo fmt              # formatting is enforced
cargo clippy --all-targets   # must be clean, including pedantic lints
cargo test             # all tests must pass
```

## Fixtures

Every analysis change needs a fixture, and fixtures here follow one rule:

**Write the expected numbers in the fixture's README before the tool can
produce them.** A fixture whose numbers were filled in afterwards is not a
specification — it just agrees with whatever the code happens to do. This
rule has already caught one real bug in this repository.

Where a fixture claims a run-time behaviour, prove it:
`tests/fixtures/crashing-cycle/verify.mjs` makes Node throw the exact error
the report predicts, and exits non-zero if it does not.

There is also a fixture the tool is **required to stay silent about**
(`tests/fixtures/clean-project`). It deliberately contains the plausible
false positives — a small barrel, `import type` edges, a `node:` builtin, a
dynamic import. If your change makes overpull speak up about it, the change
is wrong, not the fixture.

## Testing the CLI

CLI tests spawn the real binary rather than calling into the library. An
in-process test can pass while the exit code is broken, and the exit code is
what CI depends on.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org/):
`feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `perf:`, `build:`, `ci:`,
`chore:`. Imperative mood, lowercase subject, no trailing period. The body
explains *why*.

## Code style

- Comments explain constraints and reasoning that the code cannot show. They
  do not narrate what the next line does.
- Errors name the problem and the fix, not just the failure.
- New dependencies need a justification in the pull request. The current list
  is short on purpose.

## Code of conduct

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).
