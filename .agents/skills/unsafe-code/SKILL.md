---
name: unsafe-code
description: "Use when writing or reviewing Rust unsafe, FFI, raw pointers, libc, syscalls, or `extern` blocks in this workspace; when `forbid(unsafe_code)` blocks a build; or when adding `crates/ffi`. Do not use for ordinary safe Rust."
---

# Unsafe code

This workspace sets `unsafe_code = "forbid"`. App crates cannot `#[allow(unsafe_code)]`. The dedicated FFI crate uses one narrow `unsafe_code = "allow"` exception because its purpose is to contain required unsafe operations; that exception does not relax any app crate.

Do not add `crates/ffi` until a task actually needs unsafe. Do not add it empty.

## Procedure

1. Confirm the work needs `unsafe`, FFI, raw pointers, or `extern "C"`. If not, stop.
2. If `crates/ffi` is missing, add it using [references/add-ffi-crate.md](references/add-ffi-crate.md).
3. Put every `unsafe` block, `unsafe fn`, and `extern` block in `crates/ffi` only.
4. Export a **safe** function or type. Other crates depend on `ffi` and call that API.
5. On each `unsafe` block, write a `SAFETY:` comment that names the invariant.
6. Do not change workspace `unsafe_code = "forbid"`. Do not copy `unsafe_code = "allow"` onto an app crate.
7. If `crates/ffi` exists when the workspace lint tables change, synchronize its copied lint tables while preserving only the documented FFI-specific differences.

## Non-examples

- `unsafe { *ptr }` in `crates/messages` or any app crate
- `#[allow(unsafe_code)]` in an app crate
- Adding `crates/ffi` with no unsafe yet
