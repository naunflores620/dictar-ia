//! Plantillas de notas.
//!
//! Cada `SessionKind` produce un documento distinto: unos apuntes de clase y un
//! acta comercial no se parecen en nada. Estos tipos son a la vez el destino de
//! la deserialización **y** la fuente del JSON Schema que se le pide al modelo
//! (vía `schemars`), de modo que no pueden desincronizarse.
//!
//! Invariante de todo el módulo: cada elemento generado por la IA lleva su
//! `ts_ms`. Es lo que permite pinchar una afirmación y saltar al segundo exacto
//! del audio para verificarla. Sin eso no se puede confiar en unas notas
//! automáticas.

use crate::TsMs;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

str_enum! {
    pub enum NoteTemplate {
        Class  => "class",
        Client => "client",
        Team   => "team",
    }
}

impl NoteTemplate {
    pub fn nombre_humano(&self) -> &'static str {
        match self {
            NoteTemplate::Class => "Apuntes de clase",
            NoteTemplate::Client => "Acta comercial",
            NoteTemplate::Team => "Acta de reunión",
        }
    }
}

// ---------------------------------------------------------------------------
// Resumen rodante — se regenera cada ~10 min durante la sesión
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RollingSummary {
    /// Notas acumuladas hasta el momento. Máximo ~400 palabras.
    pub texto: String,
    /// Temas tratados hasta ahora, para el panel lateral durante la sesión.
    pub temas_hasta_ahora: Vec<String>,
}

