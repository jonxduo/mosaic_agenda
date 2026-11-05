//! Commands by name with JSON arguments, as the Tauri IPC sends them
//! (command name in snake_case, argument names in camelCase). Used by the
//! development HTTP bridge so the browser UI and the Playwright tests drive
//! the same core as the app.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{ApiError, ApiResult, Core};

/// Every command, in the order of the Tauri app's handler.
pub const COMMANDS: &[&str] = &[
    "status",
    "texts",
    "accept_privacy",
    "create_org",
    "join_org",
    "invite_device",
    "pairings",
    "confirm_pairing",
    "set_pin",
    "unlock",
    "config_files",
    "export_backup",
    "export_backup_to",
    "restore_backup",
    "config_app_files",
    "config_check",
    "config_simulate",
    "config_publish",
    "unlock_biometric",
    "set_biometric",
    "lock",
    "menu",
    "home",
    "form",
    "save_document",
    "step_document",
    "my_documents",
    "work_queue",
    "batch_step",
    "aggregate",
    "generate_aggregate",
    "distribute_aggregate",
    "programs",
    "program",
    "published_program",
    "next_period",
    "program_dates",
    "draft_program",
    "assign_slot",
    "assign_slots",
    "prefill_program",
    "profile",
    "save_profile",
    "address_search",
    "send_program",
    "publish_program",
    "respond",
    "add_external",
    "my_assignments",
    "board",
    "resources",
    "resource",
    "my_resources",
    "create_resource",
    "update_resource",
    "scan_resource",
    "attachment",
    "assign_resource",
    "open_request",
    "answer_request",
    "log_resource",
    "return_resource",
    "people",
    "add_person",
    "update_person",
    "grant_qualification",
    "qualification_matrix",
    "set_qualifications",
    "end_grant",
    "create_unit",
    "set_membership",
    "grant_role",
    "revoke_device",
    "set_archive",
    "set_backup_node",
    "export_person",
    "request_erasure",
    "export_person_to",
    "privacy_notice",
    "units",
    "alerts",
    "sync_status",
    "sync_now",
    "notifications",
    "settings",
    "update_settings",
];

fn args<T: DeserializeOwned>(v: Value) -> ApiResult<T> {
    let v = if v.is_null() {
        Value::Object(Default::default())
    } else {
        v
    };
    serde_json::from_value(v).map_err(|e| ApiError::invalid(format!("arguments: {e}")))
}

fn json<T: serde::Serialize>(v: T) -> ApiResult<Value> {
    serde_json::to_value(v).map_err(ApiError::internal)
}

type Map = BTreeMap<String, Value>;

