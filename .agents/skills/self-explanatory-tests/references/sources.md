# Testing Sources

These sources explain the reasoning behind the workspace policy. Use them when a test boundary, failure-message rule, dependency choice, or lint exception is disputed.

## Source Skills Adapted

- `contract-tests`: name the contract before RED, ground expected behavior, choose the smallest stable boundary, prefer observable outcomes, resist mocks, and run a refactor-survival check.
- `error-messages`: treat failures as developer-facing errors that identify what happened, why it matters, relevant context, and useful next evidence.
- `root-cause`: reproduce the symptom and protect the deepest violated promise rather than the first visible symptom.
- `red-green`: prove RED fails for the intended reason, classify failures honestly, and never report unexecuted tests as successful.
- `fool-proof-software`: remove hidden memory requirements from fixtures and make invalid setup fail at the earliest useful boundary.
- `software-taste`: make invalid states difficult to represent, parse once at honest boundaries, inject dependencies without ceremony, and test observable contracts.
- `designing-trustworthy-functions-and-data`: keep production contracts, dependencies, effects, invariants, and abstraction boundaries truthful while resisting mock-driven or type-driven over-design.

The repo-owned skill adapts these principles to Rust and the template's lint policy; it does not import the source skills' broader workflows.

## Rust Mechanics

- [The Rust Programming Language: Test Organization](https://doc.rust-lang.org/book/ch11-03-test-organization.html) — unit tests commonly live in the module they test; integration tests exercise the public API as external crates.
- [Cargo Targets: Tests](https://doc.rust-lang.org/cargo/reference/cargo-targets.html#tests) — files under `tests/` are separate integration-test crates, and unit tests can access private APIs in their target.
- [`cargo test`](https://doc.rust-lang.org/cargo/commands/cargo-test.html) — documents target selection, test-name filtering, parallel execution, captured output, and exit status.
- [Clippy Lint Configuration](https://doc.rust-lang.org/clippy/lint_configuration.html) — defines the test-specific panic, `expect`, `unwrap`, and indexing controls used by `clippy.toml`.
- [`std::error`: Common Message Styles](https://doc.rust-lang.org/std/error/index.html#common-message-styles) — explains why `expect` should describe the precondition that should have held instead of repeating the source error.

## Contract Quality

- [Google Testing Blog: Test Behavior, Not Implementation](https://testing.googleblog.com/2013/08/testing-on-toilet-test-behavior-not.html) — tests should normally survive behavior-preserving implementation changes.
- [Google Testing Blog: Test Behaviors, Not Methods](https://testing.googleblog.com/2014/04/testing-on-toilet-test-behaviors-not.html) — behavior concepts, rather than one test per method, determine useful test boundaries.
- [Google Testing Blog: Don't Overuse Mocks](https://testing.googleblog.com/2013/05/testing-on-toilet-dont-overuse-mocks.html) — interaction-heavy mocks leak implementation details and provide weaker evidence than real or hermetic alternatives.
- [Google Testing Blog: Increase Test Fidelity by Avoiding Mocks](https://testing.googleblog.com/2024/02/increase-test-fidelity-by-avoiding-mocks.html) — choose the highest-fidelity dependency that remains appropriate for the test's size and determinism constraints.
- [Addy Osmani: Test-Driven Development Skill](https://github.com/addyosmani/agent-skills/blob/main/skills/test-driven-development/SKILL.md) — source for the prove-it regression pattern, DAMP tests, behavior-sized cases, and common testing rationalizations.

## Design and Dependency Boundaries

- [Rust API Guidelines: Dependability](https://rust-lang.github.io/api-guidelines/dependability.html) — prefers argument types that statically rule out invalid input and otherwise validates input at the boundary.
- [Rust API Guidelines: Type Safety](https://rust-lang.github.io/api-guidelines/type-safety.html) — documents newtypes and deliberate argument types as ways to encode meaning and invariants.
- [Martin Fowler: Inversion of Control Containers and the Dependency Injection Pattern](https://martinfowler.com/articles/injection.html) — distinguishes dependency injection from service location and emphasizes separating configuration from use.
