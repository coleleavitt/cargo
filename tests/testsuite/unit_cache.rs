//! Tests for the per-user unit cache (`-Zunit-cache`), which shares compiled
//! units of registry and git packages between separate build directories.

use std::path::{Path, PathBuf};

use crate::prelude::*;
use cargo_test_support::registry::Package;
use cargo_test_support::{Project, git, paths, project_in};

fn cache_dir() -> PathBuf {
    paths::home().join(".cargo/unit-cache")
}

/// Cache entries for package `name`.
fn entries(name: &str) -> Vec<PathBuf> {
    let Ok(dir) = std::fs::read_dir(cache_dir().join(name)) else {
        return Vec::new();
    };
    dir.map(|e| e.unwrap().path())
        .filter(|p| !p.file_name().unwrap().to_str().unwrap().starts_with('.'))
        .collect()
}

/// Registry packages: `bar` has a build script that generates code into
/// `OUT_DIR` and adds `OUT_DIR` to the native search path, `pm` is a
/// proc-macro, and `baz` depends on `bar`.
fn publish_deps() {
    Package::new("bar", "1.0.0")
        .feature("f", &[])
        .file(
            "build.rs",
            r#"
            fn main() {
                let out = std::env::var("OUT_DIR").unwrap();
                std::fs::write(
                    format!("{out}/gen.rs"),
                    "pub fn generated() -> u32 { 42 }",
                ).unwrap();
                std::fs::create_dir_all(format!("{out}/native")).unwrap();
                println!("cargo::rustc-link-search=native={out}/native");
                println!("cargo::rerun-if-changed=build.rs");
            }
            "#,
        )
        .file(
            "src/lib.rs",
            r#"include!(concat!(env!("OUT_DIR"), "/gen.rs"));"#,
        )
        .publish();
    Package::new("pm", "1.0.0")
        .proc_macro(true)
        .file(
            "src/lib.rs",
            r#"
            extern crate proc_macro;
            use proc_macro::TokenStream;
            #[proc_macro]
            pub fn seven(_: TokenStream) -> TokenStream { "7".parse().unwrap() }
            "#,
        )
        .publish();
    Package::new("baz", "1.0.0")
        .dep("bar", "1.0")
        .file("src/lib.rs", "pub fn baz() -> u32 { bar::generated() + 1 }")
        .publish();
}

/// A stand-in for one git worktree: its own workspace and own build dir.
fn worktree(name: &str, bar_req: &str) -> Project {
    project_in(name)
        .file(
            "Cargo.toml",
            &format!(
                r#"
                [package]
                name = "foo"
                version = "0.1.0"
                edition = "2021"

                [dependencies]
                bar = {bar_req}
                baz = "1.0"
                pm = "1.0"
                "#
            ),
        )
        .file(
            "src/main.rs",
            r#"
            fn main() {
                println!("{} {} {}", bar::generated(), baz::baz(), pm::seven!());
            }
            "#,
        )
        .build()
}

