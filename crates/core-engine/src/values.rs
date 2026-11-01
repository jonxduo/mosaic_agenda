//! Field values against their definitions (§5.6) and computed fields.

use core_model::Scalar;
use core_model::cond::Expr;
use core_model::config::{FieldDef, FieldType};

use crate::auth::{Denied, deny};
use crate::ops::Values;
use crate::time::parse_date;

fn option_ids(f: &FieldDef) -> Vec<&str> {
    f.options.iter().flatten().map(|o| o.id.as_str()).collect()
}

fn check_one(f: &FieldDef, v: &Scalar) -> Result<(), String> {
    if v.is_null() {
        return Ok(());
    }
    let numeric = |n: f64| -> Result<(), String> {
        if f.min.is_some_and(|m| n < m) || f.max.is_some_and(|m| n > m) {
            return Err(format!("`{}` is out of range", f.id));
        }
        Ok(())
    };
    match (f.ty, v) {
        (FieldType::Integer | FieldType::Duration, Scalar::Int(i)) => numeric(*i as f64),
        (FieldType::Decimal, Scalar::Int(_) | Scalar::Num(_)) => numeric(v.as_f64().unwrap_or(0.0)),
        (FieldType::Boolean, Scalar::Bool(_)) => Ok(()),
        (FieldType::Text | FieldType::LongText, Scalar::Str(s)) => match f.max_length {
            Some(max) if s.chars().count() > max as usize => Err(format!("`{}` is too long", f.id)),
            _ => Ok(()),
        },
        (FieldType::Date, Scalar::Str(s)) => parse_date(s)
            .map(|_| ())
            .ok_or_else(|| format!("`{}` is not a date", f.id)),
        (FieldType::Period | FieldType::Person | FieldType::Resource, Scalar::Str(_)) => Ok(()),
        (FieldType::Choice, Scalar::Str(s)) => {
            if option_ids(f).contains(&s.as_str()) {
                Ok(())
            } else {
                Err(format!("`{}`: unknown option `{s}`", f.id))
            }
        }
        (FieldType::MultiChoice, Scalar::List(items)) => {
            let ids = option_ids(f);
            if items
                .iter()
                .all(|x| matches!(x, Scalar::Str(s) if ids.contains(&s.as_str())))
            {
                Ok(())
            } else {
                Err(format!("`{}`: unknown option", f.id))
            }
        }
        (FieldType::Photos, Scalar::List(items)) => {
            if f.max_count.is_some_and(|m| items.len() > m as usize) {
                Err(format!("`{}`: too many photos", f.id))
            } else {
                Ok(())
            }
        }
        (FieldType::Computed, _) => Err(format!("`{}` is computed and cannot be set", f.id)),
        _ => Err(format!("`{}` has the wrong type", f.id)),
    }
}

/// Checks values against fields; `complete` also requires `required` fields.
pub fn check_values(fields: &[FieldDef], values: &Values, complete: bool) -> Result<(), Denied> {
    for (k, v) in values {
        let Some(f) = fields.iter().find(|f| &f.id == k) else {
            return deny(format!("unknown field `{k}`"));
        };
        check_one(f, v).map_err(Denied)?;
    }
    if complete {
        for f in fields
            .iter()
            .filter(|f| f.required && f.ty != FieldType::Computed)
        {
            if values.get(&f.id).is_none_or(Scalar::is_null) {
                return deny(format!("`{}` is required", f.id));
            }
        }
    }
    Ok(())
}

fn eval(e: &Expr, values: &Values, depth: usize) -> f64 {
    if depth > 32 {
        return 0.0;
    }
    let get = |x: &Expr| eval(x, values, depth + 1);
    match e {
        Expr::Field(f) => values.get(f).and_then(Scalar::as_f64).unwrap_or(0.0),
        Expr::Const(c) => *c,
        Expr::Sum(v) => v.iter().map(get).sum(),
        Expr::Mul(v) => v.iter().map(get).product(),
        Expr::Sub(v) => match v.as_slice() {
            [a, b] => get(a) - get(b),
            _ => 0.0,
        },
        Expr::Div(v) => match v.as_slice() {
            [a, b] => {
                let d = get(b);
                if d == 0.0 { 0.0 } else { get(a) / d }
            }
            _ => 0.0,
        },
        Expr::If(i) => {
            let holds = i.cond.clone();
            let ok = cond_on_values(&holds, values);
            if ok { get(&i.then) } else { get(&i.otherwise) }
        }
    }
}

/// Field-only conditions inside computed expressions.
fn cond_on_values(c: &core_model::Cond, values: &Values) -> bool {
    use core_model::Cond;
    match c {
        Cond::All(v) => v.iter().all(|x| cond_on_values(x, values)),
        Cond::Any(v) => v.iter().any(|x| cond_on_values(x, values)),
        Cond::Not(x) => !cond_on_values(x, values),
        Cond::Always(b) => *b,
        Cond::Field(cmp) => cmp.op.apply(values.get(&cmp.name), &cmp.value),
        _ => false,
    }
}

/// Values with computed fields filled in.
pub fn with_computed(fields: &[FieldDef], values: &Values) -> Values {
    let mut out = values.clone();
    for f in fields.iter().filter(|f| f.ty == FieldType::Computed) {
        if let Some(e) = &f.expr {
            out.insert(f.id.clone(), Scalar::Num(eval(e, values, 0)));
        }
    }
    out
}
