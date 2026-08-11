//! Migraciones versionadas.
//!
//! Se aplican en orden usando `PRAGMA user_version` como marcador. Nunca se
//! edita una migración ya publicada: se añade otra nueva. Un usuario con datos
//! de la semana pasada debe poder actualizar sin perderlos.

use crate::{Result, StorageError};
use rusqlite::Connection;

/// Cada entrada es (versión, SQL). El índice manda: la versión N está en la
/// posición N-1.
const MIGRACIONES: &[(i32, &str)] = &[(1, M001_INICIAL), (2, M002_BUSQUEDA)];

pub fn aplicar(conn: &Connection) -> Result<i32> {
    // Integridad referencial y concurrencia. WAL permite que la UI lea la
    // transcripción mientras el pipeline sigue escribiendo: sin él, la
    // aplicación se bloquea a sí misma durante una grabación.
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;",
    )?;

    let actual: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;

    for (version, sql) in MIGRACIONES {
        if *version > actual {
            tracing::info!(version, "aplicando migración");
            conn.execute_batch(sql)?;
            // `user_version` no admite parámetros ligados.
            conn.execute_batch(&format!("PRAGMA user_version = {version}"))?;
        }
    }

    let final_ = MIGRACIONES.last().map(|(v, _)| *v).unwrap_or(0);
    if actual > final_ {
        return Err(StorageError::VersionFutura {
            encontrada: actual,
            soportada: final_,
        });
    }

    Ok(final_)
}

