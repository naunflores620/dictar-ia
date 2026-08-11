//! Sesiones: alta, cambios de estado y recuperación tras un fallo.

use crate::{columna_bool, columna_enum, Db, Result, StorageError};
use dictar_domain::{EpochMs, Session, SessionId, SessionStatus, TopicId};
use rusqlite::{params, Row};

const CAMPOS: &str = "id, topic_id, kind, title, started_at, ended_at, source, \
                      app_captured, language, audio_path, denoise_level, status, solo_local";

fn desde_fila(row: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: SessionId::from(row.get::<_, String>(0)?),
        topic_id: row.get::<_, Option<String>>(1)?.map(TopicId::from),
        kind: columna_enum(row, 2)?,
        title: row.get(3)?,
        started_at: row.get(4)?,
        ended_at: row.get(5)?,
        source: columna_enum(row, 6)?,
        app_captured: row.get(7)?,
        language: row.get(8)?,
        audio_path: row.get(9)?,
        denoise_level: columna_enum(row, 10)?,
        status: columna_enum(row, 11)?,
        solo_local: columna_bool(row, 12)?,
    })
}

impl Db {
    pub fn crear_sesion(&self, s: &Session) -> Result<()> {
        self.conn.execute(
            "INSERT INTO session
             (id, topic_id, kind, title, started_at, ended_at, source, app_captured,
              language, audio_path, denoise_level, status, solo_local)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                s.id.as_str(),
                s.topic_id.as_ref().map(|t| t.0.clone()),
                s.kind.as_str(),
                s.title,
                s.started_at,
                s.ended_at,
                s.source.as_str(),
                s.app_captured,
                s.language,
                s.audio_path,
                s.denoise_level.as_str(),
                s.status.as_str(),
                s.solo_local as i64,
            ],
        )?;
        Ok(())
    }

    pub fn actualizar_sesion(&self, s: &Session) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE session SET topic_id=?2, kind=?3, title=?4, started_at=?5, ended_at=?6,
             source=?7, app_captured=?8, language=?9, audio_path=?10, denoise_level=?11,
             status=?12, solo_local=?13 WHERE id=?1",
            params![
                s.id.as_str(),
                s.topic_id.as_ref().map(|t| t.0.clone()),
                s.kind.as_str(),
                s.title,
                s.started_at,
                s.ended_at,
                s.source.as_str(),
                s.app_captured,
                s.language,
                s.audio_path,
                s.denoise_level.as_str(),
                s.status.as_str(),
                s.solo_local as i64,
            ],
        )?;
        if n == 0 {
            return Err(StorageError::NoEncontrado(format!("sesión {}", s.id)));
        }
        Ok(())
    }

    pub fn sesion(&self, id: &SessionId) -> Result<Session> {
        self.conn
            .query_row(
                &format!("SELECT {CAMPOS} FROM session WHERE id=?1"),
                params![id.as_str()],
                desde_fila,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StorageError::NoEncontrado(format!("sesión {id}"))
                }
                otro => otro.into(),
            })
    }

    pub fn cambiar_estado(&self, id: &SessionId, estado: SessionStatus) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE session SET status=?2 WHERE id=?1",
            params![id.as_str(), estado.as_str()],
        )?;
        if n == 0 {
            return Err(StorageError::NoEncontrado(format!("sesión {id}")));
        }
        Ok(())
    }

    pub fn cerrar_sesion(&self, id: &SessionId, fin: EpochMs) -> Result<()> {
        self.conn.execute(
            "UPDATE session SET ended_at=?2, status=?3 WHERE id=?1",
            params![id.as_str(), fin, SessionStatus::Transcribing.as_str()],
        )?;
        Ok(())
    }

    pub fn sesiones_recientes(&self, limite: usize) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {CAMPOS} FROM session ORDER BY started_at DESC LIMIT ?1"
        ))?;
        let filas = stmt.query_map(params![limite as i64], desde_fila)?;
        Ok(filas.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn sesiones_de_topic(&self, topic: &TopicId) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {CAMPOS} FROM session WHERE topic_id=?1 ORDER BY started_at DESC"
        ))?;
        let filas = stmt.query_map(params![topic.as_str()], desde_fila)?;
        Ok(filas.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Sesiones que quedaron en `recording`: la aplicación murió mientras
    /// grababa. El audio está en disco, así que **hay que recuperarlas**.
    /// Perder una clase de dos horas por un crash es inaceptable, y esta es la
    /// consulta que lo evita: se ejecuta en cada arranque.
    pub fn sesiones_interrumpidas(&self) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {CAMPOS} FROM session WHERE status=?1 ORDER BY started_at DESC"
        ))?;
        let filas = stmt.query_map(params![SessionStatus::Recording.as_str()], desde_fila)?;
        Ok(filas.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Sesiones pendientes de procesar, en orden de llegada. El procesado es
    /// por lotes y en segundo plano: se lanza al dejar el equipo por la noche.
    pub fn cola_de_proceso(&self) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {CAMPOS} FROM session
             WHERE status IN (?1, ?2) AND ended_at IS NOT NULL
             ORDER BY started_at ASC"
        ))?;
        let filas = stmt.query_map(
            params![
                SessionStatus::Transcribing.as_str(),
                SessionStatus::Summarizing.as_str()
            ],
            desde_fila,
        )?;
        Ok(filas.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn borrar_sesion(&self, id: &SessionId) -> Result<()> {
        self.conn
            .execute("DELETE FROM session WHERE id=?1", params![id.as_str()])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dictar_domain::{DenoiseLevel, SessionKind, SessionSource, Topic, TopicKind};

    fn sesion_de_clase() -> Session {
        Session::nueva(
            SessionKind::Class,
            SessionSource::DesktopLoopback,
            1_754_000_000_000,
        )
    }

    #[test]
    fn ciclo_completo_de_una_sesion() {
        let db = Db::en_memoria().unwrap();
        let mut s = sesion_de_clase();
        db.crear_sesion(&s).unwrap();

        assert_eq!(db.sesion(&s.id).unwrap().status, SessionStatus::Recording);

        db.cerrar_sesion(&s.id, 1_754_007_200_000).unwrap();
        let leida = db.sesion(&s.id).unwrap();
        assert_eq!(leida.status, SessionStatus::Transcribing);
        assert_eq!(leida.duracion_ms(), Some(7_200_000)); // dos horas

        s = leida;
        s.status = SessionStatus::Ready;
        s.title = Some("Transformadas de Laplace".into());
        db.actualizar_sesion(&s).unwrap();
        assert_eq!(
            db.sesion(&s.id).unwrap().title.as_deref(),
            Some("Transformadas de Laplace")
        );
    }

    #[test]
    fn el_loopback_se_guarda_sin_denoise() {
        let db = Db::en_memoria().unwrap();
        let s = sesion_de_clase();
        db.crear_sesion(&s).unwrap();
        assert_eq!(db.sesion(&s.id).unwrap().denoise_level, DenoiseLevel::Off);
    }

    #[test]
    fn una_sesion_importada_se_guarda_con_denoise_fuerte() {
        let db = Db::en_memoria().unwrap();
        let s = Session::nueva(SessionKind::ClientMeeting, SessionSource::Import, 1);
        db.crear_sesion(&s).unwrap();
        assert_eq!(
            db.sesion(&s.id).unwrap().denoise_level,
            DenoiseLevel::Strong
        );
    }

    #[test]
    fn tras_un_crash_la_sesion_aparece_como_interrumpida() {
        let db = Db::en_memoria().unwrap();
        let viva = sesion_de_clase();
        db.crear_sesion(&viva).unwrap();

        let terminada = sesion_de_clase();
        db.crear_sesion(&terminada).unwrap();
        db.cerrar_sesion(&terminada.id, 2).unwrap();

        let interrumpidas = db.sesiones_interrumpidas().unwrap();
        assert_eq!(interrumpidas.len(), 1);
        assert_eq!(interrumpidas[0].id, viva.id);
        assert!(interrumpidas[0].status.necesita_recuperacion());
    }

    #[test]
    fn la_cola_de_proceso_respeta_el_orden_de_llegada() {
        let db = Db::en_memoria().unwrap();
        for t in [3_000i64, 1_000, 2_000] {
            let s = Session::nueva(SessionKind::Class, SessionSource::DesktopLoopback, t);
            db.crear_sesion(&s).unwrap();
            db.cerrar_sesion(&s.id, t + 100).unwrap();
        }
        let cola = db.cola_de_proceso().unwrap();
        let tiempos: Vec<_> = cola.iter().map(|s| s.started_at).collect();
        assert_eq!(tiempos, vec![1_000, 2_000, 3_000]);
    }

    #[test]
    fn borrar_un_topic_no_arrastra_sus_sesiones() {
        // ON DELETE SET NULL: si borras una asignatura, las grabaciones siguen
        // ahí. Perder clases por reorganizar carpetas sería inaceptable.
        let db = Db::en_memoria().unwrap();
        let t = Topic::nuevo(TopicKind::Course, "Cálculo II");
        db.crear_topic(&t).unwrap();

        let mut s = sesion_de_clase();
        s.topic_id = Some(t.id.clone());
        db.crear_sesion(&s).unwrap();

        db.borrar_topic(&t.id).unwrap();

        let leida = db.sesion(&s.id).unwrap();
        assert!(leida.topic_id.is_none());
    }
}
