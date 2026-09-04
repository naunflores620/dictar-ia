//! Escritura del audio a disco.
//!
//! Regla que no se rompe: **se escribe a disco antes de procesar nada**. Si la
//! aplicación muere en el minuto 90 de una clase de dos horas, el audio tiene
//! que estar entero en el archivo. Es un requisito de producto, no un detalle
//! de implementación: perder una clase por un fallo del programa es lo único
//! que no tiene arreglo después.

use crate::{AudioFrame, Result, SAMPLE_RATE};
use dictar_domain::Track;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Escribe cada pista en su propio archivo.
///
/// Dos archivos y no uno estéreo: las pistas se transcriben por separado, y un
/// archivo por pista permite reprocesar solo una si hace falta.
///
/// Formato WAV de 16 bits. Ocupa ~115 MB/hora por pista, frente a los ~5 MB de
/// Opus; se cambiará a Opus en cuanto entre `libopus`. Mientras tanto se
/// prefiere un formato sin dependencias externas: no poder abrir una grabación
/// por una librería que falta sería mucho peor que ocupar de más.
pub struct EscritorPistas {
    dir: PathBuf,
    escritores: HashMap<Track, hound::WavWriter<std::io::BufWriter<std::fs::File>>>,
    muestras: HashMap<Track, u64>,
}

impl EscritorPistas {
    pub fn nuevo(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            escritores: HashMap::new(),
            muestras: HashMap::new(),
        })
    }

    pub fn ruta(&self, track: Track) -> PathBuf {
        self.dir.join(format!("{}.wav", track.as_str()))
    }

    /// Escribe un bloque, creando el archivo de esa pista si es el primero.
    pub fn escribir(&mut self, frame: &AudioFrame) -> Result<()> {
        let track = frame.track;

        if !self.escritores.contains_key(&track) {
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: SAMPLE_RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let w = hound::WavWriter::create(self.ruta(track), spec)?;
            self.escritores.insert(track, w);
        }

        let w = self.escritores.get_mut(&track).expect("recién insertado");

        for &m in &frame.pcm {
            // Se recorta antes de convertir: una muestra fuera de rango daría
            // la vuelta al entero y produciría un chasquido muy audible.
            let recortada = m.clamp(-1.0, 1.0);
            w.write_sample((recortada * i16::MAX as f32) as i16)?;
        }

        *self.muestras.entry(track).or_insert(0) += frame.pcm.len() as u64;
        Ok(())
    }

    /// Vuelca lo pendiente sin cerrar los archivos.
    ///
    /// Se llama cada pocos segundos durante la grabación: así un corte de luz
    /// cuesta unos segundos de audio, no la clase entera.
    pub fn sincronizar(&mut self) -> Result<()> {
        for w in self.escritores.values_mut() {
            w.flush()?;
        }
        Ok(())
    }

    pub fn duracion_ms(&self, track: Track) -> i64 {
        self.muestras
            .get(&track)
            .map(|n| (*n as i64 * 1000) / SAMPLE_RATE as i64)
            .unwrap_or(0)
    }

    /// Cierra los archivos dejando la cabecera WAV correcta.
    pub fn cerrar(self) -> Result<Vec<PathBuf>> {
        let mut rutas = Vec::new();
        for (track, w) in self.escritores {
            w.finalize()?;
            rutas.push(self.dir.join(format!("{}.wav", track.as_str())));
        }
        rutas.sort();
        Ok(rutas)
    }
}

