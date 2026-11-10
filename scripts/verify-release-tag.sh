#!/usr/bin/env bash

set -euo pipefail

release_tag="${1:-}"
release_commit="${2:-}"
mode="${3:-validate-ref}"

if ! printf '%s\n' "${release_tag}" | grep -Eq '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'; then
	echo "Invalid release tag: ${release_tag}" >&2
	exit 1
fi

if ! printf '%s\n' "${release_commit}" | grep -Eq '^[0-9a-f]{40}$'; then
	echo "Release commit must be a full lowercase commit SHA" >&2
	exit 1
fi

checked_out_commit="$(git rev-parse 'HEAD^{commit}')"
if [ "${checked_out_commit}" != "${release_commit}" ]; then
	echo "Checked out commit ${checked_out_commit} does not match ${release_commit}" >&2
	exit 1
fi

package_version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
if [ -z "${package_version}" ] || [ "${release_tag#v}" != "${package_version}" ]; then
	echo "Tag ${release_tag} does not match Cargo.toml version ${package_version}" >&2
	exit 1
fi

case "${mode}" in
	validate-ref)
		;;
	require-tag)
		if ! git show-ref --verify --quiet "refs/tags/${release_tag}"; then
			echo "Release tag ${release_tag} does not exist" >&2
			exit 1
		fi

		tag_commit="$(git rev-parse "refs/tags/${release_tag}^{commit}")"
		if [ "${tag_commit}" != "${release_commit}" ]; then
			echo "Tag ${release_tag} points to ${tag_commit}, expected ${release_commit}" >&2
			exit 1
		fi
		;;
	*)
		echo "Unknown release-tag verification mode: ${mode}" >&2
		exit 1
		;;
esac
