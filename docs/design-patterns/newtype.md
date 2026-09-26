---
type: pattern
summary: "Newtype Wrapper"
last_validated: 2026-09-25
---
# Newtype Wrapper

Wrap a primitive (typically `String` or a numeric type) in a one-field struct
with a private inner value and a fallible constructor that validates it.
Downstream code can then accept the newtype and rely on the invariant being
enforced at construction.

```rust
pub struct Wrapper(Inner);

impl Wrapper {
    pub fn new(value: Inner) -> Result<Self, Error> {
        validate(&value)?;
        Ok(Self(value))
    }

    pub fn as_inner(&self) -> &Inner { &self.0 }
}
```

Sometimes phrased as “parse, don't validate”: validate once at construction so
downstream callers do not have to repeat the check.

## When to reach for one

- A primitive carries protocol meaning, such as a Git ref, hash, semantic
  version, or identifier with a documented format.
- The value crosses many layers and callers should receive proof that it was
  checked.
- Validation and use must not be separated, leaving a caller to forget the
  check or use a value under different assumptions.

## When not to

- The value is local to one function and has no reusable contract.
- The domain already has an appropriate type, such as `PathBuf` or `Uuid`.
- The value is intentionally free-form, such as a note or description.

## Current workspace status

The research crates currently have no tuple newtype. Shared values such as
`Record` use string fields for identifiers. The Core `Operation::from_str`
implementation parses external operation names into an enum; that is a parsed
sum type, not a wrapper around a primitive. Treat the shape above as guidance
for a future value with a concrete validation contract, not as a description
of an existing production type.
