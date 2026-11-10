#!/usr/bin/env bash

set -euo pipefail

: "${GH_TOKEN:?GH_TOKEN is required}"

askpass="$(mktemp)"
trap 'rm -f "${askpass}"' EXIT

cat >"${askpass}" <<'EOF'
#!/usr/bin/env bash

case "$1" in
	*Username*) printf '%s\n' x-access-token ;;
	*Password*) printf '%s\n' "${GH_TOKEN}" ;;
	*) exit 1 ;;
esac
EOF
chmod 700 "${askpass}"

GIT_ASKPASS="${askpass}" GIT_ASKPASS_REQUIRE=force GIT_TERMINAL_PROMPT=0 git "$@"
