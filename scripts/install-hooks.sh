#!/usr/bin/env sh
# Install rs-broker git hooks by symlinking scripts/hooks/<name> into .git/hooks/.
# Usage: install-hooks.sh [--force]
set -eu

die() {
  printf '%s\n' "$1" >&2
  exit 2
}

FORCE=0
for arg in "$@"; do
  case "$arg" in
    --force) FORCE=1 ;;
    -h|--help)
      printf 'usage: install-hooks.sh [--force]\n'
      exit 0
      ;;
    *) die "unknown argument: $arg" ;;
  esac
done

# Resolve repo root.
GIT_COMMON_DIR=$(git rev-parse --git-common-dir 2>/dev/null || true)
if [ -z "$GIT_COMMON_DIR" ]; then
  SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
  CAND=$SCRIPT_DIR
  while [ "$CAND" != "/" ]; do
    if [ -d "$CAND/.git" ]; then
      GIT_COMMON_DIR=$CAND/.git
      break
    fi
    CAND=$(dirname "$CAND")
  done
fi
if [ -z "$GIT_COMMON_DIR" ]; then
  die "install-hooks.sh: not inside a git repository (no .git found)"
fi

SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
HOOKS_DIR=$GIT_COMMON_DIR/hooks

if [ ! -d "$HOOKS_DIR" ]; then
  die "install-hooks.sh: hooks dir does not exist: $HOOKS_DIR"
fi

for name in pre-commit pre-push; do
  TARGET=$REPO_ROOT/scripts/hooks/$name
  HOOK_PATH=$HOOKS_DIR/$name

  if [ ! -f "$TARGET" ]; then
    die "install-hooks.sh: source hook missing: $TARGET"
  fi
  chmod +x "$TARGET"

  # Already linked to the right target → idempotent skip.
  if [ -L "$HOOK_PATH" ]; then
    LINK_VAL=$(readlink "$HOOK_PATH")
    case "$LINK_VAL" in
      /*) RESOLVED=$LINK_VAL ;;
      *)  RESOLVED=$HOOKS_DIR/$LINK_VAL ;;
    esac
    if [ "$RESOLVED" = "$TARGET" ]; then
      printf 'already linked %s -> %s\n' "$name" "$TARGET"
      continue
    fi
  fi

  # Existing entry that's neither a symlink nor a *.sample → refuse unless --force.
  if [ -e "$HOOK_PATH" ] && [ ! -L "$HOOK_PATH" ]; then
    case "$HOOK_PATH" in
      *.sample) : ;;
      *)
        if [ "$FORCE" -ne 1 ]; then
          die "install-hooks.sh: $HOOK_PATH already exists and is not a symlink. Re-run with --force to back it up and overwrite."
        fi
        mv "$HOOK_PATH" "$HOOK_PATH.bak"
        printf 'backed up %s -> %s.bak\n' "$name" "$HOOK_PATH.bak"
        ;;
    esac
  fi

  # Remove stale symlink or sample file before linking.
  rm -f "$HOOK_PATH"
  ln -s "$TARGET" "$HOOK_PATH"
  printf 'installed %s -> %s\n' "$name" "$TARGET"
done
