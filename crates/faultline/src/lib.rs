//! Error classification for services and control planes.
//!
//! This module defines [`Fault`], the error type used at service boundaries.
//! It separates failures into three categories:
//!
//! - domain errors: expected business failures that callers branch on
//! - transient errors: operational failures that are usually retried
//! - invariant violations: broken assumptions where the safest action is to
//!   abort the current operation
//!
//! The goal is to make function signatures reflect what a caller can actually
//! do: handle domain failures explicitly, decide retry policy for transient
//! failures, and treat invariant violations as "stop and unwind".
//!
//! ## Scope
//!
//! This type is intended for:
//!
//! - long-running services and control planes
//! - orchestration code that calls out to multiple dependencies
//! - code that needs consistent retry and failure handling
//!
//! It is not required everywhere. Leaf libraries can expose whatever error
//! types are natural for them (`thiserror` enums, `anyhow::Error`, etc.).
//! Conversion into [`Fault`] usually happens at service boundaries.
//!
//! ## Problem
//!
//! In practice we saw two unsatisfying extremes:
//!
//! - fully typed error enums threaded through every layer
//! - a single untyped `anyhow::Error` everywhere
//!
//! Fully typed errors are nice for pattern matching, but make it hard to share
//! generic infrastructure (retries, middleware) and tend to accumulate variants
//! that nobody matches on. A single untyped error is easy to work with, but it
//! does not clearly communicate which failures a caller is expected to handle
//! versus those that are purely operational or unrecoverable.
//!
//! ## Design
//!
//! The [`Fault`] enum keeps domain failures typed and uses `anyhow::Error` for
//! categories where callers rarely pattern match:
//!
//! ```ignore
//! Fault<D, T = anyhow::Error, I = Never>
//! ```
//!
//! - `D`: domain error type (required)
//! - `T`: transient error type (defaults to `anyhow::Error`, use [`Never`] to
//!   disallow transients)
//! - `I`: invariant violation type (defaults to [`Never`], use `anyhow::Error`
//!   where violations are possible)
//!
//! [`Never`] represents an impossible category. It lets us state at the type
//! level which failure modes a function can produce.
//!
//! ### Domain errors (`D`)
//!
//! Expected failures in business logic: missing resources, validation
//! failures, conflicts, permission checks, and similar cases. Callers are
//! expected to match on these and take different code paths depending on the
//! variant.
//!
//! `D` is usually an enum defined in the calling crate. It should be concrete
//! and exhaustively matchable.
//!
//! ### Transient errors (`T`)
//!
//! Operational failures where the usual response is some form of retry or
//! backoff: timeouts, connection failures, rate limiting, dependency overload,
//! and similar cases. Callers typically do not care about the detailed type,
//! only that the failure is transient.
//!
//! The default choice is `anyhow::Error`. This gives:
//!
//! - cheap boxing and downcasting
//! - rich context via `.context(...)`
//! - good interoperability with the rest of the Rust ecosystem
//!
//! For observability, prefer structured logging and tracing rather than
//! matching on concrete transient error types.
//!
//! ### Invariant violations (`I`)
//!
//! Situations where an assumed invariant is broken and continuing the current
//! operation is unsafe: corrupted data, impossible state, violated contracts,
//! or code paths that should be unreachable.
//!
//! The caller cannot recover from these. The correct response is to unwind the
//! current operation, perform cleanup (rollback transactions, release locks,
//! close connections), and surface a failure up the stack. This is an
//! alternative to `panic!` when the process as a whole is still healthy, but
//! the current request cannot proceed safely.
//!
//! Most functions should use [`Never`] here. Use `anyhow::Error` in
//! low-level or system code that may need to propagate rich context about a
//! violation.
//!
//! ## Serialization
//!
//! Serialization is intentionally asymmetric:
//!
//! - domain errors: serialized structurally (requires `D: Serialize`)
//! - transient/invariant errors: serialized only via their `Display` string
//!
//! The intent is to discourage shipping internal error details across process
//! boundaries and to encourage explicit API error types at the edges. At
//! network boundaries, transient failures are usually network problems anyway;
//! clients reconstruct their own transient errors based on local failures.
//!
//! ## Usage
//!
//! Some patterns that have worked well:
//!
//! ```ignore
//! // Function that can only fail with domain errors
//! fn validate(input: &str) -> Result<Data, Fault<ValidationError, Never, Never>> {
//!     // ...
//! }
//!
//! // Function that can experience transient failures
//! fn fetch_user(id: UserId) -> Result<User, Fault<UserError, anyhow::Error, Never>> {
//!     // ...
//! }
//!
//! // System-level function that may encounter invariant violations
//! fn process_request(req: Request) -> Result<Response, Fault<ApiError, anyhow::Error, anyhow::Error>> {
//!     // ...
//! }
//! ```
//!
//! A rough rule of thumb:
//!
//! - if callers should branch on it, put it in `D`
//! - if callers only need to know "retry or not", put it in `T`
//! - if the safest response is "stop this operation", put it in `I`
//!
//! ## Alternatives considered
//!
//! ### Trait-based error classification
//!
//! One option was a trait implemented by error types that exposes methods like
//! `is_transient()` or `is_invariant()`. This keeps a single error type but
//! relies on implementations to be correct. It also makes it harder to express
//! at a function boundary that a function never returns transient errors; that
//! becomes a convention instead of something the compiler can check.
//!
//! With `Fault<D, T, I>` and [`Never`], the type system enforces which
//! categories are possible.
//!
//! ### Domain traits for retry
//!
//! Another option was a trait implemented on domain error enums, used by retry
//! helpers to decide whether to back off or fail fast. This couples domain
//! types to infrastructure concerns and makes those traits part of the public
//! surface area.
//!
//! By keeping retry decisions on `Fault<_, T, _>` (where "transient" is a type
//! parameter), domain types remain free of infrastructure logic and retry code
//! can be reused across services.

