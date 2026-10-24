# Design Transformations

These paired Rust examples show how to turn misleading, overpowered, or mixed-level function designs into truthful contracts.

The snippets are independent shapes, not one compilable crate. Adapt domain names and error types, but preserve the contract distinction each pair demonstrates.

## 1. Honest Does Not Mean Pure

A function can mutate and still be honest when the mutation is declared:

```rust
fn clear<T>(values: &mut Vec<T>) {
    values.clear();
}

fn remove_disabled(accounts: &mut Vec<Account>) -> usize {
    let previous_len = accounts.len();
    accounts.retain(Account::is_enabled);
    previous_len - accounts.len()
}
```

Both functions are impure because they mutate. Both are honest because `&mut` exposes exactly what they may change. `remove_disabled` also returns the number removed, demonstrating that mutation and a return value can be separate, visible output channels.

By contrast, this function's purpose is to read a hidden environmental input:

```rust
fn current_time() -> Instant {
    Instant::now()
}
```

Under this skill's vocabulary, `current_time` is necessarily dishonest: the clock that determines its result is absent from the signature. Keep this operation at a boundary and pass its result inward:

```rust
fn session_status(session: &Session, now: Instant) -> SessionStatus {
    if now >= session.expires_at() {
        SessionStatus::Expired
    } else {
        SessionStatus::Active
    }
}

fn handle_request(session: &Session) -> Response {
    let now = Instant::now();
    let status = session_status(session, now);
    Response::from(status)
}
```

The framework boundary remains deliberately dishonest and effectful. The domain decision becomes honest and deterministic. If obtaining time is itself a caller-owned policy, pass a narrow clock capability to this adapter; do not pass an application-wide context merely to reach a clock.

## 2. An Accessor with Hidden Global State

### Non-example

The name promises required assets, but the implementation silently removes assets found in an ambient loaded-assets registry:

```rust
fn required_assets(manifest: &Manifest) -> impl Iterator<Item = &AssetId> {
    manifest
        .required_assets()
        .iter()
        .filter(|id| !loaded_assets().contains(id))
}
```

A caller cannot tell that global load state changes the result.

### Better

Make the distinction and dependency explicit:

```rust
fn required_assets(manifest: &Manifest) -> &[AssetId] {
    manifest.required_assets()
}

fn missing_required_assets<'a>(
    manifest: &'a Manifest,
    loaded: &'a HashSet<AssetId>,
) -> impl Iterator<Item = &'a AssetId> {
    manifest
        .required_assets()
        .iter()
        .filter(|id| !loaded.contains(id))
}
```

The caller chooses whether it needs the complete requirement or the currently missing subset.

## 3. Permutations Mixed with I/O

### Non-example

The algorithm chooses where results go:

```rust
fn print_permutations(input: &str) {
    let mut state = PermutationState::new(input);

    while let Some(permutation) = state.next() {
        println!("{permutation}");
    }
}
```

The permutation logic cannot be reused for a file, network stream, search, or test without capturing process output.

### Bounded materialization when the complete result is the contract

```rust
fn all_permutations(
    input: &str,
    result_limit: usize,
) -> Result<Vec<String>, TooManyPermutations> {
    let mut results = Vec::new();

    for permutation in PermutationState::new(input) {
        if results.len() == result_limit {
            return Err(TooManyPermutations { result_limit });
        }
        results.push(permutation);
    }

    Ok(results)
}

fn write_all_permutations(
    input: &str,
    output: &mut impl Write,
) -> Result<(), WritePermutationsError> {
    for permutation in all_permutations(input, 10_000)? {
        writeln!(output, "{permutation}")?;
    }

    Ok(())
}
```

The algorithm is honest, the output destination is explicit, and the materialization limit is part of the contract. An unbounded `Vec` may separate calculation from output, but storing every result usually requires `Theta(n * n!)` payload space and should not be presented as generally safe.

### Better when materialization is too expensive

```rust
fn permutations(input: &str) -> PermutationState<'_> {
    PermutationState::new(input)
}

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

A callback is one explicit consumer-injection shape. In Rust, an iterator often provides the more natural contract because the caller can compose, stop early, and control consumption without allocating all results.

## 4. `ParticleWorld::populate` and the Global RNG

### Non-example

```rust
impl ParticleWorld {
    fn populate(&mut self, count: usize) {
        let mut rng = rand::rng();

        self.particles.extend(
            std::iter::repeat_with(|| Particle {
                position: random_vec3(&mut rng),
                velocity: random_vec3(&mut rng),
                scale: rng.random(),
            })
            .take(count),
        );
    }
}

