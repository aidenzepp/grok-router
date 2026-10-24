---
name: designing-trustworthy-functions-and-data
description: Use when designing, implementing, refactoring, or reviewing functions, domain values, data structures, and APIs with hidden dependencies, discarded validation knowledge, invalid states, mixed abstraction levels, excessive mocking or traits, unclear effect boundaries, primitive-heavy signatures, or code that is difficult to test and understand locally.
---

# Designing Trustworthy Functions and Data

A trustworthy design makes functions and data tell the same truth. Functions expose what they need, can change, return, and may fail to do. Data preserves facts the system has already established so downstream functions do not re-check them or carry impossible error branches.

The governing balance is:

> Parse raw input into the smallest representation that preserves what was learned, then make every function ask for exactly the data, authority, and guarantees it requires.

## Know Whether the Function Is Honest

“Honest” and “dishonest” are the source video's pragmatic terms, not moral judgments and not standard Rust terminology.

An **honest function** exposes every operationally meaningful input, output, mutation, and external capability through its signature. A caller can see what determines the result and what the operation may affect without opening the body.

A **dishonest function** reaches beyond its signature for meaningful state or authority: a global RNG, system clock, environment variable, service locator, process state, hidden cache, filesystem, network, or mutable singleton. Its apparent contract is smaller than its real contract.

Honesty is related to purity, but they ask different questions:

| Question | Concern |
| --- | --- |
| Pure or impure? | Does evaluation have observable side effects, and is the result referentially transparent? |
| Honest or dishonest? | Does the signature reveal the operation's real inputs, outputs, effects, and authority? |

Therefore:

- `fn total(values: &[i64]) -> i64` can be pure and honest.
- `fn sort(values: &mut [i64])` is impure but honest: its mutation is explicit.
- `fn emit(output: &mut impl Write, event: &Event)` is impure but honest about where it writes.
- `fn now() -> Instant` is dishonest under this model because the clock input is hidden.
- `fn populate(world: &mut World)` is dishonest if it silently uses a global RNG; accepting `&mut impl Rng` makes the randomness explicit and therefore honest, but not pure.

Honesty does not guarantee determinism, safety, correctness, or faithful tests. An explicitly passed HTTP client, clock, RNG, callback, or transaction still performs real effects. Honesty makes those effects visible so ownership, testing, and reasoning can be deliberate.

Dishonesty is also contagious: a function that calls a hidden clock or global service acquires that undeclared dependency itself. Move the dependency into the signature or keep the call inside a deliberately effectful boundary.

Do not try to eliminate all dishonesty. Programs must eventually read clocks, receive framework calls, access files, use networks, and affect the world. Contain that necessary dishonesty in small boundary adapters, then pass explicit values or capabilities inward—unless a transaction, lock, stream, actor, retry, or durable workflow requires the effect and decision to stay together.

Draw the line at operationally meaningful dependencies. Do not pollute signatures with incidental physical influences such as allocation heat, scheduler timing that is irrelevant to the contract, or CPU temperature.

- Read [references/examples.md](references/examples.md) when implementing or refactoring.
- Read [references/non-examples.md](references/non-examples.md) when reviewing abstractions, dependency injection, strong types, parameter objects, or function extraction.
- Read [references/decision-guide.md](references/decision-guide.md) when the correct effect boundary, input shape, invariant owner, or extraction boundary is uncertain.
- Read [references/design-transformations.md](references/design-transformations.md) for paired Rust refactors from misleading or overpowered functions to trustworthy designs.
- Read [references/parse-dont-validate.md](references/parse-dont-validate.md) when validation returns `()`, checks recur, domain functions retain impossible error cases, or raw primitives cross trusted boundaries.
- Read [references/video-notes.md](references/video-notes.md) for the source video, timestamped notes, qualifications, and corrections behind this skill.

## Design the Contract Before the Body

Start with the call site that should exist. State the operation in one sentence, then inventory:

