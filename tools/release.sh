#!/usr/bin/env bash
# Publish a new version: bumps Cargo.toml, commits, tags, and pushes.
# GitHub Actions then builds it, and every copy of Goodminton offers the update.
#
#   tools/release.sh 0.2.0 "Courts can be renamed" "Fixed the timer sound"
#
# Each extra argument becomes a bullet under "What's new" in the update dialog.
set -euo pipefail
cd "$(dirname "$0")/.."

version="${1:?usage: tools/release.sh <version> [note]...}"
shift
version="${version#v}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Version must look like 1.2.3"; exit 1; }
current="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
if [ "$(printf '%s\n%s\n' "$current" "$version" | sort -V | tail -1)" != "$version" ] || [ "$current" = "$version" ]; then
  echo "New version ($version) must be higher than the current one ($current)"; exit 1
fi
if [ -n "$(git status --porcelain)" ]; then
  echo "Commit or stash your changes first"; exit 1
fi

notes=""
for n in "$@"; do notes+="• $n"$'\n'; done
[ -n "$notes" ] || notes="Improvements and fixes."

sed -i "0,/^version = \".*\"/s//version = \"$version\"/" Cargo.toml
cargo test --quiet
git add Cargo.toml Cargo.lock
git commit -m "Release v$version"
git tag -a "v$version" -m "$notes"
git push origin HEAD "v$version"
echo "Pushed v$version — the build takes a few minutes: https://github.com/petemolinero/goodminton/actions"
