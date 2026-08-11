//! Filtrado de alucinaciones de Whisper.
//!
//! Ante un tramo sin voz clara, Whisper no devuelve vacío: se inventa una
//! frase. Y no lo hace con poca confianza —«Música» salió con 0,88 en una
//! prueba real—, así que filtrar por confianza no sirve. Lo que sí funciona es
//! que el repertorio es **muy** limitado y reconocible: son restos del corpus
//! de subtítulos con el que se entrenó el modelo.
//!
//! Es la última defensa. La primera es el VAD, que evita mandarle silencio.

/// Frases que Whisper produce de la nada, sobre todo en español.
///
/// Todas vienen de subtítulos de vídeo, que es de donde salió buena parte de
/// su entrenamiento. La comparación es sobre el texto normalizado.
const ALUCINACIONES: &[&str] = &[
    "musica",
    "aplausos",
    "risas",
    "subtitulos por la comunidad de amaraorg",
    "subtitulos realizados por la comunidad de amaraorg",
    "subtitulado por la comunidad de amaraorg",
    "mas informacion wwwalimmentaorg",
    "gracias por ver el video",
    "gracias por ver este video",
    "no te olvides de suscribirte",
    "suscribete al canal",
    "dale like y suscribete",
    "hasta la proxima",
    "nos vemos en el proximo video",
    "thanks for watching",
    "thank you for watching",
    "please subscribe",
    "you",
    "bye",
    "ok",
];

/// Palabras que solo aparecen en las coletillas de subtítulos.
///
/// Se usan para cazar variantes que no están en la lista literal: en una prueba
/// real se coló «*Suscribete*», porque la lista tenía «suscribete al canal»
/// pero no la forma suelta. Enumerar todas las variantes es una carrera
/// perdida; buscar la raíz en un fragmento corto, no.
const RAICES: &[&str] = &["suscrib", "amaraorg", "subtitul", "subscribe"];

/// Si un fragmento parece inventado y hay que descartarlo.
///
/// El criterio es deliberadamente conservador: solo se descarta lo que coincide
/// con el repertorio conocido, no cualquier frase corta. Borrar una frase real
/// del profesor sería mucho peor que dejar pasar una alucinación, que como
/// mucho ensucia un poco los apuntes.
pub fn es_alucinacion(texto: &str) -> bool {
    let n = normalizar(texto);
    if n.is_empty() {
        return true;
    }

    if ALUCINACIONES.contains(&n.as_str()) {
        return true;
    }

    // Un solo carácter nunca es una intervención. Whisper los suelta en tramos
    // de ruido: en una prueba real apareció una «I» suelta. No hay palabra en
    // español que valga como frase completa con una sola letra.
    if n.chars().count() == 1 {
        return true;
    }

    let palabras: Vec<&str> = n.split_whitespace().collect();

    // Fragmento corto que contiene una raíz de coletilla. La condición de
    // brevedad es lo que lo hace seguro: «no te olvides de suscribirte» es una
    // alucinación, pero «el profesor dijo que nos suscribiéramos al aula
    // virtual para ver los apuntes» es una frase real y no se toca.
    if palabras.len() <= 5 && RAICES.iter().any(|r| n.contains(r)) {
        return true;
    }

    // Repetición de la misma palabra: el otro modo de fallo típico, cuando el
    // decodificador se engancha en un bucle. «gracias gracias gracias…».
    if palabras.len() >= 6 {
        let primera = palabras[0];
        if palabras.iter().all(|p| *p == primera) {
            return true;
        }
    }

    false
}