// ---------------------------------------------------------------------------
// Apuntes de clase
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClassNotes {
    /// Dos o tres frases: de qué fue la clase. Lo que leerías si faltaste.
    pub tldr: String,
    /// Agrupados por tema, no en orden cronológico.
    pub temas: Vec<Tema>,
    pub conceptos: Vec<Concepto>,
    #[serde(default)]
    pub formulas: Vec<Formula>,
    /// Fechas de examen, entregas, cambios de aula. El campo más importante del
    /// documento: es lo que el profesor dice una vez y se pierde.
    pub avisos: Vec<Aviso>,
    /// "Esto entra", "esto suele caer", "prestad atención aquí".
    pub senales_examen: Vec<SenalExamen>,
    #[serde(default)]
    pub dudas: Vec<Duda>,
    #[serde(default)]
    pub bibliografia: Vec<Referencia>,
    /// Términos técnicos nuevos, para realimentar el glosario del `topic`.
    #[serde(default)]
    pub glosario_nuevo: Vec<TerminoNuevo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Tema {
    pub titulo: String,
    pub puntos: Vec<String>,
    pub inicio_ms: TsMs,
    pub fin_ms: TsMs,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Concepto {
    pub termino: String,
    /// La definición **tal como la dio el profesor**, no la de manual.
    pub definicion: String,
    pub ejemplo: Option<String>,
    /// El modelo lo marca cuando la transcripción era dudosa y no pudo
    /// resolverla con el glosario ni con el OCR de la diapositiva.
    #[serde(default)]
    pub incierto: bool,
    pub ts_ms: TsMs,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Formula {
    /// En LaTeX siempre que sea posible.
    pub expresion: String,
    pub significado: Option<String>,
    /// Diapositiva visible en ese momento. Para una fórmula, la imagen vale
    /// más que cualquier transcripción de lo que se dijo sobre ella.
    pub slide_id: Option<i64>,
    pub ts_ms: TsMs,
}

str_enum! {
    pub enum AvisoKind {
        Examen        => "examen",
        Entrega       => "entrega",
        CambioAula    => "cambio_aula",
        CambioHorario => "cambio_horario",
        Otro          => "otro",
    }
}

// `str_enum!` no deriva `JsonSchema`; se implementa a mano como enumeración de
// cadenas para que el modelo reciba los valores admitidos.
impl JsonSchema for AvisoKind {
    fn schema_name() -> String {
        "AvisoKind".to_owned()
    }

    fn json_schema(_: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        use schemars::schema::{InstanceType, SchemaObject};
        SchemaObject {
            instance_type: Some(InstanceType::String.into()),
            enum_values: Some(
                AvisoKind::TODAS
                    .iter()
                    .map(|v| serde_json::Value::String(v.as_str().to_owned()))
                    .collect(),
            ),
            ..Default::default()
        }
        .into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Aviso {
    pub tipo: AvisoKind,
    pub texto: String,
    /// ISO-8601. `None` si no se mencionó fecha: es preferible un aviso sin
    /// fecha a una fecha inventada.
    pub fecha: Option<String>,
    pub ts_ms: TsMs,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SenalExamen {
    pub tema: String,
    /// Las palabras exactas del profesor. Es lo que da credibilidad al aviso.
    pub cita: String,
    pub ts_ms: TsMs,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Duda {
    pub pregunta: String,
    pub respuesta: Option<String>,
    pub ts_ms: TsMs,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Referencia {
    pub referencia: String,
    pub ts_ms: TsMs,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TerminoNuevo {
    pub termino: String,
    pub definicion: Option<String>,
}

// ---------------------------------------------------------------------------
// Acta comercial
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClientMinutes {
    pub tldr: String,
    pub contexto_cliente: ContextoCliente,
    /// Los problemas que expresó el cliente, con sus palabras. La información
    /// más valiosa del acta.
    pub necesidades: Vec<Necesidad>,
    #[serde(default)]
    pub objeciones: Vec<Objecion>,
    /// `None` si no se habló de dinero. Nunca estimado.
    pub presupuesto: Option<Presupuesto>,
    pub acuerdos: Vec<Acuerdo>,
    pub compromisos: Compromisos,
    pub proximo_paso: Option<ProximoPaso>,
    #[serde(default)]
    pub senales: Vec<Senal>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ContextoCliente {
    pub empresa: Option<String>,
    pub sector: Option<String>,
    pub tamano: Option<String>,
    #[serde(default)]
    pub herramientas: Vec<String>,
}

str_enum! {
    pub enum Prioridad {
        Alta        => "alta",
        Media       => "media",
        Baja        => "baja",
        Desconocida => "desconocida",
    }
}

impl JsonSchema for Prioridad {
    fn schema_name() -> String {
        "Prioridad".to_owned()
    }

    fn json_schema(_: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        use schemars::schema::{InstanceType, SchemaObject};
        SchemaObject {
            instance_type: Some(InstanceType::String.into()),
            enum_values: Some(
                Prioridad::TODAS
                    .iter()
                    .map(|v| serde_json::Value::String(v.as_str().to_owned()))
                    .collect(),
            ),
            ..Default::default()
        }
        .into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Necesidad {
    pub texto: String,
    pub cita: Option<String>,
    pub prioridad: Prioridad,
    pub ts_ms: TsMs,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Objecion {
    pub objecion: String,
    pub respuesta: Option<String>,
    /// Las objeciones sin resolver son las que hay que preparar para la
    /// siguiente reunión.
    pub resuelta: bool,
    pub ts_ms: TsMs,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Presupuesto {
    pub texto: String,
    pub cifra: Option<String>,
    pub ts_ms: TsMs,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Acuerdo {
    pub texto: String,
    pub ts_ms: TsMs,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct Compromisos {
    #[serde(default)]
    pub mios: Vec<Compromiso>,
    #[serde(default)]
    pub del_cliente: Vec<Compromiso>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Compromiso {
    pub texto: String,
    /// ISO-8601, o `None`. **Nunca inventada**: atribuir a un cliente una
    /// promesa o una fecha que no dio no es un fallo de calidad, es un
    /// problema de negocio.
    pub fecha: Option<String>,
    pub ts_ms: TsMs,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProximoPaso {
    pub texto: String,
    pub fecha: Option<String>,
    pub ts_ms: TsMs,
}

str_enum! {
    pub enum SenalKind {
        Interes => "interes",
        Riesgo  => "riesgo",
    }
}

impl JsonSchema for SenalKind {
    fn schema_name() -> String {
        "SenalKind".to_owned()
    }

    fn json_schema(_: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        use schemars::schema::{InstanceType, SchemaObject};
        SchemaObject {
            instance_type: Some(InstanceType::String.into()),
            enum_values: Some(
                SenalKind::TODAS
                    .iter()
                    .map(|v| serde_json::Value::String(v.as_str().to_owned()))
                    .collect(),
            ),
            ..Default::default()
        }
        .into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Senal {
    pub tipo: SenalKind,
    /// La frase que la sustenta. Sin cita, una "señal de interés" es una
    /// opinión del modelo.
    pub cita: String,
    pub ts_ms: TsMs,
}

// ---------------------------------------------------------------------------
// Acta de reunión simple
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TeamNotes {
    pub tldr: String,
    pub temas: Vec<Tema>,
    pub decisiones: Vec<Acuerdo>,
    pub tareas: Vec<Tarea>,
    #[serde(default)]
    pub abierto: Vec<Acuerdo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Tarea {
    pub texto: String,
    /// `None` si no se dijo. Preferible una tarea sin responsable que un
    /// responsable inventado.
    pub responsable: Option<String>,
    pub fecha: Option<String>,
    pub ts_ms: TsMs,
}

// ---------------------------------------------------------------------------
// Documento final, sea cual sea la plantilla
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "template", rename_all = "snake_case")]
pub enum NotesPayload {
    Class(ClassNotes),
    Client(ClientMinutes),
    Team(TeamNotes),
}

impl NotesPayload {
    pub fn plantilla(&self) -> NoteTemplate {
        match self {
            NotesPayload::Class(_) => NoteTemplate::Class,
            NotesPayload::Client(_) => NoteTemplate::Client,
            NotesPayload::Team(_) => NoteTemplate::Team,
        }
    }

    pub fn tldr(&self) -> &str {
        match self {
            NotesPayload::Class(n) => &n.tldr,
            NotesPayload::Client(n) => &n.tldr,
            NotesPayload::Team(n) => &n.tldr,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_esquema_de_apuntes_se_genera_y_trae_lo_esencial() {
        let esquema = schemars::schema_for!(ClassNotes);
        let json = serde_json::to_value(&esquema).unwrap();
        let props = json["properties"].as_object().unwrap();

        // Los campos que justifican el producto.
        assert!(props.contains_key("avisos"));
        assert!(props.contains_key("senales_examen"));
        assert!(props.contains_key("conceptos"));
    }

    #[test]
    fn el_esquema_comercial_separa_los_compromisos_por_parte() {
        let esquema = schemars::schema_for!(ClientMinutes);
        let json = serde_json::to_value(&esquema).unwrap();
        let compromisos = &json["definitions"]["Compromisos"]["properties"];
        assert!(compromisos.get("mios").is_some());
        assert!(compromisos.get("del_cliente").is_some());
    }

    #[test]
    fn una_respuesta_del_modelo_sin_campos_opcionales_se_deserializa() {
        // Los proveedores en modo `json_mode` omiten campos vacíos con
        // frecuencia. No debe ser un error: el reintento solo debe dispararse
        // por lo que falta de verdad.
        let bruto = serde_json::json!({
            "tldr": "Clase sobre transformadas.",
            "temas": [],
            "conceptos": [],
            "avisos": [],
            "senales_examen": []
        });
        let notas: ClassNotes = serde_json::from_value(bruto).unwrap();
        assert!(notas.formulas.is_empty());
        assert!(notas.bibliografia.is_empty());
    }

    #[test]
    fn el_documento_final_conserva_su_plantilla_al_serializar() {
        let p = NotesPayload::Team(TeamNotes {
            tldr: "x".into(),
            temas: vec![],
            decisiones: vec![],
            tareas: vec![],
            abierto: vec![],
        });
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["template"], "team");
        assert_eq!(p.plantilla(), NoteTemplate::Team);
    }
}
