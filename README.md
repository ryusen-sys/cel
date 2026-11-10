# CEL for Ryusen

A Rust implementation of the [Common Expression Language](https://github.com/google/cel-spec) (CEL), maintained by Ryusen.

This repository is a fork of [ponix-dev/cel-core](https://github.com/ponix-dev/cel-core), created by Simon Rall and the upstream contributors. It starts from upstream **0.5.1** ([77277ec](https://github.com/ponix-dev/cel-core/commit/77277ec6a336646d946ee549a5012116378b82e4)). We retain their MIT/Apache-2.0 licensing and notices. Thank you to the original authors for the parser, checker, evaluator and protobuf interoperability this work builds on.

The inherited pipeline includes parsing, type checking, evaluation and protobuf AST conversion for interoperation with [cel-go](https://github.com/google/cel-go) and [cel-cpp](https://github.com/google/cel-cpp). This fork is being evaluated and hardened for server-side use; **full CEL conformance and safe execution of untrusted expressions are not yet established**. Source compilation has depth and size ceilings, and native evaluation has an opt-in logical resource meter. Known gaps still include cost-model calibration, dynamic type inference and ordinary runtime error positions.

The [guard-index example](crates/cel-core/examples/guard_index/README.md) explores conservative candidate selection followed by complete CEL evaluation. It is an experiment, not a production matcher.

## Installation

```toml
[dependencies]
cel-core = { git = "https://github.com/ryusen-sys/cel", rev = "4b01f1fec93a25efd21bebe047e42497eea8d5b6" }

# Optional: proto wire format interop with cel-go/cel-cpp
cel-core-proto = { git = "https://github.com/ryusen-sys/cel", rev = "4b01f1fec93a25efd21bebe047e42497eea8d5b6" }
```

These pins select the initial prototype revision; choose a reviewed commit or release tag for your application. The existing `cel-core` and `cel-core-proto` packages on crates.io belong to upstream, not this fork. Our release pipeline publishes GitHub source archives only.

## The CEL Environment

The `Env` is the entry point for working with CEL expressions. It holds variable declarations, function definitions, and extensions, then compiles expressions into type-checked ASTs and evaluable programs.

```rust
use cel_core::{Env, CelType, Value, MapActivation};

let env = Env::with_standard_library()
    .with_variable("user", CelType::String)
    .with_variable("age", CelType::Int);

let ast = env.compile("age >= 21 && user.startsWith('admin')")?;
let program = env.program(&ast)?;

let mut activation = MapActivation::new();
activation.insert("user", "admin_alice");
activation.insert("age", 25);

let result = program.eval(&activation);
assert_eq!(result, Value::Bool(true));
```

### Compile once, evaluate many

Compilation (parsing + type checking) is the expensive step. Once you have a `Program`, evaluation is fast and can be called repeatedly with different variable bindings.

```rust
let env = Env::with_standard_library()
    .with_variable("age", CelType::Int)
    .with_variable("threshold", CelType::Int);
let ast = env.compile("age >= threshold")?;
let program = env.program(&ast)?;

for threshold in [18, 21, 65] {
    let mut activation = MapActivation::new();
    activation.insert("age", 25);
    activation.insert("threshold", threshold);
    let result = program.eval(&activation);
    println!("threshold {}: {}", threshold, result);
}
```

### Extensions

The inherited library includes opt-in CEL extensions. Extensions are separate from the core language; enable only those your application supports:

```rust
use cel_core::ext;

// All extensions
let env = Env::with_standard_library()
    .with_all_extensions();

// Or individually
let env = Env::with_standard_library()
    .with_extension(ext::string_extension())
    .with_extension(ext::math_extension())
    .with_extension(ext::encoders_extension())
    .with_extension(ext::optionals_extension());
```

**String extension** — `charAt`, `indexOf`, `lastIndexOf`, `substring`, `split`, `join`, `strings.quote`, `lowerAscii`, `upperAscii`, `replace`, `trim`, `reverse`, `format`

**Math extension** — `math.greatest`, `math.least`, `math.abs`, `math.sign`, `math.isNaN`, `math.isFinite`, `math.isInf`, `math.bitAnd`, `math.bitOr`, `math.bitNot`, `math.bitXor`, `math.bitShiftLeft`, `math.bitShiftRight`, `math.ceil`, `math.floor`, `math.round`, `math.trunc`

**Encoders extension** — `base64.encode`, `base64.decode`

**Optionals extension** — `optional.of`, `optional.none`, `optional.ofNonZeroValue`, `hasValue`, `value`, `or`, `orValue`

### Working with values

Variable bindings accept any type that implements `Into<Value>`. Results can be extracted back into Rust types with `TryFrom`:

```rust
let mut activation = MapActivation::new();
activation.insert("items", Value::list([1, 2, 3]));
activation.insert("config", Value::map([("host", "localhost"), ("port", "8080")]));

let result = program.eval(&activation);

// Extract Rust types from results
let n: i64 = (&result).try_into()?;
let s: &str = (&result).try_into()?;
let b: bool = (&result).try_into()?;
let list: &[Value] = (&result).try_into()?;
let map: &ValueMap = (&result).try_into()?;
```

### Proto message types

CEL expressions can operate on protobuf messages. Create a `ProstProtoRegistry`, load your proto descriptors, and pass it to the environment:

```rust
use std::sync::Arc;
use cel_core_proto::ProstProtoRegistry;

// Create registry (includes well-known types like Timestamp, Duration, etc.)
let mut registry = ProstProtoRegistry::new();

// Load your proto definitions from a file descriptor set
// (generated by protoc --descriptor_set_out, buf build, or prost-build)
registry.add_file_descriptor_set(MY_FILE_DESCRIPTOR_SET)?;

let env = Env::with_standard_library()
    .with_proto_registry(Arc::new(registry))
    .with_variable("request", CelType::message("my.package.Request"));
```

### Containers and abbreviations

Containers set a namespace for resolving qualified names. Abbreviations create shorthand aliases for fully-qualified type names:

```rust
let env = Env::with_standard_library()
    .with_container("my.package")
    .with_abbreviations(
        Abbreviations::from_qualified_names(&["my.package.Request"])?
    );
```

### Error handling

Source parsing rejects inputs over 100,000 bytes, parser nesting over 64 expression levels, and expanded trees over 10,000 nodes or depth 128 (a leaf has depth one). Macro-generated nodes and cloned subtree occurrences count toward the tree limits. Limit failures return positioned parse errors without a partial AST. These are library ceilings, not Ryusen's final manifest policy or hard limits on peak compilation memory. Custom macro callbacks are trusted code and must bound their own work; externally constructed or protobuf-imported ASTs bypass source parsing.

Ordinary evaluation errors are returned as `Value::Error`, subject to CEL's logical error suppression. The existing `eval` API is unbounded. Parsing limits alone are not a runtime sandbox.

```rust
let result = program.eval(&activation);
match &result {
    Value::Error(err) => println!("evaluation error: {}", err),
    value => println!("result: {}", value),
}
```

### Native evaluation limits

Use `Program::eval_with_limits(&MapActivation, EvalLimits)` to opt into one shared fuel and cumulative allocation meter. Child scopes and nested macros consume the same allowance. Ordinary CEL errors remain `Ok(Value::Error(..))`; resource refusals are fatal `Err(EvalLimitError)` values with UTF-8 byte spans and cannot be suppressed by `|| true`, `has()` or macros. Short-circuited branches are not evaluated.

```rust
use cel_core::{Env, EvalLimits, MapActivation, Value};

let env = Env::with_standard_library();
let program = env.program(&env.compile("[1, 2, 3].all(x, x > 0)")?)?;
let result = program.eval_with_limits(
    &MapActivation::new(),
    EvalLimits { fuel: 100_000, allocation_bytes: 1_000_000 },
)?;
assert_eq!(result, Value::Bool(true));
```

| Boundary | Current contract |
|---|---|
| Native values | Borrowed inputs and every expression result are structurally validated; maximum depth 64, including scalar leaves. Shared subtrees count by occurrence. Scalar invariants are not comprehensively validated. |
| Native work | Nodes, validation, iterations, lookups, comparisons, copies, conversions and string/regex work are charged. |
| Allocation | Conservative logical bytes, cumulative rather than live memory; temporary drops do not refund the allowance. |
| Regex | Bounded mode caps compiled state and DFA cache at 64 KiB each; a compiled-state refusal is fatal. |
| Host operations | Custom/lazy activations are not accepted. Function callbacks, extensions, protobuf registries/construction and message values are refused before execution. |
| Outside the meter | Input preparation, existing input/program storage and fixed evaluator setup; externally constructed/imported ASTs are not validated by this API. Use source-compiled ASTs. |

This is **not an allocator/RSS cap, a wall-clock deadline or a complete sandbox**. The initial schedule is deliberately conservative and uncalibrated: it reserves 4 KiB per expression, 8 KiB per comprehension iteration, and additional variable-size work/copy allowances. Traversing intermediate collections can itself be quadratic, so this API makes no throughput claim. Costs are not a stable ABI; callers must choose policy limits and bound input preparation separately. Ordinary CEL errors still lack source positions, and the inherited conformance harness primarily exercises the unbounded API.

> See the [examples](crates/cel-core/examples/) directory for more usage patterns including lists, maps, timestamps/durations, and error handling. For proto-related examples (enums, namespaces), see [cel-core-proto/examples](crates/cel-core-proto/examples/).

## Proto Wire Format

The `cel-core-proto` crate provides bidirectional conversion between cel-core's AST and the cel-spec protobuf format. This enables wire compatibility with cel-go and cel-cpp — you can compile an expression in Rust and send the serialized AST to a Go or C++ service for evaluation, or vice versa.

```rust
use cel_core::{Env, CelType};
use cel_core_proto::AstToProto;
use prost::Message;

let env = Env::with_standard_library()
    .with_variable("x", CelType::Int);

let ast = env.compile("x + 1")?;

// Convert to cel-spec proto format
let parsed_expr = ast.to_parsed_expr();
let checked_expr = ast.to_checked_expr()?;

// Serialize to bytes (wire-compatible with cel-go/cel-cpp)
let bytes = checked_expr.encode_to_vec();

// Deserialize from bytes received from another implementation
use cel_core_proto::CheckedExpr;
let decoded = CheckedExpr::decode(bytes.as_slice())?;
```


## Conformance

CI runs the inherited conformance harness against the **pinned** cel-spec submodule, in addition to unit, example and documentation tests. A green harness run is not proof of strict CEL conformance: some comparisons are permissive, and source audits and probes have found semantic and safety gaps outside its cases. See the [conformance README](crates/cel-core-conformance/README.md) for the exact boundary and commands. The [January 2026 failure report](CONFORMANCE_FAILURES.md) and recorded baseline are historical upstream snapshots, not current measurements.

## Crate Structure

| Crate | Description |
|-------|-------------|
| [**cel-core**](crates/cel-core) | CEL environment, parser, type checker, evaluator, and standard library |
| [**cel-core-proto**](crates/cel-core-proto) | Protobuf support: proto registry (`ProstProtoRegistry`), message values, AST conversion for wire compatibility with cel-go/cel-cpp |
| [**cel-core-conformance**](crates/cel-core-conformance) | Conformance testing against the official cel-spec test suite |

## Development

Use the pinned Nix shell for development and CI. The lock file pins the same Nix inputs used by Ryusen; the Rust toolchain is 1.98.0.

```bash
nix develop
make check                 # formatting, lint, tests and workflow checks
make conformance-setup     # initialize the pinned cel-spec submodule
make conformance           # inherited conformance harness; missing data fails
make conformance-report    # diagnostic report; does not update the baseline
nix flake check -L         # isolated library build and tests
```

For protobuf regeneration, the shell also supplies `buf`, `protoc-gen-prost` and `protoc-gen-prost-crate`; review generated diffs before committing them. Do not advance the cel-spec submodule implicitly during tests.

Pull request titles use Conventional Commits and become squash-merge subjects. See [releasing](docs/releasing.md) for allowed types, git-cliff commands, the release PR flow and repository settings. Fork changes are generated into [changelog.md](changelog.md); the original changelog is preserved byte-for-byte in [changelog.prev.md](changelog.prev.md).

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## References

- [CEL Spec](https://github.com/google/cel-spec) — Official specification and test suite
- [cel-go](https://github.com/google/cel-go) — Reference implementation in Go
- [CEL Language Definition](https://github.com/google/cel-spec/blob/master/doc/langdef.md) — Language syntax and semantics
