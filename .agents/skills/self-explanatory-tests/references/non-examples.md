# Testing Non-Examples

These patterns create green tests without trustworthy evidence or failures without enough context to repair them.

## Opaque Failure Messages

```rust
let config = load_fixture().expect("failed");
let first = values.first().unwrap();
assert!(output.status.success());
```

Problems:

- `failed` does not name the assumption or operation.
- `unwrap` provides no authored context and violates workspace policy.
- the status assertion discards stdout and stderr that would explain the failure.

## Repeating the Source Error

```rust
let value = parse_fixture().unwrap_or_else(|error| panic!("{error}"));
```

The panic repeats the source error without explaining why parsing should have succeeded. Prefer:

```rust
let value = parse_fixture()
    .expect("the checked-in fixture should satisfy the current parser contract");
```

## Testing the Implementation Path

```rust
#[test]
fn calls_the_repository_once() {
    let repository = MockRepository::new();
    list_tasks(&repository);
    assert_eq!(repository.list_call_count(), 1);
}
```

This freezes a call count, not the returned task contract. It can fail after harmless caching or pass while returned tasks are wrong.

## Tests as the Only Invariant

```rust
fn reflect(direction: Vec3, incident: Vec3) -> Vec3 {
    debug_assert!(direction.is_normalized());
    // ...
}
```

If every caller and test must remember that `direction` is normalized, the invariant has no owner. Prefer a trusted `UnitVec3` constructed at the boundary, then test its construction and preservation.

## Dependency-Injection Container

```rust
fn run(context: &ServiceContainer) {
    let clock = context.resolve_clock();
    let repository = context.resolve_repository();
    let client = context.resolve_http_client();
    // ...
}
```

The real dependencies and required capabilities are hidden behind a generic bag. Pass the narrow values or capabilities explicitly and wire them at the composition boundary.

## Trait per Struct

```rust
trait UserRepositoryLike {
    fn find(&self, id: UserId) -> Option<User>;
}
```

A trait is not automatically useful because a concrete `UserRepository` exists. Add the seam when the consumer needs a stable capability, legitimate substitution, or ownership boundary—not merely so a mocking library can replace every struct.

## Convenient Fake, Different System

An in-memory repository that silently allows duplicate keys while production rejects them is not faithful evidence. Run shared contract tests against both implementations or use the real repository hermetically.

## Broad Scenario Test

```rust
#[test]
fn account_workflow_works() {
    // creates an account, changes permissions, imports data, retries a timeout,
    // exports a report, and checks six unrelated outcomes
}
```

One failure cannot identify which promise broke. Split independent behaviors while keeping each complete contract together.

## Invented Behavior

```rust
#[test]
fn retries_exactly_three_times() {
    // No user requirement, public contract, or existing behavior establishes 3.
}
```

Do not turn an implementation guess into a permanent requirement. Find ground truth first.

## False Red

```text
RED: test does not compile because the fixture constructor is misspelled
```

That does not prove the behavior is missing or broken. RED is valid only when the test runs far enough to fail on the intended contract.

## Hidden Machine State

```rust
let config = std::fs::read_to_string("/Users/me/.config/app.toml")
    .expect("config should exist");
```

This test depends on one machine and may read real user data. Use an isolated temporary directory with an explicit fixture.

## Policy Bypass

```rust
#[allow(clippy::unwrap_used)]
#[test]
fn parses_the_fixture() {
    // ...
}
```

Also reject these bypasses:

- lowering a workspace lint because a test is inconvenient
- broadening `allow-unwrap-in-tests`
- skipping `--all-targets` so integration tests are not linted
- moving production code under `#[cfg(test)]` solely to escape production policy

Follow the policy-change procedure in `AGENTS.md` if the rule is genuinely wrong.

## Over-Abstracted Setup

```rust
let context = standard_context();
```

If the tested outcome depends on identity, permissions, time, or stored rows hidden inside `standard_context`, the reader cannot see why the result should occur. Prefer DAMP setup that names contract-bearing facts directly.

## Common Rationalizations

| Rationalization | Reality |
| --- | --- |
| "The test passes." | A passing test can still protect the wrong behavior or only agree with its mock. |
| "Mocking is easier." | Easier setup is not better evidence; try a real local dependency, fake, fixture, or stub first. |
| "Dependency injection means traits everywhere." | Inject the smallest controlled value or capability; keep concrete code when abstraction adds no honest seam. |
| "The test checks the invariant." | A test detects violations after the fact; the design should prevent or reject invalid states at their owner boundary. |
| "It is just a private helper." | Private decomposition is replaceable unless that helper embodies the stable contract. |
| "The error is already printed." | Future readers still need the failed operation and assumption that made the source error relevant. |
| "The test is obvious to me." | The failure must remain self-explanatory to an agent that did not author it. |
| "I can rerun until it passes." | Intermittent success is evidence of nondeterminism, not correctness. |
