.PHONY: check check-fmt lint test check-workflows conformance conformance-setup conformance-report changelog next-version

check: check-fmt lint test check-workflows

check-fmt:
	cargo fmt --all --check
	nixfmt --check flake.nix

lint:
	cargo clippy --workspace --exclude cel-core-conformance --all-targets --locked -- -D warnings

test:
	cargo test --workspace --exclude cel-core-conformance --all-targets --locked
	cargo test -p cel-core --doc --locked

check-workflows:
	actionlint -config-file .github/actionlint.yaml .github/workflows/*.yml
	shellcheck scripts/git-with-token.sh scripts/verify-release-tag.sh scripts/verify-release-status.sh scripts/package-release.sh
	python3 scripts/check-release.py

conformance-setup:
	git submodule update --init crates/cel-core-conformance/cel-spec

conformance:
	test -f crates/cel-core-conformance/cel-spec/tests/simple/testdata/basic.textproto
	cargo test -p cel-core-conformance --locked

conformance-report:
	bash scripts/conformance-report.sh

changelog:
	git cliff 77277ec6a336646d946ee549a5012116378b82e4..HEAD -o changelog.md

next-version:
	git cliff 77277ec6a336646d946ee549a5012116378b82e4..HEAD --bumped-version
