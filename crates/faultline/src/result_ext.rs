use crate::ErrorKind;
use crate::Fault;
use crate::Never;
use crate::make_invariant_violation;

use either::Either;

/// Extension trait for wrapping arbitrary errors as [`Fault`].
///
/// Each method returns a maximally constrained error type with [`Never`] in
/// the transient slot unless the method sets it. This allows `?` to widen via
/// the existing `From` impl.
pub trait ResultIntoFaultExt<OK, E> {
    /// Wrap error as a domain error.
    ///
    /// To widen the domain type, chain with `.upcast_err()`.
    fn map_err_into_domain(self) -> Result<OK, Fault<E, Never>>;

    /// Wrap error as a transient error.
    fn map_err_into_transient(self) -> Result<OK, Fault<Never, anyhow::Error>>
    where
        E: Into<anyhow::Error>;

    /// Wrap error as an invariant violation.
    ///
    /// The `invariant` parameter should describe the invariant that was
    /// violated.
    fn map_err_into_invariant(self, invariant: &str) -> Result<OK, Fault<Never, Never>>
    where
        E: Into<anyhow::Error>;
}

impl<OK, E> ResultIntoFaultExt<OK, E> for Result<OK, E> {
    #[inline]
    fn map_err_into_domain(self) -> Result<OK, Fault<E, Never>> {
        self.map_err(Fault::Domain)
    }

    #[inline]
    fn map_err_into_transient(self) -> Result<OK, Fault<Never, anyhow::Error>>
    where
        E: Into<anyhow::Error>,
    {
        self.map_err(|e| Fault::Transient(e.into()))
    }

    #[inline]
    fn map_err_into_invariant(self, invariant: &str) -> Result<OK, Fault<Never, Never>>
    where
        E: Into<anyhow::Error>,
    {
        self.map_err(|e| Fault::Invariant(make_invariant_violation(invariant, e)))
    }
}

pub trait ResultExt<OK, D, T>
where
    T: ErrorKind,
{
    /// Map the domain error type
    fn map_err_domain<D2>(self, f: impl FnOnce(D) -> D2) -> Result<OK, Fault<D2, T>>;

    /// Map the transient error type
    fn map_err_transient<T2>(self, f: impl FnOnce(T) -> T2) -> Result<OK, Fault<D, T2>>
    where
        T2: ErrorKind;

    /// Map the invariant error
    fn map_err_invariant(
        self,
        f: impl FnOnce(anyhow::Error) -> anyhow::Error,
    ) -> Result<OK, Fault<D, T>>;

    /// Upcast the domain error type
    fn upcast_err<U>(self) -> Result<OK, Fault<U, T>>
    where
        U: From<D>;

    /// Inspect the domain error if present (with side effects)
    #[must_use]
    fn inspect_err_domain(self, f: impl FnOnce(&D)) -> Self;

    /// Inspect the transient error if present (with side effects)
    #[must_use]
    fn inspect_err_transient(self, f: impl FnOnce(&T)) -> Self;

    /// Inspect the invariant error if present (with side effects)
    #[must_use]
    fn inspect_err_invariant(self, f: impl FnOnce(&anyhow::Error)) -> Self;

    /// Extract domain errors for handling, propagate transient/invariant
    #[must_use]
    fn extract_err_domain(self) -> Either<D, Result<OK, Fault<Never, T>>>;

    /// Extract transient errors for handling, propagate domain/invariant
    #[must_use]
    fn extract_err_transient(self) -> Either<T, Result<OK, Fault<D, Never>>>;

    /// Extract invariant errors for handling, propagate domain/transient
    #[must_use]
    fn extract_err_invariant(self) -> Either<anyhow::Error, Result<OK, Fault<D, T>>>;

    /// Converts domain errors to invariant violations; passes through transient
    /// and invariant errors unchanged.
    ///
    /// The `invariant` parameter should describe the invariant that was
    /// violated if a domain error is encountered.
    fn expect_err_not_domain(self, invariant: &str) -> Result<OK, Fault<Never, T>>
    where
        D: Into<anyhow::Error>;
}

impl<OK, D, T> ResultExt<OK, D, T> for Result<OK, Fault<D, T>>
where
    T: ErrorKind,
{
    #[inline]
    fn map_err_domain<D2>(self, f: impl FnOnce(D) -> D2) -> Result<OK, Fault<D2, T>> {
        self.map_err(|e| e.map_domain(f))
    }

    #[inline]
    fn map_err_transient<T2>(self, f: impl FnOnce(T) -> T2) -> Result<OK, Fault<D, T2>>
    where
        T2: ErrorKind,
    {
        self.map_err(|e| e.map_transient(f))
    }

    #[inline]
    fn map_err_invariant(
        self,
        f: impl FnOnce(anyhow::Error) -> anyhow::Error,
    ) -> Result<OK, Fault<D, T>> {
        self.map_err(|e| e.map_invariant(f))
    }

    #[inline]
    fn upcast_err<U>(self) -> Result<OK, Fault<U, T>>
    where
        U: From<D>,
    {
        self.map_err(Fault::upcast)
    }

    #[inline]
    fn inspect_err_domain(self, f: impl FnOnce(&D)) -> Self {
        self.map_err(|e| e.inspect_domain(f))
    }

    #[inline]
    fn inspect_err_transient(self, f: impl FnOnce(&T)) -> Self {
        self.map_err(|e| e.inspect_transient(f))
    }

    #[inline]
    fn inspect_err_invariant(self, f: impl FnOnce(&anyhow::Error)) -> Self {
        self.map_err(|e| e.inspect_invariant(f))
    }

    #[inline]
    fn extract_err_domain(self) -> Either<D, Result<OK, Fault<Never, T>>> {
        match self {
            Ok(ok) => Either::Right(Ok(ok)),
            Err(err) => match err.extract_domain() {
                Either::Left(domain) => Either::Left(domain),
                Either::Right(other) => Either::Right(Err(other)),
            },
        }
    }

    #[inline]
    fn extract_err_transient(self) -> Either<T, Result<OK, Fault<D, Never>>> {
        match self {
            Ok(ok) => Either::Right(Ok(ok)),
            Err(err) => match err.extract_transient() {
                Either::Left(transient) => Either::Left(transient),
                Either::Right(other) => Either::Right(Err(other)),
            },
        }
    }

    #[inline]
    fn extract_err_invariant(self) -> Either<anyhow::Error, Result<OK, Fault<D, T>>> {
        match self {
            Ok(ok) => Either::Right(Ok(ok)),
            Err(err) => match err.extract_invariant() {
                Either::Left(invariant) => Either::Left(invariant),
                Either::Right(other) => Either::Right(Err(other)),
            },
        }
    }

    #[inline]
    fn expect_err_not_domain(self, invariant: &str) -> Result<OK, Fault<Never, T>>
    where
        D: Into<anyhow::Error>,
    {
        self.map_err(|e| e.expect_err_not_domain(invariant))
    }
}

pub trait ResultSquashExt<OK, T>
where
    T: ErrorKind,
{
    /// Squash a domain-infallible error to any domain type
    fn squash_err<D>(self) -> Result<OK, Fault<D, T>>;
}

impl<OK, T> ResultSquashExt<OK, T> for Result<OK, Fault<Never, T>>
where
    T: ErrorKind,
{
    #[inline]
    fn squash_err<D>(self) -> Result<OK, Fault<D, T>> {
        self.map_err(Fault::squash)
    }
}
