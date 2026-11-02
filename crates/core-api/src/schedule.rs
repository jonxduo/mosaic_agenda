//! Dates and rotations of programs: the events a new program covers, from
//! the weekly recurrence of its calendar event types, and the groups of
//! events that share the same persons (`rotation`).

use core_engine::time::{PeriodKey, dates_on, weekly_days};
use core_model::config::{ProgramPeriod, ProgramTypeDef, Rotation};
use serde::{Deserialize, Serialize};
use specta::Type;
use time::{Date, Weekday};

use crate::views::{cfg, today};
use crate::{ApiError, ApiResult, Core};

/// The dates proposed for a new program of a type.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProgramDates {
    /// Period key (`YYYY-MM` or `week-YYYY-MM-DD`).
    pub period: String,
    pub label: String,
    pub previous: String,
    pub next: String,
    /// `YYYY-MM-DD`, in order.
    pub dates: Vec<String>,
    /// The same dates, for display.
    pub labels: Vec<String>,
    /// A program of this type already covers this period.
    pub exists: bool,
}

/// The period of a program type containing `date`.
pub(crate) fn period_of(def: &ProgramTypeDef, date: Date) -> PeriodKey {
    match def.period {
        ProgramPeriod::Week => PeriodKey::week_of(date),
        ProgramPeriod::Month => PeriodKey::month_of(date),
    }
}

pub(crate) fn rotation_key(rotation: Rotation, d: Date) -> Option<PeriodKey> {
    match rotation {
        Rotation::Occurrence => None,
        Rotation::Week => Some(PeriodKey::week_of(d)),
        Rotation::Month => Some(PeriodKey::month_of(d)),
    }
}

/// Occurrences sharing the persons of occurrence `occ` (itself included).
pub(crate) fn group_of(def: &ProgramTypeDef, occurrences: &[Date], occ: usize) -> Vec<usize> {
    let Some(d) = occurrences.get(occ) else {
        return Vec::new();
    };
    match rotation_key(def.rotation, *d) {
        None => vec![occ],
        Some(k) => occurrences
            .iter()
            .enumerate()
            .filter(|(_, x)| rotation_key(def.rotation, **x) == Some(k))
            .map(|(i, _)| i)
            .collect(),
    }
}

/// The groups of a program, each as the list of its occurrences.
pub(crate) fn groups(def: &ProgramTypeDef, occurrences: &[Date]) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = Vec::new();
    for i in 0..occurrences.len() {
        if !out.iter().any(|g| g.contains(&i)) {
            out.push(group_of(def, occurrences, i));
        }
    }
    out
}

/// Weekdays of the events of a program type.
pub(crate) fn weekdays(core_cfg: &core_engine::config::Cfg, def: &ProgramTypeDef) -> Vec<Weekday> {
    let mut days: Vec<Weekday> = def
        .event_type
        .iter()
        .filter_map(|e| {
            core_cfg
                .package()
                .calendar
                .event_types
                .iter()
                .find(|x| &x.id == e)
        })
        .filter_map(|e| e.default_recurrence.as_deref())
        .flat_map(weekly_days)
        .collect();
    days.sort_by_key(|d| d.number_from_monday());
    days.dedup();
    days
}

impl Core {
    /// The events a new program of `program_type` would cover. Without a
    /// period: the one after the latest program of the type, or the current one.
    pub fn program_dates(
        &self,
        program_type: String,
        period: Option<String>,
    ) -> ApiResult<ProgramDates> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let c = cfg(&v)?;
        let t = self.texts();
        let ty = core_model::Slug::new(&program_type).map_err(ApiError::invalid)?;
        let def = c
            .program_type(&ty)
            .ok_or_else(|| ApiError::not_found("program type"))?;
        let periods: Vec<PeriodKey> = v
            .proj
            .programs
            .values()
            .filter(|p| p.program_type == ty)
            .filter_map(|p| p.occurrences.first())
            .chain(
                v.proj
                    .assigned_snapshots
                    .values()
                    .chain(v.proj.published.values())
                    .filter(|s| s.program_type == ty)
                    .filter_map(|s| s.occurrences.first()),
            )
            .map(|d| period_of(def, *d))
            .collect();
        let key = match period {
            Some(p) => p.parse::<PeriodKey>().map_err(ApiError::invalid)?,
            None => {
                let current = period_of(def, today(self));
                match periods.iter().max() {
                    Some(last) if *last >= current => last.next().unwrap_or(current),
                    _ => current,
                }
            }
        };
        let (Some(start), Some(end)) = (key.start(), key.last_day()) else {
            return Err(ApiError::invalid("period"));
        };
        let dates = dates_on(&weekdays(c, def), start, end);
        let previous = period_of(def, core_engine::time::add_days(start, -1));
        Ok(ProgramDates {
            period: key.to_string(),
            label: t.period(&key),
            previous: previous.to_string(),
            next: key.next().unwrap_or(key).to_string(),
            labels: dates
                .iter()
                .map(|d| crate::views::capitalize(&t.long_date(*d)))
                .collect(),
            dates: dates.iter().map(Date::to_string).collect(),
            exists: periods.contains(&key),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn def(rotation: Rotation) -> ProgramTypeDef {
        let json = serde_json::json!({
            "id": "x", "label": "l", "event_type": ["a", "b"], "period": "month",
            "rotation": rotation, "owner_role": "r", "publish_audience": "p",
            "slots": [ { "id": "s", "label": "l" } ]
        });
        match serde_json::from_value(json) {
            Ok(d) => d,
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn rotation_groups() {
        let occ = [
            date!(2026 - 11 - 01),
            date!(2026 - 11 - 05),
            date!(2026 - 11 - 08),
            date!(2026 - 11 - 12),
        ];
        assert_eq!(groups(&def(Rotation::Occurrence), &occ).len(), 4);
        // Sunday 1 is in the week of Monday 26 October.
        assert_eq!(
            groups(&def(Rotation::Week), &occ),
            vec![vec![0], vec![1, 2], vec![3]]
        );
        assert_eq!(groups(&def(Rotation::Month), &occ), vec![vec![0, 1, 2, 3]]);
    }
}