const M001_INICIAL: &str = r#"
-- Un `topic` es una asignatura o un cliente. Comparten estructura, glosario y
-- búsqueda transversal; solo cambia la plantilla de notas que se les aplica.
CREATE TABLE topic (
  id            TEXT PRIMARY KEY,
  kind          TEXT NOT NULL,
  name          TEXT NOT NULL,
  person        TEXT,
  term          TEXT,
  color         TEXT,
  consent_ack   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_topic_kind ON topic(kind);

CREATE TABLE session (
  id            TEXT PRIMARY KEY,
  topic_id      TEXT REFERENCES topic(id) ON DELETE SET NULL,
  kind          TEXT NOT NULL,
  title         TEXT,
  started_at    INTEGER NOT NULL,
  ended_at      INTEGER,
  source        TEXT NOT NULL,
  app_captured  TEXT,
  language      TEXT,
  audio_path    TEXT,
  denoise_level TEXT NOT NULL DEFAULT 'off',
  status        TEXT NOT NULL,
  solo_local    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_session_topic   ON session(topic_id, started_at DESC);
CREATE INDEX idx_session_status  ON session(status);
CREATE INDEX idx_session_started ON session(started_at DESC);

CREATE TABLE utterance (
  id           INTEGER PRIMARY KEY,
  session_id   TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  track        TEXT NOT NULL,
  speaker      TEXT,
  start_ms     INTEGER NOT NULL,
  end_ms       INTEGER NOT NULL,
  text         TEXT NOT NULL,
  confidence   REAL,
  is_final     INTEGER NOT NULL DEFAULT 0,
  revision     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_utt_session_time ON utterance(session_id, start_ms);
-- La UI pide constantemente "la transcripción definitiva de esta sesión".
CREATE INDEX idx_utt_final ON utterance(session_id, is_final, start_ms);

-- Diapositivas capturadas de la pantalla compartida, ancladas al segundo.
CREATE TABLE slide (
  id           INTEGER PRIMARY KEY,
  session_id   TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  path         TEXT NOT NULL,
  ts_ms        INTEGER NOT NULL,
  phash        TEXT NOT NULL,
  ocr_text     TEXT,
  caption      TEXT
);
CREATE INDEX idx_slide_session_time ON slide(session_id, ts_ms);

-- Vocabulario acumulado: alimenta el initial_prompt de Whisper.
CREATE TABLE glossary_term (
  id           INTEGER PRIMARY KEY,
  topic_id     TEXT NOT NULL REFERENCES topic(id) ON DELETE CASCADE,
  term         TEXT NOT NULL,
  definition   TEXT,
  source       TEXT NOT NULL,
  hits         INTEGER NOT NULL DEFAULT 1,
  UNIQUE(topic_id, term)
);
CREATE INDEX idx_glossary_topic ON glossary_term(topic_id, hits DESC);

CREATE TABLE notes (
  id           INTEGER PRIMARY KEY,
  session_id   TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  template     TEXT NOT NULL,
  kind         TEXT NOT NULL,          -- rolling | final
  seq          INTEGER,
  provider     TEXT NOT NULL,
  model        TEXT NOT NULL,
  prompt_ver   TEXT NOT NULL,
  payload      TEXT NOT NULL,
  tokens_in    INTEGER,
  tokens_out   INTEGER,
  cost_usd     REAL,
  created_at   INTEGER NOT NULL
);
CREATE INDEX idx_notes_session ON notes(session_id, kind, seq);

-- Fechas de examen, entregas y compromisos. En tabla propia porque hay que
-- consultarlos cruzando todos los topics ("¿qué tengo pendiente este mes?"),
-- y dentro del JSON de una sesión eso no se puede hacer.
CREATE TABLE alert (
  id           INTEGER PRIMARY KEY,
  session_id   TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  topic_id     TEXT REFERENCES topic(id) ON DELETE CASCADE,
  kind         TEXT NOT NULL,
  text         TEXT NOT NULL,
  due_date     TEXT,
  ts_ms        INTEGER NOT NULL,
  dismissed    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_alert_due     ON alert(dismissed, due_date);
CREATE INDEX idx_alert_topic   ON alert(topic_id, dismissed);
CREATE INDEX idx_alert_session ON alert(session_id);
"#;

const M002_BUSQUEDA: &str = r#"
-- Búsqueda de texto completo sobre la transcripción definitiva.
--
-- `content='utterance'` evita duplicar el texto: el índice apunta a la tabla
-- real. La sincronización va por triggers y no por código, de modo que el
-- índice no puede quedar desfasado porque alguien olvidara actualizarlo.
--
-- `remove_diacritics 2` hace que buscar "funcion" encuentre "función": nadie
-- debería tener que escribir tildes para buscar en sus propios apuntes.
CREATE VIRTUAL TABLE utterance_fts USING fts5(
  text,
  content='utterance',
  content_rowid='id',
  tokenize="unicode61 remove_diacritics 2"
);

-- Solo se indexa la pasada definitiva. La transcripción en vivo es desechable
-- y se sustituye entera al terminar la sesión; indexarla llenaría el índice de
-- texto condenado a desaparecer.
CREATE TRIGGER utterance_ai AFTER INSERT ON utterance WHEN new.is_final = 1 BEGIN
  INSERT INTO utterance_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TRIGGER utterance_ad AFTER DELETE ON utterance WHEN old.is_final = 1 BEGIN
  INSERT INTO utterance_fts(utterance_fts, rowid, text)
    VALUES('delete', old.id, old.text);
END;

-- Cubre el paso de en vivo a definitiva sin borrar la fila.
CREATE TRIGGER utterance_au AFTER UPDATE ON utterance BEGIN
  INSERT INTO utterance_fts(utterance_fts, rowid, text)
    SELECT 'delete', old.id, old.text WHERE old.is_final = 1;
  INSERT INTO utterance_fts(rowid, text)
    SELECT new.id, new.text WHERE new.is_final = 1;
END;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migra_desde_cero_y_es_idempotente() {
        let conn = Connection::open_in_memory().unwrap();
        let v1 = aplicar(&conn).unwrap();
        assert_eq!(v1, 2);

        // Volver a aplicar sobre una base ya migrada no debe fallar ni
        // reejecutar nada: es lo que pasa en cada arranque de la aplicación.
        let v2 = aplicar(&conn).unwrap();
        assert_eq!(v2, 2);
    }

    #[test]
    fn las_claves_foraneas_quedan_activas() {
        let conn = Connection::open_in_memory().unwrap();
        aplicar(&conn).unwrap();
        let on: i32 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            on, 1,
            "sin claves foráneas, el borrado en cascada no funciona"
        );
    }

    #[test]
    fn una_base_de_una_version_futura_se_rechaza() {
        // Abrir con una versión antigua de la app una base escrita por una
        // nueva debe dar un error claro, no corromper datos en silencio.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA user_version = 999").unwrap();
        assert!(matches!(
            aplicar(&conn),
            Err(StorageError::VersionFutura { .. })
        ));
    }
}
