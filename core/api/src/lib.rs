//! Fachada única del núcleo.
//!
//! La interfaz —Flutter hoy, cualquier otra mañana— habla **solo** con este
//! crate. No conoce `dictar-audio`, ni `dictar-stt`, ni el esquema de la base
//! de datos. Esa frontera es lo que protege la inversión: cambiar de motor de
//! transcripción, o de tecnología de interfaz, no debería obligar a tocar la
//! otra mitad del proyecto.
//!
//! El puente `flutter_rust_bridge` se genera a partir de este módulo, así que
//! su superficie se mantiene deliberadamente pequeña.

// Generado por flutter_rust_bridge. No se edita a mano; se regenera con
// `flutter_rust_bridge_codegen generate`.
#[allow(clippy::all, dead_code, unused_imports)]
mod frb_generated;

pub mod ajustes;
pub mod eventos;
pub mod grabacion;
pub mod proceso;
pub mod puente;
pub mod transcriptor;

use dictar_domain::{Alert, NoteTemplate, Session, SessionId, Topic, TopicId, Utterance};
use dictar_notes::markdown;
use dictar_providers::config::ProvidersFile;
use dictar_providers::router::{resolver_por_defecto, Router};
use dictar_storage::Db;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub use eventos::{Evento, FaseProceso};
pub use grabacion::{Grabacion, OpcionesGrabacion};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error(transparent)]
    Almacen(#[from] dictar_storage::StorageError),

    #[error(transparent)]
    Audio(#[from] dictar_audio::AudioError),

    #[error(transparent)]
    Stt(#[from] dictar_stt::SttError),

    #[error(transparent)]
    Pantalla(#[from] dictar_screen::ScreenError),

    #[error(transparent)]
    Notas(#[from] dictar_notes::NotesError),

    #[error(transparent)]
    Proveedor(#[from] dictar_providers::ProviderError),

    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),

    #[error("ya hay una grabación en curso")]
    YaGrabando,

    #[error("no hay ninguna grabación en curso")]
    NoGrabando,

    #[error("configuración inválida: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, ApiError>;

/// Punto de entrada del núcleo.
///
/// Se abre una vez al arrancar la aplicación y vive mientras dure. Es `Sync`
/// porque la interfaz llama desde varios hilos.
pub struct Nucleo {
    db: Mutex<Db>,
    dir_datos: PathBuf,
    config: ProvidersFile,
    grabacion: Mutex<Option<Grabacion>>,
    reproductor: Mutex<Option<dictar_audio::reproductor::Reproductor>>,
    /// Transcriptor de la última sesión detenida, vaciando su cola en segundo
    /// plano. `procesar_sesion` lo espera con la barra de progreso delante.
    drenaje: Mutex<Option<(SessionId, std::sync::Arc<transcriptor::TranscriptorVivo>)>>,
}

impl Nucleo {
    /// Abre —o crea— el almacén del usuario.
    pub fn abrir(dir_datos: impl AsRef<Path>) -> Result<Self> {
        let dir_datos = dir_datos.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir_datos)?;
        std::fs::create_dir_all(dir_datos.join("audio"))?;

        let db = Db::abrir(dir_datos.join("dictar.db"))?;
        let config = cargar_config(&dir_datos)?;

        Ok(Self {
            db: Mutex::new(db),
            dir_datos,
            config,
            grabacion: Mutex::new(None),
            reproductor: Mutex::new(None),
            drenaje: Mutex::new(None),
        })
    }

    /// Carpeta de datos por defecto del usuario.
    pub fn dir_por_defecto() -> PathBuf {
        #[cfg(windows)]
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));

        #[cfg(not(windows))]
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("."));

        base.join("dictar_ia")
    }

    pub fn dir_datos(&self) -> &Path {
        &self.dir_datos
    }

    /// Router de proveedores, construido con las claves disponibles.
    ///
    /// Se crea en cada uso y no se guarda: así el usuario puede añadir una
    /// clave en `.env` y que surta efecto sin reiniciar la aplicación.
    pub fn router(&self) -> Result<Router> {
        Ok(Router::desde_config(&self.config, &resolver_por_defecto())?)
    }

    // -- Consulta ------------------------------------------------------------

    pub fn topics(&self) -> Result<Vec<Topic>> {
        Ok(self.db.lock().unwrap().listar_topics(None)?)
    }

    pub fn crear_topic(&self, t: &Topic) -> Result<()> {
        Ok(self.db.lock().unwrap().crear_topic(t)?)
    }

    pub fn sesiones(&self, limite: usize) -> Result<Vec<Session>> {
        Ok(self.db.lock().unwrap().sesiones_recientes(limite)?)
    }

    pub fn sesion(&self, id: &SessionId) -> Result<Session> {
        Ok(self.db.lock().unwrap().sesion(id)?)
    }

    pub fn transcripcion(&self, id: &SessionId) -> Result<Vec<Utterance>> {
        Ok(self.db.lock().unwrap().transcripcion(id)?)
    }

    /// Notas ya renderizadas en Markdown, listas para pintar.
    pub fn notas_markdown(&self, id: &SessionId) -> Result<Option<String>> {
        let db = self.db.lock().unwrap();
        let Some(payload) = db.notas_finales(id)? else {
            return Ok(None);
        };

        let sesion = db.sesion(id)?;
        let titulo = match &sesion.topic_id {
            Some(t) => db
                .topic(t)
                .map(|x| x.name)
                .unwrap_or_else(|_| sesion.title.clone().unwrap_or_else(|| "Sesión".to_owned())),
            None => sesion.title.clone().unwrap_or_else(|| "Sesión".to_owned()),
        };

        Ok(Some(markdown::render(&payload, &titulo)))
    }

    pub fn pendientes(&self) -> Result<Vec<Alert>> {
        Ok(self.db.lock().unwrap().pendientes()?)
    }

    pub fn descartar_aviso(&self, id: i64) -> Result<()> {
        Ok(self.db.lock().unwrap().descartar_aviso(id)?)
    }

    pub fn buscar(&self, consulta: &str) -> Result<Vec<Utterance>> {
        Ok(self.db.lock().unwrap().buscar(consulta, None)?)
    }

    /// Sesiones que quedaron a medias por un cierre inesperado.
    ///
    /// Se consulta al arrancar: el audio está en disco, así que hay que
    /// ofrecer procesarlas. Perder una clase de dos horas por un crash es lo
    /// único que no tiene arreglo después.
    pub fn interrumpidas(&self) -> Result<Vec<Session>> {
        Ok(self.db.lock().unwrap().sesiones_interrumpidas()?)
    }

    pub fn cola_de_proceso(&self) -> Result<Vec<Session>> {
        Ok(self.db.lock().unwrap().cola_de_proceso()?)
    }

    pub fn coste_sesion(&self, id: &SessionId) -> Result<f64> {
        Ok(self.db.lock().unwrap().coste_sesion(id)?)
    }

    /// Glosario acumulado de un `topic`, tal como se le pasa a Whisper.
    pub fn glosario(&self, topic: &TopicId) -> Result<String> {
        // 800 caracteres: Whisper solo atiende a los últimos ~224 tokens del
        // prompt, y pasarse desplazaría los términos importantes fuera de la
        // ventana de contexto.
        Ok(self.db.lock().unwrap().prompt_glosario(topic, 800)?)
    }

    /// Diapositivas capturadas de una sesión.
    pub fn num_diapositivas(&self, id: &SessionId) -> Result<i64> {
        Ok(self.db.lock().unwrap().diapositivas(id)?.len() as i64)
    }

    /// Estado de cada proveedor: `(id, modelo, origen de la clave, es local)`.
    ///
    /// El origen se devuelve porque la pérdida de tiempo más habitual al
    /// configurar esto es tener la clave en un archivo que la aplicación no
    /// mira; decirle al usuario de dónde la ha sacado se lo ahorra.
    pub fn estado_proveedores(&self) -> Vec<(String, String, Option<String>, bool)> {
        let resolver = resolver_por_defecto();

        self.config
            .providers
            .iter()
            .map(|p| {
                let origen = p
                    .api_key_ref
                    .as_deref()
                    .and_then(|r| resolver.resolver_con_origen(r))
                    .map(|(_, o)| o.to_string());
                (p.id.clone(), p.model.clone(), origen, p.es_local())
            })
            .collect()
    }

    /// Hace una petición mínima a un proveedor para ver si responde.
    ///
    /// Devuelve lo que contestó, recortado: ver la respuesta real da más
    /// confianza que un «correcto» genérico, y si el proveedor devuelve algo
    /// raro, se ve.
    pub fn probar_proveedor(&self, id: &str) -> Result<String> {
        use dictar_providers::config::Task;
        use dictar_providers::{CompletionRequest, Message};

        let router = self.router()?;

        // Una pregunta trivial y una respuesta corta: la prueba tiene que
        // costar céntimas, no una llamada de verdad.
        let mut req = CompletionRequest::nueva(vec![Message::usuario(
            "Responde solo con la palabra: correcto",
        )]);
        req.max_tokens = Some(16);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| ApiError::Config(e.to_string()))?;

        // Se prueba SOLO el proveedor pedido: con la cadena de respaldo
        // normal, un fallo suyo quedaría tapado por el siguiente y el usuario
        // creería que su clave funciona.
        let solo_este = dictar_providers::router::Router::desde_config_filtrado(
            &self.config,
            &dictar_providers::router::resolver_por_defecto(),
            id,
        )?;
        let _ = router;

        let salida = rt.block_on(solo_este.texto(Task::Chat, req))?;

        Ok(salida.chars().take(120).collect())
    }

    /// Carpeta de configuración del usuario.
    fn dir_config(&self) -> PathBuf {
        dictar_providers::secretos::dir_configuracion().unwrap_or_else(|| self.dir_datos.clone())
    }

    /// Área a la que se recortan las capturas, si el usuario eligió una.
    pub fn region_captura(&self) -> Option<dictar_screen::Region> {
        self.ajustes()
            .region_captura
            .map(|[x, y, ancho, alto]| dictar_screen::Region { x, y, ancho, alto })
    }

    /// Prepara una captura de la pantalla para que el usuario elija el área.
    ///
    /// Devuelve `(ruta, ancho, alto)`. La imagen va a un archivo temporal, no
    /// a los datos de la sesión: es material de trabajo, no una diapositiva.
    pub fn captura_para_seleccion(&self) -> Result<(String, u32, u32)> {
        let destino = std::env::temp_dir().join("dictar_ia_seleccion.png");
        let (ruta, w, h) = dictar_screen::captura_para_seleccion(destino)?;
        Ok((ruta.to_string_lossy().into_owned(), w, h))
    }

    /// Empieza a reproducir una sesión y devuelve su duración en ms.
    ///
    /// Si había otra sonando, se detiene: dos clases a la vez no son audio,
    /// son ruido.
    pub fn reproducir(&self, id: &SessionId, desde_ms: i64) -> Result<i64> {
        let r = dictar_audio::reproductor::Reproductor::iniciar(&self.dir_audio(id), desde_ms)?;
        let dur = r.duracion_ms();
        *self.reproductor.lock().unwrap() = Some(r);
        Ok(dur)
    }

    /// `(posicion_ms, duracion_ms, pausado, terminado)`, o `None` si no suena.
    pub fn estado_reproduccion(&self) -> Option<(i64, i64, bool, bool)> {
        self.reproductor
            .lock()
            .unwrap()
            .as_ref()
            .map(|r| (r.posicion_ms(), r.duracion_ms(), r.pausado(), r.terminado()))
    }

    pub fn pausar_reproduccion(&self, pausar: bool) {
        if let Some(r) = self.reproductor.lock().unwrap().as_ref() {
            r.pausar(pausar);
        }
    }

    pub fn saltar_reproduccion(&self, ms: i64) {
        if let Some(r) = self.reproductor.lock().unwrap().as_ref() {
            r.saltar(ms);
        }
    }

    pub fn detener_reproduccion(&self) {
        *self.reproductor.lock().unwrap() = None;
    }

    /// Borra una sesión: la base y su audio.
    ///
    /// El listado se llena de pruebas y falsos comienzos; sin poder borrarlos,
    /// encontrar la clase de verdad entre veinte «Clase · 0 min» es buscar
    /// una aguja. Los apuntes ya exportados a la carpeta de Documentos no se
    /// tocan: eso es del usuario.
    pub fn borrar_sesion(&self, id: &SessionId) -> Result<()> {
        // Si es la que suena, primero se calla.
        self.detener_reproduccion();

        self.con_db(|db| db.borrar_sesion(id))?;

        let dir = self.dir_audio(id);
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }

    /// Modelo de Whisper configurado.
    pub fn modelo_configurado(&self) -> dictar_stt::modelos::Modelo {
        use dictar_stt::modelos::Modelo;
        match self.ajustes().modelo.as_deref() {
            Some("small") => Modelo::Small,
            Some("medium") => Modelo::Medium,
            _ => Modelo::LargeV3Turbo,
        }
    }

    pub fn ajustes(&self) -> ajustes::Ajustes {
        ajustes::Ajustes::cargar(&self.dir_config())
    }

    pub fn guardar_ajustes(&self, a: &ajustes::Ajustes) -> Result<()> {
        a.guardar(&self.dir_config())
    }

    /// Escribe los apuntes de una sesión en la carpeta configurada.
    ///
    /// Apuntando a OneDrive, Drive o Nextcloud, los apuntes acaban en el móvil
    /// solos. Se prefiere eso a hablar con la API de cada nube: menos código,
    /// ningún token que caduque, y funciona con el servicio que ya uses.
    ///
    /// Devuelve la ruta escrita, o `None` si no hay carpeta configurada.
    pub fn exportar_apuntes(&self, id: &SessionId) -> Result<Option<PathBuf>> {
        // Sin configurar nada se usa <Documentos>/Apuntes dictar_ia: quien no
        // entre en ajustes nunca vería un apunte fuera de la aplicación, y no
        // sabría que esta exportación existe.
        let Some(carpeta) = self.ajustes().carpeta_efectiva() else {
            return Ok(None);
        };
        let Some(md) = self.notas_markdown(id)? else {
            return Ok(None);
        };

        let sesion = self.con_db(|db| db.sesion(id))?;

        // Una subcarpeta por asignatura o cliente: al final del semestre son
        // cien archivos, y todos juntos no hay quien los encuentre.
        let asignatura = sesion
            .topic_id
            .as_ref()
            .and_then(|t| self.con_db(|db| db.topic(t)).ok())
            .map(|t| t.name)
            .unwrap_or_else(|| "Sin clasificar".to_owned());

        let destino = carpeta.join(ajustes::nombre_seguro(&asignatura));
        std::fs::create_dir_all(&destino)?;

        // La fecha va delante para que el orden alfabético sea el cronológico.
        let fecha = dictar_notes::fecha::iso(sesion.started_at);
        let titulo = sesion
            .title
            .clone()
            .unwrap_or_else(|| sesion.kind.plantilla().nombre_humano().to_owned());

        let archivo = destino.join(format!("{fecha} - {}.md", ajustes::nombre_seguro(&titulo)));

        std::fs::write(&archivo, md)?;
        tracing::info!(archivo = %archivo.display(), "apuntes exportados");
        Ok(Some(archivo))
    }

    pub(crate) fn con_db<T>(&self, f: impl FnOnce(&mut Db) -> T) -> T {
        let mut db = self.db.lock().unwrap();
        f(&mut db)
    }

    pub(crate) fn plantilla_de(&self, s: &Session) -> NoteTemplate {
        s.kind.plantilla()
    }
}

