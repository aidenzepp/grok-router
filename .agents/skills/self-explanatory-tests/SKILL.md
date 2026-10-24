---
name: self-explanatory-tests
description: Use when writing, reviewing, organizing, or debugging tests, assessing test completeness, or shaping types and dependency boundaries so important behavior can be tested honestly in this workspace.
---

# Self-Explanatory Tests

Tests are executable contracts for future maintainers and agents. A failure should identify the broken behavior without requiring the reader to reconstruct what the original author intended.

- Read [references/examples.md](references/examples.md) when authoring or reorganizing tests.
- Read [references/non-examples.md](references/non-examples.md) when reviewing tests, diagnosing brittle coverage, or considering a mock or lint exception.
- Read [references/design-and-seams.md](references/design-and-seams.md) when tests expose a weak invariant, hidden dependency, awkward abstraction, or uncertain coverage boundary.
- Read [references/sources.md](references/sources.md) when the policy or test strategy is disputed.
- Use `designing-trustworthy-functions-and-data` before changing production types, dependencies, effects, or function boundaries in response to test pressure.

## Name the Contract First

Do not write a test until you can state the behavior it protects and the evidence for that expectation. Ground truth can come from an explicit user decision, public API or CLI contract, schema, verified documentation, existing behavior, or a reproduced bug. Do not encode guessed behavior.

Before RED, identify:

- the behavior contract
- the smallest stable boundary that exposes it
- inputs and observable outputs, effects, or errors
- the highest-fidelity deterministic dependency available
- what the test intentionally does not protect
- the command that will prove RED and GREEN

For a small unit test, the test name and arrangement may make this packet self-evident. Record it explicitly for bug regressions, integration tests, ambiguous behavior, or mock use.

## Map the Test Obligations

Do not equate completeness with one test per function, method, or module. Completeness means every meaningful promise has an owner, an enforcement mechanism, and credible evidence.

Inventory the obligations relevant to the change or system:

- domain rules, invariants, assumptions, and preconditions
- accepted, rejected, boundary, degenerate, and malformed inputs
- state transitions, lifecycle rules, side effects, and required non-effects
- success, failure, partial failure, retry, recovery, and cancellation
- persistence, serialization, parsing, and round trips
- ordering, duplication, idempotence, concurrency, and resource limits
- permissions, privacy, security, and unsafe boundaries
- time, randomness, environment, filesystem, network, and process behavior
- compatibility, migration, performance, and error contracts when they are part of the promise

Scale the inventory to the work. A small pure function may need only a few cases; a stateful or external workflow needs an explicit obligation map. For consequential work, record:

| Obligation | Owner | Enforcement | Test evidence | Remaining gap or status |
| --- | --- | --- | --- | --- |

An obligation may be covered, not applicable with a reason, or deliberately deferred with a named risk and explicit approval. Do not silently omit it. A type, constructor, parser, database constraint, or compiler check may enforce an obligation, but tests should still prove that the enforcement boundary accepts, rejects, and preserves the right states.

## Let Tests Pressure the Design

Tests provide evidence that an invariant is enforced; they should not be the only thing enforcing it.

When a test must repeatedly establish or check the same assumption:

1. Name the invariant and the type or component that should own it.
2. Ask whether invalid states can be made unrepresentable with a dedicated type or lifecycle state.
3. Otherwise, validate once at the earliest trustworthy boundary and carry a trusted value inward.
4. Make later APIs accept the trusted type instead of raw input.
5. Test construction, rejection, invariant preservation, and every path that could bypass the boundary, including parsing or deserialization.

Repeated validation, elaborate fixtures, or inaccessible behavior are design evidence. Look for a missing constructor, parser, state machine, domain type, or dependency boundary before adding more test machinery. If redesign is outside the authorized scope, add the best regression test available and record the unenforced assumption as design debt; do not leave both the design and the behavior unprotected.

Test pressure does not authorize a mechanically purer architecture. Preserve transactions, locks, ownership, ordering, cancellation, backpressure, and external-system semantics. Use `designing-trustworthy-functions-and-data` to select the production boundary, then use this skill to prove its contract.

## Choose the Test Boundary

- Put unit tests inline at the bottom of the production module they exercise.
- Use `tests/*.rs` only for public crate contracts, executable behavior, or interactions that genuinely cross a crate boundary.
- Test observable behavior. Inline access to private items is not permission to freeze private helpers, call order, data structures, or module decomposition.
- Choose a boundary that survives a behavior-preserving refactor. If moving or renaming a helper would break the test while behavior stays the same, the test is probably too coupled.
- For a bug, reproduce the symptom, trace it to the deepest violated promise, and protect that promise rather than the first visible workaround.

Use the inline unit-test layout shown in [references/examples.md](references/examples.md). Do not create sibling `*_test.rs` files or `#[path = "..."]` test modules by default. Extract unusually large shared test support only when a real project requires it.

## Design Honest Test Seams

Make dependencies explicit when they introduce IO, nondeterminism, external state, dangerous side effects, or a meaningful ownership boundary. Dependency injection is a means of controlling those dependencies, not a requirement to abstract every collaborator.

