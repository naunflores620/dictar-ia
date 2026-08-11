//! Detección de actividad de voz y troceado.
//!
//! Hace tres cosas, y la tercera es la que más importa:
//!
//! 1. Descarta los silencios, lo que reduce el tiempo de transcripción. En una
//!    clase online hay muchos: el profesor lee, busca un archivo, espera
//!    preguntas.
//! 2. Corta los segmentos en fronteras naturales de habla en vez de cada N
//!    segundos a ciegas. Partir una palabra por la mitad degrada bastante la
//!    transcripción.
//! 3. **Elimina la causa principal de las alucinaciones de Whisper**, que
//!    aparecen justo en los tramos largos sin voz: el modelo, ante silencio,
//!    se inventa frases enteras («Subtítulos por la comunidad de Amara.org» es
//!    la más famosa). Si el silencio no le llega, no las produce.

pub mod segmentador;

use dictar_audio::SAMPLE_RATE;

/// Duración de la ventana de análisis.
///
/// 30 ms es el estándar en detección de voz: lo bastante corto para localizar
/// el inicio de una palabra, y lo bastante largo para que la energía sea una
/// medida estable.
pub const VENTANA_MS: usize = 30;
pub const MUESTRAS_VENTANA: usize = (SAMPLE_RATE as usize * VENTANA_MS) / 1000;

/// Ventanas que se guardan para estimar el fondo: 10 segundos.
///
/// El tamaño es un compromiso. Corto de más, una frase larga sin pausas
/// arrastraría el suelo hacia arriba. Largo de más, el detector tardaría en
/// adaptarse a un ruido nuevo. Diez segundos es más que suficiente para que
/// aparezca alguna pausa entre frases, y bastante ágil para el resto.
pub const VENTANAS_FONDO: usize = 10_000 / VENTANA_MS;

/// Detector de voz por energía con umbral adaptativo.
///
/// No usa Silero a propósito. El caso principal de esta aplicación es audio
/// digital de Meet o Zoom, con una relación señal-ruido altísima, y ahí la
/// energía discrimina perfectamente. Arrastrar ONNX Runtime al instalador para
/// eso sería pagar mucho por nada. Para audio de sala ruidoso —reuniones
/// presenciales, que son minoría— Silero sí compensaría, y encaja detrás de
/// este mismo interfaz.
pub struct Vad {
    /// Energías recientes, para estimar el fondo por **mínimo sobre ventana
    /// deslizante**.
    ///
    /// Es la técnica estándar («minimum statistics») y sustituye a una media
    /// móvil, que aquí no vale: una media sube mientras alguien habla, y con
    /// un profesor que encadena treinta segundos sin pausa acaba tomando su
    /// voz por ruido de fondo y dejando de transcribirla.
    ///
    /// El mínimo no tiene ese problema: el habla siempre baja de nivel entre
    /// sílabas y entre frases, así que el mínimo de los últimos segundos es el
    /// fondo real. Y si arranca un ventilador a mitad de clase, en cuanto la
    /// ventana se llena de ese ruido el suelo sube solo.
    energias: Vec<f32>,
    pos: usize,
    llena: bool,
    suelo_db: f32,
    /// Cuánto hay que superar el suelo para considerarlo voz.
    margen_db: f32,
    /// Ventanas consecutivas con voz necesarias para declarar inicio.
    /// Evita que un clic de ratón abra un segmento.
    ataque: u32,
    /// Ventanas de silencio que se siguen considerando voz tras dejar de
    /// hablar. Sin esto se cortan los finales de palabra, que son de baja
    /// energía y precisamente los que más información llevan en español.
    caida: u32,

    consecutivas_voz: u32,
    consecutivas_silencio: u32,
    hablando: bool,
    /// Última decisión **sin suavizar**.
    ///
    /// Se expone aparte porque los dos consumidores quieren cosas distintas:
    /// el segmentador necesita saber si hay una pausa AHORA para cerrar por
    /// ahí, mientras que la proporción de voz quiere la versión suavizada. Si
    /// se usara la suavizada para medir pausas, el hangover de 600 ms se
    /// sumaría a los 500 ms de pausa y harían falta 1,1 s de silencio para
    /// cerrar un segmento, en vez de los 500 ms del diseño.
    ultima_bruta: bool,
}

impl Default for Vad {
    fn default() -> Self {
        Self::nuevo()
    }
}

impl Vad {
    pub fn nuevo() -> Self {
        Self {
            energias: vec![f32::INFINITY; VENTANAS_FONDO],
            pos: 0,
            llena: false,
            // Se arranca con un suelo bajo en lugar de tomarlo de la primera
            // ventana. Si la sesión empieza con el profesor ya hablando,
            // sembrarlo con esa ventana pondría el suelo a la altura de la voz
            // y la primera frase se perdería entera. Empezando bajo, como mucho
            // se transcriben unos segundos de ruido al principio, que es un
            // fallo mucho más barato.
            suelo_db: -60.0,
            margen_db: 9.0,
            // 3 ventanas = 90 ms.
            ataque: 3,
            // 20 ventanas = 600 ms. Coincide con la pausa que se usa para
            // cerrar un segmento, de modo que no se corta a mitad de frase.
            caida: 20,
            consecutivas_voz: 0,
            consecutivas_silencio: 0,
            hablando: false,
            ultima_bruta: false,
        }
    }

