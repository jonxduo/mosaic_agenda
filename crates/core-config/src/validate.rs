//! Semantic validation of a package (PROGETTO.md §6.4).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::str::FromStr;

use core_model::audience::{AudienceDef, Selector, SystemAudience};
use core_model::cond::{Cond, Expr};
use core_model::config::{
    AggregateDef, ChoiceOption, FieldDef, FieldType, FormDef, MeasureOp, PackageConfig,
    ProgramPeriod, QualificationDef, QualificationDuration, Repeat, ResourceTypeDef, RoleDef,
    Rotation, TriggerFilter, ViewParams, WorkflowDef, default_label,
};
use core_model::strings::{GroupBy, MemberOf, ObjectKind, ObjectRef, ScopeDef};
use core_model::{Scalar, Slug, TextKey};

use crate::compat::version_matches;
use crate::issue::{Issue, Severity};

const MANIFEST: &str = "package.yaml";
const QUALS: &str = "qualifications.yaml";
const UNITS: &str = "units.yaml";
const ROLES: &str = "roles.yaml";
const AUDIENCES: &str = "audiences.yaml";
const FORMS: &str = "forms.yaml";
const WORKFLOWS: &str = "workflows.yaml";
const AGGREGATES: &str = "aggregates.yaml";
const CALENDAR: &str = "calendar.yaml";
const PROGRAMS: &str = "programs.yaml";
const RESOURCES: &str = "resources.yaml";
const BOARD: &str = "board.yaml";
const NOTIFICATIONS: &str = "notifications.yaml";
const UI: &str = "ui.yaml";

/// Validates a loaded package. `dir` (when given) is used to check that
/// referenced files such as the icon exist.
pub fn validate(pkg: &PackageConfig, dir: Option<&Path>) -> Vec<Issue> {
    let mut v = Validator::new(pkg);
    v.manifest(dir);
    v.qualifications();
    v.units();
    v.roles();
    v.audiences();
    v.workflows();
    v.forms();
    v.aggregates();
    v.calendar();
    v.programs();
    v.resources();
    v.board();
    v.notifications();
    v.ui();
    v.texts();
    let mut issues = v.issues;
    issues.sort();
    issues.dedup();
    issues
}

struct Validator<'a> {
    pkg: &'a PackageConfig,
    issues: Vec<Issue>,
    /// Text key → first place it is used.
    texts_used: BTreeMap<String, (&'static str, String)>,
    quals: BTreeMap<&'a Slug, &'a QualificationDef>,
    sets: BTreeSet<&'a Slug>,
    unit_types: BTreeSet<&'a Slug>,
    roles: BTreeMap<&'a Slug, &'a RoleDef>,
    audiences: BTreeMap<&'a Slug, &'a AudienceDef>,
    forms: BTreeMap<&'a Slug, &'a FormDef>,
    workflows: BTreeMap<&'a Slug, &'a WorkflowDef>,
    aggregates: BTreeMap<&'a Slug, &'a AggregateDef>,
    event_types: BTreeSet<&'a Slug>,
    programs: BTreeSet<&'a Slug>,
    resources: BTreeMap<&'a Slug, &'a ResourceTypeDef>,
    person_attrs: BTreeSet<&'a Slug>,
}

fn list<'a, T>(items: &'a [T], id: impl Fn(&'a T) -> &'a Slug) -> BTreeMap<&'a Slug, &'a T> {
    items.iter().map(|x| (id(x), x)).collect()
}

impl<'a> Validator<'a> {
    fn new(pkg: &'a PackageConfig) -> Self {
        Self {
            pkg,
            issues: Vec::new(),
            texts_used: BTreeMap::new(),
            quals: list(&pkg.qualifications.qualifications, |q| &q.id),
            sets: pkg.qualifications.sets.iter().collect(),
            unit_types: pkg.units.unit_types.iter().map(|u| &u.id).collect(),
            roles: list(&pkg.roles.roles, |r| &r.id),
            audiences: list(&pkg.audiences.audiences, |a| &a.id),
            forms: list(&pkg.forms.forms, |f| &f.id),
            workflows: list(&pkg.workflows.workflows, |w| &w.id),
            aggregates: list(&pkg.aggregates.aggregates, |a| &a.id),
            event_types: pkg.calendar.event_types.iter().map(|e| &e.id).collect(),
            programs: pkg.programs.program_types.iter().map(|p| &p.id).collect(),
            resources: list(&pkg.resources.resource_types, |r| &r.id),
            person_attrs: pkg
                .manifest
                .person_attributes
                .iter()
                .map(|a| &a.id)
                .collect(),
        }
    }

    // ----- helpers -------------------------------------------------------

    fn err(&mut self, file: &str, path: impl Into<String>, msg: impl Into<String>) {
        self.issues.push(Issue::error(file, path, msg));
    }

    fn warn(&mut self, file: &str, path: impl Into<String>, msg: impl Into<String>) {
        self.issues.push(Issue::warning(file, path, msg));
    }

    fn text(&mut self, key: &str, file: &'static str, path: impl Into<String>) {
        self.texts_used
            .entry(key.to_owned())
            .or_insert_with(|| (file, path.into()));
    }

    fn text_key(&mut self, key: &TextKey, file: &'static str, path: impl Into<String>) {
        self.text(key.as_str(), file, path);
    }

    fn unique<'b>(
        &mut self,
        file: &str,
        what: &str,
        ids: impl IntoIterator<Item = (&'b Slug, String)>,
    ) {
        let mut seen = BTreeSet::new();
        for (id, path) in ids {
            if !seen.insert(id) {
                self.err(file, path, format!("duplicate {what} id `{id}`"));
            }
        }
    }

    fn audience_exists(&self, id: &Slug) -> bool {
        self.audiences.contains_key(id) || SystemAudience::from_str(id.as_str()).is_ok()
    }

    fn audience_ref(&mut self, id: &Slug, file: &str, path: impl Into<String>) {
        if !self.audience_exists(id) {
            self.err(file, path, format!("unknown audience `{id}`"));
        }
    }

    fn role_ref(&mut self, id: &Slug, file: &str, path: impl Into<String>) {
        if !self.roles.contains_key(id) {
            self.err(file, path, format!("unknown role `{id}`"));
        }
    }

    fn qual_ref(&mut self, id: &Slug, file: &str, path: impl Into<String>) {
        if !self.quals.contains_key(id) {
            self.err(file, path, format!("unknown qualification `{id}`"));
        }
    }

    fn unit_type_ref(&mut self, id: &Slug, file: &str, path: impl Into<String>) {
        if !self.unit_types.contains(id) {
            self.err(file, path, format!("unknown unit type `{id}`"));
        }
    }

    fn form_ref(&mut self, id: &Slug, file: &str, path: impl Into<String>) {
        if !self.forms.contains_key(id) {
            self.err(file, path, format!("unknown form `{id}`"));
        }
    }

    fn workflow_ref(&mut self, id: &Slug, file: &str, path: impl Into<String>) {
        if !self.workflows.contains_key(id) {
            self.err(file, path, format!("unknown workflow `{id}`"));
        }
    }

