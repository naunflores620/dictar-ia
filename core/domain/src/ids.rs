//! Identificadores tipados.
//!
//! `SessionId` y `TopicId` son tipos distintos a propósito: pasar uno donde se
//! espera el otro es un error frecuente y el compilador lo detecta gratis.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_type {
    ($name:ident, $prefijo:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn nuevo() -> Self {
                Self(format!("{}_{}", $prefijo, uuid::Uuid::new_v4().simple()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }
    };
}

id_type!(SessionId, "ses");
id_type!(TopicId, "top");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_ids_llevan_prefijo_y_son_unicos() {
        let a = SessionId::nuevo();
        let b = SessionId::nuevo();
        assert!(a.as_str().starts_with("ses_"));
        assert_ne!(a, b);
    }
}
