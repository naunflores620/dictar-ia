//! Transcripción, diapositivas y búsqueda de texto completo.

use crate::{columna_bool, columna_enum, Db, Result};
use dictar_domain::{SessionId, Slide, Track, TsMs, Utterance};
use rusqlite::{params, Row};

/// Frase todavía sin `id`, recién salida del motor de transcripción.
#[derive(Debug, Clone)]
pub struct NewUtterance {
    pub track: Track,
    pub speaker: Option<String>,
    pub start_ms: TsMs,
    pub end_ms: TsMs,
    pub text: String,
    pub confidence: Option<f32>,
    pub is_final: bool,
}

const CAMPOS: &str = "id, session_id, track, speaker, start_ms, end_ms, text, \
                      confidence, is_final, revision";

/// Las mismas columnas, cualificadas. `utterance` y `utterance_fts` comparten
/// el nombre `text`, así que en el JOIN de búsqueda hay que desambiguar.
const CAMPOS_U: &str = "u.id, u.session_id, u.track, u.speaker, u.start_ms, u.end_ms, u.text, \
                        u.confidence, u.is_final, u.revision";

fn desde_fila(row: &Row) -> rusqlite::Result<Utterance> {
    Ok(Utterance {
        id: row.get(0)?,
        session_id: SessionId::from(row.get::<_, String>(1)?),
        track: columna_enum(row, 2)?,
        speaker: row.get(3)?,
        start_ms: row.get(4)?,
        end_ms: row.get(5)?,
        text: row.get(6)?,
        confidence: row.get(7)?,
        is_final: columna_bool(row, 8)?,
        revision: row.get(9)?,
    })
}

