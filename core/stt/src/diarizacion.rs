//! Diarización: separar quién habla en una pista de audio.
//!
//! Dos piezas con responsabilidades distintas:
//!
//! - [`Diarizador`] envuelve `OfflineSpeakerDiarization` de Sherpa-ONNX
//!   (segmentación pyannote 3.0 + embeddings de 3D-Speaker + agrupado). Solo
//!   existe de verdad en Windows, como el resto de Sherpa en este repositorio;
//!   en las demás plataformas conserva la firma y devuelve
//!   [`SttError::NoSoportada`], para que quien lo llama no escriba `#[cfg]`.
//! - [`hablante_de_frase`] y [`etiquetar_pista`] son funciones puras: deciden a
//!   qué clúster pertenece cada frase y cómo se llama. Viven aparte del motor
//!   para poder fijarlas con pruebas sin cargar ningún modelo.

use crate::{Result, SttError};
use dictar_domain::{Track, TsMs};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Sin solape, una frase solo hereda un clúster si está a menos de esto de su
/// segmento más cercano. Más lejos, lo más probable es que la diarización no
/// viera esa voz (una interjección de medio segundo), y adivinar le pondría
/// el nombre de otra persona a lo que dijo alguien.
pub const DISTANCIA_MAXIMA_SIN_SOLAPE_MS: TsMs = 1_000;

/// Umbral del agrupado jerárquico. Más bajo separa más voces (y sobresegmenta);
/// más alto las funde. Medido en A7 (DIA-1): entre 0,4 y 0,8 las dos voces de
/// la mezcla controlada se separan igual; la diferencia está en cuántos
/// clústeres espurios de ~1 s aparecen, y desde 0,7 casi no quedan.
pub const UMBRAL_AGRUPADO: f32 = 0.7;

/// Una voz con menos habla que esto en toda la pista no se trata como persona.
/// En las clases medidas aparecen clústeres de 0,7 a 1,1 s que son ruido o el
/// arranque de una palabra; convertirlos en «Participante 3» ensuciaría la
/// lista de participantes. Es un límite declarado: una intervención real de
/// menos de 3 s en toda la reunión se atribuye a la voz más cercana.
pub const HABLA_MINIMA_VOZ_MS: TsMs = 3_000;

/// Desplazamiento de la ventana de segmentación como fracción de su tamaño
/// (10 s). El valor por omisión de Sherpa es 0,1 (una ventana por segundo), que
/// es demasiado lento y separa peor. Medido en A7 de DIA-1, con 4 hilos:
/// - RTF 0,355 con 0,1 y 0,126 con 0,25, frente a 0,0725 con 0,5 en un clip de
///   158 s y 0,025 en una pista de 1 h (tope 0,1);
/// - mezcla controlada de dos profesores: con 0,1 la voz A quedaba en 67 %, con
///   0,25 en 46 % (sobresegmenta) y con 0,5 en 98 % / 91 % (A / B; mínimo 85 %).
pub const DESPLAZAMIENTO_VENTANA: f32 = 0.5;

/// Parámetros de carga del motor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpcionesDiarizacion {
    pub hilos: i32,
    pub umbral: f32,
    pub desplazamiento_ventana: f32,
}

impl OpcionesDiarizacion {
    /// La configuración de producción para este equipo.
    pub fn produccion() -> Self {
        let nucleos = std::thread::available_parallelism().map_or(2, |n| n.get());
        Self {
            hilos: hilos_para(nucleos),
            umbral: UMBRAL_AGRUPADO,
            desplazamiento_ventana: DESPLAZAMIENTO_VENTANA,
        }
    }
}

/// Nombres de los dos archivos que debe haber en la carpeta de modelos.
pub const MODELO_SEGMENTACION: &str = "pyannote-segmentation-3-0.onnx";
pub const MODELO_EMBEDDING: &str = "3dspeaker-campplus-sv-zh-cn-16k.onnx";
/// Subcarpeta de `models/` donde viven, tanto en el instalador como en los datos
/// del usuario.
pub const CARPETA_MODELOS: &str = "diarizacion";
/// Para el arnés de evaluación y para el soporte: apunta a una carpeta con los
/// dos archivos y manda sobre las rutas habituales.
pub const VAR_MODELOS: &str = "DICTAR_DIARIZACION_MODELOS";

