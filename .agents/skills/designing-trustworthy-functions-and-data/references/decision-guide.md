# Function Design Decision Guide

Use this guide when several designs appear reasonable. The goal is not maximum purity, abstraction, or type strength. The goal is the smallest truthful contract that preserves the system's real semantics.

## Parse or Validate?

Ask what information the check learns and where that information must remain available.

| Situation | Default design |
| --- | --- |
| Raw input becomes a reusable domain value | Fallible parser returning an invariant-bearing type |
| A collection has a stable structural invariant | Fallible constructor returning a private aggregate |
| The fact matters only during one local calculation | Local checked branch; do not invent a durable type |
| The fact depends on mutable external state | Transaction, guard, lease, version, or stateful operation that keeps proof tied to reality |
| The type cannot preserve the claimed property | Return a weaker type or keep an explicit runtime result |

Prefer `TryFrom`, `FromStr`, a named fallible constructor, or a deserializer that returns the stronger type. A function such as `validate(value) -> Result<(), Error>` is usually incomplete if callers continue passing `value` around in its original weak form.

Before adding a value type, ask:

- Does the guarantee travel through several functions, fields, returns, or collections?
- Will accepting the stronger type remove checks or error variants downstream?
- Is the property intrinsic and stable for the value's lifetime?
- Are all construction, deserialization, restoration, and mutation paths sealable?

If the guarantee exists only inside one private aggregate, parsing directly into that aggregate may be simpler. If an operation returns the element and downstream code benefits from knowing the guarantee, an element type is usually earned.

## Dependency Shape

| What the operation needs | Default shape | Escalate when |
| --- | --- | --- |
| One current fact | Ordinary value such as `now`, `id`, or config | The component legitimately obtains it repeatedly or owns its policy. |
| One narrow behavior | Closure or function argument | It has a stable multi-operation capability or stateful lifecycle. |
| Standard traversal or I/O | Existing `Iterator`, `Read`, `Write`, or ecosystem trait | Domain semantics require a smaller consumer-owned contract. |
| Deterministic in-process collaborator | Concrete type or direct function | Multiple real implementations or ownership boundaries exist. |
| External authority or durable capability | Consumer-owned trait or concrete adapter boundary | A standard interface already communicates the contract better. |

Prefer values over services and existing interfaces over new traits. Do not make a dependency mockable unless substitution improves control or evidence.

## Effect Placement

Ask these questions in order:

1. Can the domain decision be expressed from ordinary supplied values?
2. Which component owns acquiring those values?
3. Must a read and later write observe the same transaction, lock, version, stream, or actor state?
4. Can cancellation, retries, partial failure, or re-entry occur between the proposed layers?
5. Does moving the effect change ordering, atomicity, resource lifetime, or performance?

Separate deterministic calculation from action when the answers permit it. Otherwise keep the decision inside the narrow effectful boundary and test that boundary honestly.

## Invariant Type Test

Introduce a dedicated type only when most answers are yes:

- Would violating the invariant cause a meaningful bug or security/correctness failure?
- Is the invariant stable in the domain rather than incidental to one algorithm?
- Can all ordinary constructors establish it?
- Can every exposed operation preserve it or return a weaker type?
- Can parsing, deserialization, persistence restoration, and foreign input validate it?
- Will downstream code become materially simpler or safer?
- Is construction and access ergonomic enough that callers will not seek bypasses?
- Has the mathematical, lifecycle, or domain claim been independently verified?
- Does construction return the stronger type rather than merely reporting that the weaker input passed?
- Will downstream signatures use the stronger type so the proof is not immediately erased?

Use a checked function, runtime assertion at an honest boundary, or explicit validation result when a wrapper would proliferate states, block legitimate restoration, or certify a condition that can become stale.

## Input Strength

Choose from semantic requirements, not abstract generality:

| Requirement | Likely Rust shape |
| --- | --- |
| One-pass consumption | `impl IntoIterator<Item = T>` or an iterator generic |
| Borrowed contiguous sequence | `&[T]` |
| Mutable contiguous sequence | `&mut [T]` |
| Ownership and growth | `Vec<T>` or another concrete collection |
| Domain behavior and invariants | A cohesive domain type |
| A small subset of unrelated state | Individual values or a narrow capability |
| Many coherent options | A parameter object with explicit required fields and semantic enums |

Do not require a concrete collection out of habit. Do not erase requirements such as multi-pass traversal, stable indexing, ownership, or cache layout merely to accept more callers.

## Extraction Test

A new function is usually justified when at least one is true:

- Its name adds domain or algorithmic vocabulary.
- It centralizes a policy or invariant.
- It separates a meaningful boundary or effect.
- It hides lower-level mechanics from a higher-level operation.
- It forms a stable independently testable contract.
- It replaces repeated logic whose copies could diverge.

Keep the code local when the proposed helper only:

- aliases a field access or obvious expression
- forwards arguments unchanged
- exists to reduce line count
- exposes private choreography for a mock
- requires a reader to chase files to understand one short operation

Comments that divide a function into phases are a review signal, not automatic proof of bad design. A transaction, parser, protocol, or performance-sensitive loop may be clearer as one auditable scope.

## Representation and Invariant Ownership

Before extracting helpers around an existing representation:

1. State the operation the domain needs without naming the current container or algorithm.
2. List every policy the implementation must remember: validation, canonicalization, uniqueness, ordering, searching, indexing, synchronization, or restoration.
3. Remove policies that exist only because of the current representation.
4. Assign each remaining invariant to the smallest natural owner.
5. Prefer a domain type for per-value validity and a standard data structure for relationships between values.
6. Compose those owners when neither can enforce the complete contract alone.
7. Recheck every construction and restoration path; the representation must not be bypassed by deserialization, database loading, or bulk import.

Do not stop at the first local improvement. A standard search can remove handwritten index arithmetic while leaving normalization and sorting as conventions. A map can remove sorting while leaving primitive keys invalid. A validated key can eliminate invalid names while leaving lookup encoded as a scan. Continue until each meaningful invariant has an explicit owner and the obsolete policy machinery can be deleted.

Do not add a wrapper merely to hide a standard collection. An outer domain collection is earned when it owns additional operations, cross-entry invariants, persistence validation, or authority. Otherwise, composing an invariant-bearing key directly with the standard collection may be the clearest complete design.

## Output and Materialization

- Return a complete value when it naturally represents a bounded result.
- Return an iterator or stream when the result can be large, incremental, cancellable, or composable.
- Accept a callback when inversion of consumption is intentional and its error, lifetime, re-entry, and cancellation behavior is defined.
- Mutate an explicitly supplied object when mutation is part of the honest contract and avoids misleading copies or allocations.

Check asymptotic payload size as well as container count. Storing all permutations of an `n`-item input requires `n!` results and usually `Theta(n * n!)` payload storage.

## Final Complexity Budget

For every new abstraction, complete this sentence:

> This exists because it ______, and a direct implementation would ______.

If the answer is only "makes mocking possible," "is cleaner," "might be reused," or "makes the function shorter," the abstraction has not yet justified itself.
