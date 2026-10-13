//! Reconocimiento incremental en español con Sherpa-ONNX.
//!
//! El reconocedor se comparte entre las pistas y cada una conserva su propio
//! flujo. La inferencia se limita a un hilo para dejar recursos al audio y al
//! video durante la clase.

use crate::nivel::interpretar_interruptor;
use crate::{Result, SttError};
use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig, OnlineStream};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const MODELO_ES: &str = "sherpa-onnx-streaming-zipformer-es-kroko-2025-08-06";
/// Marca de inicio de palabra en `tokens.txt` (U+2581).
const INICIO_PALABRA: &str = "\u{2581}";

/// Valores de producción. Las variables `DICTAR_SHERPA_*` solo existen para el
/// arnés de evaluación y los sobrescriben; sin ellas mandan estas constantes.
///
/// Siguen en `false` hasta que la medición de `sherpa_live_eval` diga lo
/// contrario con las reglas de decisión de AV-3; ver el HANDOFF de esa tarea.
pub const HAZ_POR_DEFECTO: bool = false;
pub const PALABRAS_CLAVE_POR_DEFECTO: bool = false;
/// Empuje que recibe cada palabra clave en el haz.
pub const PUNTUACION_PALABRAS_CLAVE: f32 = 1.5;
/// Caminos que conserva `modified_beam_search`. Cada uno es una decodificación
/// completa por trama, así que este número es casi directamente la carga.
pub const RUTAS_ACTIVAS: i32 = 4;
/// Por debajo de esto un término es más fácil de disparar por error que de
/// acertar: «red» o «ip» empujarían ruido en cualquier frase.
pub const LONGITUD_MIN_TERMINO: usize = 4;

pub const VAR_DECODIFICACION: &str = "DICTAR_SHERPA_DECODIFICACION";
pub const VAR_PALABRAS_CLAVE: &str = "DICTAR_SHERPA_PALABRAS_CLAVE";

static MODELO_ES_CACHE: OnceLock<Mutex<Option<Arc<SherpaModelo>>>> = OnceLock::new();

/// Cómo se convierte un término en lo que el motor entiende.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segmentacion {
    /// El motor tokeniza con `bpe.vocab` (`modeling_unit = bpe`); el texto va tal cual.
    Bpe,
    /// Se entrega ya troceado con piezas de `tokens.txt`, cogiendo siempre la
    /// pieza más larga que encaje.
    PiezaMasLarga,
    /// Se entrega ya troceado con el menor número de piezas posible.
    MinimoDePiezas,
}

#[derive(Debug, Clone)]
pub struct OpcionesSherpa {
    pub haz: bool,
    pub palabras_clave: bool,
    pub rutas_activas: i32,
    pub puntuacion: f32,
    pub segmentacion: Segmentacion,
    /// `bpe.vocab`, con una pieza y su puntuación por línea; solo con `Bpe`.
    pub vocab_bpe: Option<PathBuf>,
}

impl OpcionesSherpa {
    /// La configuración de producción, con las constantes de este módulo.
    pub fn produccion() -> Self {
        Self {
            haz: HAZ_POR_DEFECTO,
            palabras_clave: PALABRAS_CLAVE_POR_DEFECTO,
            rutas_activas: RUTAS_ACTIVAS,
            puntuacion: PUNTUACION_PALABRAS_CLAVE,
            segmentacion: Segmentacion::PiezaMasLarga,
            vocab_bpe: None,
        }
    }

    /// Único sitio donde se leen las variables de evaluación.
    pub fn desde_entorno() -> Result<Self> {
        let decodificacion = std::env::var(VAR_DECODIFICACION).ok();
        let palabras = std::env::var(VAR_PALABRAS_CLAVE).ok();
        Self::interpretar(decodificacion.as_deref(), palabras.as_deref())
    }