/// Un tramo en el que habla un clúster, en milisegundos de la pista.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentoHablante {
    pub inicio_ms: TsMs,
    pub fin_ms: TsMs,
    /// Número de clúster dentro de la pista. No es comparable entre pistas:
    /// el clúster 0 del micrófono y el del sistema no tienen nada que ver.
    pub hablante: usize,
}

impl SegmentoHablante {
    fn duracion_ms(&self) -> TsMs {
        (self.fin_ms - self.inicio_ms).max(0)
    }
}

/// Dónde están los dos modelos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RutasModelos {
    pub segmentacion: PathBuf,
    pub embedding: PathBuf,
}

impl RutasModelos {
    fn en_carpeta(dir: &std::path::Path) -> Option<Self> {
        let rutas = Self {
            segmentacion: dir.join(MODELO_SEGMENTACION),
            embedding: dir.join(MODELO_EMBEDDING),
        };
        (rutas.segmentacion.is_file() && rutas.embedding.is_file()).then_some(rutas)
    }
}

/// Busca los modelos de diarización. `None` si no están: la sesión sigue sin
/// ellos, y quien llama decide cómo decirlo.
///
/// Se comprueba que los archivos existen antes de entregarlos al motor porque
/// Sherpa, ante una ruta que no abre, puede terminar el proceso entero en
/// lugar de devolver un error.
pub fn rutas_modelos() -> Option<RutasModelos> {
    let mut carpetas = Vec::new();
    if let Some(dir) = std::env::var_os(VAR_MODELOS) {
        carpetas.push(PathBuf::from(dir));
    }
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        carpetas.push(
            PathBuf::from(base)
                .join("dictar_ia/models")
                .join(CARPETA_MODELOS),
        );
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            carpetas.push(dir.join("models").join(CARPETA_MODELOS));
        }
    }
    carpetas.iter().find_map(|c| RutasModelos::en_carpeta(c))
}

/// Hilos para cada modelo: la mitad de los núcleos lógicos con tope en 4. Es un
/// trabajo de fondo mientras el usuario sigue usando el equipo; la diarización
/// no necesita más para ir muy por debajo del tiempo real.
pub fn hilos_para(nucleos_logicos: usize) -> i32 {
    (nucleos_logicos / 2).clamp(1, 4) as i32
}

/// Motor de diarización cargado.
pub struct Diarizador {
    #[cfg(target_os = "windows")]
    motor: sherpa_onnx::OfflineSpeakerDiarization,
}

