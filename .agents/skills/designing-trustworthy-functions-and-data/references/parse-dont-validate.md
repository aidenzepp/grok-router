# Parse, Don't Validate

Use this guide when code checks raw input but keeps using the same weak representation afterward.

## The Information Test

Validation and parsing may perform the same runtime check. Their contracts differ:

```rust
fn validate_non_empty<T>(values: &[T]) -> Result<(), Empty>;
fn parse_non_empty<T>(values: Vec<T>) -> Result<NonEmptyVec<T>, Empty>;
```

The validator returns `()` and discards what it learned. The parser returns a representation that preserves the fact. Downstream code accepting `NonEmptyVec<T>` can expose an infallible `first` operation because emptiness is no longer representable.

Treat parsing as the conversion of less-structured data into more-structured data. Its input need not be text. Useful parse boundaries include:

- primitive to domain value
- raw collection to invariant-bearing collection
- deserialized record to domain entity
- database row to restored aggregate
- framework request to application command
- bytes or foreign values to safe native representations

## Boundary Shape

Keep weak data and fallibility at the edge:

```rust
fn parse_request(raw: RawRequest) -> Result<Request, RequestError>;
fn execute(request: Request, state: &mut State) -> ExecutionResult;
```

Do not spread the original parse error through `execute` when `Request` already proves the relevant facts. A different operational error may still be legitimate.

In Rust, common parser shapes are:

- `TryFrom<Raw> for Domain`
- `FromStr` for textual input
- a named constructor such as `UnitVec3::try_new`
- a fallible aggregate constructor
- custom deserialization that delegates to the ordinary parser

Keep fields private. Do not implement an infallible conversion from the weak representation when it would bypass the invariant.

## Choose the Right Proof Owner

### Value owns the proof

Use a domain value when the fact is intrinsic and should travel:

```rust
fn reflect(incident: Vec3, normal: UnitVec3) -> Vec3;
fn release(&mut self, port: NonPrivilegedPort) -> bool;
fn lookup(&self, name: &AssetName) -> Option<AssetType>;
```

These signatures remove invalid calls and corresponding error paths from trusted code.

### Aggregate owns the proof

Use a private aggregate when the rule is meaningful only inside the aggregate:

```rust
struct DependencyGraph {
    edges: HashMap<NodeId, HashSet<NodeId>>,
}
```

If every constructor and mutation preserves graph membership rules and raw edges never escape as proof-bearing values, a separate `ValidatedEdge` may add nothing.

### Live state owns the proof

Some facts are observations, not durable value properties:

- a username is currently available
- an account currently has sufficient funds
- a row still has a particular version
- a caller currently holds a permission

Do not parse these into timeless wrappers that can become stale. Keep the check and action in the same transaction, lock, lease, guard, actor message, or version-checked operation.

## Normalize Only When the Domain Defines It

Parsing may establish a canonical representation when equivalence is part of the domain:

```rust
impl TryFrom<String> for AssetName {
    type Error = AssetNameError;

    fn try_from(mut value: String) -> Result<Self, Self::Error> {
        validate_asset_name_syntax(&value)?;
        value.make_ascii_lowercase();
        Ok(Self(value))
    }
}
```

Do not silently repair ambiguous input. Trimming, case folding, path resolution, Unicode normalization, unit conversion, and lossy coercion require an explicit policy.

## Construction Is Not the Only Boundary

Preserve the invariant across:

- mutation methods
- parsing and deserialization
- database restoration
- migrations and bulk imports
- FFI and unsafe construction
- arithmetic and transformations

An operation must return the strongest type it can actually prove. A cross product of two unit vectors is not necessarily a unit vector. A previously available username is not necessarily still available.

## Review Procedure

1. Search for `validate`, `is_valid`, repeated assertions, primitive-heavy signatures, and error variants described as impossible.
2. State exactly what each check learns.
3. Decide whether the fact belongs to a value, aggregate, or live consistency boundary.
4. Make the boundary return that proof owner.
5. Change trusted function signatures to accept the stronger representation.
6. Remove redundant checks and impossible error variants.
7. Test rejection at the parse boundary and preservation through every public operation and restoration path.

## Sources

- [Parse, don't validate — Alexis King](https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/) — the original type-driven-design explanation: parsing preserves information that validation returning `()` discards.
- [Rust API Guidelines: Dependability](https://rust-lang.github.io/api-guidelines/dependability.html) — prefer argument types that rule out invalid input, with dynamic checks at boundaries when static enforcement is impractical.
- [Rust API Guidelines: Type safety](https://rust-lang.github.io/api-guidelines/type-safety.html) — use deliberate domain types and newtypes to communicate meaning and prevent confusion.
