use crate::Error;
use crate::ErrorKind;

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde::de::VariantAccess;
use serde::de::Visitor;
use serde::de::{self};
use std::fmt;
use std::marker::PhantomData;

// Note: this impl is generic over all `Error<D, T, I>`, but the set of
// serializations that can actually occur is constrained by Rust's type system.
// States that cannot be constructed (e.g. `Error<_, Never, _>::Transient(_)`)
// can never be serialized, even though they are covered by the match.
impl<D, T, I> Serialize for Error<D, T, I>
where
    D: Serialize,
    T: ErrorKind,
    I: ErrorKind,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Error::Domain(d) => serializer.serialize_newtype_variant("Error", 0, "Domain", d),
            Error::Transient(t) => {
                serializer.serialize_newtype_variant("Error", 1, "Transient", &t.to_string())
            }
            Error::Invariant(i) => {
                serializer.serialize_newtype_variant("Error", 2, "Invariant", &i.to_string())
            }
        }
    }
}
// Shared enum used by all `Deserialize` impls for concrete T/I combinations.
#[derive(Deserialize)]
#[serde(field_identifier, rename_all = "PascalCase")]
enum Variant {
    Domain,
    Transient,
    Invariant,
}

macro_rules! impl_error_deserialize {
    ($t:ty, $i:ty, $allow_t:ident, $allow_i:ident) => {
        impl<'de, D> Deserialize<'de> for Error<D, $t, $i>
        where
            D: Deserialize<'de>,
        {
            fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
            where
                De: Deserializer<'de>,
            {
                struct ErrorVisitor<D>(PhantomData<D>);

                impl<'de, D> Visitor<'de> for ErrorVisitor<D>
                where
                    D: Deserialize<'de>,
                {
                    type Value = Error<D, $t, $i>;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str("enum Error")
                    }

                    fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
                    where
                        A: de::EnumAccess<'de>,
                    {
                        let (variant, variant_access) = data.variant()?;
                        match variant {
                            Variant::Domain => {
                                let content = variant_access.newtype_variant()?;
                                Ok(Error::Domain(content))
                            }
                            Variant::Transient => impl_error_deserialize!(
                                @handle_transient
                                $allow_t,
                                $allow_i,
                                variant_access
                            ),
                            Variant::Invariant => impl_error_deserialize!(
                                @handle_invariant
                                $allow_t,
                                $allow_i,
                                variant_access
                            ),
                        }
                    }
                }

                deserializer.deserialize_enum(
                    "Error",
                    &["Domain", "Transient", "Invariant"],
                    ErrorVisitor(PhantomData),
                )
            }
        }
    };

    // Transient handler
    // T allowed: construct anyhow::Error from string
    (@handle_transient allow, $allow_i:ident, $variant_access:ident) => {{
        let s: String = $variant_access.newtype_variant()?;
        Ok(Error::Transient(anyhow::anyhow!(s)))
    }};
    // T = Never, I = Never: only Domain is valid
    (@handle_transient disallow, disallow, $variant_access:ident) => {{
        let _: String = $variant_access.newtype_variant()?;
        Err(de::Error::unknown_variant("Transient", &["Domain"]))
    }};
    // T = Never, I allowed: Domain and Invariant are valid
    (@handle_transient disallow, allow, $variant_access:ident) => {{
        let _: String = $variant_access.newtype_variant()?;
        Err(de::Error::unknown_variant(
            "Transient",
            &["Domain", "Invariant"],
        ))
    }};

    // Invariant handler
    // I allowed: construct anyhow::Error from string
    (@handle_invariant $allow_t:ident, allow, $variant_access:ident) => {{
        let s: String = $variant_access.newtype_variant()?;
        Ok(Error::Invariant(anyhow::anyhow!(s)))
    }};
    // I = Never, T = Never: only Domain is valid
    (@handle_invariant disallow, disallow, $variant_access:ident) => {{
        let _: String = $variant_access.newtype_variant()?;
        Err(de::Error::unknown_variant("Invariant", &["Domain"]))
    }};
    // I = Never, T allowed: Domain and Transient are valid
    (@handle_invariant allow, disallow, $variant_access:ident) => {{
        let _: String = $variant_access.newtype_variant()?;
        Err(de::Error::unknown_variant(
            "Invariant",
            &["Domain", "Transient"],
        ))
    }};
}

