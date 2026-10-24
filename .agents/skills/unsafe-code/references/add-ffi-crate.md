# Add `crates/ffi`

Cargo cannot mix `lints.workspace = true` with extra lint keys. The FFI crate must copy the workspace lint tables and apply a narrow local delta.

1. `cargo new crates/ffi --lib --name ffi`
2. Set `publish = false`, edition `2024`, and a description.
3. Copy `[workspace.lints.rust]` and `[workspace.lints.clippy]` from the workspace `Cargo.toml` into `crates/ffi/Cargo.toml` as `[lints.rust]` and `[lints.clippy]`.
4. Preserve every copied level, group priority, and specific override exactly. Do not add `clippy::all`; it can weaken the explicit `correctness = "deny"` policy through lint ordering.
5. Apply only this FFI-specific Rust lint delta:

   ```toml
   unsafe_code = "allow"
   unsafe_op_in_unsafe_fn = "deny"
   ffi_unwind_calls = "deny"
   ```

   `unsafe_code = "allow"` is the deliberate exception that lets this isolated crate perform unsafe work. The two denies require explicit unsafe blocks and reject unwinding FFI calls by default.
6. Do not add the Edition 2024 migration lints `missing_unsafe_on_extern` or `deprecated_safe_2024` to this new crate.
7. In `crates/ffi/src/lib.rs`, document that this crate is the only place for unsafe. Export safe wrappers.
8. App crates depend on `ffi = { path = "../ffi" }` and do not contain `unsafe`.

When the workspace lint tables change, update this crate's copied tables in the same change. The three entries above are the only intentional differences.
