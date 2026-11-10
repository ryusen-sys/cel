# CEL conformance testing

This crate runs the inherited cel-core harness against the official
[cel-spec](https://github.com/google/cel-spec) test data pinned by the submodule
at `8d28382c8229919d103749abf578f47577368e21`. CI runs it as **Upstream conformance
harness**, separately from ordinary Rust tests. Missing test files fail the run;
tests do not fetch a moving upstream branch.

**A passing run is not proof of strict CEL conformance.** The inherited harness
has permissive comparisons: expected `dyn` accepts any inferred type, errors do
not establish exact message/position equivalence, and unknown comparisons do not
validate complete unknown sets. Source audits and separate probes have found
semantic and safety gaps outside these test cases. Do not repeat the upstream
"100% conformance" claim without a stricter independently checked harness.

## What runs

| Phase | Existing contract |
| --- | --- |
| Parse/check | Parse cases and check them unless `disable_check` is set |
| Type deduction | Compare declared typed results using the inherited equivalence rules |
| Evaluation | Compare values/errors/unknowns; skip `check_only` cases |
| Enums | Exercise legacy and strong enum modes separately |

The 29 pinned test files include both core language and extensions, plus protobuf
host integrations. The harness environment enables features beyond Ryusen's
event-matching profile. See `tests/conformance.rs` and `src/service.rs` for its
actual comparison and environment behavior.

## Run without changing the baseline

```sh
nix develop --command make conformance-setup
nix develop --command make conformance
```

`make conformance-report` produces a diagnostic Markdown comparison with
`conformance-baseline.md`. That baseline is an upstream snapshot, not a fresh
certification; a report is not the CI gate. Only an explicit
`bash scripts/conformance-report.sh --update-baseline` rewrites it. Review the
raw test failures before updating a baseline.

The [January failure report](../../CONFORMANCE_FAILURES.md) is historical. The
fork's README documents outstanding hardening work; the guard-index experiment
does not repair or bypass evaluator semantics for surviving candidates.