#[cfg(target_os = "windows")]
impl Diarizador {
    /// Carga los dos modelos. Costoso (cientos de ms); se hace una vez por sesión.
    pub fn cargar(rutas: &RutasModelos, opciones: &OpcionesDiarizacion) -> Result<Self> {
        use sherpa_onnx::{
            FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
            OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
            SpeakerEmbeddingExtractorConfig,
        };

        for ruta in [&rutas.segmentacion, &rutas.embedding] {
            if !ruta.is_file() {
                return Err(SttError::ModeloNoEncontrado(ruta.display().to_string()));
            }
        }

        let config = OfflineSpeakerDiarizationConfig {
            segmentation: OfflineSpeakerSegmentationModelConfig {
                pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: Some(rutas.segmentacion.display().to_string()),
                    window_shift_ratio: opciones.desplazamiento_ventana,
                },
                num_threads: opciones.hilos,
                ..Default::default()
            },
            embedding: SpeakerEmbeddingExtractorConfig {
                model: Some(rutas.embedding.display().to_string()),
                num_threads: opciones.hilos,
                ..Default::default()
            },
            // -1: el número de voces lo decide el umbral, no se conoce de antemano.
            clustering: FastClusteringConfig {
                num_clusters: -1,
                threshold: opciones.umbral,
                ..Default::default()
            },
            ..Default::default()
        };
        let motor = OfflineSpeakerDiarization::create(&config).ok_or_else(|| {
            SttError::Carga(format!(
                "Sherpa-ONNX no pudo abrir los modelos de diarización en {}",
                rutas.segmentacion.display()
            ))
        })?;
        Ok(Self { motor })
    }

    /// Frecuencia que espera el motor. El pipeline entero va a 16 kHz.
    pub fn frecuencia(&self) -> i32 {
        self.motor.sample_rate()
    }

    /// Diariza una pista mono a 16 kHz. Tarda lo que tarde: la API no se puede
    /// cancelar.
    pub fn diarizar(&self, pcm: &[f32]) -> Result<Vec<SegmentoHablante>> {
        // `leer_wav` ya entrega 16 kHz. Si algún día cambiara, es mejor un
        // error claro que unos tiempos desplazados sin que nadie se entere.
        if self.frecuencia() != 16_000 {
            return Err(SttError::Transcripcion(format!(
                "el diarizador espera {} Hz y el audio va a 16000 Hz",
                self.frecuencia()
            )));
        }
        if pcm.is_empty() {
            return Ok(Vec::new());
        }
        let resultado = self.motor.process(pcm).ok_or_else(|| {
            SttError::Transcripcion("la diarización no devolvió resultado".to_owned())
        })?;
        Ok(resultado
            .sort_by_start_time()
            .iter()
            .map(|s| SegmentoHablante {
                inicio_ms: (s.start * 1000.0).round() as TsMs,
                fin_ms: (s.end * 1000.0).round() as TsMs,
                hablante: s.speaker.max(0) as usize,
            })
            .collect())
    }
}

#[cfg(not(target_os = "windows"))]
impl Diarizador {
    pub fn cargar(_rutas: &RutasModelos, _opciones: &OpcionesDiarizacion) -> Result<Self> {
        Err(SttError::NoSoportada(
            "la diarización solo está disponible en Windows".to_owned(),
        ))
    }

    pub fn frecuencia(&self) -> i32 {
        16_000
    }

    pub fn diarizar(&self, _pcm: &[f32]) -> Result<Vec<SegmentoHablante>> {
        Err(SttError::NoSoportada(
            "la diarización solo está disponible en Windows".to_owned(),
        ))
    }
}

/// A qué clúster pertenece una frase, o `None` si no se puede decir.
///
/// - Gana el clúster con **más solape** con la frase.
/// - Empate exacto: el clúster con más tiempo total en la pista; si también
///   empatan, el de número menor, para que el resultado no dependa del orden.
/// - Sin solape: el del segmento más cercano si está a no más de
///   [`DISTANCIA_MAXIMA_SIN_SOLAPE_MS`]; si no, `None` y quien llama usa la
///   etiqueta por defecto del canal.
pub fn hablante_de_frase(
    inicio_ms: TsMs,
    fin_ms: TsMs,
    segmentos: &[SegmentoHablante],
) -> Option<usize> {
    let mut solape: BTreeMap<usize, TsMs> = BTreeMap::new();
    for s in segmentos {
        let comun = fin_ms.min(s.fin_ms) - inicio_ms.max(s.inicio_ms);
        if comun > 0 {
            *solape.entry(s.hablante).or_default() += comun;
        }
    }
    if !solape.is_empty() {
        let maximo = solape.values().copied().max().unwrap_or(0);
        let tiempo_total = tiempo_por_hablante(segmentos);
        return solape
            .iter()
            .filter(|(_, v)| **v == maximo)
            // `max_by_key` se queda con el último de los empatados, y el
            // `BTreeMap` ordena por número: invertimos la clave secundaria
            // para que gane el menor.
            .max_by_key(|(h, _)| (tiempo_total[*h], std::cmp::Reverse(**h)))
            .map(|(h, _)| *h);
    }

    segmentos
        .iter()
        .map(|s| {
            let distancia = if s.fin_ms <= inicio_ms {
                inicio_ms - s.fin_ms
            } else {
                s.inicio_ms - fin_ms
            };
            (distancia, s.hablante)
        })
        .filter(|(d, _)| *d <= DISTANCIA_MAXIMA_SIN_SOLAPE_MS)
        .min()
        .map(|(_, h)| h)
}

