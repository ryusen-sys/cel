//! Seeded full-CEL-scan versus required-string-guard prototype.
//! Run with `cargo run --release -p cel-core --example guard_index -- --help`.

mod index;

use std::hint::black_box;
use std::time::Instant;

use cel_core::{Activation, CelType, Env, Value, ValueMap};
use index::{GuardIndex, Template};

const SEED: u64 = 0x5259_5553_454e;
const FIELDS: [&str; 8] = [
    "customer", "order", "account", "region", "session", "device", "invoice", "shipment",
];

struct Random(u64);

impl Random {
    // Same xorshift steps and default seed as Ryusen's seeded payload workload.
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

struct Registration {
    template: usize,
    captured: Value,
}

struct Bindings<'a> {
    captured: &'a Value,
    incoming: &'a Value,
}

impl Activation for Bindings<'_> {
    fn resolve(&self, name: &str) -> Option<Value> {
        match name {
            "event" => Some(self.captured.clone()),
            "async" => Some(self.incoming.clone()),
            _ => None,
        }
    }
}

fn envelope(entries: impl IntoIterator<Item = (String, Value)>) -> Value {
    Value::map([("data", Value::map(entries))])
}

fn environment() -> Env {
    Env::with_standard_library()
        .with_variable("event", CelType::Dyn)
        .with_variable("async", CelType::Dyn)
}

struct Evaluation {
    matches: Vec<u32>,
    calls: usize,
    errors: usize,
}

fn evaluate(
    templates: &[Template],
    registrations: &[Registration],
    incoming: &Value,
    ids: impl Iterator<Item = u32>,
) -> Evaluation {
    let mut result = Evaluation {
        matches: Vec::new(),
        calls: 0,
        errors: 0,
    };
    for id in ids {
        let registration = &registrations[id as usize];
        let value = templates[registration.template].program.eval(&Bindings {
            captured: &registration.captured,
            incoming,
        });
        result.calls += 1;
        match value {
            Value::Bool(true) => result.matches.push(id),
            Value::Bool(false) => {}
            _ => result.errors += 1,
        }
    }
    black_box(result)
}

#[derive(Clone, Copy)]
struct Workload {
    name: &'static str,
    cardinality: usize,
    unindexed_per_mille: usize,
    hot: bool,
    all_match: bool,
}

const WORKLOADS: [Workload; 7] = [
    Workload {
        name: "selective",
        cardinality: 1_000_000,
        unindexed_per_mille: 0,
        hot: false,
        all_match: false,
    },
    Workload {
        name: "lower-cardinality",
        cardinality: 1_000,
        unindexed_per_mille: 0,
        hot: false,
        all_match: false,
    },
    Workload {
        name: "fallback-0.1pct",
        cardinality: 1_000_000,
        unindexed_per_mille: 1,
        hot: false,
        all_match: false,
    },
    Workload {
        name: "fallback-1pct",
        cardinality: 1_000_000,
        unindexed_per_mille: 10,
        hot: false,
        all_match: false,
    },
    Workload {
        name: "hot-key",
        cardinality: 1_000_000,
        unindexed_per_mille: 0,
        hot: true,
        all_match: false,
    },
    Workload {
        name: "all-unindexed",
        cardinality: 1_000_000,
        unindexed_per_mille: 1_000,
        hot: false,
        all_match: false,
    },
    Workload {
        name: "all-match",
        cardinality: 1,
        unindexed_per_mille: 0,
        hot: false,
        all_match: true,
    },
];

fn templates() -> Vec<Template> {
    let env = environment();
    [false, true]
        .into_iter()
        .flat_map(|opaque| {
            FIELDS.iter().map(move |field| {
                let equality = if opaque {
                    format!("!(async.data.{field} != event.data.{field})")
                } else {
                    format!("async.data.{field} == event.data.{field}")
                };
                format!("{equality} && async.data.amount > event.data.minimum")
            })
        })
        .map(|source| Template::compile(&env, &source))
        .collect()
}

