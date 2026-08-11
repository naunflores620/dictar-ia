//! Notas generadas, avisos y glosario acumulado.

use crate::{columna_bool, columna_enum, Db, Result};
use dictar_domain::alert::NewAlert;
use dictar_domain::notes::NotesPayload;
use dictar_domain::transcript::GlossarySource;
use dictar_domain::{Alert, AlertKind, EpochMs, GlossaryTerm, NoteTemplate, SessionId, TopicId};
use rusqlite::{params, Row};

/// Notas recién generadas, todavía sin `id`.
#[derive(Debug, Clone)]
pub struct NewNotes {
    pub template: NoteTemplate,
    /// `rolling` durante la sesión, `final` al terminar.
    pub kind: String,
    pub seq: Option<i64>,
    pub provider: String,
    pub model: String,
    pub prompt_ver: String,
    pub payload: String,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub cost_usd: Option<f64>,
    pub created_at: EpochMs,
}

impl Db {
    // -- Notas --------------------------------------------------------------

    pub fn guardar_notas(&self, sesion: &SessionId, n: &NewNotes) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO notes
             (session_id, template, kind, seq, provider, model, prompt_ver, payload,
              tokens_in, tokens_out, cost_usd, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                sesion.as_str(),
                n.template.as_str(),
                n.kind,
                n.seq,
                n.provider,
                n.model,
                n.prompt_ver,
                n.payload,
                n.tokens_in,
                n.tokens_out,
                n.cost_usd,
                n.created_at,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Documento definitivo de la sesión. Si se ha regenerado varias veces
    /// —por cambio de proveedor o de prompt— devuelve el más reciente.
    pub fn notas_finales(&self, sesion: &SessionId) -> Result<Option<NotesPayload>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT payload FROM notes WHERE session_id=?1 AND kind='final'
                 ORDER BY created_at DESC LIMIT 1",
                params![sesion.as_str()],
                |r| r.get(0),
            )
            .ok();