/// Quita los segmentos de las voces con menos de [`HABLA_MINIMA_VOZ_MS`] de habla.
pub fn quitar_voces_residuales(segmentos: &[SegmentoHablante]) -> Vec<SegmentoHablante> {
    let tiempo = tiempo_por_hablante(segmentos);
    segmentos
        .iter()
        .filter(|s| tiempo[&s.hablante] >= HABLA_MINIMA_VOZ_MS)
        .copied()
        .collect()
}

fn tiempo_por_hablante(segmentos: &[SegmentoHablante]) -> BTreeMap<usize, TsMs> {
    let mut total = BTreeMap::new();
    for s in segmentos {
        *total.entry(s.hablante).or_default() += s.duracion_ms();
    }
    total
}

/// Pone nombre a los clústeres que sí acabaron con frases.
///
/// `usados` son los clústeres con al menos una frase; el resto (eco residual,
/// ruido) no aparecen como participantes. `por_defecto` es la etiqueta que ya
/// llevaba el canal: «Yo» en el micrófono, el nombre del interlocutor en el
/// sistema.
///
/// - **Una sola voz:** conserva `por_defecto`, como antes de la diarización.
/// - **Micrófono con varias:** «Yo» para la que más habla y «Persona en sala N»
///   para las demás, numeradas por orden de aparición.
/// - **Sistema con varias:** «Participante N», por orden de aparición.
pub fn etiquetar_pista(
    pista: Track,
    segmentos: &[SegmentoHablante],
    usados: &BTreeSet<usize>,
    por_defecto: &str,
) -> BTreeMap<usize, String> {
    let mut etiquetas = BTreeMap::new();
    if usados.len() <= 1 {
        for h in usados {
            etiquetas.insert(*h, por_defecto.to_owned());
        }
        return etiquetas;
    }

    let tiempo = tiempo_por_hablante(segmentos);
    let mut por_aparicion: Vec<usize> = Vec::new();
    for s in segmentos {
        if usados.contains(&s.hablante) && !por_aparicion.contains(&s.hablante) {
            por_aparicion.push(s.hablante);
        }
    }
    // Un clúster usado siempre tiene segmentos, pero si alguien llamara con
    // `usados` ajenos a `segmentos` no deben perderse en silencio.
    for h in usados {
        if !por_aparicion.contains(h) {
            por_aparicion.push(*h);
        }
    }

    match pista {
        Track::Mic => {
            let yo = *usados
                .iter()
                .max_by_key(|h| (tiempo.get(*h).copied().unwrap_or(0), std::cmp::Reverse(**h)))
                .expect("hay al menos dos clústeres");
            etiquetas.insert(yo, "Yo".to_owned());
            for (i, h) in por_aparicion.into_iter().filter(|h| *h != yo).enumerate() {
                etiquetas.insert(h, format!("Persona en sala {}", i + 1));
            }
        }
        Track::System => {
            for (i, h) in por_aparicion.into_iter().enumerate() {
                etiquetas.insert(h, format!("Participante {}", i + 1));
            }
        }
    }
    etiquetas
}

/// Resultado de etiquetar todas las frases de una pista.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EtiquetadoPista {
    /// Una etiqueta por frase, en el orden en que se entregaron.
    pub etiquetas: Vec<String>,
    /// Clústeres que la diarización detectó pero a los que no llegó ninguna
    /// frase: no son participantes.
    pub descartados: Vec<usize>,
    /// Voces distintas que sí tienen frases.
    pub voces: usize,
}