/// Quita puntuación y tildes para comparar.
fn normalizar(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => Some('a'),
            'é' | 'è' | 'ë' | 'ê' => Some('e'),
            'í' | 'ì' | 'ï' | 'î' => Some('i'),
            'ó' | 'ò' | 'ö' | 'ô' => Some('o'),
            'ú' | 'ù' | 'ü' | 'û' => Some('u'),
            'ñ' => Some('n'),
            c if c.is_alphanumeric() || c.is_whitespace() => Some(c),
            _ => None,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Repeticiones consecutivas que delatan un bucle del decodificador.
///
/// Es el modo de fallo más aparatoso de Whisper: ante un tramo que no entiende,
/// se engancha y devuelve la misma frase una y otra vez. En una prueba real
/// salió «Como el palo de la ciudad de México» diez veces seguidas sobre un
/// audio que hablaba de transformadas.
///
/// Repetir algo dos veces puede ser real —un profesor recalcando— pero tres o
/// más veces seguidas y palabra por palabra, no lo es nunca.
pub const REPETICIONES_QUE_DELATAN_BUCLE: usize = 3;

/// Elimina los bucles de una lista de fragmentos consecutivos.
///
/// Devuelve `(fragmentos_limpios, descartados)`. Se opera sobre los textos ya
/// normalizados para que un cambio de puntuación entre repeticiones no despiste.
pub fn colapsar_bucles<T: AsRef<str>>(textos: &[T]) -> (Vec<usize>, usize) {
    let mut conservar = Vec::new();
    let mut i = 0;

    while i < textos.len() {
        let actual = normalizar(textos[i].as_ref());

        // Cuántas veces seguidas se repite exactamente lo mismo.
        let mut j = i + 1;
        while j < textos.len() && normalizar(textos[j].as_ref()) == actual {
            j += 1;
        }
        let repeticiones = j - i;

        if repeticiones < REPETICIONES_QUE_DELATAN_BUCLE {
            // Una o dos veces: puede ser real. Se conserva solo la primera,
            // porque aunque lo fuera, repetirlo en los apuntes no aporta.
            conservar.push(i);
        }
        // Tres o más: se descarta el bloque entero. No es habla.

        i = j;
    }

    let descartados = textos.len() - conservar.len();
    (conservar, descartados)
}

/// Similitud entre dos textos, de 0 a 1, por palabras compartidas.
///
/// Se usa para detectar que las dos pistas están grabando lo mismo: si el
/// usuario escucha por altavoces en vez de auriculares, su micrófono capta al
/// profesor, y sin esto los apuntes le atribuirían a él lo que dijo el
/// profesor.
pub fn similitud(a: &str, b: &str) -> f32 {
    let pa: Vec<String> = normalizar(a).split_whitespace().map(String::from).collect();
    let pb: Vec<String> = normalizar(b).split_whitespace().map(String::from).collect();

    if pa.is_empty() || pb.is_empty() {
        return 0.0;
    }

    let set_b: std::collections::HashSet<&String> = pb.iter().collect();
    let comunes = pa.iter().filter(|p| set_b.contains(p)).count();

    // Sobre la más corta: un fragmento del micro suele recoger solo parte de
    // lo que dijo el profesor, y aun así es la misma voz filtrada.
    comunes as f32 / pa.len().min(pb.len()) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_reconocen_las_alucinaciones_tipicas() {
        assert!(es_alucinacion("Música"));
        assert!(es_alucinacion(" ¡Música! "));
        assert!(es_alucinacion("Subtítulos por la comunidad de Amara.org"));
        assert!(es_alucinacion("Gracias por ver el vídeo"));
    }

    #[test]
    fn se_cazan_las_variantes_no_enumeradas() {
        // Casos reales que se colaron en las primeras pruebas.
        assert!(es_alucinacion("*Suscribete*"));
        assert!(es_alucinacion("¡Suscríbete!"));
        assert!(es_alucinacion("Subtítulos realizados por la comunidad"));
    }

    #[test]
    fn una_frase_larga_con_esas_palabras_no_se_descarta() {
        // La brevedad es lo que hace segura la heurística de raíces.
        assert!(!es_alucinacion(
            "el profesor dijo que nos suscribiéramos al aula virtual para ver los apuntes"
        ));
        assert!(!es_alucinacion(
            "los subtítulos del vídeo del tema tres están en el campus"
        ));
    }

    #[test]
    fn no_se_descarta_lo_que_dijo_el_profesor() {
        // Borrar una frase real es mucho peor que dejar pasar una alucinación.
        assert!(!es_alucinacion("La transformada de Laplace"));
        assert!(!es_alucinacion("Esto entra en el examen"));
        assert!(!es_alucinacion("Sí"));
        assert!(!es_alucinacion("La música de Bach también sigue patrones"));
    }

    #[test]
    fn un_caracter_suelto_se_descarta() {
        // Caso real: una «I» suelta sobre un tramo de ruido.
        assert!(es_alucinacion("I"));
        assert!(es_alucinacion("¿"));
        // Pero dos letras sí pueden ser una respuesta real.
        assert!(!es_alucinacion("Sí"));
        assert!(!es_alucinacion("No"));
    }

    #[test]
    fn un_fragmento_vacio_se_descarta() {
        assert!(es_alucinacion(""));
        assert!(es_alucinacion("   "));
        assert!(es_alucinacion("..."));
    }

    #[test]
    fn se_detecta_el_bucle_del_decodificador() {
        // El otro modo de fallo: el decodificador se engancha repitiendo.
        assert!(es_alucinacion(
            "gracias gracias gracias gracias gracias gracias"
        ));
        // Pero una repetición corta puede ser real («no, no, no»).
        assert!(!es_alucinacion("no no no"));
    }

    #[test]
    fn se_descarta_un_bucle_del_decodificador() {
        // Caso real: la misma frase inventada diez veces seguidas.
        let textos = vec![
            "Como el palo de la ciudad de México",
            "Como el palo de la ciudad de México",
            "Como el palo de la ciudad de México",
            "Como el palo de la ciudad de México",
        ];
        let (conservar, descartados) = colapsar_bucles(&textos);
        assert!(conservar.is_empty(), "el bloque entero debe irse");
        assert_eq!(descartados, 4);
    }

    #[test]
    fn una_repeticion_doble_conserva_una_copia() {
        // Un profesor recalcando algo dos veces es plausible.
        let textos = vec!["esto entra en el examen", "esto entra en el examen"];
        let (conservar, descartados) = colapsar_bucles(&textos);
        assert_eq!(conservar, vec![0]);
        assert_eq!(descartados, 1);
    }

    #[test]
    fn el_texto_normal_pasa_intacto() {
        let textos = vec!["primera frase", "segunda frase", "tercera frase"];
        let (conservar, descartados) = colapsar_bucles(&textos);
        assert_eq!(conservar, vec![0, 1, 2]);
        assert_eq!(descartados, 0);
    }

    #[test]
    fn un_bucle_no_arrastra_lo_que_viene_despues() {
        let textos = vec![
            "bucle",
            "bucle",
            "bucle",
            "la transformada de Laplace converge",
        ];
        let (conservar, _) = colapsar_bucles(&textos);
        assert_eq!(conservar, vec![3], "lo real de después debe sobrevivir");
    }

    #[test]
    fn la_puntuacion_no_impide_ver_la_repeticion() {
        let textos = vec!["¡Música!", "Música", "música."];
        let (conservar, _) = colapsar_bucles(&textos);
        assert!(conservar.is_empty());
    }

    #[test]
    fn la_similitud_detecta_la_misma_frase_en_las_dos_pistas() {
        let sistema = "la región de convergencia siempre cae en el examen";
        let micro = "la región de convergencia siempre cae en el examen";
        assert!(similitud(micro, sistema) > 0.9);
    }

    #[test]
    fn un_fragmento_parcial_tambien_cuenta_como_la_misma_voz() {
        // El micro capta peor y recoge solo parte de la frase.
        let sistema = "la región de convergencia siempre cae en el examen del tema tres";
        let micro = "región de convergencia siempre cae";
        assert!(
            similitud(micro, sistema) > 0.8,
            "{}",
            similitud(micro, sistema)
        );
    }

    #[test]
    fn dos_frases_distintas_no_se_confunden() {
        let a = "la transformada de Laplace convierte ecuaciones diferenciales";
        let b = "¿puede repetir la última parte por favor?";
        assert!(similitud(a, b) < 0.3, "{}", similitud(a, b));
    }

    #[test]
    fn las_tildes_no_impiden_reconocer_la_misma_frase() {
        assert!(similitud("la region de convergencia", "la región de convergencia") > 0.9);
    }
}