    fn interpretar(decodificacion: Option<&str>, palabras: Option<&str>) -> Result<Self> {
        let mut opciones = Self::produccion();
        if let Some(valor) = decodificacion {
            opciones.haz = match valor {
                "greedy" => false,
                "haz" => true,
                otro => {
                    return Err(SttError::Carga(format!(
                        "{VAR_DECODIFICACION}={otro:?} no vale; use greedy o haz"
                    )))
                }
            };
        }
        opciones.palabras_clave =
            interpretar_interruptor(VAR_PALABRAS_CLAVE, palabras, opciones.palabras_clave)
                .map_err(SttError::Carga)?;
        // Las palabras clave solo existen dentro del haz: pedirlas con greedy
        // las dejaría mudas sin avisar, que es justo lo que este proyecto ya
        // ha sufrido con otras funciones.
        if opciones.palabras_clave && !opciones.haz {
            return Err(SttError::Carga(format!(
                "{VAR_PALABRAS_CLAVE}=1 exige {VAR_DECODIFICACION}=haz"
            )));
        }
        Ok(opciones)
    }

    fn texto_decodificacion(&self) -> &'static str {
        if self.haz {
            "modified_beam_search"
        } else {
            "greedy_search"
        }
    }
}

pub struct SherpaModelo {
    reconocedor: OnlineRecognizer,
    piezas: HashSet<String>,
    opciones: OpcionesSherpa,
}

pub struct SherpaVivo {
    modelo: Arc<SherpaModelo>,
    flujo: OnlineStream,
}

/// Por qué se descartó un término del glosario.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotivoRechazo {
    Corto,
    NoSegmentable,
    Repetido,
}

/// Resultado de preparar el glosario para el motor.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TerminosPreparados {
    /// Términos aceptados, ya normalizados y en la forma que recibe el motor.
    pub aceptados: Vec<String>,
    pub rechazados: Vec<(String, MotivoRechazo)>,
}

impl TerminosPreparados {
    /// Texto para `create_stream_with_hotwords`: términos separados por `/`.
    pub fn para_motor(&self) -> String {
        self.aceptados.join("/")
    }
}

impl SherpaModelo {
    pub fn cargar(ruta: &Path, opciones: &OpcionesSherpa) -> Result<Self> {
        for archivo in ["encoder.onnx", "decoder.onnx", "joiner.onnx", "tokens.txt"] {
            if !ruta.join(archivo).is_file() {
                return Err(SttError::ModeloNoEncontrado(
                    ruta.join(archivo).display().to_string(),
                ));
            }
        }
        let piezas = leer_piezas(&ruta.join("tokens.txt"))?;

        let archivo = |nombre: &str| Some(ruta.join(nombre).display().to_string());
        let mut config = OnlineRecognizerConfig::default();
        config.model_config.transducer.encoder = archivo("encoder.onnx");
        config.model_config.transducer.decoder = archivo("decoder.onnx");
        config.model_config.transducer.joiner = archivo("joiner.onnx");
        config.model_config.tokens = archivo("tokens.txt");
        config.model_config.provider = Some("cpu".to_owned());
        config.model_config.num_threads = 1;
        config.decoding_method = Some(opciones.texto_decodificacion().to_owned());
        if opciones.haz {
            config.max_active_paths = opciones.rutas_activas;
            config.hotwords_score = opciones.puntuacion;
        }
        match (opciones.segmentacion, &opciones.vocab_bpe) {
            (Segmentacion::Bpe, Some(vocab)) => {
                config.model_config.modeling_unit = Some("bpe".to_owned());
                config.model_config.bpe_vocab = Some(vocab.display().to_string());
            }
            (Segmentacion::Bpe, None) => {
                return Err(SttError::Carga(
                    "la segmentación Bpe necesita un bpe.vocab".to_owned(),
                ))
            }
            // Fijado a propósito: el valor por omisión depende de la versión de
            // la biblioteca y de él depende cómo se leen los términos.
            _ => config.model_config.modeling_unit = Some("cjkchar".to_owned()),
        }
        config.enable_endpoint = true;
        config.rule1_min_trailing_silence = 2.4;
        config.rule2_min_trailing_silence = 1.2;
        config.rule3_min_utterance_length = 20.0;

        let reconocedor = OnlineRecognizer::create(&config).ok_or_else(|| {
            SttError::Carga(format!(
                "Sherpa-ONNX no pudo abrir el modelo {}",
                ruta.display()
            ))
        })?;
        tracing::info!(
            decodificacion = opciones.texto_decodificacion(),
            palabras_clave = opciones.palabras_clave,
            rutas_activas = opciones.rutas_activas,
            "reconocedor Sherpa cargado"
        );
        Ok(Self {
            reconocedor,
            piezas,
            opciones: opciones.clone(),
        })
    }