// Implement deserialization for the concrete combinations we support in the
// transient/invariant slots.
impl_error_deserialize!(crate::Never, crate::Never, disallow, disallow);
impl_error_deserialize!(anyhow::Error, crate::Never, allow, disallow);
impl_error_deserialize!(crate::Never, anyhow::Error, disallow, allow);
impl_error_deserialize!(anyhow::Error, anyhow::Error, allow, allow);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Never;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestDomainError {
        code: i64,
        message: String,
    }

    impl TestDomainError {
        fn with_message(code: i64, message: impl Into<String>) -> Self {
            Self {
                code,
                message: message.into(),
            }
        }
    }
    const TRANSIENT_MSG: &str = "database connection failed";
    const INVARIANT_MSG: &str = "invariant violated";

    // Round-trip tests: Error<D, Never, Never>
    #[test]
    fn roundtrip_domain_never_never() {
        let err = Error::<TestDomainError, Never, Never>::Domain(TestDomainError::with_message(
            42,
            "test error",
        ));
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: Error<TestDomainError, Never, Never> =
            serde_json::from_str(&json).unwrap();
        assert_eq!(err, deserialized);
    }

    // Round-trip tests: Error<D, anyhow::Error, Never>
    #[test]
    fn roundtrip_domain_anyhow_never() {
        let err = Error::<TestDomainError, anyhow::Error, Never>::Domain(
            TestDomainError::with_message(42, "test error"),
        );
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: Error<TestDomainError, anyhow::Error, Never> =
            serde_json::from_str(&json).unwrap();

        match deserialized {
            Error::Domain(d) => assert_eq!(d, TestDomainError::with_message(42, "test error")),
            _ => panic!("Expected Domain variant"),
        }
    }

    #[test]
    fn roundtrip_transient_anyhow_never() {
        let err = Error::<TestDomainError, anyhow::Error, Never>::Transient(anyhow::anyhow!(
            TRANSIENT_MSG
        ));
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: Error<TestDomainError, anyhow::Error, Never> =
            serde_json::from_str(&json).unwrap();

        match deserialized {
            Error::Transient(e) => assert_eq!(e.to_string(), TRANSIENT_MSG),
            _ => panic!("Expected Transient variant"),
        }
    }

    // Round-trip tests: Error<D, Never, anyhow::Error>
    #[test]
    fn roundtrip_domain_never_anyhow() {
        let err = Error::<TestDomainError, Never, anyhow::Error>::Domain(
            TestDomainError::with_message(42, "test error"),
        );
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: Error<TestDomainError, Never, anyhow::Error> =
            serde_json::from_str(&json).unwrap();

        match deserialized {
            Error::Domain(d) => assert_eq!(d, TestDomainError::with_message(42, "test error")),
            _ => panic!("Expected Domain variant"),
        }
    }

    #[test]
    fn roundtrip_invariant_never_anyhow() {
        let err = Error::<TestDomainError, Never, anyhow::Error>::Invariant(anyhow::anyhow!(
            INVARIANT_MSG
        ));
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: Error<TestDomainError, Never, anyhow::Error> =
            serde_json::from_str(&json).unwrap();

        match deserialized {
            Error::Invariant(e) => assert_eq!(e.to_string(), INVARIANT_MSG),
            _ => panic!("Expected Invariant variant"),
        }
    }

    // Round-trip tests: Error<D, anyhow::Error, anyhow::Error>
    #[test]
    fn roundtrip_domain_anyhow_anyhow() {
        let err = Error::<TestDomainError, anyhow::Error, anyhow::Error>::Domain(
            TestDomainError::with_message(42, "test error"),
        );
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: Error<TestDomainError, anyhow::Error, anyhow::Error> =
            serde_json::from_str(&json).unwrap();

        match deserialized {
            Error::Domain(d) => assert_eq!(d, TestDomainError::with_message(42, "test error")),
            _ => panic!("Expected Domain variant"),
        }
    }

    #[test]
    fn roundtrip_transient_anyhow_anyhow() {
        let err = Error::<TestDomainError, anyhow::Error, anyhow::Error>::Transient(
            anyhow::anyhow!(TRANSIENT_MSG),
        );
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: Error<TestDomainError, anyhow::Error, anyhow::Error> =
            serde_json::from_str(&json).unwrap();

        match deserialized {
            Error::Transient(e) => assert_eq!(e.to_string(), TRANSIENT_MSG),
            _ => panic!("Expected Transient variant"),
        }
    }

    #[test]
    fn roundtrip_invariant_anyhow_anyhow() {
        let err = Error::<TestDomainError, anyhow::Error, anyhow::Error>::Invariant(
            anyhow::anyhow!(INVARIANT_MSG),
        );
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: Error<TestDomainError, anyhow::Error, anyhow::Error> =
            serde_json::from_str(&json).unwrap();

        match deserialized {
            Error::Invariant(e) => assert_eq!(e.to_string(), INVARIANT_MSG),
            _ => panic!("Expected Invariant variant"),
        }
    }

    // anyhow::Error context is preserved as Display string
    #[test]
    fn anyhow_preserves_display_string() {
        let err = Error::<TestDomainError, anyhow::Error, Never>::Transient(
            anyhow::anyhow!("inner error").context("outer context"),
        );
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: Error<TestDomainError, anyhow::Error, Never> =
            serde_json::from_str(&json).unwrap();

        match deserialized {
            Error::Transient(e) => assert_eq!(e.to_string(), "outer context"),
            _ => panic!("Expected Transient variant"),
        }
    }

    // Error cases: unrepresentable states
    #[test]
    fn error_transient_when_never() {
        let err = Error::<TestDomainError, anyhow::Error, Never>::Transient(anyhow::anyhow!(
            "some error"
        ));
        let json = serde_json::to_string(&err).unwrap();
        let result: Result<Error<TestDomainError, Never, Never>, _> = serde_json::from_str(&json);
        assert!(result.is_err());
    }

    #[test]
    fn error_invariant_when_never() {
        let err = Error::<TestDomainError, Never, anyhow::Error>::Invariant(anyhow::anyhow!(
            "some error"
        ));
        let json = serde_json::to_string(&err).unwrap();
        let result: Result<Error<TestDomainError, Never, Never>, _> = serde_json::from_str(&json);
        assert!(result.is_err());
    }

    #[test]
    fn error_transient_when_never_with_invariant_allowed() {
        let err = Error::<TestDomainError, anyhow::Error, anyhow::Error>::Transient(
            anyhow::anyhow!("some error"),
        );
        let json = serde_json::to_string(&err).unwrap();
        let result: Result<Error<TestDomainError, Never, anyhow::Error>, _> =
            serde_json::from_str(&json);
        assert!(result.is_err());
    }

    #[test]
    fn error_invariant_when_never_with_transient_allowed() {
        let err = Error::<TestDomainError, anyhow::Error, anyhow::Error>::Invariant(
            anyhow::anyhow!("some error"),
        );
        let json = serde_json::to_string(&err).unwrap();
        let result: Result<Error<TestDomainError, anyhow::Error, Never>, _> =
            serde_json::from_str(&json);
        assert!(result.is_err());
    }

    #[test]
    fn error_domain_when_never() {
        let err = Error::<TestDomainError, Never, Never>::Domain(TestDomainError::with_message(
            42,
            "test error",
        ));
        let json = serde_json::to_string(&err).unwrap();
        let result: Result<Error<Never, Never, Never>, _> = serde_json::from_str(&json);
        assert!(result.is_err());
    }
}
