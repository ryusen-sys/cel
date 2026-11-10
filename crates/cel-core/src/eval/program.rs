//! Compiled CEL program ready for evaluation.
//!
//! A `Program` combines a parsed/checked AST with a function registry,
//! providing a convenient interface for evaluating expressions.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use super::limits::Meter;
use super::proto_registry::ProtoRegistry;
use super::{
    Activation, EmptyActivation, EvalLimitError, EvalLimitKind, EvalLimits, Evaluator,
    FunctionRegistry, MapActivation, Value,
};
use crate::Ast;

/// A compiled CEL program ready for evaluation.
///
/// The program holds a reference to the AST and a function registry,
/// providing a convenient interface for evaluating the expression
/// against different variable bindings.
#[derive(Clone)]
pub struct Program {
    ast: Arc<Ast>,
    functions: Arc<FunctionRegistry>,
    proto_registry: Option<Arc<dyn ProtoRegistry>>,
    abbreviations: Option<Arc<HashMap<String, String>>>,
    strong_enums: bool,
}

impl Program {
    /// Create a new program from an AST and function registry.
    pub fn new(ast: Arc<Ast>, functions: Arc<FunctionRegistry>) -> Self {
        Self {
            ast,
            functions,
            proto_registry: None,
            abbreviations: None,
            strong_enums: true,
        }
    }

    /// Create a new program with a type registry.
    pub fn with_proto_registry(
        ast: Arc<Ast>,
        functions: Arc<FunctionRegistry>,
        proto_registry: Arc<dyn ProtoRegistry>,
    ) -> Self {
        Self {
            ast,
            functions,
            proto_registry: Some(proto_registry),
            abbreviations: None,
            strong_enums: true,
        }
    }

    /// Create a new program with abbreviations.
    pub fn with_abbreviations(
        ast: Arc<Ast>,
        functions: Arc<FunctionRegistry>,
        abbreviations: HashMap<String, String>,
    ) -> Self {
        Self {
            ast,
            functions,
            proto_registry: None,
            abbreviations: Some(Arc::new(abbreviations)),
            strong_enums: true,
        }
    }

    /// Create a new program with a type registry and abbreviations.
    pub fn with_proto_registry_and_abbreviations(
        ast: Arc<Ast>,
        functions: Arc<FunctionRegistry>,
        proto_registry: Arc<dyn ProtoRegistry>,
        abbreviations: HashMap<String, String>,
    ) -> Self {
        Self {
            ast,
            functions,
            proto_registry: Some(proto_registry),
            abbreviations: Some(Arc::new(abbreviations)),
            strong_enums: true,
        }
    }

    /// Use legacy (weak) enum mode where enum values are returned as plain integers.
    pub fn with_legacy_enums(mut self) -> Self {
        self.strong_enums = false;
        self
    }

    /// Get the AST for this program.
    pub fn ast(&self) -> &Ast {
        &self.ast
    }

    /// Get the function registry for this program.
    pub fn functions(&self) -> &FunctionRegistry {
        &self.functions
    }

    /// Evaluate the program with the given variable bindings.
    pub fn eval(&self, activation: &dyn Activation) -> Value {
        self.eval_with_container(activation, "")
    }

    /// Evaluate native CEL with one shared fuel/allocation meter.
    ///
    /// Ordinary CEL errors remain `Ok(Value::Error(..))`; resource refusals are
    /// fatal `Err`s even inside `|| true`, `has()` or comprehensions. Inputs are
    /// borrowed and validated (maximum value depth 64) before evaluation clones
    /// them. Lazy activations, protobuf registries and function callbacks are
    /// deliberately unavailable: they cannot yet consume this meter.
    ///
    /// Use source-compiled ASTs. This is logical cumulative accounting, not an
    /// allocator/RSS cap, a wall-clock deadline or protection from arbitrary
    /// host code. Input preparation and existing input/program storage are not
    /// charged. The conservative schedule is not yet a stable cost-model ABI.
    pub fn eval_with_limits(
        &self,
        activation: &MapActivation,
        limits: EvalLimits,
    ) -> Result<Value, EvalLimitError> {
        let meter = Rc::new(Meter::new(limits, self.ast.expr().span.clone()));
        if self.proto_registry.is_some() {
            meter.stop(EvalLimitKind::UnmeteredOperation);
        } else {
            for value in activation.values() {
                if meter.validate(value, 1).is_err() {
                    break;
                }
            }
        }
        if let Some(error) = meter.failure() {
            return Err(error);
        }
        let value = self
            .evaluator(activation, "")
            .with_meter(Rc::clone(&meter))
            .eval(self.ast.expr());
        match meter.failure() {
            Some(error) => Err(error),
            None => Ok(value),
        }
    }

