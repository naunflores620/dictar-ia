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

pub mod eventos;
pub mod grabacion;
pub mod proceso;

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
