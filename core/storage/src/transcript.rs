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

    /// Confirma las frases cerradas que se guardaron durante la transcripción en vivo.
    ///
    /// El reconocedor ya cerró esos tramos; solo quedaban marcados como borrador
    /// porque antes siempre se sustituían por la pasada completa de Whisper.
    /// El trigger de búsqueda indexa las filas al cambiar `is_final` a 1.
    pub fn confirmar_vivas(&mut self, sesion: &SessionId) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let actualizadas = tx.execute(
            "UPDATE utterance
             SET is_final=1, revision=revision+1
             WHERE session_id=?1 AND is_final=0",
            params![sesion.as_str()],
        )?;
        tx.commit()?;
        Ok(actualizadas)
    }

    /// Sustituye la transcripción en vivo cuando una pasada de respaldo produce
    /// una versión definitiva más completa.
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

    /// Sustituye solo las frases de una pista y confirma el resto de borradores.
    ///
    /// Es el cierre de una clase en vivo con cobertura de sistema completa a la
    /// que solo le faltaba el micrófono: la pasada por lotes se hace únicamente
    /// sobre esa pista, y borrar toda la transcripción (como hace
    /// `reemplazar_por_definitiva`) tiraría el texto de sistema ya reconocido.
    pub fn reemplazar_pista_por_definitiva(
        &mut self,
        sesion: &SessionId,
        pista: Track,
        frases: &[NewUtterance],
    ) -> Result<()> {
        debug_assert!(
            frases.iter().all(|f| f.is_final && f.track == pista),
            "solo admite frases definitivas de la pista indicada"
        );

        let tx = self.conn.transaction()?;
        {
            tx.execute(
                "DELETE FROM utterance WHERE session_id=?1 AND track=?2",
                params![sesion.as_str(), pista.as_str()],
            )?;
            tx.execute(
                "UPDATE utterance
                 SET is_final=1, revision=revision+1
                 WHERE session_id=?1 AND is_final=0",
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

    /// Cambia el hablante de varias frases a la vez, en una sola transacción.
    ///
    /// Es lo que escribe la diarización al terminar una reunión. Solo toca
    /// `speaker`: el texto, los tiempos y el índice de búsqueda quedan como están.
    pub fn asignar_hablantes(&mut self, asignaciones: &[(i64, String)]) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let mut cambiadas = 0;
        {
            let mut stmt = tx.prepare("UPDATE utterance SET speaker=?2 WHERE id=?1")?;
            for (id, hablante) in asignaciones {
                cambiadas += stmt.execute(params![id, hablante])?;
            }
        }
        tx.commit()?;
        Ok(cambiadas)
    }

    /// Renombra un hablante en una pista de una sesión: todas las frases cuyo
    /// `speaker` es `de` pasan a `a`. Devuelve cuántas cambió.
    ///
    /// No hay tabla de alias: el nombre vive en la propia frase. Así es estable
    /// aunque cambie la numeración de los clústeres, y renombrar dos etiquetas
    /// con el mismo nombre las fusiona, que es como se corrige una voz partida
    /// en dos.
    pub fn renombrar_hablante(
        &mut self,
        sesion: &SessionId,
        pista: Track,
        de: &str,
        a: &str,
    ) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let cambiadas = tx.execute(
            "UPDATE utterance SET speaker=?4
             WHERE session_id=?1 AND track=?2 AND speaker=?3",
            params![sesion.as_str(), pista.as_str(), de, a],
        )?;
        tx.commit()?;
        Ok(cambiadas)
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
    fn reemplazar_una_pista_conserva_y_confirma_las_frases_de_la_otra() {
        let (mut db, ses) = db_con_sesion();
        // Antes se usaba el reemplazo total y el cierre rápido de una clase en
        // vivo perdía el texto de sistema al añadir el del micrófono.
        db.insertar_frases(
            &ses,
            &[
                frase(Track::System, 0, "explicacion del profesor", false),
                frase(Track::Mic, 10_000, "borrador viejo", false),
            ],
        )
        .unwrap();

        db.reemplazar_pista_por_definitiva(
            &ses,
            Track::Mic,
            &[frase(Track::Mic, 10_000, "pregunta mía", true)],
        )
        .unwrap();

        let t = db.transcripcion(&ses).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].track, Track::System);
        assert_eq!(t[0].text, "explicacion del profesor");
        assert!(t.iter().all(|u| u.is_final));
        assert_eq!(t[1].text, "pregunta mía");
        assert_eq!(db.buscar("explicacion", None).unwrap().len(), 1);
        assert!(db.buscar("viejo", None).unwrap().is_empty());
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

    fn con_hablante(track: Track, ini: i64, texto: &str, hablante: &str) -> NewUtterance {
        NewUtterance {
            speaker: Some(hablante.to_owned()),
            ..frase(track, ini, texto, true)
        }
    }

    #[test]
    fn renombrar_cambia_solo_la_pista_y_el_nombre_indicados() {
        let (mut db, ses) = db_con_sesion();
        db.insertar_frases(
            &ses,
            &[
                con_hablante(Track::System, 0, "uno", "Participante 2"),
                con_hablante(Track::System, 4_000, "dos", "Participante 1"),
                con_hablante(Track::System, 8_000, "tres", "Participante 2"),
                // Mismo nombre en la otra pista: no debe tocarse.
                con_hablante(Track::Mic, 12_000, "cuatro", "Participante 2"),
            ],
        )
        .unwrap();

        let n = db
            .renombrar_hablante(&ses, Track::System, "Participante 2", "Carlos (socio)")
            .unwrap();

        assert_eq!(n, 2);
        let t = db.transcripcion(&ses).unwrap();
        let quien: Vec<_> = t.iter().map(|u| u.speaker.as_deref().unwrap()).collect();
        assert_eq!(
            quien,
            [
                "Carlos (socio)",
                "Participante 1",
                "Carlos (socio)",
                "Participante 2"
            ]
        );
    }

    #[test]
    fn renombrar_dos_etiquetas_con_el_mismo_nombre_las_fusiona() {
        // Una voz partida en dos por la diarización se arregla poniéndole el
        // mismo nombre a las dos mitades; no hay que añadir ninguna función de
        // «fusionar».
        let (mut db, ses) = db_con_sesion();
        db.insertar_frases(
            &ses,
            &[
                con_hablante(Track::System, 0, "uno", "Participante 1"),
                con_hablante(Track::System, 4_000, "dos", "Participante 3"),
            ],
        )
        .unwrap();

        db.renombrar_hablante(&ses, Track::System, "Participante 1", "Ana")
            .unwrap();
        db.renombrar_hablante(&ses, Track::System, "Participante 3", "Ana")
            .unwrap();

        let t = db.transcripcion(&ses).unwrap();
        assert!(t.iter().all(|u| u.speaker.as_deref() == Some("Ana")));
    }

    #[test]
    fn renombrar_no_toca_el_texto_ni_la_busqueda() {
        let (mut db, ses) = db_con_sesion();
        db.insertar_frases(
            &ses,
            &[con_hablante(Track::System, 0, "la transformada", "Cliente")],
        )
        .unwrap();

        db.renombrar_hablante(&ses, Track::System, "Cliente", "Marta")
            .unwrap();

        // El trigger de actualización reindexa la fila; tiene que seguir
        // encontrándose exactamente una vez.
        assert_eq!(db.buscar("transformada", None).unwrap().len(), 1);
        assert_eq!(db.transcripcion(&ses).unwrap()[0].text, "la transformada");
    }

    #[test]
    fn asignar_hablantes_actualiza_por_id_y_deja_el_resto() {
        let (mut db, ses) = db_con_sesion();
        db.insertar_frases(
            &ses,
            &[
                con_hablante(Track::System, 0, "uno", "Cliente"),
                con_hablante(Track::System, 4_000, "dos", "Cliente"),
                con_hablante(Track::System, 8_000, "tres", "Cliente"),
            ],
        )
        .unwrap();
        let ids: Vec<i64> = db
            .transcripcion(&ses)
            .unwrap()
            .iter()
            .map(|u| u.id)
            .collect();

        let n = db
            .asignar_hablantes(&[
                (ids[0], "Participante 1".to_owned()),
                (ids[2], "Participante 2".to_owned()),
            ])
            .unwrap();

        assert_eq!(n, 2);
        let t = db.transcripcion(&ses).unwrap();
        assert_eq!(t[0].speaker.as_deref(), Some("Participante 1"));
        assert_eq!(t[1].speaker.as_deref(), Some("Cliente"));
        assert_eq!(t[2].speaker.as_deref(), Some("Participante 2"));
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
