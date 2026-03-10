use crate::Error;
use crate::ErrorKind;
use crate::Never;
use crate::make_invariant_violation;

use either::Either;

/// Extension trait for wrapping arbitrary errors as [`Error`].
///
/// Each method returns a maximally constrained error type with [`Never`]
/// in all slots except the one being set. This allows `?` to widen via
/// the existing `From` impls.
pub trait ResultIntoErrorExt<OK, E> {
    /// Wrap error as a domain error.
    ///
    /// To widen the domain type, chain with `.upcast_err()`.
    fn map_err_into_domain(self) -> Result<OK, Error<E, Never, Never>>;

    /// Wrap error as a transient error.
    fn map_err_into_transient(self) -> Result<OK, Error<Never, anyhow::Error, Never>>
    where
        E: Into<anyhow::Error>;

    /// Wrap error as an invariant violation.
    ///
    /// The `invariant` parameter should describe the invariant that was
    /// violated.
    fn map_err_into_invariant(
        self,
        invariant: &str,
    ) -> Result<OK, Error<Never, Never, anyhow::Error>>
    where
        E: Into<anyhow::Error>;
}

impl<OK, E> ResultIntoErrorExt<OK, E> for Result<OK, E> {
    #[inline]
    fn map_err_into_domain(self) -> Result<OK, Error<E, Never, Never>> {
        self.map_err(Error::Domain)
    }

    #[inline]
    fn map_err_into_transient(self) -> Result<OK, Error<Never, anyhow::Error, Never>>
    where
        E: Into<anyhow::Error>,
    {
        self.map_err(|e| Error::Transient(e.into()))
    }

    #[inline]
    fn map_err_into_invariant(
        self,
        invariant: &str,
    ) -> Result<OK, Error<Never, Never, anyhow::Error>>
    where
        E: Into<anyhow::Error>,
    {
        self.map_err(|e| Error::Invariant(make_invariant_violation(invariant, e)))
    }
}

pub trait ResultExt<OK, D, T, I>
where
    T: ErrorKind,
    I: ErrorKind,
{
    /// Map the domain error type
    fn map_err_domain<D2>(self, f: impl FnOnce(D) -> D2) -> Result<OK, Error<D2, T, I>>;

    /// Map the transient error type
    fn map_err_transient<Tr2>(self, f: impl FnOnce(T) -> Tr2) -> Result<OK, Error<D, Tr2, I>>
    where
        Tr2: ErrorKind;

    /// Map the invariant error type
    fn map_err_invariant<I2>(self, f: impl FnOnce(I) -> I2) -> Result<OK, Error<D, T, I2>>
    where
        I2: ErrorKind;

    /// Upcast the domain error type
    fn upcast_err<U>(self) -> Result<OK, Error<U, T, I>>
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
    fn inspect_err_invariant(self, f: impl FnOnce(&I)) -> Self;

    /// Extract domain errors for handling, propagate transient/invariant
    #[must_use]
    fn extract_err_domain(self) -> Either<D, Result<OK, Error<Never, T, I>>>;

    /// Extract transient errors for handling, propagate domain/invariant
    #[must_use]
    fn extract_err_transient(self) -> Either<T, Result<OK, Error<D, Never, I>>>;

    /// Extract invariant errors for handling, propagate domain/transient
    #[must_use]
    fn extract_err_invariant(self) -> Either<I, Result<OK, Error<D, T, Never>>>;

    /// Converts domain errors to invariant violations; passes through transient
    /// and invariant errors unchanged.
    ///
    /// The `invariant` parameter should describe the invariant that was
    /// violated if a domain error is encountered.
    fn expect_err_not_domain(self, invariant: &str) -> Result<OK, Error<Never, T, anyhow::Error>>
    where
        D: Into<anyhow::Error>;
}

impl<OK, D, T, I> ResultExt<OK, D, T, I> for Result<OK, Error<D, T, I>>
where
    T: ErrorKind,
    I: ErrorKind,
{
    #[inline]
    fn map_err_domain<D2>(self, f: impl FnOnce(D) -> D2) -> Result<OK, Error<D2, T, I>> {
        self.map_err(|e| e.map_domain(f))
    }

    #[inline]
    fn map_err_transient<T2>(self, f: impl FnOnce(T) -> T2) -> Result<OK, Error<D, T2, I>>
    where
        T2: ErrorKind,
    {
        self.map_err(|e| e.map_transient(f))
    }

    #[inline]
    fn map_err_invariant<I2>(self, f: impl FnOnce(I) -> I2) -> Result<OK, Error<D, T, I2>>
    where
        I2: ErrorKind,
    {
        self.map_err(|e| e.map_invariant(f))
    }

    #[inline]
    fn upcast_err<U>(self) -> Result<OK, Error<U, T, I>>
    where
        U: From<D>,
    {
        self.map_err(Error::upcast)
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
    fn inspect_err_invariant(self, f: impl FnOnce(&I)) -> Self {
        self.map_err(|e| e.inspect_invariant(f))
    }

    #[inline]
    fn extract_err_domain(self) -> Either<D, Result<OK, Error<Never, T, I>>> {
        match self {
            Ok(ok) => Either::Right(Ok(ok)),
            Err(err) => match err.extract_domain() {
                Either::Left(domain) => Either::Left(domain),
                Either::Right(other) => Either::Right(Err(other)),
            },
        }
    }

    #[inline]
    fn extract_err_transient(self) -> Either<T, Result<OK, Error<D, Never, I>>> {
        match self {
            Ok(ok) => Either::Right(Ok(ok)),
            Err(err) => match err.extract_transient() {
                Either::Left(transient) => Either::Left(transient),
                Either::Right(other) => Either::Right(Err(other)),
            },
        }
    }

    #[inline]
    fn extract_err_invariant(self) -> Either<I, Result<OK, Error<D, T, Never>>> {
        match self {
            Ok(ok) => Either::Right(Ok(ok)),
            Err(err) => match err.extract_invariant() {
                Either::Left(invariant) => Either::Left(invariant),
                Either::Right(other) => Either::Right(Err(other)),
            },
        }
    }

    #[inline]
    fn expect_err_not_domain(self, invariant: &str) -> Result<OK, Error<Never, T, anyhow::Error>>
    where
        D: Into<anyhow::Error>,
    {
        self.map_err(|e| e.expect_err_not_domain(invariant))
    }
}

pub trait ResultSquashExt<OK, T, I>
where
    T: ErrorKind,
    I: ErrorKind,
{
    /// Squash a domain-infallible error to any domain type
    fn squash_err<D>(self) -> Result<OK, Error<D, T, I>>;
}

impl<OK, T, I> ResultSquashExt<OK, T, I> for Result<OK, Error<Never, T, I>>
where
    T: ErrorKind,
    I: ErrorKind,
{
    #[inline]
    fn squash_err<D>(self) -> Result<OK, Error<D, T, I>> {
        self.map_err(Error::squash)
    }
}