fn registrations(count: usize, workload: Workload, random: &mut Random) -> Vec<Registration> {
    (0..count)
        .map(|id| {
            let field = random.next() as usize % FIELDS.len();
            let key = if workload.hot && random.next().is_multiple_of(2) {
                "hot".to_owned()
            } else {
                format!("key-{}", random.next() as usize % workload.cardinality)
            };
            let minimum = if workload.all_match {
                -1
            } else {
                (random.next() % 200) as i64
            };
            let opaque = id % 1_000 < workload.unindexed_per_mille;
            Registration {
                template: field + usize::from(opaque) * FIELDS.len(),
                captured: envelope([
                    (FIELDS[field].to_owned(), key.into()),
                    ("minimum".to_owned(), minimum.into()),
                ]),
            }
        })
        .collect()
}

fn events(
    count: usize,
    workload: Workload,
    registrations: &[Registration],
    random: &mut Random,
) -> Vec<Value> {
    (0..count)
        .map(|event| {
            let target = &registrations[random.next() as usize % registrations.len()];
            let field = target.template % FIELDS.len();
            let target_data = <&ValueMap>::try_from(&target.captured).unwrap();
            let target_data =
                <&ValueMap>::try_from(target_data.get(&"data".into()).unwrap()).unwrap();
            let key = target_data.get(&FIELDS[field].into()).unwrap().clone();
            let mut entries: Vec<_> = FIELDS
                .iter()
                .enumerate()
                .map(|(position, name)| {
                    let value = if !workload.all_match && event % 4 == 3 {
                        // Deliberate misses, including on the fallback path.
                        format!("absent-{event}").into()
                    } else if position == field {
                        key.clone()
                    } else {
                        format!("key-{}", random.next() as usize % workload.cardinality).into()
                    };
                    ((*name).to_owned(), value)
                })
                .collect();
            entries.push(("amount".to_owned(), 100i64.into()));
            envelope(entries)
        })
        .collect()
}

fn rss_kib() -> usize {
    // This measurement is intentionally Linux-specific, like the benchmark orb.
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
        .unwrap()
}

fn percentile(values: &[u128], percent: usize) -> u128 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted[(sorted.len() * percent).div_ceil(100).saturating_sub(1)]
}

