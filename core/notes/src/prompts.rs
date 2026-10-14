//! Plantillas de prompt, versionadas.
//!
//! Van embebidas en el binario para que la aplicación funcione en un equipo
//! recién instalado sin depender de archivos sueltos. La versión se guarda
//! junto a cada documento generado, de modo que se puede reproducir un
//! resultado y comparar la calidad entre versiones del prompt.

/// Súbela al cambiar cualquier plantilla de este archivo. Es lo que permite
/// saber, meses después, con qué instrucciones se generaron unos apuntes.
pub const VERSION: &str = "v3";

/// Cómo leer las etiquetas de hablante de la transcripción.
///
/// Es una macro y no una constante porque `concat!` solo acepta literales, y
/// así el mismo texto entra en los tres prompts que muestran la transcripción
/// sin que puedan desviarse entre sí.
///
/// Antes decía «"Interlocutor" es el cliente». Con la diarización, el sistema
/// puede traer varias voces («Participante 1», «Participante 2»…) o el nombre
/// que el usuario les puso, y el micrófono puede traer a personas de la sala:
/// suponer quién es el cliente atribuiría sus compromisos a otra persona.
macro_rules! aviso_etiquetas {
    () => {
        concat!(
            "En la transcripción, «Yo» es el usuario; el resto de etiquetas son ",
            "otros participantes. Si una etiqueta es un nombre propio, úsalo. Si hay ",
            "varias etiquetas distintas de «Yo», no supongas cuál es el cliente ni ",
            "quién decide: dedúcelo solo de lo que dicen."
        )
    };
}

/// El mismo aviso como constante, para pruebas y para quien arme un prompt a mano.
pub const AVISO_ETIQUETAS: &str = aviso_etiquetas!();

/// Sustitución de marcadores `{nombre}`.
///
/// No se usa un motor de plantillas: los marcadores son pocos y conocidos, y
/// una dependencia más en el núcleo no compensa.
///
/// La sustitución es de **una sola pasada**, y eso no es un detalle: los
/// valores que se insertan son transcripciones reales, que pueden contener
/// llaves. Con `replace` encadenado, un `{glosario}` dicho en clase acabaría
/// reemplazado por el glosario de verdad.
pub fn render(plantilla: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(plantilla.len() + 1024);
    let mut resto = plantilla;

    while let Some(i) = resto.find('{') {
        out.push_str(&resto[..i]);

        match resto[i..].find('}') {
            Some(j) => {
                let nombre = &resto[i + 1..i + j];
                match vars.iter().find(|(k, _)| *k == nombre) {
                    Some((_, v)) => out.push_str(v),
                    // Un marcador sin valor se deja visible: es preferible
                    // verlo en el prompt y detectarlo, a que desaparezca en
                    // silencio y nadie note que falta el dato.
                    None => out.push_str(&resto[i..=i + j]),
                }
                resto = &resto[i + j + 1..];
            }
            None => {
                out.push_str(&resto[i..]);
                return out;
            }
        }
    }

    out.push_str(resto);
    out
}

// ---------------------------------------------------------------------------
// Sistema
// ---------------------------------------------------------------------------

pub const SISTEMA: &str = "\
Eres un asistente que convierte transcripciones automáticas en documentos \
estructurados y verificables.

Reglas que no se rompen nunca:
- Solo puedes afirmar lo que está en la transcripción. No añades conocimiento \
  externo, ni recomendaciones, ni opiniones propias.
- Cada elemento que generas lleva su marca de tiempo «ts_ms», copiada del \
  número entre corchetes que precede a la frase de la que sale. Es lo que \
  permite al usuario pinchar y verificarlo en el audio.
- Si un dato no se dijo, el campo va a null. Nunca lo estimas ni lo deduces.
- La transcripción es automática y tiene errores. Si una palabra parece mal \
  transcrita pero el sentido es claro, la corriges usando el glosario o el \
  texto de las diapositivas. Si no está claro, la dejas como está.
- Respondes en el idioma de la transcripción.
- FECHAS: resuelves las fechas relativas contra la fecha de la sesión que se
  te indica. «El quince de septiembre» significa el 15 de septiembre siguiente
  a esa fecha, no el de un año cualquiera. Nunca inventes el año: si dudas
  entre dos, elige el que deje la fecha en el futuro respecto de la sesión.
  Si no se mencionó fecha, el campo va a null.";

// ---------------------------------------------------------------------------
// Map — extracción por bloque
// ---------------------------------------------------------------------------

