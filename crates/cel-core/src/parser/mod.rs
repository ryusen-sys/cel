//! CEL (Common Expression Language) parser.

mod lexer;
pub mod macros;
mod parse;

use crate::types::{Span, SpannedExpr};

pub use macros::{
    ArgCount, Macro, MacroContext, MacroExpander, MacroExpansion, MacroRegistry, MacroStyle,
};
pub use parse::MacroCalls;

/// A parse error with source location.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at {}..{}",
            self.message, self.span.start, self.span.end
        )
    }
}

impl std::error::Error for ParseError {}

/// Options for parsing CEL expressions.
#[derive(Debug, Clone, Default)]
pub struct ParseOptions {
    /// Custom macro registry. If None, uses standard CEL macros.
    pub macros: Option<MacroRegistry>,
}

impl ParseOptions {
    /// Create parse options with default settings (standard macros).
    pub fn new() -> Self {
        Self::default()
    }

    /// Create parse options with a custom macro registry.
    pub fn with_macros(macros: MacroRegistry) -> Self {
        Self {
            macros: Some(macros),
        }
    }

    /// Create parse options with no macros (all macro calls become regular calls).
    pub fn without_macros() -> Self {
        Self {
            macros: Some(MacroRegistry::new()),
        }
    }
}

/// Result of parsing a CEL expression.
///
/// Supports error recovery: may return both an AST and errors.
/// The AST may contain `Expr::Error` nodes where parsing failed.
/// Includes macro_calls map for IDE features.
#[derive(Debug, Clone)]
pub struct ParseResult {
    /// The parsed AST, if any parsing succeeded.
    /// May contain `Expr::Error` nodes for unparseable sections.
    /// Macros (has, all, exists, exists_one, map, filter) are expanded inline.
    pub ast: Option<SpannedExpr>,
    /// Map of comprehension/expansion IDs to original macro call expressions.
    /// Used for IDE features like hover to show the original source form.
    pub macro_calls: MacroCalls,
    /// Any parse errors encountered.
    pub errors: Vec<ParseError>,
}

impl ParseResult {
    /// Returns true if parsing completed without errors.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty() && self.ast.is_some()
    }

    /// Returns true if there are any parse errors.
    pub fn is_err(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Converts to a Result, discarding partial AST on error.
    pub fn into_result(self) -> Result<SpannedExpr, Vec<ParseError>> {
        if self.errors.is_empty() {
            self.ast.ok_or_else(Vec::new)
        } else {
            Err(self.errors)
        }
    }

    /// Unwraps the errors, panicking if there are none.
    pub fn unwrap_err(self) -> Vec<ParseError> {
        if self.errors.is_empty() {
            panic!("called unwrap_err on a ParseResult with no errors");
        }
        self.errors
    }
}

/// Parse a CEL expression from source.
///
/// Compilation is bounded to 100,000 source bytes, 64 nested expression
/// parses, and an expanded tree of 10,000 nodes and depth 128. A leaf has
/// depth one; cloned subtrees count once per occurrence. Exceeding a limit
/// returns a positioned error, not a partial AST. These library ceilings are
/// not a host application's expression policy or a runtime resource budget.
/// Custom macro callbacks are trusted code and must bound their own work.
///
/// Returns a `ParseResult` which may contain both a partial AST and errors
/// when error recovery is successful. Macros (has, all, exists, exists_one,
/// map, filter) are expanded inline during parsing.
///
/// The `macro_calls` field in the result maps comprehension/expansion IDs
/// to the original macro call expressions, which is useful for IDE features.
pub fn parse(input: &str) -> ParseResult {
    parse_with_options(input, ParseOptions::new())
}

