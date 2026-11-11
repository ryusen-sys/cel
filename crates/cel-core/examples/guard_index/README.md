# Guard-index prototype

Compare a full CEL scan with conservative candidate selection over one million
wait registrations. This is an in-memory experiment, not a production API or a
claim of CEL conformance. The parser, checker and evaluator are unchanged.

## Run in the pinned toolchain

Use this repository's pinned Nix shell. No separate Ryusen checkout or
additional dependencies are needed.

```sh
nix develop --command cargo test -p cel-core --example guard_index --locked
nix develop --command cargo build -p cel-core --release --example guard_index --locked
target/release/examples/guard_index --registrations 1000000 --events 24 --workload selective
```

Run each workload in a fresh process for meaningful RSS comparisons. Standard
output is NDJSON: setup, per-event measurements and a summary. Do not run other
CPU-intensive jobs concurrently. `--help` lists arguments and workloads;
`--list-templates` prints every complete expression and its family label.

| Workload | Registration distribution |
| --- | --- |
| `selective` | Required equality; one-million-value key space |
| `lower-cardinality` | Required equality; one-thousand-value key space |
| `fallback-0.1pct` | 1,000 of one million registrations have an opaque predicate |
| `fallback-1pct` | 10,000 of one million registrations have an opaque predicate |
| `hot-key` | Approximately half the registrations share one hot value |
| `all-unindexed` | Every predicate is opaque to the selector |
| `all-match` | Every registration matches every generated event |

Opaque guards use NOT, OR, conditional or `exists` forms equivalent to equality
on these valid benchmark inputs. They are deliberately outside this planner's
coverage, not intrinsically impossible to index. Their extra work is included
in both routes; the revised workload is not identical to the original baseline.

Residual Booleans are wrapped in `!!(...)` so only the intended correlation
guards determine these controlled populations. Their complete CEL evaluation
remains in both routes. Add `--unwrapped-residuals` to expose residual guards
as well, including the broad `name == 'order-paid'` and `labels['tier'] == 'gold'`
guards. This mode measures anchor choice without isolating the correlation
guard; its actual fallback populations can differ from the table above and are
reported in the setup record. Put the flag before `--list-templates` to print
the unwrapped sources. Property/fuzz generators also retain unwrapped residuals.

## What is being compared

Every registration has its own captured CEL `event` value and references a
precompiled program. The corpus has **88 predicate families × eight paths × two
guard modes = 1,408 distinct source strings**. Indexed-only workloads use 704
programs; mixed workloads report their actual `used_programs` count. This models
many waiting runs of published functions, not one million independently compiled
source strings. The seed and xorshift steps match Ryusen's seeded payload
workload: `0x5259_5553_454e`.

| Coverage | Examples |
| --- | --- |
| Literals and operators | Null, bool, int, uint, double, hex, exponent, escaped/raw/Unicode strings, bytes; every arithmetic/comparison/logical operator and conditional |
| Collections and paths | Lists/maps, concatenation, four map-key types, membership, size, member/string/computed indexing, root identifiers |
| Standard macros | `has`, `all`, `exists`, `exists_one`, both `map` forms, `filter`; nested/empty/map iteration and local `event`/`async` shadowing |
| Additional inherited macros | Two-variable predicates, `existsOne`, `transformList`/`transformMap` with and without filters, `cel.bind` |
| Functions | String matching, regex, conversions, `dyn`/`type`, timestamp/duration construction, arithmetic and accessors |

Guard permutations include reversed equality, bracket access and root-scoped
identifiers. Guard position and AND association vary. The corpus test executes
every complete program with a positive control, so macro coverage is not merely
syntactic. Full scans still benefit from normal CEL short-circuiting; selective
events do not execute every residual on every registration. The all-match control
forces every used residual to execute.

The full scan evaluates the original CEL program for every registration. The
indexed route extracts each indexed incoming path once, fetches candidate IDs,
adds all unindexed registrations, then evaluates the **same complete CEL program**
for every candidate. It does not replace the residual with Rust code. Both routes
use the same activation implementation and collect matching IDs. Every measured
event asserts exact match-set equality outside the timers.

Candidate selection plus CEL confirmation is timed end to end. Parsing,
registration/index construction, warm-up and result-set canonicalization are
outside those timers. Build time and RSS before/after index construction are
reported separately. Route order alternates. Percentiles are nearest-rank sample
quantiles, not confidence bounds or a production-tail estimate. Every fourth
event is an intentional miss, except in the all-match control.