Honest does not mean pure. A function may mutate an explicit argument, consume an RNG, write through a supplied destination, or use a transaction and still be honest because its authority and effects are visible in the signature. Tests must control or observe those declared effects; do not assume explicit injection makes them deterministic or faithful to production.

Prefer the smallest honest seam:

1. Pass a value when the decision only needs data such as the current time or a generated identifier.
2. Pass a function or closure for one narrow operation.
3. Use a small, consumer-owned trait or generic bound for a coherent capability that genuinely varies.
4. Keep a concrete type when substitution would add no useful control or clarity.

Wire real implementations at a visible composition boundary. Do not hide dependencies in global state, service locators, generic context objects, or DI containers. Do not create one trait per struct, expose private choreography, or add factories solely so a test can count calls. An abstraction is earned when it expresses a stable capability, ownership boundary, or legitimate substitution—not merely because mocking tools prefer it.

Passing a dependency makes it explicit; it does not make it deterministic or faithful. Prefer an ordinary supplied value when the decision needs only a snapshot, and do not move reads or writes outside the transaction, lock, stream, or workflow boundary that gives them meaning.

Then prefer the highest-fidelity implementation of the seam that remains safe and deterministic:

1. Real local implementation.
2. Hermetic local service or emulator.
3. Maintained fake.
4. Recorded fixture.
5. Stub that returns values without asserting interactions.
6. Mock or recording double, only when the interaction itself is the stable contract.

Dangerous side effects may justify replacing a real dependency, but they do not justify arbitrary interaction assertions. Do not mock because it is easier. A mock that restates the current call graph can pass while production is broken. If a mock is necessary, record why every higher-fidelity option is unsuitable and why the asserted interaction is itself a durable contract. When a fake implements important semantics, run shared contract tests against the fake and real implementation when practical so the fake cannot quietly become a different system.

## Preserve the Lint Policy

Every crate root and direct integration-test root must include the test-only callable-contract exception documented in `README.md`. Each direct `tests/*.rs` file is a separate crate root and also needs a short `//!` description of the contract it exercises.

Inline unit tests inherit the crate-root exception. They do not need item-level documentation.

Do not weaken workspace lint levels or add local `allow` attributes to make tests pass. The approved test policy is already encoded in `clippy.toml`:

- `expect` and panic are available in tests when they provide intentional context.
- `unwrap` remains disallowed.
- correctness, safety, indexing, and other production safeguards still apply.

If a lint appears incompatible with a legitimate test, follow the policy-change procedure in `AGENTS.md` instead of bypassing it.

## Make Failures Self-Explanatory

Treat a test failure as a developer-facing error message. As directly as the assertion surface allows, it should reveal:

- what behavior or operation failed
- why the result was expected
- what was actually observed or which source error occurred
- which input, fixture, identifier, or state matters for diagnosis

Keep the diagnostic audience distinct from the product audience. When testing a user-facing or machine-readable error contract, assert that boundary output separately; use the test's own assertion message to help a developer diagnose why the contract failed.

A strong test also:

- Name each test for the behavior and condition it proves.
- Keep one coherent reason for failure per test.
- Phrase `expect` as the precondition that should have held; Rust automatically appends the source error.
- Panic only when the message identifies the failed operation, fixture, or assumption.
- Add assertion context when the default assertion output would not identify the broken contract.
- Include relevant inputs or observed output when they are needed to diagnose the failure.
- For stateful workflows, include whether the expected state or artifact was preserved when that fact changes the diagnosis.
- Never expose secrets or unnecessarily dump large payloads merely to add context.

Do not wrap every test failure in production-style error types merely to avoid panicking. Tests may fail fast; the quality requirement is precise diagnostic context.

## Prove Red, Green, and Refactor

For new behavior or a bug fix:

1. Run the focused test before implementation and confirm it fails for the intended missing or broken contract, not malformed setup.
2. Make the smallest implementation change that satisfies the contract.
3. Run the focused test and confirm it passes.
4. Refactor without changing behavior, then rerun the focused and relevant broader tests.

When a test fails, classify it before editing: the test is wrong, the implementation is wrong, the design blocks honest testing, or the environment is broken. Do not turn a wrong test green by changing production behavior, and do not hide an implementation bug by weakening the assertion.

## Keep Tests Reliable

- Use fixed inputs, clocks, identifiers, and ordering when the behavior permits it.
- Use temporary directories and isolated fixtures instead of real user data or machine state.
- Keep tests safe to run in parallel; do not rely on execution order or shared mutable globals.
- Exercise real behavior at the narrowest useful boundary.
- Keep reusable fixture builders explicit about the state they create.
- Prefer readable, behavior-sized setup over abstract helpers that hide the scenario. Repetition is acceptable when it keeps the contract visible.
- Make the correct fixture path the easiest path. If every author must remember a hidden setup rule, encode it in a builder or fail at the earliest boundary with specific context.

## Verify

Run the narrowest relevant test while iterating. Before completion, run the workspace checks:

```bash
cargo fmt
cargo clippy --workspace --all-targets
cargo test --workspace
```

Read the actual failure output before reporting success. A passing test with warnings is not complete.