    pub fn opciones(&self) -> &OpcionesSherpa {
        &self.opciones
    }

    pub fn nuevo_flujo(modelo: &Arc<Self>) -> SherpaVivo {
        SherpaVivo {
            modelo: modelo.clone(),
            flujo: modelo.reconocedor.create_stream(),
        }
    }

    /// Prepara el glosario para este modelo. Devuelve vacío si las palabras
    /// clave están apagadas: quien llama no necesita preguntarlo antes.
    pub fn preparar_terminos(&self, glosario: &str) -> TerminosPreparados {
        if !self.opciones.palabras_clave {
            return TerminosPreparados::default();
        }
        preparar_terminos(glosario, &self.piezas, self.opciones.segmentacion)
    }

    /// Crea un flujo que favorece los términos dados. Con la lista vacía es
    /// idéntico a [`Self::nuevo_flujo`]: no se paga un grafo de contexto vacío.
    pub fn nuevo_flujo_con_terminos(
        modelo: &Arc<Self>,
        terminos: &TerminosPreparados,
    ) -> SherpaVivo {
        if terminos.aceptados.is_empty() {
            return Self::nuevo_flujo(modelo);
        }
        Self::nuevo_flujo_con_texto_crudo(modelo, &terminos.para_motor())
    }

    /// Entrega `texto` al motor tal cual (menos los `\0`). Existe para explorar
    /// formatos de término; la aplicación usa `nuevo_flujo_con_terminos`.
    pub fn nuevo_flujo_con_texto_crudo(modelo: &Arc<Self>, texto: &str) -> SherpaVivo {
        // `create_stream_with_hotwords` hace `CString::new(..).unwrap()`: un
        // `\0` dentro de un término abortaría el proceso entero.
        let limpio: String = texto.chars().filter(|c| *c != '\0').collect();
        SherpaVivo {
            modelo: modelo.clone(),
            flujo: modelo.reconocedor.create_stream_with_hotwords(&limpio),
        }
    }
}

fn leer_piezas(tokens: &Path) -> Result<HashSet<String>> {
    let contenido = std::fs::read_to_string(tokens)?;
    Ok(piezas_de_tokens(&contenido))
}

/// Piezas de un `tokens.txt` (`pieza id` por línea).
pub fn piezas_de_tokens(contenido: &str) -> HashSet<String> {
    contenido
        .lines()
        .filter_map(|linea| linea.rsplit_once(' ').map(|(pieza, _)| pieza.to_owned()))
        .collect()
}

