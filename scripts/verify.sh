#!/usr/bin/env sh
# rs-broker verify pipeline runner.
# Usage: verify.sh <fast|full|all>
#   fast: fmt-check, clippy, check
#   full (or all): fast + test
set -eu

die() {
  printf '%s\n' "$1" >&2
  exit 2
}

# Resolve repo root: prefer $GIT_HOOK_VERIFY_ROOT, else walk up to find Cargo.toml.
if [ -n "${GIT_HOOK_VERIFY_ROOT:-}" ]; then
  REPO_ROOT=$GIT_HOOK_VERIFY_ROOT
else
  SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
  REPO_ROOT=$SCRIPT_DIR
  while [ "$REPO_ROOT" != "/" ]; do
    if [ -f "$REPO_ROOT/Cargo.toml" ]; then
      break
    fi
    REPO_ROOT=$(dirname "$REPO_ROOT")
  done
  if [ ! -f "$REPO_ROOT/Cargo.toml" ]; then
    die "verify.sh: could not locate repo root (no Cargo.toml found above $SCRIPT_DIR)"
  fi
fi

if [ "$#" -lt 1 ]; then
  die "usage: verify.sh <fast|full|all>"
fi

PROFILE=$1
case "$PROFILE" in
  fast) STAGES="fmt-check clippy check" ;;
  full|all) STAGES="fmt-check clippy check test" ;;
  *) die "unknown profile: $PROFILE (expected: fast|full|all)" ;;
esac

STAGE_NUM=0

run_stage() {
  num=$1
  name=$2
  shift 2
  printf '== %s. %s ==\n' "$num" "$name"
  if ! "$@"; then
    printf '== FAIL: %s ==\n' "$name" >&2
    exit 1
  fi
}

for stage in $STAGES; do
  STAGE_NUM=$((STAGE_NUM + 1))
  case "$stage" in
    fmt-check) run_stage "$STAGE_NUM" "Format" cargo fmt --check ;;
    clippy)    run_stage "$STAGE_NUM" "Lint" cargo clippy --workspace --all-targets -- -D warnings ;;
    check)     run_stage "$STAGE_NUM" "Type-check" cargo check --workspace --all-targets ;;
    test)      run_stage "$STAGE_NUM" "Test" cargo test --workspace ;;
    *) die "internal: unknown stage '$stage'" ;;
  esac
done

printf '== OK: verify (%s) passed ==\n' "$PROFILE"
