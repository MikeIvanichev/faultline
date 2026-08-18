use crate::ErrorKind;
use crate::Fault;

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde::de::VariantAccess;
use serde::de::Visitor;
use serde::de::{self};
use std::fmt;
use std::marker::PhantomData;

// Note: this impl is generic over all `Fault<D, T>`, but the set of
// serializations that can actually occur is constrained by Rust's type system.
// A state that cannot be constructed (`Fault<_, Never>::Transient(_)`) can
// never be serialized, even though it is covered by the match.
impl<D, T> Serialize for Fault<D, T>
where
    D: Serialize,
    T: ErrorKind,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Fault::Domain(d) => serializer.serialize_newtype_variant("Fault", 0, "Domain", d),
            Fault::Transient(t) => {
                serializer.serialize_newtype_variant("Fault", 1, "Transient", &t.to_string())
            }
            Fault::Invariant(i) => {
                serializer.serialize_newtype_variant("Fault", 2, "Invariant", &i.to_string())
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(field_identifier, rename_all = "PascalCase")]
enum Variant {
    Domain,
    Transient,
    Invariant,
}

macro_rules! impl_fault_deserialize {
    ($t:ty, $allow_t:ident) => {
        impl<'de, D> Deserialize<'de> for Fault<D, $t>
        where
            D: Deserialize<'de>,
        {
            fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
            where
                De: Deserializer<'de>,
            {
                struct FaultVisitor<D>(PhantomData<D>);

                impl<'de, D> Visitor<'de> for FaultVisitor<D>
                where
                    D: Deserialize<'de>,
                {
                    type Value = Fault<D, $t>;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str("enum Fault")
                    }

                    fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
                    where
                        A: de::EnumAccess<'de>,
                    {
                        let (variant, variant_access) = data.variant()?;
                        match variant {
                            Variant::Domain => {
                                let content = variant_access.newtype_variant()?;
                                Ok(Fault::Domain(content))
                            }
                            Variant::Transient => impl_fault_deserialize!(
                                @handle_transient
                                $allow_t,
                                variant_access
                            ),
                            Variant::Invariant => {
                                let s: String = variant_access.newtype_variant()?;
                                Ok(Fault::Invariant(anyhow::anyhow!(s)))
                            }
                        }
                    }
                }

                deserializer.deserialize_enum(
                    "Fault",
                    &["Domain", "Transient", "Invariant"],
                    FaultVisitor(PhantomData),
                )
            }
        }
    };

    (@handle_transient allow, $variant_access:ident) => {{
        let s: String = $variant_access.newtype_variant()?;
        Ok(Fault::Transient(anyhow::anyhow!(s)))
    }};
    (@handle_transient disallow, $variant_access:ident) => {{
        let _: String = $variant_access.newtype_variant()?;
        Err(de::Error::unknown_variant(
            "Transient",
            &["Domain", "Invariant"],
        ))
    }};
}

impl_fault_deserialize!(crate::Never, disallow);
impl_fault_deserialize!(anyhow::Error, allow);

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

    #[test]
    fn roundtrip_domain_never() {
        let err = Fault::<TestDomainError, Never>::Domain(TestDomainError::with_message(
            42,
            "test error",
        ));
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: Fault<TestDomainError, Never> = serde_json::from_str(&json).unwrap();

        match deserialized {
            Fault::Domain(d) => assert_eq!(d, TestDomainError::with_message(42, "test error")),
            _ => panic!("Expected Domain variant"),
        }
    }

    #[test]
    fn roundtrip_invariant_never() {
        let err = Fault::<TestDomainError, Never>::Invariant(anyhow::anyhow!(INVARIANT_MSG));
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: Fault<TestDomainError, Never> = serde_json::from_str(&json).unwrap();

        match deserialized {
            Fault::Invariant(e) => assert_eq!(e.to_string(), INVARIANT_MSG),
            _ => panic!("Expected Invariant variant"),
        }
    }

    #[test]
    fn roundtrip_domain_anyhow() {
        let err = Fault::<TestDomainError>::Domain(TestDomainError::with_message(42, "test error"));
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: Fault<TestDomainError> = serde_json::from_str(&json).unwrap();

        match deserialized {
            Fault::Domain(d) => assert_eq!(d, TestDomainError::with_message(42, "test error")),
            _ => panic!("Expected Domain variant"),
        }
    }

    #[test]
    fn roundtrip_transient_anyhow() {
        let err = Fault::<TestDomainError>::Transient(anyhow::anyhow!(TRANSIENT_MSG));
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: Fault<TestDomainError> = serde_json::from_str(&json).unwrap();

        match deserialized {
            Fault::Transient(e) => assert_eq!(e.to_string(), TRANSIENT_MSG),
            _ => panic!("Expected Transient variant"),
        }
    }

    #[test]
    fn roundtrip_invariant_anyhow() {
        let err = Fault::<TestDomainError>::Invariant(anyhow::anyhow!(INVARIANT_MSG));
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: Fault<TestDomainError> = serde_json::from_str(&json).unwrap();

        match deserialized {
            Fault::Invariant(e) => assert_eq!(e.to_string(), INVARIANT_MSG),
            _ => panic!("Expected Invariant variant"),
        }
    }

    #[test]
    fn anyhow_preserves_display_string() {
        let err = Fault::<TestDomainError>::Transient(
            anyhow::anyhow!("inner error").context("outer context"),
        );
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: Fault<TestDomainError> = serde_json::from_str(&json).unwrap();

        match deserialized {
            Fault::Transient(e) => assert_eq!(e.to_string(), "outer context"),
            _ => panic!("Expected Transient variant"),
        }
    }

    #[test]
    fn error_transient_when_never() {
        let err = Fault::<TestDomainError>::Transient(anyhow::anyhow!("some error"));
        let json = serde_json::to_string(&err).unwrap();
        let result: Result<Fault<TestDomainError, Never>, _> = serde_json::from_str(&json);
        assert!(result.is_err());
    }

    #[test]
    fn error_domain_when_never() {
        let err = Fault::<TestDomainError, Never>::Domain(TestDomainError::with_message(
            42,
            "test error",
        ));
        let json = serde_json::to_string(&err).unwrap();
        let result: Result<Fault<Never, Never>, _> = serde_json::from_str(&json);
        assert!(result.is_err());
    }
}