pub const MAP: &str = concat!(
    "Analiza este fragmento de la transcripción y extrae lo que contenga, ",
    "siguiendo el esquema.\n\n",
    aviso_etiquetas!(),
    "\n\n",
    "TIPO DE SESIÓN: {tipo}
CONTEXTO: {contexto}
FECHA DE LA SESIÓN: {fecha}
GLOSARIO CONOCIDO: {glosario}
{diapositivas}
FRAGMENTO (bloque {bloque} de {total}, de {inicio} a {fin} ms):
{transcripcion}

Extrae únicamente lo que aparezca en ESTE fragmento. Si un campo no tiene \
contenido aquí, devuelve una lista vacía; no lo rellenes con nada del \
contexto general. El resumen «tldr» de este bloque debe describir solo este \
fragmento, no la sesión entera."
);

// ---------------------------------------------------------------------------
// Reduce — documento final
// ---------------------------------------------------------------------------

pub const REDUCE_CLASE: &str = "\
Tienes los extractos estructurados de todos los bloques de una clase de \
{duracion}.

ASIGNATURA: {contexto}
FECHA DE LA CLASE: {fecha}
EXTRACTOS POR BLOQUE:
{extractos}
{diapositivas}
Consolida todo en un único documento de apuntes, siguiendo el esquema.

Criterios:
- «tldr»: dos o tres frases. De qué fue la clase. Lo que leerías si faltaste.
- «temas»: agrupa por tema, NO en orden cronológico. Los bloques se solapan, \
  así que la misma idea aparecerá repetida en varios extractos: fúndela en una \
  sola entrada, no la repitas.
- «conceptos»: la definición TAL COMO la dio el profesor, no la de manual. Si \
  el profesor se equivocó, transcribes lo que dijo; no lo corriges con lo que \
  sabes. Unos apuntes que «mejoran» al profesor no sirven para su examen.
- «formulas»: referencia con «slide_id» la diapositiva visible en ese momento. \
  Para una fórmula, la imagen vale más que su transcripción.
- «avisos»: fechas de examen, entregas, cambios de aula o de horario. Es el \
  campo más importante del documento: el profesor lo dice una sola vez.
- «senales_examen»: todo lo que marcó como importante —«esto entra», «esto \
  suele caer», «prestad atención aquí»—. Cita sus palabras exactas.
- «glosario_nuevo»: términos técnicos que no estaban ya en el glosario.
- Conserva el «ts_ms» original de cada elemento.";

pub const REDUCE_CLIENTE: &str = concat!(
    "Tienes los extractos estructurados de una reunión de {duracion} con un cliente.

CLIENTE: {contexto}
FECHA DE LA REUNIÓN: {fecha}
",
    aviso_etiquetas!(),
    "

EXTRACTOS POR BLOQUE:
{extractos}

Consolida todo en un acta, siguiendo el esquema.

Criterios:
- «tldr»: dos o tres frases. En qué quedó la reunión y qué pasa ahora.
- «contexto_cliente»: solo lo que se dijo en la reunión. Nada inferido.
- «necesidades»: los problemas que expresó el cliente, con sus palabras \
  siempre que sea posible. Es la información más valiosa del acta.
- «objeciones»: los reparos que planteó y cómo se respondieron. Marca como no \
  resueltas las que quedaron sin respuesta satisfactoria: son las que hay que \
  preparar para la próxima reunión.
- «presupuesto»: cifras o restricciones económicas mencionadas. Si no se habló \
  de dinero, null. No estimes.
- «compromisos»: separados en «mios» y «del_cliente», cada uno con su fecha si \
  se dijo. NUNCA inventes un compromiso ni una fecha. Atribuir al cliente una \
  promesa que no hizo no es un fallo de calidad: es un problema de negocio.
- «senales»: indicios de interés o de riesgo, citando la frase que los sustenta.
- No añades recomendaciones comerciales: documentas la reunión, no la asesoras.
- Los bloques se solapan, así que el mismo asunto aparecerá en varios \
  extractos: fúndelo en una sola entrada en lugar de repetirlo.
- Conserva el «ts_ms» original de cada elemento."
);