/// Decide la etiqueta de cada frase de una pista: la asignación a un clúster
/// ([`hablante_de_frase`]) y el nombre de cada clúster ([`etiquetar_pista`]).
///
/// `tramos` son el `(inicio_ms, fin_ms)` de cada frase. Una frase sin clúster
/// se queda con `por_defecto`. Antes de asignar se descartan las voces con
/// habla residual ([`quitar_voces_residuales`]); aparecen entonces en
/// `descartados`.
pub fn etiquetar_frases(
    pista: Track,
    tramos: &[(TsMs, TsMs)],
    segmentos: &[SegmentoHablante],
    por_defecto: &str,
) -> EtiquetadoPista {
    let todos = segmentos;
    let segmentos = &quitar_voces_residuales(todos);
    let asignadas: Vec<Option<usize>> = tramos
        .iter()
        .map(|(inicio, fin)| hablante_de_frase(*inicio, *fin, segmentos))
        .collect();
    let usados: BTreeSet<usize> = asignadas.iter().flatten().copied().collect();
    let nombres = etiquetar_pista(pista, segmentos, &usados, por_defecto);
    let descartados: BTreeSet<usize> = todos
        .iter()
        .map(|s| s.hablante)
        .filter(|h| !usados.contains(h))
        .collect();
    EtiquetadoPista {
        etiquetas: asignadas
            .iter()
            .map(|h| {
                h.and_then(|h| nombres.get(&h).cloned())
                    .unwrap_or_else(|| por_defecto.to_owned())
            })
            .collect(),
        descartados: descartados.into_iter().collect(),
        voces: usados.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(inicio_ms: TsMs, fin_ms: TsMs, hablante: usize) -> SegmentoHablante {
        SegmentoHablante {
            inicio_ms,
            fin_ms,
            hablante,
        }
    }

    #[test]
    fn una_frase_partida_entre_dos_voces_gana_la_de_mayor_solape() {
        // Frase de 0 a 10 s; la voz 1 cubre 3 s y la voz 0 cubre 7 s. Antes de
        // fijar la regla, el criterio era «el segmento donde empieza la
        // frase», que habría dado la voz 1.
        let segmentos = [seg(0, 7_000, 0), seg(7_000, 10_000, 1)];
        assert_eq!(hablante_de_frase(0, 10_000, &segmentos), Some(0));

        let invertidos = [seg(0, 3_000, 1), seg(3_000, 10_000, 0)];
        assert_eq!(hablante_de_frase(0, 10_000, &invertidos), Some(0));
    }

    #[test]
    fn el_solape_de_un_mismo_cluster_se_suma_aunque_este_troceado() {
        // La voz 1 habla en dos tramos de 2 s (4 s en total) frente a un único
        // tramo de 3 s de la voz 0.
        let segmentos = [seg(0, 2_000, 1), seg(2_000, 5_000, 0), seg(5_000, 7_000, 1)];
        assert_eq!(hablante_de_frase(0, 7_000, &segmentos), Some(1));
    }

    #[test]
    fn sin_solape_y_a_menos_de_un_segundo_se_hereda_el_cluster_mas_cercano() {
        // La frase es de 5 a 6 s. La voz 0 acaba 0,6 s antes y la voz 1
        // empieza 0,9 s después: gana la más cercana, la 0.
        let segmentos = [seg(0, 4_400, 0), seg(6_900, 9_000, 1)];
        assert_eq!(hablante_de_frase(5_000, 6_000, &segmentos), Some(0));
    }

    #[test]
    fn sin_solape_y_a_mas_de_un_segundo_no_se_adivina() {
        // La voz 0 acaba 1,5 s antes de la frase: se queda con la etiqueta
        // por defecto del canal en vez de atribuirle algo a alguien.
        let segmentos = [seg(0, 3_500, 0)];
        assert_eq!(hablante_de_frase(5_000, 6_000, &segmentos), None);
        // Y el límite es inclusivo: a exactamente 1 s todavía se hereda.
        let justo = [seg(0, 4_000, 0)];
        assert_eq!(hablante_de_frase(5_000, 6_000, &justo), Some(0));
    }

    #[test]
    fn en_un_empate_exacto_gana_quien_mas_habla_en_la_pista() {
        // Frase de 0 a 4 s: 2 s de la voz 3 y 2 s de la voz 5. Fuera de la
        // frase la voz 5 habla 10 s más, así que gana la 5, aunque el número
        // de la 3 sea menor.
        let segmentos = [
            seg(0, 2_000, 3),
            seg(2_000, 4_000, 5),
            seg(20_000, 30_000, 5),
        ];
        assert_eq!(hablante_de_frase(0, 4_000, &segmentos), Some(5));
    }

    #[test]
    fn si_el_empate_persiste_gana_el_numero_menor_siempre() {
        let segmentos = [seg(0, 2_000, 4), seg(2_000, 4_000, 2)];
        assert_eq!(hablante_de_frase(0, 4_000, &segmentos), Some(2));
        let al_reves = [seg(2_000, 4_000, 2), seg(0, 2_000, 4)];
        assert_eq!(hablante_de_frase(0, 4_000, &al_reves), Some(2));
    }

    #[test]
    fn sin_ningun_segmento_no_hay_hablante() {
        assert_eq!(hablante_de_frase(0, 1_000, &[]), None);
    }

    #[test]
    fn una_voz_por_canal_conserva_la_etiqueta_de_siempre() {
        let segmentos = [seg(0, 5_000, 0)];
        let usados = BTreeSet::from([0]);
        let mic = etiquetar_pista(Track::Mic, &segmentos, &usados, "Yo");
        assert_eq!(mic[&0], "Yo");
        let sistema = etiquetar_pista(Track::System, &segmentos, &usados, "Cliente");
        assert_eq!(sistema[&0], "Cliente");
    }

    #[test]
    fn varias_voces_en_el_sistema_son_participantes_por_orden_de_aparicion() {
        // La voz 2 aparece primero aunque su número sea mayor, y la 0, que
        // habla más, aparece después: la numeración sigue la aparición.
        let segmentos = [
            seg(0, 1_000, 2),
            seg(1_000, 9_000, 0),
            seg(9_000, 10_000, 1),
        ];
        let usados = BTreeSet::from([0, 1, 2]);
        let e = etiquetar_pista(Track::System, &segmentos, &usados, "Cliente");
        assert_eq!(e[&2], "Participante 1");
        assert_eq!(e[&0], "Participante 2");
        assert_eq!(e[&1], "Participante 3");
    }

    #[test]
    fn en_el_microfono_yo_es_quien_mas_habla_y_el_resto_son_personas_en_sala() {
        // La voz 7 habla 1 s, la 4 habla 8 s (es «Yo») y la 9 habla 2 s. Las
        // dos restantes se numeran por orden de aparición: 7 antes que 9.
        let segmentos = [
            seg(0, 1_000, 7),
            seg(1_000, 9_000, 4),
            seg(9_000, 11_000, 9),
        ];
        let usados = BTreeSet::from([4, 7, 9]);
        let e = etiquetar_pista(Track::Mic, &segmentos, &usados, "Yo");
        assert_eq!(e[&4], "Yo");
        assert_eq!(e[&7], "Persona en sala 1");
        assert_eq!(e[&9], "Persona en sala 2");
    }

    #[test]
    fn un_cluster_sin_frases_no_recibe_etiqueta_ni_cuenta_como_participante() {
        // Eco residual: la voz 1 existe en la diarización pero ninguna frase
        // acabó con ella. Si contara, «Yo» pasaría a ser «Persona en sala»
        // en una reunión con una sola voz real.
        let segmentos = [seg(0, 9_000, 0), seg(9_000, 9_500, 1)];
        let usados = BTreeSet::from([0]);
        let e = etiquetar_pista(Track::Mic, &segmentos, &usados, "Yo");
        assert_eq!(e.len(), 1);
        assert_eq!(e[&0], "Yo");
    }

    #[test]
    fn etiquetar_frases_une_la_asignacion_y_los_nombres_y_descarta_lo_sobrante() {
        // Sistema con tres clústeres: la voz 0 dice dos frases, la 1 una, y la
        // 2 (un ruido de 0,4 s en medio de un silencio) no recibe ninguna.
        let segmentos = [
            seg(0, 4_000, 0),
            seg(4_000, 8_000, 1),
            seg(8_000, 12_000, 0),
            seg(30_000, 30_400, 2),
        ];
        let tramos = [(500, 3_500), (4_500, 7_500), (8_500, 11_500)];
        let r = etiquetar_frases(Track::System, &tramos, &segmentos, "Cliente");
        assert_eq!(
            r.etiquetas,
            ["Participante 1", "Participante 2", "Participante 1"]
        );
        assert_eq!(r.descartados, [2]);
        assert_eq!(r.voces, 2);
    }

    #[test]
    fn una_frase_sin_cluster_conserva_la_etiqueta_del_canal() {
        // Una interjección a 3 s de cualquier voz: no se le inventa dueño.
        let segmentos = [seg(0, 4_000, 0), seg(4_000, 8_000, 1)];
        let tramos = [(1_000, 3_000), (5_000, 7_000), (11_500, 12_000)];
        let r = etiquetar_frases(Track::System, &tramos, &segmentos, "Cliente");
        assert_eq!(r.etiquetas, ["Participante 1", "Participante 2", "Cliente"]);
    }

    #[test]
    fn con_una_sola_voz_las_frases_siguen_llamandose_como_antes() {
        let segmentos = [seg(0, 10_000, 0)];
        let tramos = [(0, 2_000), (3_000, 5_000)];
        let r = etiquetar_frases(Track::Mic, &tramos, &segmentos, "Yo");
        assert_eq!(r.etiquetas, ["Yo", "Yo"]);
        assert_eq!(r.voces, 1);
    }

    #[test]
    fn una_voz_residual_se_descarta_y_su_frase_pasa_a_la_voz_vecina() {
        // La voz 1 solo suena 1,2 s en toda la pista (un ruido junto a la voz
        // 0). Antes de descartar las voces residuales, esa frase de 1 s
        // acababa en «Participante 2» y la lista de participantes crecía con
        // alguien que no existe.
        let segmentos = [
            seg(0, 10_000, 0),
            seg(10_000, 11_200, 1),
            seg(11_200, 20_000, 0),
        ];
        let tramos = [(1_000, 5_000), (10_100, 11_100)];
        let r = etiquetar_frases(Track::System, &tramos, &segmentos, "Cliente");
        assert_eq!(r.etiquetas, ["Cliente", "Cliente"]);
        assert_eq!(r.voces, 1);
        assert_eq!(r.descartados, [1]);
    }

    #[test]
    fn quitar_voces_residuales_conserva_solo_las_que_hablan_el_minimo() {
        let segmentos = [
            seg(0, 2_000, 4),
            seg(5_000, 6_000, 4),
            seg(6_000, 6_500, 9),
            seg(7_000, 12_000, 2),
        ];
        // La 4 suma justo 3 s (se queda); la 9 suma 0,5 s (se va).
        let quedan = quitar_voces_residuales(&segmentos);
        assert_eq!(quedan, [segmentos[0], segmentos[1], segmentos[3]]);
    }

    #[test]
    fn los_hilos_son_la_mitad_de_los_nucleos_con_tope_de_cuatro() {
        assert_eq!(hilos_para(1), 1);
        assert_eq!(hilos_para(2), 1);
        assert_eq!(hilos_para(4), 2);
        assert_eq!(hilos_para(8), 4);
        assert_eq!(hilos_para(32), 4);
    }

    #[test]
    fn sin_la_carpeta_de_modelos_no_hay_rutas() {
        let vacia = tempfile::tempdir().unwrap();
        assert!(RutasModelos::en_carpeta(vacia.path()).is_none());

        // Con un solo modelo tampoco: el motor necesita los dos.
        std::fs::write(vacia.path().join(MODELO_SEGMENTACION), b"x").unwrap();
        assert!(RutasModelos::en_carpeta(vacia.path()).is_none());

        std::fs::write(vacia.path().join(MODELO_EMBEDDING), b"x").unwrap();
        let rutas = RutasModelos::en_carpeta(vacia.path()).unwrap();
        assert!(rutas.embedding.ends_with(MODELO_EMBEDDING));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn fuera_de_windows_cargar_devuelve_no_soportada_en_vez_de_desaparecer() {
        let rutas = RutasModelos {
            segmentacion: PathBuf::from("a"),
            embedding: PathBuf::from("b"),
        };
        let r = Diarizador::cargar(&rutas, &OpcionesDiarizacion::produccion());
        assert!(matches!(r, Err(SttError::NoSoportada(_))));
    }
}
