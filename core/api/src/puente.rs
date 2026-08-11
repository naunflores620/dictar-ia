//! Superficie del puente con Flutter.
//!
//! Este módulo es lo **único** que `flutter_rust_bridge` traduce a Dart, y por
//! eso se mantiene deliberadamente estrecho: funciones libres, tipos planos y
//! nada de genéricos. Todo lo interesante ocurre detrás, en [`crate::Nucleo`].
//!
//! Los tipos de aquí (`*Dto`) duplican en parte los del dominio, y es a
//! propósito: el puente cruza un FFI y conviene que lo que lo atraviesa sean
//! datos simples, sin `Option<Box<dyn ...>>` ni ciclos. Además desacopla la
//! interfaz de los cambios internos del dominio.

use crate::eventos::Evento;
use crate::{Nucleo, OpcionesGrabacion};
use dictar_domain::{SessionId, SessionKind, Topic, TopicId, TopicKind};
use dictar_stt::modelos::Modelo;
use std::sync::mpsc::Receiver;
use std::sync::{Mutex, OnceLock};

/// Instancia única del núcleo.
///
/// Un global y no un objeto que viaje al lado Dart: pasar tipos opacos por el
/// puente complica la vida de ambos lados sin ganar nada, porque la aplicación
/// solo abre un almacén y lo tiene abierto mientras dura.
static NUCLEO: OnceLock<Nucleo> = OnceLock::new();

/// Canal de eventos de la grabación en curso, a la espera de que Dart se
/// suscriba.
static EVENTOS: Mutex<Option<Receiver<Evento>>> = Mutex::new(None);

fn nucleo() -> Result<&'static Nucleo, String> {
    NUCLEO
        .get()
        .ok_or_else(|| "el núcleo no está inicializado: llama antes a inicializar()".to_owned())
}

// ---------------------------------------------------------------------------
// Tipos que cruzan el puente
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TopicDto {
    pub id: String,
    /// "course" o "client".
    pub kind: String,
    pub name: String,
    pub person: Option<String>,
    pub term: Option<String>,
    pub consent_ack: bool,
}

#[derive(Debug, Clone)]
pub struct SesionDto {
    pub id: String,
    pub topic_id: Option<String>,
    pub kind: String,
    pub status: String,
    pub title: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub app_captured: Option<String>,
    pub num_diapositivas: i64,
    pub coste_usd: f64,
}

#[derive(Debug, Clone)]
pub struct FraseDto {
    pub track: String,
    pub speaker: Option<String>,
    pub inicio_ms: i64,
    pub fin_ms: i64,
    pub texto: String,
    pub definitiva: bool,
}

#[derive(Debug, Clone)]
pub struct AvisoDto {
    pub id: i64,
    pub session_id: String,
    pub topic_id: Option<String>,
    pub kind: String,
    pub texto: String,
    pub fecha: Option<String>,
    pub ts_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ProveedorDto {
    pub id: String,
    pub modelo: String,
    /// De dónde salió la clave, o `None` si no hay.
    pub origen_clave: Option<String>,
    pub es_local: bool,
}

#[derive(Debug, Clone)]
pub struct ProcesadaDto {
    pub session_id: String,
    pub frases: i64,
    pub avisos: i64,
    pub coste_usd: f64,
    pub proveedor: String,
}

/// Evento en la forma que espera Dart: un enum plano con campos opcionales.
///
/// Se aplana en vez de usar un enum con datos porque el generador produce
/// código mucho más simple de consumir, y la interfaz solo hace un `switch`
/// sobre `tipo`.
#[derive(Debug, Clone)]
pub struct EventoDto {
    /// "niveles" | "frase" | "progreso" | "terminada" | "error"
    pub tipo: String,

    // Niveles
    pub nivel_mic: f32,
    pub nivel_sistema: f32,
    pub transcurrido_ms: i64,

    // Frase
    pub track: Option<String>,
    pub texto: Option<String>,
    pub inicio_ms: i64,
    pub fin_ms: i64,
    pub definitiva: bool,

    // Progreso
    pub fase: Option<String>,
    pub fraccion: f32,
    pub detalle: Option<String>,

    // Terminada
    pub coste_usd: f64,
    pub avisos: i64,