    /// Detector más permisivo, para audio de sala.
    pub fn presencial() -> Self {
        Self {
            margen_db: 6.0,
            ataque: 2,
            ..Self::nuevo()
        }
    }

    pub fn suelo_db(&self) -> f32 {
        self.suelo_db
    }

    /// Decisión de la última ventana sin el suavizado de ataque y caída.
    /// El segmentador la usa para localizar pausas reales.
    pub fn voz_bruta(&self) -> bool {
        self.ultima_bruta
    }

    /// Procesa una ventana y dice si hay voz en curso.
    pub fn procesar(&mut self, ventana: &[f32]) -> bool {
        let db = energia_db(ventana);

        self.energias[self.pos] = db;
        self.pos = (self.pos + 1) % self.energias.len();
        if self.pos == 0 {
            self.llena = true;
        }

        let hasta = if self.llena {
            self.energias.len()
        } else {
            self.pos.max(1)
        };
        let minimo = self.energias[..hasta]
            .iter()
            .copied()
            .fold(f32::INFINITY, f32::min);

        // Hasta que la ventana se llena no hay una estimación fiable del fondo,
        // así que se conserva el suelo inicial —bajo— y se prefiere detectar de
        // más. Perder la primera frase del profesor sería mucho peor que
        // transcribir unos segundos de ruido.
        self.suelo_db = if self.llena {
            minimo
        } else {
            minimo.min(self.suelo_db)
        };

        let hay_voz = db > self.suelo_db + self.margen_db;
        self.ultima_bruta = hay_voz;

        if hay_voz {
            self.consecutivas_voz += 1;
            self.consecutivas_silencio = 0;
        } else {
            self.consecutivas_silencio += 1;
            self.consecutivas_voz = 0;
        }

        if self.hablando {
            if self.consecutivas_silencio >= self.caida {
                self.hablando = false;
            }
        } else if self.consecutivas_voz >= self.ataque {
            self.hablando = true;
        }

        self.hablando
    }

    /// Analiza un bloque entero y devuelve una decisión por ventana.
    pub fn procesar_bloque(&mut self, pcm: &[f32]) -> Vec<bool> {
        pcm.chunks(MUESTRAS_VENTANA)
            .map(|v| self.procesar(v))
            .collect()
    }

    /// Proporción de un bloque que contiene voz.
    ///
    /// Se usa para decidir si merece la pena mandarlo a Whisper: transcribir
    /// treinta segundos de silencio cuesta lo mismo que treinta de habla y solo
    /// puede producir alucinaciones.
    pub fn proporcion_voz(&mut self, pcm: &[f32]) -> f32 {
        let d = self.procesar_bloque(pcm);
        if d.is_empty() {
            return 0.0;
        }
        d.iter().filter(|x| **x).count() as f32 / d.len() as f32
    }

    pub fn reiniciar(&mut self) {
        self.consecutivas_voz = 0;
        self.consecutivas_silencio = 0;
        self.hablando = false;
        self.ultima_bruta = false;
    }
}

