# Guard-index prototype

Compare a full CEL scan with conservative candidate selection over one million
wait registrations. This is an in-memory experiment, not a production API or a
claim of CEL conformance. The parser, checker and evaluator are unchanged.

## Run in the pinned toolchain

From this repository, use the existing Ryusen Nix shell; replace the path with
your Ryusen checkout. No additional dependencies are needed.

```sh
nix develop /path/to/ryusen --command cargo test -p cel-core --example guard_index --locked
nix develop /path/to/ryusen --command cargo build -p cel-core --release --example guard_index --locked
target/release/examples/guard_index --registrations 1000000 --events 24 --workload selective
```

Run each workload in a fresh process for meaningful RSS comparisons. Standard
output is NDJSON: setup, per-event measurements and a summary. Do not run other
CPU-intensive jobs concurrently. `--help` lists arguments and workloads.

| Workload | Registration distribution |
| --- | --- |
| `selective` | Required equality; one-million-value key space |
| `lower-cardinality` | Required equality; one-thousand-value key space |
| `fallback-0.1pct` | 1,000 of one million registrations have an opaque predicate |
| `fallback-1pct` | 10,000 of one million registrations have an opaque predicate |
| `hot-key` | Approximately half the registrations share one hot value |
| `all-unindexed` | Every predicate is opaque to the selector |
| `all-match` | Every registration matches every generated event |

Opaque templates wrap equality as `!(a != b)`. They are deliberately outside
this planner's coverage, not intrinsically impossible to index. This keeps the
underlying predicate work comparable while measuring the cost of fallback.

## What is being compared

Every registration has its own captured CEL `event` value and a reference to one
of sixteen precompiled templates over eight paths. Indexed-only workloads use
eight of those templates. This models many waiting runs of published functions,
not one million independently compiled source strings. The default seed and
xorshift steps match Ryusen's seeded payload workload: `0x5259_5553_454e`.

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
string-key indexing. Each registration chooses its first usable guard as one
anchor; complete CEL confirmation checks every remaining conjunct. There is no
global registration scan or million-entry visited bitmap in candidate selection.

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

## What this does not establish

This measures a single-threaded pure matcher on Linux; RSS includes allocator
effects and is not a precise index allocation count. It does not measure database
reads, transactions, persistence, event ingestion, network delivery, restart,
large path inventories, multiple workspaces/event names, or production capacity.
All registrations belong to one already-routed event-name/workspace partition.

Match-set preservation is not preservation of error logs for skipped programs.
The existing engine's semantic/safety gaps remain. Native budgets, allocation
limits, strict CEL conformance and publication policy still require the hardening
work described in the CEL scope document. This prototype does not make those
problems disappear, and dense or unindexed populations still require linear work.
