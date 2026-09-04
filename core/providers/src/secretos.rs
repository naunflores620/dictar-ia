//! De dónde salen las claves de API.
//!
//! Tres orígenes, por orden de prioridad:
//!
//! 1. **Variables de entorno** — mandan siempre. Es lo que permite fijar una
//!    clave puntual para una ejecución sin tocar ningún archivo, y lo que usa CI.
//! 2. **Archivo `.env`** — lo cómodo para desarrollar. Se busca en el directorio
//!    actual, en la raíz del proyecto y en la carpeta de configuración del
//!    usuario.
//! 3. **Llavero del sistema operativo** — lo correcto para la aplicación
//!    instalada: Credential Manager en Windows, Keychain en macOS y, en
//!    Linux, el Secret Service vía D-Bus. El crate `keyring` resuelve Linux
//!    con `zbus-secret-service-keyring-store`, que es cliente D-Bus en Rust
//!    puro — **no enlaza `libsecret`**, pese a lo que hacía suponer el nombre
//!    del paquete de sistema que este repositorio arrastra desde antes de que
//!    existiera este archivo. Solo en escritorio: en Android no hay llavero
//!    al que preguntar, y la dependencia queda fuera de la compilación (ver
//!    `Cargo.toml`).
//!
//! Una clave en un `.env` está en texto plano en el disco. Para desarrollo es
//! aceptable; para la aplicación que uses a diario, el llavero es lo suyo. Por
//! eso el orden es una cadena y no una sola fuente.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Resuelve una referencia como `keyring:gemini` o `env:GEMINI_API_KEY`.
pub trait KeyResolver: Send + Sync {
    fn resolver(&self, referencia: &str) -> Option<String>;
}

/// Nombres de variable que se prueban para una referencia dada.
///
/// `keyring:gemini` busca, en este orden: `gemini`, `GEMINI` y `GEMINI_API_KEY`.
/// La última es la convención habitual y la que documentamos.
pub fn nombres_candidatos(referencia: &str) -> Vec<String> {
    let base = referencia
        .split_once(':')
        .map(|(_, resto)| resto)
        .unwrap_or(referencia)
        .trim();

    let mayus = base.to_uppercase();
    let mut v = vec![base.to_owned()];
    if mayus != base {
        v.push(mayus.clone());
    }
    if !mayus.ends_with("_API_KEY") {
        v.push(format!("{mayus}_API_KEY"));
    }
    v
}

/// El nombre "canónico" de una referencia: el último de [`nombres_candidatos`],
/// que es la convención `_API_KEY` que documentamos.
///
/// El llavero, a diferencia del entorno y del `.env`, solo lo escribe esta
/// misma aplicación —nadie edita a mano una entrada del Credential Manager
/// esperando que `dictar_ia` la encuentre—, así que no hace falta probar
/// varios nombres al leer: basta con que lectura y escritura usen el mismo.
fn nombre_canonico(referencia: &str) -> String {
    nombres_candidatos(referencia)
        .into_iter()
        .last()
        .unwrap_or_else(|| referencia.to_uppercase())
}

// ---------------------------------------------------------------------------
// Variables de entorno
// ---------------------------------------------------------------------------

pub struct EnvResolver;

impl KeyResolver for EnvResolver {
    fn resolver(&self, referencia: &str) -> Option<String> {
        nombres_candidatos(referencia)
            .into_iter()
            .find_map(|n| std::env::var(&n).ok())
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    }
}

// ---------------------------------------------------------------------------
// Archivo .env
// ---------------------------------------------------------------------------

/// Lee las claves de un archivo `.env`.
///
/// No usa ninguna dependencia: el formato son pares `CLAVE=valor`, y admitir
/// más que eso solo invita a que el archivo se convierta en un lenguaje.
pub struct DotEnvResolver {
    valores: HashMap<String, String>,
    origen: Option<PathBuf>,
}

impl DotEnvResolver {
    /// Carga el primer `.env` que encuentre entre las rutas habituales.
    pub fn buscar() -> Self {
        for ruta in rutas_dotenv() {
            if ruta.is_file() {
                if let Ok(texto) = std::fs::read_to_string(&ruta) {
                    return Self {
                        valores: parsear(&texto),
                        origen: Some(ruta),
                    };
                }
            }
        }
        Self {
            valores: HashMap::new(),
            origen: None,
        }
    }

    pub fn desde_archivo(ruta: impl AsRef<Path>) -> std::io::Result<Self> {
        let ruta = ruta.as_ref().to_path_buf();
        let texto = std::fs::read_to_string(&ruta)?;
        Ok(Self {
            valores: parsear(&texto),
            origen: Some(ruta),
        })
    }

    pub fn desde_texto(texto: &str) -> Self {
        Self {
            valores: parsear(texto),
            origen: None,
        }
    }

    /// Archivo del que se cargó, para poder decírselo al usuario.
    pub fn origen(&self) -> Option<&Path> {
        self.origen.as_deref()
    }

    pub fn esta_vacio(&self) -> bool {
        self.valores.is_empty()
    }
}

impl KeyResolver for DotEnvResolver {
    fn resolver(&self, referencia: &str) -> Option<String> {
        nombres_candidatos(referencia)
            .into_iter()
            .find_map(|n| self.valores.get(&n).cloned())
    }
}

/// Dónde se busca el `.env`, en orden.
pub fn rutas_dotenv() -> Vec<PathBuf> {
    let mut rutas = Vec::new();

    // 1. Junto a donde se ejecuta.
    if let Ok(cwd) = std::env::current_dir() {
        rutas.push(cwd.join(".env"));
        // 2. Un nivel arriba: cómodo al ejecutar desde un subdirectorio del
        //    repositorio.
        if let Some(padre) = cwd.parent() {
            rutas.push(padre.join(".env"));
        }
    }

    // 3. Configuración del usuario, que es donde vive en la app instalada.
    if let Some(cfg) = dir_configuracion() {
        rutas.push(cfg.join(".env"));
    }

    rutas
}

/// Carpeta de configuración de la aplicación, según el sistema.
pub fn dir_configuracion() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("dictar_ia"))
    }

    #[cfg(not(windows))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|c| c.join("dictar_ia"))
    }
}

fn parsear(texto: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();

    for linea in texto.lines() {
        let l = linea.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }

        // `export CLAVE=valor` también vale: es como se escribe al copiarlo de
        // la terminal, y rechazarlo solo genera desconcierto.
        let l = l.strip_prefix("export ").unwrap_or(l).trim_start();

        let Some((clave, valor)) = l.split_once('=') else {
            continue;
        };

        let clave = clave.trim();
        if clave.is_empty() {
            continue;
        }

        let mut valor = valor.trim();

        // Comentario al final de la línea, solo si el valor no va entrecomillado.
        let entrecomillado = (valor.starts_with('"') && valor.ends_with('"') && valor.len() >= 2)
            || (valor.starts_with('\'') && valor.ends_with('\'') && valor.len() >= 2);

        if entrecomillado {
            valor = &valor[1..valor.len() - 1];
        } else if let Some(i) = valor.find(" #") {
            valor = valor[..i].trim_end();
        }

        if !valor.is_empty() {
            m.insert(clave.to_owned(), valor.to_owned());
        }
    }

    m
}

// ---------------------------------------------------------------------------
// Llavero del sistema operativo
// ---------------------------------------------------------------------------

/// Nombre bajo el que se agrupan todas las claves de la aplicación en el
/// llavero. Un único "servicio" con una entrada por variable canónica, en vez
/// de un servicio por proveedor: así el Administrador de credenciales de
/// Windows o Seahorse en Linux enseñan todo agrupado bajo `dictar_ia` en vez
/// de una fila suelta por cada proveedor de IA.
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
const SERVICIO_LLAVERO: &str = "dictar_ia";