## Why pruning is conservative

The planner walks the checked AST, flattening only top-level AND. It recognizes
required string equalities between a static `async` field path and a decoded CEL
string literal or a captured `event` field path, including reversed equality and
string-key indexing. Each registration chooses the usable guard with the fewest
currently assigned anchors in its posting list; ties retain AST order. This
insertion-time heuristic replaces first-usable-guard selection without adding
another frequency table. It counts indexed anchors, not all guard occurrences
or incoming event frequencies, so it is not an optimal selectivity estimate.
Existing registrations do not rebalance when populations change; removal and
reinsertion reconsider the choice using current counts.

One anchor remains sufficient for conservative pruning: a true expression must
satisfy every extracted equality, whichever one is chosen. Complete CEL
confirmation checks every remaining conjunct. There is no global registration
scan or million-entry visited bitmap in candidate selection, and no posting-list
intersection or extra index entry per guard.

OR, NOT, macros, computed paths, numeric/null/aggregate keys and unsupported
operations stay ordinary CEL. No suitable guard means an unindexed registration,
not an invalid expression. The prototype's bindings contain only the two JSON-like
root values; qualified-variable namespaces and custom functions are not supported
environments for this selector.

One anchor per registration avoids duplicate candidates. Slot backpointers allow
swap-removal and reinsertion without scanning a hot posting list. Each run times
up to 10,000 remove/reinsert pairs and checks match sets again. Tests additionally
cover absent keys, nulls, numeric fallback, escaped literals, opaque OR/NOT,
macro-local shadowing, multiple conjuncts, removal, rebinding and slot reuse.

## Differential properties and fuzzing

`make check` runs shrinking property tests over recursively generated Boolean
expressions and typed/malformed input values. The independent reference scans
every live registration. Each case compares matching IDs with candidate selection
plus original CEL evaluation across five events, eight slots and sixteen
remove/rebind/reuse operations. It also rejects duplicate or dead candidates.
Dedicated controls check invalid source rejection and specific runtime errors.
Neither route turns an error into a match; skipped-error counts need not agree.

```sh
nix develop --command cargo test -p cel-core --example guard_index --locked
nix develop .#fuzz --command make fuzz-smoke
```

The separate, pinned nightly shell runs AddressSanitizer/libFuzzer. The structured
`guard_index` target shares the differential oracle but generates its own bounded
expression trees from bytes. `compile` feeds arbitrary UTF-8 to parse/check/program
construction, accepting explicit compile errors rather than catching panics.
CI runs both for thirty seconds and retains crashing inputs. Writable corpora and
artifacts live under ignored `fuzz/corpus` and `fuzz/artifacts`; committed seeds
stay unchanged. Longer campaigns can use `cargo fuzz run TARGET -- -max_total_time=N`.

## What this does not establish

This measures a single-threaded pure matcher on Linux; RSS includes allocator
effects and is not a precise index allocation count. It does not measure database
reads, transactions, persistence, event ingestion, network delivery, restart,
large path inventories, multiple workspaces/event names, or production capacity.
All registrations belong to one already-routed event-name/workspace partition.

This covers the listed JSON-context constructs, not every CEL expression or
overload. Protobuf messages/enums/extensions, optional values, custom functions,
and the opt-in strings/math/encoders libraries are outside the corpus. The engine
currently rejects nested `dyn` bracket access (`async['data']['key']`) and infers
two-`dyn` division incorrectly in `async.data.amount / async.data.scale > 0`.
The corpus uses supported single-level bracket access and explicit numeric casts
where needed; it does not classify those rejection bugs as CEL incompatibility
introduced by the index. Structured fuzz inputs stop at 768 bytes and generated
trees at three recursive levels; these campaigns are not exhaustive.

Match-set preservation is not preservation of error logs for skipped programs.
Source compilation now has size/depth ceilings and native evaluation offers
opt-in fuel/logical allocation limits; see the root README for their boundaries.
This benchmark still uses unbounded `eval` with trusted generated data, not the
bounded API. Strict CEL conformance and publication policy remain separate work.
This prototype does not establish a complete runtime sandbox, and dense or
unindexed populations still require linear work.