fn cargar_config(dir: &Path) -> Result<ProvidersFile> {
    let propio = dir.join("providers.toml");

    // El archivo del usuario manda; si no existe, la plantilla embebida. Así la
    // aplicación arranca en un equipo recién instalado sin configurar nada.
    let texto = if propio.is_file() {
        std::fs::read_to_string(&propio)?
    } else {
        dictar_providers::config::PLANTILLA_TOML.to_owned()
    };

    ProvidersFile::desde_toml(&texto).map_err(|e| ApiError::Config(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dictar_domain::TopicKind;

    fn nucleo() -> (Nucleo, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let n = Nucleo::abrir(dir.path()).unwrap();
        (n, dir)
    }

    #[test]
    fn abrir_crea_la_estructura_de_datos() {
        let dir = tempfile::tempdir().unwrap();
        Nucleo::abrir(dir.path()).unwrap();

        assert!(dir.path().join("dictar.db").is_file());
        assert!(dir.path().join("audio").is_dir());
    }

    #[test]
    fn abrir_dos_veces_no_pierde_datos() {
        // Es lo que pasa en cada arranque de la aplicación.
        let dir = tempfile::tempdir().unwrap();

        let n = Nucleo::abrir(dir.path()).unwrap();
        n.crear_topic(&Topic::nuevo(TopicKind::Course, "Cálculo II"))
            .unwrap();
        drop(n);

        let n = Nucleo::abrir(dir.path()).unwrap();
        assert_eq!(n.topics().unwrap().len(), 1);
    }

    #[test]
    fn sin_providers_propio_se_usa_la_plantilla_embebida() {
        // Sin esto, la aplicación no arrancaría en un equipo nuevo.
        let (n, _d) = nucleo();
        assert!(!n.config.providers.is_empty());
        assert!(
            n.config.providers.iter().any(|p| p.es_local()),
            "debe quedar el respaldo local, que funciona sin claves"
        );
    }

    #[test]
    fn una_sesion_sin_notas_devuelve_none_en_vez_de_fallar() {
        use dictar_domain::{Session, SessionKind, SessionSource};

        let (n, _d) = nucleo();
        let s = Session::nueva(SessionKind::Class, SessionSource::DesktopLoopback, 0);
        n.con_db(|db| db.crear_sesion(&s)).unwrap();

        assert!(n.notas_markdown(&s.id).unwrap().is_none());
    }

    #[test]
    fn la_carpeta_por_defecto_cuelga_de_los_datos_del_usuario() {
        let d = Nucleo::dir_por_defecto();
        assert!(d.ends_with("dictar_ia"));
    }
}