fn run(workload: Workload, count: usize, event_count: usize, seed: u64) {
    let mut random = Random(seed);
    let started = Instant::now();
    let templates = templates();
    let registrations = registrations(count, workload, &mut random);
    let events = events(event_count, workload, &registrations, &mut random);
    let preparation = started.elapsed();
    let base_rss = rss_kib();
    let started = Instant::now();
    let mut index = GuardIndex::default();
    for (id, registration) in registrations.iter().enumerate() {
        index.insert(
            id as u32,
            &templates[registration.template],
            &registration.captured,
        );
    }
    let build = started.elapsed();
    println!(
        "{{\"record\":\"setup\",\"workload\":\"{}\",\"registrations\":{count},\"templates\":{},\"events\":{event_count},\"seed\":{seed},\"preparation_ns\":{},\"index_build_ns\":{},\"rss_before_index_kib\":{base_rss},\"rss_with_index_kib\":{},\"unindexed\":{},\"paths\":{},\"concurrency\":1}}",
        workload.name, templates.len(), preparation.as_nanos(), build.as_nanos(), rss_kib(), index.unindexed_count(), index.path_count(),
    );

    // Warm both routes using the same event; no parsing in either timed route.
    let warm_scan = evaluate(&templates, &registrations, &events[0], 0..count as u32);
    let mut warm_indexed = evaluate(
        &templates,
        &registrations,
        &events[0],
        index.candidates(&events[0]).into_iter(),
    );
    warm_indexed.matches.sort_unstable();
    assert_eq!(warm_scan.matches, warm_indexed.matches);

    let mut scan_times = Vec::new();
    let mut indexed_times = Vec::new();
    let mut selection_times = Vec::new();
    let mut candidate_counts = Vec::new();
    for (event_id, event) in events.iter().enumerate() {
        let scan = || {
            let start = Instant::now();
            let result = evaluate(&templates, &registrations, event, 0..count as u32);
            (start.elapsed(), result)
        };
        let indexed = || {
            let start = Instant::now();
            let candidates = index.candidates(event);
            let selection = start.elapsed();
            let result = evaluate(&templates, &registrations, event, candidates.into_iter());
            (start.elapsed(), selection, result)
        };
        // Alternate ordering to avoid always giving one route the warmer cache.
        let ((scan_time, scan_result), (index_time, selection, mut indexed_result)) =
            if event_id % 2 == 0 {
                let scan_result = scan();
                (scan_result, indexed())
            } else {
                let index_result = indexed();
                (scan(), index_result)
            };
        // Set comparison is outside both timers. No expression is removed from
        // CEL or replaced by a hand-coded residual in either route.
        indexed_result.matches.sort_unstable();
        assert_eq!(
            scan_result.matches, indexed_result.matches,
            "event {event_id}"
        );
        assert_eq!(scan_result.errors, 0, "generated data must be valid");
        assert_eq!(indexed_result.errors, 0);
        assert_eq!(scan_result.calls, count);
        println!(
            "{{\"record\":\"event\",\"workload\":\"{}\",\"event\":{event_id},\"scan_ns\":{},\"indexed_ns\":{},\"selection_ns\":{},\"scan_calls\":{},\"indexed_calls\":{},\"matches\":{},\"match_sets_equal\":true}}",
            workload.name, scan_time.as_nanos(), index_time.as_nanos(), selection.as_nanos(), scan_result.calls, indexed_result.calls, scan_result.matches.len(),
        );
        scan_times.push(scan_time.as_nanos());
        indexed_times.push(index_time.as_nanos());
        selection_times.push(selection.as_nanos());
        candidate_counts.push(indexed_result.calls as u128);
    }

    let churn_count = count.min(10_000);
    let started = Instant::now();
    for _ in 0..churn_count {
        let id = random.next() as usize % count;
        let registration = &registrations[id];
        index.remove(id as u32);
        index.insert(
            id as u32,
            &templates[registration.template],
            &registration.captured,
        );
    }
    let churn = started.elapsed();
    for event in events.iter().take(2) {
        let scan = evaluate(&templates, &registrations, event, 0..count as u32);
        let mut indexed = evaluate(
            &templates,
            &registrations,
            event,
            index.candidates(event).into_iter(),
        );
        indexed.matches.sort_unstable();
        assert_eq!(scan.matches, indexed.matches, "after churn");
    }
    let scan_total: u128 = scan_times.iter().sum();
    let indexed_total: u128 = indexed_times.iter().sum();
    let candidates: u128 = candidate_counts.iter().sum();
    println!(
        "{{\"record\":\"summary\",\"workload\":\"{}\",\"registrations\":{count},\"events\":{event_count},\"scan_p50_ns\":{},\"scan_p95_ns\":{},\"indexed_p50_ns\":{},\"indexed_p95_ns\":{},\"selection_p50_ns\":{},\"selection_p95_ns\":{},\"mean_candidates\":{},\"max_candidates\":{},\"skipped_fraction\":{},\"total_time_speedup\":{},\"churn_pairs\":{churn_count},\"churn_ns\":{},\"all_match_sets_equal\":true}}",
        workload.name, percentile(&scan_times, 50), percentile(&scan_times, 95), percentile(&indexed_times, 50), percentile(&indexed_times, 95), percentile(&selection_times, 50), percentile(&selection_times, 95), candidates as f64 / event_count as f64, candidate_counts.iter().max().unwrap(), 1.0 - candidates as f64 / (count * event_count) as f64, scan_total as f64 / indexed_total as f64, churn.as_nanos(),
    );
}

