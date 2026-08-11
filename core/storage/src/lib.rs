//! Almacenamiento local: SQLite, un solo archivo, sin servidor.
//!
//! Toda la aplicación es local-first. No hay backend, ni cuentas, ni coste
//! mensual: los datos del usuario son un archivo que puede copiar, respaldar o
//! borrar sin intermediarios.

pub mod migrations;
pub mod notes_store;
pub mod sessions;
pub mod topics;
pub mod transcript;

use dictar_domain::DomainError;
use rusqlite::{Connection, Row};
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("error de SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("error de serialización: {0}")]
    Json(#[from] serde_json::Error),

    #[error("dato inválido en la base: {0}")]
    Domain(#[from] DomainError),

    #[error("no encontrado: {0}")]
    NoEncontrado(String),

    #[error(
        "la base de datos es de la versión {encontrada}, pero esta versión de la \
         aplicación solo entiende hasta la {soportada}. Actualiza la aplicación."
    )]
    VersionFutura { encontrada: i32, soportada: i32 },
}

pub type Result<T> = std::result::Result<T, StorageError>;

/// Conexión a la base de datos local.
///
/// No es `Clone` a propósito: una `Connection` de SQLite no debe compartirse
/// entre hilos. Quien necesite acceso concurrente abre la suya con [`Db::abrir`];
/// el modo WAL permite lectores simultáneos mientras el pipeline escribe.
pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn abrir(ruta: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(ruta)?;
        migrations::aplicar(&conn)?;
        Ok(Self { conn })
    }

    /// Base en memoria, para tests.
    pub fn en_memoria() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        migrations::aplicar(&conn)?;
        Ok(Self { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

// ---------------------------------------------------------------------------
// Ayudas de mapeo
// ---------------------------------------------------------------------------

/// Lee una columna de texto y la convierte a una enumeración del dominio.
///
/// Un valor que no se reconoce es un error explícito, no un valor por defecto
/// silencioso: si la base tiene basura, hay que enterarse.
pub(crate) fn columna_enum<T>(row: &Row, idx: usize) -> rusqlite::Result<T>
where
    T: FromStr<Err = DomainError>,
{
    let s: String = row.get(idx)?;
    T::from_str(&s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

pub(crate) fn columna_bool(row: &Row, idx: usize) -> rusqlite::Result<bool> {
    Ok(row.get::<_, i64>(idx)? != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abrir_crea_el_esquema() {
        let db = Db::en_memoria().unwrap();
        let n: i64 = db
            .conn()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(n >= 7, "faltan tablas: solo hay {n}");
    }

    #[test]
    fn el_modo_wal_queda_activo() {
        // Sin WAL, leer la transcripción desde la UI bloquearía al pipeline
        // que la está escribiendo.
        let dir = tempfile::tempdir().unwrap();
        let db = Db::abrir(dir.path().join("t.db")).unwrap();
        let modo: String = db
            .conn()
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(modo.to_lowercase(), "wal");
    }
}
