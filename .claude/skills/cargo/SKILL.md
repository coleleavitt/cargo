```markdown
# cargo Development Patterns

> Auto-generated skill from repository analysis

## Overview

This skill teaches you the core development patterns, coding conventions, and common workflows for contributing to the [cargo](https://github.com/rust-lang/cargo) codebase. cargo is the Rust package manager, and its repository is primarily written in Rust. The codebase emphasizes maintainability, modularity, and consistency, with well-defined commit patterns and workflows for diagnostics, testing, and schema features.

## Coding Conventions

- **File Naming:**  
  Use `snake_case` for all file and module names.  
  _Example:_  
  ```
  src/cargo/diagnostics/rules/deferred_parse_diagnostics.rs
  ```

- **Import Style:**  
  Prefer relative imports within the crate.  
  _Example:_  
  ```rust
  use super::lint_utils;
  use crate::diagnostics::rules::some_rule;
  ```

- **Export Style:**  
  Use named exports for modules and functions.  
  _Example:_  
  ```rust
  pub fn check_rule() { ... }
  pub mod lint_utils;
  ```

- **Commit Messages:**  
  Follow [Conventional Commits](https://www.conventionalcommits.org/), using prefixes like `refactor`, `fix`, `feat`, `test`, `docs`.  
  _Example:_  
  ```
  refactor: unify rule function signatures in diagnostics
  ```

## Workflows

### Refactor Diagnostics Rules and Passes
**Trigger:** When you want to improve, reorganize, or make diagnostics/lints more maintainable and consistent.  
**Command:** `/refactor-diagnostics`

1. Edit multiple files in `src/cargo/diagnostics/rules/` (often many `*.rs` and `mod.rs`).
2. Edit `src/cargo/diagnostics/passes.rs` to change how passes/rules are registered or invoked.
3. Optionally edit `src/cargo/diagnostics/mod.rs` or `src/cargo/diagnostics/lint.rs`.
4. Update call sites in `src/cargo/ops/cargo_compile/mod.rs` and `src/cargo/ops/cargo_fetch.rs` if the interface changes.

_Example: Making rule function signatures consistent_
```rust
// Before
pub fn check_foo(ctx: &Context) { ... }

// After
pub(crate) fn check_foo(ctx: &mut Context, lint: &Lint) { ... }
```

### Fix or Update Diagnostics and Tests
**Trigger:** When you need to fix a bug or improve consistency in diagnostics reporting, especially for deferred or special-case diagnostics.  
**Command:** `/fix-diagnostics-tests`

1. Edit `src/cargo/diagnostics/rules/deferred_parse_diagnostics.rs` and/or related diagnostics files.
2. Edit `src/cargo/core/workspace.rs` (where diagnostics are invoked).
3. Edit many files in `tests/testsuite/` to update or add tests for the changed diagnostics behavior.

_Example: Updating a test for new diagnostics output_
```rust
#[test]
fn test_deferred_parse_diagnostics() {
    // ... setup ...
    assert!(output.contains("new diagnostic message"));
}
```

### Test Lints Optimization
**Trigger:** When you want to optimize or refactor lint-related tests for speed and efficiency.  
**Command:** `/optimize-lint-tests`

1. Edit many files in `tests/testsuite/lints/` (including `mod.rs` and individual lint test files).
2. Reduce or remove compilation steps where possible.

_Example: Removing unnecessary compilation in a test_
```rust
// Before
cargo_build();
assert!(check_lint_output());

// After
assert!(check_lint_output_without_build());
```

### Schema Feature Development
**Trigger:** When you want to add, expose, or update schema/config features for registries.  
**Command:** `/add-schema-feature`

1. Edit or add files in `crates/cargo-util-schemas/` (such as `src/index.rs`, `*.schema.json`).
2. Edit `src/cargo/sources/registry/` files as needed.
3. Optionally update related util or download logic.

_Example: Adding a new schema file_
```json
// crates/cargo-util-schemas/registry.schema.json
{
  "type": "object",
  "properties": {
    "index": { "type": "string" }
  }
}
```

## Testing Patterns

- **Test Framework:**  
  The specific framework is not detected, but Rust's built-in test framework is commonly used.

- **Test File Pattern:**  
  Test files are typically located in `tests/testsuite/` and named with `snake_case` (`*.rs`).  
  _Example:_  
  ```
  tests/testsuite/lints/dead_code.rs
  ```

- **Test Example:**
  ```rust
  #[test]
  fn test_lint_dead_code() {
      // ... test logic ...
      assert!(output.contains("dead code"));
  }
  ```

## Commands

| Command                | Purpose                                                      |
|------------------------|--------------------------------------------------------------|
| /refactor-diagnostics  | Refactor diagnostics rules, passes, and registration         |
| /fix-diagnostics-tests | Fix or update diagnostics reporting and related tests        |
| /optimize-lint-tests   | Optimize or refactor lint-related tests for speed/efficiency |
| /add-schema-feature    | Add or update schema/config features for registries          |
```