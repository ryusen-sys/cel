.PHONY: check check-fmt lint test check-workflows conformance conformance-setup conformance-report changelog next-version fuzz-smoke

check: check-fmt lint test check-workflows

check-fmt:
	cargo fmt --all --check
	cargo fmt --manifest-path fuzz/Cargo.toml --check
	nixfmt --check flake.nix

lint:
	cargo clippy --workspace --exclude cel-core-conformance --all-targets --locked -- -D warnings

test:
	cargo test --workspace --exclude cel-core-conformance --all-targets --locked
	cargo test -p cel-core --doc --locked

check-workflows:
	actionlint .github/workflows/*.yml
	shellcheck scripts/git-with-token.sh scripts/verify-release-tag.sh scripts/verify-release-status.sh scripts/package-release.sh
	python3 scripts/check-release.py

conformance-setup:
	git submodule update --init crates/cel-core-conformance/cel-spec

conformance:
	test -f crates/cel-core-conformance/cel-spec/tests/simple/testdata/basic.textproto
	cargo test -p cel-core-conformance --locked

conformance-report:
	bash scripts/conformance-report.sh

fuzz-smoke:
	cargo fetch --manifest-path fuzz/Cargo.toml --locked
	mkdir -p fuzz/corpus/guard_index fuzz/corpus/compile
	cargo fuzz run guard_index fuzz/corpus/guard_index fuzz/seeds/guard_index -- -max_total_time=30 -max_len=768 -seed=905436370 -timeout=10
	cargo fuzz run compile fuzz/corpus/compile fuzz/seeds/compile -- -max_total_time=30 -max_len=4096 -seed=905436370 -timeout=10

changelog:
	git cliff 77277ec6a336646d946ee549a5012116378b82e4..HEAD -o changelog.md

next-version:
	git cliff 77277ec6a336646d946ee549a5012116378b82e4..HEAD --bumped-version
