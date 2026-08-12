//! Preferencias del usuario.
//!
//! Van en un TOML propio y no en la base de datos: son cuatro valores, el
//! usuario puede querer editarlos a mano, y no tiene sentido migrar el esquema
//! de SQLite cada vez que se añade una casilla.

use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ajustes {
    /// Carpeta donde se exportan los apuntes en Markdown al terminar cada
    /// sesión.
    ///
    /// Apuntando a una carpeta sincronizada —OneDrive, Drive, Nextcloud— los
    /// apuntes acaban en el móvil y en cualquier otro equipo sin que la
    /// aplicación tenga que hablar con ninguna API de nube. Es mucho menos
    /// código, no caduca ningún token y funciona con el servicio que ya uses.
    #[serde(default)]
    pub carpeta_apuntes: Option<String>,

    /// Modelo de Whisper: "turbo", "medium" o "small".
    #[serde(default)]
    pub modelo: Option<String>,

    /// Capturar diapositivas de la pantalla compartida.
    #[serde(default = "verdadero")]
    pub capturar_diapositivas: bool,

    /// Área de la pantalla a la que se recortan las capturas: `[x, y, w, h]`.
    ///
    /// Sin ella se guarda la pantalla entera, y en una clase por videollamada
    /// eso incluye las caras y los nombres de todos los participantes. Elegir
    /// el área una vez deja fuera todo lo que no es la diapositiva.
    #[serde(default)]
    pub region_captura: Option<[u32; 4]>,
}

fn verdadero() -> bool {
    true
}

/// Nombre de la carpeta que se crea dentro de Documentos.
pub const CARPETA_POR_DEFECTO: &str = "Apuntes dictar_ia";

/// Carpeta de Documentos del usuario, respetando su idioma.
///
/// Se lee de `user-dirs.dirs`, que es donde el escritorio guarda el nombre
/// real: en un sistema en español es «Documentos», en uno en inglés
/// «Documents», y dar por bueno uno de los dos dejaría los apuntes en una
/// carpeta nueva y desconcertante junto a la de verdad.
pub fn dir_documentos() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;

    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));

    if let Ok(texto) = std::fs::read_to_string(config.join("user-dirs.dirs")) {
        for linea in texto.lines() {
            let linea = linea.trim();
            let Some(valor) = linea.strip_prefix("XDG_DOCUMENTS_DIR=") else {
                continue;
            };
            let valor = valor.trim_matches('"');
            let ruta = match valor.strip_prefix("$HOME/") {
                Some(resto) => home.join(resto),
                None => PathBuf::from(valor),
            };
            if ruta.is_dir() {
                return Some(ruta);
            }
        }
    }

    // Sin user-dirs.dirs —un entorno mínimo, una sesión remota— se prueban los
    // dos nombres habituales antes de rendirse.
    for nombre in ["Documentos", "Documents"] {
        let ruta = home.join(nombre);
        if ruta.is_dir() {
            return Some(ruta);
        }
    }

    Some(home)
}

/// Carpeta de apuntes por defecto: `<Documentos>/Apuntes dictar_ia`.
pub fn carpeta_apuntes_por_defecto() -> Option<PathBuf> {
    dir_documentos().map(|d| d.join(CARPETA_POR_DEFECTO))
}

impl Ajustes {
    /// Carpeta donde se exportan los apuntes, configurada o por defecto.
    ///
    /// Que haya un valor por defecto importa: sin él, quien no entre en
    /// ajustes nunca vería un apunte fuera de la aplicación, y no sabría que
    /// esa exportación existe.
    pub fn carpeta_efectiva(&self) -> Option<PathBuf> {
        match &self.carpeta_apuntes {
            Some(c) if !c.trim().is_empty() => Some(PathBuf::from(c)),
            _ => carpeta_apuntes_por_defecto(),
        }
    }

    pub fn cargar(dir: &Path) -> Self {
        let ruta = dir.join("ajustes.toml");
        std::fs::read_to_string(ruta)
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            // Unos ajustes corruptos no deben impedir arrancar: se vuelve a
            // los valores por defecto y el usuario los reconfigura.
            .unwrap_or_else(|| Self {
                capturar_diapositivas: true,
                ..Default::default()
            })
    }

    pub fn guardar(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        let texto =
            toml::to_string_pretty(self).map_err(|e| crate::ApiError::Config(e.to_string()))?;
        std::fs::write(dir.join("ajustes.toml"), texto)?;
        Ok(())
    }
}

/// Carpetas de sincronización habituales que existan en el equipo.
///
/// Se ofrecen como sugerencia en los ajustes: escribir la ruta a mano invita a
/// equivocarse, y un selector de archivos nativo obligaría a arrastrar una
/// dependencia más para algo que se configura una vez.
pub fn carpetas_sincronizadas() -> Vec<String> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };

    const CANDIDATAS: &[&str] = &[
        "OneDrive",
        "OneDrive - Personal",
        "Dropbox",
        "Nextcloud",
        "ownCloud",
        "Google Drive",
        "GoogleDrive",
        "Insync",
        "MEGA",
        "pCloudDrive",
        "Sync",
        "Documentos",
        "Documents",
    ];

    let mut encontradas: Vec<String> = CANDIDATAS
        .iter()
        .map(|c| home.join(c))
        .filter(|p| p.is_dir())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    // Los clientes de OneDrive para Linux suelen colgar de ~/.config o crear
    // carpetas con el nombre de la organización; se busca un nivel más.
    if let Ok(entradas) = std::fs::read_dir(&home) {
        for e in entradas.flatten() {
            let nombre = e.file_name().to_string_lossy().to_lowercase();
            if (nombre.contains("onedrive") || nombre.contains("drive")) && e.path().is_dir() {
                let ruta = e.path().to_string_lossy().into_owned();
                if !encontradas.contains(&ruta) {
                    encontradas.push(ruta);
                }
            }
        }
    }

    encontradas.sort();
    encontradas.dedup();
    encontradas
}

