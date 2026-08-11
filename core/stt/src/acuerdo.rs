//! LocalAgreement-2: estabilización de la transcripción en vivo.
//!
//! El problema que resuelve: al transcribir ventanas solapadas, cada pasada
//! corrige la anterior y el texto «baila» en pantalla. Leerlo mientras el
//! profesor sigue hablando resulta imposible, y ver cómo se reescribe lo que
//! acabas de leer da la impresión de que la aplicación no funciona.
//!
//! La técnica: solo se da por firme el **prefijo en el que coinciden dos
//! hipótesis consecutivas**. Lo que ambas pasadas dicen igual es estable; lo
//! que difiere todavía puede cambiar, así que se muestra como provisional.
//!
//! Es la misma idea de `whisper_streaming`, y es lo que separa una
//! transcripción en vivo usable de una que marea.

/// Estado del acuerdo entre pasadas sucesivas.
#[derive(Debug, Default)]
pub struct AcuerdoLocal {
    /// Palabras de la hipótesis anterior, aún sin confirmar.
    anterior: Vec<String>,
    /// Palabras ya emitidas como definitivas.
    confirmadas: Vec<String>,
}

impl AcuerdoLocal {
    pub fn nuevo() -> Self {
        Self::default()
    }

    /// Incorpora una hipótesis nueva.
    ///
    /// Devuelve `(confirmado_nuevo, provisional)`: lo primero ya no cambiará y
    /// se puede escribir en firme; lo segundo se pinta atenuado porque todavía
    /// puede corregirse.
    pub fn actualizar(&mut self, hipotesis: &str) -> (String, String) {
        let palabras: Vec<String> = hipotesis.split_whitespace().map(String::from).collect();

        // Prefijo común con la hipótesis anterior: eso es lo que dos pasadas
        // independientes han dicho igual.
        let acuerdo = prefijo_comun(&self.anterior, &palabras);

        // Solo interesa lo que va más allá de lo ya emitido.
        let nuevas: Vec<String> = if acuerdo.len() > self.confirmadas.len() {
            acuerdo[self.confirmadas.len()..].to_vec()
        } else {
            Vec::new()
        };

        self.confirmadas = acuerdo;
        self.anterior = palabras.clone();

        let provisional = palabras
            .get(self.confirmadas.len()..)
            .map(|p| p.join(" "))
            .unwrap_or_default();

        (nuevas.join(" "), provisional)
    }

    /// Cierra el flujo dando por buena la última hipótesis entera.
    ///
    /// Se llama cuando el hablante hace una pausa larga: no habrá otra pasada
    /// que confirme el final, y descartarlo perdería la última frase.
    pub fn cerrar(&mut self) -> String {
        let resto = self
            .anterior
            .get(self.confirmadas.len()..)
            .map(|p| p.join(" "))
            .unwrap_or_default();

        self.anterior.clear();
        self.confirmadas.clear();
        resto
    }

    pub fn texto_confirmado(&self) -> String {
        self.confirmadas.join(" ")
    }
}

/// Prefijo común entre dos secuencias de palabras.
///
/// La comparación normaliza mayúsculas, puntuación **y tildes**: que una pasada
/// escriba «Laplace,» y la siguiente «Laplace» no es un desacuerdo real, y
/// tratarlo como tal impediría confirmar nada en una frase con comas.
///
/// Las tildes importan especialmente: Whisper las añade en pasadas posteriores,
/// cuando ha visto más contexto. Sin normalizarlas, un «que» que luego pasa a
/// «qué» bloquearía la confirmación de todo lo que viniera detrás.
fn prefijo_comun(a: &[String], b: &[String]) -> Vec<String> {
    let n = a
        .iter()
        .zip(b.iter())
        .take_while(|(x, y)| normalizar(x) == normalizar(y))
        .count();

    // Se devuelve la versión de la hipótesis nueva: suele traer mejor
    // puntuación, porque el modelo ha visto más contexto.
    b[..n].to_vec()
}