/// Energía de la ventana en dBFS.
pub fn energia_db(ventana: &[f32]) -> f32 {
    if ventana.is_empty() {
        return -100.0;
    }
    let suma: f32 = ventana.iter().map(|x| x * x).sum();
    let rms = (suma / ventana.len() as f32).sqrt();
    // El suelo evita `log10(0) = -inf`, que envenenaría la media móvil.
    20.0 * rms.max(1e-7).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ruido(n: usize, amplitud: f32) -> Vec<f32> {
        // Pseudoaleatorio determinista: un test que falla una vez de cada diez
        // es peor que no tener test.
        let mut x = 12345u32;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1103515245).wrapping_add(12345);
                ((x >> 16) as f32 / 32768.0 - 1.0) * amplitud
            })
            .collect()
    }

    fn tono(n: usize, amplitud: f32) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * 0.15).sin() * amplitud).collect()
    }

    #[test]
    fn el_silencio_absoluto_da_la_energia_minima() {
        assert!(energia_db(&vec![0.0; 480]) <= -100.0);
    }

    #[test]
    fn una_ventana_vacia_no_produce_infinito() {
        // log10(0) sería -inf y envenenaría la media móvil del suelo de ruido.
        assert!(energia_db(&[]).is_finite());
    }

    #[test]
    fn la_energia_crece_con_la_amplitud() {
        let bajo = energia_db(&tono(480, 0.01));
        let alto = energia_db(&tono(480, 0.5));
        assert!(alto > bajo + 20.0, "bajo {bajo} alto {alto}");
    }

    #[test]
    fn el_silencio_continuo_no_se_toma_por_voz() {
        let mut vad = Vad::nuevo();
        let mut alguna = false;
        for _ in 0..100 {
            if vad.procesar(&ruido(MUESTRAS_VENTANA, 0.0005)) {
                alguna = true;
            }
        }
        assert!(!alguna, "el ruido de fondo no debe activar el detector");
    }

    #[test]
    fn la_voz_sobre_ruido_de_fondo_se_detecta() {
        let mut vad = Vad::nuevo();

        // Ambiente de aula.
        for _ in 0..40 {
            vad.procesar(&ruido(MUESTRAS_VENTANA, 0.002));
        }
        // Empieza a hablar.
        let mut detectada = false;
        for _ in 0..20 {
            if vad.procesar(&tono(MUESTRAS_VENTANA, 0.2)) {
                detectada = true;
            }
        }
        assert!(detectada, "no detectó voz claramente por encima del ruido");
    }

    #[test]
    fn un_clic_aislado_no_abre_un_segmento() {
        // El ataque de 3 ventanas existe justo para esto: un clic de ratón o
        // una silla no deben iniciar una frase.
        let mut vad = Vad::nuevo();
        for _ in 0..40 {
            vad.procesar(&ruido(MUESTRAS_VENTANA, 0.002));
        }
        assert!(!vad.procesar(&tono(MUESTRAS_VENTANA, 0.5)));
    }

    #[test]
    fn el_final_de_palabra_no_se_corta_en_seco() {
        // La caída mantiene la voz activa durante ~600 ms: en español los
        // finales de palabra son de baja energía y llevan mucha información
        // (plurales, conjugaciones).
        let mut vad = Vad::nuevo();
        for _ in 0..40 {
            vad.procesar(&ruido(MUESTRAS_VENTANA, 0.002));
        }
        for _ in 0..10 {
            vad.procesar(&tono(MUESTRAS_VENTANA, 0.3));
        }

        // Dos ventanas de silencio: todavía debe considerarse habla.
        vad.procesar(&ruido(MUESTRAS_VENTANA, 0.002));
        assert!(vad.procesar(&ruido(MUESTRAS_VENTANA, 0.002)));
    }

    #[test]
    fn tras_una_pausa_larga_se_cierra_el_habla() {
        let mut vad = Vad::nuevo();
        for _ in 0..40 {
            vad.procesar(&ruido(MUESTRAS_VENTANA, 0.002));
        }
        for _ in 0..10 {
            vad.procesar(&tono(MUESTRAS_VENTANA, 0.3));
        }

        let mut ultimo = true;
        for _ in 0..30 {
            ultimo = vad.procesar(&ruido(MUESTRAS_VENTANA, 0.002));
        }
        assert!(!ultimo, "una pausa de casi un segundo debe cerrar el habla");
    }

    #[test]
    fn el_suelo_de_ruido_se_adapta_a_un_ambiente_mas_ruidoso() {
        // Es lo que permite que funcione con el aire acondicionado del aula sin
        // que nadie configure un umbral.
        let mut vad = Vad::nuevo();
        for _ in 0..10 {
            vad.procesar(&ruido(MUESTRAS_VENTANA, 0.0005));
        }
        let inicial = vad.suelo_db();

        // Un ventilador que arranca a mitad de clase: ruido nuevo y sostenido
        // durante más de lo que dura la ventana de fondo. El suelo debe acabar
        // subiendo, o ese ruido se clasificaría como voz para siempre.
        for _ in 0..(VENTANAS_FONDO + 50) {
            vad.procesar(&ruido(MUESTRAS_VENTANA, 0.02));
        }
        assert!(
            vad.suelo_db() > inicial,
            "el suelo pasó de {inicial} a {}",
            vad.suelo_db()
        );
    }

    #[test]
    fn un_bloque_de_puro_silencio_tiene_proporcion_de_voz_cero() {
        // Es la comprobación que evita mandar treinta segundos de nada a
        // Whisper, que es donde se inventa frases.
        let mut vad = Vad::nuevo();
        assert_eq!(vad.proporcion_voz(&vec![0.0; SAMPLE_RATE as usize]), 0.0);
    }

    #[test]
    fn un_bloque_hablado_tiene_proporcion_alta() {
        let mut vad = Vad::nuevo();
        // Preludio de ambiente para fijar el suelo.
        vad.procesar_bloque(&ruido(MUESTRAS_VENTANA * 40, 0.002));

        // Modulado, como el habla real: un tono plano sostenido acabaría
        // siendo, con razón, el propio fondo.
        let voz: Vec<f32> = (0..SAMPLE_RATE as usize)
            .map(|i| {
                let env = 0.5 + 0.5 * (i as f32 / SAMPLE_RATE as f32 * 25.0).sin().abs();
                (i as f32 * 0.15).sin() * 0.25 * env
            })
            .collect();
        let p = vad.proporcion_voz(&voz);
        assert!(p > 0.8, "proporción de voz {p}");
    }
}