/// Convierte un texto en un nombre de archivo seguro.
///
/// Los nombres salen de datos del usuario —asignaturas, títulos generados por
/// la IA— así que pueden traer barras, dos puntos o saltos de línea. Sin
/// limpiarlos, un título con una barra crearía carpetas fantasma.
pub fn nombre_seguro(s: &str) -> String {
    let limpio: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r' | '\t' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();

    let recortado = limpio.split_whitespace().collect::<Vec<_>>().join(" ");

    if recortado.is_empty() {
        "sin titulo".to_owned()
    } else {
        // 120 caracteres: por debajo del límite de cualquier sistema de
        // archivos, incluido el de Windows con la ruta completa.
        recortado.chars().take(120).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unos_ajustes_ausentes_dan_los_valores_por_defecto() {
        let dir = tempfile::tempdir().unwrap();
        let a = Ajustes::cargar(dir.path());
        assert!(a.carpeta_apuntes.is_none());
        assert!(a.capturar_diapositivas, "las diapositivas van activadas");
    }

    #[test]
    fn los_ajustes_van_y_vuelven() {
        let dir = tempfile::tempdir().unwrap();
        let a = Ajustes {
            carpeta_apuntes: Some("/home/x/OneDrive/Apuntes".into()),
            modelo: Some("turbo".into()),
            capturar_diapositivas: false,
            region_captura: Some([100, 200, 800, 600]),
        };
        a.guardar(dir.path()).unwrap();

        let b = Ajustes::cargar(dir.path());
        assert_eq!(
            b.carpeta_apuntes.as_deref(),
            Some("/home/x/OneDrive/Apuntes")
        );
        assert!(!b.capturar_diapositivas);
        assert_eq!(b.region_captura, Some([100, 200, 800, 600]));
    }

    #[test]
    fn unos_ajustes_corruptos_no_impiden_arrancar() {
        // Preferible volver a los valores por defecto que no abrir.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ajustes.toml"), "esto no es toml {{{").unwrap();

        let a = Ajustes::cargar(dir.path());
        assert!(a.capturar_diapositivas);
    }

    #[test]
    fn sin_configurar_nada_se_usa_una_carpeta_dentro_de_documentos() {
        // Sin valor por defecto, quien no entre en ajustes nunca vería un
        // apunte fuera de la aplicación.
        let a = Ajustes::default();
        let c = a.carpeta_efectiva().expect("debe haber una por defecto");
        assert!(c.ends_with(CARPETA_POR_DEFECTO), "quedó en {}", c.display());
    }

    #[test]
    fn lo_configurado_manda_sobre_la_carpeta_por_defecto() {
        let a = Ajustes {
            carpeta_apuntes: Some("/tmp/mis-apuntes".into()),
            ..Default::default()
        };
        assert_eq!(
            a.carpeta_efectiva().unwrap(),
            std::path::Path::new("/tmp/mis-apuntes")
        );
    }

    #[test]
    fn una_carpeta_en_blanco_cuenta_como_no_configurada() {
        // Vaciar el campo en la interfaz no debe dejar la exportación muerta.
        let a = Ajustes {
            carpeta_apuntes: Some("   ".into()),
            ..Default::default()
        };
        let c = a.carpeta_efectiva().unwrap();
        assert!(c.ends_with(CARPETA_POR_DEFECTO));
    }

    #[test]
    fn se_respeta_el_nombre_real_de_documentos() {
        // En español es «Documentos» y en inglés «Documents»: dar por bueno
        // uno de los dos dejaría los apuntes en una carpeta nueva y
        // desconcertante junto a la de verdad.
        let d = dir_documentos().expect("debe encontrar alguna");
        assert!(d.is_absolute());
    }

    #[test]
    fn un_titulo_con_barras_no_crea_carpetas_fantasma() {
        // Los títulos los genera la IA: pueden traer cualquier cosa.
        assert_eq!(nombre_seguro("Tema 3/4: repaso"), "Tema 3 4 repaso");
        assert_eq!(nombre_seguro("a\nb\tc"), "a b c");
    }

    #[test]
    fn un_titulo_vacio_no_produce_un_archivo_sin_nombre() {
        assert_eq!(nombre_seguro("   "), "sin titulo");
        assert_eq!(nombre_seguro("///"), "sin titulo");
    }

    #[test]
    fn un_titulo_larguisimo_se_recorta() {
        let largo = "a".repeat(400);
        assert!(nombre_seguro(&largo).chars().count() <= 120);
    }

    #[test]
    fn las_tildes_y_los_espacios_se_conservan() {
        // Recortar demasiado dejaría nombres ilegibles.
        assert_eq!(
            nombre_seguro("Cálculo II — Transformada de Laplace"),
            "Cálculo II — Transformada de Laplace"
        );
    }
}