/// Resuelve una clave contra el llavero nativo del sistema.
///
/// Solo en escritorio: en Android no hay llavero al que enlazar, y la
/// dependencia `keyring` queda fuera de la compilación (ver `Cargo.toml`), así
/// que esta es una de dos definiciones con la misma firma —esta y la de más
/// abajo—, igual que hace `dictar_screen::iniciar` con `xcap`. Quien llama
/// (`resolver_por_defecto`) no escribe un solo `#[cfg]`.
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
pub struct LlaveroResolver;

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
impl LlaveroResolver {
    pub fn nuevo() -> Self {
        Self
    }
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
impl KeyResolver for LlaveroResolver {
    fn resolver(&self, referencia: &str) -> Option<String> {
        // Cualquier fallo se convierte en `None`, nunca en pánico ni en un
        // error propagado: sin sesión gráfica, sin demonio de secretos, con
        // D-Bus caído... Es el último eslabón de la cadena, y el que no
        // sabe, calla.
        let nombre = nombre_canonico(referencia);
        keyring::Entry::new(SERVICIO_LLAVERO, &nombre)
            .ok()?
            .get_password()
            .ok()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub struct LlaveroResolver;

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
impl LlaveroResolver {
    pub fn nuevo() -> Self {
        Self
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
impl KeyResolver for LlaveroResolver {
    fn resolver(&self, _referencia: &str) -> Option<String> {
        // En Android no hay llavero de sistema al que preguntar.
        None
    }
}

/// Intenta escribir —o borrar, con `None`— en el llavero. Devuelve si lo
/// consiguió.
///
/// Cualquier fallo hace que [`guardar_clave`] recurra al `.env`: guardar la
/// clave en el sitio menos ideal es mejor que no guardarla, y con [`Origen`]
/// el usuario sigue sabiendo dónde quedó. No se distingue "no hay llavero" de
/// "el llavero falló al escribir": el borrado de una entrada que nunca
/// existió ahí también cuenta como fallo a propósito, para que se recurra al
/// `.env` y se limpie ahí una clave vieja si la hubiera — sin este detalle,
/// borrar una clave que solo vivía en el `.env` "tendría éxito" en el
/// llavero sin tocar el archivo, y el valor viejo seguiría activo porque el
/// `.env` manda sobre el llavero al leer.
///
/// Al guardar (`Some(v)`), además de escribir se relee de inmediato antes de
/// dar la operación por buena. Es la decisión que le falta a
/// [`purgar_del_env`] cuando decide si retirar la única copia de respaldo
/// que quedaba en el `.env`: "escribir con éxito" tiene que significar "de
/// verdad disponible para la próxima lectura de esta misma sesión", no solo
/// "la llamada a `set_password` no devolvió error" —hay backends donde eso
/// no es lo mismo, por ejemplo si la escritura y la lectura resolvieran a
/// una colección de Secret Service distinta—. No cubre el riesgo de que una
/// sesión *futura* se quede sin llavero (ver el comentario de
/// [`purgar_del_env`]), solo el de purgar un respaldo por un éxito que en
/// realidad nunca quedó accesible.
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
fn escribir_en_llavero(referencia: &str, valor: Option<&str>) -> bool {
    let nombre = nombre_canonico(referencia);
    let resultado = keyring::Entry::new(SERVICIO_LLAVERO, &nombre).and_then(|entrada| {
        match valor {
            // Ninguna prueba pasa por esta rama —es la única línea de todo
            // el archivo que sigue tocando el almacén real, ver el
            // comentario de `registrar_resultado_de_llavero`—, así que si
            // algún día hace falta diagnosticar un fallo aquí con
            // `tracing`, que el evento lleve `referencia` o el error, pero
            // NUNCA `v`: eso filtraría el valor de la clave sin que ninguna
            // prueba automática lo notara.
            Some(v) => {
                entrada.set_password(v)?;
                entrada.get_password().map(|_| ())
            }
            None => entrada.delete_credential(),
        }
    });
    registrar_resultado_de_llavero(resultado)
}

/// Qué hacer con el resultado de haber tocado el llavero: avisar por
/// `tracing` si falló —nunca con el valor que se intentaba guardar, solo con
/// el error de la plataforma— y decir si tuvo éxito.
///
/// Aislado de [`escribir_en_llavero`] para poder comprobar sus dos ramas sin
/// necesitar un llavero real: esta función no toca `keyring::Entry` en
/// absoluto, así que un test puede fabricar un `Ok`/`Err` sintético en vez de
/// escribir de verdad en el Credential Manager o el Secret Service. La única
/// línea de todo el archivo que sigue tocando el almacén real es la de
/// arriba, y por construcción no puede filtrar la clave: ni siquiera recibe
/// `valor` como parámetro.
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
fn registrar_resultado_de_llavero(resultado: keyring::Result<()>) -> bool {
    if let Err(e) = &resultado {
        tracing::warn!(error = %e, "no se pudo usar el llavero del sistema, se recurre al .env");
    }
    resultado.is_ok()
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn escribir_en_llavero(_referencia: &str, _valor: Option<&str>) -> bool {
    false
}

// ---------------------------------------------------------------------------
// Cadena
// ---------------------------------------------------------------------------

/// De dónde salió una clave. Se muestra al usuario para que pueda depurar por
/// qué su clave «no se coge»: casi siempre está en el archivo equivocado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origen {
    Entorno,
    Archivo(PathBuf),
    Llavero,
    Memoria,
}

impl std::fmt::Display for Origen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Origen::Entorno => write!(f, "variable de entorno"),
            Origen::Archivo(p) => write!(f, "{}", p.display()),
            Origen::Llavero => write!(f, "llavero del sistema"),
            Origen::Memoria => write!(f, "memoria"),
        }
    }
}

/// Prueba varios orígenes en orden y se queda con el primero que responda.
pub struct CadenaResolvers {
    eslabones: Vec<(Origen, Box<dyn KeyResolver>)>,
}

impl CadenaResolvers {
    pub fn nueva() -> Self {
        Self {
            eslabones: Vec::new(),
        }
    }

    pub fn con(mut self, origen: Origen, r: Box<dyn KeyResolver>) -> Self {
        self.eslabones.push((origen, r));
        self
    }