/// Lleva un término del glosario a la forma en que lo escribe el modelo y le
/// quita lo que el motor leería como estructura.
///
/// El motor separa términos por `/` o salto de línea, la puntuación por `:` y
/// las piezas por espacio, y un `\0` hace abortar al enlace. Las mayúsculas
/// cuentan: `tokens.txt` distingue `▁La` de `▁la`, y una palabra clave que no
/// coincide con lo que el modelo emite no empuja nada.
pub fn normalizar_termino(termino: &str) -> String {
    let limpio: String = termino
        .chars()
        .map(|c| match c {
            '\0' | '/' | ':' | ',' | ';' | '\n' | '\r' | '\t' => ' ',
            otro => otro,
        })
        .collect();
    limpio
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Trocea una palabra en piezas de `tokens.txt`. `None` si algún carácter no
/// existe ni suelto ni con el prefijo `▁`.
pub fn segmentar_palabra(
    palabra: &str,
    piezas: &HashSet<String>,
    estrategia: Segmentacion,
) -> Option<Vec<String>> {
    let caracteres: Vec<char> = palabra.chars().collect();
    if caracteres.is_empty() {
        return None;
    }
    let trozo =
        |desde: usize, hasta: usize| -> String { caracteres[desde..hasta].iter().collect() };

    // Lo normal: la primera pieza lleva el prefijo de inicio de palabra.
    let con_prefijo = segmentar_con(estrategia, caracteres.len(), |desde, hasta| {
        let t = trozo(desde, hasta);
        let candidata = if desde == 0 {
            format!("{INICIO_PALABRA}{t}")
        } else {
            t
        };
        piezas.contains(&candidata).then_some(candidata)
    });
    if con_prefijo.is_some() {
        return con_prefijo;
    }
    // Ninguna pieza con prefijo cubre el arranque: el prefijo va como pieza propia.
    if !piezas.contains(INICIO_PALABRA) {
        return None;
    }
    let mut suelto = segmentar_con(estrategia, caracteres.len(), |desde, hasta| {
        let t = trozo(desde, hasta);
        piezas.contains(&t).then_some(t)
    })?;
    suelto.insert(0, INICIO_PALABRA.to_owned());
    Some(suelto)
}

/// Recorre `n` caracteres eligiendo piezas con `pieza(desde, hasta)`.
fn segmentar_con(
    estrategia: Segmentacion,
    n: usize,
    pieza: impl Fn(usize, usize) -> Option<String>,
) -> Option<Vec<String>> {
    match estrategia {
        Segmentacion::Bpe | Segmentacion::PiezaMasLarga => {
            let mut resultado = Vec::new();
            let mut desde = 0;
            while desde < n {
                let (hasta, elegida) = (desde + 1..=n)
                    .rev()
                    .find_map(|hasta| pieza(desde, hasta).map(|p| (hasta, p)))?;
                resultado.push(elegida);
                desde = hasta;
            }
            Some(resultado)
        }
        Segmentacion::MinimoDePiezas => {
            // mejor[i] = (piezas para cubrir los primeros i caracteres, dónde
            // empieza la última).
            let mut mejor: Vec<Option<(usize, usize)>> = vec![None; n + 1];
            mejor[0] = Some((0, 0));
            for hasta in 1..=n {
                for desde in 0..hasta {
                    let Some((coste, _)) = mejor[desde] else {
                        continue;
                    };
                    let mejora = mejor[hasta].is_none_or(|(actual, _)| coste + 1 < actual);
                    if mejora && pieza(desde, hasta).is_some() {
                        mejor[hasta] = Some((coste + 1, desde));
                    }
                }
            }
            mejor[n]?;
            let mut resultado = Vec::new();
            let mut fin = n;
            while fin > 0 {
                let (_, desde) = mejor[fin]?;
                resultado.push(pieza(desde, fin)?);
                fin = desde;
            }
            resultado.reverse();
            Some(resultado)
        }
    }
}

/// Convierte el glosario (términos separados por comas) en lo que recibe el motor.
pub fn preparar_terminos(
    glosario: &str,
    piezas: &HashSet<String>,
    segmentacion: Segmentacion,
) -> TerminosPreparados {
    let mut preparados = TerminosPreparados::default();
    let mut vistos = HashSet::new();
    for crudo in glosario.split([',', ';', '\n', '/']) {
        let termino = normalizar_termino(crudo);
        if termino.is_empty() {
            continue;
        }
        if termino.chars().count() < LONGITUD_MIN_TERMINO {
            preparados.rechazados.push((termino, MotivoRechazo::Corto));
            continue;
        }
        if !vistos.insert(termino.clone()) {
            preparados
                .rechazados
                .push((termino, MotivoRechazo::Repetido));
            continue;
        }
        // Con Bpe el motor tokeniza él, pero solo vale si cada carácter existe;
        // si no, "Encode hotwords failed" tira abajo la lista entera.
        let estrategia = match segmentacion {
            Segmentacion::Bpe => Segmentacion::PiezaMasLarga,
            otra => otra,
        };
        let por_palabra: Option<Vec<Vec<String>>> = termino
            .split(' ')
            .map(|palabra| segmentar_palabra(palabra, piezas, estrategia))
            .collect();
        match por_palabra {
            Some(_) if segmentacion == Segmentacion::Bpe => preparados.aceptados.push(termino),
            Some(piezas_por_palabra) => preparados
                .aceptados
                .push(piezas_por_palabra.concat().join(" ")),
            None => preparados
                .rechazados
                .push((termino, MotivoRechazo::NoSegmentable)),
        }
    }
    preparados
}

/// Carga una vez el modelo español y comparte sus pesos entre sesiones.
pub fn modelo_es_compartido() -> Result<Arc<SherpaModelo>> {
    let cache = MODELO_ES_CACHE.get_or_init(|| Mutex::new(None));
    let mut cache = cache
        .lock()
        .map_err(|e| SttError::Carga(format!("caché Sherpa bloqueada: {e}")))?;
    if let Some(modelo) = cache.as_ref() {
        return Ok(modelo.clone());
    }

    let ruta = modelo_es()?;
    let opciones = OpcionesSherpa::desde_entorno()?;
    let modelo = Arc::new(SherpaModelo::cargar(&ruta, &opciones)?);
    *cache = Some(modelo.clone());
    Ok(modelo)
}

/// Deja listos los pesos antes de empezar una sesión en español.
pub fn precalentar_modelo_es() -> Result<()> {
    modelo_es_compartido().map(|_| ())
}

impl SherpaVivo {
    /// Consume audio mono a 16 kHz y devuelve la hipotesis actual o la frase cerrada.
    pub fn aceptar(&mut self, pcm: &[f32]) -> Result<(String, bool)> {
        if pcm.is_empty() {
            return Ok((String::new(), false));
        }
        self.flujo.accept_waveform(16_000, pcm);
        self.decodificar();
        let cerrado = self.modelo.reconocedor.is_endpoint(&self.flujo);
        let texto = self
            .modelo
            .reconocedor
            .get_result(&self.flujo)
            .map(|r| r.text)
            .unwrap_or_default();
        if cerrado {
            self.modelo.reconocedor.reset(&self.flujo);
        }
        Ok((texto, cerrado))
    }

    /// Vacía el último tramo al cerrar la sesión.
    pub fn finalizar(&mut self) -> Result<String> {
        self.flujo.input_finished();
        self.decodificar();
        Ok(self
            .modelo
            .reconocedor
            .get_result(&self.flujo)
            .map(|r| r.text)
            .unwrap_or_default())
    }

    /// Descarta el contexto acústico después de un hueco en la captura.
    pub fn reiniciar(&mut self) {
        self.modelo.reconocedor.reset(&self.flujo);
    }

    fn decodificar(&self) {
        while self.modelo.reconocedor.is_ready(&self.flujo) {
            self.modelo.reconocedor.decode(&self.flujo);
        }
    }
}

/// Busca el paquete local de modelo español instalado junto a la aplicación.
pub fn modelo_es() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("DICTAR_SHERPA_MODEL_ES") {
        let path = PathBuf::from(path);
        if validar_modelo(&path) {
            return Ok(path);
        }
    }

    let mut rutas = Vec::new();
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        rutas.push(PathBuf::from(base).join("dictar_ia/models").join(MODELO_ES));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            rutas.push(dir.join("models").join(MODELO_ES));
        }
    }
    rutas
        .into_iter()
        .find(|p| validar_modelo(p))
        .ok_or_else(|| {
            SttError::ModeloNoEncontrado(format!(
                "no se encontró el modelo local de español {MODELO_ES}"
            ))
        })
}

