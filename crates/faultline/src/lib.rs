//! > Error classification for services and control planes.

#![cfg_attr(docsrs, feature(doc_cfg))]

#[doc = include_str!("../README.md")]
#[cfg(doctest)]
pub struct ReadmeDoctests;

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
#[derive(Debug, thiserror::Error)]
pub enum Fault<D, T = anyhow::Error>
where
    T: ErrorKind,
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
    /// remain running but the request cannot proceed.
    #[error("Invariant violation: {0}")]
    Invariant(anyhow::Error),
}

impl<D> Fault<D, Never> {
    #[inline]
    pub fn domain(error: D) -> Self {
        Fault::Domain(error)
    }
}

impl<D> Fault<D, anyhow::Error> {
    #[inline]
    pub fn transient(error: impl Into<anyhow::Error>) -> Self {
        Fault::Transient(error.into())
    }
}

impl<D, T> Fault<D, T>
where
    T: ErrorKind,
{
    #[inline]
    pub fn invariant(error: impl Into<anyhow::Error>) -> Self {
        Fault::Invariant(error.into())
    }

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
    pub fn extract_domain(self) -> Either<D, Fault<Never, T>> {
        match self {
            Fault::Domain(d) => Either::Left(d),
            Fault::Transient(t) => Either::Right(Fault::Transient(t)),
            Fault::Invariant(i) => Either::Right(Fault::Invariant(i)),
        }
    }

    #[inline]
    pub fn extract_transient(self) -> Either<T, Fault<D, Never>> {
        match self {
            Fault::Domain(d) => Either::Right(Fault::Domain(d)),
            Fault::Transient(t) => Either::Left(t),
            Fault::Invariant(i) => Either::Right(Fault::Invariant(i)),
        }
    }

    #[inline]
    pub fn extract_invariant(self) -> Either<anyhow::Error, Fault<D, T>> {
        match self {
            Fault::Domain(d) => Either::Right(Fault::Domain(d)),
            Fault::Transient(t) => Either::Right(Fault::Transient(t)),
            Fault::Invariant(i) => Either::Left(i),
        }
    }

    #[inline]
    pub fn map_domain<D2>(self, f: impl FnOnce(D) -> D2) -> Fault<D2, T> {
        match self {
            Fault::Domain(d) => Fault::Domain(f(d)),
            Fault::Transient(t) => Fault::Transient(t),
            Fault::Invariant(i) => Fault::Invariant(i),
        }
    }

    #[inline]
    pub fn map_transient<T2>(self, f: impl FnOnce(T) -> T2) -> Fault<D, T2>
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
    pub fn map_invariant(self, f: impl FnOnce(anyhow::Error) -> anyhow::Error) -> Self {
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
    pub fn inspect_invariant(self, f: impl FnOnce(&anyhow::Error)) -> Self {
        if let Fault::Invariant(ref i) = self {
            f(i);
        }
        self
    }

    #[inline]
    pub fn upcast<D2>(self) -> Fault<D2, T>
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
    pub fn expect_err_not_domain(self, invariant: &str) -> Fault<Never, T>
    where
        D: Into<anyhow::Error>,
    {
        match self {
            Fault::Domain(d) => Fault::Invariant(make_invariant_violation(invariant, d)),
            Fault::Transient(t) => Fault::Transient(t),
            Fault::Invariant(i) => Fault::Invariant(i),
        }
    }
}

impl<T> Fault<Never, T>
where
    T: ErrorKind,
{
    #[inline]
    pub fn squash<D>(self) -> Fault<D, T> {
        match self {
            Fault::Invariant(err) => Fault::Invariant(err),
            Fault::Transient(err) => Fault::Transient(err),
        }
    }
}

impl<D> From<Fault<D, Never>> for Fault<D, anyhow::Error> {
    #[inline]
    fn from(value: Fault<D, Never>) -> Self {
        match value {
            Fault::Domain(d) => Fault::Domain(d),
            Fault::Invariant(i) => Fault::Invariant(i),
        }
    }
}
