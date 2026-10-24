# Trustworthy Function Non-Examples

These patterns often look modular or strongly typed while making behavior less truthful, less testable, or harder to understand.

For paired before-and-after Rust refactors, read [design-transformations.md](design-transformations.md).

## Dishonest Hidden Ambient State

```rust
fn populate(world: &mut ParticleWorld, count: usize) {
    let velocity = GLOBAL_RNG.random();
    // ...
}
```

The result depends on mutable state absent from the signature. Other callers or threads can change behavior at a distance.
Passing an RNG explicitly would make the effect honest, not pure.

## Service Locator Disguised as Injection

```rust
fn populate(context: &AppContext, world: &mut ParticleWorld) {
    let rng = context.services.randomness.global_rng();
    // ...
}
```

Passing the global-shaped object does not reveal the function's actual requirements and grants access to unrelated authority.

## Trait per Deterministic Helper

```rust
trait AdditionService {
    fn add(&self, left: i32, right: i32) -> i32;
}

trait StringTrimmer {
    fn trim<'a>(&self, value: &'a str) -> &'a str;
}
```

These traits manufacture mocks rather than express real substitution or external authority. Call deterministic operations directly and test their results.

## A False Type-Level Guarantee

```rust
fn cross(left: UnitVec3, right: UnitVec3) -> UnitVec3 {
    // Incorrect: the cross product has magnitude sin(theta).
}
```

Parallel unit vectors produce zero. A strong return type would certify a mathematical falsehood. Return `Vec3`, or make normalization a separately named fallible operation.

## Validate and Forget

```rust
fn validate_port(port: u16) -> Result<(), InvalidPort> {
    if port < 1_024 {
        return Err(InvalidPort { port });
    }

    Ok(())
}

fn release(&mut self, port: u16) -> Result<bool, InvalidPort> {
    validate_port(port)?;
    Ok(self.free.insert(port))
}
```

The check learns that the port is non-privileged, returns `()`, and keeps the original `u16`. Every later function must either trust undocumented history, repeat the check, or retain an error case that should already be impossible. Parse the primitive into a `NonPrivilegedPort` and make trusted operations accept that type.

Do not mechanically wrap every checked value. If the fact is temporary, contextual, or owned completely by one sealed aggregate, preserve it in that boundary instead. The problem is discarded knowledge, not the spelling of the helper.

## A Stale Proof Token

```rust
struct InitializationCompleted;

fn initialize() -> InitializationCompleted {
    InitializationCompleted
}

fn use_resource(_: InitializationCompleted) {
    // Assumes the resource is still initialized.
}
```

An empty token proves nothing if it can be copied, constructed elsewhere, or outlive the state it represents. A mutex guard works because ownership and lifetime remain connected to the live lock. Proof-bearing types must preserve that connection.

## Effects Moved Outside Their Consistency Boundary

```rust
let accounts = repository.load_accounts(command.from, command.to)?;
let plan = plan_transfer(&accounts, &command)?;
repository.apply(plan)?;
```

Another writer may change the accounts between the check and the write. Purity has introduced a time-of-check/time-of-use bug. Load or lock, decide, write, and commit inside the required transaction.

## Parameter Bag Instead of a Concept

```rust
struct DoEverythingOptions {
    user_id: String,
    raw_sql: String,
    retry_count: usize,
    send_email: bool,
    output_path: PathBuf,
    logger: Logger,
}
```

The fields do not form one coherent value. The bag hides excessive responsibility and permits combinations that may not make sense.

## Primitive and Boolean Soup

```rust
set_timer("refresh", 5, true, false, true);
```

The call site does not communicate units or boolean meaning. Use semantic types or one cohesive options object when the configuration is genuinely one concept.

## Mechanical Helper Explosion

```rust
fn get_name(user: &User) -> &str {
    &user.name
}

fn name_is_empty(user: &User) -> bool {
    get_name(user).is_empty()
}
```

The helpers add navigation without adding vocabulary, policy, safety, reuse, or a meaningful testing boundary.

## Wrapper Ladder

```rust
fn handle(request: Request) -> Result<Response, Error> {
    process_request(request)
}

fn process_request(request: Request) -> Result<Response, Error> {
    execute_request(request)
}
```

Pass-through layers make local reasoning worse unless they translate a boundary, own policy, or create an actual contract.

## Weakest Syntax, Missing Semantics

```rust
fn repeatedly_partition(values: impl Iterator<Item = Item>) {
    // The algorithm actually needs multiple passes and stable indexing.
}
```

The weakest type that compiles is not automatically the most truthful. Ask for a slice or owned collection when the algorithm genuinely needs its guarantees.

## Callback as Accidental Authority

```rust
fn for_each_item(items: &[Item], callback: impl FnMut(&Item)) {
    // The callback may block, re-enter, retain references, or perform I/O.
}
```

A callback makes the authority explicit but does not make the operation pure. Specify cancellation, re-entrancy, lifetime, error, ordering, and backpressure semantics when they matter; prefer an iterator when caller-controlled consumption is the better contract.

## Mocked External Semantics

```rust
let repository = MockRepository::new();
repository.expect_save().returning(|_| Ok(()));
```

This cannot prove uniqueness constraints, transaction isolation, SQL behavior, rollback, ordering, or connection failures. Use real hermetic adapters and shared contract tests where those behaviors matter.

## Common Rationalizations

| Rationalization | Reality |
| --- | --- |
| "Everything should be injectable." | Inject caller-owned policy, nondeterminism, or external authority; keep ordinary implementation details local. |
| "A trait makes it testable." | Deterministic concrete code is already testable through inputs and outputs. |
| "Invalid states should always be impossible." | Only encode invariants the type can establish and preserve without making legitimate restoration or evolution impossible. |
| "Effects belong at the top." | Effects belong at the narrowest boundary that preserves consistency, ownership, and lifecycle semantics. |
| "The weakest interface is most reusable." | The truthful interface includes semantic and performance guarantees, not only methods called today. |
| "Small functions are easier to read." | Functions are easier to read when they encapsulate meaningful concepts; wrapper ladders increase cognitive load. |
| "The mock passed." | A mock proves agreement with the mock, not the external system. |
