//! Per-evaluation resource accounting. Ordinary CEL errors never clear this meter.

use std::cell::{Cell, RefCell};
use std::fmt;

use super::{MapKey, OptionalValue, Value};
use crate::Span;

/// Opt-in limits for native evaluation. Costs are deterministic logical units,
/// not elapsed time or measurements of the process allocator.
#[derive(Debug, Clone, Copy)]
pub struct EvalLimits {
    /// Work units available to nodes, validation, iteration and native helpers.
    pub fuel: u64,
    /// Cumulative allocation allowance, never refunded when temporaries drop.
    /// Includes conservative container/copy allowances, not just output size.
    pub allocation_bytes: u64,
}

/// Why bounded evaluation stopped, independently of ordinary CEL errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalLimitKind {
    Fuel,
    Allocation,
    /// Native values may have at most 64 levels, including scalar leaves.
    ValueDepth,
    /// A callback, protobuf operation or unsupported native value needs metering.
    UnmeteredOperation,
}

/// A fatal evaluation refusal. CEL logical/error-suppressing operators cannot
/// catch it. Spans use UTF-8 byte offsets; input validation uses the root span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalLimitError {
    pub kind: EvalLimitKind,
    pub span: Span,
}

impl fmt::Display for EvalLimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "evaluation stopped: {:?} at {:?}", self.kind, self.span)
    }
}

impl std::error::Error for EvalLimitError {}

pub(super) struct Meter {
    fuel: Cell<u64>,
    allocation: Cell<u64>,
    pub span: RefCell<Span>,
    failure: RefCell<Option<EvalLimitError>>,
    // A single sentinel avoids allocating another ordinary error after exhaustion.
    stopped: Value,
}

impl Meter {
    pub fn new(limits: EvalLimits, span: Span) -> Self {
        Self {
            fuel: Cell::new(limits.fuel),
            allocation: Cell::new(limits.allocation_bytes),
            span: RefCell::new(span),
            failure: RefCell::new(None),
            stopped: Value::error("bounded evaluation stopped"),
        }
    }

    pub fn failure(&self) -> Option<EvalLimitError> {
        self.failure.borrow().clone()
    }

    pub fn stop(&self, kind: EvalLimitKind) -> Value {
        let mut failure = self.failure.borrow_mut();
        if failure.is_none() {
            *failure = Some(EvalLimitError {
                kind,
                span: self.span.borrow().clone(),
            });
        }
        self.stopped.clone()
    }

    pub fn charge(&self, fuel: u64, allocation: u64) -> Result<(), Value> {
        if self.failure.borrow().is_some() {
            return Err(self.stopped.clone());
        }
        let Some(fuel_left) = self.fuel.get().checked_sub(fuel) else {
            return Err(self.stop(EvalLimitKind::Fuel));
        };
        let Some(allocation_left) = self.allocation.get().checked_sub(allocation) else {
            return Err(self.stop(EvalLimitKind::Allocation));
        };
        self.fuel.set(fuel_left);
        self.allocation.set(allocation_left);
        Ok(())
    }

    /// Validate before an activation can clone values, and after every expression
    /// before a growing accumulator can be retained. This deliberately conservative
    /// traversal is charged, including repeated/shared subtrees by occurrence.
    pub fn validate(&self, value: &Value, depth: usize) -> Result<(), Value> {
        self.charge(1, 0)?;
        if depth > 64 {
            return Err(self.stop(EvalLimitKind::ValueDepth));
        }
        match value {
            Value::List(values) => {
                for value in values.iter() {
                    self.validate(value, depth + 1)?;
                }
            }
            Value::Map(values) => {
                for value in values.values() {
                    self.validate(value, depth + 1)?;
                }
            }
            Value::Optional(OptionalValue::Some(value)) => self.validate(value, depth + 1)?,
            Value::Message(_) => return Err(self.stop(EvalLimitKind::UnmeteredOperation)),
            _ => {}
        }
        Ok(())
    }

    /// Bound copying/formatting/lookup involving a native value. This walks
    /// containers without building a temporary traversal stack or cloning values.
    pub fn value_cost(&self, value: &Value) -> Result<u64, Value> {
        self.charge(1, 0)?;
        let mut cost = std::mem::size_of::<Value>() as u64;
        match value {
            Value::String(s) => cost = cost.saturating_add(s.len() as u64),
            Value::Bytes(b) => cost = cost.saturating_add(b.len() as u64),
            Value::List(values) => {
                for value in values.iter() {
                    cost = cost.saturating_add(self.value_cost(value)?);
                }
            }
            Value::Map(values) => {
                for (key, value) in values.iter() {
                    cost = cost.saturating_add(self.value_cost(value)?);
                    if let MapKey::String(s) = key {
                        cost = cost.saturating_add(s.len() as u64);
                    }
                }
            }
            Value::Optional(OptionalValue::Some(value)) => {
                cost = cost.saturating_add(self.value_cost(value)?);
            }
            Value::Enum(value) => cost = cost.saturating_add(value.type_name.len() as u64),
            Value::Type(value) => cost = cost.saturating_add(value.name.len() as u64),
            Value::Error(value) => cost = cost.saturating_add(value.message.len() as u64),
            Value::Message(_) => return Err(self.stop(EvalLimitKind::UnmeteredOperation)),
            _ => {}
        }
        Ok(cost)
    }
}
