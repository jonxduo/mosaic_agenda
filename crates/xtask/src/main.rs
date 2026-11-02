//! Build tasks: `cargo xtask <command>` (PROGETTO.md §4.1).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use core_config::{Issue, Severity};

#[derive(Parser)]
#[command(name = "xtask", about = "Build tasks for the core and its packages")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Regenerate schemas/package.schema.json from core-model.
    Schema {
        /// Fail instead of writing when the committed schema is out of date.
        #[arg(long)]
        check: bool,
    },
    /// Validate a package (or every package with --all).
    Validate {
        #[arg(long)]
        package: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// Validate and compile a package into target/package/<name>/bundle.bin.
    /// `--package` takes a name under packages/ or a path to a package folder.
    Bundle {
        #[arg(long)]
        package: String,
    },
    /// Fail if domain terms declared by any package appear in the core.
    CheckDomainLeak,
    /// Run the scenario tests of a package (phase F3).
    TestPackage {
        #[arg(long)]
        package: String,
    },
    /// Full app build for a target (`desktop` today; mobile arrives in F7/F8).
    Build {
        #[arg(long)]
        package: String,
        #[arg(long, default_value = "desktop")]
        target: String,
        #[arg(long)]
        release: bool,
        #[command(flatten)]
        server: ServerArgs,
    },
    /// Desktop development run (Tauri dev with hot reload of the UI).
    Dev {
        #[arg(long)]
        package: String,
        #[command(flatten)]
        server: ServerArgs,
    },
    /// Regenerate the TypeScript bindings of the app commands.
    Bindings {
        /// Fail instead of writing when the committed bindings are out of date.
        #[arg(long)]
        check: bool,
    },
    /// Multi-device simulation (phase F4).
    Simulate {
        #[arg(long)]
        package: String,
        #[arg(long, default_value_t = 20)]
        devices: u32,
        #[arg(long, default_value_t = 60)]
        days: u32,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
}

/// Own server written in the genesis of organizations created by the app
/// (docs/DEPLOY.md). Without it: n0's public relays (development).
#[derive(clap::Args, Debug, Default)]
struct ServerArgs {
    /// Domain of the support server: relay.<domain>:8443, api.<domain>.
    #[arg(long)]
    server: Option<String>,
    /// Also use the discovery service at dns.<domain>.
    #[arg(long, requires = "server")]
    dns: bool,
}

impl ServerArgs {
    fn env(&self) -> Vec<(&'static str, String)> {
        match &self.server {
            None => Vec::new(),
            Some(d) => {
                let mut v = vec![
                    ("APP_RELAY_URLS", format!("https://relay.{d}:8443")),
                    ("APP_PUSH_URL", format!("https://api.{d}")),
                ];
                if self.dns {
                    v.push(("APP_DNS_URL", format!("https://dns.{d}/pkarr")));
                }
                v
            }
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// A package by name (under packages/) or by path to its folder.
fn package_dir(name: &str) -> Result<PathBuf> {
    let as_path = Path::new(name);
    if as_path.components().count() > 1 && as_path.join("package.yaml").is_file() {
        return Ok(as_path.to_path_buf());
    }
    let dir = root().join("packages").join(name);
    if !dir.join("package.yaml").is_file() {
        bail!("package `{name}` not found in {}", dir.display());
    }
    Ok(dir)
}

/// Folder name used for the build output of a package.
fn package_key(name: &str) -> String {
    Path::new(name)
        .file_name()
        .map_or_else(|| name.to_owned(), |n| n.to_string_lossy().into_owned())
}

fn all_packages() -> Result<Vec<String>> {
    let mut out = Vec::new();
    let dir = root().join("packages");
    for entry in std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        if entry.path().join("package.yaml").is_file() {
            out.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    out.sort();
    Ok(out)
}

fn print_issues(issues: &[Issue]) {
    for i in issues {
        eprintln!("  {i}");
    }
}

fn validate(name: &str) -> Result<core_model::config::PackageConfig> {
    let dir = package_dir(name)?;
    match core_config::load_and_validate(&dir) {
        Ok((pkg, warnings)) => {
            print_issues(&warnings);
            println!("package `{name}`: valid ({} warnings)", warnings.len());
            Ok(pkg)
        }
        Err(issues) => {
            print_issues(&issues);
            let errors = issues
                .iter()
                .filter(|i| i.severity == Severity::Error)
                .count();
            bail!("package `{name}`: {errors} errors");
        }
    }
}

fn schema(check: bool) -> Result<()> {
    let path = root().join("schemas/package.schema.json");
    let fresh = core_config::schema::package_schema_pretty();
    if check {
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        if current != fresh {
            bail!(
                "{} is out of date: run `cargo xtask schema`",
                path.display()
            );
        }
        println!("schema is up to date");
    } else {
        std::fs::write(&path, fresh).with_context(|| format!("writing {}", path.display()))?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

fn bundle(name: &str) -> Result<PathBuf> {
    let pkg = validate(name)?;
    let bundle = core_config::compile(pkg).context("compiling the package")?;
    let out = root().join("target/package").join(package_key(name));
    std::fs::create_dir_all(&out)?;
    std::fs::write(out.join("bundle.bin"), &bundle.bytes)?;
    std::fs::write(out.join("bundle.hash"), bundle.hash_hex())?;
    // Round-trip check: what we ship must decode.
    core_config::decode(&bundle.bytes).context("decoding the bundle just written")?;
    println!(
        "wrote {} ({} bytes, blake3 {})",
        out.join("bundle.bin").display(),
        bundle.bytes.len(),
        bundle.hash_hex()
    );
    Ok(out.join("bundle.bin"))
}

/// Directories scanned for domain terms, and parts skipped inside them.
const LEAK_SCAN: &[&str] = &["crates", "apps/app", "server"];
const LEAK_SKIP: &[&str] = &[
    "target",
    "node_modules",
    "gen",
    "static/package",
    ".svelte-kit",
    "build",
];

fn domain_terms() -> Result<Vec<(String, String)>> {
    let mut terms = Vec::new();
    for name in all_packages()? {
        let path = root().join("packages").join(&name).join("package.yaml");
        let text = std::fs::read_to_string(&path)?;
        let manifest = core_config::parse_manifest(&text)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .with_context(|| format!("reading domain_terms from {}", path.display()))?;
        for t in manifest.domain_terms {
            terms.push((t.to_lowercase(), name.clone()));
        }
    }
    Ok(terms)
}

/// True when a path (relative to a scanned directory) is inside a skipped part.
fn is_skipped(rel: &str) -> bool {
    rel.split('/').any(|seg| LEAK_SKIP.contains(&seg))
        || LEAK_SKIP.iter().any(|s| {
            s.contains('/')
                && (rel == *s
                    || rel.starts_with(&format!("{s}/"))
                    || rel.contains(&format!("/{s}/"))
                    || rel.ends_with(&format!("/{s}")))
        })
}

/// Case-insensitive search of `terms` (lowercase, with their package) in the
/// text files under `root/<dir>` for each scanned dir. Returns `path:line: term`.
/// Names of third-party APIs that happen to contain a domain term; removed
/// from a line before it is checked (lowercase).
const THIRD_PARTY_NAMES: &[&str] = &[
    // iroh's pkarr discovery announcer (crate iroh::address_lookup).
    "pkarrpublisher",
];

fn scan_for_terms(root: &Path, dirs: &[&str], terms: &[(String, String)]) -> Result<Vec<String>> {
    let mut hits = Vec::new();
    for base in dirs {
        let dir = root.join(base);
        if !dir.exists() {
            continue;
        }
        let walker = walkdir::WalkDir::new(&dir).into_iter().filter_entry(|e| {
            let rel = e.path().strip_prefix(&dir).unwrap_or(e.path());
            !is_skipped(&rel.to_string_lossy().replace('\\', "/"))
        });
        for entry in walker {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue; // binary file
            };
            for (n, line) in text.lines().enumerate() {
                let mut lower = line.to_lowercase();
                for name in THIRD_PARTY_NAMES {
                    lower = lower.replace(name, "");
                }
                for (term, pkg) in terms {
                    if lower.contains(term.as_str()) {
                        let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
                        hits.push(format!(
                            "{}:{}: `{term}` (from package `{pkg}`)",
                            rel.display(),
                            n + 1
                        ));
                    }
                }
            }
        }
    }
    Ok(hits)
}

fn check_domain_leak() -> Result<()> {
    let terms = domain_terms()?;
    if terms.is_empty() {
        println!("no domain terms declared");
        return Ok(());
    }
    let hits = scan_for_terms(&root(), LEAK_SCAN, &terms)?;
    if hits.is_empty() {
        println!(
            "no domain terms found in the core ({} terms checked)",
            terms.len()
        );
        Ok(())
    } else {
        for h in &hits {
            eprintln!("  {h}");
        }
        bail!("{} domain term occurrences found in the core", hits.len());
    }
}

/// Runs every scenario in `packages/<name>/tests/*.yaml` (§15.4).
fn test_package(name: &str) -> Result<()> {
    let pkg = validate(name)?;
    let bundle = core_config::compile(pkg).context("compiling the package")?;
    let dir = package_dir(name)?.join("tests");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "yaml"))
        .collect();
    files.sort();
    let mut failed = 0;
    for f in &files {
        let text = std::fs::read_to_string(f)?;
        let file = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let started = std::time::Instant::now();
        match core_testkit::run_scenario(bundle.bytes.clone(), &text) {
            Ok(title) => println!(
                "  ok    {file}: {title} ({:.1}s)",
                started.elapsed().as_secs_f64()
            ),
            Err(e) => {
                failed += 1;
                println!("  FAIL  {file}: {e}");
            }
        }
    }
    if failed > 0 {
        bail!("{failed} of {} scenarios failed", files.len());
    }
    println!("{} scenarios passed", files.len());
    Ok(())
}

/// Simulates the package's organization (§15.3); fails if the delivery target is missed.
fn simulate(name: &str, devices: u32, days: u32, seed: u64) -> Result<()> {
    let pkg = validate(name)?;
    let bundle = core_config::compile(pkg).context("compiling the package")?;
    let path = package_dir(name)?.join("simulation.yaml");
    let population =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let start = core_engine_date("2026-09-01")?;
    let opts = core_testkit::sim::SimOptions {
        devices: devices as usize,
        days,
        seed,
        start,
    };
    let report =
        core_testkit::sim::run(bundle.bytes, &population, &opts).map_err(|e| anyhow::anyhow!(e))?;
    println!("{report}");
    if !report.meets_target() {
        bail!("delivery target missed");
    }
    Ok(())
}

fn core_engine_date(s: &str) -> Result<time::Date> {
    core_testkit::sim::parse_date(s).ok_or_else(|| anyhow::anyhow!("bad date {s}"))
}

const APP_DIR: &str = "apps/app";
const UI_DIR: &str = "apps/app/ui";
const PNPM: &[&str] = &["-y", "pnpm@10.34.9"];

fn sh(cmd: &mut std::process::Command) -> Result<()> {
    let shown = format!("{cmd:?}");
    let status = cmd.status().with_context(|| format!("starting {shown}"))?;
    if !status.success() {
        bail!("{shown} failed with {status}");
    }
    Ok(())
}

fn pnpm(args: &[&str]) -> std::process::Command {
    let mut c = std::process::Command::new("npx");
    c.args(PNPM)
        .arg("--dir")
        .arg(root().join(UI_DIR))
        .args(args);
    c
}

fn bindings(check: bool) -> Result<()> {
    let committed = root().join(UI_DIR).join("src/lib/bindings.ts");
    let target = if check {
        root().join("target/bindings.ts")
    } else {
        committed.clone()
    };
    sh(std::process::Command::new(env!("CARGO"))
        .current_dir(root())
        .args(["run", "-q", "-p", "app", "--bin", "export-bindings", "--"])
        .arg(&target))?;
    if check {
        let fresh = std::fs::read_to_string(&target)?;
        let current = std::fs::read_to_string(&committed).unwrap_or_default();
        if fresh != current {
            bail!(
                "{} is out of date: run `cargo xtask bindings`",
                committed.display()
            );
        }
        println!("bindings are up to date");
    }
    Ok(())
}

/// Tauri configuration overlay with the package branding (§4.1).
fn branding_overlay(name: &str) -> Result<PathBuf> {
    let pkg = validate(name)?;
    let dir = package_dir(name)?;
    let b = &pkg.manifest.branding;
    let app_name = pkg
        .texts
        .get(&pkg.manifest.default_locale)
        .and_then(|t| t.get(b.app_name.as_str()))
        .cloned()
        .unwrap_or_else(|| b.app_name.as_str().to_owned());
    let out = root().join("target/package").join(package_key(name));
    let icons = out.join("icons");
    std::fs::create_dir_all(&icons)?;
    sh(pnpm(&["exec", "tauri", "icon"])
        .arg(dir.join(&b.icon))
        .arg("-o")
        .arg(&icons))?;
    let icon = |f: &str| icons.join(f).to_string_lossy().into_owned();
    let overlay = serde_json::json!({
        "productName": app_name,
        "identifier": b.identifier,
        "app": { "windows": [{ "title": app_name, "width": 1200, "height": 820, "minWidth": 360, "minHeight": 600 }] },
        "bundle": { "icon": [icon("32x32.png"), icon("128x128.png"), icon("128x128@2x.png"), icon("icon.icns"), icon("icon.ico"), icon("icon.png")] },
    });
    let path = out.join("tauri.overlay.json");
    std::fs::write(&path, serde_json::to_string_pretty(&overlay)?)?;
    Ok(path)
}

fn tauri(name: &str, args: &[&str], server: &ServerArgs) -> Result<()> {
    let bundle = bundle(name)?;
    // The Tauri CLI (used for the icons too) comes from the UI dependencies.
    sh(&mut pnpm(&["install", "--frozen-lockfile"]))?;
    let overlay = branding_overlay(name)?;
    let tauri_bin = root().join(UI_DIR).join("node_modules/.bin/tauri");
    sh(std::process::Command::new(tauri_bin)
        .current_dir(root().join(APP_DIR))
        .args(args)
        .arg("--config")
        .arg(&overlay)
        .env("APP_PACKAGE_BUNDLE", &bundle)
        .envs(server.env()))
}

fn dev(name: &str, server: &ServerArgs) -> Result<()> {
    tauri(name, &["dev"], server)
}

fn build(name: &str, target: &str, release: bool, server: &ServerArgs) -> Result<()> {
    match target {
        "desktop" if release => tauri(name, &["build"], server),
        "desktop" => tauri(name, &["build", "--debug"], server),
        "android" => not_yet("build --target android", "F7"),
        "ios" => not_yet("build --target ios", "F8"),
        other => bail!("unknown target `{other}` (desktop, android, ios)"),
    }
}

fn not_yet(what: &str, phase: &str) -> Result<()> {
    bail!("`{what}` is not available yet: it arrives in phase {phase} (PROGETTO.md §16)")
}

fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Schema { check } => schema(check),
        Command::Validate { package, all } => {
            let names = match (package, all) {
                (Some(p), false) => vec![p],
                (None, true) => all_packages()?,
                _ => bail!("pass either --package <name> or --all"),
            };
            let mut failed = 0;
            for n in &names {
                if let Err(e) = validate(n) {
                    eprintln!("{e}");
                    failed += 1;
                }
            }
            if failed > 0 {
                bail!("{failed} of {} packages failed validation", names.len());
            }
            Ok(())
        }
        Command::Bundle { package } => bundle(&package).map(|_| ()),
        Command::CheckDomainLeak => check_domain_leak(),
        Command::TestPackage { package } => test_package(&package),
        Command::Build {
            package,
            target,
            release,
            server,
        } => build(&package, &target, release, &server),
        Command::Dev { package, server } => dev(&package, &server),
        Command::Bindings { check } => bindings(check),
        Command::Simulate {
            package,
            devices,
            days,
            seed,
        } => simulate(&package, devices, days, seed),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_finds_terms_case_insensitively_and_skips_generated_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("crates/a/src")).unwrap();
        std::fs::create_dir_all(root.join("crates/a/target")).unwrap();
        std::fs::write(
            root.join("crates/a/src/lib.rs"),
            "fn ok() {}\n// a ForbiddenWord here\n",
        )
        .unwrap();
        std::fs::write(root.join("crates/a/target/out.rs"), "forbiddenword").unwrap();
        std::fs::write(root.join("crates/a/src/bin.dat"), [0xff, 0xfe, 0x00]).unwrap();
        let terms = vec![("forbiddenword".to_owned(), "pkg".to_owned())];
        let hits = scan_for_terms(root, &["crates", "apps/app"], &terms).unwrap();
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(hits[0].contains("lib.rs:2"), "{hits:?}");
    }

    #[test]
    fn skip_rules() {
        assert!(is_skipped("target"));
        assert!(is_skipped("x/node_modules/y.js"));
        assert!(is_skipped("ui/static/package/texts.json"));
        assert!(!is_skipped("src/targets.rs"));
    }
}