pub async fn dispatch(core: &Arc<Core>, cmd: &str, input: Value) -> ApiResult<Value> {
    macro_rules! call {
        ({ $($field:ident : $ty:ty),* } => $body:expr) => {{
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct A { $($field: $ty),* }
            let A { $($field),* } = args::<A>(input)?;
            json($body)
        }};
    }
    match cmd {
        "status" => json(core.status()?),
        "texts" => json(core.texts_map()),
        "accept_privacy" => json(core.accept_privacy()?),
        "create_org" => {
            call!({ name: String, my_name: String } => core.create_org(name, my_name).await?)
        }
        "join_org" => call!({ ticket: String } => core.join_org(ticket).await?),
        "invite_device" => call!({ person: String } => core.invite_device(person).await?),
        "pairings" => json(core.pairings()?),
        "confirm_pairing" => {
            call!({ code: String, accept: bool } => core.confirm_pairing(code, accept).await?)
        }
        "set_pin" => call!({ pin: Option<String> } => core.set_pin_cmd(pin)?),
        "unlock" => call!({ pin: String } => core.unlock(&pin)?),
        "config_files" => json(core.config_files()?),
        "export_backup" => call!({ passphrase: String } => core.export_backup_base64(&passphrase)?),
        "export_backup_to" => {
            call!({ path: String, passphrase: String } => core.export_backup_to(&path, &passphrase)?)
        }
        "restore_backup" => {
            call!({ data: String, passphrase: String } => core.restore_backup_base64(&data, &passphrase)?)
        }
        "config_app_files" => json(core.config_app_files()?),
        "config_check" => {
            call!({ files: Vec<crate::config::ConfigFile> } => core.config_check(files)?)
        }
        "config_simulate" => {
            call!({ files: Vec<crate::config::ConfigFile> } => core.config_simulate(files)?)
        }
        "config_publish" => {
            call!({ files: Vec<crate::config::ConfigFile> } => core.config_publish(files)?)
        }
        "unlock_biometric" => json(core.unlock_biometric()?),
        "set_biometric" => call!({ on: bool } => core.set_biometric(on)?),
        "lock" => {
            core.lock_now();
            json(())
        }
        "menu" => json(core.menu()?),
        "home" => json(core.home()?),
        "form" => {
            call!({ form: String, subject: Option<String>, period: String } => core.form(form, subject, period)?)
        }
        "save_document" => {
            call!({ form: String, subject: Option<String>, period: String, values: Map } => core.save_document(form, subject, period, values)?)
        }
        "step_document" => {
            call!({ document: String, step: String, note: Option<String> } => core.step_document(document, step, note)?)
        }
        "my_documents" => call!({ form: Option<String> } => core.my_documents(form)?),
        "work_queue" => {
            call!({ workflow: String, unit_type: Option<String> } => core.work_queue(workflow, unit_type)?)
        }
        "batch_step" => {
            call!({ form: String, unit: String, period: String, step: String } => core.batch_step(form, unit, period, step)?)
        }
        "aggregate" => {
            call!({ aggregate: String, period: Option<String> } => core.aggregate(aggregate, period)?)
        }
        "generate_aggregate" => {
            call!({ aggregate: String, period: String, note: Option<String> } => core.generate_aggregate(aggregate, period, note)?)
        }
        "distribute_aggregate" => {
            call!({ aggregate: String, period: String } => core.distribute_aggregate(aggregate, period)?)
        }
        "programs" => call!({ types: Option<String> } => core.programs(types)?),
        "program" => call!({ program: String } => core.program(program)?),
        "published_program" => call!({ program: String } => core.published_program(program)?),
        "next_period" => call!({ form: String } => core.next_period(form)?),
        "program_dates" => {
            call!({ program_type: String, period: Option<String> } => core.program_dates(program_type, period)?)
        }
        "draft_program" => {
            call!({ program_type: String, dates: Vec<String> } => core.draft_program(program_type, dates)?)
        }
        "assign_slot" => {
            call!({ program: String, occurrence: u32, slot: String, index: u32, person: String } => core.assign_slot(program, occurrence, slot, index, person)?)
        }
        "assign_slots" => {
            call!({ program: String, changes: Vec<crate::dto::SlotChange>, details: Vec<crate::dto::DetailChange>, parts: Vec<crate::dto::PartChange> } => core.assign_slots(program, changes, details, parts)?)
        }
        "send_program" => call!({ program: String } => core.send_program(program)?),
        "publish_program" => call!({ program: String } => core.publish_program(program)?),
        "respond" => {
            call!({ program: String, program_type: String, occurrence: u32, slot: String, index: u32, accept: bool, note: Option<String>, assignee: Option<String> } => core.respond(program, program_type, occurrence, slot, index, accept, note, assignee)?)
        }
        "add_external" => call!({ name: String } => core.add_external(name)?),
        "my_assignments" => json(core.my_assignments()?),
        "board" => call!({ month: Option<String> } => core.board(month)?),
        "resources" => call!({ resource_type: String } => core.resources(resource_type)?),
        "resource" => call!({ resource: String } => core.resource(resource)?),
        "my_resources" => {
            call!({ resource_type: Option<String> } => core.my_resources(resource_type)?)
        }
        "update_resource" => {
            call!({ resource: String, attributes: Map } => core.update_resource(resource, attributes)?)
        }
        "scan_resource" => {
            call!({ resource_type: String, image: String } => core.scan_resource(resource_type, image).await?)
        }
        "attachment" => call!({ reference: String } => core.attachment(reference)?),
        "create_resource" => {
            call!({ resource_type: String, attributes: Map } => core.create_resource(resource_type, attributes)?)
        }
        "assign_resource" => {
            call!({ resource: String, holder: String } => core.assign_resource(resource, holder)?)
        }
        "open_request" => {
            call!({ resource: String, request: String } => core.open_request(resource, request)?)
        }
        "answer_request" => {
            call!({ request: String, answer: String } => core.answer_request(request, answer)?)
        }
        "log_resource" => {
            call!({ resource: String, values: Map } => core.log_resource(resource, values)?)
        }
        "return_resource" => call!({ resource: String } => core.return_resource(resource)?),
        "people" => json(core.people()?),
        "add_person" => {
            call!({ name: String, attributes: Map } => core.add_person(name, attributes)?)
        }
        "update_person" => {
            call!({ person: String, name: Option<String>, attributes: Map } => core.update_person(person, name, attributes)?)
        }
        "grant_qualification" => {
            call!({ person: String, qualification: String, from: Option<String>, until: Option<String> } => core.grant_qualification(person, qualification, from, until)?)
        }
        "qualification_matrix" => {
            call!({ qualifications: String } => core.qualification_matrix(qualifications)?)
        }
        "set_qualifications" => {
            call!({ qualifications: String, changes: Vec<crate::matrix::QualificationChange> } => core.set_qualifications(qualifications, changes)?)
        }
        "end_grant" => {
            call!({ kind: String, grant: String, end: Option<String> } => core.end_grant(kind, grant, end)?)
        }
        "create_unit" => {
            call!({ unit_type: String, name: String } => core.create_unit(unit_type, name)?)
        }
        "set_membership" => {
            call!({ person: String, unit: String, from: Option<String> } => core.set_membership(person, unit, from)?)
        }
        "grant_role" => {
            call!({ person: String, role: String, unit: Option<String> } => core.grant_role(person, role, unit)?)
        }
        "revoke_device" => call!({ device: String } => core.revoke_device(device)?),
        "set_backup_node" => call!({ node: Option<String> } => core.set_backup_node(node)?),
        "set_archive" => {
            call!({ device: String, archive: bool } => core.set_archive(device, archive)?)
        }
        "export_person" => call!({ person: String } => core.export_person(person)?),
        "request_erasure" => call!({ person: String } => core.request_erasure(person)?),
        "export_person_to" => {
            call!({ person: String, path: String } => core.export_person_to(person, &path)?)
        }
        "privacy_notice" => json(core.privacy_notice()?),
        "units" => json(core.units()?),
        "alerts" => json(core.alerts()?),
        "sync_status" => json(core.sync_status().await?),
        "sync_now" => json(core.sync_now().await?),
        "profile" => json(core.profile()?),
        "save_profile" => {
            call!({ given_name: Option<String>, family_name: Option<String>, attributes: Map, dated: std::collections::BTreeMap<String, String> } => core.save_profile(given_name, family_name, attributes, dated)?)
        }
        "address_search" => call!({ query: String } => core.address_search(query).await?),
        "prefill_program" => {
            call!({ program: String, occurrences: Option<Vec<u32>> } => core.prefill_program(program, occurrences).await?)
        }
        "notifications" => json(core.take_notifications()?),
        "settings" => json(core.settings()?),
        "update_settings" => {
            call!({ auto_lock_minutes: u32, mobile_data_photos: bool, muted_categories: Vec<String> } => core.update_settings(auto_lock_minutes, mobile_data_photos, muted_categories)?)
        }
        other => Err(ApiError::not_found(format!("command `{other}`"))),
    }
}