    // Error
    pub mensaje: Option<String>,
}

impl Default for EventoDto {
    fn default() -> Self {
        Self {
            tipo: String::new(),
            nivel_mic: 0.0,
            nivel_sistema: 0.0,
            transcurrido_ms: 0,
            track: None,
            texto: None,
            inicio_ms: 0,
            fin_ms: 0,
            definitiva: false,
            fase: None,
            fraccion: 0.0,
            detalle: None,
            coste_usd: 0.0,
            avisos: 0,
            mensaje: None,
        }
    }
}

impl From<Evento> for EventoDto {
    fn from(e: Evento) -> Self {
        match e {
            Evento::Niveles {
                mic,
                sistema,
                transcurrido_ms,
            } => EventoDto {
                tipo: "niveles".into(),
                nivel_mic: mic,
                nivel_sistema: sistema,
                transcurrido_ms,
                ..Default::default()
            },
            Evento::Frase {
                track,
                inicio_ms,
                fin_ms,
                texto,
                definitiva,
                ..
            } => EventoDto {
                tipo: "frase".into(),
                track: Some(track),
                texto: Some(texto),
                inicio_ms,
                fin_ms,
                definitiva,
                ..Default::default()
            },
            Evento::Progreso {
                fase,
                fraccion,
                detalle,
            } => EventoDto {
                tipo: "progreso".into(),
                fase: Some(fase.etiqueta().to_owned()),
                fraccion,
                detalle: Some(detalle),
                ..Default::default()
            },
            Evento::Terminada {
                coste_usd, avisos, ..
            } => EventoDto {
                tipo: "terminada".into(),
                coste_usd,
                avisos: avisos as i64,
                ..Default::default()
            },
            Evento::Error { mensaje, .. } => EventoDto {
                tipo: "error".into(),
                mensaje: Some(mensaje),
                ..Default::default()
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Funciones del puente
// ---------------------------------------------------------------------------

/// Abre el almacén del usuario. Debe llamarse una vez al arrancar.
///
/// Pasar `None` usa la carpeta estándar del sistema.
pub fn inicializar(dir_datos: Option<String>) -> Result<String, String> {
    let dir = dir_datos
        .map(std::path::PathBuf::from)
        .unwrap_or_else(Nucleo::dir_por_defecto);

    let n = Nucleo::abrir(&dir).map_err(|e| e.to_string())?;

    // Si ya estaba inicializado se ignora, en vez de fallar: un *hot reload* de
    // Flutter vuelve a ejecutar el arranque, y eso no debería romper nada.
    let _ = NUCLEO.set(n);
    Ok(dir.to_string_lossy().into_owned())
}

pub fn listar_topics() -> Result<Vec<TopicDto>, String> {
    let n = nucleo()?;
    Ok(n.topics()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|t| TopicDto {
            id: t.id.0,
            kind: t.kind.as_str().to_owned(),
            name: t.name,
            person: t.person,
            term: t.term,
            consent_ack: t.consent_ack,
        })
        .collect())
}

pub fn crear_topic(
    nombre: String,
    persona: Option<String>,
    es_cliente: bool,
) -> Result<String, String> {
    let n = nucleo()?;
    let kind = if es_cliente {
        TopicKind::Client
    } else {
        TopicKind::Course
    };

    let mut t = Topic::nuevo(kind, nombre);
    t.person = persona;
    n.crear_topic(&t).map_err(|e| e.to_string())?;
    Ok(t.id.0)
}

pub fn listar_sesiones(limite: u32) -> Result<Vec<SesionDto>, String> {
    let n = nucleo()?;
    let sesiones = n.sesiones(limite as usize).map_err(|e| e.to_string())?;

    Ok(sesiones
        .into_iter()
        .map(|s| {
            let coste = n.coste_sesion(&s.id).unwrap_or(0.0);
            let diapos = n.num_diapositivas(&s.id).unwrap_or(0);
            SesionDto {
                id: s.id.0,
                topic_id: s.topic_id.map(|t| t.0),
                kind: s.kind.as_str().to_owned(),
                status: s.status.as_str().to_owned(),
                title: s.title,
                started_at: s.started_at,
                ended_at: s.ended_at,
                app_captured: s.app_captured,
                num_diapositivas: diapos,
                coste_usd: coste,
            }
        })
        .collect())
}

pub fn obtener_transcripcion(session_id: String) -> Result<Vec<FraseDto>, String> {
    let n = nucleo()?;
    Ok(n.transcripcion(&SessionId::from(session_id))
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|u| FraseDto {
            track: u.track.as_str().to_owned(),
            speaker: u.speaker,
            inicio_ms: u.start_ms,
            fin_ms: u.end_ms,
            texto: u.text,
            definitiva: u.is_final,
        })
        .collect())
}

/// Notas ya renderizadas en Markdown.
pub fn obtener_notas(session_id: String) -> Result<Option<String>, String> {
    nucleo()?
        .notas_markdown(&SessionId::from(session_id))
        .map_err(|e| e.to_string())
}

pub fn listar_pendientes() -> Result<Vec<AvisoDto>, String> {
    Ok(nucleo()?
        .pendientes()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|a| AvisoDto {
            id: a.id,
            session_id: a.session_id.0,
            topic_id: a.topic_id.map(|t| t.0),
            kind: a.kind.as_str().to_owned(),
            texto: a.text,
            fecha: a.due_date,
            ts_ms: a.ts_ms,
        })
        .collect())
}

pub fn descartar_aviso(id: i64) -> Result<(), String> {
    nucleo()?.descartar_aviso(id).map_err(|e| e.to_string())
}

pub fn buscar(consulta: String) -> Result<Vec<FraseDto>, String> {
    Ok(nucleo()?
        .buscar(&consulta)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|u| FraseDto {
            track: u.track.as_str().to_owned(),
            speaker: u.speaker,
            inicio_ms: u.start_ms,
            fin_ms: u.end_ms,
            texto: u.text,
            definitiva: u.is_final,
        })
        .collect())
}

/// Sesiones que quedaron a medias por un cierre inesperado.
pub fn sesiones_interrumpidas() -> Result<Vec<SesionDto>, String> {
    let n = nucleo()?;
    Ok(n.interrumpidas()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|s| SesionDto {
            id: s.id.0,
            topic_id: s.topic_id.map(|t| t.0),
            kind: s.kind.as_str().to_owned(),
            status: s.status.as_str().to_owned(),
            title: s.title,
            started_at: s.started_at,
            ended_at: s.ended_at,
            app_captured: s.app_captured,
            num_diapositivas: 0,
            coste_usd: 0.0,
        })
        .collect())
}

