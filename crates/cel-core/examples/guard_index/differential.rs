//! Shared property/fuzz oracle. The reference route never consults the index.

use cel_core::{CelType, Env, MapActivation, Value};

use super::corpus;
use super::index::{GuardIndex, Template};

struct Input<'a>(std::slice::Iter<'a, u8>);

impl Input<'_> {
    fn byte(&mut self) -> u8 {
        self.0.next().copied().unwrap_or(0)
    }

    fn value(&mut self) -> Option<Value> {
        Some(match self.byte() % 16 {
            0..=4 => ["a", "b", "猫", "a\nb", ""][usize::from(self.byte()) % 5].into(),
            5 => return None,
            6 => Value::Null,
            7 => Value::Bool(self.byte().is_multiple_of(2)),
            8 => Value::Int(i64::from(self.byte()) - 128),
            9 => Value::Int(9_007_199_254_740_993),
            10 => Value::UInt(u64::MAX),
            11 => Value::Double(f64::from(self.byte()) - 128.0),
            12 => Value::Double(-0.0),
            13 => Value::from(&[self.byte(), self.byte()][..]),
            14 => Value::list([i64::from(self.byte()), -1]),
            _ => Value::map([("key", i64::from(self.byte()))]),
        })
    }

    fn event(&mut self) -> Value {
        let shape = self.byte() % 16;
        let mut fields = Vec::new();
        for name in ["key", "other", "amount", "scale", "items"] {
            if let Some(value) = self.value() {
                fields.push((name.to_owned(), value));
            }
        }
        match shape {
            0 => Value::Null,
            1 => Value::map([("data", Value::Int(1))]),
            2 => Value::map([] as [(&str, Value); 0]),
            _ => corpus::incoming(fields),
        }
    }
}

pub fn source_from_bytes(data: &[u8]) -> String {
    fn expression(input: &mut Input<'_>, depth: usize) -> String {
        let choice = input.byte() % 10;
        if depth != 0 && choice < 4 {
            let left = expression(input, depth - 1);
            return match choice {
                0 => format!("!({left})"),
                1 => format!("({left}) && ({})", expression(input, depth - 1)),
                2 => format!("({left}) || ({})", expression(input, depth - 1)),
                _ => format!(
                    "({left}) ? ({}) : ({})",
                    expression(input, depth - 1),
                    expression(input, depth - 1)
                ),
            };
        }
        match input.byte() % 8 {
            0 => "async.data.key == event.data.key".into(),
            1 => "event.data['key'] == async.data['key']".into(),
            2 => "[{'data': {'key': 'a'}}].exists(async, async.data.key == 'a')".into(),
            3 => "async.data.key == 'a\\nb'".into(),
            4 => "double(async.data.amount) / double(async.data.scale) > 0.0".into(),
            5 => "async.data.missing == null || true".into(),
            6 => "[1, 2].all(event, event > 0)".into(),
            _ => corpus::PREDICATES[usize::from(input.byte()) % corpus::PREDICATES.len()]
                .1
                .into(),
        }
    }
    expression(&mut Input(data.iter()), 3)
}

pub fn check(source: &str, data: &[u8]) {
    let env = Env::with_standard_library()
        .with_variable("event", CelType::Dyn)
        .with_variable("async", CelType::Dyn);
    let sources = [
        source.to_owned(),
        format!("async.data.key == event.data.key && ({source})"),
        format!("({source}) && ('a' == async.data.other)"),
        format!("async.data.key == event.data.key || ({source})"),
        "async.data.key == event.data.key".to_owned(),
        "async.data.key == event.data.key && async.data.other == 'a'".to_owned(),
    ];
    let templates: Vec<_> = sources
        .iter()
        .map(|source| Template::compile(&env, source))
        .collect();
    let mut input = Input(data.iter());
    let mut events = vec![
        corpus::incoming([("key".into(), "a".into()), ("other".into(), "a".into())]),
        corpus::incoming([("key".into(), "b".into())]),
        input.event(),
        input.event(),
    ];
    events.push(Value::map([("data", Value::Null)]));
    let mut model: Vec<Option<(usize, Value)>> = (0..8)
        .map(|id| Some((id % templates.len(), input.event())))
        .collect();
    // Keep a guaranteed well-typed, positive control alongside malformed data.
    model[0] = Some((4, corpus::incoming([("key".into(), "a".into())])));
    model[1] = Some((5, corpus::incoming([("key".into(), "a".into())])));
    let mut index = GuardIndex::default();
    for (id, registration) in model.iter().enumerate() {
        let (template, captured) = registration.as_ref().unwrap();
        index.insert(id as u32, &templates[*template], captured);
    }

    for step in 0..=16 {
        for incoming in &events {
            let mut candidates = index.candidates(incoming);
            candidates.sort_unstable();
            assert!(
                candidates.windows(2).all(|pair| pair[0] != pair[1]),
                "duplicate candidates"
            );
            assert!(
                candidates.iter().all(|&id| model[id as usize].is_some()),
                "removed registration selected"
            );
            let evaluate = |id: u32| {
                let (template, captured) = model[id as usize].as_ref().unwrap();
                let activation: MapActivation = [
                    ("event".to_owned(), captured.clone()),
                    ("async".to_owned(), incoming.clone()),
                ]
                .into_iter()
                .collect();
                matches!(
                    templates[*template].program.eval(&activation),
                    Value::Bool(true)
                )
            };
            let expected: Vec<_> = (0..model.len() as u32)
                .filter(|&id| model[id as usize].is_some() && evaluate(id))
                .collect();
            let actual: Vec<_> = candidates
                .into_iter()
                .filter(|&id| {
                    let (template, captured) = model[id as usize].as_ref().unwrap();
                    templates[*template].may_match(captured, incoming)
                })
                .filter(|&id| evaluate(id))
                .collect();
            assert_eq!(
                actual, expected,
                "source={source}, step={step}, event={incoming:?}, model={model:?}"
            );
        }
        if step == 16 {
            break;
        }
        let id = usize::from(input.byte()) % model.len();
        if model[id].take().is_some() {
            index.remove(id as u32);
        }
        if !input.byte().is_multiple_of(3) {
            let template = usize::from(input.byte()) % templates.len();
            let captured = input.event();
            index.insert(id as u32, &templates[template], &captured);
            model[id] = Some((template, captured));
        }
    }
}
