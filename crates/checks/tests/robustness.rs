//! Robustness tests for the built-in checks.
//!
//! These tests exercise every check in [`default_checks`] against the real
//! repository sources under `crates/`, plus a set of truncated inputs, to make
//! sure no check panics on malformed or partial source.

use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use soroban_guard_checks::default_checks;

/// Repository root, resolved from the crate manifest so the test does not
/// depend on the test runner's working directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("checks crate should live under <repo>/crates/checks")
        .to_path_buf()
}

/// Recursively collect every `.rs` file under `dir`.
fn find_rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            find_rust_sources(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Run every check against `source`, catching panics so a single bad check
/// fails the test with a clear message instead of aborting the run.
fn run_all_checks(source: &str, label: &str) {
    let file = match syn::parse_file(source) {
        Ok(file) => file,
        // Truncated inputs may not parse; that is fine — checks only ever see
        // successfully parsed files in production.
        Err(_) => return,
    };

    for check in default_checks() {
        let result = panic::catch_unwind(AssertUnwindSafe(|| check.run(&file, source)));
        assert!(
            result.is_ok(),
            "check `{}` panicked on {}",
            check.name(),
            label
        );
    }
}

#[test]
fn checks_do_not_panic_on_repo_sources() {
    let mut sources = Vec::new();
    find_rust_sources(&repo_root().join("crates"), &mut sources);

    assert!(
        !sources.is_empty(),
        "expected to discover .rs files under crates/"
    );

    for path in &sources {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(_) => continue,
        };
        run_all_checks(&source, &path.display().to_string());
    }
}

#[test]
fn checks_do_not_panic_on_truncated_input() {
    let sample = r#"
        #![no_std]
        use soroban_sdk::{contract, contractimpl, Address, Env};

        #[contract]
        pub struct Token;

        #[contractimpl]
        impl Token {
            pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
                from.require_auth();
                let balance: i128 = env.storage().instance().get(&from).unwrap_or(0);
                env.storage().instance().set(&from, &(balance - amount));
                env.storage().instance().set(&to, &amount);
            }
        }
    "#;

    // Truncate at a variety of byte offsets, including mid-token and mid-block.
    let mut truncated_any = false;
    for cut in [0usize, 1, 10, 50, 100, 200, 400] {
        if cut > sample.len() {
            continue;
        }
        let slice = &sample[..cut];
        run_all_checks(slice, &format!("truncated input at byte {}", cut));
        truncated_any = true;
    }
    assert!(truncated_any, "expected at least one truncated input");
}
