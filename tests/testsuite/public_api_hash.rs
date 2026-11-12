//! Tests for `-Zpublic-api-hash`: library dependents are not recompiled when a dependency's
//! public API hash is unchanged.

use crate::prelude::*;
use cargo_test_support::{Project, project, str};

/// The rustc under test must accept `-Zpublic-api-hash` (rust-lang/rust#156940).
fn rustc_supports_public_api_hash() -> bool {
    std::process::Command::new("rustc")
        .args(["-", "--crate-name", "___", "--print=sysroot", "-Zpublic-api-hash"])
        .stdin(std::process::Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success())
}

/// `foo` (bin) -> `b` (lib) -> `a` (lib). `a`'s private function is in a submodule file: any edit
/// of the crate root file changes the public API hash, as the crate root's span covers it.
fn chain() -> Project {
    project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.1.0"
                edition = "2021"

                [dependencies]
                b = { path = "b" }
            "#,
        )
        .file("src/main.rs", "fn main() { println!(\"{}\", b::get()); }")
        .file(
            "b/Cargo.toml",
            r#"
                [package]
                name = "b"
                version = "0.1.0"
                edition = "2021"

                [dependencies]
                a = { path = "../a" }
            "#,
        )
        .file(
            "b/src/lib.rs",
            "pub fn get() -> u32 { a::value() }\nfn unused() {}\n",
        )
        .file(
            "a/Cargo.toml",
            r#"
                [package]
                name = "a"
                version = "0.1.0"
                edition = "2021"
            "#,
        )
        .file("a/src/lib.rs", "mod imp;\npub fn value() -> u32 { imp::private() }\n")
        .file("a/src/imp.rs", "pub(crate) fn private() -> u32 { 1 }\n")
        .build()
}

#[cargo_test(nightly, reason = "-Zpublic-api-hash is unstable")]
fn private_change_only_relinks() {
    if !rustc_supports_public_api_hash() {
        return;
    }
    let p = chain();
    p.cargo("run -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .with_stdout_data(str![[r#"
1

"#]])
        .run();

    p.change_file("a/src/imp.rs", "pub(crate) fn private() -> u32 { 2 }\n");
    // `b` is not recompiled; its cached warning is still shown. `foo` is relinked against the
    // new `a`.
    p.cargo("run -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .with_stdout_data(str![[r#"
2

"#]])
        .with_stderr_data(str![[r#"
[COMPILING] a v0.1.0 ([ROOT]/foo/a)
[WARNING] function `unused` is never used
...
[COMPILING] foo v0.1.0 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `target/debug/foo[EXE]`

"#]])
        .run();

    p.cargo("build -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .with_stderr_data(str![[r#"
[WARNING] function `unused` is never used
...
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    // A new public item changes `a`'s public API hash, so `b` is recompiled.
    p.change_file(
        "a/src/lib.rs",
        "mod imp;\npub fn value() -> u32 { imp::private() }\npub fn new() {}\n",
    );
    p.cargo("run -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .with_stdout_data(str![[r#"
2

"#]])
        .with_stderr_data(str![[r#"
[COMPILING] a v0.1.0 ([ROOT]/foo/a)
[COMPILING] b v0.1.0 ([ROOT]/foo/b)
[WARNING] function `unused` is never used
...
[COMPILING] foo v0.1.0 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `target/debug/foo[EXE]`

"#]])
        .run();
}

#[cargo_test(nightly, reason = "-Zpublic-api-hash is unstable")]
fn private_change_check_stops_at_changed_crate() {
    if !rustc_supports_public_api_hash() {
        return;
    }
    let p = chain();
    p.cargo("check -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .run();

    p.change_file("a/src/imp.rs", "pub(crate) fn private() -> u32 { 2 }\n");
    // Checking links nothing, so neither `b` nor the binary is checked again.
    p.cargo("check -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .with_stderr_data(str![[r#"
[CHECKING] a v0.1.0 ([ROOT]/foo/a)
[WARNING] function `unused` is never used
...
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}
