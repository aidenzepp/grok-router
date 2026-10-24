# Design Pressure and Test Seams

Use this reference when a test is difficult to write, an invariant is repeatedly checked, or a dependency seems to require a large abstraction.

This reference translates design pressure into testing consequences. Use `designing-trustworthy-functions-and-data` to decide whether and how production types, dependencies, effects, or function boundaries should change. Test difficulty is evidence, not automatic authorization for a refactor.

## Build the Obligation Map

Start from promises, not files. One component may own several obligations, and one obligation may cross several components.

| Obligation | Owner | Enforcement | Test evidence | Remaining gap or status |
| --- | --- | --- | --- | --- |
| A direction is normalized | `UnitVec3` | Fallible constructor and private field | Constructor rejection cases plus preservation/property tests | Covered |
| Sessions expire at the deadline | Session policy | Pure comparison using an explicit current time | Boundary cases immediately before, at, and after the deadline | Covered |
| A notification is not duplicated after retry | Delivery workflow | Idempotency key and durable result | Retry test against a hermetic store and recording sink | Covered |
| Concurrent writers preserve ordering | Repository | Not yet established | None | Deferred only with named risk and approval |

The map is not paperwork for every helper. Use it when the work has multiple promises, state, external effects, safety implications, or meaningful failure modes. Its purpose is to reveal obligations that otherwise disappear between unit tests.

## Move Invariants into the Design

Suppose several functions accept `Vec3` but require a normalized vector. Repeating `assert!(vector.is_normalized())` in callers and tests leaves the invariant distributed and easy to forget.

Prefer a `UnitVec3` with a private inner value and a fallible constructor. Downstream functions accept `UnitVec3`, making the requirement visible and preventing unchecked construction through the ordinary API.

The type does not eliminate testing. It changes the test obligations:

- valid normalized vectors are accepted
- zero-length, non-finite, or otherwise invalid vectors are rejected as the contract requires
- every successfully constructed value satisfies the invariant
- operations that return `UnitVec3` preserve the invariant
- parsing and deserialization cannot bypass construction
- unchecked constructors, if genuinely necessary, are isolated behind the appropriate safety boundary

The design enforces the invariant. The tests prove that the enforcement and preservation are correct.

Do not assume a stronger type is automatically safer. First verify the mathematical, lifecycle, or domain claim and confirm that every operation and restoration path can preserve it. For example, crossing two unit vectors does not necessarily produce a unit vector; parallel inputs produce zero. The honest return type is weaker unless the operation performs a separately named fallible normalization.

## Inject the Value Before the Service

If a decision only needs the current time, pass the current time:

```rust
let status = session.status_at(fixed_time);
assert_eq!(status, SessionStatus::Expired);
```

This is dependency injection without an interface. It keeps the domain decision pure and gives the test exact control.

Introduce a clock capability only when the component itself legitimately owns the act of obtaining time across multiple operations. Do not add a `Clock` trait merely because one function reads one timestamp.

Use the same progression for randomness and identifiers: inject the resulting value first, a narrow generator function second, and a trait only when a coherent capability actually needs an owner.

## Make Capability Seams Consumer-Owned

When a workflow must perform an external effect, define the smallest capability the workflow needs in domain terms. For example, a reminder workflow may need `deliver(reminder)`, not an entire HTTP client, SMTP client, provider SDK, or application context.

A good seam:

- names a stable domain capability
- makes failure explicit
- has no methods used only by tests
- can be implemented by the real adapter and a faithful fake
- does not expose transport choreography to the core
- is wired to the real implementation at a visible composition boundary

Do not invent a broad repository, manager, provider, or service abstraction before the consumer contract is known.

An explicit capability may still be nondeterministic, stateful, or unfaithful to production. Keep transaction, lock, ordering, cancellation, and backpressure behavior at the boundary that owns it. A repository mock does not prove database uniqueness, isolation, rollback, or query behavior.

## Choose the Double by Evidence Quality

- Use the real local implementation when it is safe and deterministic.
- Use a hermetic service or maintained fake when real wiring matters but remote effects do not.
- Use a fixture or stub when only a returned result matters.
- Use a recording double or mock when the external interaction is itself the promise—for example, a dangerous charge must not be issued twice.

If the fake owns behavior such as uniqueness, transactions, ordering, or retry semantics, run the same contract suite against the fake and real implementation. Otherwise the fake may make tests green by behaving more conveniently than production.

## Diagnose Test Pain

When testing is difficult, classify the pressure before adding an abstraction:

| Pressure | Likely response |
| --- | --- |
| Invalid values flow through many functions | Introduce a trusted domain type or boundary parser. |
| Time, randomness, or IDs make results unstable | Inject the value or narrow generator. |
| External effects are inseparable from decisions | Separate the pure decision from the effectful adapter. |
| A mock needs many ordered expectations | Test a higher-level outcome or reduce choreography exposed by the seam. |
| The fixture requires unrelated system state | Reconsider component ownership or split the boundary by responsibility. |
| A fake repeatedly disagrees with production | Add shared contract tests or replace it with a higher-fidelity dependency. |

Test pain is evidence, not automatic authorization for a refactor. Keep redesign within the requested scope; otherwise preserve the behavior with a test and report the design debt.

Before separating a decision from an effect, verify that doing so does not create a time-of-check/time-of-use gap or break atomicity. The correct response may be a deterministic inner calculation called from inside a real transaction rather than moving the entire decision outside it.
