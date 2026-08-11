//! Tipos compartidos por todo el sistema.
//!
//! Este crate no depende de nada del proyecto y no hace E/S. Todo lo demás
//! depende de él, así que debe mantenerse pequeño y estable.

#[macro_use]
pub mod strenum;

pub mod alert;
pub mod ids;
pub mod notes;
pub mod session;
pub mod transcript;

pub use alert::{Alert, AlertKind};
pub use ids::{SessionId, TopicId};
pub use notes::{ClassNotes, ClientMinutes, NoteTemplate, RollingSummary, TeamNotes};
pub use session::{Session, SessionKind, SessionSource, SessionStatus, Topic, TopicKind};
pub use transcript::{DenoiseLevel, GlossaryTerm, Slide, Track, Utterance};

/// Milisegundos desde el inicio de la sesión.
///
/// Es un alias deliberado y no un tipo nuevo: aparece en casi todas las
/// estructuras y envolverlo daría más ruido que seguridad. Lo que sí es
/// invariante en todo el sistema: **siempre** es relativo al inicio de la
/// sesión, medido con un reloj monótono, nunca un instante absoluto.
pub type TsMs = i64;

/// Milisegundos desde la época Unix. Para fechas reales de calendario.
pub type EpochMs = i64;

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("valor inválido para {campo}: {motivo}")]
    Invalid { campo: &'static str, motivo: String },

    #[error("no se reconoce el valor «{0}»")]
    Unknown(String),
}

pub type Result<T> = std::result::Result<T, DomainError>;
