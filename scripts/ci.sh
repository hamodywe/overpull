#!/usr/bin/env sh
# Every check CI runs, runnable on a laptop.
#
# This exists because hosted CI can stop being available — a billing lock, an
# outage, a fork without Actions enabled — and "we could not verify it" is not
# an acceptable state for a tool whose whole claim is that its verdicts match
# what Node does. Nothing here needs a network connection except the optional
# dogfood stage.
#
#   sh scripts/ci.sh            everything that works offline
#   sh scripts/ci.sh --quick    lint and tests only
#   sh scripts/ci.sh --dogfood  also clone a large real project and analyse it
#
# Exit code is 0 only if every stage passed.

set -eu

QUICK=0
DOGFOOD=0
for argument in "$@"; do
  case "$argument" in
    --quick) QUICK=1 ;;
    --dogfood) DOGFOOD=1 ;;
    -h|--help)
      sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "unknown option: $argument (try --help)" >&2
      exit 2
      ;;
  esac
done

cd "$(dirname "$0")/.."

MSRV=$(grep '^rust-version' Cargo.toml | head -1 | cut -d'"' -f2)
FAILED=""
PASSED=""
SKIPPED=""

# Warnings are errors here for the same reason they are in the workflow: a
# lint nobody has to fix is a lint nobody fixes.
export RUSTFLAGS="${RUSTFLAGS:--D warnings}"
export CARGO_TERM_COLOR=always

stage() {
  name="$1"
  shift
  printf '\n\033[1m▶ %s\033[0m\n' "$name"
  if "$@"; then
    PASSED="$PASSED $name"
  else
    printf '\033[31m✗ %s failed\033[0m\n' "$name"
    FAILED="$FAILED $name"
  fi
}

skip() {
  printf '\n\033[2m▶ %s — skipped (%s)\033[0m\n' "$1" "$2"
  SKIPPED="$SKIPPED $1"
}

have() {
  command -v "$1" >/dev/null 2>&1
}

# ---- lint ---------------------------------------------------------------

stage fmt cargo fmt --all --check
stage clippy cargo clippy --all-targets

# ---- tests --------------------------------------------------------------

stage test cargo test --all-targets

if [ "$QUICK" -eq 1 ]; then
  DOGFOOD=0
fi

# ---- minimum supported rust version -------------------------------------
#
# Builds on the version `rust-version` claims, so the claim is tested rather
# than asserted.

if [ "$QUICK" -eq 0 ]; then
  # `rust-version` is "1.95" but a toolchain is named "1.95.0-<host>", so the
  # exact name has to be looked up rather than guessed.
  TOOLCHAIN=$(rustup toolchain list 2>/dev/null | cut -d' ' -f1 | grep "^$MSRV" | head -1)
  if [ -n "${TOOLCHAIN:-}" ]; then
    stage "msrv ($MSRV)" cargo "+$TOOLCHAIN" build
  else
    skip "msrv ($MSRV)" "install it with: rustup toolchain install $MSRV"
  fi
fi

# ---- fixtures behave as documented --------------------------------------
#
# The crash fixtures must actually crash in Node and the benign ones must
# actually run. Without this, a "crash" verdict is an assertion agreeing with
# itself.

verify_fixtures() {
  for fixture in crashing-cycle benign-cycle namespace-cycle iife-cycle test-entry-cycle; do
    printf '  node tests/fixtures/%s/verify.mjs\n' "$fixture"
    node "tests/fixtures/$fixture/verify.mjs" || return 1
  done
}

behaviour_checks() {
  cargo build --release || return 1
  binary=./target/release/overpull
  [ -x "$binary" ] || binary=./target/release/overpull.exe

  # The tool must stay completely silent about a correct project, at every
  # severity. A rule that fires on the correct example fires everywhere.
  printf '  clean project is silent at --fail-on any\n'
  "$binary" check --root tests/fixtures/clean-project --fail-on any >/dev/null || return 1

  printf '  crashing cycle fails the build\n'
  if "$binary" cycles --root tests/fixtures/crashing-cycle >/dev/null; then
    echo "  expected a non-zero exit for a crashing cycle" >&2
    return 1
  fi

  printf '  benign cycle does not fail the build\n'
  "$binary" cycles --root tests/fixtures/benign-cycle >/dev/null || return 1

  printf '  an invoked arrow in a cycle fails the build\n'
  if "$binary" cycles --root tests/fixtures/iife-cycle >/dev/null; then
    echo "  expected a non-zero exit for the IIFE fixture" >&2
    return 1
  fi

  printf '  a test-only entry does not claim a crash
'
  "$binary" cycles --root tests/fixtures/test-entry-cycle >/dev/null || return 1
  if "$binary" cycles --root tests/fixtures/test-entry-cycle --fail-on hazard >/dev/null; then
    echo "  expected --fail-on hazard to catch the test-entry fixture" >&2
    return 1
  fi

  printf '  json and sarif documents parse\n'
  "$binary" check --root tests/fixtures/barrel-project --json >/dev/null || return 1
  "$binary" check --root tests/fixtures/crashing-cycle --sarif --fail-on never >/dev/null || return 1
  return 0
}

if [ "$QUICK" -eq 0 ]; then
  if have node; then
    stage "fixtures (node)" verify_fixtures
  else
    skip "fixtures (node)" "node is not on PATH"
  fi
  stage "behaviour" behaviour_checks
fi

# ---- dogfood ------------------------------------------------------------
#
# Not a pass/fail gate on someone else's code — a smoke test that the analyser
# survives a large real graph and emits valid JSON. Needs the network, so it
# is opt-in.

dogfood() {
  target="${TMPDIR:-/tmp}/overpull-dogfood"
  if [ ! -d "$target" ]; then
    git clone --depth 1 https://github.com/vuejs/core "$target" || return 1
  fi
  binary=./target/release/overpull
  [ -x "$binary" ] || binary=./target/release/overpull.exe
  "$binary" check --root "$target" --json --fail-on never > "$target.json" || return 1
  node -e "JSON.parse(require('fs').readFileSync(process.argv[1],'utf8'))" "$target.json" || return 1
  printf '  analysed %s, JSON is valid\n' "$target"
}

if [ "$DOGFOOD" -eq 1 ]; then
  if have git && have node; then
    stage "dogfood (vuejs/core)" dogfood
  else
    skip "dogfood (vuejs/core)" "needs git and node"
  fi
fi

# ---- summary ------------------------------------------------------------

printf '\n\033[1msummary\033[0m\n'
[ -n "$PASSED" ] && printf '  \033[32mpassed\033[0m %s\n' "$PASSED"
[ -n "$SKIPPED" ] && printf '  \033[2mskipped\033[0m%s\n' "$SKIPPED"
if [ -n "$FAILED" ]; then
  printf '  \033[31mfailed\033[0m%s\n' "$FAILED"
  exit 1
fi
printf '  \033[32mall checks passed\033[0m\n'
