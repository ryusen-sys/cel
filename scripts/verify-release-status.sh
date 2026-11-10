#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
	echo "usage: $0 <expected-creator>" >&2
	exit 2
fi

expected_creator=$1
context='Release build dry run'

if ! status=$(jq -er --arg context "${context}" '
	[.statuses[] | select((.context | ascii_downcase) == ($context | ascii_downcase))][0]
	| [(.state // "missing"), (.creator.login // "missing")]
	| @tsv
'); then
	echo "Release build dry run status response is malformed" >&2
	exit 1
fi

IFS=$'\t' read -r state creator <<<"${status}"
if [[ "${state}" != success || "${creator}" != "${expected_creator}" ]]; then
	printf 'Release build dry run is %s from %s; expected success from %s\n' \
		"${state}" "${creator}" "${expected_creator}" >&2
	exit 1
fi

printf 'Verified release build dry run from %s\n' "${expected_creator}"