    /// Igual que `resolver`, pero diciendo de dónde salió.
    pub fn resolver_con_origen(&self, referencia: &str) -> Option<(String, Origen)> {
        self.eslabones
            .iter()
            .find_map(|(o, r)| r.resolver(referencia).map(|v| (v, o.clone())))
    }
}

impl KeyResolver for CadenaResolvers {
    fn resolver(&self, referencia: &str) -> Option<String> {
        self.eslabones
            .iter()
            .find_map(|(_, r)| r.resolver(referencia))
    }
}

/// Compone los tres eslabones ya construidos en el orden que fija el
/// criterio 2 de la HU-05: entorno, luego archivo, luego llavero, sin alterar
/// los dos primeros.
///
/// Aislada de [`resolver_por_defecto`] para poder comprobar el orden real de
/// ensamblado sin depender de variables de entorno ni de un `.env` reales:
/// mutar variables de entorno globales desde un test que corre en paralelo
/// con otros hace que se pisen entre sí —el mismo motivo, ya razonado en el
/// comentario de [`guardar_clave_en`], por el que ese otro punto del archivo
/// tampoco toca `XDG_CONFIG_HOME`—. `resolver_por_defecto` no hace nada más
/// que llamar a esto con las piezas reales, así que una prueba que reordene
/// los `.con(...)` de aquí sigue ejercitando exactamente lo que usa la
/// aplicación.
fn ensamblar_cadena_por_defecto(
    entorno: Box<dyn KeyResolver>,
    origen_archivo: Origen,
    archivo: Box<dyn KeyResolver>,
    llavero: Box<dyn KeyResolver>,
) -> CadenaResolvers {
    CadenaResolvers::nueva()
        .con(Origen::Entorno, entorno)
        .con(origen_archivo, archivo)
        .con(Origen::Llavero, llavero)
}

/// Arma la cadena a partir de un resolutor de entorno, un `DotEnvResolver` ya
/// cargado y un resolutor de llavero, y decide con ellos las dos cosas que
/// [`ensamblar_cadena_por_defecto`] no decide por sí sola: qué objeto ocupa
/// cada posición, y qué [`Origen`] le corresponde al eslabón del archivo
/// según si ese `DotEnvResolver` vino de un archivo real o no.
///
/// Aislada de [`resolver_por_defecto`] por la misma razón que ya separaba el
/// orden: para poder comprobar, con un `DotEnvResolver` fabricado a mano
/// (`desde_texto`/`desde_archivo`, que no tocan ninguna variable de entorno
/// global ni dependen de que exista un `.env` real) y resolutores de prueba
/// en las otras dos posiciones, que ni el cableado ni esa traducción quedan
/// sin proteger. Antes de que existiera esta función, `resolver_por_defecto`
/// pasaba los cuatro argumentos posicionales de
/// `ensamblar_cadena_por_defecto` directamente: tres de ellos del mismo tipo
/// exacto (`Box<dyn KeyResolver>`), así que intercambiar cuál iba como
/// "archivo" y cuál como "llavero" compilaba sin ningún aviso, y ninguna
/// prueba llamaba nunca a `resolver_por_defecto()` para notarlo.
/// `resolver_por_defecto` no hace nada más que llamar a esto con las tres
/// piezas reales.
fn cadena_con(
    entorno: Box<dyn KeyResolver>,
    dotenv: DotEnvResolver,
    llavero: Box<dyn KeyResolver>,
) -> CadenaResolvers {
    let origen_archivo = dotenv
        .origen()
        .map(|p| Origen::Archivo(p.to_path_buf()))
        .unwrap_or(Origen::Memoria);

    ensamblar_cadena_por_defecto(entorno, origen_archivo, Box::new(dotenv), llavero)
}

/// Cadena por defecto de la aplicación: entorno primero, luego `.env`, y el
/// llavero al final.
///
/// El entorno manda sobre el archivo para que se pueda sobrescribir una clave
/// en una ejecución concreta sin editar nada. El llavero va último porque es
/// donde escribe la aplicación instalada cuando no hay nada más: si una clave
/// también vive en el `.env` —por ejemplo, porque el usuario ya la tenía ahí
/// antes de esta cadena—, esa sigue mandando y el llavero no la tapa.
pub fn resolver_por_defecto() -> CadenaResolvers {
    cadena_con(
        Box::new(EnvResolver),
        DotEnvResolver::buscar(),
        Box::new(LlaveroResolver::nuevo()),
    )
}

// ---------------------------------------------------------------------------
// Escritura
// ---------------------------------------------------------------------------

/// Qué falta por hacer tras el intento de escribir en el llavero: nada si
/// tuvo éxito, o el respaldo si no.
///
/// Aislado en su propia función para poder probar la traducción a [`Origen`]
/// sin depender de si esta máquina tiene un llavero real: con éxito, el
/// origen es el llavero y punto — no hay que fabricar un `Origen::Archivo`
/// con el texto "llavero del sistema" ni nada parecido, y esta función lo dice
/// sin necesidad de tocar ningún almacén de verdad.
fn resultado_de_guardar(
    en_llavero: bool,
    respaldo: impl FnOnce() -> std::io::Result<Origen>,
) -> std::io::Result<Origen> {
    if en_llavero {
        Ok(Origen::Llavero)
    } else {
        respaldo()
    }
}

/// Guarda —o borra, con `None`— una clave de la configuración del usuario.
///
/// Se intenta primero el llavero del sistema operativo: es lo correcto para
/// la aplicación instalada, y evita dejar el secreto en un archivo de texto.
/// Si no hay uno disponible —una plataforma sin soporte, una sesión sin
/// demonio de secretos, un entorno mínimo— se recurre al `.env` de siempre,
/// en `~/.config/dictar_ia/.env` con permisos 0600. El [`Origen`] devuelto
/// dice cuál de los dos se usó.
///
/// Si el llavero tiene éxito y esa misma clave ya vivía en el `.env` —el
/// caso normal en cualquier instalación anterior a esta HU, cuando el `.env`
/// era el único mecanismo—, esa copia vieja se retira del archivo. Es
/// necesario y no cosmético: el `.env` manda sobre el llavero al leer (ver
/// [`resolver_por_defecto`]), así que dejarla ahí haría que la aplicación
/// siguiera usando el valor viejo en cada petición mientras la interfaz dice
/// «guardada en el llavero del sistema». Si esa limpieza falla, la función
/// devuelve el error en vez de responder éxito: lo contrario dejaría la clave
/// guardada en dos sitios sin avisar, con el `.env` todavía mandando.
pub fn guardar_clave(referencia: &str, valor: Option<&str>) -> std::io::Result<Origen> {
    guardar_clave_orquestada(referencia, valor, escribir_en_llavero, dir_configuracion)
}

/// La composición real de [`guardar_clave`], con el intento de llavero y la
/// carpeta de configuración como parámetros.
///
/// Aislada así por dos razones a la vez: para poder comprobar la composición
/// completa —intenta el llavero, y solo si falla cae al `.env`; si tiene
/// éxito, limpia ahí cualquier copia vieja— sin depender de si esta máquina
/// tiene un llavero real, y sin arriesgarse a reescribir la carpeta de
/// configuración *real* del usuario durante un `cargo test`. `guardar_clave`
/// no hace nada más que llamar a esto con las piezas reales, así que una
/// prueba que rompa el orden («caer al `.env` antes de intentar el llavero»,
/// o guardar sin limpiar la copia vieja) sigue ejercitando exactamente lo que
/// usa la aplicación.
fn guardar_clave_orquestada(
    referencia: &str,
    valor: Option<&str>,
    intentar_llavero: impl FnOnce(&str, Option<&str>) -> bool,
    carpeta_config: impl Fn() -> Option<PathBuf>,
) -> std::io::Result<Origen> {
    let en_llavero = intentar_llavero(referencia, valor);

    if en_llavero {
        // Sin carpeta de configuración no puede haber un `.env` real que
        // limpiar: no es un fallo, es que no hay nada que hacer.
        if let Some(dir) = carpeta_config() {
            purgar_del_env(&dir, referencia)?;
        }
    }

    resultado_de_guardar(en_llavero, || {
        let dir = carpeta_config().ok_or_else(|| {
            std::io::Error::other("no se pudo determinar la carpeta de configuración")
        })?;
        guardar_clave_en(&dir, referencia, valor)
    })
}

/// El nombre de variable que declara una línea de `.env`, si declara alguno.
///
/// Comparte con [`parsear`] el reconocimiento del prefijo `export ` y del
/// signo `=`, pero no necesita el resto de ese análisis —comillas,
/// comentarios de fin de línea— solo para decidir a qué clave pertenece una
/// línea.
fn nombre_de_linea(linea: &str) -> Option<&str> {
    linea
        .trim()
        .strip_prefix("export ")
        .unwrap_or(linea.trim())
        .split_once('=')
        .map(|(k, _)| k.trim())
}

/// Si una línea de `.env` declara `referencia`, bajo cualquiera de los
/// nombres que acepta [`nombres_candidatos`] —no solo el canónico.
///
/// Es el mismo contrato que usa [`DotEnvResolver::resolver`] al leer:
/// [`guardar_clave_en`] (al sustituir una línea) y [`purgar_del_env`] (al
/// decidir si hay algo que limpiar) tienen que reconocer exactamente las
/// mismas líneas que esa lectura reconocería, o una de las dos deja una
/// copia fantasma que la otra no ve. Antes, las dos comparaban solo contra
/// `nombre_canonico(referencia)` —una reimplementación reducida del
/// contrato real, cada una por su cuenta—: un `.env` con `GEMINI=sk-vieja`
/// (una de las otras dos formas que `nombres_candidatos` sí acepta al leer)
/// no se reconocía como "la misma clave" al escribir ni al purgar.
fn linea_declara(linea: &str, referencia: &str) -> bool {
    let candidatos = nombres_candidatos(referencia);
    nombre_de_linea(linea).is_some_and(|k| candidatos.iter().any(|c| c.as_str() == k))
}

/// Igual que [`guardar_clave`], pero escribiendo el `.env` directamente, sin
/// intentar el llavero antes.
///
/// Existe para poder probarlo sin depender de si esta máquina tiene un
/// llavero real —en Windows el Credential Manager siempre está disponible,
/// así que una prueba que pasara por [`guardar_clave`] completa escribiría
/// una entrada de verdad ahí— ni tocar variables de entorno globales: los
/// tests corren en paralelo, y mutar `XDG_CONFIG_HOME` desde varios a la vez
/// hace que se pisen entre ellos y fallen de forma intermitente.
///
/// Se conserva el resto del archivo: si tenía comentarios o claves de otros
/// proveedores, siguen ahí. Cualquier línea que declare la misma clave bajo
/// una forma no canónica ([`linea_declara`]) se normaliza a la forma
/// canónica en vez de dejarse intacta junto a la nueva: dejar las dos a la
/// vez volvería a exponer la ambigüedad que [`nombres_candidatos`] documenta
/// —al leer, la forma corta gana por ir primero en esa lista—.
pub fn guardar_clave_en(
    dir: &Path,
    referencia: &str,
    valor: Option<&str>,
) -> std::io::Result<Origen> {
    std::fs::create_dir_all(dir)?;

    let ruta = dir.join(".env");
    let nombre = nombre_canonico(referencia);

    let previo = std::fs::read_to_string(&ruta).unwrap_or_default();
    let mut lineas: Vec<String> = Vec::new();
    let mut sustituida = false;

    for linea in previo.lines() {
        if linea_declara(linea, referencia) {
            // Como mucho una línea sobrevive por clave: si el archivo tenía
            // más de una forma a la vez —algo que esta función nunca
            // produce por sí sola, solo un `.env` tocado a mano llega así—,
            // las coincidencias siguientes se descartan en vez de duplicar
            // la línea canónica.
            if !sustituida {
                if let Some(v) = valor {
                    lineas.push(format!("{nombre}={v}"));
                }
            }
            // Con `None` y sin ninguna coincidencia previa, simplemente no
            // se reescribe: eso la borra.
            sustituida = true;
        } else {
            lineas.push(linea.to_owned());
        }
    }

    if !sustituida {
        if let Some(v) = valor {
            if lineas.is_empty() {
                lineas.push("# Claves de API de dictar_ia. Permisos 0600.".to_owned());
            }
            lineas.push(format!("{nombre}={v}"));
        }
    }

    let contenido = format!("{}\n", lineas.join("\n"));

    // Escritura atómica: antes de esta HU, `std::fs::write` truncando el
    // `.env` en el sitio solo se disparaba cuando el usuario guardaba o
    // borraba una clave a mano. Ahora `purgar_del_env` la dispara también
    // como efecto colateral de cada guardado con éxito en el llavero, sin
    // que el usuario haya pedido tocar el archivo en ese instante — así que
    // un corte de energía o un cierre forzado justo en medio ya no puede
    // dejarlo vacío o a medias, perdiendo claves que ni siquiera eran la que
    // se estaba tocando. Se escribe en un archivo temporal del mismo
    // directorio —mismo sistema de archivos, para que el renombrado no
    // pueda fallar por cruzar de dispositivo— y se reemplaza el definitivo
    // con `rename`, que en Unix y en Windows sustituye el destino de una
    // sola vez: si algo falla antes del `rename`, el `.env` real ni se
    // entera.
    let temporal = dir.join(format!(".env.tmp.{}", std::process::id()));
    std::fs::write(&temporal, contenido)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // 0600 antes de renombrar: son credenciales, y el renombrado
        // conserva los permisos del archivo temporal, no los del destino.
        std::fs::set_permissions(&temporal, std::fs::Permissions::from_mode(0o600))?;
    }

