//! Conservative, in-memory prototype. Not a production indexing API.
//!
//! Only required, top-level string equalities are indexed. Everything else
//! remains ordinary CEL. Each registration uses one anchor, so candidates are
//! unique without a million-entry visited bitmap or a scan of all registrations.

use std::collections::HashMap;
use std::sync::Arc;

use cel_core::{Ast, BinaryOp, Env, Expr, MapKey, Program, SpannedExpr, Value};

type Path = Vec<Arc<str>>;

enum Expected {
    Literal(Arc<str>),
    Captured(Path),
}

struct Guard {
    path: Path,
    expected: Expected,
}

pub struct Template {
    pub program: Program,
    guards: Vec<Guard>,
}

impl Template {
    pub fn compile(env: &Env, source: &str) -> Self {
        let ast = env
            .compile(source)
            .unwrap_or_else(|error| panic!("benchmark expression must check: {source}: {error}"));
        Self {
            program: env.program(&ast).expect("benchmark program must build"),
            guards: required_guards(&ast),
        }
    }

    fn anchor(&self, captured: &Value) -> Option<(&Path, Arc<str>)> {
        self.guards.iter().find_map(|guard| {
            let key = match &guard.expected {
                Expected::Literal(key) => key.clone(),
                Expected::Captured(path) => match read_path(captured, path)? {
                    Value::String(key) => key.clone(),
                    // Do not hash numbers using Rust variants or f64 coercion.
                    // Numeric, null and aggregate keys stay unindexed for now.
                    _ => return None,
                },
            };
            Some((&guard.path, key))
        })
    }
}

fn required_guards(ast: &Ast) -> Vec<Guard> {
    let mut pending = vec![ast.expr()];
    let mut guards = Vec::new();
    while let Some(expr) = pending.pop() {
        match &expr.node {
            Expr::Binary {
                op: BinaryOp::And,
                left,
                right,
            } => {
                pending.push(right);
                pending.push(left);
            }
            Expr::Binary {
                op: BinaryOp::Eq,
                left,
                right,
            } => {
                if let Some(guard) =
                    equality_guard(left, right).or_else(|| equality_guard(right, left))
                {
                    guards.push(guard);
                }
            }
            // Never descend through OR, NOT, calls, conditionals or macro
            // scopes. An opaque subtree is may-match, not a failed predicate.
            _ => {}
        }
    }
    guards
}

fn equality_guard(left: &SpannedExpr, right: &SpannedExpr) -> Option<Guard> {
    let path = field_path(left, "async")?;
    let expected = match &right.node {
        Expr::String(value) => Expected::Literal(Arc::from(value.as_str())),
        _ => Expected::Captured(field_path(right, "event")?),
    };
    Some(Guard { path, expected })
}

fn field_path(mut expr: &SpannedExpr, root: &str) -> Option<Path> {
    let mut path = Vec::new();
    loop {
        match &expr.node {
            Expr::Member {
                expr: receiver,
                field,
                optional: false,
            } => {
                path.push(Arc::from(field.as_str()));
                expr = receiver;
            }
            Expr::Index {
                expr: receiver,
                index,
                optional: false,
            } => {
                let Expr::String(field) = &index.node else {
                    return None;
                };
                path.push(Arc::from(field.as_str()));
                expr = receiver;
            }
            Expr::Ident(name) | Expr::RootIdent(name) if name == root && !path.is_empty() => {
                path.reverse();
                return Some(path);
            }
            _ => return None,
        }
    }
}

fn read_path<'a>(mut value: &'a Value, path: &[Arc<str>]) -> Option<&'a Value> {
    for field in path {
        let Value::Map(map) = value else {
            return None;
        };
        value = map.get(&MapKey::String(field.clone()))?;
    }
    Some(value)
}

struct PathIndex {
    path: Path,
    postings: HashMap<Arc<str>, Vec<u32>>,
}

enum Location {
    Indexed {
        path: usize,
        key: Arc<str>,
        offset: usize,
    },
    Unindexed {
        offset: usize,
    },
}

#[derive(Default)]
pub struct GuardIndex {
    path_ids: HashMap<Path, usize>,
    paths: Vec<PathIndex>,
    unindexed: Vec<u32>,
    locations: Vec<Option<Location>>,
}

impl GuardIndex {
    pub fn insert(&mut self, id: u32, template: &Template, captured: &Value) {
        let location = if let Some((path, key)) = template.anchor(captured) {
            let path_id = *self.path_ids.entry(path.clone()).or_insert_with(|| {
                self.paths.push(PathIndex {
                    path: path.clone(),
                    postings: HashMap::new(),
                });
                self.paths.len() - 1
            });
            let postings = self.paths[path_id].postings.entry(key.clone()).or_default();
            let offset = postings.len();
            postings.push(id);
            Location::Indexed {
                path: path_id,
                key,
                offset,
            }
        } else {
            let offset = self.unindexed.len();
            self.unindexed.push(id);
            Location::Unindexed { offset }
        };
        if id as usize >= self.locations.len() {
            self.locations.resize_with(id as usize + 1, || None);
        }
        assert!(self.locations[id as usize].replace(location).is_none());
    }

    pub fn remove(&mut self, id: u32) {
        let location = self.locations[id as usize]
            .take()
            .expect("remove only live registrations");
        let (postings, offset) = match &location {
            Location::Indexed { path, key, offset } => {
                (self.paths[*path].postings.get_mut(key).unwrap(), *offset)
            }
            Location::Unindexed { offset } => (&mut self.unindexed, *offset),
        };
        assert_eq!(postings.swap_remove(offset), id);
        if let Some(&moved) = postings.get(offset) {
            match self.locations[moved as usize].as_mut().unwrap() {
                Location::Indexed { offset: slot, .. } | Location::Unindexed { offset: slot } => {
                    *slot = offset
                }
            }
        }
        if postings.is_empty() {
            if let Location::Indexed { path, key, .. } = location {
                self.paths[path].postings.remove(&key);
            }
        }
    }

    pub fn candidates(&self, incoming: &Value) -> Vec<u32> {
        let mut candidates = self.unindexed.clone();
        // Each path is extracted once per incoming event, not per expression.
        for path in &self.paths {
            if let Some(Value::String(key)) = read_path(incoming, &path.path) {
                if let Some(postings) = path.postings.get(key) {
                    candidates.extend_from_slice(postings);
                }
            }
        }
        candidates
    }

    pub fn unindexed_count(&self) -> usize {
        self.unindexed.len()
    }

    pub fn path_count(&self) -> usize {
        self.paths.len()
    }
}
