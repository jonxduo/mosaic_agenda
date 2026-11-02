//! Tauri shell: typed commands over `core-api`, system tray, background
//! sync and local notifications. Nothing here knows the domain.

#[cfg(target_os = "android")]
mod background;
mod secrets;

use std::collections::BTreeMap;
use std::sync::Arc;

use core_api::{ApiError, Core, CoreOptions, dto::*};
use core_sync::net::Network;
#[cfg(desktop)]
use tauri::menu::{Menu, MenuItem as TrayItem};
#[cfg(desktop)]
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, State};
use tauri_plugin_notification::NotificationExt;
use tauri_specta::{Builder, collect_commands};

/// Package embedded at build time (see build.rs).
const BUNDLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bundle.bin"));

type R<T> = Result<T, ApiError>;

/// The core, once opened. On mobile it opens off the main thread (the
/// secret store plugin needs that thread), so the first calls may find it
/// still empty and answer `starting`; the UI retries.
#[derive(Default)]
pub struct CoreSlot(std::sync::OnceLock<Arc<Core>>);

impl CoreSlot {
    fn get(&self) -> R<Arc<Core>> {
        self.0
            .get()
            .cloned()
            .ok_or_else(|| ApiError::new("starting", "the app is starting"))
    }
}
type Map = JsonMap;

macro_rules! cmd {
    ($name:ident ( $($arg:ident : $ty:ty),* ) -> $ret:ty => |$core:ident| $body:expr) => {
        // Arguments mirror the IPC payload one to one.
        #[allow(clippy::too_many_arguments)]
        #[tauri::command]
        #[specta::specta]
        async fn $name(state: State<'_, CoreSlot>, $($arg: $ty),*) -> R<$ret> {
            let $core = state.get()?;
            $body
        }
    };
}

