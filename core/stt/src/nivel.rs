//! Control de ganancia para el audio que llega al reconocedor en vivo.
//!
//! Las clases medidas el 08/10 van de un RMS de 0,039 a 0,19 según la sesión y
//! la pista. Una ganancia *g* suma `2·ln g` a cada coeficiente log-mel, así que
//! el modelo ve entradas distintas solo por el volumen de la sala. Este módulo
//! acerca el nivel a un objetivo fijo, siempre sobre una **copia**: el WAV que
//! se guarda a disco no pasa por aquí.
//!
//! Es puro y no conoce ningún motor. Sus decisiones dependen solo de las
//! muestras que ha visto, no de cómo llegaron troceadas.

/// Muestras por ventana de análisis: 30 ms a 16 kHz.
pub const VENTANA_MUESTRAS: usize = 480;
const FRECUENCIA_HZ: f32 = 16_000.0;
const VENTANA_S: f32 = VENTANA_MUESTRAS as f32 / FRECUENCIA_HZ;

/// Nivel RMS al que se quiere llevar el habla, en dBFS.
pub const OBJETIVO_DBFS: f32 = -20.0;
pub const GANANCIA_MIN_DB: f32 = -6.0;
pub const GANANCIA_MAX_DB: f32 = 20.0;
/// La ganancia sube despacio para no inflar el ruido de fondo en cada pausa…
pub const SUBIDA_MAX_DB_POR_S: f32 = 3.0;
/// …y baja deprisa para que una voz fuerte no sature al reconocedor.
pub const BAJADA_MAX_DB_POR_S: f32 = 20.0;
/// Por debajo de este RMS la ventana es silencio o ruido y no cuenta.
pub const UMBRAL_ACTIVA: f32 = 0.003;

/// Hasta aquí el limitador es lineal.
const RODILLA: f32 = 0.9;
/// Nada de lo que sale del limitador la supera.
pub const TECHO: f32 = 0.98;

/// Control de ganancia con estado entre bloques.
///
/// Los bloques reales de captura son de 1024 muestras y no se alinean con las
/// ventanas de 480, así que la ventana en curso se arrastra de una llamada a
/// otra. Por eso partir el mismo audio de otra forma da la misma salida.
#[derive(Debug, Clone)]
pub struct Nivelador {
    /// Ganancia (dB) en la primera muestra de la ventana en curso.
    inicio_db: f32,
    /// Ganancia (dB) al final de la ventana en curso. Se decide con la ventana
    /// anterior, y por eso la salida es causal y continua.
    fin_db: f32,
    muestras_en_ventana: usize,
    energia_ventana: f64,
}

impl Default for Nivelador {
    fn default() -> Self {
        Self::nuevo()
    }
}

impl Nivelador {
    pub fn nuevo() -> Self {
        Self {
            inicio_db: 0.0,
            fin_db: 0.0,
            muestras_en_ventana: 0,
            energia_ventana: 0.0,
        }
    }

    /// Ganancia que se está aplicando ahora mismo, en dB.
    pub fn ganancia_db(&self) -> f32 {
        let avance = self.muestras_en_ventana as f32 / VENTANA_MUESTRAS as f32;
        self.inicio_db + (self.fin_db - self.inicio_db) * avance
    }

    /// Nivela `pcm` en el sitio, conservando el estado para la siguiente llamada.
    pub fn procesar(&mut self, pcm: &mut [f32]) {
        for muestra in pcm.iter_mut() {
            let entrada = *muestra;
            self.energia_ventana += f64::from(entrada) * f64::from(entrada);
            self.muestras_en_ventana += 1;

            let avance = self.muestras_en_ventana as f32 / VENTANA_MUESTRAS as f32;
            let db = self.inicio_db + (self.fin_db - self.inicio_db) * avance;
            *muestra = limitar(entrada * 10f32.powf(db / 20.0));

            if self.muestras_en_ventana == VENTANA_MUESTRAS {
                self.cerrar_ventana();
            }
        }
    }