fn validar_modelo(ruta: &Path) -> bool {
    ["encoder.onnx", "decoder.onnx", "joiner.onnx", "tokens.txt"]
        .iter()
        .all(|nombre| ruta.join(nombre).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piezas() -> HashSet<String> {
        // Un `tokens.txt` mínimo: letras sueltas, unas pocas piezas de palabra y `▁`.
        piezas_de_tokens(
            "<blk> 0\n▁ 1\np 2\nr 3\no 4\nt 5\nc 6\nl 7\n▁pro 8\nto 9\ncolo 10\n\
             co 11\nlo 12\ne 13\nd 14\n",
        )
    }

    #[test]
    fn el_nul_y_los_separadores_no_llegan_al_motor() {
        // Un `\0` dentro de un término hacía abortar el proceso en el
        // `CString::new(..).unwrap()` del enlace; `/` y `:` partían un término
        // en dos o le inventaban una puntuación.
        let limpio = normalizar_termino("Proto\0colo/de:red\n");
        assert!(!limpio.contains(['\0', '/', ':', '\n']));
        assert_eq!(limpio, "proto colo de red");
    }

    #[test]
    fn las_mayusculas_se_llevan_a_minusculas_como_emite_el_modelo() {
        assert_eq!(
            normalizar_termino("  Protocolo  DE   Red "),
            "protocolo de red"
        );
    }

    #[test]
    fn los_terminos_cortos_y_repetidos_se_cuentan_como_rechazados() {
        let preparados = preparar_terminos(
            "red, protocolo, Protocolo, ip",
            &piezas(),
            Segmentacion::PiezaMasLarga,
        );
        assert_eq!(preparados.aceptados.len(), 1);
        assert_eq!(
            preparados.rechazados,
            vec![
                ("red".to_owned(), MotivoRechazo::Corto),
                ("protocolo".to_owned(), MotivoRechazo::Repetido),
                ("ip".to_owned(), MotivoRechazo::Corto),
            ]
        );
    }

    #[test]
    fn un_caracter_ausente_de_tokens_rechaza_el_termino_en_vez_de_perderlo_en_silencio() {
        // El motor omite con un registro lo que no puede codificar; aquí se
        // cuenta para que el HANDOFF y el registro puedan decir cuántos fallaron.
        let preparados = preparar_terminos("protocolo, zzzzz", &piezas(), Segmentacion::Bpe);
        assert_eq!(preparados.aceptados, vec!["protocolo".to_owned()]);
        assert_eq!(
            preparados.rechazados,
            vec![("zzzzz".to_owned(), MotivoRechazo::NoSegmentable)]
        );
    }

    #[test]
    fn las_piezas_entregadas_al_motor_existen_en_tokens_y_reconstruyen_la_palabra() {
        for estrategia in [Segmentacion::PiezaMasLarga, Segmentacion::MinimoDePiezas] {
            let trozos = segmentar_palabra("protocolo", &piezas(), estrategia).unwrap();
            assert_eq!(trozos.concat().replace(INICIO_PALABRA, ""), "protocolo");
            assert!(trozos.iter().all(|t| piezas().contains(t)));
        }
    }

    #[test]
    fn el_minimo_de_piezas_usa_menos_piezas_que_la_voraz_cuando_esta_se_despista() {
        // Con «▁ab», «▁a» y «bcd», la voraz toma «▁ab» y debe terminar con «c» y
        // «d» sueltas; el mínimo encuentra ▁a + bcd.
        let piezas = piezas_de_tokens("▁ab 0\n▁a 1\nbcd 2\nc 3\nd 4\n");
        assert_eq!(
            segmentar_palabra("abcd", &piezas, Segmentacion::MinimoDePiezas),
            Some(vec!["▁a".to_owned(), "bcd".to_owned()])
        );
        assert_eq!(
            segmentar_palabra("abcd", &piezas, Segmentacion::PiezaMasLarga),
            Some(vec!["▁ab".to_owned(), "c".to_owned(), "d".to_owned()])
        );
    }

    #[test]
    fn sin_pieza_con_prefijo_se_antepone_el_prefijo_suelto() {
        let piezas = piezas_de_tokens("▁ 0\nx 1\ny 2\n");
        assert_eq!(
            segmentar_palabra("xy", &piezas, Segmentacion::PiezaMasLarga),
            Some(vec!["▁".to_owned(), "x".to_owned(), "y".to_owned()])
        );
        assert_eq!(
            segmentar_palabra("xz", &piezas, Segmentacion::PiezaMasLarga),
            None
        );
    }

    #[test]
    fn los_interruptores_de_entorno_se_interpretan_y_los_valores_raros_fallan() {
        let base = OpcionesSherpa::interpretar(None, None).unwrap();
        assert_eq!(base.haz, HAZ_POR_DEFECTO);
        assert_eq!(base.palabras_clave, PALABRAS_CLAVE_POR_DEFECTO);

        let con_haz = OpcionesSherpa::interpretar(Some("haz"), Some("1")).unwrap();
        assert!(con_haz.haz && con_haz.palabras_clave);

        assert!(OpcionesSherpa::interpretar(Some("beam"), None).is_err());
        assert!(OpcionesSherpa::interpretar(Some("haz"), Some("quizas")).is_err());
        // Pedir palabras clave con greedy las dejaría mudas sin aviso.
        assert!(OpcionesSherpa::interpretar(Some("greedy"), Some("1")).is_err());
    }
}
