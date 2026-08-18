# Faultline

Error classification for services and control planes.

This module defines [`Fault`], the error type used at service boundaries.
It separates failures into three categories:

- domain errors: expected business failures that callers branch on
- transient errors: operational failures that are usually retried
- invariant violations: broken assumptions where the safest action is to
  abort the current operation

The goal is to make function signatures reflect what a caller can actually
do: handle domain failures explicitly, decide retry policy for transient
failures, and treat invariant violations as "stop and unwind".

## Scope

This type is intended for:

- long-running services and control planes
- orchestration code that calls out to multiple dependencies
- code that needs consistent retry and failure handling

It is not required everywhere. Leaf libraries can expose whatever error
types are natural for them (`thiserror` enums, `anyhow::Error`, etc.).
Conversion into [`Fault`] usually happens at service boundaries.

## Problem

In practice we saw two unsatisfying extremes:

- fully typed error enums threaded through every layer
- a single untyped `anyhow::Error` everywhere

Fully typed errors are nice for pattern matching, but make it hard to share
generic infrastructure (retries, middleware) and tend to accumulate variants
that nobody matches on. A single untyped error is easy to work with, but it
does not clearly communicate which failures a caller is expected to handle
versus those that are purely operational or unrecoverable.

## Design

The [`Fault`] enum keeps domain failures typed and uses `anyhow::Error` for
categories where callers rarely pattern match:

```ignore
Fault<D, T = anyhow::Error>
```

- `D`: domain error type (required)
- `T`: transient error type (defaults to `anyhow::Error`, use [`Never`] to
  disallow transients)
- invariant violations always hold `anyhow::Error`

[`Never`] represents an impossible transient category. Invariant violations
remain available on every [`Fault`], since any function that classifies errors
may also need to report a broken assumption.

### Domain errors (`D`)

Expected failures in business logic: missing resources, validation
failures, conflicts, permission checks, and similar cases. Callers are
expected to match on these and take different code paths depending on the
variant.

`D` is usually an enum defined in the calling crate. It should be concrete
and exhaustively matchable.

### Transient errors (`T`)

Operational failures where the usual response is some form of retry or
backoff: timeouts, connection failures, rate limiting, dependency overload,
and similar cases. Callers typically do not care about the detailed type,
only that the failure is transient.

The default choice is `anyhow::Error`. This gives:

- cheap boxing and downcasting
- rich context via `.context(...)`
- good interoperability with the rest of the Rust ecosystem

For observability, prefer structured logging and tracing rather than
matching on concrete transient error types.

### Invariant violations

Situations where an assumed invariant is broken and continuing the current
operation is unsafe: corrupted data, impossible state, violated contracts,
or code paths that should be unreachable.

The caller cannot recover from these. The correct response is to unwind the
current operation, perform cleanup (rollback transactions, release locks,
close connections), and surface a failure up the stack. This is an
alternative to `panic!` when the process as a whole is still healthy, but
the current request cannot proceed safely.

Invariant violations always use `anyhow::Error` so they can carry rich
context without adding another type parameter to every [`Fault`].

## Serialization

Serialization is intentionally asymmetric:

- domain errors: serialized structurally (requires `D: Serialize`)
- transient/invariant errors: serialized only via their `Display` string

The intent is to discourage shipping internal error details across process
boundaries and to encourage explicit API error types at the edges. At
network boundaries, transient failures are usually network problems anyway;
clients reconstruct their own transient errors based on local failures.

## Usage

Some patterns that have worked well:

```ignore
// Function with no transient failures
fn validate(input: &str) -> Result<Data, Fault<ValidationError, Never>> {
    // ...
}

// Function that can experience transient failures
fn fetch_user(id: UserId) -> Result<User, Fault<UserError>> {
    // ...
}

// Invariant violations remain available in both signatures
fn process_request(req: Request) -> Result<Response, Fault<ApiError>> {
    // ...
}
```

A rough rule of thumb:

- if callers should branch on it, put it in `D`
- if callers only need to know "retry or not", put it in `T`
- if the safest response is "stop this operation", use `Invariant`

## Alternatives considered

### Trait-based error classification

One option was a trait implemented by error types that exposes methods like
`is_transient()` or `is_invariant()`. This keeps a single error type but
relies on implementations to be correct. It also makes it harder to express
at a function boundary that a function never returns transient errors; that
becomes a convention instead of something the compiler can check.

With `Fault<D, T>` and [`Never`], the type system still enforces whether
transient failures are possible. Invariant violations remain an option on all
faults.

### Domain traits for retry

Another option was a trait implemented on domain error enums, used by retry
helpers to decide whether to back off or fail fast. This couples domain
types to infrastructure concerns and makes those traits part of the public
surface area.

By keeping retry decisions on `Fault<_, T>` (where "transient" is a type
parameter), domain types remain free of infrastructure logic and retry code
can be reused across services.

[`Fault`]: https://docs.rs/faultline/latest/faultline/enum.Fault.html
[`Never`]: https://docs.rs/faultline/latest/faultline/enum.Never.html
