# Source Video and Analysis Notes

## Primary Source

[How to write the perfect function — Logan Smith](https://www.youtube.com/watch?v=2OMRWPOSw9s) (47:32)

These notes synthesize the complete English transcript, the video, and its code slides. They preserve the useful reasoning while identifying claims that should not become template policy. The video is linked rather than vendored because the skill needs the source and timestamp map, not a large copyrighted media file.

## Central Thesis

Functions are cognitive boundaries. They do not add computational power; they make complex systems manageable by enabling local reasoning, named abstraction, focused testing, and reuse. Extraction can be valuable before duplication when it creates a real concept, but one occurrence is a prompt for judgment rather than an extraction quota.

The title is aspirational. The speaker presents current opinions, not a universally solved formula.

## Guideline 1: Make Outside Access Visible

The video calls a function "honest" when everything it reads or changes is reachable through its arguments. Explicit mutation can therefore be honest, while reading a clock or global RNG without declaring it is not.

Key examples:

- In-place sorting is explicit mutation; a clock read has a hidden environmental input.
- A required-assets accessor secretly consults global "already loaded" state and surprises a later caller.
- Permutation generation is separated from console or file output. An iterator or callback avoids materializing every result.
- A particle system accepts an RNG rather than reaching into a global generator, enabling reproducibility and deterministic debugging.
- Framework callbacks and entry points remain small and delegate quickly into application-owned functions.

Important limits:

- Environmental effects are necessary and often constitute the program's purpose.
- Passing a capability exposes an effect; it does not make the function deterministic or pure.
- Effects cannot always move to the call-tree root. Transactions, locks, retries, streams, actors, and durable workflows may require reads, decisions, and writes to remain interleaved.
- The durable rule is to place effects at the narrowest correct consistency boundary.

## Guideline 2: Design the Signature for the Caller

A signature should communicate required values and guarantees, prevent consequential misuse when practical, and avoid demanding unrelated power.

Key examples:

- A primitive-heavy timer call becomes a cohesive parameter object with readable fields.
- A function that only traverses integers does not require a concrete `vector`; a view or iterable may be more truthful.
- A mutex guard can act as proof that a lock remains held because its lifetime is connected to the lock.
- A reflection calculation accepts a normalized-vector type because its mathematics requires a unit normal.
- Passing an entire object when two unrelated fields suffice is compared to handing a cashier a whole wallet instead of a payment card.

Important limits:

- Parameter objects and strong types add boilerplate and may add copying, conversion, serialization, and evolution costs.
- A whole domain object is appropriate when its behavior, authorization, ownership, or invariants are part of the operation.
- The least syntactically sufficient interface can still be semantically too weak. Algorithms may require stable references, ordering, contiguity, ownership, random access, or multiple passes.
- Empty proof tokens do not establish freshness or continued validity unless construction, ownership, and lifetimes preserve the proven condition.

## The Normalized-Vector Example

The reflection formula requires a unit-length normal. Accepting an arbitrary vector hides that precondition. Normalizing inside every reflection call repeats work and introduces a failure policy for zero, non-finite, overflowing, or underflowing input.

A private `UnitVec3` wrapper with fallible construction can establish the invariant at the best-informed boundary. Reflection then accepts the trusted type, while conversion back to the underlying vector remains easy.

This does not imply that every condition belongs in a type. The speaker explicitly says it is neither wise nor feasible to encode every combination of preconditions and postconditions and admits there is no universal placement line.

### Necessary correction

The video suggests that the cross product of two normalized vectors could return a normalized vector. This is mathematically false:

```text
|a x b| = |a| |b| sin(theta)
```

For unit inputs the magnitude is `sin(theta)`. Parallel inputs produce zero; only perpendicular unit inputs produce unit magnitude. The ordinary operation must return an unconstrained vector, or normalization must be a separately named fallible step.

The video also entertains public inheritance or easy implicit conversion from the stronger vector type. A mutable base interface could violate the stronger invariant while leaving the static type unchanged. Rust should use private composition and invariant-preserving operations.

Floating-point normalization is only approximate and requires an explicit policy for tolerance, zero, non-finite values, overflow, and underflow.

## Guideline 3: Keep One Level of Abstraction

Every line in a function should contribute at the same conceptual level. Domain orchestration should not repeatedly descend into byte manipulation, handwritten binary search, storage representation, or protocol mechanics and then return to domain intent.

The asset example mixes ASCII lowercasing, a buggy handwritten binary search, absence handling, and a domain type check. The refactor first uses named helpers and a standard algorithm, then recognizes that case normalization plus ordered insertion and lookup form an ad hoc case-insensitive map. Encapsulating that data structure prevents insertion and lookup policy from diverging.

Important limits:

- Several statements can remain at one abstraction level.
- A standard algorithm followed by checking its result need not be wrapped again.
- Section comments are a review signal, not proof that extraction is required.
- Parsers, transactions, state machines, protocols, and hot loops may be easier to audit when short related phases remain in one scope.
- Extracting every expression produces wrapper ladders and worse local reasoning.

## Timestamp Map

| Time | Topic |
| --- | --- |
| 0:37–1:45 | "Write the functions you want to see" and the opinionated framing. |
| 2:52–7:08 | Local reasoning, abstraction, testability, reuse, and cognitive complexity. |
| 7:15–7:44 | Three-guideline framing. |
| 10:03–12:50 | Honest versus hidden inputs, outputs, and propagation through callers. |
| 13:17–14:46 | Pragmatic limits, explicit mutation, and multiple output channels. |
| 14:49–15:45 | Required-assets accessor coupled to hidden loaded-state. |
| 15:50–18:01 | Necessary effects and thin framework-owned callbacks. |
| 18:20–20:20 | Permutation logic separated from console and file I/O. |
| 20:21–21:47 | Factorial materialization cost, callbacks, and Rust iterators. |
| 21:48–25:56 | Global PRNG replaced by explicitly supplied RNG state. |
| 26:02–27:32 | Caller empathy, primitive-heavy calls, parameter objects, and their cost. |
| 27:34–28:50 | Concrete containers versus views and iterables; request real guarantees. |
| 28:54–29:59 | Ordering proof token and the stronger mutex-guard example. |
| 30:00–35:38 | Reflection and normalized-vector invariant. |
| 35:41–36:33 | Cross-product proposal, mathematical mistake, and explicit slippery-slope warning. |
| 36:44–38:32 | Single responsibility, calculation versus action, no raw loops, and one abstraction level. |
| 38:34–40:54 | Mixed-level asset lookup and section-comment smell. |
| 41:00–44:11 | Hierarchical outline, discovered binary-search bug, and standard algorithm. |
| 44:13–46:09 | Case-insensitive map as the missing data structure. |
| 46:14–46:50 | Final synthesis. |

## Claims Not Adopted as Rules

- Extract after the first occurrence.
- Move every effect to the topmost caller.
- Turn every long positional call into a parameter object.
- Replace every non-trivial loop with a helper.
- Treat every section comment as proof of a bad function.
- Assume a passed dependency is deterministic merely because it is explicit.
- Treat an injected repository mock as evidence of real database behavior.
- Encode every precondition or postcondition in a distinct type.

The skill adopts the video's direction while adding consistency, lifecycle, mathematical-verification, and abstraction-cost guardrails.