    fn cerrar_ventana(&mut self) {
        let rms = (self.energia_ventana / VENTANA_MUESTRAS as f64).sqrt() as f32;
        self.energia_ventana = 0.0;
        self.muestras_en_ventana = 0;
        self.inicio_db = self.fin_db;

        // El silencio no mueve la ganancia: si la subiera, cada pausa de la
        // clase inflaría el ruido de la sala hasta el objetivo.
        if rms <= UMBRAL_ACTIVA {
            return;
        }
        let nivel_db = 20.0 * rms.log10();
        let objetivo_db = (OBJETIVO_DBFS - nivel_db).clamp(GANANCIA_MIN_DB, GANANCIA_MAX_DB);
        let paso = (objetivo_db - self.fin_db).clamp(
            -BAJADA_MAX_DB_POR_S * VENTANA_S,
            SUBIDA_MAX_DB_POR_S * VENTANA_S,
        );
        self.fin_db = (self.fin_db + paso).clamp(GANANCIA_MIN_DB, GANANCIA_MAX_DB);
    }
}

/// Limitador suave: lineal hasta la rodilla y compresión continua por encima.
///
/// Un recorte duro metería armónicos justo en los picos de voz fuerte, que es
/// donde el reconocedor menos lo soporta. La curva tiene pendiente 1 en la
/// rodilla, así que no hay codo audible, y tiende al techo sin tocarlo.
pub fn limitar(x: f32) -> f32 {
    let magnitud = x.abs();
    if magnitud <= RODILLA {
        return x;
    }
    let margen = TECHO - RODILLA;
    let comprimida = RODILLA + margen * ((magnitud - RODILLA) / margen).tanh();
    comprimida.min(TECHO).copysign(x)
}

