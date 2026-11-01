//! Declarative condition language (`Cond`) and limited arithmetic (`Expr`).
//!
//! Both are plain data, written in YAML as single-key maps
//! (`{ all: [...] }`, `{ has_qualification: x }`). There is no scripting.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::scalar::Scalar;
use crate::strings::{MemberOf, Slug, UnitWhere};

/// Condition evaluated against a context (evaluated person, subject, date,
/// subject's unit, field values). See PROGETTO.md §5.4.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Cond {
    /// All sub-conditions hold (empty = true).
    All(Vec<Cond>),
    /// At least one sub-condition holds (empty = false).
    Any(Vec<Cond>),
    Not(Box<Cond>),
    /// The person holds the qualification (directly or by implication) at the reference date.
    HasQualification(Slug),
    /// The person holds a qualification belonging to the named set.
    InSet(Slug),
    HoldsRole(HoldsRole),
    MemberOf(MemberOf),
    /// Compares a person attribute.
    Attribute(Comparison),
    /// Compares a document field (only in form contexts).
    Field(Comparison),
    /// The evaluated person is (or is not) the subject.
    IsSubject(bool),
    /// Constant.
    Always(bool),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HoldsRole {
    pub role: Slug,
    #[serde(rename = "where", default)]
    pub location: UnitWhere,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    pub name: Slug,
    pub op: CompareOp,
    #[serde(default)]
    pub value: Scalar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompareOp {
    Eq,
    Ne,
    /// Value is a list containing the actual value.
    In,
    Gt,
    Ge,
    Lt,
    Le,
    /// The value is present (not null).
    Set,
    /// The value is absent or null.
    Unset,
}

impl CompareOp {
    /// Applies the operator to an actual value (`None` = absent).
    pub fn apply(self, actual: Option<&Scalar>, expected: &Scalar) -> bool {
        use std::cmp::Ordering::*;
        let actual = actual.filter(|v| !v.is_null());
        match self {
            Self::Set => actual.is_some(),
            Self::Unset => actual.is_none(),
            Self::Eq => actual.is_some_and(|a| a.loose_eq(expected)),
            Self::Ne => !actual.is_some_and(|a| a.loose_eq(expected)),
            Self::In => match (actual, expected) {
                (Some(a), Scalar::List(list)) => list.iter().any(|e| a.loose_eq(e)),
                _ => false,
            },
            Self::Gt => actual.and_then(|a| a.compare(expected)) == Some(Greater),
            Self::Ge => matches!(
                actual.and_then(|a| a.compare(expected)),
                Some(Greater | Equal)
            ),
            Self::Lt => actual.and_then(|a| a.compare(expected)) == Some(Less),
            Self::Le => matches!(actual.and_then(|a| a.compare(expected)), Some(Less | Equal)),
        }
    }
}

impl Cond {
    /// Visits this condition and all nested ones, depth first.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Cond)) {
        f(self);
        match self {
            Self::All(v) | Self::Any(v) => v.iter().for_each(|c| c.walk(f)),
            Self::Not(c) => c.walk(f),
            _ => {}
        }
    }
}

/// Limited arithmetic for `computed` fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Expr {
    /// Value of another field of the same variant (missing = 0).
    Field(Slug),
    Const(f64),
    Sum(Vec<Expr>),
    /// Exactly two operands: a - b.
    Sub(Vec<Expr>),
    Mul(Vec<Expr>),
    /// Exactly two operands: a / b (division by zero = 0).
    Div(Vec<Expr>),
    If(Box<IfExpr>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IfExpr {
    pub cond: Cond,
    pub then: Expr,
    #[serde(rename = "else")]
    pub otherwise: Expr,
}

impl Expr {
    /// Field names referenced by this expression (including inside `if` conditions).
    pub fn referenced_fields(&self, out: &mut Vec<Slug>) {
        match self {
            Self::Field(f) => out.push(f.clone()),
            Self::Const(_) => {}
            Self::Sum(v) | Self::Sub(v) | Self::Mul(v) | Self::Div(v) => {
                v.iter().for_each(|e| e.referenced_fields(out));
            }
            Self::If(i) => {
                i.cond.walk(&mut |c| {
                    if let Cond::Field(cmp) = c {
                        out.push(cmp.name.clone());
                    }
                });
                i.then.referenced_fields(out);
                i.otherwise.referenced_fields(out);
            }
        }
    }

    /// Operators with a fixed arity that is not respected.
    pub fn arity_errors(&self, out: &mut Vec<&'static str>) {
        match self {
            Self::Sub(v) | Self::Div(v) => {
                if v.len() != 2 {
                    out.push(if matches!(self, Self::Sub(_)) {
                        "sub"
                    } else {
                        "div"
                    });
                }
                v.iter().for_each(|e| e.arity_errors(out));
            }
            Self::Sum(v) | Self::Mul(v) => v.iter().for_each(|e| e.arity_errors(out)),
            Self::If(i) => {
                i.then.arity_errors(out);
                i.otherwise.arity_errors(out);
            }
            Self::Field(_) | Self::Const(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_like_shapes_parse() {
        let json = r#"{ "all": [
            { "has_qualification": "member" },
            { "attribute": { "name": "level", "op": "ge", "value": 2 } },
            { "holds_role": { "role": "lead", "where": "subject_unit" } },
            { "not": { "member_of": "unit_type:team" } },
            { "always": true }
        ]}"#;
        let c: Result<Cond, _> = serde_json::from_str(json);
        assert!(c.is_ok(), "{c:?}");
    }

    #[test]
    fn unknown_keys_rejected() {
        let c: Result<Cond, _> = serde_json::from_str(r#"{ "has_qualifcation": "x" }"#);
        assert!(c.is_err());
    }

    #[test]
    fn compare_ops() {
        let two = Scalar::Int(2);
        assert!(CompareOp::Eq.apply(Some(&two), &Scalar::Num(2.0)));
        assert!(CompareOp::Ne.apply(None, &two));
        assert!(CompareOp::Gt.apply(Some(&two), &Scalar::Int(1)));
        assert!(!CompareOp::Gt.apply(None, &Scalar::Int(1)));
        assert!(CompareOp::Set.apply(Some(&two), &Scalar::Null));
        assert!(CompareOp::Unset.apply(Some(&Scalar::Null), &Scalar::Null));
        assert!(CompareOp::In.apply(
            Some(&Scalar::Str("b".into())),
            &Scalar::List(vec!["a".into(), "b".into()])
        ));
    }

    #[test]
    fn binary_round_trip() {
        let c = Cond::Any(vec![
            Cond::HasQualification(Slug::new("a").unwrap_or_else(|_| unreachable!())),
            Cond::Attribute(Comparison {
                name: Slug::new("x").unwrap_or_else(|_| unreachable!()),
                op: CompareOp::In,
                value: Scalar::List(vec![Scalar::Int(1)]),
            }),
        ]);
        let bytes = postcard::to_allocvec(&c).unwrap_or_default();
        assert_eq!(postcard::from_bytes::<Cond>(&bytes).ok(), Some(c));
    }

    #[test]
    fn expr_references_and_arity() {
        let e: Expr = serde_json::from_str(
            r#"{ "sum": [ { "field": "a" }, { "sub": [ { "field": "b" } ] } ] }"#,
        )
        .unwrap_or(Expr::Const(0.0));
        let mut refs = Vec::new();
        e.referenced_fields(&mut refs);
        assert_eq!(refs.len(), 2);
        let mut errs = Vec::new();
        e.arity_errors(&mut errs);
        assert_eq!(errs, vec!["sub"]);
    }
}