    /// Evaluate the program with the given variable bindings and container namespace.
    ///
    /// The container is used for resolving unqualified type names following
    /// C++ namespace rules. For example, with container "cel.expr.conformance.proto3"
    /// and type name "TestAllTypes", resolution tries:
    /// 1. cel.expr.conformance.proto3.TestAllTypes
    /// 2. cel.expr.conformance.TestAllTypes
    /// 3. cel.expr.TestAllTypes
    /// 4. cel.TestAllTypes
    /// 5. TestAllTypes
    pub fn eval_with_container(&self, activation: &dyn Activation, container: &str) -> Value {
        self.evaluator(activation, container).eval(self.ast.expr())
    }

    fn evaluator<'a>(&'a self, activation: &'a dyn Activation, container: &str) -> Evaluator<'a> {
        let mut evaluator = Evaluator::new(activation, &self.functions);

        // Pass type info from checked AST
        if let Some(type_info) = self.ast.type_info() {
            evaluator = evaluator.with_reference_map(&type_info.reference_map);
        }

        // Pass type registry
        if let Some(ref proto_registry) = self.proto_registry {
            evaluator = evaluator.with_proto_registry(proto_registry.as_ref());
        }

        // Set container for type resolution
        if !container.is_empty() {
            evaluator = evaluator.with_container(container);
        }

        // Pass abbreviations
        if let Some(ref abbreviations) = self.abbreviations {
            evaluator = evaluator.with_abbreviations(abbreviations);
        }

        // Pass strong enum setting
        if !self.strong_enums {
            evaluator = evaluator.with_legacy_enums();
        }

        evaluator
    }

    /// Evaluate the program with no variable bindings.
    pub fn eval_empty(&self) -> Value {
        self.eval(&EmptyActivation)
    }
}