// Re-export for convenience
pub use either;

mod errorkind;
pub mod result_ext;
mod serde;

pub use errorkind::ErrorKind;
pub use errorkind::Never;

use either::Either;

/// Creates an invariant violation error with logging.
///
/// Logs the violation at error level and wraps the error with the invariant
/// description. The `invariant` parameter should describe what invariant was
/// violated.
#[inline]
pub(crate) fn make_invariant_violation(
    invariant: &str,
    error: impl Into<anyhow::Error>,
) -> anyhow::Error {
    let source = error.into();
    tracing::error!(
        invariant = invariant,
        error = %source,
        "invariant violated"
    );
    anyhow::anyhow!("{invariant}: {source}")
}

/// Common error type for classifying failures inside services.
///
/// Encodes failures as domain errors, transient errors, or invariant
/// violations. See the [module documentation](self) for rationale and usage
/// patterns and for guidance on mapping this to API-facing error types.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum Fault<D, T = anyhow::Error, I = Never>
where
    T: ErrorKind,
    I: ErrorKind,
{
    /// Domain error: expected business logic failures.
    ///
    /// Callers are expected to match on this variant and branch on the domain
    /// error type `D`. `D` should be a concrete type, usually an enum that is
    /// exhaustively matchable.
    #[error("Terminal error: {0}")]
    Domain(D),

    /// Transient error: operational failures that are typically retried.
    ///
    /// Used for timeouts, connection failures, rate limiting, and similar
    /// cases. The type parameter `T` defaults to `anyhow::Error`; set it to
    /// [`Never`] when a function cannot experience transient failures.
    #[error("Transient failure: {0}")]
    Transient(T),

    /// Invariant violation: broken state where continuing is unsafe.
    ///
    /// Used when an assumed invariant is not upheld and the current operation
    /// must abort. This is an alternative to `panic!` when the process can
    /// remain running but the request cannot proceed. `I` defaults to
    /// [`Never`]; use `anyhow::Error` in system-level code that may encounter
    /// violations.
    #[error("Invariant violation: {0}")]
    Invariant(I),
}

impl<D> Fault<D, Never, Never> {
    #[inline]
    pub fn domain(error: D) -> Self {
        Fault::Domain(error)
    }
}

impl<D> Fault<D, anyhow::Error, Never> {
    #[inline]
    pub fn transient(error: impl Into<anyhow::Error>) -> Self {
        Fault::Transient(error.into())
    }
}

impl<D> Fault<D, Never, anyhow::Error> {
    #[inline]
    pub fn invariant(error: impl Into<anyhow::Error>) -> Self {
        Fault::Invariant(error.into())
    }
}

