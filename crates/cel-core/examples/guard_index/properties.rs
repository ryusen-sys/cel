use cel_core::{CelType, Env, EvalErrorKind, MapActivation, Value};
use proptest::prelude::*;

use super::{corpus, differential};

#[test]
fn malformed_sources_and_runtime_errors_do_not_become_matches() {
    let env = Env::with_standard_library().with_variable("async", CelType::Dyn);
    for source in ["(", "'unterminated", "unknown == 1", "1 + 'x'"] {
        assert!(env.compile(source).is_err(), "{source}");
    }
    let activation: MapActivation = [("async".to_owned(), corpus::incoming([]))]
        .into_iter()
        .collect();
    for (source, kind) in [
        ("1 / 0 == 1", EvalErrorKind::DivisionByZero),
        ("[1][2] == 0", EvalErrorKind::IndexOutOfBounds),
        ("int('x') == 0", EvalErrorKind::InvalidConversion),
        ("async.data.missing == 0", EvalErrorKind::KeyNotFound),
    ] {
        let ast = env.compile(source).unwrap();
        let value = env.program(&ast).unwrap().eval(&activation);
        assert!(
            matches!(value, Value::Error(ref error) if error.kind == kind),
            "{source}: {value:?}"
        );
        differential::check(source, &[]);
    }
}

fn expression() -> BoxedStrategy<String> {
    let leaf = prop_oneof![
        Just("async.data.key == event.data.key".to_owned()),
        Just("event.data['key'] == async.data['key']".to_owned()),
        Just("async.data.key == 'a'".to_owned()),
        Just("[{'data': {'key': 'a'}}].all(async, async.data.key == 'a')".to_owned()),
        Just("[1, 2].exists(event, event == 2)".to_owned()),
        Just("double(async.data.amount) / double(async.data.scale) > 0.0".to_owned()),
        (0..corpus::PREDICATES.len()).prop_map(|id| corpus::PREDICATES[id].1.to_owned()),
    ];
    leaf.prop_recursive(3, 32, 3, |inner| {
        prop_oneof![
            inner.clone().prop_map(|source| format!("!({source})")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}) && ({b})")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a}) || ({b})")),
            (inner.clone(), inner.clone(), inner)
                .prop_map(|(a, b, c)| format!("({a}) ? ({b}) : ({c})")),
        ]
    })
    .boxed()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn generated_expressions_and_lifecycle_preserve_matches(
        source in expression(),
        data in proptest::collection::vec(any::<u8>(), 128..768),
    ) {
        differential::check(&source, &data);
    }

    #[test]
    fn fuzz_decoder_preserves_matches(data in proptest::collection::vec(any::<u8>(), 0..768)) {
        differential::check(&differential::source_from_bytes(&data), &data);
    }
}
