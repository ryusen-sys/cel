#![no_main]

use cel_core::{CelType, Env, EvalLimits, MapActivation, Value};

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if data.len() > 4096 {
        return;
    }
    let env = Env::with_standard_library()
        .with_variable("event", CelType::Dyn)
        .with_variable("async", CelType::Dyn);
    // Keep early-refusal paths and enough budget to reach native regex helpers.
    let limits = if data.first().is_some_and(|byte| byte & 1 != 0) {
        EvalLimits {
            fuel: 16_777_216,
            allocation_bytes: 1_048_576,
        }
    } else {
        EvalLimits {
            fuel: 8_192,
            allocation_bytes: 262_144,
        }
    };
    let compile = |source: &str| {
        if let Ok(ast) = env.compile(source) {
            assert!(ast.is_checked());
            let program = env.program(&ast).unwrap();
            let mut activation = MapActivation::new();
            let value = Value::map([
                ("data", Value::list(data.iter().take(64).copied())),
                (
                    "text",
                    Value::from(String::from_utf8_lossy(data).into_owned()),
                ),
            ]);
            activation.insert("event", value.clone());
            activation.insert("async", value);
            if let Err(error) = program.eval_with_limits(&activation, limits) {
                assert!(error.span.start <= error.span.end);
                assert!(error.span.end <= source.len());
                assert!(source.is_char_boundary(error.span.start));
                assert!(source.is_char_boundary(error.span.end));
            }
        }
    };
    if let Ok(source) = std::str::from_utf8(data) {
        compile(source);
    }
    // Random source rarely grows syntactically valid deep trees. Derive
    // structured adversarial expressions too, including the old crash shape.
    // Successful compiles also run with small fatal runtime budgets. Panics
    // escape the target; ordinary errors and positioned refusals are permitted.
    if let [low, high, shape, ..] = data {
        let depth = usize::from(u16::from_le_bytes([*low, *high])) % 2048;
        if shape % 7 == 6 {
            compile(&format!("cel.block([{}0],0)", "0,".repeat(depth)));
            return;
        }
        let (prefix, suffix) = match shape % 7 {
            0 => ("(", ")"),
            1 => ("!", ""),
            2 => ("true?", ":false"),
            3 => ("[", "]"),
            4 => ("[0].all(x,", ")"),
            _ => ("", "&&true"),
        };
        compile(&format!(
            "{}true{}",
            prefix.repeat(depth),
            suffix.repeat(depth)
        ));
    }
});
