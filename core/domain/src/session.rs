//! Sesiones y `topic`.
//!
//! Un `Topic` es la entidad organizadora: una asignatura o un cliente. Ambos
//! comparten estructura, glosario acumulado y búsqueda transversal; lo único
//! que cambia es la plantilla de notas que se les aplica. Unificarlos evita
//! duplicar media base de datos.

use crate::ids::{SessionId, TopicId};
use crate::notes::NoteTemplate;
use crate::transcript::DenoiseLevel;
use crate::EpochMs;
use serde::{Deserialize, Serialize};

str_enum! {
    pub enum TopicKind {
        Course => "course",
        Client => "client",
    }
}

str_enum! {
    /// Determina qué plantilla de notas se aplica al finalizar.
    pub enum SessionKind {
        Class        => "class",
        ClientMeeting => "client_meeting",
        TeamMeeting  => "team_meeting",
        VoiceNote    => "voice_note",
    }
}

impl SessionKind {
    pub fn plantilla(&self) -> NoteTemplate {
        match self {
            SessionKind::Class => NoteTemplate::Class,
            SessionKind::ClientMeeting => NoteTemplate::Client,
            SessionKind::TeamMeeting | SessionKind::VoiceNote => NoteTemplate::Team,
        }
    }

    /// Las reuniones con clientes exigen consentimiento explícito antes de
    /// empezar: grabar a un tercero sin avisar es un problema legal, no una
    /// preferencia de la aplicación.
    pub fn requiere_consentimiento_previo(&self) -> bool {
        matches!(self, SessionKind::ClientMeeting)
    }
}

str_enum! {
    pub enum SessionSource {
        /// Loopback del sistema + micrófono. El caso principal.
        DesktopLoopback => "desktop_loopback",
        /// Solo micrófono en el escritorio.
        DesktopMic      => "desktop_mic",
        /// Grabado con el móvil y transferido.
        MobileMic       => "mobile_mic",
        /// Archivo de audio arrastrado a la aplicación.
        Import          => "import",
    }
}

impl SessionSource {
    /// Solo el audio captado por un micrófono en una sala necesita la cadena
    /// de denoise. Aplicarla al loopback, que ya es digital y limpio, empeora
    /// la transcripción.
    pub fn denoise_por_defecto(&self) -> DenoiseLevel {
        match self {
            SessionSource::DesktopLoopback => DenoiseLevel::Off,
            SessionSource::DesktopMic => DenoiseLevel::Light,
            SessionSource::MobileMic | SessionSource::Import => DenoiseLevel::Strong,
        }
    }

    pub fn tiene_pista_sistema(&self) -> bool {
        matches!(self, SessionSource::DesktopLoopback)
    }
}

str_enum! {
    pub enum SessionStatus {
        Recording    => "recording",
        Transcribing => "transcribing",
        Summarizing  => "summarizing",
        Ready        => "ready",
        Failed       => "failed",
    }
}

impl SessionStatus {
    /// Una sesión en `Recording` al arrancar la aplicación significa que el
    /// proceso murió durante la grabación. El audio está en disco: hay que
    /// recuperarla, nunca descartarla.
    pub fn necesita_recuperacion(&self) -> bool {
        matches!(self, SessionStatus::Recording)
    }

    pub fn en_proceso(&self) -> bool {
        matches!(
            self,
            SessionStatus::Recording | SessionStatus::Transcribing | SessionStatus::Summarizing
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Topic {
    pub id: TopicId,
    pub kind: TopicKind,
    /// "Cálculo II" o "Acme S.A."
    pub name: String,
    /// Profesor o persona de contacto.
    pub person: Option<String>,
    /// Cuatrimestre o periodo: "2026-1".
    pub term: Option<String>,
    pub color: Option<String>,
    /// Permiso del docente o del cliente, registrado una sola vez.
    pub consent_ack: bool,
}

impl Topic {
    pub fn nuevo(kind: TopicKind, name: impl Into<String>) -> Self {
        Self {
            id: TopicId::nuevo(),
            kind,
            name: name.into(),
            person: None,
            term: None,
            color: None,
            consent_ack: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub topic_id: Option<TopicId>,
    pub kind: SessionKind,
    pub title: Option<String>,
    pub started_at: EpochMs,
    pub ended_at: Option<EpochMs>,
    pub source: SessionSource,
    /// Aplicación detectada: "meet", "zoom", "teams".
    pub app_captured: Option<String>,
    pub language: Option<String>,
    pub audio_path: Option<String>,
    pub denoise_level: DenoiseLevel,
    pub status: SessionStatus,
    /// Cuando está activo, esta sesión nunca se envía a un proveedor externo,
    /// aunque la configuración global lo permita. Para el cliente que pide
    /// confidencialidad.
    pub solo_local: bool,
}

impl Session {
    pub fn nueva(kind: SessionKind, source: SessionSource, started_at: EpochMs) -> Self {
        Self {
            id: SessionId::nuevo(),
            topic_id: None,
            kind,
            title: None,
            started_at,
            ended_at: None,
            source,
            app_captured: None,
            language: None,
            audio_path: None,
            denoise_level: source.denoise_por_defecto(),
            status: SessionStatus::Recording,
            solo_local: false,
        }
    }

    pub fn duracion_ms(&self) -> Option<i64> {
        self.ended_at.map(|fin| fin - self.started_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn ida_y_vuelta_a_texto_para_sqlite() {
        for k in SessionKind::TODAS {
            assert_eq!(SessionKind::from_str(k.as_str()).unwrap(), *k);
        }
        for s in SessionStatus::TODAS {
            assert_eq!(SessionStatus::from_str(s.as_str()).unwrap(), *s);
        }
    }

    #[test]
    fn el_loopback_no_lleva_denoise() {
        // Aplicar denoise a audio digital limpio degrada la transcripción.
        assert_eq!(
            SessionSource::DesktopLoopback.denoise_por_defecto(),
            DenoiseLevel::Off
        );
        assert_eq!(
            SessionSource::MobileMic.denoise_por_defecto(),
            DenoiseLevel::Strong
        );
    }

    #[test]
    fn una_sesion_grabando_al_arrancar_es_un_crash() {
        assert!(SessionStatus::Recording.necesita_recuperacion());
        assert!(!SessionStatus::Ready.necesita_recuperacion());
    }

    #[test]
    fn las_reuniones_con_cliente_exigen_consentimiento() {
        assert!(SessionKind::ClientMeeting.requiere_consentimiento_previo());
        assert!(!SessionKind::Class.requiere_consentimiento_previo());
    }
}