cmd!(status() -> AppStatus => |c| c.status());
cmd!(texts() -> BTreeMap<String, String> => |c| Ok(c.texts_map()));
cmd!(accept_privacy() -> () => |c| c.accept_privacy());
cmd!(create_org(name: String, my_name: String) -> AppStatus => |c| c.create_org(name, my_name).await);
cmd!(join_org(ticket: String) -> String => |c| c.join_org(ticket).await);
cmd!(invite_device(person: String) -> Invitation => |c| c.invite_device(person).await);
cmd!(pairings() -> Vec<Pairing> => |c| c.pairings());
cmd!(confirm_pairing(code: String, accept: bool) -> () => |c| c.confirm_pairing(code, accept).await);
cmd!(set_pin(pin: Option<String>) -> () => |c| c.set_pin_cmd(pin));
cmd!(unlock(pin: String) -> bool => |c| c.unlock(&pin));
cmd!(unlock_biometric() -> bool => |c| c.unlock_biometric());
cmd!(set_biometric(on: bool) -> () => |c| c.set_biometric(on));
cmd!(lock() -> () => |c| { c.lock_now(); Ok(()) });
cmd!(menu() -> Vec<MenuItem> => |c| c.menu());
cmd!(home() -> Home => |c| c.home());
cmd!(form(form: String, subject: Option<String>, period: String) -> FormView => |c| c.form(form, subject, period));
cmd!(save_document(form: String, subject: Option<String>, period: String, values: Map) -> FormView => |c| c.save_document(form, subject, period, values.0));
cmd!(step_document(document: String, step: String, note: Option<String>) -> FormView => |c| c.step_document(document, step, note));
cmd!(my_documents(form: Option<String>) -> Vec<DocumentRow> => |c| c.my_documents(form));
cmd!(work_queue(workflow: String, unit_type: Option<String>) -> WorkQueue => |c| c.work_queue(workflow, unit_type));
cmd!(batch_step(form: String, unit: String, period: String, step: String) -> () => |c| c.batch_step(form, unit, period, step));
cmd!(aggregate(aggregate: String, period: Option<String>) -> AggregateView => |c| c.aggregate(aggregate, period));
cmd!(generate_aggregate(aggregate: String, period: String, note: Option<String>) -> AggregateView => |c| c.generate_aggregate(aggregate, period, note));
cmd!(distribute_aggregate(aggregate: String, period: String) -> AggregateView => |c| c.distribute_aggregate(aggregate, period));
cmd!(programs(types: Option<String>) -> ProgramList => |c| c.programs(types));
cmd!(program(program: String) -> ProgramEditor => |c| c.program(program));
cmd!(draft_program(program_type: String, dates: Vec<String>) -> String => |c| c.draft_program(program_type, dates));
cmd!(assign_slot(program: String, occurrence: u32, slot: String, index: u32, person: String) -> ProgramEditor => |c| c.assign_slot(program, occurrence, slot, index, person));
cmd!(send_program(program: String) -> ProgramEditor => |c| c.send_program(program));
cmd!(publish_program(program: String) -> ProgramEditor => |c| c.publish_program(program));
cmd!(respond(program: String, program_type: String, occurrence: u32, slot: String, index: u32, accept: bool, note: Option<String>, assignee: Option<String>) -> () => |c| c.respond(program, program_type, occurrence, slot, index, accept, note, assignee));
cmd!(add_external(name: String) -> String => |c| c.add_external(name));
cmd!(my_assignments() -> Vec<AssignmentItem> => |c| c.my_assignments());
cmd!(board() -> Vec<BoardItem> => |c| c.board());
cmd!(resources(resource_type: String) -> ResourceList => |c| c.resources(resource_type));
cmd!(resource(resource: String) -> ResourceDetail => |c| c.resource(resource));
cmd!(my_resources(resource_type: Option<String>) -> Vec<HeldResource> => |c| c.my_resources(resource_type));
cmd!(create_resource(resource_type: String, attributes: Map) -> String => |c| c.create_resource(resource_type, attributes.0));
cmd!(assign_resource(resource: String, holder: String) -> () => |c| c.assign_resource(resource, holder));
cmd!(open_request(resource: String, request: String) -> () => |c| c.open_request(resource, request));
cmd!(answer_request(request: String, answer: String) -> () => |c| c.answer_request(request, answer));
cmd!(log_resource(resource: String, values: Map) -> () => |c| c.log_resource(resource, values.0));
cmd!(return_resource(resource: String) -> () => |c| c.return_resource(resource));
cmd!(people() -> People => |c| c.people());
cmd!(add_person(name: String, attributes: Map) -> String => |c| c.add_person(name, attributes.0));
cmd!(update_person(person: String, name: Option<String>, attributes: Map) -> () => |c| c.update_person(person, name, attributes.0));
cmd!(grant_qualification(person: String, qualification: String, from: Option<String>, until: Option<String>) -> () => |c| c.grant_qualification(person, qualification, from, until));
cmd!(end_grant(kind: String, grant: String, end: Option<String>) -> () => |c| c.end_grant(kind, grant, end));
cmd!(create_unit(unit_type: String, name: String) -> String => |c| c.create_unit(unit_type, name));
cmd!(set_membership(person: String, unit: String, from: Option<String>) -> () => |c| c.set_membership(person, unit, from));
cmd!(grant_role(person: String, role: String, unit: Option<String>) -> () => |c| c.grant_role(person, role, unit));
cmd!(revoke_device(device: String) -> () => |c| c.revoke_device(device));
cmd!(units() -> Vec<UnitView> => |c| c.units());
cmd!(alerts() -> Vec<Alert> => |c| c.alerts());
cmd!(sync_status() -> SyncStatus => |c| c.sync_status().await);
cmd!(sync_now() -> SyncStatus => |c| c.sync_now().await);
cmd!(notifications() -> Vec<Notification> => |c| c.take_notifications());
cmd!(settings() -> Settings => |c| c.settings());
cmd!(update_settings(auto_lock_minutes: u32, mobile_data_photos: bool, muted_categories: Vec<String>) -> Settings => |c| c.update_settings(auto_lock_minutes, mobile_data_photos, muted_categories));

/// The typed command set; also used to export the TypeScript bindings.
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        status,
        texts,
        accept_privacy,
        create_org,
        join_org,
        invite_device,
        pairings,
        confirm_pairing,
        set_pin,
        unlock,
        unlock_biometric,
        set_biometric,
        lock,
        menu,
        home,
        form,
        save_document,
        step_document,
        my_documents,
        work_queue,
        batch_step,
        aggregate,
        generate_aggregate,
        distribute_aggregate,
        programs,
        program,
        draft_program,
        assign_slot,
        send_program,
        publish_program,
        respond,
        add_external,
        my_assignments,
        board,
        resources,
        resource,
        my_resources,
        create_resource,
        assign_resource,
        open_request,
        answer_request,
        log_resource,
        return_resource,
        people,
        add_person,
        update_person,
        grant_qualification,
        end_grant,
        create_unit,
        set_membership,
        grant_role,
        revoke_device,
        units,
        alerts,
        sync_status,
        sync_now,
        notifications,
        settings,
        update_settings,
    ])
}

/// Writes `bindings.ts` for the UI.
pub fn export_bindings(path: &str) -> Result<(), String> {
    let ts = specta_typescript::Typescript::default()
        .header("// Generated by `cargo xtask bindings`. Do not edit.\n");
    specta_builder().export(ts, path).map_err(|e| e.to_string())
}