pub const REDUCE_EQUIPO: &str = concat!(
    "Tienes los extractos estructurados de una reunión de trabajo de {duracion}.

CONTEXTO: {contexto}
FECHA DE LA REUNIÓN: {fecha}
",
    aviso_etiquetas!(),
    "
EXTRACTOS POR BLOQUE:
{extractos}

Consolida todo en un acta, siguiendo el esquema.

Criterios:
- «tldr»: dos o tres frases. Qué se decidió y qué pasa ahora.
- «decisiones»: solo lo acordado de forma explícita. Lo que quedó a medias va \
  en «abierto», no en «decisiones».
- «tareas»: responsable y fecha ÚNICAMENTE si se dijeron. En caso contrario, \
  null. Es preferible una tarea sin responsable que un responsable inventado.
- Los bloques se solapan: funde los duplicados en lugar de repetirlos.
- Conserva el «ts_ms» original de cada elemento."
);

// ---------------------------------------------------------------------------
// Resumen rodante — durante la sesión
// ---------------------------------------------------------------------------

pub const RODANTE: &str = "\
Estás tomando notas de una sesión EN CURSO.

NOTAS ACUMULADAS HASTA AHORA:
{previo}

NUEVO FRAGMENTO ({inicio} – {fin} ms):
{transcripcion}

Actualiza las notas acumuladas integrando el nuevo fragmento.
- Conserva lo relevante de las notas previas; no las reescribas desde cero.
- Añade solo lo que aparezca de forma explícita en la transcripción.
- Si el fragmento contradice las notas previas, prevalece lo más reciente y lo \
  indicas.
- Máximo 400 palabras.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_sustituyen_los_marcadores() {
        let r = render(
            "hola {quien}, son las {hora}",
            &[("quien", "Ana"), ("hora", "5")],
        );
        assert_eq!(r, "hola Ana, son las 5");
    }

    #[test]
    fn un_marcador_sin_valor_se_queda_visible() {
        // Preferible ver `{glosario}` en el prompt y detectarlo, a que
        // desaparezca en silencio y nadie note que falta el dato.
        let r = render("a {b} c", &[]);
        assert!(r.contains("{b}"));
    }

    #[test]
    fn el_texto_a_sustituir_no_se_reinterpreta_como_marcador() {
        // Una transcripción puede contener llaves; sustituir en cadena no debe
        // provocar sustituciones en el valor ya insertado.
        let r = render("{a}|{b}", &[("a", "{b}"), ("b", "FIN")]);
        assert_eq!(r, "{b}|FIN", "el valor insertado no debe re-sustituirse");
    }

    #[test]
    fn las_reglas_criticas_estan_en_el_prompt_de_sistema() {
        // Son las que evitan el fallo más dañino: inventar datos.
        assert!(SISTEMA.contains("ts_ms"));
        assert!(SISTEMA.contains("null"));
        assert!(SISTEMA.contains("No añades conocimiento externo"));
    }

    #[test]
    fn los_prompts_que_muestran_la_transcripcion_explican_las_etiquetas() {
        // Con diarización, el sistema puede traer «Participante 1», «Participante
        // 2» o un nombre puesto por el usuario. Antes los prompts decían que
        // «Interlocutor» era el cliente, y las actas de reuniones con varias
        // personas habrían atribuido a una sola todo lo que dijeron los demás.
        for p in [MAP, REDUCE_CLIENTE, REDUCE_EQUIPO] {
            assert!(p.contains("«Yo» es el usuario"));
            assert!(p.contains("nombre propio, úsalo"));
            assert!(p.contains(AVISO_ETIQUETAS));
            assert!(!p.contains("«Interlocutor» es el cliente"));
        }
    }

    #[test]
    fn el_aviso_de_etiquetas_no_introduce_marcadores_sin_sustituir() {
        // `render` deja visible cualquier `{algo}` que no conozca; si el aviso
        // trajera llaves, cada prompt saldría con un marcador huérfano.
        assert!(!AVISO_ETIQUETAS.contains(['{', '}']));
    }

    #[test]
    fn el_acta_de_cliente_prohibe_inventar_compromisos() {
        assert!(REDUCE_CLIENTE.contains("NUNCA inventes un compromiso"));
    }

    #[test]
    fn los_apuntes_prohiben_corregir_al_profesor() {
        assert!(REDUCE_CLASE.contains("no lo corriges"));
    }

    #[test]
    fn todas_las_plantillas_de_reduce_avisan_del_solape() {
        // Sin este aviso, el documento final repite cada idea tantas veces
        // como bloques la contengan.
        for p in [REDUCE_CLASE, REDUCE_CLIENTE, REDUCE_EQUIPO] {
            assert!(
                p.contains("solap"),
                "una plantilla de reduce no menciona el solape entre bloques"
            );
        }
    }
}
