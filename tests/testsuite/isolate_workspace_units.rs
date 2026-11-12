//! Tests for `-Zisolate-workspace-units` / `build.isolate-workspace-units`.
//!
//! The scenario is two checkouts of one workspace (same layout, different
//! absolute roots) that share a target-dir and a build-dir, as git worktrees do
//! when `CARGO_TARGET_DIR` points at one place.

use crate::prelude::*;
use cargo_test_support::registry::Package;
use cargo_test_support::{Project, paths, project, str};

/// One checkout under `paths::root()/<tree>`. Its workspace member `corelib`
/// returns the tree name, and both trees depend on the same registry crate.
fn checkout(tree: &str) -> Project {
    project()
        .at(tree)
        .file(
            "Cargo.toml",
            r#"
            [workspace]
            resolver = "3"
            members = ["corelib", "app"]
            "#,
        )
        .file(
            "corelib/Cargo.toml",
            r#"
            [package]
            name = "corelib"
            edition = "2024"

            [dependencies]
            dep = "1.0"
            "#,
        )
        .file(
            "corelib/src/lib.rs",
            &format!(r#"pub fn tree() -> String {{ format!("{tree}-{{}}", dep::d()) }}"#),
        )
        .file(
            "app/Cargo.toml",
            r#"
            [package]
            name = "app"
            edition = "2024"

            [dependencies]
            corelib = { path = "../corelib" }
            "#,
        )
        .file(
            "app/src/main.rs",
            r#"fn main() { println!("tree={}", corelib::tree()); }"#,
        )
        .build()
}

fn setup() -> (Project, Project) {
    Package::new("dep", "1.0.0")
        .file("src/lib.rs", "pub fn d() -> u32 { 7 }")
        .publish();
    // Both checkouts exist before the first build, so the second one's sources
    // are not newer than the first one's outputs.
    (checkout("a"), checkout("b"))
}

fn run_shared(p: &Project, args: &str) -> cargo_test_support::Execs {
    let mut e = p.cargo(&format!("run -p app {args}"));
    e.env("CARGO_TARGET_DIR", paths::root().join("shared-target"))
        .env("CARGO_BUILD_BUILD_DIR", paths::root().join("shared-build"));
    e
}

#[cargo_test]
fn shared_dirs_isolate_workspace_units() {
    let (a, b) = setup();
    let z = "-Zisolate-workspace-units";
    let nightly = ["isolate-workspace-units"];

    run_shared(&a, z)
        .masquerade_as_nightly_cargo(&nightly)
        .with_stdout_data(str![[r#"
tree=a-7

"#]])
        .with_stderr_data(
            str![[r#"
[UPDATING] `dummy-registry` index
[LOCKING] 1 package to [..] compatible version
[DOWNLOADING] crates ...
[DOWNLOADED] dep v1.0.0 (registry `dummy-registry`)
[COMPILING] dep v1.0.0
[COMPILING] corelib v0.0.0 ([ROOT]/a/corelib)
[COMPILING] app v0.0.0 ([ROOT]/a/app)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `[ROOT]/shared-target/debug/app[EXE]`

"#]]
            .unordered(),
        )
        .run();

    // The registry crate is reused; only b's own crates are built.
    run_shared(&b, z)
        .masquerade_as_nightly_cargo(&nightly)
        .with_stdout_data(str![[r#"
tree=b-7

"#]])
        .with_stderr_data(str![[r#"
[UPDATING] `dummy-registry` index
[LOCKING] 1 package to [..] compatible version
[COMPILING] corelib v0.0.0 ([ROOT]/b/corelib)
[COMPILING] app v0.0.0 ([ROOT]/b/app)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `[ROOT]/shared-target/debug/app[EXE]`

"#]])
        .run();

    // Switching back and forth rebuilds nothing and runs each tree's own binary.
    for (p, tree) in [(&a, "a"), (&b, "b")] {
        run_shared(p, z)
            .masquerade_as_nightly_cargo(&nightly)
            .with_stdout_data(format!("tree={tree}-7\n"))
            .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `[ROOT]/shared-target/debug/app[EXE]`

"#]])
            .run();
    }

    // An edit in one tree rebuilds that tree only.
    b.change_file(
        "corelib/src/lib.rs",
        r#"pub fn tree() -> String { format!("b2-{}", dep::d()) }"#,
    );
    run_shared(&b, z)
        .masquerade_as_nightly_cargo(&nightly)
        .with_stdout_data(str![[r#"
tree=b2-7

"#]])
        .with_stderr_data(str![[r#"
[COMPILING] corelib v0.0.0 ([ROOT]/b/corelib)
[COMPILING] app v0.0.0 ([ROOT]/b/app)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `[ROOT]/shared-target/debug/app[EXE]`

"#]])
        .run();
    run_shared(&a, z)
        .masquerade_as_nightly_cargo(&nightly)
        .with_stdout_data(str![[r#"
tree=a-7

"#]])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `[ROOT]/shared-target/debug/app[EXE]`

"#]])
        .run();
}

#[cargo_test]
fn shared_dirs_without_isolation_share_workspace_units() {
    let (a, b) = setup();

    run_shared(&a, "")
        .with_stdout_data(str![[r#"
tree=a-7

"#]])
        .run();

    // Without the option the trees map to the same units, and b's sources are
    // older than a's outputs, so b reuses a's binary.
    run_shared(&b, "")
        .with_stdout_data(str![[r#"
tree=a-7

"#]])
        .with_stderr_data(str![[r#"
[UPDATING] `dummy-registry` index
[LOCKING] 1 package to [..] compatible version
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `[ROOT]/shared-target/debug/app[EXE]`

"#]])
        .run();
}

#[cargo_test]
fn isolation_enabled_by_config_or_env() {
    let (a, b) = setup();

    run_shared(&a, "--config build.isolate-workspace-units=true")
        .masquerade_as_nightly_cargo(&["isolate-workspace-units"])
        .with_stdout_data(str![[r#"
tree=a-7

"#]])
        .run();
    run_shared(&b, "")
        .masquerade_as_nightly_cargo(&["isolate-workspace-units"])
        .env("CARGO_ISOLATE_WORKSPACE_UNITS", "1")
        .with_stdout_data(str![[r#"
tree=b-7

"#]])
        .run();
}

#[cargo_test]
fn isolation_config_ignored_on_stable() {
    let (a, _b) = setup();

    run_shared(&a, "")
        .env("CARGO_ISOLATE_WORKSPACE_UNITS", "1")
        .with_stderr_data(str![[r#"
[WARNING] ignoring `build.isolate-workspace-units`, it requires a nightly toolchain
...
"#]])
        .run();
}