        match json {
            Some(j) => Ok(Some(serde_json::from_str(&j)?)),
            None => Ok(None),
        }
    }

    /// Último resumen rodante, para retomar la acumulación donde se quedó.
    pub fn ultimo_rodante(&self, sesion: &SessionId) -> Result<Option<(i64, String)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT seq, payload FROM notes WHERE session_id=?1 AND kind='rolling'
                 ORDER BY seq DESC LIMIT 1",
                params![sesion.as_str()],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .ok())
    }

    /// Coste acumulado de una sesión. Que el usuario pueda ver "esta clase
    /// costó $0.02" genera confianza y detecta configuraciones caras por
    /// accidente.
    pub fn coste_sesion(&self, sesion: &SessionId) -> Result<f64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(cost_usd), 0.0) FROM notes WHERE session_id=?1",
            params![sesion.as_str()],
            |r| r.get(0),
        )?)
    }

    // -- Avisos -------------------------------------------------------------

    pub fn guardar_avisos(
        &mut self,
        sesion: &SessionId,
        topic: Option<&TopicId>,
        avisos: &[NewAlert],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            // Regenerar las notas no debe duplicar los avisos.
            tx.execute(
                "DELETE FROM alert WHERE session_id=?1",
                params![sesion.as_str()],
            )?;

            let mut stmt = tx.prepare(
                "INSERT INTO alert (session_id, topic_id, kind, text, due_date, ts_ms)
                 VALUES (?1,?2,?3,?4,?5,?6)",
            )?;
            for a in avisos {
                stmt.execute(params![
                    sesion.as_str(),
                    topic.map(|t| t.0.clone()),
                    a.kind.as_str(),
                    a.text,
                    a.due_date,
                    a.ts_ms,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Panel de pendientes: cruza todas las asignaturas y clientes.
    ///
    /// Los que tienen fecha van primero y en orden; los que no —una pista de
    /// examen, por ejemplo— van después. Es la consulta que responde a "¿qué
    /// tengo pendiente?" sin abrir una sola grabación.
    pub fn pendientes(&self) -> Result<Vec<Alert>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, topic_id, kind, text, due_date, ts_ms, dismissed
             FROM alert WHERE dismissed = 0
             ORDER BY (due_date IS NULL), due_date ASC, ts_ms ASC",
        )?;
        let filas = stmt.query_map([], alerta_desde_fila)?;
        Ok(filas.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn avisos_de_topic(&self, topic: &TopicId, kind: Option<AlertKind>) -> Result<Vec<Alert>> {
        let (sql, args): (String, Vec<Box<dyn rusqlite::ToSql>>) = match kind {
            Some(k) => (
                "SELECT id, session_id, topic_id, kind, text, due_date, ts_ms, dismissed
                 FROM alert WHERE topic_id=?1 AND kind=?2 ORDER BY ts_ms"
                    .to_owned(),
                vec![Box::new(topic.0.clone()), Box::new(k.as_str().to_owned())],
            ),
            None => (
                "SELECT id, session_id, topic_id, kind, text, due_date, ts_ms, dismissed
                 FROM alert WHERE topic_id=?1 ORDER BY ts_ms"
                    .to_owned(),
                vec![Box::new(topic.0.clone())],
            ),
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let filas = stmt.query_map(rusqlite::params_from_iter(args.iter()), alerta_desde_fila)?;
        Ok(filas.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn descartar_aviso(&self, id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE alert SET dismissed=1 WHERE id=?1", params![id])?;
        Ok(())
    }

    // -- Glosario -----------------------------------------------------------

    /// Añade un término, o incrementa su contador si ya existía.
    ///
    /// Si el término llega de una fuente más fiable que la registrada —el OCR
    /// de una diapositiva frente a una repetición detectada por el propio
    /// STT—, se actualiza la fuente: el texto escrito manda sobre el oído.
    pub fn upsert_termino(
        &self,
        topic: &TopicId,
        termino: &str,
        definicion: Option<&str>,
        fuente: GlossarySource,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO glossary_term (topic_id, term, definition, source, hits)
             VALUES (?1, ?2, ?3, ?4, 1)
             ON CONFLICT(topic_id, term) DO UPDATE SET
               hits = hits + 1,
               definition = COALESCE(excluded.definition, definition),
               source = CASE
                 WHEN ?5 > (CASE source
                              WHEN 'user' THEN 4 WHEN 'slide_ocr' THEN 3
                              WHEN 'syllabus' THEN 2 ELSE 1 END)
                 THEN excluded.source ELSE source END",
            params![
                topic.as_str(),
                termino,
                definicion,
                fuente.as_str(),
                fuente.fiabilidad() as i64,
            ],
        )?;
        Ok(())
    }

    pub fn glosario(&self, topic: &TopicId) -> Result<Vec<GlossaryTerm>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, topic_id, term, definition, source, hits
             FROM glossary_term WHERE topic_id=?1
             ORDER BY hits DESC, term",
        )?;
        let filas = stmt.query_map(params![topic.as_str()], |row| {
            Ok(GlossaryTerm {
                id: row.get(0)?,
                topic_id: TopicId::from(row.get::<_, String>(1)?),
                term: row.get(2)?,
                definition: row.get(3)?,
                source: columna_enum(row, 4)?,
                hits: row.get(5)?,
            })
        })?;
        Ok(filas.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Construye el `initial_prompt` de Whisper con el vocabulario del topic.
    ///
    /// Es el mecanismo por el que cada clase mejora la transcripción de la
    /// siguiente. El presupuesto de caracteres existe porque Whisper solo
    /// atiende a los últimos ~224 tokens de contexto: meterle el glosario
    /// entero desplazaría los términos más importantes fuera de la ventana.
    /// Por eso se ordena por fiabilidad de la fuente y frecuencia, y se corta.
    pub fn prompt_glosario(&self, topic: &TopicId, max_chars: usize) -> Result<String> {
        let mut terminos = self.glosario(topic)?;
        terminos.sort_by(|a, b| {
            b.source
                .fiabilidad()
                .cmp(&a.source.fiabilidad())
                .then(b.hits.cmp(&a.hits))
        });

        let mut out = String::new();
        for t in terminos {
            let siguiente = if out.is_empty() {
                t.term.clone()
            } else {
                format!(", {}", t.term)
            };
            if out.len() + siguiente.len() > max_chars {
                break;
            }
            out.push_str(&siguiente);
        }
        Ok(out)
    }
}

fn alerta_desde_fila(row: &Row) -> rusqlite::Result<Alert> {
    Ok(Alert {
        id: row.get(0)?,
        session_id: SessionId::from(row.get::<_, String>(1)?),
        topic_id: row.get::<_, Option<String>>(2)?.map(TopicId::from),
        kind: columna_enum(row, 3)?,
        text: row.get(4)?,
        due_date: row.get(5)?,
        ts_ms: row.get(6)?,
        dismissed: columna_bool(row, 7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dictar_domain::notes::TeamNotes;
    use dictar_domain::{Session, SessionKind, SessionSource, Topic, TopicKind};

    fn base() -> (Db, SessionId, TopicId) {
        let db = Db::en_memoria().unwrap();
        let t = Topic::nuevo(TopicKind::Course, "Cálculo II");
        db.crear_topic(&t).unwrap();
        let mut s = Session::nueva(SessionKind::Class, SessionSource::DesktopLoopback, 0);
        s.topic_id = Some(t.id.clone());
        db.crear_sesion(&s).unwrap();
        (db, s.id, t.id)
    }

    fn notas_de_prueba() -> NewNotes {
        let payload = NotesPayload::Team(TeamNotes {
            tldr: "Se acordó el alcance".into(),
            temas: vec![],
            decisiones: vec![],
            tareas: vec![],
            abierto: vec![],
        });
        NewNotes {
            template: NoteTemplate::Team,
            kind: "final".into(),
            seq: None,
            provider: "gemini-flash".into(),
            model: "gemini-2.5-flash".into(),
            prompt_ver: "v1".into(),
            payload: serde_json::to_string(&payload).unwrap(),
            tokens_in: Some(20_000),
            tokens_out: Some(5_000),
            cost_usd: Some(0.019),
            created_at: 1_754_000_000_000,
        }
    }

    #[test]
    fn guardar_y_recuperar_las_notas_finales() {
        let (db, ses, _) = base();
        db.guardar_notas(&ses, &notas_de_prueba()).unwrap();

        let leidas = db.notas_finales(&ses).unwrap().unwrap();
        assert_eq!(leidas.plantilla(), NoteTemplate::Team);
        assert_eq!(leidas.tldr(), "Se acordó el alcance");
    }

    #[test]
    fn el_coste_se_acumula_y_es_visible() {
        let (db, ses, _) = base();
        db.guardar_notas(&ses, &notas_de_prueba()).unwrap();
        let mut segunda = notas_de_prueba();
        segunda.cost_usd = Some(0.007);
        db.guardar_notas(&ses, &segunda).unwrap();

        assert!((db.coste_sesion(&ses).unwrap() - 0.026).abs() < 1e-9);
    }

    #[test]
    fn regenerar_las_notas_no_duplica_los_avisos() {
        let (mut db, ses, topic) = base();
        let avisos = vec![NewAlert {
            kind: AlertKind::ExamDate,
            text: "Parcial tema 3".into(),
            due_date: Some("2026-09-15".into()),
            ts_ms: 1000,
        }];

        db.guardar_avisos(&ses, Some(&topic), &avisos).unwrap();
        db.guardar_avisos(&ses, Some(&topic), &avisos).unwrap();

        assert_eq!(db.pendientes().unwrap().len(), 1);
    }

    #[test]
    fn los_pendientes_con_fecha_van_primero_y_ordenados() {
        let (mut db, ses, topic) = base();
        db.guardar_avisos(
            &ses,
            Some(&topic),
            &[
                NewAlert {
                    kind: AlertKind::ExamHint,
                    text: "esto cae".into(),
                    due_date: None,
                    ts_ms: 10,
                },
                NewAlert {
                    kind: AlertKind::Deadline,
                    text: "entrega práctica".into(),
                    due_date: Some("2026-10-01".into()),
                    ts_ms: 20,
                },
                NewAlert {
                    kind: AlertKind::ExamDate,
                    text: "parcial".into(),
                    due_date: Some("2026-09-15".into()),
                    ts_ms: 30,
                },
            ],
        )
        .unwrap();

        let p = db.pendientes().unwrap();
        assert_eq!(p[0].due_date.as_deref(), Some("2026-09-15"));
        assert_eq!(p[1].due_date.as_deref(), Some("2026-10-01"));
        assert!(
            p[2].due_date.is_none(),
            "los que no tienen fecha van al final"
        );
    }

    #[test]
    fn el_glosario_cuenta_repeticiones_y_prefiere_la_fuente_fiable() {
        let (db, _, topic) = base();

        db.upsert_termino(&topic, "Laplace", None, GlossarySource::AutoRecurrent)
            .unwrap();
        db.upsert_termino(&topic, "Laplace", None, GlossarySource::AutoRecurrent)
            .unwrap();
        // El OCR de una diapositiva es ortografía cierta, no oída.
        db.upsert_termino(
            &topic,
            "Laplace",
            Some("Transformada integral"),
            GlossarySource::SlideOcr,
        )
        .unwrap();

        let g = db.glosario(&topic).unwrap();
        assert_eq!(g.len(), 1, "debe ser un único término, no tres filas");
        assert_eq!(g[0].hits, 3);
        assert_eq!(g[0].source, GlossarySource::SlideOcr);
        assert_eq!(g[0].definition.as_deref(), Some("Transformada integral"));
    }

    #[test]
    fn una_fuente_menos_fiable_no_degrada_la_ya_registrada() {
        let (db, _, topic) = base();
        db.upsert_termino(&topic, "Fourier", None, GlossarySource::User)
            .unwrap();
        db.upsert_termino(&topic, "Fourier", None, GlossarySource::AutoRecurrent)
            .unwrap();

        assert_eq!(db.glosario(&topic).unwrap()[0].source, GlossarySource::User);
    }

    #[test]
    fn el_prompt_del_glosario_respeta_el_presupuesto_de_caracteres() {
        let (db, _, topic) = base();
        for i in 0..200 {
            db.upsert_termino(
                &topic,
                &format!("termino{i:03}"),
                None,
                GlossarySource::User,
            )
            .unwrap();
        }
        // Whisper solo atiende a los últimos ~224 tokens: pasarse desplazaría
        // los términos importantes fuera de la ventana de contexto.
        let prompt = db.prompt_glosario(&topic, 200).unwrap();
        assert!(prompt.len() <= 200, "longitud real {}", prompt.len());
        assert!(!prompt.is_empty());
    }
}
