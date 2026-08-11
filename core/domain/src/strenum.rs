//! Macro para enumeraciones que se guardan como texto en SQLite.
//!
//! Genera `as_str`, `FromStr`, `Display` y las conversiones de `rusqlite` en un
//! solo sitio, de forma que el valor que va a la base de datos y el que vuelve
//! no puedan divergir.

#[macro_export]
macro_rules! str_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$vmeta:meta])*
                $variant:ident => $texto:literal
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash,
                 serde::Serialize, serde::Deserialize)]
        $vis enum $name {
            $(
                $(#[$vmeta])*
                #[serde(rename = $texto)]
                $variant
            ),+
        }

        impl $name {
            pub const TODAS: &'static [$name] = &[ $( $name::$variant ),+ ];

            pub fn as_str(&self) -> &'static str {
                match self {
                    $( $name::$variant => $texto ),+
                }
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::DomainError;

            fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
                match s {
                    $( $texto => Ok($name::$variant), )+
                    otro => Err($crate::DomainError::Unknown(otro.to_owned())),
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}
