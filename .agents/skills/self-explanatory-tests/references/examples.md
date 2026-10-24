# Testing Examples

Use these examples as shapes, not as domain requirements. Preserve the repository's real types and contracts.

## Inline Unit Contract

Production behavior and its unit contract live in the same module:

```rust
/// Select the first enabled account.
#[must_use]
pub fn select_first_enabled(accounts: &[Account]) -> Option<&Account> {
    accounts.iter().find(|account| account.enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_the_first_enabled_account() {
        let accounts = [
            Account::new("disabled", false),
            Account::new("first", true),
            Account::new("second", true),
        ];

        let selected = select_first_enabled(&accounts)
            .expect("the fixture should contain an enabled account");

        assert_eq!(selected.id(), "first");
    }
}
```

The test names the behavior, uses visible setup, asserts the observable result, and gives `expect` the precondition that should have held.

## Diagnostic Assertion

When the default assertion would hide the useful process output, include it:

```rust
let output = command
    .output()
    .expect("the compiled CLI should start successfully");
let stderr = String::from_utf8(output.stderr)
    .expect("the CLI should emit UTF-8 stderr");

assert!(
    !output.status.success(),
    "an invalid limit should fail, but the command succeeded; stderr: {stderr}"
);
assert!(
    stderr.contains("limit must be between 1 and 100"),
    "stderr should explain the accepted range; stderr: {stderr}"
);
```

The failure explains the contract, observed status, and captured evidence. It does not merely say that a boolean was false.

## Error Contract and Test Diagnostic

Keep the product error contract separate from the message that helps a developer repair a failed test:

```rust
let error = parse_limit("500")
    .expect_err("a limit above the documented maximum should be rejected");

assert_eq!(
    error.code(),
    "limit_out_of_range",
    "the parser should preserve the stable machine-readable error code"
);
assert_eq!(
    error.message(),
    "limit must be between 1 and 100; received 500",
    "the CLI error should identify the constraint and offending value"
);
```

The asserted strings belong to the product contract. The assertion messages explain to a developer which part of that contract changed.

## Bug Regression

Protect the violated behavior rather than the internal helper that caused it:

```rust
#[test]
fn rejects_an_end_before_the_start() {
    let error = TimeWindow::new(20, 10)
        .expect_err("an end before the start should be rejected");

    assert_eq!(error, TimeWindowError::EndBeforeStart { start: 20, end: 10 });
}
```

Before fixing the bug, run this test and confirm it fails because the invalid window is accepted or misclassified—not because the fixture does not compile.

## Invariant-Carrying Type

When many callers require the same precondition, move it into a type and test the enforcement boundary:

```rust
let direction = UnitVec3::try_new(Vec3::new(3.0, 4.0, 0.0))
    .expect("the finite nonzero fixture should define a unit direction");

assert!(
    direction.satisfies_invariant(),
    "every successfully constructed UnitVec3 should satisfy its documented unit-length tolerance"
);

let error = UnitVec3::try_new(Vec3::ZERO)
    .expect_err("a zero-length vector should not establish a direction");
assert_eq!(error, UnitVec3Error::ZeroLength);
```

Also test non-finite input, very large and subnormal finite components, every operation that claims to preserve the type, and every parsing or restoration path that can construct it. The wrapper enforces a documented numerical invariant; the tests prove that its enforcement is correct and cannot be bypassed.

## Injected Value

Prefer passing the needed value over inventing an interface:

```rust
let status = session.status_at(deadline);

assert_eq!(
    status,
    SessionStatus::Expired,
    "a session should be expired exactly at its deadline"
);
```

The test controls time directly. A clock trait is unnecessary unless obtaining time is itself a coherent dependency owned by the component.

## Narrow Capability Seam

A workflow that sends reminders may depend on a small `ReminderSink` capability. Production wires it to the real adapter; tests use a maintained in-memory sink and assert the workflow result and delivered reminder.

This is stronger than injecting an HTTP client or application-wide service container. The seam describes what the workflow needs, not how one provider currently performs it.

## Justified Interaction Test

An interaction assertion is appropriate when the interaction is the promise:

```rust
let result = retry_payment(&gateway, payment.clone());

assert_eq!(result, Ok(PaymentStatus::AlreadyCompleted));
assert_eq!(
    gateway.charge_attempts_for(payment.id()),
    0,
    "a completed payment must not issue another external charge"
);
```

The non-interaction protects a dangerous external effect. It is not asserting incidental helper order or internal call count.

## Direct Integration-Test Root

Every direct file under `tests/` is a separate crate root:

```rust
#![cfg_attr(
    test,
    allow(
        clippy::missing_errors_doc,
        clippy::missing_panics_doc,
        reason = "tests report failures rather than document callable contracts"
    )
)]

//! Verify the public configuration-loading contract.

use example::Config;

#[test]
fn public_api_reports_the_missing_required_field() {
    let error = Config::parse("port = 8080")
        .expect_err("configuration without a name should be rejected");

    assert_eq!(error.field(), "name");
    assert_eq!(error.reason(), "required");
}
```

Use integration tests for public or process-level contracts, not merely to place tests in another directory.

## Deterministic Fixture Builder

A builder should make important state explicit and reject incomplete fixtures early:

```rust
let fixture = AccountFixture::new()
    .with_id("account-1")
    .enabled(true)
    .build()
    .expect("the explicit account fixture should satisfy all required fields");
```

The builder removes hidden defaults from each test without hiding the scenario that matters.

## Dependency Fidelity

For a repository contract, prefer a real temporary repository over a call-count mock:

```rust
let directory = tempfile::tempdir()
    .expect("creating an isolated repository directory should succeed");
let repository = Repository::create(directory.path())
    .expect("initializing the fixture repository should succeed");

repository.save(Task::new("task-1", "Ship"))
    .expect("saving the fixture task should succeed");

let tasks = repository.list()
    .expect("listing tasks from the fixture repository should succeed");
assert_eq!(tasks.iter().map(Task::id).collect::<Vec<_>>(), ["task-1"]);
```

This proves the stored and returned state. It does not freeze which internal method or query produced it.
