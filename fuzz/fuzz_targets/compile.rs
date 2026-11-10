#![no_main]

use cel_core::{CelType, Env};

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if data.len() > 4096 {
        return;
    }
    let env = Env::with_standard_library()
        .with_variable("event", CelType::Dyn)
        .with_variable("async", CelType::Dyn);
    let compile = |source: &str| {
        if let Ok(ast) = env.compile(source) {
            assert!(ast.is_checked());
            assert!(env.program(&ast).is_ok());
        }
    };
    if let Ok(source) = std::str::from_utf8(data) {
        compile(source);
    }
    // Random source rarely grows syntactically valid deep trees. Derive
    // structured adversarial expressions too, including the old crash shape.
    // This covers compilation and destruction, not runtime resource safety.
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
