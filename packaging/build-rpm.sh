#!/usr/bin/env bash
# Builds dist/listen-<version>-1.<dist>.x86_64.rpm and a portable tarball from target/release/listen
# (run `cargo build --release -p listen` first).
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
name="listen-${version}-x86_64"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

[[ -x target/release/listen ]] || { echo "target/release/listen missing; build it first" >&2; exit 1; }
mkdir -p "$stage/$name" dist
cp target/release/listen packaging/io.github.leks2205.Listen.desktop packaging/io.github.leks2205.Listen.svg LICENSE NOTICE.md README.md "$stage/$name/"
cargo about generate packaging/about.hbs -o "$stage/$name/THIRD_PARTY_LICENSES.html"
tar -C "$stage" -czf "dist/$name.tar.gz" "$name"

top="$stage/rpmbuild"
mkdir -p "$top/SOURCES" "$top/SPECS"
cp "dist/$name.tar.gz" "$top/SOURCES/"
# Building on a non-Fedora machine (e.g. the Ubuntu runner): rpmbuild has no dist tag there.
dist_def=()
if [[ -z "$(rpm --eval '%{?dist}' 2>/dev/null)" ]]; then
    dist_def=(--define "dist .fc43")
fi
rpmbuild -bb --quiet \
    "${dist_def[@]}" \
    --define "_topdir $top" \
    --define "listen_version $version" \
    packaging/listen.spec
find "$top/RPMS" -name '*.rpm' -exec cp {} dist/ \;
ls -1 dist/ | grep -E "^listen-${version}"