    fn scope_ref(&mut self, scope: &ScopeDef, file: &str, path: impl Into<String>) {
        if let ScopeDef::UnitType(t) = scope {
            self.unit_type_ref(t, file, path);
        }
    }

    /// Checks references inside a condition. `fields` = field ids usable by
    /// `field` conditions (None = not a form context).
    fn cond(&mut self, c: &Cond, file: &str, path: &str, fields: Option<&BTreeSet<Slug>>) {
        match c {
            Cond::All(v) | Cond::Any(v) => {
                let op = if matches!(c, Cond::All(_)) {
                    "all"
                } else {
                    "any"
                };
                for (i, x) in v.iter().enumerate() {
                    self.cond(x, file, &format!("{path}.{op}[{i}]"), fields);
                }
            }
            Cond::Not(x) => self.cond(x, file, &format!("{path}.not"), fields),
            Cond::HasQualification(q) => {
                self.qual_ref(q, file, format!("{path}.has_qualification"));
            }
            Cond::InSet(s) => {
                if !self.sets.contains(s) {
                    self.err(
                        file,
                        format!("{path}.in_set"),
                        format!("unknown qualification set `{s}`"),
                    );
                }
            }
            Cond::HoldsRole(h) => self.role_ref(&h.role, file, format!("{path}.holds_role.role")),
            Cond::MemberOf(MemberOf::UnitType(t)) => {
                self.unit_type_ref(t, file, format!("{path}.member_of"));
            }
            Cond::MemberOf(_) | Cond::IsSubject(_) | Cond::Always(_) => {}
            Cond::Attribute(cmp) => {
                if !self.person_attrs.contains(&cmp.name) {
                    self.err(
                        file,
                        format!("{path}.attribute.name"),
                        format!("unknown person attribute `{}`", cmp.name),
                    );
                }
            }
            Cond::Field(cmp) => match fields {
                None => self.err(
                    file,
                    format!("{path}.field"),
                    "field conditions are only allowed where document fields exist",
                ),
                Some(set) if !set.contains(&cmp.name) => self.err(
                    file,
                    format!("{path}.field.name"),
                    format!("unknown field `{}`", cmp.name),
                ),
                Some(_) => {}
            },
        }
    }