- required values and semantic guarantees
- mutable inputs and observable outputs
- errors, absence, and partial failure
- preconditions, postconditions, and preserved invariants
- time, randomness, environment, filesystem, network, process, framework, or global state
- ownership, lifetime, ordering, transaction, cancellation, backpressure, and performance requirements

Do not make the signature describe the current implementation by accident. Make it describe the durable contract a caller must understand.

## Make Dependencies and Effects Visible

Keep deterministic domain decisions separate from environmental work when that separation preserves the real semantics. Framework callbacks and entry points should translate framework-owned control flow into application-owned operations quickly.

Prefer the smallest honest input:

1. Pass an ordinary value when the function only needs a snapshot such as `now`, an identifier, or configuration.
2. Pass a closure for one narrow operation that genuinely varies.
3. Use an existing capability such as `Iterator`, `Read`, `Write`, or `Rng` when its semantics fit.
4. Keep a concrete collaborator when substitution adds no useful control.
5. Define a consumer-owned trait when there is a stable capability, real alternative implementations, or external authority to isolate.

Wire environmental implementations at a visible composition boundary. A service locator, generic context, or dependency bag hides dependencies even when it is passed as an argument.

Place effects at the narrowest correct consistency boundary—not mechanically at the root of the call tree. Keep reads, decisions, and writes together when a transaction, lock, stream, retry, actor, or durable workflow requires their ordering or shared lifetime.

## Ask for the Right Amount of Power

Ask for the least powerful interface that still communicates the real semantic and performance contract:

- an iterator when only one-pass traversal is required
- a slice when contiguous borrowed storage is required
- a concrete collection when ownership, random access, capacity, mutation, or representation is part of the operation
- a cohesive domain object when its behavior or invariants matter
- individual values when passing the whole object would expose unrelated authority

Do not generalize a parameter merely because today's loop happens to compile against a weaker trait. Multi-pass traversal, stable references, ordering, allocation, vectorization, and asymptotic cost are legitimate requirements.

Use a parameter object when its fields form one coherent concept or meaningful option set. It is not a cure for an operation that owns too many unrelated responsibilities.

## Parse at Boundaries and Preserve What You Learn

Treat parsing broadly: it transforms less-structured input into more-structured data or fails. The input may be text, but it may also be a primitive, collection, deserialized record, database row, framework object, or foreign type.

Prefer:

```text
raw input -> fallible parser -> invariant-bearing value -> infallible domain operations
```

over:

```text
raw input -> validate and return () -> keep using raw input -> re-check or propagate impossible errors
```

A `validate_*` or `is_valid` function that returns no refined value is a design smell when the caller continues using the original weak type. The check learned something, but the program discarded the proof. Return a type that preserves the fact, or parse directly into the aggregate that owns it.

Choose the proof owner deliberately:

- Use a value type when the guarantee is intrinsic to the value, appears across functions, or should travel through returns, fields, collections, and APIs.
- Use a private aggregate when the rule is meaningful only as membership or state inside that aggregate and every construction and mutation path remains sealed.
- Use a transaction, guard, lease, capability, or stateful operation when the fact depends on mutable external state and can become stale. Do not encode a temporary observation as a permanent value guarantee.

Keep fallibility at raw boundaries such as CLI parsing, configuration, HTTP, files, databases, deserialization, FFI, and framework input. Once parsing succeeds, core operations should not retain the same validation error unless they genuinely accept weaker input again.

Parsing does not justify a wrapper automatically. The parsed representation must remove checks, prevent misuse, clarify meaning, or make downstream operations total. Avoid generic `Validated<T>` markers that fail to name the established fact.

## Encode Only Invariants the Design Can Preserve

A dedicated type is earned when an invariant is consequential, stable, repeatedly needed, and enforceable through every construction and mutation path.

For an invariant-bearing type:

- keep the representation private
- make construction validate or establish the invariant
- define failure precisely
- validate parsing, deserialization, database restoration, and foreign input
- expose only operations that preserve the invariant, or return a weaker type
- make legitimate access to the underlying value ergonomic
- test construction, rejection, preservation, and bypass paths

