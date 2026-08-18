# Continuous integration

Three ways to run the checks, in the order they were added. All three run the
same things; the first one needs nothing but a clone.

## 1. Locally — `scripts/ci.sh`

```sh
sh scripts/ci.sh            # everything that works offline
sh scripts/ci.sh --quick    # fmt, clippy, tests
sh scripts/ci.sh --dogfood  # also clone a large real project and analyse it
```

Works on Linux, macOS, and Windows through Git Bash. Exits 0 only when every
stage passed, and prints a summary naming what passed, what was skipped, and
why:

```
summary
  passed  fmt clippy test msrv (1.95) fixtures (node) behaviour
  all checks passed
```

It sets `RUSTFLAGS=-D warnings`, exactly as the workflow does. That matters
more than it sounds: a bare `cargo clippy` re-run prints a cached `Finished`
without re-linting, so warnings can sit unnoticed until a CI job that treats
them as errors finally runs. Both of the metadata bugs fixed in 0.2.0 — the
wrong `rust-version` and five `collapsible_if` lints — were found by this
script on its first pass, after the Actions jobs meant to catch them had been
blocked for a day.

Stages:

| Stage | What it proves |
|---|---|
| `fmt` | `cargo fmt --all --check` |
| `clippy` | `cargo clippy --all-targets`, warnings as errors |
| `test` | `cargo test --all-targets` |
| `msrv` | the version `rust-version` claims actually builds |
| `fixtures` | Node throws where the fixtures say it throws, and loads where they say it loads |
| `behaviour` | the release binary's exit codes, and that `--json` / `--sarif` parse |
| `dogfood` | the analyser survives a 1,500-module real graph (needs the network) |

`msrv` is skipped rather than failed when the toolchain is not installed; the
output says how to install it. A skipped stage is never counted as a pass.

### As a pre-push hook

```sh
sh scripts/install-hooks.sh
```

Points `core.hooksPath` at `.githooks/`, which runs `scripts/ci.sh --quick`
before every push. It is per-clone and never committed, so it is opt-in:

```sh
git config --unset core.hooksPath   # turn it off
git push --no-verify                # bypass once
```

## 2. GitHub Actions — `.github/workflows/ci.yml`

The full matrix: tests on Linux, Windows and macOS, plus lint, MSRV, fixtures
and dogfood jobs.

**Currently blocked, and not by anything in this repository.** Every job fails
in about two seconds with zero steps executed and one annotation:

> The job was not started because your account is locked due to a billing
> issue.

That lock is account-wide. It applies to public repositories even though
Actions minutes are free for them, and no change to the workflow files can
work around it. It clears when the billing issue on the account is settled.

**One line in `ci.yml` is stale and cannot be pushed from here.** The `msrv`
job still pins `dtolnay/rust-toolchain@1.85.0`, while the real minimum is
1.95 (see the 0.2.0 changelog). Updating a workflow file needs a token with
the `workflow` scope, which the token in use does not have, so the fix has to
be applied either by granting that scope or by editing the line directly on
GitHub:

```yaml
      - uses: dtolnay/rust-toolchain@1.95.0
```

Until then the `msrv` job would fail if it ran. `.cirrus.yml` and
`scripts/ci.sh` already use 1.95, so both are correct.

## 3. Cirrus CI — `.cirrus.yml`

A hosted alternative that does not touch GitHub Actions billing at all: free
for public repositories on its community Linux cluster, configured entirely by
the committed `.cirrus.yml`.

To enable it, install the [Cirrus CI GitHub App](https://github.com/marketplace/cirrus-ci)
on the repository. Nothing else is required — no tokens, no secrets, no
changes to this repository.

It runs three tasks mirroring the Actions workflow: `fmt + clippy + test`,
`minimum supported rust version`, and `fixtures behave as documented` (which
shells out to `scripts/ci.sh`, so the two can never drift apart).

The one thing it does not give back is the OS matrix — the free Linux cluster
is Linux only. Windows and macOS coverage returns with GitHub Actions.

### If you would rather mirror to GitLab

GitLab's free tier also has CI minutes for public projects. Push a mirror and
add a `.gitlab-ci.yml`:

```yaml
image: rust:latest
variables:
  RUSTFLAGS: "-D warnings"
check:
  script:
    - rustup component add rustfmt clippy
    - sh scripts/ci.sh --quick
```

Not committed here, because an unmirrored repository does not need it.

## What a contributor actually has to do

Run `sh scripts/ci.sh` before opening a pull request, and paste the summary
line in the description. That is the whole requirement — hosted CI is a
convenience, not the source of truth about whether the code is correct.
