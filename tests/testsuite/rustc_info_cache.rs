//! Tests for the cache file for the rustc version info.

use std::env;

use crate::prelude::*;
use cargo_test_support::basic_bin_manifest;
use cargo_test_support::{basic_manifest, project};

const MISS: &str = "[..] rustc info cache miss[..]";
const HIT: &str = "[..]rustc info cache hit[..]";
const UPDATE: &str = "[..]updated rustc info cache[..]";
const RETRY: &str = "[..]cached output was a failure; retrying[..]";

#[cargo_test]
fn rustc_info_cache() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("hello"); }"#)
        .build();

    p.cargo("build")
        .env("CARGO_LOG", "cargo::util::rustc=debug")
        .with_stderr_contains("[..]failed to read rustc info cache[..]")
        .with_stderr_contains(MISS)
        .with_stderr_does_not_contain(HIT)
        .with_stderr_contains(UPDATE)
        .run();

    p.cargo("build")
        .env("CARGO_LOG", "cargo::util::rustc=debug")
        .with_stderr_contains("[..]reusing existing rustc info cache[..]")
        .with_stderr_contains(HIT)
        .with_stderr_does_not_contain(MISS)
        .with_stderr_does_not_contain(UPDATE)
        .run();

    p.cargo("build")
        .env("CARGO_LOG", "cargo::util::rustc=debug")
        .env("CARGO_CACHE_RUSTC_INFO", "0")
        .with_stderr_contains("[..]rustc info cache disabled[..]")
        .with_stderr_does_not_contain(UPDATE)
        .run();

    let other_rustc = {
        let p = project()
            .at("compiler")
            .file("Cargo.toml", &basic_manifest("compiler", "0.1.0"))
            .file(
                "src/main.rs",
                r#"
                    use std::process::Command;
                    use std::env;

                    fn main() {
                        let mut cmd = Command::new("rustc");
                        for arg in env::args_os().skip(1) {
                            cmd.arg(arg);
                        }
                        std::process::exit(cmd.status().unwrap().code().unwrap());
                    }
                "#,
            )
            .build();
        p.cargo("build").run();

        p.root()
            .join("target/debug/compiler")
            .with_extension(env::consts::EXE_EXTENSION)
    };

    p.cargo("build")
        .env("CARGO_LOG", "cargo::util::rustc=debug")
        .env("RUSTC", other_rustc.display().to_string())
        .with_stderr_contains("[..]different compiler, creating new rustc info cache[..]")
        .with_stderr_contains(MISS)
        .with_stderr_does_not_contain(HIT)
        .with_stderr_contains(UPDATE)
        .run();

    p.cargo("build")
        .env("CARGO_LOG", "cargo::util::rustc=debug")
        .env("RUSTC", other_rustc.display().to_string())
        .with_stderr_contains("[..]reusing existing rustc info cache[..]")
        .with_stderr_contains(HIT)
        .with_stderr_does_not_contain(MISS)
        .with_stderr_does_not_contain(UPDATE)
        .run();

    other_rustc.move_into_the_future();

    p.cargo("build")
        .env("CARGO_LOG", "cargo::util::rustc=debug")
        .env("RUSTC", other_rustc.display().to_string())
        .with_stderr_contains("[..]different compiler, creating new rustc info cache[..]")
        .with_stderr_contains(MISS)
        .with_stderr_does_not_contain(HIT)
        .with_stderr_contains(UPDATE)
        .run();

    p.cargo("build")
        .env("CARGO_LOG", "cargo::util::rustc=debug")
        .env("RUSTC", other_rustc.display().to_string())
        .with_stderr_contains("[..]reusing existing rustc info cache[..]")
        .with_stderr_contains(HIT)
        .with_stderr_does_not_contain(MISS)
        .with_stderr_does_not_contain(UPDATE)
        .run();
}