impl<D, T, I> Fault<D, T, I>
where
    T: ErrorKind,
    I: ErrorKind,
{
    #[inline]
    pub fn is_domain(&self) -> bool {
        matches!(self, Fault::Domain(_))
    }

    #[inline]
    pub fn is_transient(&self) -> bool {
        matches!(self, Fault::Transient(_))
    }

    #[inline]
    pub fn is_invariant(&self) -> bool {
        matches!(self, Fault::Invariant(_))
    }

    #[inline]
    pub fn extract_domain(self) -> Either<D, Fault<Never, T, I>> {
        match self {
            Fault::Domain(d) => Either::Left(d),
            Fault::Transient(t) => Either::Right(Fault::Transient(t)),
            Fault::Invariant(i) => Either::Right(Fault::Invariant(i)),
        }
    }

    #[inline]
    pub fn extract_transient(self) -> Either<T, Fault<D, Never, I>> {
        match self {
            Fault::Domain(d) => Either::Right(Fault::Domain(d)),
            Fault::Transient(t) => Either::Left(t),
            Fault::Invariant(i) => Either::Right(Fault::Invariant(i)),
        }
    }

    #[inline]
    pub fn extract_invariant(self) -> Either<I, Fault<D, T, Never>> {
        match self {
            Fault::Domain(d) => Either::Right(Fault::Domain(d)),
            Fault::Transient(t) => Either::Right(Fault::Transient(t)),
            Fault::Invariant(i) => Either::Left(i),
        }
    }

    #[inline]
    pub fn map_domain<D2>(self, f: impl FnOnce(D) -> D2) -> Fault<D2, T, I> {
        match self {
            Fault::Domain(d) => Fault::Domain(f(d)),
            Fault::Transient(t) => Fault::Transient(t),
            Fault::Invariant(i) => Fault::Invariant(i),
        }
    }

    #[inline]
    pub fn map_transient<T2>(self, f: impl FnOnce(T) -> T2) -> Fault<D, T2, I>
    where
        T2: ErrorKind,
    {
        match self {
            Fault::Domain(d) => Fault::Domain(d),
            Fault::Transient(t) => Fault::Transient(f(t)),
            Fault::Invariant(i) => Fault::Invariant(i),
        }
    }

    #[inline]
    pub fn map_invariant<I2>(self, f: impl FnOnce(I) -> I2) -> Fault<D, T, I2>
    where
        I2: ErrorKind,
    {
        match self {
            Fault::Domain(d) => Fault::Domain(d),
            Fault::Transient(t) => Fault::Transient(t),
            Fault::Invariant(i) => Fault::Invariant(f(i)),
        }
    }

    #[inline]
    #[must_use]
    pub fn inspect_domain(self, f: impl FnOnce(&D)) -> Self {
        if let Fault::Domain(ref d) = self {
            f(d);
        }
        self
    }

    #[inline]
    #[must_use]
    pub fn inspect_transient(self, f: impl FnOnce(&T)) -> Self {
        if let Fault::Transient(ref t) = self {
            f(t);
        }
        self
    }

    #[inline]
    #[must_use]
    pub fn inspect_invariant(self, f: impl FnOnce(&I)) -> Self {
        if let Fault::Invariant(ref i) = self {
            f(i);
        }
        self
    }

    #[inline]
    pub fn upcast<D2>(self) -> Fault<D2, T, I>
    where
        D2: From<D>,
    {
        match self {
            Fault::Domain(t) => Fault::Domain(D2::from(t)),
            Fault::Invariant(e) => Fault::Invariant(e),
            Fault::Transient(e) => Fault::Transient(e),
        }
    }

    /// Converts domain errors to invariant violations; passes through transient
    /// and invariant errors unchanged.
    ///
    /// The `invariant` parameter should describe the invariant that was
    /// violated if a domain error is encountered.
    #[inline]
    pub fn expect_err_not_domain(self, invariant: &str) -> Fault<Never, T, anyhow::Error>
    where
        D: Into<anyhow::Error>,
    {
        match self {
            Fault::Domain(d) => Fault::Invariant(make_invariant_violation(invariant, d)),
            Fault::Transient(t) => Fault::Transient(t),
            Fault::Invariant(i) => Fault::Invariant(i.into()),
        }
    }
}

impl<T, I> Fault<Never, T, I>
where
    T: ErrorKind,
    I: ErrorKind,
{
    #[inline]
    pub fn squash<D>(self) -> Fault<D, T, I> {
        match self {
            Fault::Invariant(err) => Fault::Invariant(err),
            Fault::Transient(err) => Fault::Transient(err),
        }
    }
}

impl From<Fault<Never, Never, Never>> for Never {
    #[inline]
    fn from(value: Fault<Never, Never, Never>) -> Self {
        match value {}
    }
}

impl<D, I> From<Fault<D, Never, I>> for Fault<D, anyhow::Error, I>
where
    I: ErrorKind,
{
    #[inline]
    fn from(value: Fault<D, Never, I>) -> Self {
        match value {
            Fault::Domain(d) => Fault::Domain(d),
            Fault::Invariant(i) => Fault::Invariant(i),
        }
    }
}

impl<D, T> From<Fault<D, T, Never>> for Fault<D, T, anyhow::Error>
where
    T: ErrorKind,
{
    #[inline]
    fn from(value: Fault<D, T, Never>) -> Self {
        match value {
            Fault::Domain(d) => Fault::Domain(d),
            Fault::Transient(t) => Fault::Transient(t),
        }
    }
}

impl<D> From<Fault<D, Never, Never>> for Fault<D, anyhow::Error, anyhow::Error> {
    #[inline]
    fn from(value: Fault<D, Never, Never>) -> Self {
        match value {
            Fault::Domain(d) => Fault::Domain(d),
        }
    }
}
