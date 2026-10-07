#!/usr/bin/env bash
# Local reproduction of the CI gates before `git push`: every command below is
# the one .github/workflows/ci.yml, security-audit.yml or fuzz.yml runs, with
# the same arguments. A step this script does not cover is a step that can
# only fail remotely, so a step added to a workflow is added here in the same
# commit.
#
# usage: scripts/preflight.sh [--quick]
#   (none)   every gate: static checks, clippy, docs, the test suites, the
#            example, benches compile, MSRV, the security jobs (cargo audit /
#            deny / machete) and the fuzz target build
#   --quick  static checks, clippy, docs and `cargo test --lib`; skips the
#            full suites, example, benches, MSRV, security jobs and fuzz build
#
# Not reproduced here: the time-boxed fuzz runs of fuzz.yml (60 s per target in
# CI; run `cd fuzz && cargo +nightly fuzz run <target> -- -max_total_time=60`)
# and the four-OS matrix of the test job (this runs on the local OS only).
set -euo pipefail
cd "$(dirname "$0")/.."

quick=0
case "${1:-}" in
  --quick) quick=1 ;;
  "") ;;
  *) echo "usage: scripts/preflight.sh [--quick]" >&2; exit 2 ;;
esac
MSRV=1.87

step() { printf '\n\033[1;34m== %s\033[0m\n' "$*"; }
need() { command -v "$1" >/dev/null 2>&1 || { echo "missing tool: $1 ($2)" >&2; exit 1; }; }
# `cargo clippy` reuses fresh `cargo check` artifacts and then lints nothing;
# touching the crate root invalidates only this crate's fingerprints.
relint() { touch src/lib.rs; }

need actionlint "brew install actionlint"
need python3 "python 3.9+"

# CI creates manifest stubs for the optional sibling bridges with
# .github/stub-siblings.sh. That script overwrites ../ALICE-*/Cargo.toml, so it
# is not run here: a real checkout next to this one must not be replaced.
for d in ALICE-ML ALICE-Voice ALICE-Search ALICE-Font; do
  if [[ ! -f "../$d/Cargo.toml" ]]; then
    echo "missing ../$d/Cargo.toml (clone it, or run: bash .github/stub-siblings.sh)" >&2
    exit 1
  fi
done

step "ci.yml / actionlint: workflow YAML"
actionlint .github/workflows/*.yml

step "ci.yml / fmt: cargo fmt --check"
cargo fmt -- --check

step "ci.yml / docs-lint: tests + public documents / CHANGELOG structure"
python3 scripts/test_docs_lint.py
python3 scripts/docs_lint.py --check

step "security-audit.yml / stub-guard"
scripts/stub_guard.sh

step "ci.yml / clippy: default, ffi + python (pedantic + nursery via Cargo.toml)"
relint
cargo clippy --all-targets -- -D warnings
relint
cargo clippy --all-targets --features ffi,python -- -D warnings

step "ci.yml / doc: rustdoc -D warnings (default, ffi + python)"
RUSTDOCFLAGS="-Dwarnings" cargo doc --lib --no-deps
RUSTDOCFLAGS="-Dwarnings" cargo doc --lib --no-deps --features ffi,python

if [[ $quick -eq 1 ]]; then
  step "cargo test --lib (quick)"
  cargo test --lib
  echo; echo "preflight --quick OK (full test suites, example, benches, MSRV, security jobs and fuzz build skipped)"; exit 0
fi

step "ci.yml / test: default features, ffi"
cargo test
cargo test --features ffi

step "ci.yml / test: example text_law"
cargo run --example text_law

step "ci.yml / test: benches compile"
cargo bench --no-run

step "ci.yml / msrv: rust-version = $MSRV"
if rustup toolchain list | grep -q "^$MSRV"; then
  cargo +"$MSRV" check --lib
  cargo +"$MSRV" check --lib --features ffi,python
else
  echo "toolchain $MSRV not installed (rustup toolchain install $MSRV --profile minimal)" >&2
  exit 1
fi

step "security-audit.yml: cargo audit / cargo deny / cargo machete"
need cargo-audit "cargo install cargo-audit --locked"
need cargo-deny "cargo install cargo-deny --locked"
need cargo-machete "cargo install cargo-machete --locked"
cargo audit --db "${CARGO_TARGET_DIR:-target}/advisory-db" --deny yanked
cargo deny --all-features check all
cargo machete

step "fuzz.yml: build every fuzz target"
need cargo-fuzz "cargo install cargo-fuzz --locked"
( cd fuzz && cargo +nightly fuzz build )

echo; echo "preflight OK"
