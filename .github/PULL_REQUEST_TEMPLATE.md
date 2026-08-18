## What this changes

<!-- One or two sentences. What was wrong, or what is now possible? -->

## Why

<!-- The reasoning. If this fixes a wrong verdict, show the code overpull
     got wrong and what it should have said. -->

## Checklist

- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --all-targets` is clean
- [ ] `cargo test` passes
- [ ] Analysis changes come with a fixture whose expected numbers were
      written in its README **before** the tool could produce them
- [ ] `tests/fixtures/clean-project` still produces zero findings
- [ ] Commits follow Conventional Commits
