# Trustworthy Function Examples

Use these as reasoning patterns, not APIs to copy mechanically. Preserve the project's actual domain, ownership, error, and performance contracts.

For paired before-and-after Rust refactors, read [design-transformations.md](design-transformations.md).

## Classify the Real Contract

```rust
// Pure and honest: all inputs and the returned output are visible.
fn total(values: &[i64]) -> i64 {
    values.iter().sum()
}

// Impure and honest: mutation is explicit in the signature.
fn sort_by_priority(tasks: &mut [Task]) {
    tasks.sort_by_key(Task::priority);
}

// Impure and honest: external output is an explicit capability.
fn write_receipt(
    output: &mut impl Write,
    receipt: &Receipt,
) -> io::Result<()> {
    writeln!(output, "{receipt}")
}

// Dishonest: the signature hides time as a real input.
fn session_status(session: &Session) -> SessionStatus {
    status_at(session, Instant::now())
}
```

“Honest” does not mean “pure.” It means the signature tells the truth about the operation. The boundary adapter may obtain the current time, then pass that value to an honest domain decision.

## Pass the Value the Decision Needs

A domain rule that compares a deadline needs a timestamp, not permanent access to a clock:

```rust
fn status_at(session: &Session, now: Instant) -> SessionStatus {
    if now >= session.expires_at() {
        SessionStatus::Expired
    } else {
        SessionStatus::Active
    }
}

fn refresh(clock: &impl Clock, session: &Session) -> SessionStatus {
    status_at(session, clock.now())
}
```

The environmental adapter owns obtaining time. The rule receives an ordinary controlled value.

## Pass a Real Nondeterministic Capability

A generator that consumes randomness across many operations legitimately accepts an RNG:

```rust
fn populate<R: Rng + ?Sized>(
    world: &mut ParticleWorld,
    count: usize,
    rng: &mut R,
) {
    for _ in 0..count {
        world.push(Particle::new(random_velocity(rng)));
    }
}

fn run(world: &mut ParticleWorld) {
    let mut rng = StdRng::from_os_rng();
    populate(world, 100, &mut rng);
}
```

Production chooses an unpredictable seed. Tests and reproducible simulations choose a known seed. No repository-specific `RandomService` trait is necessary.

## Carry a Proven Invariant

```rust
#[derive(Clone, Copy)]
struct UnitVec3(Vec3);

impl UnitVec3 {
    fn try_new(value: Vec3) -> Result<Self, UnitVec3Error> {
        if !value.is_finite() {
            return Err(UnitVec3Error::NonFinite);
        }

        // Scale first so finite large or subnormal components cannot overflow
        // or underflow while the length is calculated.
        let scale = value.abs().max_element();
        if scale == 0.0 {
            return Err(UnitVec3Error::ZeroLength);
        }

        let scaled = value / scale;
        let unit = scaled / scaled.length();
        if !unit.is_finite() {
            return Err(UnitVec3Error::NonFinite);
        }

        Ok(Self(unit))
    }

    fn into_inner(self) -> Vec3 {
        self.0
    }
}

fn reflect(incident: Vec3, normal: UnitVec3) -> Vec3 {
    let normal = normal.into_inner();
    incident - 2.0 * incident.dot(normal) * normal
}
```

`UnitVec3` owns a stable mathematical requirement and prevents every downstream caller from remembering it independently.

An operation must return the type it can prove:

```rust
fn cross(left: UnitVec3, right: UnitVec3) -> Vec3 {
    left.into_inner().cross(right.into_inner())
}

fn normalized_cross(
    left: UnitVec3,
    right: UnitVec3,
) -> Result<UnitVec3, UnitVec3Error> {
    UnitVec3::try_new(cross(left, right))
}
```

The ordinary cross product is not necessarily unit length. Parallel unit vectors produce zero.

## Parse a Primitive into a Domain Value

```rust
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct NonPrivilegedPort(u16);

impl TryFrom<u16> for NonPrivilegedPort {
    type Error = InvalidPort;

    fn try_from(port: u16) -> Result<Self, Self::Error> {
        if port < 1_024 {
            return Err(InvalidPort { port });
        }

        Ok(Self(port))
    }
}

struct PortPool {
    free: BTreeSet<NonPrivilegedPort>,
}

impl PortPool {
    fn release(&mut self, port: NonPrivilegedPort) -> bool {
        self.free.insert(port)
    }

    fn allocate(&mut self) -> Option<NonPrivilegedPort> {
        self.free.pop_first()
    }
}
```

The raw `u16` is parsed once at configuration, CLI, database, or network boundaries. The parser returns the proof-bearing value. Pool operations no longer repeat the check or return `InvalidPort`, and an allocated value retains the guarantee when passed elsewhere.

If the rule were meaningful only inside `PortPool` and elements never crossed its boundary, a private aggregate that parses raw ports itself could own the invariant without a separate element type. Here the element is both accepted and returned, so preserving the guarantee is useful.

## Preserve the Transaction Boundary

```rust
fn transfer(
    mut transaction: Transaction<'_>,
    command: Transfer,
) -> Result<Receipt, TransferError> {
    let accounts = transaction.lock_accounts(command.from, command.to)?;
    let plan = plan_transfer(&accounts, &command)?;

    transaction.apply(plan)?;
    transaction.append_ledger_entry(&command)?;
    transaction.commit()
}
```

`plan_transfer` can be deterministic, but acquiring locks, deciding from current state, writing, and committing remain inside one consistency boundary. Hoisting all effects to `main` would make the design less correct.

## Request the Capability Actually Used

```rust
fn total(values: impl IntoIterator<Item = i64>) -> i64 {
    values.into_iter().sum()
}

fn checksum(bytes: &[u8]) -> Checksum {
    // Contiguous byte access is part of this algorithm's contract.
}
```

The first operation only traverses values. The second honestly asks for contiguous bytes.

## Use a Cohesive Parameter Object

```rust
struct TimerOptions {
    id: TimerId,
    interval: Duration,
    first_tick: FirstTick,
    repetition: Repetition,
}

set_timer(TimerOptions {
    id: TimerId::new("refresh")?,
    interval: Duration::from_secs(5),
    first_tick: FirstTick::AfterInterval,
    repetition: Repetition::Forever,
});
```

The fields describe one timer policy. Enums replace positional booleans and rule out ambiguous combinations.

## Keep Domain Intent at One Level

```rust
fn is_asset_of_type(
    &self,
    name: &AssetName,
    expected: AssetType,
) -> bool {
    self.types_by_name
        .get(name)
        .is_some_and(|asset_type| *asset_type == expected)
}
```

`AssetName` owns validation and canonical casing. `HashMap<AssetName, AssetType>` owns unique-key association and lookup. The domain function does not lowercase bytes, preserve a sorted sequence, or implement binary search.

## Stream Results That Should Not Be Materialized

```rust
fn write_permutations(
    input: &str,
    output: &mut impl Write,
) -> io::Result<()> {
    for permutation in permutations(input) {
        writeln!(output, "{permutation}")?;
    }

    Ok(())
}
```

An iterator preserves separation and caller control without allocating every factorial-sized result at once.