let mut world = ParticleWorld::default();
world.populate(42);
```

`populate` appears to depend only on the world and count, but it also consumes hidden thread-local RNG state. Other random calls change its result at a distance, and callers cannot choose reproducibility.

### Honest Rust version

```rust
impl ParticleWorld {
    fn populate<R: Rng + ?Sized>(
        &mut self,
        count: usize,
        rng: &mut R,
    ) {
        self.particles.extend(
            std::iter::repeat_with(|| Particle {
                position: random_vec3(rng),
                velocity: random_vec3(rng),
                scale: rng.random(),
            })
            .take(count),
        );
    }
}

let mut rng = ChaCha12Rng::try_from_os_rng()
    .map_err(PopulateError::EntropyUnavailable)?;
let mut world = ParticleWorld::default();
world.populate(42, &mut rng);
```

Production selects an unpredictable seed at the boundary and exposes entropy acquisition failure. Tests, simulations, and bug reproductions select a known seed with a named portable algorithm:

```rust
let mut rng = ChaCha12Rng::seed_from_u64(0x5EED);
let mut world = ParticleWorld::default();

world.populate(42, &mut rng);

assert_eq!(world, expected_world());
```

This is dependency injection without a DI framework or repository-specific RNG trait. `populate` is still impure—it mutates the world and generator—but both mutations are honest. Durable replay data must also record or pin the RNG algorithm and relevant distribution/code versions; a seed alone does not make an evolving dependency graph a permanent replay format.

## 5. Primitive and Boolean Timer Arguments

### Non-example

```rust
set_timer(None, "refresh", 5.0, true, false, true)?;
```

The call site does not identify the time unit or what any Boolean means.

### Better

```rust
set_timer(TimerOptions {
    context: None,
    id: TimerId::new("refresh")?,
    interval: Duration::from_secs(5),
    first_tick: FirstTick::AfterInterval,
    repetition: Repetition::Once,
    retrigger: RetriggerPolicy::Restart,
})?;
```

The parameter object describes one timer configuration. Semantic types replace positional primitives and Boolean blindness.

### Overcorrection

```rust
fn set_timer(application: &Application) -> Result<TimerHandle, TimerError> {
    // Reads six fields from a much larger object.
}
```

Passing the entire application grants unrelated knowledge and authority. Use a cohesive parameter object, not a renamed context bag.

## 6. Requiring a Concrete Collection

### Non-example

```rust
fn total(values: &Vec<i64>) -> i64 {
    values.iter().sum()
}
```

The implementation only traverses borrowed values, but the signature rejects arrays, slices, sets, and other iterables.

### Better when only traversal is required

```rust
fn total<'a>(values: impl IntoIterator<Item = &'a i64>) -> i64 {
    values.into_iter().copied().sum()
}
```

### Better when contiguous borrowed storage is required

```rust
fn checksum(values: &[u8]) -> Checksum {
    // The algorithm genuinely requires a contiguous byte slice.
}
```

Do not choose the weakest type merely because it compiles. Ask for `&[T]`, `Vec<T>`, or another concrete representation when the operation genuinely requires contiguity, ownership, mutation, stable indexing, capacity, or multiple passes.

## 7. “Must Call A Before B”

### Non-example

```rust
let mut system = System::new();
system.initialize()?;

// Must call initialize first.
system.run()?;
```

The comment carries a lifecycle invariant the API permits callers to violate.

### Better Rust typestate

```rust
struct UninitializedSystem {
    configuration: Configuration,
}

struct InitializedSystem {
    resources: Resources,
}

impl UninitializedSystem {
    fn initialize(self) -> Result<InitializedSystem, InitializeError> {
        let resources = Resources::load(self.configuration)?;
        Ok(InitializedSystem { resources })
    }
}

impl InitializedSystem {
    fn run(&mut self) -> Result<(), RunError> {
        self.resources.run()
    }
}
```

Initialization consumes the uninitialized state and returns the state that can run. This is stronger than an empty receipt token, which can become stale or prove nothing unless construction, ownership, and lifetimes remain tied to the real condition.

## 8. “The Mutex Must Be Locked”

### Non-example

```rust
// The mutex must be locked before calling this function.
fn update_shared_state() {
    // Reaches ambient shared state.
}
```

### Better

```rust
fn update_shared_state(
    state: &mut MutexGuard<'_, SharedState>,
) {
    state.advance();
}

let mut state = shared_state
    .lock()
    .map_err(|_| UpdateError::LockPoisoned)?;