impl std::fmt::Debug for Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Program")
            .field("ast", &self.ast)
            .field("functions", &format!("{} functions", self.functions.len()))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::MapActivation;
    use crate::parse;

    fn create_program(source: &str) -> Program {
        let result = parse(source);
        assert!(result.errors.is_empty());
        let ast = Ast::new_unchecked(result.ast.unwrap(), source);
        Program::new(Arc::new(ast), Arc::new(FunctionRegistry::new()))
    }

    #[test]
    fn test_eval_literal() {
        let program = create_program("42");
        assert_eq!(program.eval_empty(), Value::Int(42));
    }

    #[test]
    fn test_eval_with_variables() {
        let program = create_program("x + y");
        let mut activation = MapActivation::new();
        activation.insert("x", Value::Int(1));
        activation.insert("y", Value::Int(2));
        assert_eq!(program.eval(&activation), Value::Int(3));
    }

    #[test]
    fn test_reuse_program() {
        let program = create_program("x * 2");

        let mut act1 = MapActivation::new();
        act1.insert("x", Value::Int(5));
        assert_eq!(program.eval(&act1), Value::Int(10));

        let mut act2 = MapActivation::new();
        act2.insert("x", Value::Int(21));
        assert_eq!(program.eval(&act2), Value::Int(42));
    }

    const GENEROUS: EvalLimits = EvalLimits {
        fuel: 100_000_000,
        allocation_bytes: 100_000_000,
    };

    fn checked_program(source: &str) -> Program {
        let env = crate::Env::with_standard_library()
            .with_variable("x", crate::CelType::Dyn)
            .with_variable("xs", crate::CelType::list(crate::CelType::Int));
        env.program(&env.compile(source).unwrap()).unwrap()
    }

    #[test]
    fn bounded_errors_are_fatal_but_cel_errors_still_suppress() {
        let activation = MapActivation::new();
        for source in [
            "1 / 0 == 0 || true",
            "[1].all(x, 1 / 0 == 0) || true",
            "has({}.missing.child)",
        ] {
            let program = checked_program(source);
            assert_eq!(
                program.eval_with_limits(&activation, GENEROUS).unwrap(),
                program.eval(&activation)
            );
        }
        let program = checked_program("1 / 0");
        assert!(matches!(
            program.eval_with_limits(&activation, GENEROUS),
            Ok(Value::Error(_))
        ));
        for source in [
            "1 / 0 == 0 || true",
            "has({}.missing.child)",
            "[1,2].all(x, true) || true",
        ] {
            let error = checked_program(source)
                .eval_with_limits(
                    &activation,
                    EvalLimits {
                        fuel: 2,
                        ..GENEROUS
                    },
                )
                .unwrap_err();
            assert_eq!(error.kind, EvalLimitKind::Fuel, "{source}");
            assert!(error.span.end <= source.len());
        }
        let program = checked_program("false && [1,2].all(x, true)");
        assert_eq!(
            program
                .eval_with_limits(
                    &activation,
                    EvalLimits {
                        fuel: 4,
                        ..GENEROUS
                    }
                )
                .unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn nested_macros_share_fuel_and_each_evaluation_starts_fresh() {
        let mut activation = MapActivation::new();
        activation.insert("xs", Value::list(0..20));
        let limits = EvalLimits {
            fuel: 10_000,
            ..GENEROUS
        };
        let simple = checked_program("xs.all(a, true)");
        assert_eq!(
            simple.eval_with_limits(&activation, limits).unwrap(),
            Value::Bool(true)
        );
        let nested = checked_program("xs.all(a, xs.all(b, true))");
        for _ in 0..2 {
            assert_eq!(
                nested
                    .eval_with_limits(&activation, limits)
                    .unwrap_err()
                    .kind,
                EvalLimitKind::Fuel
            );
            assert_eq!(
                nested.eval_with_limits(&activation, GENEROUS).unwrap(),
                Value::Bool(true)
            );
        }
    }

    #[test]
    fn copies_and_native_helpers_cannot_hide_behind_one_node() {
        let mut activation = MapActivation::new();
        activation.insert("x", "é".repeat(50_000));
        // Returning an existing Arc is allowed; copying it is not.
        assert!(checked_program("x")
            .eval_with_limits(
                &activation,
                EvalLimits {
                    allocation_bytes: 8_192,
                    ..GENEROUS
                }
            )
            .is_ok());
        for source in ["x + x", "bytes(x)", "x[0]"] {
            let program = create_program(source);
            assert_eq!(
                program
                    .eval_with_limits(
                        &activation,
                        EvalLimits {
                            allocation_bytes: 32_768,
                            ..GENEROUS
                        }
                    )
                    .unwrap_err()
                    .kind,
                EvalLimitKind::Allocation,
                "{source}"
            );
            assert_eq!(
                program.eval_with_limits(&activation, GENEROUS).unwrap(),
                program.eval(&activation)
            );
        }
        for source in ["size(x)", "x.contains('z')", "int(x)", "x == x", "x in [x]"] {
            assert_eq!(
                create_program(source)
                    .eval_with_limits(
                        &activation,
                        EvalLimits {
                            fuel: 1_000,
                            ..GENEROUS
                        }
                    )
                    .unwrap_err()
                    .kind,
                EvalLimitKind::Fuel,
                "{source}"
            );
        }
    }

    #[test]
    fn regex_work_and_compiled_state_refusals_are_not_suppressed() {
        let activation = MapActivation::new();
        for source in ["'abc'.matches('a')", "'abc'.matches('a') || true"] {
            let program = checked_program(source);
            assert_eq!(
                program
                    .eval_with_limits(
                        &activation,
                        EvalLimits {
                            fuel: 100_000,
                            ..GENEROUS
                        }
                    )
                    .unwrap_err()
                    .kind,
                EvalLimitKind::Fuel
            );
            assert_eq!(
                program.eval_with_limits(&activation, GENEROUS).unwrap(),
                Value::Bool(true)
            );
        }
        let program = checked_program("'a'.matches('a{100000}') || true");
        assert_eq!(
            program
                .eval_with_limits(&activation, GENEROUS)
                .unwrap_err()
                .kind,
            EvalLimitKind::Allocation
        );
        let program = checked_program("'a'.matches('[') || true");
        assert_eq!(
            program.eval_with_limits(&activation, GENEROUS).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn bounded_native_values_have_a_depth_ceiling() {
        let mut value = Value::Int(7);
        for _ in 0..63 {
            value = Value::list([value]);
        }
        let mut activation = MapActivation::new();
        activation.insert("x", value.clone());
        let program = checked_program("x");
        assert_eq!(
            program.eval_with_limits(&activation, GENEROUS).unwrap(),
            value
        );
        activation.insert("x", Value::list([value]));
        assert_eq!(
            program
                .eval_with_limits(&activation, GENEROUS)
                .unwrap_err()
                .kind,
            EvalLimitKind::ValueDepth
        );
    }

    #[test]
    fn presence_checks_charge_map_key_work() {
        let field = "f".repeat(4096);
        let source = format!("has(x.{field}) || true");
        let mut activation = MapActivation::new();
        activation.insert("x", Value::map([(field, 1)]));
        let program = checked_program(&source);
        assert_eq!(
            program.eval_with_limits(&activation, GENEROUS).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            program
                .eval_with_limits(
                    &activation,
                    EvalLimits {
                        fuel: 10_000,
                        ..GENEROUS
                    }
                )
                .unwrap_err()
                .kind,
            EvalLimitKind::Fuel
        );
    }

    #[test]
    fn source_compiled_runtime_depth_is_safe_on_a_two_megabyte_stack() {
        const CHILD: &str = "CEL_RUNTIME_LIMIT_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "eval::program::tests::source_compiled_runtime_depth_is_safe_on_a_two_megabyte_stack",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                for source in [
                    format!("{}true", "!".repeat(127)),
                    format!("true{}", "&&true".repeat(127)),
                    format!("{}true{}", "[0].all(v,".repeat(18), ")".repeat(18)),
                ] {
                    let program = checked_program(&source);
                    let result = program
                        .eval_with_limits(&MapActivation::new(), GENEROUS)
                        .unwrap();
                    assert!(matches!(result, Value::Bool(_)), "{source}");
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn bounded_evaluation_refuses_host_callbacks_before_calling_them() {
        let env = crate::Env::with_standard_library().with_function(
            crate::FunctionDecl::new("host").with_overload(
                crate::OverloadDecl::function("host", vec![], crate::CelType::Bool)
                    .with_impl(|_| panic!("unmetered callback ran")),
            ),
        );
        let program = env
            .program(&env.compile("host() || true").unwrap())
            .unwrap();
        assert_eq!(
            program
                .eval_with_limits(&MapActivation::new(), GENEROUS)
                .unwrap_err()
                .kind,
            EvalLimitKind::UnmeteredOperation
        );
    }

    #[test]
    fn allocation_exhaustion_preserves_the_innermost_utf8_span() {
        let mut activation = MapActivation::new();
        activation.insert("x", "é".repeat(50_000));
        let source = "false || (x + x == 'é')";
        let error = checked_program(source)
            .eval_with_limits(
                &activation,
                EvalLimits {
                    allocation_bytes: 32_768,
                    ..GENEROUS
                },
            )
            .unwrap_err();
        assert_eq!(error.kind, EvalLimitKind::Allocation);
        let start = source.find("x + x").unwrap();
        assert_eq!(error.span, start..start + "x + x".len());
    }

    #[test]
    fn allocation_is_cumulative_across_macro_iterations() {
        let mut activation = MapActivation::new();
        activation.insert("x", "s".repeat(100_000));
        let limits = EvalLimits {
            allocation_bytes: 4_000_000,
            ..GENEROUS
        };
        assert_eq!(
            checked_program("[1].all(v, size(x+x)>0)")
                .eval_with_limits(&activation, limits)
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            checked_program("[1,2].all(v, size(x+x)>0)")
                .eval_with_limits(&activation, limits)
                .unwrap_err()
                .kind,
            EvalLimitKind::Allocation
        );
    }

    #[test]
    fn oversized_time_inputs_are_ordinary_errors_not_panics() {
        for source in [
            "duration('170141183460469231731687303715884105727h')",
            "duration('170141183460469231731687303715884105727ns1ns')",
            "timestamp('2000-01-01T00:00:00Z').getHours('2147483647:00')",
            "timestamp('2000-01-01T00:00:00Z').getHours('00:2147483647')",
            "timestamp('2000-01-01T00:00:00Z').getHours('596523:15')",
        ] {
            let result = checked_program(source)
                .eval_with_limits(&MapActivation::new(), GENEROUS)
                .unwrap();
            assert!(result.is_error(), "{source}");
        }
    }

    #[test]
    fn type_diagnostics_charge_native_metadata_before_copying_it() {
        let value = Value::Enum(crate::EnumValue::new("E".repeat(1_000_000), 0));
        for value in [
            value.clone(),
            Value::optional_some(Value::optional_some(value)),
        ] {
            let mut activation = MapActivation::new();
            activation.insert("x", value);
            for source in [
                "!x",
                "!x || true",
                "x ? true : false",
                "x[0]",
                "x.all(v, true)",
                "bytes(x)",
            ] {
                let program = checked_program(source);
                let result = program.eval_with_limits(
                    &activation,
                    EvalLimits {
                        allocation_bytes: 50_000,
                        ..GENEROUS
                    },
                );
                assert!(result.is_err(), "unmetered diagnostic: {source}");
                assert_eq!(result.unwrap_err().kind, EvalLimitKind::Allocation);
                let actual = program.eval_with_limits(&activation, GENEROUS).unwrap();
                let expected = program.eval(&activation);
                match (&actual, &expected) {
                    (Value::Error(actual), Value::Error(expected)) => {
                        assert_eq!(actual.kind, expected.kind);
                        assert!(actual.message == expected.message, "{source}");
                    }
                    _ => assert_eq!(actual, expected, "{source}"),
                }
            }
        }
    }

    mod properties {
        use super::*;
        use proptest::prelude::*;

        fn values() -> impl Strategy<Value = Value> {
            prop_oneof![
                Just(Value::Null),
                any::<bool>().prop_map(Value::Bool),
                any::<i64>().prop_map(Value::Int),
                any::<u64>().prop_map(Value::UInt),
                (-1e6f64..1e6).prop_map(Value::Double),
                ".{0,24}".prop_map(Value::from),
                prop::collection::vec(any::<u8>(), 0..24).prop_map(Value::from),
                ("[A-Za-z.]{0,256}", any::<i32>())
                    .prop_map(|(name, value)| Value::Enum(crate::EnumValue::new(name, value))),
                (-62_135_596_800i64..=253_402_300_799i64)
                    .prop_map(|seconds| Value::Timestamp(crate::Timestamp::from_seconds(seconds))),
                (-315_576_000_000i64..=315_576_000_000i64)
                    .prop_map(|seconds| Value::Duration(crate::Duration::from_seconds(seconds))),
                Just(Value::optional_none()),
            ]
            .prop_recursive(3, 64, 8, |inner| {
                prop_oneof![
                    prop::collection::vec(inner.clone(), 0..8).prop_map(Value::list),
                    prop::collection::btree_map("[a-z]{0,4}", inner.clone(), 0..8)
                        .prop_map(Value::map),
                    inner.prop_map(Value::optional_some),
                ]
            })
        }

        fn sources() -> impl Strategy<Value = String> {
            prop::sample::select(vec![
                "x == 1",
                "1 in x",
                "has(x.field)",
                "x.contains('é')",
                "x[0] == 3",
                "size(x) > 1",
                "bytes(x) == b'abc'",
                "int(x) > 0",
                "string(x) == 's'",
                "x.getHours('UTC') > 0",
                "x.all(v, v == 1)",
                "x.exists(v, size(v)>0)",
                "x.exists_one(v, v != null)",
                "x.map(v, v).size() == 2",
                "x.filter(v, v != null).size() == 1",
                "x.matches('a.*')",
            ])
            .prop_map(str::to_owned)
            .prop_recursive(3, 32, 3, |inner| {
                prop_oneof![
                    inner.clone().prop_map(|a| format!("!({a})")),
                    (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}) && ({b})")),
                    (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}) || ({b})")),
                    (inner.clone(), inner.clone(), inner)
                        .prop_map(|(a, b, c)| format!("({a}) ? ({b}) : ({c})")),
                ]
            })
        }

        proptest! {
            #[test]
            fn bounded_native_evaluation_preserves_values_and_ordinary_errors(
                source in sources(), value in values(),
                fuel in 0u64..20_000, allocation_bytes in 0u64..200_000,
            ) {
                let program = create_program(&source);
                let mut activation = MapActivation::new();
                activation.insert("x", value);
                let expected = program.eval(&activation);
                let actual = program.eval_with_limits(&activation, GENEROUS).unwrap();
                match (&expected, &actual) {
                    (Value::Error(expected), Value::Error(actual)) => {
                        prop_assert_eq!(expected.kind, actual.kind);
                        prop_assert_eq!(&expected.message, &actual.message);
                    }
                    _ => prop_assert_eq!(&expected, &actual),
                }
                match program.eval_with_limits(&activation, EvalLimits { fuel, allocation_bytes }) {
                    Ok(small) => match (&actual, &small) {
                        (Value::Error(a), Value::Error(b)) => prop_assert_eq!(a.kind, b.kind),
                        _ => prop_assert_eq!(actual, small),
                    },
                    Err(error) => {
                        prop_assert!(matches!(error.kind, EvalLimitKind::Fuel | EvalLimitKind::Allocation));
                        prop_assert!(error.span.start <= error.span.end);
                        prop_assert!(error.span.end <= source.len());
                        prop_assert!(source.is_char_boundary(error.span.start));
                        prop_assert!(source.is_char_boundary(error.span.end));
                    }
                }
            }
        }
    }
}