/// Parse a CEL expression with custom options.
///
/// This allows configuring the macro registry used during parsing.
/// Use `ParseOptions::without_macros()` to disable macro expansion,
/// or `ParseOptions::with_macros(registry)` for custom macros.
pub fn parse_with_options(input: &str, options: ParseOptions) -> ParseResult {
    // Reject before lexing, which allocates tokens and decodes literals.
    const MAX_SOURCE_BYTES: usize = 100_000;
    if input.len() > MAX_SOURCE_BYTES {
        return ParseResult {
            ast: None,
            macro_calls: MacroCalls::new(),
            errors: vec![ParseError {
                message: format!("source exceeds byte limit of {MAX_SOURCE_BYTES}"),
                span: 0..input.len(),
            }],
        };
    }

    // First, lex the input
    let tokens = match lexer::lex(input) {
        Ok(tokens) => tokens,
        Err(e) => {
            return ParseResult {
                ast: None,
                macro_calls: MacroCalls::new(),
                errors: vec![ParseError {
                    message: e.message,
                    span: e.span,
                }],
            };
        }
    };

    // Get macro registry
    let macros = options.macros.unwrap_or_else(MacroRegistry::standard);

    // Parse the tokens (with inline macro expansion)
    let (ast, parse_errors, macro_calls) = parse::parse_tokens_with_macros(&tokens, macros);

    let errors: Vec<ParseError> = parse_errors
        .into_iter()
        .map(|e| ParseError {
            message: e.message,
            span: e.span,
        })
        .collect();

    ParseResult {
        ast,
        macro_calls,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CompileError, EmptyActivation, Env, Value};
    use proptest::prelude::*;

    fn assert_limit_error(source: &str, limit: &str) {
        let parsed = parse(source);
        assert!(parsed.ast.is_none(), "limit must not return an AST");
        assert!(parsed.macro_calls.is_empty());
        assert!(
            parsed.errors.iter().any(|e| e.message.contains(limit)),
            "{:?}",
            parsed.errors
        );
        for error in parsed.errors {
            assert!(
                source.get(error.span).is_some(),
                "error must have a valid byte span"
            );
        }
        assert!(matches!(
            Env::with_standard_library().compile(source),
            Err(CompileError::Parse(_))
        ));
    }

    #[test]
    fn compilation_limits_do_not_exhaust_the_stack() {
        // Isolate aborts: a regressed parser must fail this test, not kill the
        // entire test runner. Exercise both building and dropping the trees.
        const CHILD: &str = "CEL_PARSER_LIMIT_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "parser::tests::compilation_limits_do_not_exhaust_the_stack",
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
        // A shallow argument can expand into a long binding spine. A smaller
        // stack distinguishes preflight rejection from recursively dropping
        // thousands of already-constructed bindings after a late check.
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                assert_limit_error(
                    &format!("cel.block([{}0],0)", "0,".repeat(9998)),
                    "depth limit",
                );
            })
            .unwrap()
            .join()
            .unwrap();
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                // The original 4,004-byte crash, plus recursion through every
                // expression-bearing syntax and chains built by iterative rules.
                for (prefix, leaf, suffix) in [
                    ("(", "true", ")"),
                    ("[", "0", "]"),
                    ("{0:", "0", "}"),
                    ("f(", "0", ")"),
                    ("a[", "0", "]"),
                    ("T{x:", "0", "}"),
                    ("true?", "0", ":0"),
                    ("false?0:", "0", ""),
                    ("[0].all(x,", "true", ")"),
                ] {
                    let source = format!("{}{}{}", prefix.repeat(2000), leaf, suffix.repeat(2000));
                    assert_limit_error(&source, "nesting limit");
                }
                for source in [
                    format!("{}true", "!".repeat(5000)),
                    format!("{}1", "-".repeat(5000)),
                    format!("1{}", "+1".repeat(5000)),
                    format!("1{}", "*1".repeat(5000)),
                    format!("1{}", "<1".repeat(5000)),
                    format!("true{}", "&&true".repeat(5000)),
                    format!("false{}", "||false".repeat(5000)),
                    format!("a{}", ".x".repeat(5000)),
                    format!("a{}", "[0]".repeat(5000)),
                    format!("f{}", "()".repeat(5000)),
                    format!("[0]{}", ".map(x,x)".repeat(1000)),
                ] {
                    assert_limit_error(&source, "depth limit");
                }
                // Safe maximum nesting must survive the checker, AST cloning,
                // interpreter, unparser and recursive destruction too.
                let source = format!("{}true{}", "(".repeat(63), ")".repeat(63));
                let env = Env::with_standard_library();
                let ast = env.compile(&source).unwrap();
                assert_eq!(
                    env.program(&ast).unwrap().eval(&EmptyActivation),
                    Value::Bool(true)
                );
                assert_eq!(ast.clone().to_cel_string(), "true");
                for (source, expected) in [
                    (format!("{}true", "!".repeat(127)), false),
                    (format!("true{}", "&&true".repeat(127)), true),
                ] {
                    let ast = env.compile(&source).unwrap();
                    assert_eq!(
                        env.program(&ast).unwrap().eval(&EmptyActivation),
                        Value::Bool(expected)
                    );
                    assert!(!ast.clone().to_cel_string().is_empty());
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn compilation_limits_include_boundaries_and_expanded_macros() {
        let source = format!("true{}", " ".repeat(99_996));
        assert!(parse(&source).is_ok());
        assert_limit_error(&(source + " "), "byte limit");
        let source = format!("'{}'", "é".repeat(50_000));
        assert_limit_error(&source, "byte limit");

        // A list node and 9,999 leaf nodes are exactly at the node ceiling.
        let source = format!("[{}0]", "0,".repeat(9998));
        assert!(parse(&source).is_ok());
        assert_limit_error(&format!("[{}0]", "0,".repeat(9999)), "node limit");
        // Much smaller source trees must also count nodes introduced by CEL
        // macros, rather than counting only source tokens or unique IDs.
        assert_limit_error(
            &format!("[{}true]", "[].all(x,true),".repeat(1500)),
            "node limit",
        );

        for (prefix, leaf, suffix) in [("(", "true", ")"), ("[", "0", "]"), ("f(", "0", ")")] {
            assert!(parse(&format!(
                "{}{}{}",
                prefix.repeat(63),
                leaf,
                suffix.repeat(63)
            ))
            .is_ok());
            assert_limit_error(
                &format!("{}{}{}", prefix.repeat(64), leaf, suffix.repeat(64)),
                "nesting limit",
            );
        }
        for (leaf, operator) in [
            ("1", "+1"),
            ("1", "*1"),
            ("1", "<1"),
            ("true", "&&true"),
            ("false", "||false"),
            ("a", ".x"),
            ("a", "[0]"),
            ("f", "()"),
        ] {
            assert!(parse(&format!("{leaf}{}", operator.repeat(127))).is_ok());
            assert_limit_error(&format!("{leaf}{}", operator.repeat(128)), "depth limit");
        }
        assert!(parse(&format!("{}true", "!".repeat(127))).is_ok());
        assert_limit_error(&format!("{}true", "!".repeat(128)), "depth limit");
        let env = Env::with_standard_library();
        let source = format!("cel.block([{}0],42)", "0,".repeat(126));
        let ast = env.compile(&source).unwrap();
        assert_eq!(
            env.program(&ast).unwrap().eval(&EmptyActivation),
            Value::Int(42)
        );
        assert_limit_error(
            &format!("cel.block([{}0],42)", "0,".repeat(127)),
            "depth limit",
        );
        // A short slot list must still check the depth of its result subtree.
        assert_limit_error(
            &format!("cel.block([0],{}true)", "!".repeat(127)),
            "depth limit",
        );
        for (source, expected) in [
            ("-9223372036854775808", Value::Int(i64::MIN)),
            ("- -1", Value::Int(1)),
            ("!!false", Value::Bool(false)),
        ] {
            // The iterative unary parser preserves order and the signed
            // minimum literal rule; unchecked parsing permits runtime errors.
            let ast = env.parse_only(source).unwrap();
            assert_eq!(env.program(&ast).unwrap().eval(&EmptyActivation), expected);
        }
        let ast = env.parse_only("!-1").unwrap();
        let Value::Error(error) = env.program(&ast).unwrap().eval(&EmptyActivation) else {
            panic!("unary order must preserve the runtime type error");
        };
        assert_eq!(error.kind, crate::EvalErrorKind::TypeMismatch);
    }

    #[test]
    fn macro_clones_count_each_occurrence_not_unique_ids() {
        let mut macros = MacroRegistry::new();
        macros.register(Macro::new(
            "dup",
            MacroStyle::Global,
            ArgCount::Exact(1),
            |ctx, span, _, mut args| {
                let arg = args.remove(0);
                MacroExpansion::Expanded(crate::Spanned::new(
                    ctx.next_id(),
                    crate::Expr::Binary {
                        op: crate::BinaryOp::And,
                        left: Box::new(arg.clone()),
                        right: Box::new(arg),
                    },
                    span,
                ))
            },
        ));
        // Full binary trees: 8,191 occurrences fit, 16,383 don't. There are
        // only a few dozen distinct IDs in either expanded expression.
        let source = format!("{}true{}", "dup(".repeat(12), ")".repeat(12));
        assert!(parse_with_options(&source, ParseOptions::with_macros(macros.clone())).is_ok());
        let source = format!("{}true{}", "dup(".repeat(13), ")".repeat(13));
        let parsed = parse_with_options(&source, ParseOptions::with_macros(macros));
        assert!(parsed.ast.is_none());
        assert!(parsed
            .errors
            .iter()
            .any(|e| e.message.contains("node limit")));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        #[test]
        fn mixed_nesting_fails_or_preserves_the_value(layers in prop_oneof![
            prop::collection::vec(0u8..4, 0..24), prop::collection::vec(0u8..4, 24..200)
        ]) {
            let mut source = "true".to_string();
            let mut value = true;
            for layer in layers {
                source = match layer {
                    0 => format!("({source})"),
                    1 => { value = !value; format!("!({source})") },
                    2 => format!("true ? ({source}) : false"),
                    _ => format!("[0].all(x,({source}))"),
                };
            }
            let env = Env::with_standard_library();
            match env.compile(&source) {
                Ok(ast) => prop_assert_eq!(env.program(&ast).unwrap().eval(&EmptyActivation), Value::Bool(value)),
                Err(CompileError::Parse(errors)) => {
                    prop_assert!(errors.iter().any(|e| e.message.contains("limit")));
                    prop_assert!(errors.iter().all(|e| source.get(e.span.clone()).is_some()));
                }
                other => prop_assert!(false, "unexpected compile result: {other:?}"),
            }
        }
    }
}