#[cargo_test]
fn rustc_info_cache_with_wrappers() {
    let wrapper_project = project()
        .at("wrapper")
        .file("Cargo.toml", &basic_bin_manifest("wrapper"))
        .file("src/main.rs", r#"fn main() { }"#)
        .build();
    let wrapper = wrapper_project.bin("wrapper");

    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "test"
                version = "0.0.0"
                authors = []
                [workspace]
            "#,
        )
        .file("src/main.rs", r#"fn main() { println!("hello"); }"#)
        .build();

    for &wrapper_env in ["RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"].iter() {
        p.cargo("clean").with_status(0).run();
        wrapper_project.change_file(
            "src/main.rs",
            r#"
            fn main() {
                let mut args = std::env::args_os();
                let _me = args.next().unwrap();
                let rustc = args.next().unwrap();
                let status = std::process::Command::new(rustc).args(args).status().unwrap();
                std::process::exit(if status.success() { 0 } else { 1 })
            }
            "#,
        );
        wrapper_project.cargo("build").with_status(0).run();

        p.cargo("build")
            .env("CARGO_LOG", "cargo::util::rustc=debug")
            .env(wrapper_env, &wrapper)
            .with_stderr_contains("[..]failed to read rustc info cache[..]")
            .with_stderr_contains(MISS)
            .with_stderr_contains(UPDATE)
            .with_stderr_does_not_contain(HIT)
            .with_status(0)
            .run();
        p.cargo("build")
            .env("CARGO_LOG", "cargo::util::rustc=debug")
            .env(wrapper_env, &wrapper)
            .with_stderr_contains("[..]reusing existing rustc info cache[..]")
            .with_stderr_contains(HIT)
            .with_stderr_does_not_contain(UPDATE)
            .with_stderr_does_not_contain(MISS)
            .with_status(0)
            .run();

        wrapper_project.change_file("src/main.rs", r#"fn main() { panic!() }"#);
        wrapper_project.cargo("build").with_status(0).run();

        p.cargo("build")
            .env("CARGO_LOG", "cargo::util::rustc=debug")
            .env(wrapper_env, &wrapper)
            .with_stderr_contains("[..]different compiler, creating new rustc info cache[..]")
            .with_stderr_contains(MISS)
            .with_stderr_does_not_contain(HIT)
            .with_stderr_does_not_contain(UPDATE)
            .with_status(101)
            .run();
        // Failures are not cached — a subsequent run should retry the command
        // rather than replaying the cached failure.
        p.cargo("build")
            .env("CARGO_LOG", "cargo::util::rustc=debug")
            .env(wrapper_env, &wrapper)
            .with_stderr_contains(MISS)
            .with_stderr_does_not_contain(HIT)
            .with_stderr_does_not_contain(UPDATE)
            .with_status(101)
            .run();
    }
}

#[cargo_test]
fn rustc_info_cache_transient_failure_recovery() {
    let wrapper_project = project()
        .at("wrapper")
        .file("Cargo.toml", &basic_bin_manifest("wrapper"))
        .file("src/main.rs", r#"fn main() { }"#)
        .build();
    let wrapper = wrapper_project.bin("wrapper");

    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "test"
                version = "0.0.0"
                authors = []
                [workspace]
            "#,
        )
        .file("src/main.rs", r#"fn main() { println!("hello"); }"#)
        .build();

    // Build a wrapper that fails (simulates a transient sccache failure).
    wrapper_project.change_file(
        "src/main.rs",
        r#"
        fn main() {
            eprintln!("wrapper: error: Operation not permitted (os error 1)");
            std::process::exit(1);
        }
        "#,
    );
    wrapper_project.cargo("build").with_status(0).run();

    p.cargo("build")
        .env("CARGO_LOG", "cargo::util::rustc=debug")
        .env("RUSTC_WRAPPER", &wrapper)
        .with_stderr_contains("[..]Operation not permitted[..]")
        .with_status(101)
        .run();

    // Now fix the wrapper (simulates sccache recovering).
    wrapper_project.change_file(
        "src/main.rs",
        r#"
        fn main() {
            let mut args = std::env::args_os();
            let _me = args.next().unwrap();
            let rustc = args.next().unwrap();
            let status = std::process::Command::new(rustc).args(args).status().unwrap();
            std::process::exit(if status.success() { 0 } else { 1 })
        }
        "#,
    );
    wrapper_project.cargo("build").with_status(0).run();

    // The build should now succeed — the previous failure must not be cached.
    p.cargo("build")
        .env("CARGO_LOG", "cargo::util::rustc=debug")
        .env("RUSTC_WRAPPER", &wrapper)
        .with_stderr_contains(MISS)
        .with_stderr_does_not_contain(HIT)
        .with_stderr_contains(UPDATE)
        .with_status(0)
        .run();
}

/// Backward-compat: cache files written by older cargo versions may contain
/// failure entries (`"success": false`). Verify that those entries are
/// detected, dropped, and the command is retried rather than replayed.
#[cargo_test]
fn rustc_info_cache_drops_legacy_failure_entries() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("hello"); }"#)
        .build();

    // First build: populates `.rustc_info.json` with successful entries.
    p.cargo("build").with_status(0).run();

    let cache_path = p.root().join("target/.rustc_info.json");
    let raw = std::fs::read_to_string(&cache_path).unwrap();
    let mut cache: serde_json::Value = serde_json::from_str(&raw).unwrap();

    // Simulate a cache file written by older cargo: flip every cached output's
    // `success` field to false and inject a failure-shaped stderr.
    let outputs = cache
        .get_mut("outputs")
        .and_then(|v| v.as_object_mut())
        .expect("cache should have outputs object");
    assert!(
        !outputs.is_empty(),
        "first build should have populated cache outputs"
    );
    for (_key, entry) in outputs.iter_mut() {
        let obj = entry.as_object_mut().unwrap();
        obj.insert("success".to_string(), serde_json::Value::Bool(false));
        obj.insert(
            "status".to_string(),
            serde_json::Value::String("exit status: 1".to_string()),
        );
        obj.insert("code".to_string(), serde_json::json!(1));
        obj.insert(
            "stderr".to_string(),
            serde_json::Value::String("stale wrapper failure".to_string()),
        );
    }
    std::fs::write(&cache_path, serde_json::to_string(&cache).unwrap()).unwrap();

    // Second build: cargo should see the cached failures, drop them, and re-run
    // the rustc probes. The build must succeed and never surface the stale
    // failure stderr.
    p.cargo("build")
        .env("CARGO_LOG", "cargo::util::rustc=debug")
        .with_stderr_contains(RETRY)
        .with_stderr_does_not_contain("[..]stale wrapper failure[..]")
        .with_status(0)
        .run();
}
