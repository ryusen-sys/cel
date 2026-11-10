#!/usr/bin/env bash
set -euo pipefail

tag="${1:?release tag required}"
commit="${2:?release commit required}"
bash scripts/verify-release-tag.sh "${tag}" "${commit}"
mkdir -p dist
archive="ryusen-cel-${tag#v}.tar.gz"
# git archive does not include submodule contents. Include the pinned test data
# explicitly so a release source checkout can also run the conformance harness.
source_dir="$(mktemp -d)"
trap 'rm -rf "${source_dir}"' EXIT
git archive --prefix="ryusen-cel-${tag#v}/" "${commit}" | tar -xf - -C "${source_dir}"
submodule=crates/cel-core-conformance/cel-spec
expected="$(git ls-tree "${commit}" "${submodule}" | awk '{print $3}')"
test "$(git -C "${submodule}" rev-parse HEAD)" = "${expected}"
git -C "${submodule}" archive "${expected}" | tar -xf - -C "${source_dir}/ryusen-cel-${tag#v}/${submodule}"
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -czf "dist/${archive}" -C "${source_dir}" "ryusen-cel-${tag#v}"
(cd dist && sha256sum "${archive}" > SHA256SUMS)
