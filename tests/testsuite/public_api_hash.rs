//! Tests for `-Zpublic-api-hash`: library dependents are not recompiled when a dependency's
//! public API hash and public span hash are unchanged.

use crate::prelude::*;
use cargo_test_support::{Project, project, str};

/// The rustc under test must accept `-Zpublic-api-hash` (rust-lang/rust#156940).
fn rustc_supports_public_api_hash() -> bool {
    std::process::Command::new("rustc")
        .args([
            "-",
            "--crate-name",
            "___",
            "--print=sysroot",
            "-Zpublic-api-hash",
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success())
}

/// `foo` (bin) -> `b` (lib) -> `a` (lib).
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
        .file(
            "a/src/lib.rs",
            "mod imp;\npub fn value() -> u32 { imp::private() }\n",
        )
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

#[cargo_test(nightly, reason = "-Zpublic-api-hash is unstable")]
fn public_const_value_change_rebuilds_dependents() {
    if !rustc_supports_public_api_hash() {
        return;
    }
    let p = project()
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
        .file("b/src/lib.rs", "pub fn get() -> u32 { a::C }\n")
        .file(
            "a/Cargo.toml",
            r#"
                [package]
                name = "a"
                version = "0.1.0"
                edition = "2021"
            "#,
        )
        .file("a/src/lib.rs", "mod m;\nmod p;\npub use m::C;\n")
        .file("a/src/m.rs", "pub const C: u32 = crate::p::V;\n")
        .file("a/src/p.rs", "pub(crate) const V: u32 = 1;\n")
        .build();
    p.cargo("run -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .with_stdout_data(str![[r#"
1

"#]])
        .run();

    // `C`'s value is part of `a`'s public API although it comes from a private const, and `b`
    // compiled it into its own code.
    p.change_file("a/src/p.rs", "pub(crate) const V: u32 = 2;\n");
    p.cargo("run -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .with_stdout_data(str![[r#"
2

"#]])
        .with_stderr_data(str![[r#"
[COMPILING] a v0.1.0 ([ROOT]/foo/a)
[COMPILING] b v0.1.0 ([ROOT]/foo/b)
[COMPILING] foo v0.1.0 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `target/debug/foo[EXE]`

"#]])
        .run();
}

#[cargo_test(nightly, reason = "-Zpublic-api-hash is unstable")]
fn crate_root_private_edit_skips_dependents() {
    if !rustc_supports_public_api_hash() {
        return;
    }
    let p = chain();
    p.change_file(
        "a/src/lib.rs",
        "pub fn value() -> u32 { private() }\nfn private() -> u32 { 1 }\n",
    );
    p.cargo("build -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .run();

    // The private function moves and changes in the crate root file.
    p.change_file(
        "a/src/lib.rs",
        "pub fn value() -> u32 { private() }\n\n\nfn private() -> u32 {\n    2\n}\n",
    );
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
}

#[cargo_test(nightly, reason = "-Zpublic-api-hash is unstable")]
fn line_shift_above_non_generic_fn_skips_dependents() {
    if !rustc_supports_public_api_hash() {
        return;
    }
    let p = chain();
    p.cargo("build -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .run();

    // `value`'s code is not copied into dependents, so its new position does not matter to them.
    p.change_file(
        "a/src/lib.rs",
        "\n\nmod imp;\npub fn value() -> u32 { imp::private() }\n",
    );
    p.cargo("run -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .with_stdout_data(str![[r#"
1

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
}

/// `foo` (bin) runs `b::run()`, which calls `a`'s `boom`, which panics. `b` compiles its own copy
/// of `boom` when `boom` is generic or `#[inline]`, including `boom`'s panic location.
fn panic_chain(a_lib: &str, b_lib: &str) -> Project {
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
        .file("src/main.rs", "fn main() { b::run(); }")
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
        .file("b/src/lib.rs", b_lib)
        .file(
            "a/Cargo.toml",
            r#"
                [package]
                name = "a"
                version = "0.1.0"
                edition = "2021"
            "#,
        )
        .file("a/src/lib.rs", a_lib)
        .build()
}

/// The location `foo`'s panic reports, building into `target_dir`.
fn panic_location(p: &Project, target_dir: &str) -> String {
    let output = p
        .cargo("run -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .env("CARGO_TARGET_DIR", p.root().join(target_dir))
        .with_status(101)
        .with_stderr_contains("[..]panicked at [..]")
        .run();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let (_, panic) = stderr.split_once("panicked at ").expect("a panic message");
    panic
        .lines()
        .next()
        .unwrap()
        .trim_end_matches(':')
        .to_owned()
}

/// Shifts `a`'s code two lines down, checks whether `b` is recompiled, and returns the panic
/// location after checking it is the same as in a clean build.
fn line_shift(a_lib: &str, b_lib: &str, b_recompiled: bool) -> String {
    let p = panic_chain(a_lib, b_lib);
    p.cargo("build -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .run();

    p.change_file("a/src/lib.rs", &format!("\n\n{a_lib}"));
    let expected = if b_recompiled {
        str![[r#"
[COMPILING] a v0.1.0 ([ROOT]/foo/a)
[COMPILING] b v0.1.0 ([ROOT]/foo/b)
[COMPILING] foo v0.1.0 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]]
    } else {
        str![[r#"
[COMPILING] a v0.1.0 ([ROOT]/foo/a)
[COMPILING] foo v0.1.0 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]]
    };
    p.cargo("build -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .with_stderr_data(expected)
        .run();
    let location = panic_location(&p, "target");
    assert_eq!(location, panic_location(&p, "clean-target"));
    location
}

#[cargo_test(nightly, reason = "-Zpublic-api-hash is unstable")]
fn line_shift_above_generic_fn_rebuilds_dependents() {
    if !rustc_supports_public_api_hash() {
        return;
    }
    let location = line_shift(
        "pub fn boom<T>(_: T) {\n    panic!(\"boom\");\n}\n",
        "pub fn run() { a::boom(1u8) }\n",
        true,
    );
    assert_eq!(location, "a/src/lib.rs:4:5");
}

#[cargo_test(nightly, reason = "-Zpublic-api-hash is unstable")]
fn line_shift_above_inline_fn_rebuilds_dependents() {
    if !rustc_supports_public_api_hash() {
        return;
    }
    let location = line_shift(
        "#[inline]\npub fn boom() {\n    panic!(\"boom\");\n}\n",
        "pub fn run() { a::boom() }\n",
        true,
    );
    assert_eq!(location, "a/src/lib.rs:5:5");
}

#[cargo_test(nightly, reason = "-Zpublic-api-hash is unstable")]
fn line_shift_above_track_caller_generic_fn_rebuilds_dependents() {
    if !rustc_supports_public_api_hash() {
        return;
    }
    // `boom` reports its caller's location, and its caller `relay` is generic, so `b`'s copy of
    // `relay` holds a position in `a`.
    let location = line_shift(
        "#[track_caller]\nfn boom() {\n    panic!(\"boom\");\n}\npub fn relay<T>(_: T) {\n    boom();\n}\n",
        "pub fn run() { a::relay(1u8) }\n",
        true,
    );
    assert_eq!(location, "a/src/lib.rs:8:5");
}

#[cargo_test(nightly, reason = "-Zpublic-api-hash is unstable")]
fn line_shift_above_used_macro_rebuilds_dependents() {
    if !rustc_supports_public_api_hash() {
        return;
    }
    line_shift(
        "#[macro_export]\nmacro_rules! boom {\n    () => {\n        panic!(\"boom\")\n    };\n}\n",
        "pub fn run() { a::boom!() }\n",
        true,
    );
}

#[cargo_test(nightly, reason = "-Zpublic-api-hash is unstable")]
fn line_shift_above_unused_generic_fn_skips_dependents() {
    if !rustc_supports_public_api_hash() {
        return;
    }
    // The crate's span hash changes as `unused` moves, but `b` copied nothing from `unused`.
    let location = line_shift(
        "pub fn unused<T>(_: T) {}\npub fn boom() {\n    panic!(\"boom\");\n}\n",
        "pub fn run() { a::boom() }\n",
        false,
    );
    assert_eq!(location, "a/src/lib.rs:5:5");
}

#[cargo_test(nightly, reason = "-Zpublic-api-hash is unstable")]
fn moving_unused_generic_skips_and_moving_used_generic_rebuilds() {
    if !rustc_supports_public_api_hash() {
        return;
    }
    let used = "pub fn used<T>(_: T) {\n    panic!(\"boom\");\n}\n";
    let unused = "pub fn unused<T>(_: T) {}\n";
    let p = panic_chain(
        &format!("{used}{unused}"),
        "pub fn run() { a::used(1u8) }\n",
    );
    p.cargo("build -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .run();

    // Only `unused` moves: rustc reports that `b`'s copied code kept its positions.
    p.change_file("a/src/lib.rs", &format!("{used}\n\n{unused}"));
    p.cargo("build -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .with_stderr_data(str![[r#"
[COMPILING] a v0.1.0 ([ROOT]/foo/a)
[COMPILING] foo v0.1.0 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    assert_eq!(panic_location(&p, "target"), "a/src/lib.rs:2:5");
    assert_eq!(panic_location(&p, "clean-target"), "a/src/lib.rs:2:5");

    // `used` moves: `b`'s copy of it must be recompiled.
    p.change_file("a/src/lib.rs", &format!("\n\n{used}\n\n{unused}"));
    p.cargo("build -Zpublic-api-hash")
        .masquerade_as_nightly_cargo(&["public-api-hash"])
        .with_stderr_data(str![[r#"
[COMPILING] a v0.1.0 ([ROOT]/foo/a)
[COMPILING] b v0.1.0 ([ROOT]/foo/b)
[COMPILING] foo v0.1.0 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    assert_eq!(panic_location(&p, "target"), "a/src/lib.rs:4:5");
    assert_eq!(panic_location(&p, "clean-target"), "a/src/lib.rs:4:5");
}

/// The lines of `stderr` that point into `a`'s source.
fn locations_in_a(stderr: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(stderr)
        .lines()
        .filter(|line| line.contains("a/src/lib.rs:"))
        .map(|line| line.trim().to_owned())
        .collect()
}

#[cargo_test(nightly, reason = "-Zpublic-api-hash is unstable")]
fn replayed_warning_points_at_moved_dependency_line() {
    if !rustc_supports_public_api_hash() {
        return;
    }
    // `b`'s warning has a note pointing at `a::START`, which is not in `a`'s span hash.
    let a_lib = "pub struct Counter {\n    pub count: u32,\n}\n// ABOVE\npub const START: Counter = Counter { count: 0 };\n// BELOW\npub fn below() -> u32 {\n    7\n}\n";
    let p = panic_chain(
        a_lib,
        "pub fn run() {\n    a::START.count = 1;\n    let _ = a::below();\n}\n",
    );
    let build = |p: &Project, target_dir: &str| {
        p.cargo("build -Zpublic-api-hash")
            .masquerade_as_nightly_cargo(&["public-api-hash"])
            .env("CARGO_TARGET_DIR", p.root().join(target_dir))
            .with_stderr_contains("[..]a/src/lib.rs:[..]")
            .run()
    };
    assert_eq!(
        locations_in_a(&build(&p, "target").stderr),
        ["--> a/src/lib.rs:5:1"]
    );

    // Lines below `START` move: `b` is skipped and its replayed warning is still right.
    p.change_file(
        "a/src/lib.rs",
        &a_lib.replace("// BELOW\n", "// BELOW\n\n\n"),
    );
    let output = build(&p, "target");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("[COMPILING] b") && !stderr.contains("Compiling b"));
    assert_eq!(
        locations_in_a(&output.stderr),
        locations_in_a(&build(&p, "clean-1").stderr)
    );

    // `START` moves: `b` is recompiled and its warning points at the new line.
    p.change_file(
        "a/src/lib.rs",
        &a_lib
            .replace("// BELOW\n", "// BELOW\n\n\n")
            .replace("// ABOVE\n", "// ABOVE\n\n\n"),
    );
    let output = build(&p, "target");
    assert!(String::from_utf8_lossy(&output.stderr).contains("Compiling b"));
    assert_eq!(locations_in_a(&output.stderr), ["--> a/src/lib.rs:7:1"]);
    assert_eq!(
        locations_in_a(&output.stderr),
        locations_in_a(&build(&p, "clean-2").stderr)
    );
}

/// The `DW_AT_decl_line` of `Handle` in `foo`'s debuginfo, if `llvm-dwarfdump` is installed.
fn handle_decl_line(p: &Project, target_dir: &str) -> Option<String> {
    let output = std::process::Command::new("llvm-dwarfdump")
        .arg("--name=Handle")
        .arg(p.root().join(target_dir).join("debug/foo"))
        .output()
        .ok()?;
    let dump = String::from_utf8(output.stdout).unwrap();
    let line = dump.lines().find(|line| line.contains("DW_AT_decl_line"));
    Some(line.expect("Handle has a decl_line").trim().to_owned())
}

#[cargo_test(nightly, reason = "-Zpublic-api-hash is unstable")]
fn moved_type_in_debuginfo_rebuilds_dependents() {
    if !rustc_supports_public_api_hash() {
        return;
    }
    // `b`'s debuginfo holds the line of `a::Handle`, which is not in `a`'s span hash.
    let a_lib = "// ABOVE\npub struct Handle {\n    pub id: u64,\n}\n// BELOW\n";
    let p = panic_chain(
        a_lib,
        "#[inline(never)]\npub fn handle(h: a::Handle) -> u64 {\n    h.id\n}\npub fn run() {\n    let _ = handle(a::Handle { id: 1 });\n}\n",
    );
    let build = |p: &Project, target_dir: &str| {
        p.cargo("build -Zpublic-api-hash")
            .masquerade_as_nightly_cargo(&["public-api-hash"])
            .env("RUSTFLAGS", "-Zdebug-info-type-line-numbers")
            .env("CARGO_TARGET_DIR", p.root().join(target_dir))
            .run()
    };
    build(&p, "target");

    // Lines below `Handle` move: `b` is skipped.
    p.change_file("a/src/lib.rs", &format!("{a_lib}\n\n"));
    let stderr = String::from_utf8(build(&p, "target").stderr).unwrap();
    assert!(!stderr.contains("Compiling b"), "{stderr}");
    build(&p, "clean-1");
    assert_eq!(
        handle_decl_line(&p, "target"),
        handle_decl_line(&p, "clean-1")
    );

    // `Handle` moves: `b` is recompiled and its debuginfo has the new line.
    p.change_file("a/src/lib.rs", &format!("\n\n{a_lib}\n\n"));
    let stderr = String::from_utf8(build(&p, "target").stderr).unwrap();
    assert!(stderr.contains("Compiling b"), "{stderr}");
    build(&p, "clean-2");
    assert_eq!(
        handle_decl_line(&p, "target"),
        handle_decl_line(&p, "clean-2")
    );
}
