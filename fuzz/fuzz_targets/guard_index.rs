#![no_main]
#![allow(dead_code)]

#[path = "../../crates/cel-core/examples/guard_index/corpus.rs"]
mod corpus;
#[path = "../../crates/cel-core/examples/guard_index/differential.rs"]
mod differential;
#[path = "../../crates/cel-core/examples/guard_index/index.rs"]
mod index;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if data.len() <= 768 {
        differential::check(&differential::source_from_bytes(data), data);
    }
});
