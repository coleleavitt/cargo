---
name: refactor-diagnostics-rules-and-passes
description: Workflow command scaffold for refactor-diagnostics-rules-and-passes in cargo.
allowed_tools: ["Bash", "Read", "Write", "Grep", "Glob"]
---

# /refactor-diagnostics-rules-and-passes

Use this workflow when working on **refactor-diagnostics-rules-and-passes** in `cargo`.

## Goal

Refactoring the diagnostics system, including moving, renaming, or making data-driven the diagnostics passes and rules. This often involves making rule function names consistent, reducing visibility, changing parameter lists, and restructuring how diagnostics are registered and called.

## Common Files

- `src/cargo/diagnostics/rules/*.rs`
- `src/cargo/diagnostics/rules/mod.rs`
- `src/cargo/diagnostics/passes.rs`
- `src/cargo/diagnostics/mod.rs`
- `src/cargo/diagnostics/lint.rs`
- `src/cargo/ops/cargo_compile/mod.rs`

## Suggested Sequence

1. Understand the current state and failure mode before editing.
2. Make the smallest coherent change that satisfies the workflow goal.
3. Run the most relevant verification for touched files.
4. Summarize what changed and what still needs review.

## Typical Commit Signals

- Edit multiple files in src/cargo/diagnostics/rules/ (often many *.rs and mod.rs)
- Edit src/cargo/diagnostics/passes.rs to change how passes/rules are registered or invoked
- Optionally edit src/cargo/diagnostics/mod.rs or src/cargo/diagnostics/lint.rs
- Update call sites in src/cargo/ops/cargo_compile/mod.rs and src/cargo/ops/cargo_fetch.rs if the interface changes

## Notes

- Treat this as a scaffold, not a hard-coded script.
- Update the command if the workflow evolves materially.