/// Lee un WAV y lo devuelve como mono a 16 kHz, para importar grabaciones
/// hechas con el móvil o con cualquier otra herramienta.
pub fn leer_wav(ruta: impl AsRef<Path>) -> Result<(Vec<f32>, u32)> {
    let mut lector = hound::WavReader::open(ruta)?;
    let spec = lector.spec();

    let muestras: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => lector
            .samples::<f32>()
            .collect::<std::result::Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            lector
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<std::result::Result<_, _>>()?
        }
    };

    let mono = crate::mezcla::a_mono(&muestras, spec.channels as usize);

    // El contrato de esta función es entregar mono a 16 kHz, no solo mono:
    // un WAV importado del móvil suele estar a 48 kHz, y si se dejara pasar,
    // `mezclar` (reproducción) y la transcripción de importadas lo tratarían
    // como si estuviera a 16 kHz y sonaría a 3× la velocidad.
    let mut r = crate::mezcla::Remuestreador::nuevo(spec.sample_rate)?;
    let mut a_16k = r.procesar(&mono)?;
    // `procesar` solo emite bloques completos de 1024 muestras: al leer el
    // archivo entero de una sola pasada (no en vivo, donde siempre llega más
    // audio detrás) hay que pedirle el resto a `vaciar`, o se pierden hasta
    // 1023 muestras del final —hasta 21 ms a 48 kHz— sin ningún aviso.
    a_16k.extend(r.vaciar()?);
    Ok((a_16k, SAMPLE_RATE))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(track: Track, n: usize, valor: f32) -> AudioFrame {
        AudioFrame {
            track,
            pcm: vec![valor; n],
            timestamp_ms: 0,
        }
    }

    #[test]
    fn cada_pista_va_a_su_propio_archivo() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = EscritorPistas::nuevo(dir.path()).unwrap();

        e.escribir(&frame(Track::Mic, 16_000, 0.1)).unwrap();
        e.escribir(&frame(Track::System, 16_000, 0.2)).unwrap();

        let rutas = e.cerrar().unwrap();
        assert_eq!(rutas.len(), 2);
        assert!(dir.path().join("mic.wav").is_file());
        assert!(dir.path().join("system.wav").is_file());
    }

    #[test]
    fn solo_se_crea_el_archivo_de_la_pista_que_llega() {
        // En una reunión presencial no hay pista de sistema: no debe quedar un
        // system.wav vacío confundiendo al procesado posterior.
        let dir = tempfile::tempdir().unwrap();
        let mut e = EscritorPistas::nuevo(dir.path()).unwrap();
        e.escribir(&frame(Track::Mic, 1600, 0.1)).unwrap();
        e.cerrar().unwrap();

        assert!(dir.path().join("mic.wav").is_file());
        assert!(!dir.path().join("system.wav").exists());
    }

    #[test]
    fn el_audio_escrito_se_puede_volver_a_leer() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = EscritorPistas::nuevo(dir.path()).unwrap();
        e.escribir(&frame(Track::Mic, 8_000, 0.5)).unwrap();
        e.cerrar().unwrap();

        let (muestras, sr) = leer_wav(dir.path().join("mic.wav")).unwrap();
        assert_eq!(sr, SAMPLE_RATE);
        assert_eq!(muestras.len(), 8_000);
        // 0.5 debe sobrevivir al viaje por 16 bits.
        assert!((muestras[0] - 0.5).abs() < 0.001);
    }

    #[test]
    fn una_muestra_fuera_de_rango_se_recorta_en_vez_de_dar_la_vuelta() {
        // Sin el clamp, 1.5 desbordaría el i16 y sonaría como un chasquido.
        let dir = tempfile::tempdir().unwrap();
        let mut e = EscritorPistas::nuevo(dir.path()).unwrap();
        e.escribir(&AudioFrame {
            track: Track::Mic,
            pcm: vec![1.5, -1.5, 0.0],
            timestamp_ms: 0,
        })
        .unwrap();
        e.cerrar().unwrap();

        let (m, _) = leer_wav(dir.path().join("mic.wav")).unwrap();
        assert!(m[0] > 0.99 && m[0] <= 1.0, "valor {}", m[0]);
        assert!(m[1] < -0.99 && m[1] >= -1.0, "valor {}", m[1]);
    }

    #[test]
    fn la_duracion_se_lleva_al_dia_durante_la_grabacion() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = EscritorPistas::nuevo(dir.path()).unwrap();

        e.escribir(&frame(Track::Mic, 16_000, 0.1)).unwrap();
        assert_eq!(e.duracion_ms(Track::Mic), 1000);

        e.escribir(&frame(Track::Mic, 8_000, 0.1)).unwrap();
        assert_eq!(e.duracion_ms(Track::Mic), 1500);

        // Una pista que no ha recibido nada dura cero, no falla.
        assert_eq!(e.duracion_ms(Track::System), 0);
    }

    #[test]
    fn leer_wav_no_pierde_el_final_de_un_archivo_a_48khz() {
        // Antes de que `leer_wav` llamara a `Remuestreador::vaciar`, las
        // muestras que no llegaban a completar el último bloque de 1024 se
        // quedaban dentro del remuestreador y la función las descartaba en
        // silencio: un WAV importado del móvil a 48 kHz salía hasta 21 ms más
        // corto de lo que en realidad duraba.
        let dir = tempfile::tempdir().unwrap();
        let ruta = dir.path().join("importado.wav");

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&ruta, spec).unwrap();
        // 1,5 s a 48 kHz: no es múltiplo del bloque de 1024 del remuestreador,
        // así que algo queda pendiente hasta el final del archivo.
        for i in 0..72_000 {
            let v = ((i as f32 * 0.01).sin() * 0.5 * i16::MAX as f32) as i16;
            w.write_sample(v).unwrap();
        }
        w.finalize().unwrap();

        let (muestras, sr) = leer_wav(&ruta).unwrap();
        assert_eq!(sr, SAMPLE_RATE);

        // 72 000 muestras a 48 kHz equivalen a ~24 000 a 16 kHz (un tercio de
        // la duración). La tolerancia cubre el redondeo del remuestreo, no
        // una pérdida real de audio.
        let esperado = 24_000;
        assert!(
            (muestras.len() as i64 - esperado).abs() < 400,
            "salieron {} muestras, esperaba ~{esperado}",
            muestras.len()
        );
    }

    #[test]
    fn sincronizar_deja_el_archivo_legible_a_mitad_de_grabacion() {
        // Es lo que hace que un corte de luz cueste segundos y no la clase.
        let dir = tempfile::tempdir().unwrap();
        let mut e = EscritorPistas::nuevo(dir.path()).unwrap();

        e.escribir(&frame(Track::Mic, 16_000, 0.3)).unwrap();
        e.sincronizar().unwrap();

        let (m, _) = leer_wav(dir.path().join("mic.wav")).unwrap();
        assert_eq!(m.len(), 16_000);
    }
}
