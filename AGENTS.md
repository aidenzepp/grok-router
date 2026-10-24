# Rust Workspace Template

This repository is an intentionally empty stable Rust workspace. It is a foundation for future projects, not a sample application. Create only the crates the project needs under `crates/`; do not pre-create shared or FFI crates.

## Policy

`Cargo.toml` and `clippy.toml` are the source of truth.

- Fix code instead of weakening policy. Do not change lint levels, add broad `allow` attributes, or skip verification targets without explicit approval.
- Treat warnings as design feedback. They may remain while exploring, but resolve them before calling work complete.
- Keep workspace lint inheritance in every ordinary crate. Current Cargo adds it when creating a crate in this workspace.
- Give private production items the same documentation scrutiny as public items.
- Tests may omit callable-contract documentation, but they remain subject to production correctness and safety rules. `expect` requires a useful failure message; `unwrap` remains disallowed.
- Unsafe Rust is forbidden by default. Add an FFI boundary only when the project actually requires one and the exception has been approved.

If a rule appears inappropriate, explain the failing rule, why changing the code would be worse, the smallest policy change, and its consequences. Change the policy only after approval.

## Workflow

Follow `README.md` to create the first crate. The workspace has no build targets until then.

Before completion, run:

```bash
cargo fmt
cargo clippy --workspace --all-targets
cargo test --workspace
```

Resolve all warnings before finishing.

## Skills

- `.agents/skills/conventional-commits`: commit decisions, boundaries, staging, and messages.
- `.agents/skills/designing-trustworthy-functions-and-data`: functions, domain types, data structures, parsing boundaries, dependencies, effects, invariants, and abstraction levels.
- `.agents/skills/self-explanatory-tests`: test design, organization, failure messages, and debugging.
- `.agents/skills/unsafe-code`: unsafe Rust, FFI, raw pointers, syscalls, and `extern` blocks.

Keep only broadly reusable foundation skills here. Project-specific skills belong in the project that needs them.