fn normalizar(p: &str) -> String {
    p.chars()
        .flat_map(|c| c.to_lowercase())
        .filter_map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => Some('a'),
            'é' | 'è' | 'ë' | 'ê' => Some('e'),
            'í' | 'ì' | 'ï' | 'î' => Some('i'),
            'ó' | 'ò' | 'ö' | 'ô' => Some('o'),
            'ú' | 'ù' | 'ü' | 'û' => Some('u'),
            c if c.is_alphanumeric() => Some(c),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_primera_hipotesis_no_confirma_nada() {
        // Con una sola pasada no hay con qué comparar: darla por buena sería
        // exactamente el texto que luego baila.
        let mut a = AcuerdoLocal::nuevo();
        let (firme, provisional) = a.actualizar("la transformada de");
        assert_eq!(firme, "");
        assert_eq!(provisional, "la transformada de");
    }

    #[test]
    fn dos_pasadas_de_acuerdo_confirman_el_prefijo() {
        let mut a = AcuerdoLocal::nuevo();
        a.actualizar("la transformada de la place");
        let (firme, provisional) = a.actualizar("la transformada de Laplace convierte");

        // "la transformada de" coincide en ambas: firme.
        assert_eq!(firme, "la transformada de");
        // A partir de ahí discrepan, así que sigue siendo provisional.
        assert_eq!(provisional, "Laplace convierte");
    }

    #[test]
    fn lo_ya_confirmado_no_se_vuelve_a_emitir() {
        let mut a = AcuerdoLocal::nuevo();
        a.actualizar("uno dos tres");
        let (firme1, _) = a.actualizar("uno dos tres cuatro");
        assert_eq!(firme1, "uno dos tres");

        let (firme2, _) = a.actualizar("uno dos tres cuatro cinco");
        // Solo lo nuevo: repetir "uno dos tres" duplicaría la transcripción.
        assert_eq!(firme2, "cuatro");
    }

    #[test]
    fn la_puntuacion_no_cuenta_como_desacuerdo() {
        // Si «Laplace,» y «Laplace» se consideraran distintos, en una frase con
        // comas no se confirmaría nunca nada.
        let mut a = AcuerdoLocal::nuevo();
        a.actualizar("la transformada de Laplace, que");
        let (firme, _) = a.actualizar("La transformada de Laplace que converge");
        assert!(firme.contains("Laplace"), "confirmado: {firme}");
    }

    #[test]
    fn las_tildes_anadidas_despues_no_bloquean_la_confirmacion() {
        // Whisper añade tildes cuando ha visto más contexto. Si «que» y «qué»
        // contaran como desacuerdo, no se confirmaría nada a partir de ahí.
        let mut a = AcuerdoLocal::nuevo();
        a.actualizar("depende de la parte real de ese");
        let (firme, _) = a.actualizar("depende de la parté real de ese numero");
        assert!(firme.contains("real"), "confirmado: {firme}");
    }

    #[test]
    fn se_conserva_la_puntuacion_de_la_hipotesis_mas_reciente() {
        // La pasada nueva ha visto más contexto y suele puntuar mejor.
        let mut a = AcuerdoLocal::nuevo();
        a.actualizar("hola que tal");
        let (firme, _) = a.actualizar("Hola, qué tal");
        assert_eq!(firme, "Hola, qué tal");
    }

    #[test]
    fn una_correccion_completa_no_confirma_nada() {
        let mut a = AcuerdoLocal::nuevo();
        a.actualizar("esto está mal");
        let (firme, provisional) = a.actualizar("otra cosa distinta");
        assert_eq!(firme, "");
        assert_eq!(provisional, "otra cosa distinta");
    }

    #[test]
    fn al_cerrar_se_emite_lo_que_quedaba_pendiente() {
        // En una pausa larga no habrá otra pasada: descartar el resto perdería
        // la última frase del profesor.
        let mut a = AcuerdoLocal::nuevo();
        a.actualizar("uno dos");
        a.actualizar("uno dos tres");
        assert_eq!(a.cerrar(), "tres");
    }

    #[test]
    fn cerrar_deja_el_estado_limpio_para_el_siguiente_segmento() {
        let mut a = AcuerdoLocal::nuevo();
        a.actualizar("uno dos");
        a.cerrar();

        let (firme, _) = a.actualizar("tres cuatro");
        assert_eq!(
            firme, "",
            "tras cerrar, la primera hipótesis vuelve a ser provisional"
        );
    }

    #[test]
    fn una_hipotesis_vacia_no_rompe_nada() {
        let mut a = AcuerdoLocal::nuevo();
        a.actualizar("algo");
        let (firme, provisional) = a.actualizar("");
        assert_eq!(firme, "");
        assert_eq!(provisional, "");
    }
}