    std::fs::rename(&temporal, &ruta)?;

    Ok(Origen::Archivo(ruta))
}

/// Retira una clave del `.env`, si estuviera ahí bajo cualquiera de los
/// nombres que acepta [`nombres_candidatos`] ([`linea_declara`]). No hace
/// nada —ni crea el archivo ni la carpeta— si el `.env` no existe o si
/// ninguno de esos nombres está en él: no hay nada que limpiar, y crear un
/// `.env` vacío solo porque el llavero tuvo éxito ensuciaría el disco sin
/// necesidad.
///
/// Existe para [`guardar_clave`]: desde esta HU, el llavero puede tener éxito
/// mientras una versión más vieja de la misma clave sigue en el `.env`, y
/// como el `.env` manda sobre el llavero al leer, dejarla ahí sin tocar
/// dejaría a la aplicación usando el valor viejo en cada petición.
///
/// **Decisión, sobre la red de seguridad que esto retira:** una vez purgada,
/// si el llavero deja de estar disponible en una sesión posterior —el caso
/// es Linux/headless: sin sesión D-Bus reenviada (`cron`, `ssh`), un
/// contenedor mínimo, un demonio de secretos que no arrancó tras reanudar—,
/// la clave no está en ningún sitio, y el proveedor se ve "sin configurar"
/// sin ningún diagnóstico que apunte a la causa real. Se acepta el riesgo en
/// vez de no purgar nunca, por dos razones: (1) no purgar reabre el
/// Bloqueante original —la interfaz volvería a decir "guardada en el
/// llavero" mientras una copia vieja del `.env` sigue mandando—, y (2) la
/// mitigación real —avisar al usuario de que la clave "se mudó" y ya no
/// tiene respaldo en disco— es una decisión de interfaz, y ninguno de los
/// archivos que puede tocar esta vuelta pinta nada en pantalla. Lo que sí se
/// hizo, dentro de lo que este archivo puede decidir por su cuenta: purgar
/// solo ocurre después de que [`escribir_en_llavero`] releyó el valor recién
/// escrito con éxito (ver su comentario), así que la copia de respaldo no se
/// pierde por un "éxito" que en realidad nunca llegó a estar disponible en
/// *esta* sesión — el riesgo que queda, y que se documenta en vez de
/// resolverse en silencio, es específicamente el de una sesión *distinta y
/// futura* sin acceso al llavero.
fn purgar_del_env(dir: &Path, referencia: &str) -> std::io::Result<()> {
    purgar_del_env_con(dir, referencia, guardar_clave_en)
}

/// La lógica real de [`purgar_del_env`], con la reescritura como parámetro.
///
/// Aislada así, con el mismo patrón que ya usa el resto del archivo
/// (`guardar_clave_orquestada`, `cadena_con`), para poder comprobar que la
/// reescritura no se dispara cuando no hace falta y que un fallo al
/// reescribir se propaga, sin depender de trucos de permisos de archivo que
/// no se comportan igual en Windows y en Unix —en Unix, por ejemplo, el
/// permiso del propio archivo no impide un `rename` sobre él: lo que manda
/// es el permiso de escritura del directorio que lo contiene—.
fn purgar_del_env_con(
    dir: &Path,
    referencia: &str,
    escribir: impl FnOnce(&Path, &str, Option<&str>) -> std::io::Result<Origen>,
) -> std::io::Result<()> {
    let ruta = dir.join(".env");
    let previo = match std::fs::read_to_string(&ruta) {
        Ok(texto) => texto,
        // Solo "no existe" cuenta como "nada que limpiar". Cualquier otro
        // error —permiso denegado, contenido no UTF-8, un bloqueo
        // transitorio de un antivirus o un sincronizador de archivos sobre
        // la carpeta de configuración— se propaga en vez de tratarse como
        // éxito: si la causa fuera transitoria, la próxima vez que el
        // archivo volviera a ser legible la copia vieja seguiría intacta
        // ahí, con el mismo síntoma que el Bloqueante original, por una
        // tercera vía.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };

    let ya_estaba = previo.lines().any(|linea| linea_declara(linea, referencia));

    if ya_estaba {
        escribir(dir, referencia, None)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// En memoria, para tests
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MapResolver(pub HashMap<String, String>);

impl MapResolver {
    pub fn con(mut self, clave: &str, valor: &str) -> Self {
        self.0.insert(clave.to_owned(), valor.to_owned());
        self
    }
}

impl KeyResolver for MapResolver {
    fn resolver(&self, referencia: &str) -> Option<String> {
        self.0.get(referencia).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn una_referencia_prueba_varios_nombres_de_variable() {
        let n = nombres_candidatos("keyring:gemini");
        assert!(n.contains(&"gemini".to_owned()));
        assert!(n.contains(&"GEMINI".to_owned()));
        assert!(n.contains(&"GEMINI_API_KEY".to_owned()));
    }

    #[test]
    fn una_referencia_que_ya_es_api_key_no_se_duplica() {
        let n = nombres_candidatos("env:OPENAI_API_KEY");
        assert_eq!(
            n.iter().filter(|x| x.contains("API_KEY")).count(),
            1,
            "no debe generar OPENAI_API_KEY_API_KEY"
        );
    }

    #[test]
    fn se_leen_pares_clave_valor() {
        let r = DotEnvResolver::desde_texto("GEMINI_API_KEY=abc123\nDEEPSEEK_API_KEY=xyz");
        assert_eq!(r.resolver("keyring:gemini").as_deref(), Some("abc123"));
        assert_eq!(r.resolver("keyring:deepseek").as_deref(), Some("xyz"));
    }

    #[test]
    fn se_ignoran_comentarios_y_lineas_vacias() {
        let r = DotEnvResolver::desde_texto(
            "# esto es un comentario\n\n  \nGEMINI_API_KEY=abc\n# otro\n",
        );
        assert_eq!(r.resolver("keyring:gemini").as_deref(), Some("abc"));
    }

    #[test]
    fn se_admite_el_prefijo_export() {
        // Es como queda al copiarlo de la terminal; rechazarlo solo desconcierta.
        let r = DotEnvResolver::desde_texto("export GEMINI_API_KEY=abc");
        assert_eq!(r.resolver("keyring:gemini").as_deref(), Some("abc"));
    }

    #[test]
    fn se_quitan_las_comillas() {
        let r = DotEnvResolver::desde_texto("GEMINI_API_KEY=\"abc def\"\nOPENAI_API_KEY='xyz'");
        assert_eq!(r.resolver("keyring:gemini").as_deref(), Some("abc def"));
        assert_eq!(r.resolver("keyring:openai").as_deref(), Some("xyz"));
    }

    #[test]
    fn un_comentario_al_final_no_se_traga_como_parte_de_la_clave() {
        let r = DotEnvResolver::desde_texto("GEMINI_API_KEY=abc123 # la de la cuenta personal");
        assert_eq!(r.resolver("keyring:gemini").as_deref(), Some("abc123"));
    }

    #[test]
    fn una_almohadilla_dentro_de_una_clave_entrecomillada_se_conserva() {
        // Las claves de API pueden contener casi cualquier carácter: cortar por
        // una almohadilla dentro de comillas rompería una clave válida.
        let r = DotEnvResolver::desde_texto("OPENAI_API_KEY=\"sk-a#b#c\"");
        assert_eq!(r.resolver("keyring:openai").as_deref(), Some("sk-a#b#c"));
    }

    #[test]
    fn una_clave_vacia_cuenta_como_ausente() {
        // Dejar `GEMINI_API_KEY=` en el archivo debe omitir el proveedor, no
        // intentar autenticarse con la cadena vacía y devolver un 401 confuso.
        let r = DotEnvResolver::desde_texto("GEMINI_API_KEY=");
        assert!(r.resolver("keyring:gemini").is_none());
    }

    #[test]
    fn la_cadena_prefiere_el_primer_eslabon() {
        let cadena = CadenaResolvers::nueva()
            .con(
                Origen::Entorno,
                Box::new(MapResolver::default().con("keyring:gemini", "del-entorno")),
            )
            .con(
                Origen::Archivo("/tmp/.env".into()),
                Box::new(MapResolver::default().con("keyring:gemini", "del-archivo")),
            );

        let (valor, origen) = cadena.resolver_con_origen("keyring:gemini").unwrap();
        assert_eq!(valor, "del-entorno");
        assert_eq!(origen, Origen::Entorno);
    }

    #[test]
    fn la_cadena_sigue_buscando_si_el_primero_no_la_tiene() {
        let cadena = CadenaResolvers::nueva()
            .con(Origen::Entorno, Box::new(MapResolver::default()))
            .con(
                Origen::Archivo("/tmp/.env".into()),
                Box::new(MapResolver::default().con("keyring:deepseek", "del-archivo")),
            );

        let (valor, origen) = cadena.resolver_con_origen("keyring:deepseek").unwrap();
        assert_eq!(valor, "del-archivo");
        assert!(matches!(origen, Origen::Archivo(_)));
    }

    /// Extrae la ruta de un [`Origen::Archivo`]. Los tests de esta sección
    /// pasan siempre por [`guardar_clave_en`], que nunca toca el llavero, así
    /// que el origen que sale nunca puede ser otra cosa.
    fn ruta_de(origen: Origen) -> PathBuf {
        match origen {
            Origen::Archivo(p) => p,
            otro => panic!("se esperaba Origen::Archivo, salió {otro:?}"),
        }
    }

    #[test]
    fn guardar_una_clave_la_deja_legible_para_el_resolutor() {
        let dir = tempfile::tempdir().unwrap();

        let ruta = ruta_de(
            guardar_clave_en(dir.path(), "keyring:deepseek", Some("sk-prueba-123")).unwrap(),
        );
        assert!(ruta.is_file());

        let r = DotEnvResolver::desde_archivo(&ruta).unwrap();
        assert_eq!(
            r.resolver("keyring:deepseek").as_deref(),
            Some("sk-prueba-123")
        );
    }

    #[test]
    fn guardar_no_pisa_las_claves_de_otros_proveedores() {
        let dir = tempfile::tempdir().unwrap();

        guardar_clave_en(dir.path(), "keyring:deepseek", Some("aaa")).unwrap();
        let ruta = ruta_de(guardar_clave_en(dir.path(), "keyring:gemini", Some("bbb")).unwrap());

        let r = DotEnvResolver::desde_archivo(&ruta).unwrap();
        assert_eq!(r.resolver("keyring:deepseek").as_deref(), Some("aaa"));
        assert_eq!(r.resolver("keyring:gemini").as_deref(), Some("bbb"));
    }

    #[test]
    fn guardar_dos_veces_sustituye_en_vez_de_acumular() {
        let dir = tempfile::tempdir().unwrap();

        guardar_clave_en(dir.path(), "keyring:deepseek", Some("vieja")).unwrap();
        let ruta =
            ruta_de(guardar_clave_en(dir.path(), "keyring:deepseek", Some("nueva")).unwrap());

        let texto = std::fs::read_to_string(&ruta).unwrap();
        assert_eq!(texto.matches("DEEPSEEK_API_KEY").count(), 1);

        let r = DotEnvResolver::desde_archivo(&ruta).unwrap();
        assert_eq!(r.resolver("keyring:deepseek").as_deref(), Some("nueva"));
    }

    #[test]
    fn borrar_una_clave_la_quita_del_archivo() {
        let dir = tempfile::tempdir().unwrap();

        guardar_clave_en(dir.path(), "keyring:deepseek", Some("aaa")).unwrap();
        let ruta = ruta_de(guardar_clave_en(dir.path(), "keyring:deepseek", None).unwrap());

        let r = DotEnvResolver::desde_archivo(&ruta).unwrap();
        assert!(r.resolver("keyring:deepseek").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn el_archivo_de_claves_queda_ilegible_para_los_demas() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();

        let ruta =
            ruta_de(guardar_clave_en(dir.path(), "keyring:openai", Some("sk-secreta")).unwrap());
        let modo = std::fs::metadata(&ruta).unwrap().permissions().mode() & 0o777;
        assert_eq!(modo, 0o600, "permisos {modo:o}: son credenciales");
    }

    #[test]
    fn se_busca_el_env_en_varios_sitios() {
        let rutas = rutas_dotenv();
        assert!(!rutas.is_empty());
        assert!(
            rutas.iter().all(|r| r.file_name().unwrap() == ".env"),
            "todas las rutas candidatas deben apuntar a un archivo .env"
        );
    }

    // -----------------------------------------------------------------------
    // Llavero — HU-05
    // -----------------------------------------------------------------------

    #[test]
    fn el_llavero_va_despues_del_entorno_y_del_env() {
        // Antes, esta prueba armaba su propia `CadenaResolvers` desde cero,
        // sin pasar por ninguna función de producción: si alguien invertía
        // el orden dentro de `resolver_por_defecto`, seguía en verde. Ahora
        // llama a `ensamblar_cadena_por_defecto`, que es la única función que
        // decide ese orden -`resolver_por_defecto` no hace nada más que
        // pasarle las piezas reales-, así que una mutación ahí sí la tira.
        // Se usa `MapResolver` en las tres posiciones porque el
        // `LlaveroResolver`/`EnvResolver`/`DotEnvResolver` reales dependen de
        // esta máquina; lo que se comprueba es el orden de ensamblado, no qué
        // resolutor concreto ocupa cada eslabón.
        let cadena = ensamblar_cadena_por_defecto(
            Box::new(MapResolver::default().con("keyring:gemini", "del-entorno")),
            Origen::Archivo("/tmp/.env".into()),
            Box::new(MapResolver::default().con("keyring:gemini", "del-archivo")),
            Box::new(MapResolver::default().con("keyring:gemini", "del-llavero")),
        );

        let (valor, origen) = cadena.resolver_con_origen("keyring:gemini").unwrap();
        assert_eq!(valor, "del-entorno");
        assert_eq!(origen, Origen::Entorno);

        // Y si ni el entorno ni el archivo la tienen, se llega hasta el
        // llavero: no basta con que gane el primero, tiene que seguir
        // buscando hasta el último eslabón.
        let cadena_sin_los_dos_primeros = ensamblar_cadena_por_defecto(
            Box::new(MapResolver::default()),
            Origen::Archivo("/tmp/.env".into()),
            Box::new(MapResolver::default()),
            Box::new(MapResolver::default().con("keyring:gemini", "del-llavero")),
        );

        let (valor, origen) = cadena_sin_los_dos_primeros
            .resolver_con_origen("keyring:gemini")
            .unwrap();
        assert_eq!(valor, "del-llavero");
        assert_eq!(origen, Origen::Llavero);
    }

    #[test]
    fn cadena_con_no_intercambia_el_archivo_con_el_llavero() {
        // `resolver_por_defecto()` no tiene más lógica propia que llamar a
        // `cadena_con(...)` con las tres piezas reales, pero antes de esta
        // prueba nada llamaba a `cadena_con()` ni a `resolver_por_defecto()`:
        // intercambiar `Box::new(dotenv)` y `Box::new(LlaveroResolver::nuevo())`
        // en esa llamada compila sin ningún aviso -el compilador no distingue
        // dos `Box<dyn KeyResolver>`- e invierte la prioridad real del
        // criterio 2 sin que nada lo note.
        let dotenv_con_valor = DotEnvResolver::desde_texto("GEMINI_API_KEY=del-archivo");
        let cadena = cadena_con(
            Box::new(MapResolver::default()),
            dotenv_con_valor,
            Box::new(MapResolver::default().con("keyring:gemini", "del-llavero")),
        );
        let (valor, _) = cadena.resolver_con_origen("keyring:gemini").unwrap();
        assert_eq!(
            valor, "del-archivo",
            "el .env debe ganarle al llavero; si sale \"del-llavero\" es que \
             quedaron en las posiciones intercambiadas"
        );

        // Y si el archivo no la tiene, se debe seguir buscando hasta el
        // llavero -que quede en su propia posición, no que desaparezca-.
        let dotenv_vacio = DotEnvResolver::desde_texto("");
        let cadena_sin_archivo = cadena_con(
            Box::new(MapResolver::default()),
            dotenv_vacio,
            Box::new(MapResolver::default().con("keyring:gemini", "del-llavero")),
        );
        let (valor, origen) = cadena_sin_archivo
            .resolver_con_origen("keyring:gemini")
            .unwrap();
        assert_eq!(valor, "del-llavero");
        assert_eq!(origen, Origen::Llavero);
    }

    #[test]
    fn un_env_encontrado_se_reporta_como_origen_archivo_no_como_memoria() {
        // La otra decisión de `cadena_con` que ninguna prueba cubría: la
        // traducción de `dotenv.origen()` a `Origen`. Invertirla haría que
        // una clave leída de un archivo real -el caso más común de todos- se
        // reportara como si hubiera salido de la nada, sin que nada lo note.
        let dir = tempfile::tempdir().unwrap();
        let ruta = dir.path().join(".env");
        std::fs::write(&ruta, "GEMINI_API_KEY=del-archivo\n").unwrap();
        let dotenv = DotEnvResolver::desde_archivo(&ruta).unwrap();

        let cadena = cadena_con(
            Box::new(MapResolver::default()),
            dotenv,
            Box::new(MapResolver::default()),
        );

        let (valor, origen) = cadena.resolver_con_origen("keyring:gemini").unwrap();
        assert_eq!(valor, "del-archivo");
        assert_eq!(origen, Origen::Archivo(ruta));
    }

    #[test]
    fn sin_env_encontrado_el_origen_es_memoria_no_una_ruta_inventada() {
        // La otra dirección de la misma traducción: un `DotEnvResolver` que
        // no vino de ningún archivo real (`desde_texto`, como usan casi
        // todas las pruebas de este módulo) debe etiquetarse `Memoria`, no
        // `Archivo` con una ruta que nadie escribió.
        let dotenv = DotEnvResolver::desde_texto("GEMINI_API_KEY=del-archivo");

        let cadena = cadena_con(
            Box::new(MapResolver::default()),
            dotenv,
            Box::new(MapResolver::default()),
        );

        let (valor, origen) = cadena.resolver_con_origen("keyring:gemini").unwrap();
        assert_eq!(valor, "del-archivo");
        assert_eq!(origen, Origen::Memoria);
    }

    #[test]
    fn el_resolutor_real_del_llavero_no_entra_en_panico_sin_sesion() {
        // Antes, un `.unwrap()` en vez de `.ok()` tumbaba el proceso entero
        // en cualquier máquina sin sesión gráfica —el caso más común en la
        // práctica, y el del propio CI—. No se puede asumir tampoco lo
        // contrario: en la máquina de un desarrollador con GNOME hay un
        // llavero real, y ahí la referencia inventada de abajo falla por "no
        // existe" en vez de por "no hay llavero", así que el `.unwrap()`
        // hubiera entrado en pánico en los dos entornos por igual.
        let resolutor = LlaveroResolver::nuevo();
        let _: Option<String> =
            resolutor.resolver("keyring:esta_referencia_no_deberia_existir_en_ningun_llavero");
    }

    #[test]
    fn un_error_del_llavero_se_convierte_en_none() {
        // El nombre es deliberadamente imposible para forzar un error tanto
        // si esta máquina no tiene llavero (falla al no haber almacén por
        // defecto) como si lo tiene de verdad: en Windows, un nombre de
        // usuario de miles de caracteres supera el límite del Credential
        // Manager y falla con "Invalid" antes de tocar el almacén; en
        // cualquier backend que sí lo aceptara, nadie ha guardado nunca nada
        // con ese nombre, así que falla con "no existe". Antes, propagar ese
        // error con `?` en vez de convertirlo en `None` hubiera roto la
        // cadena entera por culpa de su último eslabón, el que menos
        // prioridad tiene.
        let referencia_imposible = format!("keyring:{}", "x".repeat(5000));
        let resultado = LlaveroResolver::nuevo().resolver(&referencia_imposible);
        assert!(resultado.is_none());
    }

    #[test]
    fn guardar_en_el_llavero_informa_de_su_origen_no_de_una_ruta() {
        // Antes de que existiera `Origen`, la tentación fácil hubiera sido
        // fabricar un `PathBuf` con el texto "llavero del sistema" en vez de
        // cambiar el tipo de retorno. Esta prueba no depende de si la
        // máquina tiene un llavero real: aísla la decisión en
        // `resultado_de_guardar` y la comprueba directamente. El pánico en el
        // respaldo demuestra además que, con éxito en el llavero, ni siquiera
        // se consulta el archivo.
        let origen = resultado_de_guardar(true, || {
            panic!("con éxito en el llavero no debe consultar el respaldo del archivo")
        });
        assert!(matches!(origen, Ok(Origen::Llavero)));
    }

    #[test]
    fn guardar_una_clave_en_el_llavero_no_deja_dos_copias_activas() {
        // Antes, guardar_clave (guardar_clave_orquestada) devolvía
        // Origen::Llavero con éxito y nunca tocaba el .env: en cualquier
        // instalación que ya tuviera esa clave ahí -el caso normal antes de
        // esta HU, cuando el .env era el único mecanismo- la aplicación
        // seguía usando el valor viejo en cada petición, porque el .env
        // manda sobre el llavero al leer (resolver_por_defecto), mientras la
        // interfaz decía "guardada en el llavero del sistema". Se ejercita
        // la composición real de guardar_clave, no sus piezas por separado.
        let dir = tempfile::tempdir().unwrap();
        guardar_clave_en(dir.path(), "keyring:deepseek", Some("vieja-del-env")).unwrap();
        // Una clave de otro proveedor, que no debe verse afectada.
        guardar_clave_en(dir.path(), "keyring:gemini", Some("de-gemini")).unwrap();

        let origen = guardar_clave_orquestada(
            "keyring:deepseek",
            Some("nueva-del-llavero"),
            |_referencia, _valor| true, // simula éxito real en el llavero
            || Some(dir.path().to_path_buf()),
        )
        .unwrap();
        assert!(matches!(origen, Origen::Llavero));

        let r = DotEnvResolver::desde_archivo(dir.path().join(".env")).unwrap();
        assert!(
            r.resolver("keyring:deepseek").is_none(),
            "la copia vieja debía retirarse del .env al tener éxito en el llavero"
        );
        assert_eq!(
            r.resolver("keyring:gemini").as_deref(),
            Some("de-gemini"),
            "una clave de otro proveedor no debía tocarse"
        );
    }

    #[test]
    fn guardar_en_el_llavero_purga_una_copia_vieja_escrita_con_nombre_corto() {
        // El Bloqueante original, reproducido letra por letra por una
        // segunda vía: nombres_candidatos("keyring:gemini") acepta "gemini",
        // "GEMINI" y "GEMINI_API_KEY" -las tres, no solo la última-, y
        // DotEnvResolver::resolver prueba las tres al leer. La corrección
        // anterior de este mismo bug solo reconocía la forma canónica al
        // purgar, así que un .env escrito a mano con la forma corta -algo
        // habitual al exportar una variable desde la terminal, y que
        // README.md documenta como aceptada aunque no la enseñe- sobrevivía
        // a la purga: la interfaz decía "guardada en el llavero del
        // sistema" y la siguiente lectura seguía devolviendo el valor
        // viejo, porque el .env antecede al llavero en la cadena.
        let dir = tempfile::tempdir().unwrap();
        // Escrito a mano, no con guardar_clave_en: esa función siempre
        // produce la forma canónica, así que no reproduciría el escenario.
        std::fs::write(dir.path().join(".env"), "GEMINI=sk-vieja\n").unwrap();

        let origen = guardar_clave_orquestada(
            "keyring:gemini",
            Some("sk-nueva-del-llavero"),
            |_referencia, _valor| true, // simula éxito real en el llavero
            || Some(dir.path().to_path_buf()),
        )
        .unwrap();
        assert!(matches!(origen, Origen::Llavero));

        let r = DotEnvResolver::desde_archivo(dir.path().join(".env")).unwrap();
        assert!(
            r.resolver("keyring:gemini").is_none(),
            "la copia vieja escrita con el nombre corto debía purgarse igual \
             que la escrita con el nombre canónico"
        );
    }

    #[test]
    fn guardar_en_el_llavero_sin_copia_vieja_no_crea_el_env() {
        // Si nunca hubo un .env, tener éxito en el llavero no debe crear uno
        // vacío solo para "limpiarlo": no hay nada que limpiar ahí.
        let dir = tempfile::tempdir().unwrap();

        let origen = guardar_clave_orquestada(
            "keyring:openai",
            Some("nueva"),
            |_referencia, _valor| true,
            || Some(dir.path().to_path_buf()),
        )
        .unwrap();
        assert!(matches!(origen, Origen::Llavero));
        assert!(
            !dir.path().join(".env").exists(),
            "no debía crearse un .env solo por haber guardado en el llavero"
        );
    }

    #[test]
    fn si_el_llavero_no_esta_disponible_la_clave_no_se_pierde() {
        // La otra mitad de la composición: sin el llavero, guardar_clave
        // debe seguir cayendo al .env exactamente como guardar_clave_en.
        // Antes de esta HU no había ninguna prueba que ejercitara
        // guardar_clave/guardar_clave_orquestada como un todo, solo sus
        // piezas (resultado_de_guardar por un lado, guardar_clave_en por
        // otro).
        let dir = tempfile::tempdir().unwrap();

        let origen = guardar_clave_orquestada(
            "keyring:deepseek",
            Some("sk-respaldo-9c31"),
            |_referencia, _valor| false, // simula que el llavero no está disponible
            || Some(dir.path().to_path_buf()),
        )
        .unwrap();

        let r = DotEnvResolver::desde_archivo(ruta_de(origen)).unwrap();
        assert_eq!(
            r.resolver("keyring:deepseek").as_deref(),
            Some("sk-respaldo-9c31")
        );
    }

    #[test]
    fn purgar_del_env_no_crea_el_archivo_si_no_existia() {
        // Caso de borde explícito de H1: instalación nueva, sin .env
        // todavía. Purgar una clave que nunca estuvo en ningún lado no debe
        // crear ni el archivo ni la carpeta.
        let dir = tempfile::tempdir().unwrap();
        purgar_del_env(dir.path(), "keyring:deepseek").unwrap();
        assert!(!dir.path().join(".env").exists());
    }

    #[test]
    fn purgar_del_env_no_toca_el_archivo_si_la_clave_no_esta() {
        // Caso de borde explícito de H1: el .env existe, pero con otras
        // claves. El archivo debe quedar intacto, byte a byte -incluidos
        // comentarios y orden- si la clave a purgar no está en él.
        let dir = tempfile::tempdir().unwrap();
        guardar_clave_en(dir.path(), "keyring:gemini", Some("de-gemini")).unwrap();
        let antes = std::fs::read_to_string(dir.path().join(".env")).unwrap();

        purgar_del_env(dir.path(), "keyring:deepseek").unwrap();

        let despues = std::fs::read_to_string(dir.path().join(".env")).unwrap();
        assert_eq!(
            antes, despues,
            "un archivo sin la clave a purgar no debía tocarse"
        );
    }

    #[test]
    fn purgar_del_env_reconoce_una_clave_escrita_con_un_nombre_corto() {
        // Mismo escenario que
        // guardar_en_el_llavero_purga_una_copia_vieja_escrita_con_nombre_corto,
        // pero aislado en purgar_del_env en vez de en la composición
        // completa: antes comparaba solo contra nombre_canonico, así que una
        // línea "GEMINI=..." -una de las otras dos formas que
        // nombres_candidatos sí acepta al leer- no se reconocía como la
        // misma clave y sobrevivía a la purga.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "GEMINI=sk-vieja\n").unwrap();

        purgar_del_env(dir.path(), "keyring:gemini").unwrap();

        let r = DotEnvResolver::desde_archivo(dir.path().join(".env")).unwrap();
        assert!(r.resolver("keyring:gemini").is_none());
    }

    #[test]
    fn purgar_del_env_no_reescribe_si_la_clave_no_esta() {
        // La comparación de contenido de
        // purgar_del_env_no_toca_el_archivo_si_la_clave_no_esta, de arriba,
        // no detectaba que se quitara el `if ya_estaba` de purgar_del_env:
        // reescribir un archivo sin ninguna coincidencia produce el mismo
        // contenido, byte a byte, así que esa mutación pasaba igual. Aquí se
        // inyecta la reescritura -el mismo patrón que ya usa
        // guardar_en_el_llavero_informa_de_su_origen_no_de_una_ruta con
        // resultado_de_guardar- y se comprueba que ni siquiera se llama, no
        // solo que el resultado final coincida.
        let dir = tempfile::tempdir().unwrap();
        guardar_clave_en(dir.path(), "keyring:gemini", Some("de-gemini")).unwrap();

        purgar_del_env_con(dir.path(), "keyring:deepseek", |_, _, _| {
            panic!("no debía reescribirse: \"deepseek\" no estaba en el archivo")
        })
        .unwrap();
    }

    #[test]
    fn purgar_del_env_propaga_el_error_si_falla_al_reescribir() {
        // Cuarto caso de borde de H1, sin prueba hasta ahora: si la
        // reescritura que retira la copia vieja falla, purgar_del_env tiene
        // que devolver ese error, no tragárselo -informar éxito con la copia
        // vieja todavía en el .env sería la misma mentira que motivó el
        // hallazgo original, por un camino distinto-.
        let dir = tempfile::tempdir().unwrap();
        guardar_clave_en(dir.path(), "keyring:gemini", Some("de-gemini")).unwrap();

        let resultado = purgar_del_env_con(dir.path(), "keyring:gemini", |_, _, _| {
            Err(std::io::Error::other("fallo simulado al reescribir"))
        });

        assert!(
            resultado.is_err(),
            "un fallo al reescribir debía propagarse, no convertirse en éxito silencioso"
        );
    }

    #[test]
    fn purgar_del_env_propaga_un_error_de_lectura_que_no_es_archivo_ausente() {
        // `let Ok(previo) = read_to_string(&ruta) else { return Ok(()) }`
        // trataba cualquier error de lectura -permiso denegado, contenido no
        // UTF-8, un bloqueo transitorio de un antivirus o un sincronizador
        // de archivos- igual que "no existe": si la causa era transitoria,
        // la próxima vez que el archivo volviera a ser legible la copia
        // vieja seguiría intacta, con el mismo síntoma que el Bloqueante
        // original, por una tercera vía. Se fuerza un error de lectura que
        // no es "archivo ausente" -el ".env" es un directorio, no un
        // archivo- sin depender de permisos, que se comportan distinto en
        // el CI y en una máquina de desarrollo.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".env")).unwrap();

        let resultado = purgar_del_env(dir.path(), "keyring:gemini");

        assert!(
            resultado.is_err(),
            "un error de lectura que no es \"archivo ausente\" no debía tratarse como éxito"
        );
    }

    #[test]
    fn el_texto_del_error_no_contiene_la_clave() {
        // Un archivo común donde `guardar_clave_en` espera un directorio:
        // crear un subdirectorio ahí falla siempre, en cualquier sistema
        // operativo, sin depender de permisos que varían entre el CI y una
        // máquina de desarrollo — a diferencia de forzar el error por la vía
        // del llavero, aquí no hace falta ninguna suposición sobre si esta
        // máquina tiene uno.
        let secreto = "sk-no-debe-aparecer-en-el-texto-del-error-4b7d";

        let base = tempfile::tempdir().unwrap();
        let obstaculo = base.path().join("esto-es-un-archivo-no-un-directorio");
        std::fs::write(&obstaculo, b"").unwrap();
        let dir_invalido = obstaculo.join("subdirectorio");

        let error = guardar_clave_en(&dir_invalido, "keyring:prueba-error", Some(secreto))
            .expect_err("crear un directorio dentro de un archivo debe fallar");
        assert!(!error.to_string().contains(secreto));
    }

    /// Subscriptor mínimo que junta el texto de cada evento de `tracing`, para
    /// comprobar que ninguno lleva un valor dado.
    ///
    /// Implementado a mano contra el trait `Subscriber` del propio `tracing`,
    /// sin añadir `tracing-subscriber` como dependencia de este crate solo
    /// para una prueba: el trait ya alcanza para esto.
    #[derive(Clone, Default)]
    struct CapturaEventos {
        textos: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl CapturaEventos {
        fn textos(&self) -> Vec<String> {
            self.textos.lock().unwrap().clone()
        }
    }

    impl tracing::Subscriber for CapturaEventos {
        fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            struct Texto(String);
            impl tracing::field::Visit for Texto {
                fn record_debug(
                    &mut self,
                    field: &tracing::field::Field,
                    value: &dyn std::fmt::Debug,
                ) {
                    use std::fmt::Write;
                    let _ = write!(self.0, " {}={value:?}", field.name());
                }
            }

            let mut texto = Texto(String::new());
            event.record(&mut texto);
            self.textos.lock().unwrap().push(texto.0);
        }

        fn enter(&self, _span: &tracing::span::Id) {}
        fn exit(&self, _span: &tracing::span::Id) {}
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    fn ningun_evento_de_tracing_contiene_el_valor_de_la_clave() {
        // El vector real de fuga no es el texto de un `Result`, sino un
        // campo de `tracing`: un `tracing::warn!(valor = %clave, ...)` no se
        // vería en ningún `.to_string()` de error, pero sí en cualquier log
        // configurado para ese nivel.
        //
        // Antes, esta prueba llamaba a `escribir_en_llavero` de verdad, así
        // que escribía en el Credential Manager / Keychain reales al correr
        // `cargo test` -contradiciendo la Decisión #1 de este mismo
        // archivo-, y encima solo podía llegar a capturar el evento de la
        // rama de error: en el CI de Linux la escritura real falla siempre
        // por no haber D-Bus, así que la rama de éxito -la que de verdad
        // escribe- nunca se ejercitaba ahí, y en un Windows con Credential
        // Manager funcional (donde sí tiene éxito) tampoco había ninguna
        // aserción que la comprobara. Ahora se llama directamente a
        // `registrar_resultado_de_llavero`, la función que decide qué avisar
        // por `tracing` -la única línea que sigue tocando el almacén real,
        // dentro de `escribir_en_llavero`, queda deliberadamente fuera de
        // esta prueba, ver su comentario-, con un `Ok`/`Err` sintéticos: las
        // dos ramas quedan cubiertas de verdad, en cualquier plataforma,
        // incluido el CI de Linux, y sin tocar ningún almacén real.
        let captura = CapturaEventos::default();
        let secreto = "sk-no-debe-aparecer-en-ningun-log-7a91";

        tracing::subscriber::with_default(captura.clone(), || {
            // Rama de éxito: no debe avisar nada.
            assert!(registrar_resultado_de_llavero(Ok(())));
            assert!(
                captura.textos().is_empty(),
                "la rama de éxito no debía emitir ningún evento de tracing"
            );

            // Rama de error: avisa, pero solo con el error de la plataforma.
            assert!(!registrar_resultado_de_llavero(Err(keyring::Error::NoEntry)));
            assert!(
                !captura.textos().is_empty(),
                "la rama de error debía emitir al menos un evento"
            );

            // guardar_clave_en nunca llama a tracing, pero si algún día lo
            // hiciera, esto lo detectaría sin arriesgar la carpeta real de
            // configuración del usuario: usa un directorio temporal.
            let dir = tempfile::tempdir().unwrap();
            let _ = guardar_clave_en(dir.path(), "keyring:prueba-tracing", Some(secreto));

            for texto in captura.textos() {
                assert!(
                    !texto.contains(secreto),
                    "un evento de tracing contuvo el valor de la clave: {texto}"
                );
            }
        });
    }
}