    /// Qualifications mutually exclusive with `q` (declared on either side).
    fn exclusions(&self, q: &Slug) -> BTreeSet<&'a Slug> {
        let mut out: BTreeSet<&Slug> = self
            .quals
            .get(q)
            .map(|d| d.excludes.iter().collect())
            .unwrap_or_default();
        for (id, d) in &self.quals {
            if d.excludes.contains(q) {
                out.insert(id);
            }
        }
        out
    }

    fn closure(&self, q: &Slug) -> BTreeSet<Slug> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![q.clone()];
        while let Some(cur) = stack.pop() {
            if seen.insert(cur.clone()) {
                if let Some(d) = self.quals.get(&cur) {
                    stack.extend(d.implies.iter().cloned());
                }
            }
        }
        seen
    }

    /// Static satisfiability: false when the condition can never hold
    /// (constant false, mutually exclusive qualifications, conflicting
    /// attribute values, including through qualification requirements).
    /// `any` branches are checked one by one.
    fn satisfiable(&self, c: &Cond, depth: usize) -> bool {
        if depth > 32 {
            return true;
        }
        let mut terms: Vec<&Cond> = Vec::new();
        flatten_all(c, &mut terms);
        // Pull in the requirements of every positively required qualification.
        let mut required: BTreeSet<Slug> = BTreeSet::new();
        let mut i = 0;
        while i < terms.len() {
            if let Cond::HasQualification(q) = terms[i] {
                for implied in self.closure(q) {
                    if required.insert(implied.clone()) {
                        if let Some(req) =
                            self.quals.get(&implied).and_then(|d| d.requires.as_ref())
                        {
                            flatten_all(req, &mut terms);
                        }
                    }
                }
            }
            i += 1;
        }
        for q in &required {
            let excl = self.exclusions(q);
            if required.iter().any(|other| excl.contains(other)) {
                return false;
            }
        }
        let mut attr_eq: BTreeMap<&Slug, &Scalar> = BTreeMap::new();
        for t in &terms {
            match t {
                Cond::Always(false) => return false,
                Cond::Any(v) if v.iter().all(|b| !self.satisfiable(b, depth + 1)) => return false,
                Cond::Attribute(cmp) if cmp.op == core_model::CompareOp::Eq => {
                    if let Some(prev) = attr_eq.insert(&cmp.name, &cmp.value) {
                        if !prev.loose_eq(&cmp.value) {
                            return false;
                        }
                    }
                }
                _ => {}
            }
        }
        true
    }

    fn option_list(&mut self, opts: &[ChoiceOption], file: &'static str, path: &str) {
        self.unique(
            file,
            "option",
            opts.iter()
                .enumerate()
                .map(|(i, o)| (&o.id, format!("{path}[{i}]"))),
        );
        for (i, o) in opts.iter().enumerate() {
            match &o.label {
                Some(l) => self.text_key(l, file, format!("{path}[{i}].label")),
                None => self.text(
                    &default_label::option(o.id.as_str()),
                    file,
                    format!("{path}[{i}]"),
                ),
            }
        }
    }

    /// Checks a field list; returns its ids.
    fn fields(&mut self, list: &[FieldDef], file: &'static str, path: &str) -> BTreeSet<Slug> {
        self.unique(
            file,
            "field",
            list.iter()
                .enumerate()
                .map(|(i, f)| (&f.id, format!("{path}[{i}].id"))),
        );
        let ids: BTreeSet<Slug> = list.iter().map(|f| f.id.clone()).collect();
        let mut deps: BTreeMap<&Slug, Vec<Slug>> = BTreeMap::new();
        for (i, f) in list.iter().enumerate() {
            let p = format!("{path}[{i}]");
            match &f.label {
                Some(l) => self.text_key(l, file, format!("{p}.label")),
                None => self.text(&default_label::field(f.id.as_str()), file, p.clone()),
            }
            if let Some(u) = &f.unit {
                self.text_key(u, file, format!("{p}.unit"));
            }
            let numeric = matches!(
                f.ty,
                FieldType::Integer | FieldType::Decimal | FieldType::Duration
            );
            let texty = matches!(f.ty, FieldType::Text | FieldType::LongText);
            let choice = matches!(f.ty, FieldType::Choice | FieldType::MultiChoice);
            let refy = matches!(f.ty, FieldType::Person | FieldType::Resource);
            let photos = f.ty == FieldType::Photos;
            let computed = f.ty == FieldType::Computed;
            let misplaced = |name: &str, present: bool, allowed: bool| {
                if present && !allowed {
                    Some(format!(
                        "option `{name}` does not apply to fields of type {:?}",
                        f.ty
                    ))
                } else {
                    None
                }
            };
            let checks = [
                misplaced("min", f.min.is_some(), numeric),
                misplaced("max", f.max.is_some(), numeric),
                misplaced("step", f.step.is_some(), numeric),
                misplaced("unit", f.unit.is_some(), numeric || computed),
                misplaced("max_length", f.max_length.is_some(), texty),
                misplaced("options", f.options.is_some(), choice),
                misplaced("filter", f.filter.is_some(), refy),
                misplaced("max_count", f.max_count.is_some(), photos),
                misplaced("keep_exif", f.keep_exif, photos),
                misplaced("expr", f.expr.is_some(), computed),
            ];
            for msg in checks.into_iter().flatten() {
                self.err(file, p.clone(), msg);
            }
            if let (Some(min), Some(max)) = (f.min, f.max) {
                if min > max {
                    self.err(file, format!("{p}.min"), "min is greater than max");
                }
            }
            if choice {
                match &f.options {
                    Some(o) if !o.is_empty() => self.option_list(o, file, &format!("{p}.options")),
                    _ => self.err(file, p.clone(), "choice fields need at least one option"),
                }
            }
            if let Some(filter) = &f.filter {
                self.cond(filter, file, &format!("{p}.filter"), None);
            }
            if computed {
                match &f.expr {
                    None => self.err(file, p.clone(), "computed fields need `expr`"),
                    Some(e) => {
                        let mut arity = Vec::new();
                        e.arity_errors(&mut arity);
                        for op in arity {
                            self.err(
                                file,
                                format!("{p}.expr"),
                                format!("`{op}` needs exactly two operands"),
                            );
                        }
                        let mut refs = Vec::new();
                        e.referenced_fields(&mut refs);
                        for r in &refs {
                            if !ids.contains(r) {
                                self.err(file, format!("{p}.expr"), format!("unknown field `{r}`"));
                            }
                        }
                        self.expr_conds(e, file, &format!("{p}.expr"), &ids);
                        deps.insert(&f.id, refs);
                    }
                }
            }
        }
        // Circular references between computed fields.
        let graph: BTreeMap<&Slug, &Vec<Slug>> = deps.iter().map(|(k, v)| (*k, v)).collect();
        for start in deps.keys() {
            if reaches(&graph, start, start) {
                let i = list.iter().position(|f| &f.id == *start).unwrap_or(0);
                self.err(
                    file,
                    format!("{path}[{i}].expr"),
                    format!("computed field `{start}` refers to itself (circular reference)"),
                );
            }
        }
        ids
    }

    fn expr_conds(&mut self, e: &Expr, file: &str, path: &str, ids: &BTreeSet<Slug>) {
        match e {
            Expr::If(i) => {
                self.cond(&i.cond, file, &format!("{path}.if.cond"), Some(ids));
                self.expr_conds(&i.then, file, path, ids);
                self.expr_conds(&i.otherwise, file, path, ids);
            }
            Expr::Sum(v) | Expr::Sub(v) | Expr::Mul(v) | Expr::Div(v) => {
                v.iter().for_each(|x| self.expr_conds(x, file, path, ids));
            }
            Expr::Field(_) | Expr::Const(_) => {}
        }
    }

    // ----- files ---------------------------------------------------------

    fn manifest(&mut self, dir: Option<&Path>) {
        let m = &self.pkg.manifest;
        if !m.locales.contains(&m.default_locale) {
            self.err(
                MANIFEST,
                "default_locale",
                "default locale must be listed in `locales`",
            );
        }
        match version_matches(&m.core_compat, core_model::CORE_VERSION) {
            Ok(true) => {}
            Ok(false) => self.err(
                MANIFEST,
                "core_compat",
                format!(
                    "core version {} is outside `{}`",
                    core_model::CORE_VERSION,
                    m.core_compat
                ),
            ),
            Err(e) => self.err(MANIFEST, "core_compat", e.to_string()),
        }
        if version_matches("=0", &m.version).is_err() {
            self.err(MANIFEST, "version", "version must look like 1.2.3");
        }
        let b = &m.branding;
        self.text_key(&b.app_name, MANIFEST, "branding.app_name");
        let color_ok = b.primary_color.len() == 7
            && b.primary_color.starts_with('#')
            && b.primary_color[1..].chars().all(|c| c.is_ascii_hexdigit());
        if !color_ok {
            self.err(
                MANIFEST,
                "branding.primary_color",
                "expected a color like #3B5BDB",
            );
        }
        let segments: Vec<&str> = b.identifier.split('.').collect();
        let id_ok = segments.len() >= 2
            && segments.iter().all(|s| {
                s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                    && s.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            });
        if !id_ok {
            self.err(
                MANIFEST,
                "branding.identifier",
                "expected a reverse-DNS identifier like org.example.app",
            );
        }
        if let Some(dir) = dir {
            if !dir.join(&b.icon).is_file() {
                self.err(
                    MANIFEST,
                    "branding.icon",
                    format!("icon file `{}` not found", b.icon),
                );
            }
        }
        let attrs = &self.pkg.manifest.person_attributes;
        self.unique(
            MANIFEST,
            "person attribute",
            attrs
                .iter()
                .enumerate()
                .map(|(i, a)| (&a.id, format!("person_attributes[{i}].id"))),
        );
        for (i, a) in attrs.iter().enumerate() {
            let p = format!("person_attributes[{i}]");
            self.audience_ref(&a.read_audience, MANIFEST, format!("{p}.read_audience"));
            match &a.label {
                Some(l) => self.text_key(l, MANIFEST, format!("{p}.label")),
                None => self.text(&default_label::field(a.id.as_str()), MANIFEST, p.clone()),
            }
            let choice = matches!(a.ty, FieldType::Choice | FieldType::MultiChoice);
            match (&a.options, choice) {
                (Some(o), true) if !o.is_empty() => {
                    self.option_list(o, MANIFEST, &format!("{p}.options"))
                }
                (_, true) => self.err(MANIFEST, p, "choice attributes need at least one option"),
                (Some(_), false) => {
                    self.err(MANIFEST, p, "options only apply to choice attributes")
                }
                (None, false) => {
                    if matches!(a.ty, FieldType::Computed | FieldType::Photos) {
                        self.err(
                            MANIFEST,
                            p,
                            "person attributes cannot be computed or photos",
                        );
                    }
                }
            }
        }
    }

    fn qualifications(&mut self) {
        let defs = &self.pkg.qualifications.qualifications;
        self.unique(
            QUALS,
            "qualification",
            defs.iter()
                .enumerate()
                .map(|(i, q)| (&q.id, format!("qualifications[{i}].id"))),
        );
        self.unique(
            QUALS,
            "set",
            self.pkg
                .qualifications
                .sets
                .iter()
                .enumerate()
                .map(|(i, s)| (s, format!("sets[{i}]"))),
        );
        for (i, q) in defs.iter().enumerate() {
            let p = format!("qualifications[{i}]");
            self.text_key(&q.label, QUALS, format!("{p}.label"));
            for (j, x) in q.implies.iter().enumerate() {
                self.qual_ref(x, QUALS, format!("{p}.implies[{j}]"));
            }
            for (j, x) in q.excludes.iter().enumerate() {
                self.qual_ref(x, QUALS, format!("{p}.excludes[{j}]"));
                if x == &q.id {
                    self.err(
                        QUALS,
                        format!("{p}.excludes[{j}]"),
                        "a qualification cannot exclude itself",
                    );
                }
            }
            for (j, s) in q.sets.iter().enumerate() {
                if !self.sets.contains(s) {
                    self.err(
                        QUALS,
                        format!("{p}.sets[{j}]"),
                        format!("set `{s}` is not declared in `sets`"),
                    );
                }
            }
            if let Some(a) = &q.visibility {
                self.audience_ref(a, QUALS, format!("{p}.visibility"));
            }
            match (q.duration, q.fixed_period_unit) {
                (QualificationDuration::FixedPeriod, None) => {
                    self.err(
                        QUALS,
                        format!("{p}.fixed_period_unit"),
                        "fixed_period needs fixed_period_unit",
                    );
                }
                (QualificationDuration::Permanent | QualificationDuration::FreePeriod, Some(_)) => {
                    self.err(
                        QUALS,
                        format!("{p}.fixed_period_unit"),
                        "only fixed_period qualifications take a unit",
                    );
                }
                _ => {}
            }
            if let Some(r) = &q.requires {
                self.cond(r, QUALS, &format!("{p}.requires"), None);
            }
        }
        // Implication cycles.
        let graph: BTreeMap<&Slug, &Vec<Slug>> = defs.iter().map(|q| (&q.id, &q.implies)).collect();
        for (i, q) in defs.iter().enumerate() {
            if reaches(&graph, &q.id, &q.id) {
                self.err(
                    QUALS,
                    format!("qualifications[{i}].implies"),
                    format!("implication cycle through `{}`", q.id),
                );
            }
        }
        // Contradictions and never-obtainable qualifications.
        for (i, q) in defs.iter().enumerate() {
            let closure = self.closure(&q.id);
            let mut contradiction = None;
            for x in &closure {
                for y in self.exclusions(x) {
                    if closure.contains(y) {
                        contradiction = Some((x.clone(), y.clone()));
                    }
                }
            }
            if let Some((x, y)) = contradiction {
                self.err(
                    QUALS,
                    format!("qualifications[{i}]"),
                    format!(
                        "`{}` implies both `{x}` and `{y}`, which exclude each other",
                        q.id
                    ),
                );
            } else if !self.satisfiable(&Cond::HasQualification(q.id.clone()), 0) {
                self.err(
                    QUALS,
                    format!("qualifications[{i}].requires"),
                    format!("qualification `{}` can never be obtained", q.id),
                );
            }
        }
    }

    fn units(&mut self) {
        let defs = &self.pkg.units.unit_types;
        self.unique(
            UNITS,
            "unit type",
            defs.iter()
                .enumerate()
                .map(|(i, u)| (&u.id, format!("unit_types[{i}].id"))),
        );
        for (i, u) in defs.iter().enumerate() {
            self.text_key(&u.label, UNITS, format!("unit_types[{i}].label"));
            if let Some(c) = &u.member_requires {
                self.cond(c, UNITS, &format!("unit_types[{i}].member_requires"), None);
            }
        }
    }

    fn object_ref(&mut self, r: &ObjectRef, file: &str, path: String) {
        let exists = match r.kind {
            ObjectKind::Form => self.forms.contains_key(&r.id),
            ObjectKind::Program => self.programs.contains(&r.id),
            ObjectKind::Resource => self.resources.contains_key(&r.id),
            ObjectKind::Aggregate => self.aggregates.contains_key(&r.id),
            ObjectKind::Workflow => self.workflows.contains_key(&r.id),
        };
        if !exists {
            self.err(
                file,
                path,
                format!("unknown {} `{}`", r.kind.as_str(), r.id),
            );
        }
    }

    fn roles(&mut self) {
        let defs = &self.pkg.roles.roles;
        self.unique(
            ROLES,
            "role",
            defs.iter()
                .enumerate()
                .map(|(i, r)| (&r.id, format!("roles[{i}].id"))),
        );
        for (i, r) in defs.iter().enumerate() {
            let p = format!("roles[{i}]");
            self.text_key(&r.label, ROLES, format!("{p}.label"));
            self.scope_ref(&r.scope, ROLES, format!("{p}.scope"));
            if let Some(c) = &r.requires {
                self.cond(c, ROLES, &format!("{p}.requires"), None);
                if !self.satisfiable(c, 0) {
                    self.err(
                        ROLES,
                        format!("{p}.requires"),
                        format!(
                            "role `{}` requires qualifications that can never be obtained together",
                            r.id
                        ),
                    );
                }
            }
            if let Some(c) = &r.mandatory {
                self.cond(c, ROLES, &format!("{p}.mandatory"), None);
            }
            if let Some(max) = r.holders.max {
                if r.holders.min > max {
                    self.err(
                        ROLES,
                        format!("{p}.holders"),
                        "holders.min is greater than holders.max",
                    );
                }
                if max == 0 {
                    self.err(
                        ROLES,
                        format!("{p}.holders.max"),
                        "holders.max must be at least 1",
                    );
                }
            }
            for (j, g) in r.permissions.iter().enumerate() {
                let pp = format!("{p}.permissions[{j}]");
                if let Some(on) = &g.on {
                    let allowed = g.permission.allowed_objects();
                    if !allowed.contains(&on.kind) {
                        self.err(
                            ROLES,
                            format!("{pp}.on"),
                            format!("this permission cannot apply to a {}", on.kind.as_str()),
                        );
                    } else {
                        self.object_ref(on, ROLES, format!("{pp}.on"));
                    }
                }
            }
            for (field, target) in [
                ("substitute_for", &r.substitute_for),
                ("supervises", &r.supervises),
                ("assistant_of", &r.assistant_of),
            ] {
                if let Some(t) = target {
                    self.role_ref(t, ROLES, format!("{p}.{field}"));
                    if t == &r.id {
                        self.err(
                            ROLES,
                            format!("{p}.{field}"),
                            "a role cannot refer to itself",
                        );
                    }
                }
            }
            for (j, x) in r.exclusive_with.iter().enumerate() {
                self.role_ref(x, ROLES, format!("{p}.exclusive_with[{j}]"));
                if x == &r.id {
                    self.err(
                        ROLES,
                        format!("{p}.exclusive_with[{j}]"),
                        "a role cannot exclude itself",
                    );
                }
            }
        }
    }

    fn selector(&mut self, s: &Selector, path: &str) {
        match s {
            Selector::Union(v) | Selector::Intersect(v) | Selector::Except(v) => {
                let op = match s {
                    Selector::Union(_) => "union",
                    Selector::Intersect(_) => "intersect",
                    _ => "except",
                };
                for (i, x) in v.iter().enumerate() {
                    self.selector(x, &format!("{path}.{op}[{i}]"));
                }
            }
            Selector::WithQualification(q) => {
                self.qual_ref(q, AUDIENCES, format!("{path}.with_qualification"))
            }
            Selector::WithSet(set) => {
                if !self.sets.contains(set) {
                    self.err(
                        AUDIENCES,
                        format!("{path}.with_set"),
                        format!("unknown qualification set `{set}`"),
                    );
                }
            }
            Selector::RoleHolders(rh) => {
                self.role_ref(&rh.role, AUDIENCES, format!("{path}.role_holders.role"))
            }
            Selector::Audience(a) => self.audience_ref(a, AUDIENCES, format!("{path}.audience")),
            _ => {}
        }
    }

    fn audiences(&mut self) {
        let defs = &self.pkg.audiences.audiences;
        self.unique(
            AUDIENCES,
            "audience",
            defs.iter()
                .enumerate()
                .map(|(i, a)| (&a.id, format!("audiences[{i}].id"))),
        );
        for (i, a) in defs.iter().enumerate() {
            let p = format!("audiences[{i}]");
            if SystemAudience::from_str(a.id.as_str()).is_ok() {
                self.err(
                    AUDIENCES,
                    format!("{p}.id"),
                    format!("`{}` is a system audience and cannot be redefined", a.id),
                );
            }
            if let Some(l) = &a.label {
                self.text_key(l, AUDIENCES, format!("{p}.label"));
            }
            self.selector(&a.select, &format!("{p}.select"));
        }
        let graph: BTreeMap<&Slug, Vec<Slug>> = defs
            .iter()
            .map(|a| {
                let mut refs = Vec::new();
                a.select.referenced_audiences(&mut refs);
                (&a.id, refs)
            })
            .collect();
        let graph_ref: BTreeMap<&Slug, &Vec<Slug>> = graph.iter().map(|(k, v)| (*k, v)).collect();
        for (i, a) in defs.iter().enumerate() {
            if reaches(&graph_ref, &a.id, &a.id) {
                self.err(
                    AUDIENCES,
                    format!("audiences[{i}].select"),
                    format!("audience `{}` is defined recursively", a.id),
                );
            }
        }
    }

    fn workflows(&mut self) {
        let defs = &self.pkg.workflows.workflows;
        self.unique(
            WORKFLOWS,
            "workflow",
            defs.iter()
                .enumerate()
                .map(|(i, w)| (&w.id, format!("workflows[{i}].id"))),
        );
        for (i, w) in defs.iter().enumerate() {
            let p = format!("workflows[{i}]");
            if w.states.is_empty() {
                self.err(
                    WORKFLOWS,
                    format!("{p}.states"),
                    "a workflow needs at least one state",
                );
                continue;
            }
            self.unique(
                WORKFLOWS,
                "state",
                w.states
                    .iter()
                    .enumerate()
                    .map(|(j, s)| (s, format!("{p}.states[{j}]"))),
            );
            let states: BTreeSet<&Slug> = w.states.iter().collect();
            if !states.contains(&w.initial) {
                self.err(
                    WORKFLOWS,
                    format!("{p}.initial"),
                    format!("initial state `{}` is not in `states`", w.initial),
                );
            }
            self.unique(
                WORKFLOWS,
                "step",
                w.steps
                    .iter()
                    .enumerate()
                    .map(|(j, s)| (&s.id, format!("{p}.steps[{j}].id"))),
            );
            let field_ids = self.workflow_field_ids(&w.id);
            for (j, s) in w.steps.iter().enumerate() {
                let sp = format!("{p}.steps[{j}]");
                match &s.label {
                    Some(l) => self.text_key(l, WORKFLOWS, format!("{sp}.label")),
                    None if !s.kind.is_automatic() => {
                        self.text(&default_label::step(s.id.as_str()), WORKFLOWS, sp.clone());
                    }
                    None => {}
                }
                if s.from.is_empty() {
                    self.err(
                        WORKFLOWS,
                        format!("{sp}.from"),
                        "a step needs at least one source state",
                    );
                }
                for (k, f) in s.from.iter().enumerate() {
                    if !states.contains(f) {
                        self.err(
                            WORKFLOWS,
                            format!("{sp}.from[{k}]"),
                            format!("unknown state `{f}`"),
                        );
                    }
                }
                if !states.contains(&s.to) {
                    self.err(
                        WORKFLOWS,
                        format!("{sp}.to"),
                        format!("unknown state `{}`", s.to),
                    );
                }
                match &s.by {
                    Some(a) => self.audience_ref(a, WORKFLOWS, format!("{sp}.by")),
                    None if !s.kind.is_automatic() => {
                        self.err(
                            WORKFLOWS,
                            format!("{sp}.by"),
                            "this step needs `by` (who can perform it)",
                        );
                    }
                    None => {}
                }
                if let Some(c) = &s.when {
                    self.cond(c, WORKFLOWS, &format!("{sp}.when"), Some(&field_ids));
                }
                for (k, e) in s.effects.iter().enumerate() {
                    let ep = format!("{sp}.effects[{k}]");
                    match e {
                        core_model::config::Effect::SetReadAudience(a) => {
                            self.audience_ref(a, WORKFLOWS, format!("{ep}.set_read_audience"));
                        }
                        core_model::config::Effect::Notify(n) => {
                            self.audience_ref(
                                &n.audience,
                                WORKFLOWS,
                                format!("{ep}.notify.audience"),
                            );
                            self.text_key(&n.text, WORKFLOWS, format!("{ep}.notify.text"));
                        }
                        core_model::config::Effect::CreateAggregate(a) => {
                            if !self.aggregates.contains_key(a) {
                                self.err(
                                    WORKFLOWS,
                                    format!("{ep}.create_aggregate"),
                                    format!("unknown aggregate `{a}`"),
                                );
                            }
                        }
                    }
                }
            }
            for (j, t) in w.timers.iter().enumerate() {
                let core_model::config::TimerAction::Remind(n) = &t.action;
                self.audience_ref(
                    &n.audience,
                    WORKFLOWS,
                    format!("{p}.timers[{j}].do.remind.audience"),
                );
                self.text_key(
                    &n.text,
                    WORKFLOWS,
                    format!("{p}.timers[{j}].do.remind.text"),
                );
            }
            // Reachability and termination.
            let mut reachable: BTreeSet<&Slug> = BTreeSet::new();
            let mut stack = vec![&w.initial];
            while let Some(s) = stack.pop() {
                if reachable.insert(s) {
                    for step in &w.steps {
                        if step.from.contains(s) {
                            stack.push(&step.to);
                        }
                    }
                }
            }
            for (j, s) in w.states.iter().enumerate() {
                if !reachable.contains(s) {
                    self.err(
                        WORKFLOWS,
                        format!("{p}.states[{j}]"),
                        format!("state `{s}` is unreachable from `{}`", w.initial),
                    );
                }
            }
            let has_final = reachable
                .iter()
                .any(|s| !w.steps.iter().any(|step| step.from.contains(s)));
            if !has_final {
                self.err(
                    WORKFLOWS,
                    format!("{p}.steps"),
                    "no final state is reachable from the initial state",
                );
            }
        }
    }

    /// Fields of every form using the workflow (for `when` conditions).
    fn workflow_field_ids(&self, workflow: &Slug) -> BTreeSet<Slug> {
        self.pkg
            .forms
            .forms
            .iter()
            .filter(|f| &f.workflow == workflow)
            .flat_map(form_field_ids)
            .collect()
    }

    fn forms(&mut self) {
        let defs = &self.pkg.forms.forms;
        self.unique(
            FORMS,
            "form",
            defs.iter()
                .enumerate()
                .map(|(i, f)| (&f.id, format!("forms[{i}].id"))),
        );
        for (i, f) in defs.iter().enumerate() {
            let p = format!("forms[{i}]");
            self.text_key(&f.label, FORMS, format!("{p}.label"));
            self.cond(&f.subject, FORMS, &format!("{p}.subject"), None);
            self.workflow_ref(&f.workflow, FORMS, format!("{p}.workflow"));
            if let Some(b) = &f.batching {
                self.unit_type_ref(&b.unit_type, FORMS, format!("{p}.batching.unit_type"));
            }
            if let Some(a) = &f.fillers_on_behalf {
                self.audience_ref(a, FORMS, format!("{p}.fillers_on_behalf"));
            }
            let states: BTreeSet<&Slug> = self
                .workflows
                .get(&f.workflow)
                .map(|w| w.states.iter().collect())
                .unwrap_or_default();
            for (state, audience) in &f.read_audience_by_state {
                let sp = format!("{p}.read_audience_by_state.{state}");
                if self.workflows.contains_key(&f.workflow) && !states.contains(state) {
                    self.err(
                        FORMS,
                        sp.clone(),
                        format!("`{state}` is not a state of workflow `{}`", f.workflow),
                    );
                }
                self.audience_ref(audience, FORMS, sp);
            }
            if f.variants.is_empty() {
                self.err(
                    FORMS,
                    format!("{p}.variants"),
                    "a form needs at least one variant",
                );
            }
            self.unique(
                FORMS,
                "variant",
                f.variants
                    .iter()
                    .enumerate()
                    .map(|(j, v)| (&v.id, format!("{p}.variants[{j}].id"))),
            );
            for (j, v) in f.variants.iter().enumerate() {
                let vp = format!("{p}.variants[{j}]");
                self.cond(&v.when, FORMS, &format!("{vp}.when"), None);
                self.fields(&v.fields, FORMS, &format!("{vp}.fields"));
            }
            if let Some(last) = f.variants.last() {
                if last.when != Cond::Always(true) {
                    self.warn(
                        FORMS,
                        format!("{p}.variants[{}].when", f.variants.len() - 1),
                        "the last variant is not `always: true`: some subjects may match no variant",
                    );
                }
            }
        }
    }

    fn aggregates(&mut self) {
        let defs = &self.pkg.aggregates.aggregates;
        self.unique(
            AGGREGATES,
            "aggregate",
            defs.iter()
                .enumerate()
                .map(|(i, a)| (&a.id, format!("aggregates[{i}].id"))),
        );
        for (i, a) in defs.iter().enumerate() {
            let p = format!("aggregates[{i}]");
            self.text_key(&a.label, AGGREGATES, format!("{p}.label"));
            self.audience_ref(&a.read_audience, AGGREGATES, format!("{p}.read_audience"));
            let form = self.forms.get(&a.source.form).copied();
            let Some(form) = form else {
                self.err(
                    AGGREGATES,
                    format!("{p}.source.form"),
                    format!("unknown form `{}`", a.source.form),
                );
                continue;
            };
            if let Some(w) = self.workflows.get(&form.workflow) {
                if !w.states.contains(&a.source.state) {
                    self.err(
                        AGGREGATES,
                        format!("{p}.source.state"),
                        format!("`{}` is not a state of workflow `{}`", a.source.state, w.id),
                    );
                }
            }
            for (j, g) in a.group_by.iter().enumerate() {
                match g {
                    GroupBy::Variant => {}
                    GroupBy::QualificationSet(s) => {
                        if !self.sets.contains(s) {
                            self.err(
                                AGGREGATES,
                                format!("{p}.group_by[{j}]"),
                                format!("unknown qualification set `{s}`"),
                            );
                        }
                    }
                    GroupBy::UnitType(t) => {
                        self.unit_type_ref(t, AGGREGATES, format!("{p}.group_by[{j}]"))
                    }
                }
            }
            let field_types: BTreeMap<Slug, FieldType> = form
                .variants
                .iter()
                .flat_map(|v| v.fields.iter().map(|f| (f.id.clone(), f.ty)))
                .collect();
            let field_ids: BTreeSet<Slug> = field_types.keys().cloned().collect();
            self.unique(
                AGGREGATES,
                "measure",
                a.measures
                    .iter()
                    .enumerate()
                    .map(|(j, m)| (&m.id, format!("{p}.measures[{j}].id"))),
            );
            for (j, m) in a.measures.iter().enumerate() {
                let mp = format!("{p}.measures[{j}]");
                match &m.label {
                    Some(l) => self.text_key(l, AGGREGATES, format!("{mp}.label")),
                    None => self.text(
                        &default_label::measure(m.id.as_str()),
                        AGGREGATES,
                        mp.clone(),
                    ),
                }
                if let Some(c) = &m.filter {
                    self.cond(c, AGGREGATES, &format!("{mp}.filter"), Some(&field_ids));
                }
                match (&m.field, m.op) {
                    (None, MeasureOp::Count) => {}
                    (None, _) => self.err(AGGREGATES, mp.clone(), "this measure needs `field`"),
                    (Some(f), op) => match field_types.get(f) {
                        None => self.err(
                            AGGREGATES,
                            format!("{mp}.field"),
                            format!(
                                "field `{f}` is not present in any variant of form `{}`",
                                form.id
                            ),
                        ),
                        Some(ty) => {
                            let numeric = matches!(
                                ty,
                                FieldType::Integer
                                    | FieldType::Decimal
                                    | FieldType::Duration
                                    | FieldType::Computed
                            );
                            let ok = match op {
                                MeasureOp::Count => true,
                                MeasureOp::CountTrue => *ty == FieldType::Boolean,
                                _ => numeric,
                            };
                            if !ok {
                                self.err(
                                    AGGREGATES,
                                    format!("{mp}.field"),
                                    format!(
                                        "{op:?} cannot be applied to field `{f}` of type {ty:?}"
                                    ),
                                );
                            }
                        }
                    },
                }
            }
        }
    }

    fn calendar(&mut self) {
        let defs = &self.pkg.calendar.event_types;
        self.unique(
            CALENDAR,
            "event type",
            defs.iter()
                .enumerate()
                .map(|(i, e)| (&e.id, format!("event_types[{i}].id"))),
        );
        for (i, e) in defs.iter().enumerate() {
            let p = format!("event_types[{i}]");
            self.text_key(&e.label, CALENDAR, format!("{p}.label"));
            self.scope_ref(&e.scope, CALENDAR, format!("{p}.scope"));
            if let Some(r) = &e.default_recurrence {
                if !r.starts_with("FREQ=") {
                    self.err(
                        CALENDAR,
                        format!("{p}.default_recurrence"),
                        "expected an RRULE starting with FREQ=",
                    );
                }
                const DAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
                let bad_day = r
                    .split(';')
                    .find_map(|x| x.strip_prefix("BYDAY="))
                    .is_some_and(|by| by.split(',').any(|d| !DAYS.contains(&d)));
                if bad_day {
                    self.err(
                        CALENDAR,
                        format!("{p}.default_recurrence"),
                        "BYDAY expects MO, TU, WE, TH, FR, SA or SU",
                    );
                }
            }
            if let Some(t) = &e.default_time {
                let ok = t.len() == 5
                    && t.as_bytes()[2] == b':'
                    && t[..2].parse::<u8>().is_ok_and(|h| h < 24)
                    && t[3..].parse::<u8>().is_ok_and(|m| m < 60);
                if !ok {
                    self.err(
                        CALENDAR,
                        format!("{p}.default_time"),
                        "expected a time like 19:00",
                    );
                }
            }
        }
    }

    fn programs(&mut self) {
        let defs = &self.pkg.programs.program_types;
        self.unique(
            PROGRAMS,
            "program type",
            defs.iter()
                .enumerate()
                .map(|(i, x)| (&x.id, format!("program_types[{i}].id"))),
        );
        for (i, t) in defs.iter().enumerate() {
            let p = format!("program_types[{i}]");
            self.text_key(&t.label, PROGRAMS, format!("{p}.label"));
            if t.event_type.0.is_empty() {
                self.err(
                    PROGRAMS,
                    format!("{p}.event_type"),
                    "at least one event type is needed",
                );
            }
            for e in t.event_type.iter() {
                if !self.event_types.contains(e) {
                    self.err(
                        PROGRAMS,
                        format!("{p}.event_type"),
                        format!("unknown calendar event type `{e}`"),
                    );
                }
            }
            if t.rotation == Rotation::Month && t.period != ProgramPeriod::Month {
                self.err(
                    PROGRAMS,
                    format!("{p}.rotation"),
                    "a monthly rotation needs `period: month`",
                );
            }
            if !t.slots.is_empty() && t.slots.iter().all(|s| !s.enabled) {
                self.err(
                    PROGRAMS,
                    format!("{p}.slots"),
                    "at least one slot must be enabled",
                );
            }
            self.role_ref(&t.owner_role, PROGRAMS, format!("{p}.owner_role"));
            for (j, r) in t.assistant_roles.iter().enumerate() {
                self.role_ref(r, PROGRAMS, format!("{p}.assistant_roles[{j}]"));
            }
            self.audience_ref(
                &t.publish_audience,
                PROGRAMS,
                format!("{p}.publish_audience"),
            );
            if t.slots.is_empty() {
                self.err(
                    PROGRAMS,
                    format!("{p}.slots"),
                    "a program type needs at least one slot",
                );
            }
            self.unique(
                PROGRAMS,
                "slot",
                t.slots
                    .iter()
                    .enumerate()
                    .map(|(j, s)| (&s.id, format!("{p}.slots[{j}].id"))),
            );
            let slot_ids: BTreeSet<&Slug> = t.slots.iter().map(|s| &s.id).collect();
            for (j, s) in t.slots.iter().enumerate() {
                let sp = format!("{p}.slots[{j}]");
                self.text_key(&s.label, PROGRAMS, format!("{sp}.label"));
                if s.count == 0 {
                    self.err(PROGRAMS, format!("{sp}.count"), "count must be at least 1");
                }
                if let Some(c) = &s.requires {
                    self.cond(c, PROGRAMS, &format!("{sp}.requires"), None);
                    if !self.satisfiable(c, 0) {
                        self.err(
                            PROGRAMS,
                            format!("{sp}.requires"),
                            "nobody can ever satisfy these requirements",
                        );
                    }
                }
                let extra = self.fields(&s.extra_fields, PROGRAMS, &format!("{sp}.extra_fields"));
                match &s.repeat {
                    Some(Repeat::Count(0)) => self.err(
                        PROGRAMS,
                        format!("{sp}.repeat.count"),
                        "count must be at least 1",
                    ),
                    Some(Repeat::Field(f)) => {
                        let numeric = s
                            .extra_fields
                            .iter()
                            .any(|x| &x.id == f && x.ty == FieldType::Integer);
                        if !extra.contains(f) || !numeric {
                            self.err(
                                PROGRAMS,
                                format!("{sp}.repeat.field"),
                                format!("`{f}` must be an integer extra field of this slot"),
                            );
                        }
                    }
                    _ => {}
                }
                if let Some(other) = &s.pair_with {
                    if other == &s.id {
                        self.err(
                            PROGRAMS,
                            format!("{sp}.pair_with"),
                            "a slot cannot be paired with itself",
                        );
                    } else if !slot_ids.contains(other) {
                        self.err(
                            PROGRAMS,
                            format!("{sp}.pair_with"),
                            format!("unknown slot `{other}`"),
                        );
                    }
                }
            }
        }
    }

    fn resources(&mut self) {
        let defs = &self.pkg.resources.resource_types;
        self.unique(
            RESOURCES,
            "resource type",
            defs.iter()
                .enumerate()
                .map(|(i, r)| (&r.id, format!("resource_types[{i}].id"))),
        );
        for (i, r) in defs.iter().enumerate() {
            let p = format!("resource_types[{i}]");
            self.text_key(&r.label, RESOURCES, format!("{p}.label"));
            self.fields(&r.attributes, RESOURCES, &format!("{p}.attributes"));
            self.fields(&r.log_fields, RESOURCES, &format!("{p}.log_fields"));
            if let Some(w) = &r.lifecycle {
                self.workflow_ref(w, RESOURCES, format!("{p}.lifecycle"));
            }
            if let Some(c) = &r.holder_requires {
                self.cond(c, RESOURCES, &format!("{p}.holder_requires"), None);
            }
            self.role_ref(&r.manager_role, RESOURCES, format!("{p}.manager_role"));
            for (j, a) in r.assistant_roles.iter().enumerate() {
                self.role_ref(a, RESOURCES, format!("{p}.assistant_roles[{j}]"));
            }
            self.audience_ref(
                &r.read_audience.list,
                RESOURCES,
                format!("{p}.read_audience.list"),
            );
            self.audience_ref(
                &r.read_audience.history,
                RESOURCES,
                format!("{p}.read_audience.history"),
            );
            self.unique(
                RESOURCES,
                "request",
                r.requests
                    .iter()
                    .enumerate()
                    .map(|(j, q)| (&q.id, format!("{p}.requests[{j}].id"))),
            );
            for (j, q) in r.requests.iter().enumerate() {
                let qp = format!("{p}.requests[{j}]");
                self.text_key(&q.label, RESOURCES, format!("{qp}.label"));
                if q.answers.is_empty() {
                    self.err(
                        RESOURCES,
                        format!("{qp}.answers"),
                        "a request needs at least one answer",
                    );
                }
                self.option_list(&q.answers, RESOURCES, &format!("{qp}.answers"));
            }
            if r.rules.max_per_holder == Some(0) {
                self.err(
                    RESOURCES,
                    format!("{p}.rules.max_per_holder"),
                    "max_per_holder must be at least 1",
                );
            }
        }
    }

    fn board(&mut self) {
        let Some(b) = &self.pkg.board.board else {
            return;
        };
        self.audience_ref(&b.default_audience, BOARD, "board.default_audience");
        for (i, a) in b.free_notices.audiences.iter().enumerate() {
            self.audience_ref(a, BOARD, format!("board.free_notices.audiences[{i}]"));
        }
    }

    fn trigger_filter(&mut self, f: &TriggerFilter, file: &str, path: &str) {
        if let Some(x) = &f.form {
            self.form_ref(x, file, format!("{path}.form"));
        }
        if let Some(state) = &f.to_state {
            let workflow = f
                .form
                .as_ref()
                .and_then(|x| self.forms.get(x))
                .map(|x| &x.workflow);
            match workflow.and_then(|w| self.workflows.get(w)) {
                Some(w) if !w.states.contains(state) => {
                    self.err(
                        file,
                        format!("{path}.to_state"),
                        format!("`{state}` is not a state of workflow `{}`", w.id),
                    );
                }
                Some(_) => {}
                None => self.err(file, format!("{path}.to_state"), "`to_state` needs `form`"),
            }
        }
        if let Some(x) = &f.aggregate {
            if !self.aggregates.contains_key(x) {
                self.err(
                    file,
                    format!("{path}.aggregate"),
                    format!("unknown aggregate `{x}`"),
                );
            }
        }
        if let Some(x) = &f.program_type {
            if !self.programs.contains(x) {
                self.err(
                    file,
                    format!("{path}.program_type"),
                    format!("unknown program type `{x}`"),
                );
            }
        }
        if let Some(x) = &f.resource_type {
            if !self.resources.contains_key(x) {
                self.err(
                    file,
                    format!("{path}.resource_type"),
                    format!("unknown resource type `{x}`"),
                );
            }
        }
        if let Some(x) = &f.qualification {
            self.qual_ref(x, file, format!("{path}.qualification"));
        }
    }

    fn notifications(&mut self) {
        let n = &self.pkg.notifications;
        let ids = n
            .notifications
            .iter()
            .enumerate()
            .map(|(i, r)| (&r.id, format!("notifications[{i}].id")))
            .chain(
                n.reminders
                    .iter()
                    .enumerate()
                    .map(|(i, r)| (&r.id, format!("reminders[{i}].id"))),
            );
        self.unique(NOTIFICATIONS, "notification", ids);
        for (i, r) in n.notifications.iter().enumerate() {
            let p = format!("notifications[{i}]");
            self.audience_ref(&r.audience, NOTIFICATIONS, format!("{p}.audience"));
            self.text_key(&r.text, NOTIFICATIONS, format!("{p}.text"));
            self.trigger_filter(&r.filter, NOTIFICATIONS, &format!("{p}.filter"));
        }
        for (i, r) in n.reminders.iter().enumerate() {
            let p = format!("reminders[{i}]");
            self.audience_ref(&r.audience, NOTIFICATIONS, format!("{p}.audience"));
            self.text_key(&r.text, NOTIFICATIONS, format!("{p}.text"));
            self.trigger_filter(&r.filter, NOTIFICATIONS, &format!("{p}.filter"));
        }
    }

    fn view_params(&mut self, params: &ViewParams, path: &str) {
        if let Some(w) = &params.workflow {
            self.workflow_ref(w, UI, format!("{path}.workflow"));
        }
        if let Some(f) = &params.form {
            self.form_ref(f, UI, format!("{path}.form"));
        }
        if let Some(a) = &params.aggregate {
            if !self.aggregates.contains_key(a) {
                self.err(
                    UI,
                    format!("{path}.aggregate"),
                    format!("unknown aggregate `{a}`"),
                );
            }
        }
        if let Some(r) = &params.resource_type {
            if !self.resources.contains_key(r) {
                self.err(
                    UI,
                    format!("{path}.resource_type"),
                    format!("unknown resource type `{r}`"),
                );
            }
        }
        if let Some(u) = &params.unit_type {
            self.unit_type_ref(u, UI, format!("{path}.unit_type"));
        }
        for (i, t) in params.types.iter().enumerate() {
            if !self.programs.contains(t) {
                self.err(
                    UI,
                    format!("{path}.types[{i}]"),
                    format!("unknown program type `{t}`"),
                );
            }
        }
        for (i, q) in params.qualifications.iter().enumerate() {
            self.qual_ref(q, UI, format!("{path}.qualifications[{i}]"));
        }
    }

    fn ui(&mut self) {
        let ui = &self.pkg.ui;
        for (i, v) in ui.home.iter().enumerate() {
            self.view_params(&v.params, &format!("home[{i}].params"));
        }
        for (i, m) in ui.menu.iter().enumerate() {
            let p = format!("menu[{i}]");
            self.text_key(&m.label, UI, format!("{p}.label"));
            if let Some(a) = &m.visible_to {
                self.audience_ref(a, UI, format!("{p}.visible_to"));
            }
            self.view_params(&m.params, &format!("{p}.params"));
        }
        let people = &self.pkg.ui.people;
        for (i, a) in people.attributes.iter().enumerate() {
            if !self
                .pkg
                .manifest
                .person_attributes
                .iter()
                .any(|x| &x.id == a)
            {
                self.err(
                    UI,
                    format!("people.attributes[{i}]"),
                    format!("unknown person attribute `{a}`"),
                );
            }
        }
        let quals: Vec<(String, Slug)> = people
            .dated_qualifications
            .iter()
            .enumerate()
            .map(|(i, q)| (format!("people.dated_qualifications[{i}]"), q.clone()))
            .chain(
                people
                    .qualifications
                    .iter()
                    .enumerate()
                    .map(|(i, q)| (format!("people.qualifications[{i}]"), q.clone())),
            )
            .collect();
        for (path, q) in quals {
            self.qual_ref(&q, UI, path);
        }
    }

    fn texts(&mut self) {
        let default = &self.pkg.manifest.default_locale;
        let empty = BTreeMap::new();
        let base = self.pkg.texts.get(default).unwrap_or(&empty);
        let mut missing = Vec::new();
        for (key, (file, path)) in &self.texts_used {
            if !base.contains_key(key) {
                missing.push(Issue::error(
                    *file,
                    path.clone(),
                    format!("missing text `{key}` for default locale `{default}`"),
                ));
            }
        }
        for (locale, texts) in &self.pkg.texts {
            if locale == default {
                continue;
            }
            for key in self.texts_used.keys() {
                if !texts.contains_key(key) {
                    missing.push(Issue {
                        severity: Severity::Warning,
                        file: format!("texts/{locale}.yaml"),
                        path: key.clone(),
                        message: format!("missing translation for `{key}`"),
                    });
                }
            }
        }
        self.issues.append(&mut missing);
    }
}

fn flatten_all<'c>(c: &'c Cond, out: &mut Vec<&'c Cond>) {
    match c {
        Cond::All(v) => v.iter().for_each(|x| flatten_all(x, out)),
        other => out.push(other),
    }
}

/// True if `to` is reachable from `from` through at least one edge.
fn reaches(graph: &BTreeMap<&Slug, &Vec<Slug>>, from: &Slug, to: &Slug) -> bool {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<&Slug> = graph
        .get(from)
        .map(|v| v.iter().collect())
        .unwrap_or_default();
    while let Some(cur) = stack.pop() {
        if cur == to {
            return true;
        }
        if seen.insert(cur) {
            if let Some(next) = graph.get(cur) {
                stack.extend(next.iter());
            }
        }
    }
    false
}

fn form_field_ids(f: &FormDef) -> impl Iterator<Item = Slug> + '_ {
    f.variants
        .iter()
        .flat_map(|v| v.fields.iter().map(|x| x.id.clone()))
}