update_shared_state(&mut state);
```

The guard proves that the lock remains held for the call. In many Rust designs, keeping the guarded data private and accepting `&mut SharedState` inside the owning module is even simpler; the important point is that ownership and lifetime—not a comment or freely constructible token—preserve the condition.

## 9. Normalized Surface Normals

### Non-example

```rust
fn reflect(
    incident: Vec3,
    normal: Vec3,
) -> Result<Vec3, ReflectError> {
    if !is_unit_length(normal) {
        return Err(ReflectError::NormalNotNormalized);
    }

    Ok(incident - 2.0 * incident.dot(normal) * normal)
}
```

Every reflection call accepts invalid normals and must remember the same validation and failure policy.

### Better when unit length is a stable domain invariant

```rust
fn reflect(incident: Vec3, normal: UnitVec3) -> Vec3 {
    let normal = normal.into_inner();
    incident - 2.0 * incident.dot(normal) * normal
}
```

`UnitVec3` uses private composition and fallible, overflow-safe construction. See [examples.md](examples.md#carry-a-proven-invariant) for the constructor.

Do not overstate what the type proves:

```rust
fn cross(left: UnitVec3, right: UnitVec3) -> Vec3 {
    left.into_inner().cross(right.into_inner())
}
```

This overload must not return a normalized vector: the magnitude is `sin(theta)`, and parallel unit vectors produce zero. A separately named normalized cross product must be fallible.

## 10. One Level of Abstraction in Asset Lookup

### Non-example

```rust
fn is_asset_of_type(
    &self,
    asset_name: &str,
    expected: AssetType,
) -> Result<bool, AssetLookupError> {
    // Phase 1: representation policy.
    let normalized = asset_name.to_ascii_lowercase();

    // Phase 2: search mechanics.
    let mut low = 0;
    let mut high = self.assets.len();
    while low < high {
        let middle = low + (high - low) / 2;
        let asset = self
            .assets
            .get(middle)
            .ok_or(AssetLookupError::CorruptIndex { middle })?;

        match asset.name().cmp(normalized.as_str()) {
            Ordering::Less => low = middle + 1,
            Ordering::Greater => high = middle,
            Ordering::Equal => return Ok(asset.kind() == expected),
        }
    }

    Ok(false)
}
```

The function alternates between domain intent, name-normalization policy, index arithmetic, and search implementation. Handwritten binary search also concentrates subtle correctness risk inside a domain operation.

### Intermediate improvement

```rust
fn find_asset(&self, asset_name: &str) -> Option<&Asset> {
    let normalized = asset_name.to_ascii_lowercase();
    let index = self
        .assets
        .binary_search_by(|asset| asset.name().cmp(&normalized))
        .ok()?;

    self.assets.get(index)
}
```

The standard algorithm removes handwritten search mechanics, but insertion and lookup must still remember the same normalization and ordering policy.

### Better: let the representation own both invariants

```rust
use std::collections::HashMap;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct AssetName(String);

impl AssetName {
    fn parse(value: &str) -> Result<Self, AssetNameError> {
        validate_asset_name(value)?;
        Ok(Self(value.to_ascii_lowercase()))
    }
}

#[derive(Default)]
struct AssetCatalog {
    types_by_name: HashMap<AssetName, AssetType>,
}

impl AssetCatalog {
    fn insert(
        &mut self,
        name: AssetName,
        asset_type: AssetType,
    ) -> Option<AssetType> {
        self.types_by_name.insert(name, asset_type)
    }

    fn asset_type(&self, name: &AssetName) -> Option<AssetType> {
        self.types_by_name.get(name).copied()
    }

    fn is_asset_of_type(
        &self,
        name: &AssetName,
        expected: AssetType,
    ) -> bool {
        self.asset_type(name) == Some(expected)
    }
}
```

`AssetName` owns validation and lowercase canonicalization. `HashMap` owns unique-key association, replacement, and lookup. `AssetCatalog` is justified only if it owns additional catalog behavior such as insertion or restoration boundaries; otherwise the composed map type may be sufficient. Raw strings from files, databases, or user input must pass through `AssetName::parse` before entering the map.

Ordering disappears because it was required by the old binary-search representation, not by the domain. The domain operation remains at one conceptual level, and neither callers nor helpers must remember normalization, sorting, or search policy.

This example intentionally uses an ASCII-only policy. Unicode case folding and normalization require a separately specified policy.

## 11. Thin Framework Callbacks

Framework-owned callbacks have a hidden call site. Keep them as small adapters into application-owned functions:

```rust
fn update(app: &mut FrameworkApp, frame: FrameContext) {
    let input = InputSnapshot::from(frame.input());
    let elapsed = frame.elapsed();

    app.world.step(input, elapsed);
    frame.render(app.world.view());
}
```

The callback is not pure, and the framework still controls when it runs. Its job is boundary translation, not domain policy.
