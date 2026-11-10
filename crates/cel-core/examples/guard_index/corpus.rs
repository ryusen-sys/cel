//! Standard, JSON-context CEL constructs; not an exhaustive conformance suite.

use cel_core::Value;

pub const PREDICATES: &[(&str, &str)] = &[
    ("addition", "async.data.amount + 2 == 102"),
    ("subtraction", "async.data.amount - 3 == 97"),
    ("multiplication", "async.data.scale * 3 == 6"),
    ("division", "async.data.amount / 4 == 25"),
    ("remainder", "async.data.amount % 7 == 2"),
    ("negation", "-async.data.amount == -100"),
    ("uint", "uint(async.data.amount) + 1u == 101u"),
    ("double", "double(async.data.amount) / 8.0 == 12.5"),
    ("hex", "async.data.amount == 0x64 && 0x64u == 100u"),
    ("exponent", "double(async.data.amount) == 1e2"),
    ("less", "async.data.scale < async.data.amount"),
    ("less-equal", "async.data.amount <= 100"),
    ("greater", "async.data.amount > 99"),
    ("greater-equal", "async.data.amount >= 100"),
    ("not-equal", "async.data.amount != 99"),
    ("logical-not", "!(async.data.amount < 100)"),
    (
        "logical-or",
        "async.data.amount < 0 || async.data.scale == 2",
    ),
    (
        "conditional",
        "async.data.scale > 0 ? async.data.amount == 100 : false",
    ),
    ("error-or-true", "async.data.missing == 1 || true"),
    ("false-and-error", "!(false && async.data.missing == 1)"),
    ("string-add", "async.data.name + '-ok' == 'order-paid-ok'"),
    ("contains", "async.data.name.contains('paid')"),
    ("starts-with", "async.data.name.startsWith('order')"),
    ("ends-with", "async.data.name.endsWith('paid')"),
    ("regex", "async.data.name.matches('^order-[a-z]+$')"),
    ("raw-string", "r'order-paid' == async.data.name"),
    ("escaped-string", "'a\\nb' == \"a\\nb\""),
    ("unicode", "size('猫é') == 2"),
    ("bytes", "b'abc' + b'd' == bytes('abcd')"),
    ("bytes-size", "size(bytes(async.data.name)) == 10"),
    ("string-conversion", "string(async.data.amount) == '100'"),
    ("int-conversion", "int('100') == async.data.amount"),
    ("bool-conversion", "bool('true') && bool(true)"),
    (
        "bytes-conversion",
        "string(bytes(async.data.name)) == async.data.name",
    ),
    ("dyn", "dyn(async.data.amount) == 100"),
    ("type", "type(async.data.amount) == int"),
    ("null", "null == null && type(null) == null_type"),
    ("list-add-index", "([1, 2] + [async.data.amount])[2] == 100"),
    ("list-in", "async.data.scale in [1, 2, 3]"),
    ("list-size", "async.data.items.size() == 3"),
    ("map-in", "'tier' in async.data.labels"),
    ("map-string-index", "async.data.labels['tier'] == 'gold'"),
    ("map-int-key", "{1: async.data.amount}[1] == 100"),
    ("map-uint-key", "{1u: async.data.amount}[1u] == 100"),
    ("map-bool-key", "{true: async.data.amount}[true] == 100"),
    ("map-size", "size(async.data.labels) == 2"),
    ("has", "has(async.data.amount) && !has(async.data.missing)"),
    ("all", "async.data.items.all(x, x > 0)"),
    (
        "exists",
        "async.data.items.exists(x, x == async.data.scale)",
    ),
    ("exists-one", "async.data.items.exists_one(x, x == 2)"),
    (
        "map",
        "async.data.items.map(x, x * async.data.scale) == [2, 4, 6]",
    ),
    (
        "map-with-filter",
        "async.data.items.map(x, x > 1, x * 2) == [4, 6]",
    ),
    ("filter", "async.data.items.filter(x, x > 1) == [2, 3]"),
    ("all-two", "async.data.items.all(i, x, i >= 0 && x > 0)"),
    (
        "exists-two",
        "async.data.items.exists(i, x, i == 1 && x == 2)",
    ),
    (
        "exists-one-two",
        "async.data.items.exists_one(i, x, i == 1 && x == 2)",
    ),
    ("exists-one-alias", "async.data.items.existsOne(x, x == 2)"),
    (
        "exists-one-two-alias",
        "async.data.items.existsOne(i, x, i == 1 && x == 2)",
    ),
    (
        "transform-list",
        "async.data.items.transformList(i, x, i + x) == [1, 3, 5]",
    ),
    (
        "transform-list-filter",
        "async.data.items.transformList(i, x, i > 0, x * 2) == [4, 6]",
    ),
    (
        "transform-map",
        "{'a': 2}.transformMap(k, v, v * 2) == {'a': 4}",
    ),
    (
        "transform-map-filter",
        "{'a': 2, 'b': 3}.transformMap(k, v, k == 'a', v * 2) == {'a': 4}",
    ),
    (
        "bind-shadow",
        "cel.bind(async, {'data': {'key': 'a'}}, async.data.key == 'a')",
    ),
    (
        "nested-macros",
        "async.data.items.all(x, async.data.items.exists(y, x == y))",
    ),
    ("empty-macro", "[].all(x, x == 0) && ![].exists(x, x == 0)"),
    (
        "map-macro",
        "async.data.labels.all(k, k in ['tier', 'live'])",
    ),
    (
        "async-shadow",
        "[{'data': {'customer': 'shadow'}}].all(async, async.data.customer == 'shadow')",
    ),
    (
        "event-shadow",
        "[100].exists(event, event == async.data.amount)",
    ),
    (
        "timestamp",
        "timestamp(async.data.time) == timestamp('2026-01-02T03:04:05.006Z')",
    ),
    ("duration", "duration(async.data.delay) == duration('1.5s')"),
    (
        "time-add",
        "timestamp(async.data.time) + duration('1s') == timestamp('2026-01-02T03:04:06.006Z')",
    ),
    (
        "time-subtract",
        "timestamp(async.data.time) - timestamp('2026-01-02T03:04:04.006Z') == duration('1s')",
    ),
    (
        "duration-add",
        "duration(async.data.delay) + duration('0.5s') == duration('2s')",
    ),
    (
        "duration-subtract",
        "duration(async.data.delay) - duration('0.5s') == duration('1s')",
    ),
    (
        "time-year",
        "timestamp(async.data.time).getFullYear() == 2026",
    ),
    ("time-month", "timestamp(async.data.time).getMonth() == 0"),
    ("time-date", "timestamp(async.data.time).getDate() == 2"),
    (
        "time-day-month",
        "timestamp(async.data.time).getDayOfMonth() == 1",
    ),
    (
        "time-day-week",
        "timestamp(async.data.time).getDayOfWeek() == 5",
    ),
    (
        "time-day-year",
        "timestamp(async.data.time).getDayOfYear() == 1",
    ),
    (
        "time-hours-zone",
        "timestamp(async.data.time).getHours('UTC') == 3",
    ),
    (
        "time-minutes",
        "timestamp(async.data.time).getMinutes() == 4",
    ),
    (
        "time-seconds",
        "timestamp(async.data.time).getSeconds() == 5",
    ),
    (
        "time-millis",
        "timestamp(async.data.time).getMilliseconds() == 6",
    ),
    (
        "duration-access",
        "duration(async.data.delay).getMilliseconds() == 500",
    ),
    ("root-ident", ".async.data.amount == 100"),
    (
        "computed-index",
        "async.data.items[async.data.scale - 1] == 2",
    ),
    (
        "nested-literals",
        "{'items': [async.data.amount, 2]}.items[0] == 100",
    ),
];

pub fn incoming(entries: impl IntoIterator<Item = (String, Value)>) -> Value {
    let defaults = [
        ("amount".into(), 100i64.into()),
        ("scale".into(), 2i64.into()),
        ("name".into(), "order-paid".into()),
        ("items".into(), Value::list([1i64, 2, 3])),
        (
            "labels".into(),
            Value::map([("tier", Value::from("gold")), ("live", Value::from(true))]),
        ),
        ("time".into(), "2026-01-02T03:04:05.006Z".into()),
        ("delay".into(), "1.5s".into()),
    ];
    Value::map([("data", Value::map(defaults.into_iter().chain(entries)))])
}
