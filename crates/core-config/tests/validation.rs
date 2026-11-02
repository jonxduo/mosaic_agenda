//! Every validation rule of PROGETTO.md §6.4, with a valid case (the
//! fixture) and an invalid one (the fixture with one mutation). Errors must
//! name file and YAML path.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use core_config::{Issue, Severity, compile, decode, load_and_validate};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/club")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A copy of the fixture with textual edits: `(file, from, to)`.
fn mutated(edits: &[(&str, &str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture_dir(), dir.path());
    for (file, from, to) in edits {
        let path = dir.path().join(file);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains(from),
            "fixture {file} does not contain {from:?}"
        );
        std::fs::write(&path, text.replacen(from, to, 1)).unwrap();
    }
    dir
}

fn errors(edits: &[(&str, &str, &str)]) -> Vec<Issue> {
    let dir = mutated(edits);
    match load_and_validate(dir.path()) {
        Ok((_, warnings)) => {
            panic!("expected errors, got a valid package (warnings: {warnings:?})")
        }
        Err(issues) => issues
            .into_iter()
            .filter(|i| i.severity == Severity::Error)
            .collect(),
    }
}

#[track_caller]
fn assert_error(edits: &[(&str, &str, &str)], file: &str, path: &str, message: &str) {
    let errs = errors(edits);
    assert!(
        errs.iter()
            .any(|e| e.file == file && e.path.contains(path) && e.message.contains(message)),
        "no error in {file} at {path:?} containing {message:?}; got:\n{}",
        errs.iter()
            .map(|e| format!("  {e}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn fixture_is_valid_and_compiles_deterministically() {
    let (pkg, warnings) = load_and_validate(&fixture_dir()).unwrap_or_else(|e| {
        panic!(
            "fixture invalid:\n{}",
            e.iter().map(|i| format!("  {i}\n")).collect::<String>()
        )
    });
    assert!(warnings.is_empty(), "{warnings:?}");
    // x- keys are dropped and anchors expanded.
    let host = &pkg.programs.program_types[0].slots[0];
    assert!(host.requires.is_some());
    // Nested text keys are flattened.
    assert!(pkg.texts.values().next().unwrap().contains_key("q.member"));
    let a = compile(pkg.clone()).unwrap();
    let b = compile(pkg.clone()).unwrap();
    assert_eq!(a, b, "compilation must be deterministic");
    let decoded = decode(&a.bytes).unwrap();
    assert_eq!(decoded.package, pkg);
    assert!(
        decoded
            .index
            .implies_closure
            .contains_key(&"senior".parse().unwrap())
    );
}

#[test]
fn every_package_in_the_repository_is_valid() {
    let packages = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages");
    let mut count = 0;
    for entry in std::fs::read_dir(packages).unwrap() {
        let dir = entry.unwrap().path();
        if dir.join("package.yaml").is_file() {
            count += 1;
            if let Err(issues) = load_and_validate(&dir) {
                panic!(
                    "{} is invalid:\n{}",
                    dir.display(),
                    issues
                        .iter()
                        .map(|i| format!("  {i}\n"))
                        .collect::<String>()
                );
            }
        }
    }
    assert!(count > 0);
}

// ----- parsing -------------------------------------------------------------

#[test]
fn yaml_syntax_error_names_the_file() {
    assert_error(
        &[("units.yaml", "unit_types:", "unit_types: [")],
        "units.yaml",
        "",
        "invalid YAML",
    );
}

#[test]
fn unknown_key_names_its_path() {
    assert_error(
        &[("roles.yaml", "priority: 200", "priorty: 200")],
        "roles.yaml",
        "roles[0]",
        "unknown field `priorty`",
    );
}

#[test]
fn misspelled_condition_names_its_path() {
    assert_error(
        &[(
            "roles.yaml",
            "requires: { has_qualification: senior }",
            "requires: { has_qualifcation: senior }",
        )],
        "roles.yaml",
        "roles[0].requires",
        "has_qualifcation",
    );
}

#[test]
fn bad_slug_is_rejected() {
    assert_error(
        &[("units.yaml", "id: team", "id: Team")],
        "units.yaml",
        "unit_types[0].id",
        "[a-z][a-z0-9_]*",
    );
}

#[test]
fn missing_required_file_is_reported() {
    assert_error(
        &[("package.yaml", " ui.yaml]", "]")],
        "package.yaml",
        "files",
        "ui.yaml",
    );
}

// ----- references -----------------------------------------------------------

#[test]
fn unknown_qualification_reference() {
    assert_error(
        &[(
            "roles.yaml",
            "requires: { has_qualification: senior }",
            "requires: { has_qualification: nobody }",
        )],
        "roles.yaml",
        "roles[0].requires.has_qualification",
        "unknown qualification `nobody`",
    );
}

#[test]
fn unknown_audience_reference() {
    assert_error(
        &[("forms.yaml", "sent: log_readers", "sent: strangers")],
        "forms.yaml",
        "forms[0].read_audience_by_state.sent",
        "unknown audience `strangers`",
    );
}

#[test]
fn unknown_permission_object() {
    assert_error(
        &[(
            "roles.yaml",
            "on: form:log, target",
            "on: form:diary, target",
        )],
        "roles.yaml",
        "roles[0].permissions[0].on",
        "unknown form `diary`",
    );
}

#[test]
fn permission_on_wrong_object_kind() {
    assert_error(
        &[(
            "roles.yaml",
            "batch.forward, on: form:log",
            "batch.forward, on: program:weekly_plan",
        )],
        "roles.yaml",
        "roles[0].permissions[1].on",
        "cannot apply to a program",
    );
}

#[test]
fn state_of_another_workflow() {
    assert_error(
        &[("forms.yaml", "filed: log_readers", "archived: log_readers")],
        "forms.yaml",
        "read_audience_by_state.archived",
        "not a state of workflow",
    );
}

#[test]
fn system_audience_cannot_be_redefined() {
    assert_error(
        &[("audiences.yaml", "id: keepers", "id: subject_only")],
        "audiences.yaml",
        "audiences[2].id",
        "system audience",
    );
}

// ----- workflows -----------------------------------------------------------

#[test]
fn unreachable_workflow_state() {
    assert_error(
        &[(
            "workflows.yaml",
            "states: [draft, sent,",
            "states: [draft, limbo, sent,",
        )],
        "workflows.yaml",
        "workflows[0].states[1]",
        "`limbo` is unreachable",
    );
}

#[test]
fn workflow_without_final_state() {
    assert_error(
        &[(
            "workflows.yaml",
            "    timers:",
            "      - { id: reopen, kind: transition, from: [filed], to: draft, by: keepers }\n    timers:",
        )],
        "workflows.yaml",
        "workflows[0].steps",
        "no final state",
    );
}

#[test]
fn step_without_by() {
    assert_error(
        &[("workflows.yaml", "to: filed, by: keepers", "to: filed")],
        "workflows.yaml",
        "workflows[0].steps[3].by",
        "needs `by`",
    );
}

#[test]
fn step_with_unresolvable_by() {
    assert_error(
        &[(
            "workflows.yaml",
            "to: filed, by: keepers",
            "to: filed, by: ghosts",
        )],
        "workflows.yaml",
        "workflows[0].steps[3].by",
        "unknown audience `ghosts`",
    );
}

// ----- qualifications -------------------------------------------------------

#[test]
fn implies_and_excludes_contradiction() {
    assert_error(
        &[(
            "qualifications.yaml",
            "implies: [member], excludes: [senior]",
            "implies: [member, senior], excludes: [senior]",
        )],
        "qualifications.yaml",
        "qualifications[2]",
        "exclude each other",
    );
}

#[test]
fn implication_cycle() {
    assert_error(
        &[(
            "qualifications.yaml",
            "{ id: member, label: q.member, duration: permanent }",
            "{ id: member, label: q.member, duration: permanent, implies: [senior] }",
        )],
        "qualifications.yaml",
        "qualifications[0].implies",
        "implication cycle",
    );
}

#[test]
fn role_requiring_impossible_qualifications() {
    assert_error(
        &[(
            "roles.yaml",
            "requires: { has_qualification: senior }",
            "requires: { all: [ { has_qualification: senior }, { has_qualification: junior } ] }",
        )],
        "roles.yaml",
        "roles[0].requires",
        "can never be obtained",
    );
}

#[test]
fn role_requiring_conflicting_attributes_through_qualification() {
    // senior requires level >= 2 (not an equality), so pin `size` instead.
    assert_error(
        &[
            (
                "qualifications.yaml",
                "requires: { attribute: { name: level, op: ge, value: 2 } }",
                "requires: { attribute: { name: size, op: eq, value: l } }",
            ),
            (
                "roles.yaml",
                "requires: { has_qualification: senior }",
                "requires: { all: [ { has_qualification: senior }, { attribute: { name: size, op: eq, value: s } } ] }",
            ),
        ],
        "roles.yaml",
        "roles[0].requires",
        "can never be obtained",
    );
}

#[test]
fn fixed_period_without_unit() {
    assert_error(
        &[(
            "qualifications.yaml",
            "duration: fixed_period, fixed_period_unit: month",
            "duration: fixed_period",
        )],
        "qualifications.yaml",
        "qualifications[3].fixed_period_unit",
        "needs fixed_period_unit",
    );
}

// ----- audiences ------------------------------------------------------------

#[test]
fn recursive_audience() {
    assert_error(
        &[(
            "audiences.yaml",
            "{ role_holders: { role: keeper } }",
            "{ audience: log_readers }",
        )],
        "audiences.yaml",
        "audiences[2].select",
        "defined recursively",
    );
}

// ----- fields ---------------------------------------------------------------

#[test]
fn computed_field_cycle() {
    assert_error(
        &[(
            "forms.yaml",
            "- { id: bonus, type: integer, min: 0 }",
            "- { id: bonus, type: computed, expr: { field: total } }",
        )],
        "forms.yaml",
        "forms[0].variants[0].fields",
        "circular reference",
    );
}

#[test]
fn computed_field_unknown_reference_and_arity() {
    let edits = [(
        "forms.yaml",
        "expr: { sum: [ { field: count }, { field: bonus } ] }",
        "expr: { sub: [ { field: nothing } ] }",
    )];
    assert_error(
        &edits,
        "forms.yaml",
        "forms[0].variants[0].fields[2].expr",
        "unknown field `nothing`",
    );
    assert_error(
        &edits,
        "forms.yaml",
        "forms[0].variants[0].fields[2].expr",
        "exactly two operands",
    );
}

#[test]
fn misplaced_field_option() {
    assert_error(
        &[(
            "forms.yaml",
            "- { id: done, type: boolean }",
            "- { id: done, type: boolean, max_length: 3 }",
        )],
        "forms.yaml",
        "forms[0].variants[1].fields[0]",
        "`max_length` does not apply",
    );
}

#[test]
fn field_condition_outside_form_context() {
    assert_error(
        &[(
            "roles.yaml",
            "requires: { has_qualification: member }",
            "requires: { field: { name: done, op: set } }",
        )],
        "roles.yaml",
        "roles[1].requires.field",
        "only allowed where document fields exist",
    );
}

// ----- aggregates -----------------------------------------------------------

#[test]
fn aggregate_field_missing_from_variants() {
    assert_error(
        &[(
            "aggregates.yaml",
            "op: sum, field: count",
            "op: sum, field: weight",
        )],
        "aggregates.yaml",
        "aggregates[0].measures[1].field",
        "not present in any variant",
    );
}

#[test]
fn aggregate_op_incompatible_with_field_type() {
    assert_error(
        &[(
            "aggregates.yaml",
            "op: count_true, field: done",
            "op: count_true, field: count",
        )],
        "aggregates.yaml",
        "aggregates[0].measures[0].field",
        "cannot be applied",
    );
}

// ----- texts ----------------------------------------------------------------

#[test]
fn missing_text_for_default_locale() {
    assert_error(
        &[("texts/en.yaml", "r.keeper: Keeper\n", "")],
        "roles.yaml",
        "roles[1].label",
        "missing text `r.keeper`",
    );
}

#[test]
fn missing_default_label_text() {
    // fld.bonus is the default label of a field without `label`.
    assert_error(
        &[("texts/en.yaml", "fld.bonus: Bonus\n", "")],
        "forms.yaml",
        "forms[0].variants[0].fields[1]",
        "missing text `fld.bonus`",
    );
}

// ----- programs, manifest ---------------------------------------------------

#[test]
fn slot_paired_with_unknown_slot() {
    assert_error(
        &[("programs.yaml", "pair_with: helper", "pair_with: nobody")],
        "programs.yaml",
        "program_types[0].slots[1].pair_with",
        "unknown slot `nobody`",
    );
}

#[test]
fn incompatible_core_version() {
    assert_error(
        &[("package.yaml", "\">=1.0, <2.0\"", "\">=2.0\"")],
        "package.yaml",
        "core_compat",
        "outside",
    );
}

#[test]
fn bad_branding_values() {
    let edits = [
        ("package.yaml", "\"#336699\"", "blue"),
        ("package.yaml", "org.example.club", "club"),
    ];
    assert_error(&edits, "package.yaml", "branding.primary_color", "color");
    assert_error(&edits, "package.yaml", "branding.identifier", "reverse-DNS");
}

#[test]
fn errors_display_file_and_path() {
    let errs = errors(&[("units.yaml", "label: u.team", "label: u.missing")]);
    let line = errs[0].to_string();
    assert!(
        line.starts_with("error: units.yaml: unit_types[0].label: "),
        "{line}"
    );
}

#[test]
fn empty_package_is_valid() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let files: Vec<&str> = core_config::PACKAGE_FILES.iter().map(|(f, _)| *f).collect();
    std::fs::write(
        root.join("package.yaml"),
        format!(
            "id: empty\nversion: 0.0.1\ncore_compat: \">=1.0\"\ndefault_locale: en\nlocales: [en]\n\
             branding: {{ app_name: app.name, identifier: org.example.empty, icon: icon.png, primary_color: \"#000000\" }}\n\
             files: [{}]\nprivacy_notice: privacy.md\n",
            files.join(", ")
        ),
    )
    .unwrap();
    for f in &files {
        std::fs::write(root.join(f), "").unwrap();
    }
    std::fs::create_dir_all(root.join("texts")).unwrap();
    std::fs::write(root.join("texts/en.yaml"), "app.name: Empty\n").unwrap();
    std::fs::write(root.join("privacy.md"), "# Privacy\n").unwrap();
    std::fs::copy(
        fixture_dir().join("branding/icon.png"),
        root.join("icon.png"),
    )
    .unwrap();
    let (pkg, warnings) = load_and_validate(root).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(warnings.is_empty());
    assert!(compile(pkg).is_ok());
}

/// A bundle whose compiled part a newer core cannot read (a field added to
/// the model) is compiled again from the sources it carries.
#[test]
fn bundle_recompiled_from_sources_when_the_model_changed() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/club");
    let (pkg, _) = core_config::load_and_validate(&dir).unwrap();
    let sources = core_config::source_files(&dir).unwrap();
    let bundle = core_config::compile_with_sources(pkg.clone(), sources.clone()).unwrap();
    let normal = core_config::decode(&bundle.bytes).unwrap();
    assert_eq!(normal.package, pkg);
    assert_eq!(normal.sources, sources);

    // Same header, unreadable compiled part. The compiled part is the same
    // in a bundle without sources, whose header is: format, version, empty map.
    let bare = core_config::compile(pkg.clone()).unwrap();
    let bare_header = 1 + 1 + core_model::CORE_VERSION.len() + 1;
    let body = bare.bytes.len() - bare_header;
    let mut broken = bundle.bytes[..bundle.bytes.len() - body].to_vec();
    broken.extend([0xff; 8]);
    let again = core_config::decode(&broken).unwrap();
    assert_eq!(again.package, pkg);

    // Without sources there is nothing to recompile from.
    let mut broken = bare.bytes[..bare_header].to_vec();
    broken.extend([0xff; 8]);
    assert!(core_config::decode(&broken).is_err());
}