/// Development fallback; `APP_NETWORK=local` for tests on one machine.
fn network() -> Network {
    match std::env::var("APP_NETWORK").as_deref() {
        Ok("local") => Network::Local,
        _ => Network::PublicRelays,
    }
}

/// Server written in the genesis of organizations created by this build:
/// `APP_RELAY_URLS` (comma separated), `APP_DNS_URL`, `APP_PUSH_URL`, set by
/// `cargo xtask build --server <file>`.
fn server() -> Option<core_api::NetworkParams> {
    let relays: Vec<String> = option_env!("APP_RELAY_URLS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    (!relays.is_empty()).then(|| core_api::NetworkParams {
        relay_urls: relays,
        dns_url: option_env!("APP_DNS_URL")
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        push_url: option_env!("APP_PUSH_URL")
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
    })
}

/// Keeps syncing while the app runs and shows new notifications (§9.2, §11.1).
async fn background(app: tauri::AppHandle, core: Arc<Core>) {
    let _ = core.start_network().await;
    let mut announced_locked = 0;
    loop {
        let _ = core.sync_round().await;
        if core.is_locked() {
            // Content stays sealed until the unlock: a generic notice only.
            let pending = core.pending_while_locked();
            if pending > announced_locked {
                let title = core
                    .status()
                    .map(|s| s.branding.app_name)
                    .unwrap_or_default();
                let body = core.texts().get("push.generic");
                let _ = app
                    .notification()
                    .builder()
                    .title(&title)
                    .body(&body)
                    .show();
            }
            announced_locked = pending;
        } else {
            announced_locked = 0;
            if let Ok(list) = core.take_notifications() {
                let title = core
                    .status()
                    .map(|s| s.branding.app_name)
                    .unwrap_or_default();
                for n in list {
                    let _ = app
                        .notification()
                        .builder()
                        .title(&title)
                        .body(&n.text)
                        .show();
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }
}

#[cfg(desktop)]
fn tray(app: &tauri::App, core: &Arc<Core>) -> tauri::Result<()> {
    let title = core
        .status()
        .map(|s| s.branding.app_name)
        .unwrap_or_default();
    let open = TrayItem::with_id(app, "open", "Apri", true, None::<&str>)?;
    let sync = TrayItem::with_id(app, "sync", "Sincronizza", true, None::<&str>)?;
    let quit = TrayItem::with_id(app, "quit", "Esci", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &sync, &quit])?;
    let core = core.clone();
    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip(&title)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "sync" => {
                let core = core.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = core.sync_round().await;
                });
            }
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn open_core(
    data_dir: std::path::PathBuf,
    secrets: Arc<dyn core_crypto::SecretStore>,
) -> Result<Arc<Core>, Box<dyn std::error::Error>> {
    Core::open(CoreOptions {
        data_dir,
        bundle: BUNDLE.to_vec(),
        secrets,
        network: Some(network()),
        clock: None,
        pin_params: None,
        server: server(),
        security: None,
    })
    .map_err(|e| Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error>)
}

/// Makes the core available to the commands and starts the background loop.
fn started(app: &tauri::AppHandle, core: Arc<Core>) {
    let _ = app.state::<CoreSlot>().0.set(core.clone());
    #[cfg(target_os = "android")]
    let _ = background::RUNNING.set(core.clone());
    tauri::async_runtime::spawn(background(app.clone(), core));
}

/// Entry point on Android and iOS.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn start() {
    if let Err(e) = run() {
        eprintln!("{e}");
    }
}

pub fn run() -> Result<(), String> {
    if BUNDLE.is_empty() {
        return Err(
            "this build has no package: build it with `cargo xtask dev|build --package <name>`"
                .into(),
        );
    }
    let builder = specta_builder();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_secret_store::init());
    // Biometrics and QR scanning exist on mobile only.
    #[cfg(mobile)]
    let app = app
        .plugin(tauri_plugin_biometric::init())
        .plugin(tauri_plugin_barcode_scanner::init());
    app.invoke_handler(builder.invoke_handler())
        .manage(CoreSlot::default())
        .setup(move |app| {
            builder.mount_events(app);
            let data_dir = app.path().app_data_dir()?;
            #[cfg(desktop)]
            {
                let secrets = Arc::new(secrets::KeyringSecretStore::new(&app.config().identifier));
                let core = open_core(data_dir, secrets)?;
                started(app.handle(), core.clone());
                tray(app, &core)?;
            }
            #[cfg(mobile)]
            {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    let secrets = Arc::new(secrets::PluginSecretStore(handle.clone()));
                    match open_core(data_dir, secrets) {
                        Ok(core) => started(&handle, core),
                        Err(e) => eprintln!("cannot open the core: {e}"),
                    }
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .map_err(|e| e.to_string())
}