fn main() {
    let mut count = 1_000_000;
    let mut event_count = 24;
    let mut seed = SEED;
    let mut workload = "selective".to_owned();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--registrations" => count = args.next().unwrap().parse().unwrap(),
            "--events" => event_count = args.next().unwrap().parse().unwrap(),
            "--seed" => seed = args.next().unwrap().parse().unwrap(),
            "--workload" => workload = args.next().unwrap(),
            "--help" => {
                println!("guard_index [--registrations 1000000] [--events 24] [--seed {SEED}] [--workload NAME]\nWorkloads: {}\nLinux RSS measurement; release build; one process/thread; NDJSON on stdout.", WORKLOADS.map(|w| w.name).join(", "));
                return;
            }
            _ => panic!("unknown argument {arg}"),
        }
    }
    assert!((1..=u32::MAX as usize).contains(&count));
    assert!(event_count > 0 && seed != 0);
    let workload = WORKLOADS
        .into_iter()
        .find(|candidate| candidate.name == workload)
        .expect("unknown workload");
    run(workload, count, event_count, seed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compare(sources: &[&str], captured: &Value, incoming: &Value) -> (Vec<u32>, usize) {
        let env = environment();
        let templates: Vec<_> = sources
            .iter()
            .map(|source| Template::compile(&env, source))
            .collect();
        let registrations: Vec<_> = (0..sources.len())
            .map(|template| Registration {
                template,
                captured: captured.clone(),
            })
            .collect();
        let mut index = GuardIndex::default();
        for (id, registration) in registrations.iter().enumerate() {
            index.insert(id as u32, &templates[id], &registration.captured);
        }
        let scan = evaluate(
            &templates,
            &registrations,
            incoming,
            0..sources.len() as u32,
        );
        let candidates = index.candidates(incoming);
        let mut indexed = evaluate(&templates, &registrations, incoming, candidates.into_iter());
        indexed.matches.sort_unstable();
        assert_eq!(scan.matches, indexed.matches);
        (scan.matches, indexed.calls)
    }

    #[test]
    fn opaque_branches_scopes_and_exact_values_preserve_matches() {
        let captured = envelope([("key".into(), "x".into())]);
        let incoming = envelope([
            ("key".into(), "z".into()),
            ("other".into(), "y".into()),
            ("n".into(), Value::Int(9_007_199_254_740_993)),
            ("u".into(), Value::UInt(u64::MAX)),
            ("escaped".into(), "a\nb".into()),
        ]);
        let sources = [
            "async.data.key == event.data.key",
            "!(async.data.key == 'x' && async.data.other == 'y')",
            "async.data.key == 'x' || true",
            "async.data.key == 'x' || async.data.n > 0",
            "async.data.other == 'y' && (async.data.key == 'x' || true)",
            "async.data.other == 'y' && [1, 2].exists(async, async == 2)",
            "async.data.n == 9007199254740993",
            "async.data.u == 18446744073709551615u",
            r#"async.data.escaped == "a\nb""#,
            "async.data.missing == null || true",
            "async.data['other'] == 'y' && async.data.key == 'x'",
            "'y' == async.data.other",
        ];
        let (matches, calls) = compare(&sources, &captured, &incoming);
        assert_eq!(matches, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 11]);
        assert_eq!(calls, sources.len() - 1);
        for incoming in [
            envelope([]),
            envelope([("key".into(), Value::Null)]),
            envelope([("key".into(), Value::Int(1))]),
        ] {
            compare(&sources, &captured, &incoming);
        }
    }

    #[test]
    fn seeded_match_sets_survive_removal_rebinding_and_slot_reuse() {
        let templates = templates();
        let workload = WORKLOADS[3];
        let mut random = Random(SEED);
        let mut registrations = registrations(512, workload, &mut random);
        let events = events(40, workload, &registrations, &mut random);
        let mut index = GuardIndex::default();
        for (id, registration) in registrations.iter().enumerate() {
            index.insert(
                id as u32,
                &templates[registration.template],
                &registration.captured,
            );
        }
        for round in 0..3 {
            let removed: Vec<_> = (0..registrations.len())
                .filter(|id| id % 7 == round)
                .collect();
            for &id in &removed {
                index.remove(id as u32);
            }
            for event in &events {
                let scan = evaluate(
                    &templates,
                    &registrations,
                    event,
                    (0..registrations.len() as u32).filter(|id| *id as usize % 7 != round),
                );
                let mut indexed = evaluate(
                    &templates,
                    &registrations,
                    event,
                    index.candidates(event).into_iter(),
                );
                indexed.matches.sort_unstable();
                assert_eq!(scan.matches, indexed.matches);
            }
            for id in removed {
                // Rebind the reused ID to a different captured event and template.
                registrations[id].captured = registrations[(id + 1) % registrations.len()]
                    .captured
                    .clone();
                registrations[id].template = registrations[(id + 1) % registrations.len()].template;
                let registration = &registrations[id];
                index.insert(
                    id as u32,
                    &templates[registration.template],
                    &registration.captured,
                );
            }
        }
        for event in &events {
            let scan = evaluate(
                &templates,
                &registrations,
                event,
                0..registrations.len() as u32,
            );
            let candidates = index.candidates(event);
            let mut unique = candidates.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(unique.len(), candidates.len());
            let mut indexed = evaluate(&templates, &registrations, event, candidates.into_iter());
            indexed.matches.sort_unstable();
            assert_eq!(scan.matches, indexed.matches);
        }
    }
}