#[cargo_test]
fn registry_deps_reused_across_build_dirs() {
    publish_deps();
    let a = worktree("wt-a", r#""1.0""#);
    let b = worktree("wt-b", r#""1.0""#);

    a.cargo("run -Zunit-cache")
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .with_stdout_data("42 43 7\n")
        .with_stderr_contains("[COMPILING] bar v1.0.0")
        .run();
    for name in ["bar", "baz", "pm"] {
        assert!(!entries(name).is_empty(), "no cache entry for {name}");
    }

    // The second worktree only compiles its own package. Its build-script
    // output is relocated to its own OUT_DIR.
    b.cargo("run -v -Zunit-cache")
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .with_stdout_data("42 43 7\n")
        .with_stderr_contains("[FRESH] bar v1.0.0")
        .with_stderr_contains("[FRESH] baz v1.0.0")
        .with_stderr_contains("[FRESH] pm v1.0.0")
        .with_stderr_contains("[COMPILING] foo v0.1.0 ([ROOT]/wt-b/foo)")
        .with_stderr_contains("[RUNNING] `rustc --crate-name foo [..]-L native=[ROOT]/wt-b/foo/target/debug/build/bar/[..]/out/native[..]")
        .with_stderr_does_not_contain("[COMPILING] bar[..]")
        .with_stderr_does_not_contain("[COMPILING] baz[..]")
        .with_stderr_does_not_contain("[COMPILING] pm[..]")
        .with_stderr_does_not_contain("[RUNNING] [..]build-script-build[..]")
        .with_stderr_does_not_contain("[..]wt-a[..]")
        .run();

    // A rebuild in the second worktree stays fresh.
    b.cargo("build -Zunit-cache")
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .with_stderr_does_not_contain("[COMPILING][..]")
        .run();
}

#[cargo_test]
fn without_flag_nothing_is_cached() {
    publish_deps();
    let a = worktree("wt-a", r#""1.0""#);
    let b = worktree("wt-b", r#""1.0""#);
    a.cargo("build").run();
    assert!(!cache_dir().exists());
    b.cargo("build")
        .with_stderr_contains("[COMPILING] bar v1.0.0")
        .run();
}

#[cargo_test]
fn env_var_enables_cache() {
    publish_deps();
    let a = worktree("wt-a", r#""1.0""#);
    let b = worktree("wt-b", r#""1.0""#);
    a.cargo("build").env("CARGO_UNIT_CACHE", "1").run();
    b.cargo("build")
        .env("CARGO_UNIT_CACHE", "1")
        .with_stderr_does_not_contain("[COMPILING] bar[..]")
        .with_stderr_contains("[COMPILING] foo v0.1.0 ([ROOT]/wt-b/foo)")
        .run();
}

#[cargo_test]
fn changed_features_or_rustflags_miss() {
    publish_deps();
    let a = worktree("wt-a", r#""1.0""#);
    let b = worktree("wt-b", r#"{ version = "1.0", features = ["f"] }"#);
    a.cargo("build -Zunit-cache")
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .run();

    // `bar` and its dependent `baz` have other features; `pm` is shared.
    b.cargo("build -Zunit-cache")
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .with_stderr_contains("[COMPILING] bar v1.0.0")
        .with_stderr_contains("[COMPILING] baz v1.0.0")
        .with_stderr_does_not_contain("[COMPILING] pm[..]")
        .run();

    let c = worktree("wt-c", r#""1.0""#);
    c.cargo("build -Zunit-cache")
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .env("RUSTFLAGS", "--cfg unit_cache_test")
        .with_stderr_contains("[COMPILING] bar v1.0.0")
        .with_stderr_contains("[COMPILING] pm v1.0.0")
        .run();
}

#[cargo_test]
fn path_deps_are_never_cached() {
    Package::new("reg", "1.0.0").publish();
    let mk = |name: &str, value: u32| {
        project_in(name)
            .file(
                "Cargo.toml",
                r#"
                [package]
                name = "foo"
                version = "0.1.0"
                edition = "2021"

                [dependencies]
                local = { path = "local" }
                reg = "1.0"
                "#,
            )
            .file("src/main.rs", "fn main() { println!(\"{}\", local::v()); }")
            .file(
                "local/Cargo.toml",
                &cargo_test_support::basic_manifest("local", "0.1.0"),
            )
            .file(
                "local/src/lib.rs",
                &format!("pub fn v() -> u32 {{ {value} }}"),
            )
            .build()
    };
    let a = mk("wt-a", 1);
    let b = mk("wt-b", 2);
    a.cargo("run -Zunit-cache")
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .with_stdout_data("1\n")
        .run();
    assert!(!entries("reg").is_empty());
    assert!(entries("local").is_empty());
    assert!(entries("foo").is_empty());

    b.cargo("run -Zunit-cache")
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .with_stdout_data("2\n")
        .with_stderr_contains("[COMPILING] local v0.1.0 ([ROOT]/wt-b/foo/local)")
        .with_stderr_does_not_contain("[COMPILING] reg[..]")
        .run();
}

#[cargo_test]
fn registry_dep_of_patched_path_dep_is_not_cached() {
    // `user` comes from the registry but depends on `bar`, which is patched
    // to a path package, so `user` must not be shared either.
    Package::new("bar", "1.0.0").publish();
    Package::new("user", "1.0.0")
        .dep("bar", "1.0")
        .file("src/lib.rs", "pub fn v() -> u32 { bar::v() }")
        .publish();
    let mk = |name: &str, value: u32| {
        project_in(name)
            .file(
                "Cargo.toml",
                r#"
                [package]
                name = "foo"
                version = "0.1.0"
                edition = "2021"

                [dependencies]
                user = "1.0"

                [patch.crates-io]
                bar = { path = "bar" }
                "#,
            )
            .file("src/main.rs", "fn main() { println!(\"{}\", user::v()); }")
            .file(
                "bar/Cargo.toml",
                &cargo_test_support::basic_manifest("bar", "1.0.0"),
            )
            .file(
                "bar/src/lib.rs",
                &format!("pub fn v() -> u32 {{ {value} }}"),
            )
            .build()
    };
    let a = mk("wt-a", 1);
    let b = mk("wt-b", 2);
    a.cargo("run -Zunit-cache")
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .with_stdout_data("1\n")
        .run();
    assert!(entries("user").is_empty());
    b.cargo("run -Zunit-cache")
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .with_stdout_data("2\n")
        .run();
}

#[cargo_test]
fn dependent_not_restored_over_a_different_dep_entry() {
    // Simulates two worktrees that built `bar` at the same time: the cache
    // holds another build of `bar` than the one `baz` was compiled against.
    publish_deps();
    let a = worktree("wt-a", r#""1.0""#);
    let b = worktree("wt-b", r#""1.0""#);
    a.cargo("build -Zunit-cache")
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .run();
    let bar_lib = entries("bar")
        .into_iter()
        .find(|e| {
            e.join("out").read_dir().is_ok_and(|mut d| {
                d.any(|f| f.unwrap().file_name().to_str().unwrap().ends_with(".rlib"))
            })
        })
        .unwrap();
    std::fs::write(bar_lib.join("unit-cache-id"), "0000000000000000").unwrap();

    b.cargo("build -Zunit-cache")
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .with_stderr_contains("[COMPILING] baz v1.0.0")
        .with_stderr_does_not_contain("[COMPILING] bar[..]")
        .with_stderr_does_not_contain("[COMPILING] pm[..]")
        .run();
}

#[cargo_test]
fn git_deps_reused_across_build_dirs() {
    let dep = git::new("gitdep", |p| {
        p.file(
            "Cargo.toml",
            &cargo_test_support::basic_lib_manifest("gitdep"),
        )
        .file("src/lib.rs", "pub fn v() -> u32 { 5 }")
    });
    let mk = |name: &str| {
        project_in(name)
            .file(
                "Cargo.toml",
                &format!(
                    r#"
                    [package]
                    name = "foo"
                    version = "0.1.0"
                    edition = "2021"

                    [dependencies]
                    gitdep = {{ git = '{}' }}
                    "#,
                    dep.url()
                ),
            )
            .file(
                "src/main.rs",
                "fn main() { println!(\"{}\", gitdep::v()); }",
            )
            .build()
    };
    let a = mk("wt-a");
    let b = mk("wt-b");
    a.cargo("run -Zunit-cache")
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .with_stdout_data("5\n")
        .run();
    b.cargo("run -Zunit-cache")
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .with_stdout_data("5\n")
        .with_stderr_does_not_contain("[COMPILING] gitdep[..]")
        .run();
}

#[cargo_test]
fn custom_cache_dir() {
    publish_deps();
    let a = worktree("wt-a", r#""1.0""#);
    let b = worktree("wt-b", r#""1.0""#);
    let dir = paths::root().join("my-cache");
    let cfg = format!("build.unit-cache-dir='{}'", dir.display());
    a.cargo("build -Zunit-cache --config")
        .arg(&cfg)
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .run();
    assert!(Path::new(&dir).join("bar").exists());
    assert!(!cache_dir().exists());
    b.cargo("build -Zunit-cache --config")
        .arg(&cfg)
        .masquerade_as_nightly_cargo(&["unit-cache"])
        .with_stderr_does_not_contain("[COMPILING] bar[..]")
        .run();
}
