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
//!    instalada. Pendiente de `libsecret`; el hueco ya está previsto.
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

/// Cadena por defecto de la aplicación: entorno primero, luego `.env`.
///
/// El entorno manda sobre el archivo para que se pueda sobrescribir una clave
/// en una ejecución concreta sin editar nada.
pub fn resolver_por_defecto() -> CadenaResolvers {
    let dotenv = DotEnvResolver::buscar();
    let origen = dotenv
        .origen()
        .map(|p| Origen::Archivo(p.to_path_buf()))
        .unwrap_or(Origen::Memoria);

    CadenaResolvers::nueva()
        .con(Origen::Entorno, Box::new(EnvResolver))
        .con(origen, Box::new(dotenv))
}

// ---------------------------------------------------------------------------
// Escritura
// ---------------------------------------------------------------------------

/// Guarda —o borra, con `None`— una clave en la configuración del usuario.
///
/// Escribe en `~/.config/dictar_ia/.env` con permisos 0600. No se usa el
/// llavero del sistema, y conviene decir por qué: exige un demonio de
/// secretos corriendo (gnome-keyring o similar), y cuando no está —una sesión
/// remota, un entorno mínimo— falla de formas difíciles de diagnosticar. Un
/// archivo del propio usuario, ilegible para los demás, es honesto y funciona
/// siempre. El usuario además puede editarlo a mano si le apetece.
///
/// Se conserva el resto del archivo: si tenía comentarios o claves de otros
/// proveedores, siguen ahí.
pub fn guardar_clave(referencia: &str, valor: Option<&str>) -> std::io::Result<PathBuf> {
    let dir = dir_configuracion().ok_or_else(|| {
        std::io::Error::other("no se pudo determinar la carpeta de configuración")
    })?;
    guardar_clave_en(&dir, referencia, valor)
}

/// Igual que [`guardar_clave`], pero con el directorio explícito.
///
/// Existe para poder probarlo sin tocar variables de entorno globales: los
/// tests corren en paralelo, y mutar `XDG_CONFIG_HOME` desde varios a la vez
/// hace que se pisen entre ellos y fallen de forma intermitente.
pub fn guardar_clave_en(
    dir: &Path,
    referencia: &str,
    valor: Option<&str>,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;

    let ruta = dir.join(".env");
    let nombre = nombres_candidatos(referencia)
        .into_iter()
        .last()
        .unwrap_or_else(|| referencia.to_uppercase());

    let previo = std::fs::read_to_string(&ruta).unwrap_or_default();
    let mut lineas: Vec<String> = Vec::new();
    let mut sustituida = false;

    for linea in previo.lines() {
        let clave_linea = linea
            .trim()
            .strip_prefix("export ")
            .unwrap_or(linea.trim())
            .split_once('=')
            .map(|(k, _)| k.trim());

        if clave_linea == Some(nombre.as_str()) {
            if let Some(v) = valor {
                lineas.push(format!("{nombre}={v}"));
            }
            // Con `None` simplemente no se reescribe: eso la borra.
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
    std::fs::write(&ruta, contenido)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // 0600 antes de que nadie pueda leerlo: son credenciales.
        std::fs::set_permissions(&ruta, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(ruta)
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

    #[test]
    fn guardar_una_clave_la_deja_legible_para_el_resolutor() {
        let dir = tempfile::tempdir().unwrap();

        let ruta = guardar_clave_en(dir.path(), "keyring:deepseek", Some("sk-prueba-123")).unwrap();
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
        let ruta = guardar_clave_en(dir.path(), "keyring:gemini", Some("bbb")).unwrap();

        let r = DotEnvResolver::desde_archivo(&ruta).unwrap();
        assert_eq!(r.resolver("keyring:deepseek").as_deref(), Some("aaa"));
        assert_eq!(r.resolver("keyring:gemini").as_deref(), Some("bbb"));
    }

    #[test]
    fn guardar_dos_veces_sustituye_en_vez_de_acumular() {
        let dir = tempfile::tempdir().unwrap();

        guardar_clave_en(dir.path(), "keyring:deepseek", Some("vieja")).unwrap();
        let ruta = guardar_clave_en(dir.path(), "keyring:deepseek", Some("nueva")).unwrap();

        let texto = std::fs::read_to_string(&ruta).unwrap();
        assert_eq!(texto.matches("DEEPSEEK_API_KEY").count(), 1);

        let r = DotEnvResolver::desde_archivo(&ruta).unwrap();
        assert_eq!(r.resolver("keyring:deepseek").as_deref(), Some("nueva"));
    }

    #[test]
    fn borrar_una_clave_la_quita_del_archivo() {
        let dir = tempfile::tempdir().unwrap();

        guardar_clave_en(dir.path(), "keyring:deepseek", Some("aaa")).unwrap();
        let ruta = guardar_clave_en(dir.path(), "keyring:deepseek", None).unwrap();

        let r = DotEnvResolver::desde_archivo(&ruta).unwrap();
        assert!(r.resolver("keyring:deepseek").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn el_archivo_de_claves_queda_ilegible_para_los_demas() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();

        let ruta = guardar_clave_en(dir.path(), "keyring:openai", Some("sk-secreta")).unwrap();
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
}