impl Db {
    /// Inserta frases en bloque, dentro de una transacción.
    ///
    /// El pipeline produce decenas por minuto; una transacción por frase haría
    /// un `fsync` cada vez y castigaría al disco durante dos horas seguidas.
    pub fn insertar_frases(&mut self, sesion: &SessionId, frases: &[NewUtterance]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO utterance
                 (session_id, track, speaker, start_ms, end_ms, text, confidence, is_final)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            )?;
            for f in frases {
                stmt.execute(params![
                    sesion.as_str(),
                    f.track.as_str(),
                    f.speaker,
                    f.start_ms,
                    f.end_ms,
                    f.text,
                    f.confidence,
                    f.is_final as i64,
                ])?;
                // El índice FTS lo mantienen los triggers de la migración 2:
                // solo se indexa `is_final = 1`.
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Sustituye la transcripción en vivo por la definitiva.
    ///
    /// Es lo que ocurre al terminar la sesión: el texto del modelo pequeño se
    /// descarta y entra el de `large-v3-turbo` con glosario y contexto completo.
    pub fn reemplazar_por_definitiva(
        &mut self,
        sesion: &SessionId,
        frases: &[NewUtterance],
    ) -> Result<()> {
        debug_assert!(
            frases.iter().all(|f| f.is_final),
            "reemplazar_por_definitiva solo admite frases definitivas"
        );

        let tx = self.conn.transaction()?;
        {
            // El trigger `utterance_ad` limpia el índice de búsqueda por cada
            // fila borrada, así que no quedan entradas huérfanas que luego
            // aparecerían como resultados fantasma.
            tx.execute(
                "DELETE FROM utterance WHERE session_id=?1",
                params![sesion.as_str()],
            )?;

            let mut stmt = tx.prepare(
                "INSERT INTO utterance
                 (session_id, track, speaker, start_ms, end_ms, text, confidence, is_final, revision)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,1,1)",
            )?;

            for f in frases {
                stmt.execute(params![
                    sesion.as_str(),
                    f.track.as_str(),
                    f.speaker,
                    f.start_ms,
                    f.end_ms,
                    f.text,
                    f.confidence,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn transcripcion(&self, sesion: &SessionId) -> Result<Vec<Utterance>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {CAMPOS} FROM utterance WHERE session_id=?1 ORDER BY start_ms, id"
        ))?;
        let filas = stmt.query_map(params![sesion.as_str()], desde_fila)?;
        Ok(filas.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Texto plano con hablante, listo para mandar al modelo.
    pub fn transcripcion_plana(&self, sesion: &SessionId) -> Result<String> {
        let frases = self.transcripcion(sesion)?;
        Ok(frases
            .iter()
            .map(|u| {
                let quien = u.speaker.as_deref().unwrap_or(match u.track {
                    Track::Mic => "Yo",
                    Track::System => "Interlocutor",
                });
                // El ts_ms viaja en el texto: es lo que permite al modelo
                // devolver marcas de tiempo verificables en las notas.
                format!("[{}] {}: {}", fmt_ts(u.start_ms), quien, u.text)
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// Búsqueda de texto completo en todas las sesiones, o dentro de una.
    pub fn buscar(&self, consulta: &str, sesion: Option<&SessionId>) -> Result<Vec<Utterance>> {
        // `unicode61 remove_diacritics 2` hace que "laplace" encuentre
        // "Laplace"; el usuario no debería escribir tildes para buscar.
        let (sql, args): (String, Vec<Box<dyn rusqlite::ToSql>>) = match sesion {
            Some(s) => (
                format!(
                    "SELECT {CAMPOS_U} FROM utterance u
                     JOIN utterance_fts f ON f.rowid = u.id
                     WHERE utterance_fts MATCH ?1 AND u.session_id = ?2
                     ORDER BY rank LIMIT 200"
                ),
                vec![Box::new(consulta.to_owned()), Box::new(s.0.clone())],
            ),
            None => (
                format!(
                    "SELECT {CAMPOS_U} FROM utterance u
                     JOIN utterance_fts f ON f.rowid = u.id
                     WHERE utterance_fts MATCH ?1
                     ORDER BY rank LIMIT 200"
                ),
                vec![Box::new(consulta.to_owned())],
            ),
        };

        let mut stmt = self.conn.prepare(&sql)?;
        let filas = stmt.query_map(rusqlite::params_from_iter(args.iter()), desde_fila)?;
        Ok(filas.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // -- Diapositivas -------------------------------------------------------

    pub fn insertar_diapositiva(
        &self,
        sesion: &SessionId,
        path: &str,
        ts_ms: TsMs,
        phash: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO slide (session_id, path, ts_ms, phash) VALUES (?1,?2,?3,?4)",
            params![sesion.as_str(), path, ts_ms, phash],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn guardar_ocr(&self, slide_id: i64, texto: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE slide SET ocr_text=?2 WHERE id=?1",
            params![slide_id, texto],
        )?;
        Ok(())
    }

    pub fn diapositivas(&self, sesion: &SessionId) -> Result<Vec<Slide>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, path, ts_ms, phash, ocr_text, caption
             FROM slide WHERE session_id=?1 ORDER BY ts_ms",
        )?;
        let filas = stmt.query_map(params![sesion.as_str()], |row| {
            Ok(Slide {
                id: row.get(0)?,
                session_id: SessionId::from(row.get::<_, String>(1)?),
                path: row.get(2)?,
                ts_ms: row.get(3)?,
                phash: row.get(4)?,
                ocr_text: row.get(5)?,
                caption: row.get(6)?,
            })
        })?;
        Ok(filas.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Diapositiva visible en un instante dado: la última cuyo `ts_ms` no ha
    /// pasado todavía. Es lo que enlaza una fórmula de los apuntes con la
    /// lámina donde estaba escrita.
    pub fn diapositiva_en(&self, sesion: &SessionId, ts_ms: TsMs) -> Result<Option<i64>> {
        let id = self
            .conn
            .query_row(
                "SELECT id FROM slide WHERE session_id=?1 AND ts_ms <= ?2
                 ORDER BY ts_ms DESC LIMIT 1",
                params![sesion.as_str(), ts_ms],
                |r| r.get::<_, i64>(0),
            )
            .ok();
        Ok(id)
    }
}

fn fmt_ts(ms: TsMs) -> String {
    let total = ms / 1000;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use dictar_domain::{Session, SessionKind, SessionSource};

    fn db_con_sesion() -> (Db, SessionId) {
        let db = Db::en_memoria().unwrap();
        let s = Session::nueva(SessionKind::Class, SessionSource::DesktopLoopback, 0);
        db.crear_sesion(&s).unwrap();
        (db, s.id)
    }

    fn frase(track: Track, ini: i64, texto: &str, definitiva: bool) -> NewUtterance {
        NewUtterance {
            track,
            speaker: None,
            start_ms: ini,
            end_ms: ini + 3_000,
            text: texto.to_owned(),
            confidence: Some(0.9),
            is_final: definitiva,
        }
    }

    #[test]
    fn insertar_y_leer_en_orden_temporal() {
        let (mut db, ses) = db_con_sesion();
        db.insertar_frases(
            &ses,
            &[
                frase(Track::System, 5_000, "segunda", true),
                frase(Track::Mic, 1_000, "primera", true),
            ],
        )
        .unwrap();

        let t = db.transcripcion(&ses).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].text, "primera");
        assert_eq!(t[1].text, "segunda");
    }

    #[test]
    fn la_pasada_definitiva_sustituye_a_la_de_en_vivo() {
        let (mut db, ses) = db_con_sesion();

        db.insertar_frases(
            &ses,
            &[frase(Track::System, 0, "transformada de la place", false)],
        )
        .unwrap();
        assert_eq!(db.transcripcion(&ses).unwrap().len(), 1);

        db.reemplazar_por_definitiva(
            &ses,
            &[frase(Track::System, 0, "transformada de Laplace", true)],
        )
        .unwrap();

        let t = db.transcripcion(&ses).unwrap();
        assert_eq!(t.len(), 1, "la versión en vivo debe desaparecer");
        assert_eq!(t[0].text, "transformada de Laplace");
        assert!(t[0].is_final);
        assert_eq!(t[0].revision, 1);
    }

    #[test]
    fn el_indice_de_busqueda_no_deja_huerfanos_al_reemplazar() {
        let (mut db, ses) = db_con_sesion();

        db.insertar_frases(
            &ses,
            &[frase(Track::System, 0, "texto viejo equivocado", true)],
        )
        .unwrap();
        assert_eq!(db.buscar("equivocado", None).unwrap().len(), 1);

        db.reemplazar_por_definitiva(
            &ses,
            &[frase(Track::System, 0, "texto nuevo correcto", true)],
        )
        .unwrap();

        // Si el FTS no se limpiara, esto devolvería una fila fantasma.
        assert!(db.buscar("equivocado", None).unwrap().is_empty());
        assert_eq!(db.buscar("correcto", None).unwrap().len(), 1);
    }

    #[test]
    fn la_transcripcion_en_vivo_no_se_indexa() {
        let (mut db, ses) = db_con_sesion();
        db.insertar_frases(&ses, &[frase(Track::Mic, 0, "provisional", false)])
            .unwrap();
        assert!(db.buscar("provisional", None).unwrap().is_empty());
    }

    #[test]
    fn buscar_ignora_las_tildes() {
        let (mut db, ses) = db_con_sesion();
        db.insertar_frases(
            &ses,
            &[frase(Track::System, 0, "la función es continua", true)],
        )
        .unwrap();
        // El usuario no debería tener que escribir tildes para encontrar algo.
        assert_eq!(db.buscar("funcion", None).unwrap().len(), 1);
    }

    #[test]
    fn el_texto_plano_lleva_marca_de_tiempo_y_hablante() {
        let (mut db, ses) = db_con_sesion();
        db.insertar_frases(
            &ses,
            &[
                frase(Track::Mic, 0, "¿puede repetir?", true),
                frase(Track::System, 3_671_000, "claro", true),
            ],
        )
        .unwrap();

        let plano = db.transcripcion_plana(&ses).unwrap();
        assert!(plano.starts_with("[00:00:00] Yo: ¿puede repetir?"));
        assert!(plano.contains("[01:01:11] Interlocutor: claro"));
    }

    #[test]
    fn la_diapositiva_visible_es_la_ultima_anterior_al_instante() {
        let (db, ses) = db_con_sesion();
        db.insertar_diapositiva(&ses, "/s/1.webp", 10_000, "aaaa")
            .unwrap();
        let segunda = db
            .insertar_diapositiva(&ses, "/s/2.webp", 60_000, "bbbb")
            .unwrap();
        db.insertar_diapositiva(&ses, "/s/3.webp", 120_000, "cccc")
            .unwrap();

        assert_eq!(db.diapositiva_en(&ses, 90_000).unwrap(), Some(segunda));
        // Antes de la primera lámina no hay ninguna visible.
        assert_eq!(db.diapositiva_en(&ses, 5_000).unwrap(), None);
    }
}
