//! Asignaturas y clientes.

use crate::{columna_bool, columna_enum, Db, Result, StorageError};
use dictar_domain::{Topic, TopicId, TopicKind};
use rusqlite::{params, Row};

fn desde_fila(row: &Row) -> rusqlite::Result<Topic> {
    Ok(Topic {
        id: TopicId::from(row.get::<_, String>(0)?),
        kind: columna_enum(row, 1)?,
        name: row.get(2)?,
        person: row.get(3)?,
        term: row.get(4)?,
        color: row.get(5)?,
        consent_ack: columna_bool(row, 6)?,
    })
}

const CAMPOS: &str = "id, kind, name, person, term, color, consent_ack";

impl Db {
    pub fn crear_topic(&self, t: &Topic) -> Result<()> {
        self.conn.execute(
            "INSERT INTO topic (id, kind, name, person, term, color, consent_ack)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                t.id.as_str(),
                t.kind.as_str(),
                t.name,
                t.person,
                t.term,
                t.color,
                t.consent_ack as i64,
            ],
        )?;
        Ok(())
    }

    pub fn actualizar_topic(&self, t: &Topic) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE topic SET kind=?2, name=?3, person=?4, term=?5, color=?6, consent_ack=?7
             WHERE id=?1",
            params![
                t.id.as_str(),
                t.kind.as_str(),
                t.name,
                t.person,
                t.term,
                t.color,
                t.consent_ack as i64,
            ],
        )?;
        if n == 0 {
            return Err(StorageError::NoEncontrado(format!("topic {}", t.id)));
        }
        Ok(())
    }

    pub fn topic(&self, id: &TopicId) -> Result<Topic> {
        self.conn
            .query_row(
                &format!("SELECT {CAMPOS} FROM topic WHERE id=?1"),
                params![id.as_str()],
                desde_fila,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StorageError::NoEncontrado(format!("topic {id}"))
                }
                otro => otro.into(),
            })
    }

    pub fn listar_topics(&self, kind: Option<TopicKind>) -> Result<Vec<Topic>> {
        let (sql, args): (String, Vec<String>) = match kind {
            Some(k) => (
                format!("SELECT {CAMPOS} FROM topic WHERE kind=?1 ORDER BY name"),
                vec![k.as_str().to_owned()],
            ),
            None => (
                format!("SELECT {CAMPOS} FROM topic ORDER BY kind, name"),
                vec![],
            ),
        };

        let mut stmt = self.conn.prepare(&sql)?;
        let filas = stmt.query_map(rusqlite::params_from_iter(args), desde_fila)?;
        Ok(filas.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Registra el permiso del docente o del cliente. Se pide una sola vez por
    /// `topic`, no en cada sesión: confirmar lo mismo cada clase acaba en que
    /// el usuario pulsa "sí" sin leer.
    pub fn registrar_consentimiento(&self, id: &TopicId) -> Result<()> {
        self.conn.execute(
            "UPDATE topic SET consent_ack=1 WHERE id=?1",
            params![id.as_str()],
        )?;
        Ok(())
    }

    pub fn borrar_topic(&self, id: &TopicId) -> Result<()> {
        self.conn
            .execute("DELETE FROM topic WHERE id=?1", params![id.as_str()])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alta_lectura_y_listado() {
        let db = Db::en_memoria().unwrap();

        let mut calculo = Topic::nuevo(TopicKind::Course, "Cálculo II");
        calculo.person = Some("Dra. Pérez".into());
        calculo.term = Some("2026-1".into());
        db.crear_topic(&calculo).unwrap();

        let acme = Topic::nuevo(TopicKind::Client, "Acme S.A.");
        db.crear_topic(&acme).unwrap();

        let leido = db.topic(&calculo.id).unwrap();
        assert_eq!(leido.name, "Cálculo II");
        assert_eq!(leido.person.as_deref(), Some("Dra. Pérez"));
        assert!(!leido.consent_ack);

        assert_eq!(db.listar_topics(None).unwrap().len(), 2);
        assert_eq!(db.listar_topics(Some(TopicKind::Client)).unwrap().len(), 1);
    }

    #[test]
    fn el_consentimiento_se_guarda_una_sola_vez_por_topic() {
        let db = Db::en_memoria().unwrap();
        let t = Topic::nuevo(TopicKind::Client, "Acme");
        db.crear_topic(&t).unwrap();

        db.registrar_consentimiento(&t.id).unwrap();
        assert!(db.topic(&t.id).unwrap().consent_ack);
    }

    #[test]
    fn pedir_un_topic_inexistente_da_error_claro() {
        let db = Db::en_memoria().unwrap();
        let err = db.topic(&TopicId::from("no-existe")).unwrap_err();
        assert!(matches!(err, StorageError::NoEncontrado(_)));
    }
}