The parser should return the invariant-bearing type rather than `()`. Once constructed, functions should accept that type and omit redundant checks and corresponding impossible error variants.

Never strengthen a parameter or return type because a theorem, lifecycle fact, or domain assumption merely sounds plausible. Verify that the implementation can prove and preserve the claim. Prefer runtime validation or a weaker result when the type system would encode a lie.

## Keep One Conceptual Level Per Function

A function may orchestrate several sibling operations or implement one detailed operation. It should not repeatedly alternate between domain intent and low-level bytes, indices, protocol mechanics, storage representation, or handwritten algorithms.

Extract a cohesive block when the new function:

- names a real operation or policy
- encapsulates an invariant or representation rule
- isolates a meaningful effect
- removes distracting lower-level machinery
- creates a useful contract or testing seam
- has meaningful reuse

Keep short local control flow together when extraction would only add navigation. A helper whose name restates its body has not created an abstraction.

Prefer standard-library and established repository operations over reimplementing algorithms inside domain code. Repeated normalization, ordering, lookup, or parsing rules may reveal a missing data structure rather than a need for more scattered helpers.

## Let the Representation Enforce the Contract

Do not assume the current representation deserves to survive the refactor. Before improving its helpers or algorithms, separate:

- behavior the domain actually requires
- constraints imposed only by the current representation
- per-value invariants such as validity, canonical form, or units
- relationship invariants such as uniqueness, association, ordering, or queue discipline

Choose the smallest natural owner for each real invariant. A domain type can make each value valid by construction. A standard collection can provide relationships and operations such as unique-key lookup. Compose those owners when the contract contains both kinds of invariant; solving one does not solve the other.

For example, replacing a sorted sequence with a map can remove sorting and search policy, but a string-keyed map still does not prove that its keys are valid or canonical. Conversely, a validated key type does not by itself provide association or efficient lookup. A map keyed by the validated type lets both structures do the work they naturally own.

Prefer this structural composition over comments, call-site conventions, duplicated normalization, defensive checks, wrapper ladders, or custom algorithms. Add an outer domain collection type only when it owns additional behavior or policy beyond what its field types already guarantee.

Judge simplicity by the total policy burden left in the system, not by the number of types. One small invariant-bearing type plus one standard collection may eliminate more complexity than a primitive collection surrounded by helpers.

## Make Every Abstraction Earn Its Cost

Before keeping a new function, wrapper, trait, generic, parameter object, callback, fake, or plan type, name what it buys:

- a truthful contract
- prevention of an important invalid state
- controlled nondeterminism or external authority
- preservation of ownership or consistency
- reusable policy
- materially better local reasoning or testing

If it buys none of these, remove it. Test deterministic code through values and results before adding mocks. Test databases, networks, filesystems, transactions, and framework wiring with the highest-fidelity safe implementation practical; an injected trait does not reproduce external semantics.

## Review the Result

Before completion:

1. Read the call site without opening the implementation. Confirm that intent and required guarantees are apparent.
2. Confirm every meaningful dependency and effect is visible without a service locator or hidden global.
3. Confirm the function receives no unrelated authority.
4. Confirm every strong type states a true invariant and all construction paths preserve it.
5. Search for validation returning `()`, Boolean validity checks, and repeated assertions. Confirm each learned fact is either preserved in a type or intentionally confined to an aggregate or live consistency boundary.
6. Confirm errors occur where weak input becomes strong data, not throughout functions that already receive proven values.
7. Name the owner of every remaining invariant. Reject invariants owned only by comments, duplicated helpers, or caller discipline.
8. Confirm the representation does not preserve accidental requirements such as sorting or indexing when the domain only needs association or lookup.
9. Confirm transaction, lock, lifetime, ordering, cancellation, and performance behavior survived decomposition.
10. Remove mechanical helpers, speculative traits, wrapper ladders, and interaction-only test seams.
11. Use `self-explanatory-tests` to prove the contract, invariant boundaries, deterministic core, and effectful adapters at appropriate fidelity.
12. Run the repository's focused checks and normal verification commands.
