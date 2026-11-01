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
    /// Full app build for a target (phase F5 onwards).
    Build {
        #[arg(long)]
        package: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        release: bool,
    },
    /// Desktop development run (phase F5).
    Dev {
        #[arg(long)]
        package: String,
    },
    /// Regenerate the TypeScript bindings (phase F5).
    Bindings,
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

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn package_dir(name: &str) -> Result<PathBuf> {
    let dir = root().join("packages").join(name);
    if !dir.join("package.yaml").is_file() {
        bail!("package `{name}` not found in {}", dir.display());
    }
    Ok(dir)
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

fn bundle(name: &str) -> Result<()> {
    let pkg = validate(name)?;
    let bundle = core_config::compile(pkg).context("compiling the package")?;
    let out = root().join("target/package").join(name);
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
    Ok(())
}

/// Directories scanned for domain terms, and parts skipped inside them.
const LEAK_SCAN: &[&str] = &["crates", "apps/app"];
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
                let lower = line.to_lowercase();
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
        Command::Bundle { package } => bundle(&package),
        Command::CheckDomainLeak => check_domain_leak(),
        Command::TestPackage { package } => test_package(&package),
        Command::Build { .. } => not_yet("build", "F5"),
        Command::Dev { .. } => not_yet("dev", "F5"),
        Command::Bindings => not_yet("bindings", "F5"),
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
