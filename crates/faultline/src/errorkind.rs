use std::convert::Infallible;
use std::fmt::Display;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

#[derive(
    Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Serialize, Deserialize, JsonSchema,
)]
pub enum Never {}

impl Display for Never {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {}
    }
}

impl Default for Never {
    fn default() -> Self {
        panic!("Never cannot be instantiated")
    }
}

impl std::error::Error for Never {}

impl From<Infallible> for Never {
    fn from(value: Infallible) -> Self {
        match value {}
    }
}

mod sealed {
    #[expect(unnameable_types, reason = "sealed trait pattern")]
    pub trait Sealed {}

    impl Sealed for super::Never {}
    impl Sealed for anyhow::Error {}
}

pub trait ErrorKind: sealed::Sealed + Display + Into<anyhow::Error> {}

impl ErrorKind for Never {}
impl ErrorKind for anyhow::Error {}
