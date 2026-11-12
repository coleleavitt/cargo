//! Tests for `build.build-dir` config property.
//!
//! The testing strategy for build-dir functionality is primarily checking if directories / files
//! are in the expected locations.
//! The rational is that other tests will verify each individual feature, while the tests in this
//! file verify the files saved to disk are in the correct locations according to the `build-dir`
//! configuration.
//!
//! Tests check if directories match some "layout" by using [`CargoPathExt::assert_build_dir_layout`]
//!
//! Note: This file is mirrored by build_dir.rs and build_dir_legacy.rs which contain the same tests with
//! no unstable feature and the old build-dir layout respectively. New tests should be added to all when applicable.

use std::path::PathBuf;

use crate::prelude::*;
use cargo_test_support::registry::{Package, RegistryBuilder};
use cargo_test_support::{Execs, Project, paths, prelude::*, project, str};
use std::env::consts::{DLL_PREFIX, DLL_SUFFIX, EXE_SUFFIX};

#[cargo_test]
fn binary_with_debug() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .build();

    p.cargo("build")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    assert_not_exists(&p.root().join("target"));

    p.root().join("build-dir").assert_build_dir_layout(str![[r#"
[ROOT]/foo/build-dir/.rustc_info.json
[ROOT]/foo/build-dir/CACHEDIR.TAG
[ROOT]/foo/build-dir/debug/.cargo-build-lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..].d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock

"#]]);

    p.root()
        .join("target-dir")
        .assert_build_dir_layout(str![[r#"
[ROOT]/foo/target-dir/CACHEDIR.TAG
[ROOT]/foo/target-dir/debug/.cargo-lock
[ROOT]/foo/target-dir/debug/.cargo-artifact-lock
[ROOT]/foo/target-dir/debug/foo[EXE]
[ROOT]/foo/target-dir/debug/foo.d

"#]]);
}

#[cargo_test]
fn binary_with_release() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .build();

    p.cargo("build --release")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    assert_exists_patterns_with_base_dir(
        &p.root(),
        &[
            // Check the pre-uplifted binary in the build-dir
            &format!("build-dir/release/build/foo/*/out/foo*{EXE_SUFFIX}"),
            "build-dir/release/build/foo/*/out/foo*.d",
            // Verify the binary was copied to the target-dir
            &format!("target-dir/release/foo{EXE_SUFFIX}"),
            "target-dir/release/foo.d",
        ],
    );
    p.root().join("build-dir").assert_build_dir_layout(str![[r#"
[ROOT]/foo/build-dir/.rustc_info.json
[ROOT]/foo/build-dir/CACHEDIR.TAG
[ROOT]/foo/build-dir/release/.cargo-build-lock
[ROOT]/foo/build-dir/release/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/foo/build-dir/release/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/foo/build-dir/release/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/foo/build-dir/release/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/release/build/foo/[HASH]/fingerprint/output-bin-foo
[ROOT]/foo/build-dir/release/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/build-dir/release/build/foo/[HASH]/out/foo[..].d
[ROOT]/foo/build-dir/release/build/foo/[HASH]/.lock

"#]]);

    p.root()
        .join("target-dir")
        .assert_build_dir_layout(str![[r#"
[ROOT]/foo/target-dir/CACHEDIR.TAG
[ROOT]/foo/target-dir/release/.cargo-lock
[ROOT]/foo/target-dir/release/.cargo-artifact-lock
[ROOT]/foo/target-dir/release/foo[EXE]
[ROOT]/foo/target-dir/release/foo.d

"#]]);
}

#[cargo_test]
fn libs() {
    // https://doc.rust-lang.org/reference/linkage.html#r-link.staticlib
    let (staticlib_prefix, staticlib_suffix) =
        if cfg!(target_os = "windows") && cfg!(target_env = "msvc") {
            ("", ".lib")
        } else {
            ("lib", ".a")
        };

    // (crate-type, list of final artifacts)
    let lib_types = [
        ("lib", ["libfoo.rlib", "libfoo.d"]),
        (
            "dylib",
            [
                &format!("{DLL_PREFIX}foo{DLL_SUFFIX}"),
                &format!("{DLL_PREFIX}foo.d"),
            ],
        ),
        (
            "cdylib",
            [
                &format!("{DLL_PREFIX}foo{DLL_SUFFIX}"),
                &format!("{DLL_PREFIX}foo.d"),
            ],
        ),
        (
            "staticlib",
            [
                &format!("{staticlib_prefix}foo{staticlib_suffix}"),
                &format!("{staticlib_prefix}foo.d"),
            ],
        ),
    ];

    for (lib_type, expected_files) in lib_types {
        let p = project()
            .file("src/lib.rs", r#"fn foo() { println!("Hello, World!") }"#)
            .file(
                "Cargo.toml",
                &format!(
                    r#"
            [package]
            name = "foo"
            version = "0.0.1"
            authors = []
            edition = "2015"

            [lib]
            crate-type = ["{lib_type}"]
            "#
                ),
            )
            .file(
                ".cargo/config.toml",
                r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
            )
            .build();

        p.cargo("build")
            .arg("-Zfine-grain-locking")
            .masquerade_as_nightly_cargo(&["fine grain locking"])
            .enable_mac_dsym()
            .run();

        // Verify lib artifacts were copied into the artifact dir
        assert_exists_patterns_with_base_dir(&p.root().join("target-dir/debug"), &expected_files);
    }
}

#[cargo_test]
fn should_default_to_target() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .build();

    p.cargo("build")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    p.root().join("target").assert_build_dir_layout(str![[r#"
[ROOT]/foo/target/.rustc_info.json
[ROOT]/foo/target/CACHEDIR.TAG
[ROOT]/foo/target/debug/.cargo-lock
[ROOT]/foo/target/debug/.cargo-artifact-lock
[ROOT]/foo/target/debug/.cargo-build-lock
[ROOT]/foo/target/debug/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/foo/target/debug/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/foo/target/debug/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/foo/target/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/target/debug/build/foo/[HASH]/fingerprint/output-bin-foo
[ROOT]/foo/target/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/target/debug/build/foo/[HASH]/out/foo[..].d
[ROOT]/foo/target/debug/foo[EXE]
[ROOT]/foo/target/debug/foo.d
[ROOT]/foo/target/debug/build/foo/[HASH]/.lock

"#]]);
}

#[cargo_test]
fn should_respect_env_var() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .build();

    p.cargo("build")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .env("CARGO_BUILD_BUILD_DIR", "build-dir")
        .enable_mac_dsym()
        .run();

    p.root().join("build-dir").assert_build_dir_layout(str![[r#"
[ROOT]/foo/build-dir/.rustc_info.json
[ROOT]/foo/build-dir/CACHEDIR.TAG
[ROOT]/foo/build-dir/debug/.cargo-build-lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..].d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-bin-foo

"#]]);
}

#[cargo_test]
fn build_script_should_output_to_build_dir() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file(
            "build.rs",
            r#"
            fn main() {
                std::fs::write(
                    format!("{}/foo.txt", std::env::var("OUT_DIR").unwrap()),
                    "Hello, world!",
                )
                .unwrap();
            }
            "#,
        )
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .build();

    p.cargo("build")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    p.root().join("build-dir").assert_build_dir_layout(str![[r#"
[ROOT]/foo/build-dir/CACHEDIR.TAG
[ROOT]/foo/build-dir/debug/.cargo-build-lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo.txt
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/build_script_build.d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/build_script_build[EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..].d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/foo/build-dir/.rustc_info.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/run-build-script-build-script-build
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/run-build-script-build-script-build.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-build-script-build-script-build
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/build-script-build-script-build
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/build-script-build-script-build.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/run/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/run/stdout
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/run/root-output
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/run/stderr
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-build-script-build-script-build

"#]]);
}

#[cargo_test]
fn cargo_tmpdir_should_output_to_build_dir() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file(
            "tests/foo.rs",
            r#"
            #[test]
            fn test() {
                std::fs::write(
                    format!("{}/foo.txt", env!("CARGO_TARGET_TMPDIR")),
                    "Hello, world!",
                )
                .unwrap();
            }
            "#,
        )
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .build();

    p.cargo("test")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    p.root().join("build-dir").assert_build_dir_layout(str![[r#"
[ROOT]/foo/build-dir/CACHEDIR.TAG
[ROOT]/foo/build-dir/debug/.cargo-build-lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo-[HASH].d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo.d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..].d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo-[HASH][EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-test-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/test-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/test-bin-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-test-integration-test-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/test-integration-test-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/test-integration-test-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock
[ROOT]/foo/build-dir/tmp/foo.txt
[ROOT]/foo/build-dir/.rustc_info.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-test-integration-test-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-test-bin-foo

"#]]);

    p.root()
        .join("target-dir")
        .assert_build_dir_layout(str![[r#"
[ROOT]/foo/target-dir/CACHEDIR.TAG
[ROOT]/foo/target-dir/debug/.cargo-lock
[ROOT]/foo/target-dir/debug/.cargo-artifact-lock
[ROOT]/foo/target-dir/debug/foo[EXE]

"#]]);
}

#[cargo_test]
fn examples_should_output_to_build_dir_and_uplift_to_target_dir() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file("examples/foo.rs", r#"fn main() { }"#)
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .build();

    p.cargo("build --examples")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    p.root().join("build-dir").assert_build_dir_layout(str![[r#"
[ROOT]/foo/build-dir/.rustc_info.json
[ROOT]/foo/build-dir/CACHEDIR.TAG
[ROOT]/foo/build-dir/debug/.cargo-build-lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-example-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/example-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/example-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-example-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..].d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock

"#]]);

    assert!(!p.root().join("build-dir/debug/examples").exists());

    p.root()
        .join("target-dir")
        .assert_build_dir_layout(str![[r#"
[ROOT]/foo/target-dir/CACHEDIR.TAG
[ROOT]/foo/target-dir/debug/.cargo-lock
[ROOT]/foo/target-dir/debug/.cargo-artifact-lock
[ROOT]/foo/target-dir/debug/examples/foo[EXE]
[ROOT]/foo/target-dir/debug/examples/foo.d

"#]]);
}

#[cargo_test]
fn benches_should_output_to_build_dir() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file("benches/foo.rs", r#"fn main() { }"#)
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .build();

    p.cargo("build --bench=foo")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    p.root().join("build-dir").assert_build_dir_layout(str![[r#"
[ROOT]/foo/build-dir/CACHEDIR.TAG
[ROOT]/foo/build-dir/debug/.cargo-build-lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo-[HASH].d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..].d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo-[HASH][EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-test-bench-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/test-bench-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/test-bench-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock
[ROOT]/foo/build-dir/.rustc_info.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-test-bench-foo

"#]]);

    p.root()
        .join("target-dir")
        .assert_build_dir_layout(str![[r#"
[ROOT]/foo/target-dir/CACHEDIR.TAG
[ROOT]/foo/target-dir/debug/.cargo-lock
[ROOT]/foo/target-dir/debug/.cargo-artifact-lock
[ROOT]/foo/target-dir/debug/foo[EXE]

"#]]);
}

#[cargo_test]
fn cargo_doc_should_output_to_target_dir() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .build();

    p.cargo("doc")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    let docs_dir = p.root().join("target-dir/doc");

    assert_exists(&docs_dir);
    assert_exists(&docs_dir.join("foo/index.html"));
}

#[cargo_test(nightly, reason = "--output-format is unstable")]
fn cargo_rustdoc_json_should_output_to_target_dir() {
    let p = project()
        .file("src/lib.rs", "")
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .build();

    p.cargo("rustdoc --output-format json")
        .arg("-Zfine-grain-locking")
        .arg("-Zunstable-options")
        .masquerade_as_nightly_cargo(&["fine grain locking", "rustdoc-output-format"])
        .enable_mac_dsym()
        .run();

    let docs_dir = p.root().join("target-dir/doc");

    assert_exists(&docs_dir);
    assert_exists(&docs_dir.join("foo.json"));

    p.root().join("build-dir").assert_build_dir_layout(str![
        r#"
[ROOT]/foo/build-dir/.rustc_info.json
[ROOT]/foo/build-dir/.rustdoc_fingerprint.json
[ROOT]/foo/build-dir/CACHEDIR.TAG
[ROOT]/foo/build-dir/debug/.cargo-build-lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/doc-lib-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/doc-lib-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock

"#
    ]);
}

#[cargo_test]
fn cargo_package_should_build_in_build_dir_and_output_to_target_dir() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .build();

    p.cargo("package")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    let package_artifact_dir = p.root().join("target-dir/package");
    assert_exists(&package_artifact_dir);
    assert_exists(&package_artifact_dir.join("foo-0.0.1.crate"));
    assert!(package_artifact_dir.join("foo-0.0.1.crate").is_file());
    // FIXME: The `.cargo-lock` file should be in target-dir not build-dir. See #16707
    p.root().join("build-dir").assert_build_dir_layout(str![[r#"
[ROOT]/foo/build-dir/.rustc_info.json
[ROOT]/foo/build-dir/debug/.cargo-lock
[ROOT]/foo/build-dir/debug/.cargo-artifact-lock
[ROOT]/foo/build-dir/debug/.cargo-build-lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..].d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock
[ROOT]/foo/build-dir/debug/foo[EXE]
[ROOT]/foo/build-dir/debug/foo.d
[ROOT]/foo/build-dir/package/foo-0.0.1/Cargo.lock
[ROOT]/foo/build-dir/package/foo-0.0.1/Cargo.toml
[ROOT]/foo/build-dir/package/foo-0.0.1/Cargo.toml.orig
[ROOT]/foo/build-dir/package/foo-0.0.1/src/main.rs
[ROOT]/foo/build-dir/CACHEDIR.TAG
[ROOT]/foo/build-dir/package/tmp-crate/foo-0.0.1.crate

"#]]);

    p.root()
        .join("target-dir")
        .assert_build_dir_layout(str![[r#"
[ROOT]/foo/target-dir/package/foo-0.0.1.crate
[ROOT]/foo/target-dir/CACHEDIR.TAG

"#]]);
}

#[cargo_test]
fn cargo_publish_should_only_touch_build_dir() {
    let registry = RegistryBuilder::new().http_api().http_index().build();

    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .build();

    p.cargo("publish")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .replace_crates_io(registry.index_url())
        .enable_mac_dsym()
        .run();

    let package_artifact_dir = p.root().join("target-dir/package");
    assert!(!package_artifact_dir.exists());

    let package_build_dir = p.root().join("build-dir/package");
    assert_exists(&package_build_dir);
    assert_exists(&package_build_dir.join("foo-0.0.1"));
    assert!(package_build_dir.join("foo-0.0.1").is_dir());
}

#[cargo_test]
fn cargo_clean_should_clean_the_target_dir_and_build_dir() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .build();

    p.cargo("build")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    p.root().join("build-dir").assert_build_dir_layout(str![[r#"
[ROOT]/foo/build-dir/.rustc_info.json
[ROOT]/foo/build-dir/CACHEDIR.TAG
[ROOT]/foo/build-dir/debug/.cargo-build-lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..].d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock

"#]]);

    p.root()
        .join("target-dir")
        .assert_build_dir_layout(str![[r#"
[ROOT]/foo/target-dir/CACHEDIR.TAG
[ROOT]/foo/target-dir/debug/.cargo-lock
[ROOT]/foo/target-dir/debug/.cargo-artifact-lock
[ROOT]/foo/target-dir/debug/foo[EXE]
[ROOT]/foo/target-dir/debug/foo.d

"#]]);

    p.cargo("clean")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    assert_not_exists(&p.root().join("build-dir"));
    assert_not_exists(&p.root().join("target-dir"));
}

#[cargo_test]
fn cargo_clean_should_remove_correct_files() {
    Package::new("bar", "0.1.0").publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.1"
                edition = "2015"
                authors = []
                exclude = ["*.txt"]
                license = "MIT"
                description = "foo"

                [dependencies]
                bar = "0.1"
            "#,
        )
        .file("src/main.rs", r#"fn main() { println!("Hello, World!"); }"#)
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .build();

    p.cargo("build")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    p.root().join("build-dir").assert_build_dir_layout(str![[r#"
[ROOT]/foo/build-dir/.rustc_info.json
[ROOT]/foo/build-dir/CACHEDIR.TAG
[ROOT]/foo/build-dir/debug/.cargo-build-lock
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/out/bar-[HASH].d
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/out/libbar-[HASH].rlib
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/out/libbar-[HASH].rmeta
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/fingerprint/dep-lib-bar
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/fingerprint/lib-bar
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/fingerprint/lib-bar.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..].d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/fingerprint/output-lib-bar
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-bin-foo
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/.lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock

"#]]);

    p.cargo("clean -p bar")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    p.root().join("build-dir").assert_build_dir_layout(str![[r#"
[ROOT]/foo/build-dir/.rustc_info.json
[ROOT]/foo/build-dir/CACHEDIR.TAG
[ROOT]/foo/build-dir/debug/.cargo-build-lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..].d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-bin-foo

"#]]);
}

#[cargo_test]
fn timings_report_should_output_to_target_dir() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .build();

    p.cargo("build --timings")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    assert_exists(&p.root().join("target-dir/cargo-timings/cargo-timing.html"));
}

#[cargo_test(
    nightly,
    reason = "-Zfuture-incompat-test requires nightly (permanently)"
)]
fn future_incompat_should_output_to_build_dir() {
    let p = project()
        .file("src/main.rs", r#"fn main() { let x = 1; }"#)
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .build();

    p.cargo("build")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .arg("--future-incompat-report")
        .env("RUSTFLAGS", "-Zfuture-incompat-test")
        .run();

    assert_exists(&p.root().join("build-dir/.future-incompat-report.json"));
}

#[cargo_test]
fn template_should_error_for_invalid_variables() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            build-dir = "{fake}/build-dir"
            target-dir = "target-dir"
            "#,
        )
        .build();

    p.cargo("build")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .with_status(101)
        .with_stderr_data(str![[r#"
[ERROR] unexpected variable `fake` in build.build-dir path `{fake}/build-dir`

[HELP] available template variables are `{workspace-root}`, `{cargo-cache-home}`, `{workspace-path-hash}`

"#]])
        .run();
}

#[cargo_test]
fn template_should_suggest_nearest_variable() {
    let p = project()
        .file("src/lib.rs", "")
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            build-dir = "{workspace-ro}/build-dir"
            "#,
        )
        .build();

    p.cargo("build")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .with_status(101)
        .with_stderr_data(str![[r#"
[ERROR] unexpected variable `workspace-ro` in build.build-dir path `{workspace-ro}/build-dir`

[HELP] a template variable with a similar name exists: `workspace-root`

"#]])
        .run();
}

#[cargo_test]
fn template_workspace_root() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            build-dir = "{workspace-root}/build-dir"
            target-dir = "target-dir"
            "#,
        )
        .build();

    p.cargo("build")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    // Verify the binary was uplifted to the target-dir
    assert_exists(&p.root().join(&format!("target-dir/debug/foo{EXE_SUFFIX}")));
    p.root().join("build-dir").assert_build_dir_layout(str![[r#"
[ROOT]/foo/build-dir/.rustc_info.json
[ROOT]/foo/build-dir/CACHEDIR.TAG
[ROOT]/foo/build-dir/debug/.cargo-build-lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..].d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock

"#]]);

    p.root()
        .join("target-dir")
        .assert_build_dir_layout(str![[r#"
[ROOT]/foo/target-dir/CACHEDIR.TAG
[ROOT]/foo/target-dir/debug/.cargo-lock
[ROOT]/foo/target-dir/debug/.cargo-artifact-lock
[ROOT]/foo/target-dir/debug/foo[EXE]
[ROOT]/foo/target-dir/debug/foo.d

"#]]);
}

#[cargo_test]
fn template_cargo_cache_home() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            build-dir = "{cargo-cache-home}/build-dir"
            target-dir = "target-dir"
            "#,
        )
        .build();

    p.cargo("build")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    // Verify the binary was uplifted to the target-dir
    assert_exists(&p.root().join(&format!("target-dir/debug/foo{EXE_SUFFIX}")));
    paths::cargo_home()
        .join("build-dir")
        .assert_build_dir_layout(str![[r#"
[ROOT]/home/.cargo/build-dir/.rustc_info.json
[ROOT]/home/.cargo/build-dir/CACHEDIR.TAG
[ROOT]/home/.cargo/build-dir/debug/.cargo-build-lock
[ROOT]/home/.cargo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/home/.cargo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/home/.cargo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/home/.cargo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/home/.cargo/build-dir/debug/build/foo/[HASH]/fingerprint/output-bin-foo
[ROOT]/home/.cargo/build-dir/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/home/.cargo/build-dir/debug/build/foo/[HASH]/out/foo[..].d
[ROOT]/home/.cargo/build-dir/debug/build/foo/[HASH]/.lock

"#]]);

    p.root()
        .join("target-dir")
        .assert_build_dir_layout(str![[r#"
[ROOT]/foo/target-dir/CACHEDIR.TAG
[ROOT]/foo/target-dir/debug/.cargo-lock
[ROOT]/foo/target-dir/debug/.cargo-artifact-lock
[ROOT]/foo/target-dir/debug/foo[EXE]
[ROOT]/foo/target-dir/debug/foo.d

"#]]);
}

#[cargo_test]
fn template_workspace_path_hash() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file(
            "Cargo.toml",
            r#"
            [package]
            name = "foo"
            version = "1.0.0"
            authors = []
            edition = "2015"
            "#,
        )
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            build-dir = "foo/{workspace-path-hash}/build-dir"
            target-dir = "target-dir"
            "#,
        )
        .build();

    p.cargo("build")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    let foo_dir = p.root().join("foo");
    assert_exists(&foo_dir);
    let hash_dir = parse_workspace_manifest_path_hash(&foo_dir);

    let build_dir = hash_dir.as_path().join("build-dir");

    // Verify the binary was uplifted to the target-dir
    assert_exists(&p.root().join(&format!("target-dir/debug/foo{EXE_SUFFIX}")));
    build_dir.assert_build_dir_layout(str![[r#"
[ROOT]/foo/foo/[HASH]/build-dir/.rustc_info.json
[ROOT]/foo/foo/[HASH]/build-dir/CACHEDIR.TAG
[ROOT]/foo/foo/[HASH]/build-dir/debug/.cargo-build-lock
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/fingerprint/output-bin-foo
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/out/foo[..].d
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/.lock

"#]]);

    p.root()
        .join("target-dir")
        .assert_build_dir_layout(str![[r#"
[ROOT]/foo/target-dir/CACHEDIR.TAG
[ROOT]/foo/target-dir/debug/.cargo-lock
[ROOT]/foo/target-dir/debug/.cargo-artifact-lock
[ROOT]/foo/target-dir/debug/foo[EXE]
[ROOT]/foo/target-dir/debug/foo.d

"#]]);
}

/// Verify that the {workspace-path-hash} does not changes if cargo is run from inside of
/// a symlinked directory.
/// The test approach is to build a project twice from the non-symlinked directory and a symlinked
/// directory and then compare the build-dir paths.
#[cargo_test]
fn template_workspace_path_hash_should_handle_symlink() {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    #[cfg(windows)]
    use std::os::windows::fs::symlink_dir as symlink;

    let p = project()
        .file("src/lib.rs", "")
        .file(
            "Cargo.toml",
            r#"
             [package]
             name = "foo"
             version = "1.0.0"
             authors = []
             edition = "2015"
             "#,
        )
        .file(
            ".cargo/config.toml",
            r#"
             [build]
             build-dir = "foo/{workspace-path-hash}/build-dir"
             "#,
        )
        .build();

    // Build from the non-symlinked directory
    p.cargo("check")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    // Parse and verify the hash dir created from the non-symlinked dir
    let foo_dir = p.root().join("foo");
    assert_exists(&foo_dir);
    let original_hash_dir = parse_workspace_manifest_path_hash(&foo_dir);

    original_hash_dir.assert_build_dir_layout(str![[r#"
[ROOT]/foo/foo/[HASH]/build-dir/.rustc_info.json
[ROOT]/foo/foo/[HASH]/build-dir/CACHEDIR.TAG
[ROOT]/foo/foo/[HASH]/build-dir/debug/.cargo-build-lock
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/fingerprint/dep-lib-foo
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/fingerprint/lib-foo
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/fingerprint/lib-foo.json
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/fingerprint/output-lib-foo
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/out/foo-[HASH].d
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/out/libfoo-[HASH].rmeta
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/.lock

"#]]);

    // NOTE: We do not have a `.cargo-artifact-lock` but we do take a shared `.cargo-lock`
    p.root().join("target").assert_build_dir_layout(str![[r#"
[ROOT]/foo/target/CACHEDIR.TAG
[ROOT]/foo/target/debug/.cargo-lock

"#]]);

    // Create a symlink of the project root.
    let mut symlinked_dir = p.root().clone();
    symlinked_dir.pop();
    symlinked_dir = symlinked_dir.join("symlink-dir");
    symlink(p.root(), &symlinked_dir).unwrap();

    // Remove the foo dir (which contains the build-dir) before we rebuild from a symlinked dir.
    foo_dir.rm_rf();

    // Run cargo from the symlinked dir
    p.cargo("check")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .cwd(&symlinked_dir)
        .run();

    // Parse and verify the hash created from the symlinked dir
    assert_exists(&foo_dir);
    let symlink_hash_dir = parse_workspace_manifest_path_hash(&foo_dir);

    symlink_hash_dir.assert_build_dir_layout(str![[r#"
[ROOT]/foo/foo/[HASH]/build-dir/.rustc_info.json
[ROOT]/foo/foo/[HASH]/build-dir/CACHEDIR.TAG
[ROOT]/foo/foo/[HASH]/build-dir/debug/.cargo-build-lock
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/fingerprint/dep-lib-foo
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/fingerprint/lib-foo
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/fingerprint/lib-foo.json
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/fingerprint/output-lib-foo
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/out/foo-[HASH].d
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/out/libfoo-[HASH].rmeta
[ROOT]/foo/foo/[HASH]/build-dir/debug/build/foo/[HASH]/.lock

"#]]);

    // NOTE: We do not have a `.cargo-artifact-lock` but we do take a shared `.cargo-lock`
    p.root().join("target").assert_build_dir_layout(str![[r#"
[ROOT]/foo/target/CACHEDIR.TAG
[ROOT]/foo/target/debug/.cargo-lock

"#]]);

    // Verify the hash dir created from the symlinked and non-symlinked dirs are the same.
    assert_eq!(original_hash_dir, symlink_hash_dir);
}

#[cargo_test]
fn template_should_handle_reject_unmatched_brackets() {
    let p = project()
        .file("src/lib.rs", "")
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            build-dir = "foo/{bar"
            "#,
        )
        .build();

    p.cargo("build")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .with_status(101)
        .with_stderr_data(str![[r#"
[ERROR] unexpected opening bracket `{` in build.build-dir path `foo/{bar`

"#]])
        .run();

    let p = project()
        .file("src/lib.rs", "")
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            build-dir = "foo/}bar"
            "#,
        )
        .build();

    p.cargo("build")
        .with_status(101)
        .with_stderr_data(str![[r#"
[ERROR] unexpected closing bracket `}` in build.build-dir path `foo/}bar`

"#]])
        .run();
}

#[cargo_test]
fn artifact_deps() {
    let p = project()
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.0"
                edition = "2015"
                authors = []
                resolver = "2"

                [dependencies]
                bar = { path = "bar/", artifact = ["bin:bar"] }
            "#,
        )
        .file(
            "src/main.rs",
            r#"
            fn main() {
                 println!("CARGO_BIN_DIR_BAR={}", env!("CARGO_BIN_DIR_BAR"));
                 println!("CARGO_BIN_FILE_BAR_bar={}", env!("CARGO_BIN_FILE_BAR_bar"));
            }
            "#,
        )
        .file(
            "bar/Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.1.0"
                edition = "2015"
                authors = []

                [[bin]]
                name = "bar"
            "#,
        )
        .file("bar/src/main.rs", "fn main() {}")
        .build();

    p.cargo("run")
        .arg("-Zbindeps")
        .masquerade_as_nightly_cargo(&["bindeps", "build-dir-new-layout"])
        .enable_mac_dsym()
        .with_stderr_data(str![[r#"
[LOCKING] 1 package to highest compatible version
[COMPILING] bar v0.1.0 ([ROOT]/foo/bar)
[COMPILING] foo v0.0.0 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `target-dir/debug/foo[EXE]`

"#]])
        .with_stdout_data(str![[r#"
CARGO_BIN_DIR_BAR=[ROOT]/foo/build-dir/debug/build/bar/[HASH]/artifact/bin
CARGO_BIN_FILE_BAR_bar=[ROOT]/foo/build-dir/debug/build/bar/[HASH]/artifact/bin/bar[..][EXE]

"#]])
        .run();

    p.root().join("build-dir").assert_build_dir_layout(str![[r#"
[ROOT]/foo/build-dir/.rustc_info.json
[ROOT]/foo/build-dir/CACHEDIR.TAG
[ROOT]/foo/build-dir/debug/.cargo-build-lock
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/fingerprint/bin-bar
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/fingerprint/bin-bar.json
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/fingerprint/dep-bin-bar
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/fingerprint/output-bin-bar
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-bin-foo
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/artifact/bin/bar[..][EXE]
[ROOT]/foo/build-dir/debug/build/bar/[HASH]/artifact/bin/bar[..].d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..].d

"#]]);

    p.root()
        .join("target-dir")
        .assert_build_dir_layout(str![[r#"
[ROOT]/foo/target-dir/CACHEDIR.TAG
[ROOT]/foo/target-dir/debug/.cargo-lock
[ROOT]/foo/target-dir/debug/.cargo-artifact-lock
[ROOT]/foo/target-dir/debug/foo[EXE]
[ROOT]/foo/target-dir/debug/foo.d

"#]]);
}

// Fine grain locking needs the new layout so __CARGO_TEMPORARY_BUILD_DIR_NEW_LAYOUT_OPT_OUT
// should lose and we should use the new layout anyways.
#[cargo_test]
fn new_layout_opt_out() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("Hello, World!") }"#)
        .file(
            ".cargo/config.toml",
            r#"
            [build]
            target-dir = "target-dir"
            build-dir = "build-dir"
            "#,
        )
        .build();

    p.cargo("build")
        .env("__CARGO_TEMPORARY_BUILD_DIR_NEW_LAYOUT_OPT_OUT", "1")
        .arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"])
        .enable_mac_dsym()
        .run();

    assert_not_exists(&p.root().join("target"));

    p.root().join("build-dir").assert_build_dir_layout(str![[r#"
[ROOT]/foo/build-dir/.rustc_info.json
[ROOT]/foo/build-dir/CACHEDIR.TAG
[ROOT]/foo/build-dir/debug/.cargo-build-lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/bin-foo.json
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/dep-bin-foo
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/invoked.timestamp
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..][EXE]
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/out/foo[..].d
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/.lock
[ROOT]/foo/build-dir/debug/build/foo/[HASH]/fingerprint/output-bin-foo

"#]]);

    p.root()
        .join("target-dir")
        .assert_build_dir_layout(str![[r#"
[ROOT]/foo/target-dir/CACHEDIR.TAG
[ROOT]/foo/target-dir/debug/.cargo-lock
[ROOT]/foo/target-dir/debug/.cargo-artifact-lock
[ROOT]/foo/target-dir/debug/foo[EXE]
[ROOT]/foo/target-dir/debug/foo.d

"#]]);
}

fn fine_grain(p: &Project, args: &str) -> Execs {
    let mut e = p.cargo(args);
    e.arg("-Zfine-grain-locking")
        .masquerade_as_nightly_cargo(&["fine grain locking"]);
    e
}

/// Source of a library that takes a moment to check, so that concurrent builds overlap.
fn slow_lib() -> String {
    (0..300)
        .map(|k| {
            format!(
                "pub fn f{k}(x: u64) -> u64 {{ (0..{}).fold(x, |a, b| a.wrapping_mul(31).rotate_left(b) ^ {k}) }}\n",
                k % 40 + 1
            )
        })
        .collect()
}

/// Spawns `cmds` at once and waits for all of them. Fails if they do not finish in time, which
/// would be a lock wait cycle, or if one fails. Returns the stderr of each.
fn run_concurrently(cmds: Vec<std::process::Command>) -> Vec<String> {
    use std::process::Stdio;
    use std::time::Instant;

    let logs: Vec<_> = (0..cmds.len())
        .map(|i| paths::root().join(format!("stderr-{i}.log")))
        .collect();
    let mut children: Vec<_> = cmds
        .into_iter()
        .zip(&logs)
        .map(|(mut cmd, log)| {
            cmd.stdout(Stdio::null())
                .stderr(std::fs::File::create(log).unwrap());
            cmd.spawn().unwrap()
        })
        .collect();
    let read_logs = || -> Vec<String> {
        logs.iter()
            .map(|log| std::fs::read_to_string(log).unwrap())
            .collect()
    };
    let deadline = Instant::now() + cargo_test_support::slow_cpu_multiplier(60);
    while children.iter_mut().any(|c| c.try_wait().unwrap().is_none()) {
        if Instant::now() > deadline {
            for c in &mut children {
                let _ = c.kill();
            }
            panic!(
                "concurrent builds did not finish:\n{}",
                read_logs().join("\n----\n")
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let stderr = read_logs();
    for (c, err) in children.iter_mut().zip(&stderr) {
        assert!(c.wait().unwrap().success(), "a build failed:\n{err}");
    }
    stderr
}

/// The crates that a `-v` build ran rustc for, from its stderr.
fn rustc_runs(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .filter(|line| line.trim_start().starts_with("Running `"))
        .filter_map(|line| line.split("--crate-name ").nth(1))
        .filter_map(|rest| rest.split(' ').next())
        .map(str::to_string)
        .collect()
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

#[cargo_test]
fn concurrent_builds_of_one_workspace_compile_each_unit_once() {
    // The shape of cargo#17508: both builds plan every unit as dirty. Each must not keep the units
    // that it built locked while it waits for the ones that the other is building, and must use
    // those once they are built instead of compiling them again.
    let mut p = project().file(
        "Cargo.toml",
        "[workspace]\nmembers = [\"app\", \"dep*\"]\nresolver = \"3\"\n",
    );
    let mut deps = String::new();
    for i in 1..=8 {
        p = p
            .file(
                &format!("dep{i}/Cargo.toml"),
                &format!("[package]\nname = \"dep{i}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
            )
            .file(&format!("dep{i}/src/lib.rs"), &slow_lib());
        deps.push_str(&format!("dep{i} = {{ path = \"../dep{i}\" }}\n"));
    }
    let p = p
        .file(
            "app/Cargo.toml",
            &format!(
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n{deps}"
            ),
        )
        .file(
            "app/src/main.rs",
            "fn main() { println!(\"{}\", dep1::f1(1) ^ dep8::f1(8)); }",
        )
        .build();
    let expected: Vec<_> = (1..=8)
        .map(|i| format!("dep{i}"))
        .chain(["app".to_string()])
        .collect();

    for _ in 0..3 {
        let _ = std::fs::remove_dir_all(p.root().join("target"));
        let stderr = run_concurrently(
            (0..2)
                .map(|_| fine_grain(&p, "check -v -p app").build_command())
                .collect(),
        );
        let runs = stderr.iter().flat_map(|s| rustc_runs(s)).collect();
        assert_eq!(
            sorted(runs),
            sorted(expected.clone()),
            "each unit is compiled by one build:\n{}",
            stderr.join("\n----\n")
        );
    }
}

#[cargo_test]
fn concurrent_builds_sharing_a_build_dir_reuse_each_others_units() {
    // Like two worktrees of one repository with a shared build directory. The units of registry
    // crates are the same for both, including a build script whose output both builds need.
    let mut deps = String::new();
    for i in 0..6 {
        Package::new(&format!("shared{i}"), "1.0.0")
            .file("src/lib.rs", &slow_lib())
            .publish();
        deps.push_str(&format!("shared{i} = \"1.0\"\n"));
    }
    Package::new("scripted", "1.0.0")
        .links("scripted")
        .file(
            "Cargo.toml",
            "[package]\nname = \"scripted\"\nversion = \"1.0.0\"\nlinks = \"scripted\"\n",
        )
        .file(
            "build.rs",
            r#"fn main() {
                println!("cargo::rerun-if-changed=build.rs");
                println!("cargo::metadata=value=42");
                println!("cargo::rustc-env=SCRIPTED=yes");
            }"#,
        )
        .file(
            "src/lib.rs",
            "pub fn scripted() -> &'static str { env!(\"SCRIPTED\") }",
        )
        .publish();
    let projects: Vec<_> = ["first", "second"]
        .iter()
        .map(|name| {
            project()
                .at(name)
                .file(
                    "Cargo.toml",
                    &format!(
                        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nscripted = \"1.0\"\n{deps}"
                    ),
                )
                .file(
                    "build.rs",
                    r#"fn main() {
                        let value = std::env::var("DEP_SCRIPTED_VALUE").unwrap();
                        println!("cargo::rustc-env=VALUE={value}");
                    }"#,
                )
                .file(
                    "src/main.rs",
                    r#"fn main() {
                        println!("{} {}", env!("VALUE"), scripted::scripted());
                        let _ = (shared0::f1(1), shared5::f1(5));
                    }"#,
                )
                .build()
        })
        .collect();

    for round in 0..2 {
        let build_dir = paths::root().join(format!("shared-build-{round}"));
        let stderr = run_concurrently(
            projects
                .iter()
                .map(|p| {
                    fine_grain(p, "build -v")
                        .env("CARGO_BUILD_BUILD_DIR", &build_dir)
                        .build_command()
                })
                .collect(),
        );
        let runs: Vec<_> = stderr.iter().flat_map(|s| rustc_runs(s)).collect();
        let expected: Vec<_> = (0..6)
            .map(|i| format!("shared{i}"))
            .chain(["scripted", "build_script_build", "first", "second"].map(String::from))
            .chain(["build_script_build"; 2].map(String::from))
            .collect();
        assert_eq!(
            sorted(runs),
            sorted(expected),
            "shared units are compiled by one build:\n{}",
            stderr.join("\n----\n")
        );
        let script_runs = stderr
            .iter()
            .flat_map(|s| s.lines())
            .filter(|line| {
                line.contains("Running `")
                    && !line.contains("--crate-name")
                    && line.contains("/build/scripted/")
            })
            .count();
        assert_eq!(script_runs, 1, "{}", stderr.join("\n----\n"));
        for (p, name) in projects.iter().zip(["first", "second"]) {
            p.process(p.bin(name))
                .with_stdout_data(str![[r#"
42 yes

"#]])
                .run();
            // The fingerprints that the two builds wrote agree with each other.
            let out = fine_grain(p, "build -v")
                .env("CARGO_BUILD_BUILD_DIR", &build_dir)
                .build_command()
                .output()
                .unwrap();
            let stderr = String::from_utf8(out.stderr).unwrap();
            assert!(
                out.status.success() && !stderr.contains("Running `"),
                "{stderr}"
            );
        }
    }
}

/// A build script for a registry crate that, in the build that sets `GATE`, waits until the file
/// that `GATE` names exists, and creates `<GATE>.ready` first. Other builds pass right through.
const GATE_BUILD_RS: &str = r#"
    fn main() {
        if let Ok(path) = std::env::var("GATE") {
            std::fs::write(format!("{path}.ready"), "").unwrap();
            while !std::path::Path::new(&path).exists() {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
"#;

fn wait_for(path: &std::path::Path) {
    let deadline = std::time::Instant::now() + cargo_test_support::slow_cpu_multiplier(60);
    while !path.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "{} did not appear",
            path.display()
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[cargo_test]
fn a_build_reuses_units_that_another_build_compiles_while_it_waits() {
    // The second build starts while the first runs a build script. It plans that unit and the
    // ones that depend on it as dirty, waits for the first build to finish the script, and uses
    // its output. Of the units left, each build compiles the ones that it gets to first, and the
    // other one uses them: each unit is compiled once.
    Package::new("gate", "1.0.0")
        .file("build.rs", GATE_BUILD_RS)
        .file("src/lib.rs", "pub fn gate() {}")
        .publish();
    Package::new("after", "1.0.0")
        .dep("gate", "1.0")
        .file("src/lib.rs", "pub fn after() { gate::gate() }")
        .publish();
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.1.0"
                edition = "2021"

                [dependencies]
                after = "1.0"
            "#,
        )
        .file("src/lib.rs", "pub fn foo() { after::after() }")
        .build();
    let release = paths::root().join("release");

    let first_log = paths::root().join("first.log");
    let mut first = fine_grain(&p, "check -v")
        .env("GATE", &release)
        .build_command();
    first
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&first_log).unwrap());
    let mut first = first.spawn().unwrap();
    wait_for(&paths::root().join("release.ready"));

    let second_log = paths::root().join("second.log");
    let mut second = fine_grain(&p, "check -v").build_command();
    second
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&second_log).unwrap());
    let mut second = second.spawn().unwrap();
    // Let the second build reach the unit's lock before the first one goes on.
    let deadline = std::time::Instant::now() + cargo_test_support::slow_cpu_multiplier(60);
    while !std::fs::read_to_string(&second_log)
        .unwrap()
        .contains("Blocking waiting for file lock on gate")
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the second build did not wait for the first:\n{}",
            std::fs::read_to_string(&second_log).unwrap()
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::fs::write(&release, "").unwrap();

    assert!(first.wait().unwrap().success());
    assert!(second.wait().unwrap().success());
    let stderr = [&first_log, &second_log].map(|log| std::fs::read_to_string(log).unwrap());
    let runs = stderr.iter().flat_map(|s| rustc_runs(s)).collect();
    assert_eq!(
        sorted(runs),
        sorted(["after", "build_script_build", "foo", "gate"].map(String::from).to_vec()),
        "each unit is compiled once:\n{}",
        stderr.join("\n----\n")
    );
    assert!(
        !stderr[1].contains("out/build_script_build`"),
        "the second build does not run the build script:\n{}",
        stderr[1]
    );
}

#[cargo_test]
fn a_build_waiting_for_a_lock_gives_back_its_jobserver_token() {
    // With one job, the second build waits for a unit that the first build compiles, and still
    // builds its other units meanwhile (cargo#6747). A proc macro holds up the first build's rustc.
    Package::new("gatemac", "1.0.0")
        .proc_macro(true)
        .file(
            "src/lib.rs",
            &format!(
                "extern crate proc_macro;\n\
                 #[proc_macro]\n\
                 pub fn gate(_: proc_macro::TokenStream) -> proc_macro::TokenStream {{\n{}\n\
                 \"\".parse().unwrap()\n}}",
                GATE_BUILD_RS
                    .replace("fn main() {", "")
                    .trim_end()
                    .trim_end_matches('}')
            ),
        )
        .publish();
    Package::new("gate", "1.0.0")
        .dep("gatemac", "1.0")
        .file("src/lib.rs", "gatemac::gate!();\npub fn gate() {}")
        .publish();
    Package::new("other", "1.0.0")
        .file("src/lib.rs", "pub fn other() {}")
        .publish();
    let manifest = |name: &str, deps: &str| {
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{deps}")
    };
    let first = project()
        .at("first")
        .file("Cargo.toml", &manifest("first", "gate = \"1.0\""))
        .file("src/lib.rs", "")
        .build();
    let second = project()
        .at("second")
        .file(
            "Cargo.toml",
            &manifest("second", "gate = \"1.0\"\nother = \"1.0\""),
        )
        .file("src/lib.rs", "")
        .build();
    let build_dir = paths::root().join("shared-build");
    let release = paths::root().join("release");

    let mut a = fine_grain(&first, "check")
        .env("CARGO_BUILD_BUILD_DIR", &build_dir)
        .env("GATE", &release)
        .build_command();
    a.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut a = a.spawn().unwrap();
    wait_for(&paths::root().join("release.ready"));

    let mut b = fine_grain(&second, "check -j1")
        .env("CARGO_BUILD_BUILD_DIR", &build_dir)
        .build_command();
    b.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut b = b.spawn().unwrap();
    // `other` gets built while the second build waits for `gate`.
    let other = build_dir.join("debug/build/other");
    let deadline = std::time::Instant::now() + cargo_test_support::slow_cpu_multiplier(60);
    while !other.exists()
        || !std::fs::read_dir(&other).unwrap().any(|unit| {
            let out = unit.unwrap().path().join("out");
            out.exists()
                && std::fs::read_dir(out)
                    .unwrap()
                    .any(|f| f.unwrap().path().extension().is_some_and(|e| e == "rmeta"))
        })
    {
        if std::time::Instant::now() > deadline {
            std::fs::write(&release, "").unwrap();
            let _ = (a.wait(), b.wait());
            panic!("the waiting build did not build its other units");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::fs::write(&release, "").unwrap();
    assert!(a.wait().unwrap().success());
    assert!(b.wait().unwrap().success());
}

#[cargo_test]
fn a_build_waits_for_a_unit_that_another_build_uses_with_other_settings() {
    // `RUSTFLAGS` with a path remap stay out of the unit's id: both builds share the unit
    // directory of `shared-dep`, with different fingerprints. The second build waits until the
    // first is done with it, then builds it.
    Package::new("shared-dep", "1.0.0")
        .file("src/lib.rs", "pub fn shared() {}")
        .publish();
    let manifest = |name: &str| {
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nshared-dep = \"1.0\"\n")
    };
    // `first`'s own build script holds the build, with shared-dep locked shared as fresh.
    let first = project()
        .at("first")
        .file("Cargo.toml", &manifest("first"))
        .file("src/lib.rs", "pub fn f() { shared_dep::shared() }")
        .file("build.rs", &GATE_BUILD_RS.replace("fn main() {", "fn main() {\n println!(\"cargo::rerun-if-env-changed=GATE\");"))
        .build();
    let second = project()
        .at("second")
        .file("Cargo.toml", &manifest("second"))
        .file("src/lib.rs", "pub fn f() { shared_dep::shared() }")
        .build();
    let build_dir = paths::root().join("shared-build");
    let first_flags = "--remap-path-prefix=/c=/d";
    fine_grain(&first, "check")
        .env("CARGO_BUILD_BUILD_DIR", &build_dir)
        .env("RUSTFLAGS", first_flags)
        .run();

    let release = paths::root().join("release");
    let mut a = fine_grain(&first, "check")
        .env("CARGO_BUILD_BUILD_DIR", &build_dir)
        .env("RUSTFLAGS", first_flags)
        .env("GATE", &release)
        .build_command();
    a.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut a = a.spawn().unwrap();
    wait_for(&paths::root().join("release.ready"));

    let second_log = paths::root().join("second.log");
    let mut b = fine_grain(&second, "check -v")
        .env("CARGO_BUILD_BUILD_DIR", &build_dir)
        .env("RUSTFLAGS", "--remap-path-prefix=/a=/b")
        .build_command();
    b.stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&second_log).unwrap());
    let mut b = b.spawn().unwrap();
    let deadline = std::time::Instant::now() + cargo_test_support::slow_cpu_multiplier(60);
    while !std::fs::read_to_string(&second_log)
        .unwrap()
        .contains("Blocking waiting for file lock on shared-dep")
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the second build did not wait for the first:\n{}",
            std::fs::read_to_string(&second_log).unwrap()
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // It keeps waiting while the first build holds the unit.
    std::thread::sleep(std::time::Duration::from_secs(1));
    assert!(b.try_wait().unwrap().is_none());
    std::fs::write(&release, "").unwrap();
    assert!(a.wait().unwrap().success());
    assert!(b.wait().unwrap().success());
    let stderr = std::fs::read_to_string(&second_log).unwrap();
    assert_eq!(
        sorted(rustc_runs(&stderr)),
        sorted(vec!["second".to_string(), "shared_dep".to_string()]),
        "{stderr}"
    );
}

fn parse_workspace_manifest_path_hash(hash_dir: &PathBuf) -> PathBuf {
    // Since the hash will change between test runs simply find the first directories and assume
    // that is the hash dir. The format is a 2 char directory followed by the remaining hash in the
    // inner directory (ie. `34/f9d02eb8411c05`)
    let mut dirs = std::fs::read_dir(hash_dir).unwrap().into_iter();
    let outer_hash_dir = dirs.next().unwrap().unwrap();
    // Validate there are no other directories in `hash_dir`
    assert!(
        dirs.next().is_none(),
        "Found multiple dir entries in {hash_dir:?}"
    );
    // Validate the outer hash dir hash is a directory and has the correct hash length
    assert!(
        outer_hash_dir.path().is_dir(),
        "{outer_hash_dir:?} was not a directory"
    );
    assert_eq!(
        outer_hash_dir.path().file_name().unwrap().len(),
        2,
        "Path {:?} should have been 2 chars",
        outer_hash_dir.path().file_name()
    );

    let mut dirs = std::fs::read_dir(outer_hash_dir.path())
        .unwrap()
        .into_iter();
    let inner_hash_dir = dirs.next().unwrap().unwrap();
    // Validate there are no other directories in first hash dir
    assert!(
        dirs.next().is_none(),
        "Found multiple dir entries in {outer_hash_dir:?}"
    );
    // Validate the outer hash dir hash is a directory and has the correct hash length
    assert!(
        inner_hash_dir.path().is_dir(),
        "{inner_hash_dir:?} was not a directory"
    );
    assert_eq!(
        inner_hash_dir.path().file_name().unwrap().len(),
        14,
        "Path {:?} should have been 2 chars",
        inner_hash_dir.path().file_name()
    );
    return inner_hash_dir.path();
}

#[track_caller]
fn assert_exists(path: &PathBuf) {
    assert!(
        path.exists(),
        "Expected `{}` to exist but was not found.",
        path.display()
    );
}

#[track_caller]
fn assert_not_exists(path: &PathBuf) {
    assert!(
        !path.exists(),
        "Expected `{}` to NOT exist but was found.",
        path.display()
    );
}

#[track_caller]
fn assert_exists_patterns_with_base_dir(base: &PathBuf, patterns: &[&str]) {
    let root = base.to_str().unwrap();
    let p: Vec<_> = patterns.iter().map(|p| format!("{root}/{p}")).collect();
    let p: Vec<&str> = p.iter().map(|v| v.as_str()).collect();
    assert_exists_patterns(&p);
}

#[track_caller]
fn assert_exists_patterns(patterns: &[&str]) {
    for p in patterns {
        assert_exists_pattern(p);
    }
}

#[track_caller]
fn assert_exists_pattern(pattern: &str) {
    use glob::glob;

    let mut z = glob(pattern).unwrap();

    assert!(
        z.next().is_some(),
        "Expected `{pattern}` to match existing file but was not found.",
    )
}
