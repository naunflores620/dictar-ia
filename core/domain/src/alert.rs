//! Avisos con fecha, extraídos de las notas.
//!
//! Viven en su propia tabla y no dentro del JSON de la sesión, por un motivo
//! concreto: hay que poder consultarlos **de forma transversal** —"¿qué tengo
//! pendiente este mes?" cruzando todas las asignaturas y clientes— y disparar
//! recordatorios. Enterrados en el documento de una sesión no sirven para eso.

use crate::ids::{SessionId, TopicId};
use crate::notes::{ClassNotes, ClientMinutes, NotesPayload, TeamNotes};
use crate::TsMs;
use serde::{Deserialize, Serialize};

str_enum! {
    pub enum AlertKind {
        ExamDate          => "exam_date",
        Deadline          => "deadline",
        /// "Esto entra en el examen".
        ExamHint          => "exam_hint",
        RoomChange        => "room_change",
        /// Lo que yo prometí a un cliente.
        CommitmentMine    => "commitment_mine",
        /// Lo que el cliente prometió.
        CommitmentTheirs  => "commitment_theirs",
    }
}

impl AlertKind {
    /// Si aparece en el panel de pendientes con fecha límite o solo como nota.
    pub fn es_accionable(&self) -> bool {
        !matches!(self, AlertKind::ExamHint | AlertKind::RoomChange)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: i64,
    pub session_id: SessionId,
    pub topic_id: Option<TopicId>,
    pub kind: AlertKind,
    pub text: String,
    /// ISO-8601 si se mencionó fecha.
    pub due_date: Option<String>,
    /// Salta al audio donde se dijo, para verificarlo.
    pub ts_ms: TsMs,
    pub dismissed: bool,
}

/// Aviso todavía sin `id`, recién extraído de unas notas.
#[derive(Debug, Clone, PartialEq)]
pub struct NewAlert {
    pub kind: AlertKind,
    pub text: String,
    pub due_date: Option<String>,
    pub ts_ms: TsMs,
}

/// Extrae los avisos de un documento de notas.
///
/// Es código normal, no una llamada extra al modelo: los datos ya vienen
/// estructurados de la fase de map-reduce, así que volver a preguntarle a la IA
/// sería pagar dos veces por lo mismo y añadir una oportunidad de alucinar.
pub fn extraer(payload: &NotesPayload) -> Vec<NewAlert> {
    match payload {
        NotesPayload::Class(n) => de_clase(n),
        NotesPayload::Client(n) => de_cliente(n),
        NotesPayload::Team(n) => de_equipo(n),
    }
}

fn de_clase(n: &ClassNotes) -> Vec<NewAlert> {
    use crate::notes::AvisoKind;

    let mut out = Vec::new();

    for a in &n.avisos {
        out.push(NewAlert {
            kind: match a.tipo {
                AvisoKind::Examen => AlertKind::ExamDate,
                AvisoKind::Entrega => AlertKind::Deadline,
                AvisoKind::CambioAula | AvisoKind::CambioHorario => AlertKind::RoomChange,
                AvisoKind::Otro => AlertKind::Deadline,
            },
            text: a.texto.clone(),
            due_date: a.fecha.clone(),
            ts_ms: a.ts_ms,
        });
    }

    for s in &n.senales_examen {
        out.push(NewAlert {
            kind: AlertKind::ExamHint,
            // Se conserva la cita literal del profesor: es lo que hace creíble
            // el aviso cuando lo consultas semanas después.
            text: format!("{} — «{}»", s.tema, s.cita),
            due_date: None,
            ts_ms: s.ts_ms,
        });
    }

    out
}

fn de_cliente(n: &ClientMinutes) -> Vec<NewAlert> {
    let mut out = Vec::new();

    for c in &n.compromisos.mios {
        out.push(NewAlert {
            kind: AlertKind::CommitmentMine,
            text: c.texto.clone(),
            due_date: c.fecha.clone(),
            ts_ms: c.ts_ms,
        });
    }

    for c in &n.compromisos.del_cliente {
        out.push(NewAlert {
            kind: AlertKind::CommitmentTheirs,
            text: c.texto.clone(),
            due_date: c.fecha.clone(),
            ts_ms: c.ts_ms,
        });
    }

    if let Some(p) = &n.proximo_paso {
        out.push(NewAlert {
            kind: AlertKind::CommitmentMine,
            text: format!("Próximo paso: {}", p.texto),
            due_date: p.fecha.clone(),
            ts_ms: p.ts_ms,
        });
    }

    out
}

fn de_equipo(n: &TeamNotes) -> Vec<NewAlert> {
    n.tareas
        .iter()
        .map(|t| NewAlert {
            kind: AlertKind::Deadline,
            text: match &t.responsable {
                Some(r) => format!("{}: {}", r, t.texto),
                None => t.texto.clone(),
            },
            due_date: t.fecha.clone(),
            ts_ms: t.ts_ms,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::*;

    #[test]
    fn de_unos_apuntes_salen_avisos_y_senales_de_examen() {
        let notas = ClassNotes {
            tldr: "x".into(),
            temas: vec![],
            conceptos: vec![],
            formulas: vec![],
            avisos: vec![Aviso {
                tipo: AvisoKind::Examen,
                texto: "Parcial del tema 3".into(),
                fecha: Some("2026-09-15".into()),
                ts_ms: 120_000,
            }],
            senales_examen: vec![SenalExamen {
                tema: "Transformadas".into(),
                cita: "esto cae seguro".into(),
                ts_ms: 300_000,
            }],
            dudas: vec![],
            bibliografia: vec![],
            glosario_nuevo: vec![],
        };

        let avisos = extraer(&NotesPayload::Class(notas));
        assert_eq!(avisos.len(), 2);

        let examen = &avisos[0];
        assert_eq!(examen.kind, AlertKind::ExamDate);
        assert_eq!(examen.due_date.as_deref(), Some("2026-09-15"));
        assert!(examen.kind.es_accionable());

        let pista = &avisos[1];
        assert_eq!(pista.kind, AlertKind::ExamHint);
        // La cita literal se conserva: es lo que da credibilidad al aviso.
        assert!(pista.text.contains("esto cae seguro"));
        assert!(!pista.kind.es_accionable());
    }

    #[test]
    fn los_compromisos_no_se_mezclan_entre_partes() {
        let notas = ClientMinutes {
            tldr: "x".into(),
            contexto_cliente: ContextoCliente::default(),
            necesidades: vec![],
            objeciones: vec![],
            presupuesto: None,
            acuerdos: vec![],
            compromisos: Compromisos {
                mios: vec![Compromiso {
                    texto: "Enviar propuesta".into(),
                    fecha: Some("2026-08-20".into()),
                    ts_ms: 60_000,
                }],
                del_cliente: vec![Compromiso {
                    texto: "Revisar con su equipo".into(),
                    fecha: None,
                    ts_ms: 90_000,
                }],
            },
            proximo_paso: None,
            senales: vec![],
        };

        let avisos = extraer(&NotesPayload::Client(notas));
        assert_eq!(avisos[0].kind, AlertKind::CommitmentMine);
        assert_eq!(avisos[1].kind, AlertKind::CommitmentTheirs);
        // Sin fecha inventada cuando el cliente no la dio.
        assert!(avisos[1].due_date.is_none());
    }
}
