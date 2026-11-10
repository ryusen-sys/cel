#![no_main]

use cel_core::{CelType, Env};

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    // Bounded smoke coverage, not a parser-depth safety guarantee. Increase
    // these limits only once the inherited recursive parser is hardened.
    if data.len() <= 512 {
        if let Ok(source) = std::str::from_utf8(data) {
            let env = Env::with_standard_library()
                .with_variable("event", CelType::Dyn)
                .with_variable("async", CelType::Dyn);
            if let Ok(ast) = env.compile(source) {
                assert!(ast.is_checked());
                assert!(env.program(&ast).is_ok());
            }
        }
    }
});