/// Lee un interruptor `0|1` de evaluación (D6 de AV-3).
///
/// Un valor desconocido es un error y no el valor por defecto: una medición
/// hecha con `DICTAR_STT_NIVELAR=si` creyendo que nivelaba, y sin nivelar,
/// sería peor que no tener medición.
pub fn interpretar_interruptor(
    nombre: &str,
    valor: Option<&str>,
    defecto: bool,
) -> Result<bool, String> {
    match valor {
        None => Ok(defecto),
        Some("1") => Ok(true),
        Some("0") => Ok(false),
        Some(otro) => Err(format!("{nombre}={otro:?} no vale; use 0 o 1")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tono(amplitud: f32, muestras: usize) -> Vec<f32> {
        (0..muestras)
            .map(|i| amplitud * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 16_000.0).sin())
            .collect()
    }

    fn dbfs_a_amplitud_pico(dbfs_rms: f32) -> f32 {
        // El RMS de un seno es el pico entre raíz de dos.
        10f32.powf(dbfs_rms / 20.0) * 2f32.sqrt()
    }

    #[test]
    fn un_tono_a_menos_cuarenta_dbfs_sube_sin_superar_el_limite_de_subida() {
        // Antes de fijar la pendiente, la ganancia saltaba al objetivo de una
        // ventana a otra y el reconocedor veía un escalón de volumen.
        let mut nivelador = Nivelador::nuevo();
        let amplitud = dbfs_a_amplitud_pico(-40.0);
        let segundos = 4;
        for s in 1..=segundos {
            let mut bloque = tono(amplitud, 16_000);
            nivelador.procesar(&mut bloque);
            let limite = SUBIDA_MAX_DB_POR_S * s as f32 + 0.01;
            assert!(
                nivelador.ganancia_db() <= limite,
                "tras {s} s la ganancia es {} dB y el límite {limite}",
                nivelador.ganancia_db()
            );
        }
        assert!(nivelador.ganancia_db() > 5.0, "tras 4 s debía haber subido");
    }

    #[test]
    fn el_silencio_no_sube_la_ganancia() {
        let mut nivelador = Nivelador::nuevo();
        // Una voz fuerte durante un rato largo asienta la ganancia por debajo de
        // cero, en un múltiplo de ventana para que no quede una rampa a medias…
        let mut fuerte = tono(0.9, VENTANA_MUESTRAS * 100);
        nivelador.procesar(&mut fuerte);
        let antes = nivelador.ganancia_db();
        assert!(antes < 0.0);
        // …y diez segundos de ruido muy bajo no pueden devolverla hacia arriba.
        let mut silencio = tono(0.001, VENTANA_MUESTRAS * 300);
        nivelador.procesar(&mut silencio);
        assert_eq!(nivelador.ganancia_db(), antes);
    }

    #[test]
    fn la_salida_nunca_pasa_de_098_ni_salta_de_una_muestra_a_otra() {
        let mut nivelador = Nivelador::nuevo();
        let mut pcm = tono(1.0, 48_000);
        nivelador.procesar(&mut pcm);
        assert!(
            pcm.iter().all(|m| m.abs() <= TECHO),
            "hay muestras por encima del techo"
        );
        let salto_max = pcm
            .windows(2)
            .map(|p| (p[1] - p[0]).abs())
            .fold(0.0f32, f32::max);
        // Un seno de 220 Hz a amplitud 1 avanza como mucho ~0,087 por muestra.
        // La ganancia baja 6 dB en los primeros 0,3 s; si lo hiciera a saltos de
        // ventana, el escalón se sumaría a eso y superaría el umbral.
        assert!(
            salto_max < 0.1,
            "salto de {salto_max} entre muestras contiguas"
        );
    }

    #[test]
    fn el_limitador_es_lineal_hasta_la_rodilla_y_monotono_despues() {
        assert_eq!(limitar(0.5), 0.5);
        assert_eq!(limitar(-0.9), -0.9);
        let mut anterior = limitar(0.9);
        for i in 1..400 {
            let x = 0.9 + i as f32 * 0.01;
            let y = limitar(x);
            assert!(y >= anterior && y <= TECHO);
            anterior = y;
        }
        assert_eq!(limitar(-50.0), -TECHO);
    }

    #[test]
    fn el_rango_de_menos_seis_a_mas_veinte_db_se_respeta() {
        let mut bajo = Nivelador::nuevo();
        // Un tono casi inaudible pero activo empuja hacia el máximo.
        for _ in 0..30 {
            let mut bloque = tono(0.005, 16_000);
            bajo.procesar(&mut bloque);
        }
        assert!(bajo.ganancia_db() <= GANANCIA_MAX_DB + 1e-3);
        assert!(bajo.ganancia_db() > GANANCIA_MAX_DB - 0.5);

        let mut alto = Nivelador::nuevo();
        for _ in 0..5 {
            let mut bloque = tono(1.0, 16_000);
            alto.procesar(&mut bloque);
        }
        assert!(alto.ganancia_db() >= GANANCIA_MIN_DB - 1e-3);
        assert!(alto.ganancia_db() < GANANCIA_MIN_DB + 0.5);
    }

    #[test]
    fn bloques_de_1024_dan_lo_mismo_que_un_unico_bloque() {
        // Los bloques de captura no se alinean con las ventanas de 480: si el
        // análisis reiniciara la ventana en cada llamada, esto divergiría.
        let mut origen = tono(0.02, 8_000);
        origen.extend(tono(0.4, 8_000));
        origen.extend(tono(0.05, 8_000));

        let mut entero = origen.clone();
        Nivelador::nuevo().procesar(&mut entero);

        let mut troceado = origen.clone();
        let mut nivelador = Nivelador::nuevo();
        for trozo in troceado.chunks_mut(1024) {
            nivelador.procesar(trozo);
        }
        assert_eq!(entero, troceado);
    }

    #[test]
    fn el_estado_sobrevive_a_audio_no_contiguo_sin_volver_a_cero_db() {
        // Un hueco de captura reinicia al reconocedor, no a la ganancia: volver
        // a 0 dB haría que el modelo viera un salto de volumen justo después de
        // cada hueco. Que `bucle` no recree el nivelador no se puede probar
        // desde aquí; esto fija que el propio estado no se pierde entre
        // llamadas aunque el audio que sigue no continúe al anterior.
        let mut nivelador = Nivelador::nuevo();
        let mut fuerte = tono(0.9, 16_000);
        nivelador.procesar(&mut fuerte);
        let antes = nivelador.ganancia_db();
        let mut despues = tono(0.9, 100);
        nivelador.procesar(&mut despues);
        assert!((nivelador.ganancia_db() - antes).abs() < 0.7);
        assert!(nivelador.ganancia_db() < 0.0);
    }

    #[test]
    fn un_interruptor_desconocido_es_un_error_y_no_el_valor_por_defecto() {
        assert_eq!(interpretar_interruptor("X", None, true), Ok(true));
        assert_eq!(interpretar_interruptor("X", Some("0"), true), Ok(false));
        assert_eq!(interpretar_interruptor("X", Some("1"), false), Ok(true));
        assert!(interpretar_interruptor("X", Some("si"), false).is_err());
    }
}