/// Estado de los proveedores de IA, para la pantalla de ajustes.
pub fn listar_proveedores() -> Result<Vec<ProveedorDto>, String> {
    let n = nucleo()?;
    Ok(n.estado_proveedores()
        .into_iter()
        .map(|(id, modelo, origen, local)| ProveedorDto {
            id,
            modelo,
            origen_clave: origen,
            es_local: local,
        })
        .collect())
}

/// Guarda la clave de API de un proveedor, o la borra con `None`.
///
/// Escribe en `~/.config/dictar_ia/.env` con permisos 0600. Surte efecto en la
/// siguiente petición, sin reiniciar: el router se construye en cada uso.
pub fn guardar_clave(proveedor: String, clave: Option<String>) -> Result<String, String> {
    let referencia = format!("keyring:{proveedor}");
    let limpia = clave.map(|c| c.trim().to_owned()).filter(|c| !c.is_empty());

    dictar_providers::secretos::guardar_clave(&referencia, limpia.as_deref())
        .map(|r| r.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

/// Comprueba que un proveedor responde de verdad.
///
/// Hace una petición mínima real en lugar de validar el formato de la clave:
/// una clave con la forma correcta pero revocada, o un identificador de modelo
/// que ya no existe, solo se detectan preguntando. Descubrirlo aquí y no a
/// mitad de una clase es toda la diferencia.
pub fn probar_proveedor(id: String) -> Result<String, String> {
    let n = nucleo()?;
    n.probar_proveedor(&id).map_err(|e| e.to_string())
}

// -- Grabación ---------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn iniciar_grabacion(
    tipo: String,
    topic_id: Option<String>,
    titulo: Option<String>,
    capturar_sistema: bool,
    capturar_diapositivas: bool,
    solo_local: bool,
    ahora_ms: i64,
) -> Result<String, String> {
    let n = nucleo()?;

    let kind = match tipo.as_str() {
        "class" => SessionKind::Class,
        "client_meeting" => SessionKind::ClientMeeting,
        "voice_note" => SessionKind::VoiceNote,
        _ => SessionKind::TeamMeeting,
    };

    let (id, rx) = n
        .iniciar_grabacion(
            OpcionesGrabacion {
                kind,
                topic_id: topic_id.map(TopicId::from),
                titulo,
                capturar_sistema,
                capturar_diapositivas,
                solo_local,
            },
            ahora_ms,
        )
        .map_err(|e| e.to_string())?;

    // El receptor queda a la espera de que Dart abra el flujo. No se puede
    // entregar en esta misma llamada porque el generador trata los flujos como
    // una función aparte.
    *EVENTOS.lock().unwrap() = Some(rx);
    Ok(id.0)
}

pub fn detener_grabacion(ahora_ms: i64) -> Result<String, String> {
    let id = nucleo()?
        .detener_grabacion(ahora_ms)
        .map_err(|e| e.to_string())?;
    Ok(id.0)
}

pub fn grabando() -> Result<Option<String>, String> {
    Ok(nucleo()?.grabando().map(|id| id.0))
}

/// Consume los eventos de la grabación en curso.
///
/// Devuelve el siguiente evento, o `None` cuando la grabación termina. Se
/// consulta en bucle desde Dart: es más simple que un `StreamSink` y evita que
/// el generador tenga que gestionar el ciclo de vida de un flujo a través del
/// FFI.
pub fn siguiente_evento(espera_ms: u32) -> Result<Option<EventoDto>, String> {
    let guard = EVENTOS.lock().unwrap();
    let Some(rx) = guard.as_ref() else {
        return Ok(None);
    };

    match rx.recv_timeout(std::time::Duration::from_millis(espera_ms as u64)) {
        Ok(e) => Ok(Some(e.into())),
        // Ni el tiempo agotado ni el canal cerrado son errores: el primero
        // significa «todavía nada» y el segundo «ya terminó». La interfaz
        // trata ambos igual y sigue consultando.
        Err(_) => Ok(None),
    }
}

/// Transcribe y genera las notas de una sesión ya grabada.
///
/// Es la llamada larga —minutos— así que en Dart va en un aislado propio para
/// no bloquear la interfaz.
pub fn procesar_sesion(session_id: String, ahora_ms: i64) -> Result<ProcesadaDto, String> {
    let n = nucleo()?;
    let id = SessionId::from(session_id);

    // El progreso viaja por el mismo canal que los eventos de grabación, así
    // que la interfaz no necesita un segundo mecanismo para enseñar la barra:
    // sigue llamando a `siguiente_evento` igual que durante la grabación.
    let (tx, rx) = std::sync::mpsc::channel();
    *EVENTOS.lock().unwrap() = Some(rx);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;

    let r = rt
        .block_on(n.procesar_sesion(&id, Modelo::LargeV3Turbo, ahora_ms, Some(tx)))
        .map_err(|e| e.to_string())?;

    Ok(ProcesadaDto {
        session_id: r.session_id.0,
        frases: r.frases as i64,
        avisos: r.avisos as i64,
        coste_usd: r.coste_usd,
        proveedor: r.proveedor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_inicializar_el_error_dice_que_hacer() {
        // Si el núcleo no está abierto, el mensaje tiene que llevar a la
        // solución, no limitarse a fallar.
        // `Nucleo` no implementa Debug, así que no vale `unwrap_err()`.
        match nucleo() {
            Err(e) => assert!(e.contains("inicializar()"), "mensaje: {e}"),
            Ok(_) => { /* otro test ya lo inicializó: no es un fallo */ }
        }
    }

    #[test]
    fn los_eventos_se_aplanan_conservando_lo_suyo() {
        let e: EventoDto = Evento::Niveles {
            mic: 0.4,
            sistema: 0.8,
            transcurrido_ms: 5000,
        }
        .into();
        assert_eq!(e.tipo, "niveles");
        assert_eq!(e.nivel_sistema, 0.8);
        assert_eq!(e.transcurrido_ms, 5000);
        assert!(e.texto.is_none());
    }

    #[test]
    fn una_frase_cruza_con_su_pista_y_sus_tiempos() {
        let e: EventoDto = Evento::frase(
            &SessionId::from("s"),
            dictar_domain::Track::System,
            1000,
            4000,
            "hola",
            false,
        )
        .into();

        assert_eq!(e.tipo, "frase");
        assert_eq!(e.track.as_deref(), Some("system"));
        assert_eq!(e.texto.as_deref(), Some("hola"));
        assert!(!e.definitiva);
    }

    #[test]
    fn el_progreso_lleva_la_fase_ya_traducida() {
        // La interfaz pinta la etiqueta tal cual: traducirla aquí evita
        // duplicar el diccionario de fases en Dart.
        let e: EventoDto =
            Evento::progreso(crate::eventos::FaseProceso::Transcribiendo, 0.5, "3 de 6").into();

        assert_eq!(e.tipo, "progreso");
        assert_eq!(e.fase.as_deref(), Some("Transcribiendo"));
        assert_eq!(e.fraccion, 0.5);
    }
}
