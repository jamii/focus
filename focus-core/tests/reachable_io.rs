use std::collections::BTreeSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SNAPSHOT_HEADER: &str = "\
# Ambient I/O reachable from focus_core
#
# Generated from the core fuzz harness with Cackle's link-time reachability
# analysis. Intentional effects are handled by MockIO and therefore don't
# appear here. `random_hash_seed` entries are constructors of std HashMap or
# HashSet; their RandomState initialization reads OS randomness.
#
# Update intentionally with:
#   UPDATE_REACHABLE_IO=1 cargo test -p focus-core --test reachable_io
";

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Finding {
    api: String,
    package: String,
    source: String,
    line: String,
    cause: String,
    target: String,
}

#[test]
fn reachable_ambient_io_matches_snapshot() {
    let core_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = core_dir.parent().unwrap();
    let fixture_dir = core_dir.join("tests/reachable_io/fixture");

    assert_dependency_lock_matches(workspace_dir, &fixture_dir);

    let output = cargo(&fixture_dir)
        .args(["acl", "--path"])
        .arg(&fixture_dir)
        .args(["--no-ui", "--colour", "never"])
        .env(
            "CARGO_TARGET_DIR",
            workspace_dir.join("target/reachable-io-cackle"),
        )
        .output()
        .expect("failed to run `cargo acl`; enter `nix-shell ./shell.nix` first");

    let raw = command_output(&output);
    let findings = parse_findings(&raw, workspace_dir);
    if !output.status.success() && findings.is_empty() {
        panic!(
            "Cackle failed without reporting reachable I/O. \
             Enter `nix-shell ./shell.nix` and retry.\n\n{raw}"
        );
    }

    let actual = format_snapshot(&findings);
    let snapshot_path = core_dir.join("tests/reachable_io.snap");
    if env::var_os("UPDATE_REACHABLE_IO").is_some() {
        std::fs::write(&snapshot_path, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(&snapshot_path).unwrap_or_else(|error| {
        panic!(
            "failed to read {}: {error}; create it with UPDATE_REACHABLE_IO=1",
            snapshot_path.display()
        )
    });
    assert_eq!(expected, actual);
}

fn assert_dependency_lock_matches(workspace_dir: &Path, fixture_dir: &Path) {
    let workspace = dependency_tree(workspace_dir);
    let fixture = dependency_tree(fixture_dir);
    assert_eq!(
        workspace, fixture,
        "the reachability fixture must analyze the versions in the workspace \
         Cargo.lock; update focus-core/tests/reachable_io/fixture/Cargo.lock"
    );
}

fn dependency_tree(manifest_dir: &Path) -> Vec<String> {
    let output = cargo(manifest_dir)
        .args([
            "tree",
            "--locked",
            "--edges",
            "normal",
            "--prefix",
            "none",
            "--format",
            "{p}",
            "--package",
            "focus-core",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", command_output(&output));
    let mut packages: Vec<_> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    packages.sort();
    packages
}

fn cargo(current_dir: &Path) -> Command {
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(current_dir);
    command
}

fn command_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn parse_findings(raw: &str, workspace_dir: &Path) -> BTreeSet<Finding> {
    let mut findings = BTreeSet::new();
    let mut api: Option<String> = None;
    let mut package: Option<String> = None;
    let mut source: Option<String> = None;
    let mut cause: Option<String> = None;

    for line in raw.lines() {
        if let Some((new_package, new_api)) = parse_problem_header(line) {
            package = Some(new_package.to_owned());
            api = Some(new_api.to_owned());
            source = None;
            cause = None;
        } else if line.starts_with("  /") {
            source = Some(normalize_source(line.trim(), workspace_dir));
            cause = None;
        } else if let Some(target_and_location) = line.strip_prefix("      -> ") {
            let Some((target, location)) = target_and_location.rsplit_once(" [") else {
                continue;
            };
            let Some(location) = location.strip_suffix(']') else {
                continue;
            };
            let (Some(api), Some(package), Some(source), Some(cause)) =
                (&api, &package, &source, &cause)
            else {
                continue;
            };
            if api == "random_hash_seed" && !is_random_hash_constructor(target) {
                continue;
            }
            findings.insert(Finding {
                api: api.clone(),
                package: package.clone(),
                source: source.clone(),
                line: location.to_owned(),
                cause: erase_generics(cause),
                target: erase_generics(target),
            });
        } else if let Some(new_cause) = line.strip_prefix("    ") {
            cause = Some(new_cause.to_owned());
        }
    }
    findings
}

fn parse_problem_header(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("ERROR: '")?;
    let (package, rest) = rest.split_once("' uses disallowed API `")?;
    Some((package, rest.strip_suffix('`')?))
}

fn normalize_source(source: &str, workspace_dir: &Path) -> String {
    let path = Path::new(source);
    if let Ok(relative) = path.strip_prefix(workspace_dir) {
        return relative.display().to_string();
    }
    if let Some((_, registry_path)) = source.split_once("/registry/src/")
        && let Some((_, crate_path)) = registry_path.split_once('/')
    {
        return format!("registry/{crate_path}");
    }
    source.to_owned()
}

fn is_random_hash_constructor(target: &str) -> bool {
    let randomized_hash = target.contains("std::collections::hash::map::HashMap")
        || target.contains("std::collections::hash::set::HashSet")
        || target.contains("std::hash::random::RandomState")
        || target.contains("std::hash::RandomState");
    randomized_hash
        && [
            ">::new<",
            ">::with_capacity<",
            ">::default<",
            ">::from<",
            ">::from_iter<",
            "Iterator::collect<",
        ]
        .iter()
        .any(|constructor| target.contains(constructor))
}

fn erase_generics(name: &str) -> String {
    let mut output = String::with_capacity(name.len());
    let mut depth = 0;
    for ch in name.chars() {
        match ch {
            '<' => {
                if depth == 0 {
                    output.push_str("<…>");
                }
                depth += 1;
            }
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => output.push(ch),
            _ => {}
        }
    }
    output
}

fn format_snapshot(findings: &BTreeSet<Finding>) -> String {
    use std::fmt::Write;

    let mut output = SNAPSHOT_HEADER.to_owned();
    let mut previous_api: Option<&str> = None;
    for finding in findings {
        if previous_api != Some(&finding.api) {
            writeln!(output, "\n{}:", finding.api).unwrap();
            previous_api = Some(&finding.api);
        }
        writeln!(
            output,
            "- {} | {}:{}\n  {}\n  -> {}",
            finding.package, finding.source, finding.line, finding.cause, finding.target
        )
        .unwrap();
    }
    output
}

#[test]
fn generic_arguments_are_erased_from_snapshot_symbols() {
    assert_eq!(
        erase_generics("std::HashMap<usize, Vec<Option<u8>>>::new<usize>"),
        "std::HashMap<…>::new<…>"
    );
}

#[test]
fn only_randomized_hash_construction_is_a_seed_cause() {
    assert!(is_random_hash_constructor(
        "std::collections::hash::map::HashMap<u8, u8, std::hash::random::RandomState>::new<u8, u8>"
    ));
    assert!(!is_random_hash_constructor(
        "std::collections::hash::map::HashMap<u8, u8, std::hash::random::RandomState>::get<u8>"
    ));
}

#[test]
fn cackle_output_is_normalized_and_deduplicated() {
    let raw = "\
ERROR: 'focus-core' uses disallowed API `random_hash_seed`
  /work/focus-core/src/fuzz.rs
    focus_core::fuzz::MockIO::new
      -> std::collections::hash::map::HashMap<u8, u8>::new<u8, u8> [76:20]
      -> std::collections::hash::map::HashMap<u8, u8>::new<u8, u8> [76:20]
      -> std::collections::hash::map::HashMap<u8, u8>::get<u8> [77:20]
";
    let findings = parse_findings(raw, Path::new("/work"));
    assert_eq!(findings.len(), 1);
    let finding = findings.first().unwrap();
    assert_eq!(finding.source, "focus-core/src/fuzz.rs");
    assert_eq!(
        finding.target,
        "std::collections::hash::map::HashMap<…>::new<…>"
    );
}
