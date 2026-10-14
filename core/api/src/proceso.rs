//! Cierre de una sesión: confirmar el texto en vivo o transcribir por lotes,
//! y después generar los apuntes.
//!
//! El lote se reserva para grabaciones importadas, falta de texto vivo o una
//! cola final sin cubrir. Una sesión en vivo con cobertura suficiente conserva
//! sus tramos y evita volver a decodificar todo el audio.

use crate::eventos::{Evento, FaseProceso};
use crate::limpieza_transcripcion::deduplicar_solape;
use crate::{ApiError, Nucleo, Result};
use dictar_audio::wav::leer_wav;
use dictar_domain::alert;
use dictar_domain::notes::NotesPayload;
use dictar_domain::transcript::GlossarySource;
use dictar_domain::{EpochMs, SessionId, SessionKind, SessionStatus, Track};
use dictar_notes::{NotesGenerator, SessionContext, fecha};
use dictar_storage::notes_store::NewNotes;
use dictar_storage::transcript::NewUtterance;
use dictar_stt::diarizacion::{
    etiquetar_frases, Diarizador, OpcionesDiarizacion, RutasModelos, SegmentoHablante,
};
use dictar_stt::modelos::{self, Modelo};
use dictar_stt::whisper::Transcriptor;
use dictar_stt::{SttError, SttOpciones};
use dictar_vad::segmentador::{SOLAPE_MS, Segmentador};
use std::sync::Arc;
use std::sync::mpsc::Sender;

/// Cola sin transcribir que se tolera antes de rehacer el tramo final.
///
/// Un segmento dura como mucho 45 s, así que si la transcripción en vivo se
/// quedó a más de un minuto del final es que algo se la comió: un cierre a
/// medias, el modelo caído a mitad de clase. Menos que eso suele ser solo el
/// silencio de la despedida, y no merece rehacerse.
pub(crate) const COLA_SIN_CUBRIR_MS: i64 = 60_000;

/// Desfase de reloj tolerado entre el WAV y el último timestamp reconocido.
pub(crate) const DESFASE_TIMESTAMPS_TOLERADO_MS: i64 = 2_000;

/// Si la transcripción en vivo dejó un tramo final sin cubrir.
pub(crate) fn falta_cobertura(duracion_ms: Option<i64>, fin_transcrito_ms: i64) -> bool {
    duracion_ms
        .map(|d| {
            fin_transcrito_ms < 0
                || fin_transcrito_ms.saturating_sub(d) > DESFASE_TIMESTAMPS_TOLERADO_MS
                || d.saturating_sub(fin_transcrito_ms) > COLA_SIN_CUBRIR_MS
        })
        .unwrap_or(false)
}

/// Qué hace el cierre de una sesión que ya tiene texto en vivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanCierre {
    /// El texto en vivo basta: solo se confirma.
    Confirmar,
    /// Sistema cubierto; se transcribe por lotes solo el micrófono.
    SoloMicrofono,
    /// Falta cobertura real: pasada completa de ambas pistas.
    Completa,
}

/// Decide el cierre. La cobertura temporal es de la pista de sistema (la que
/// transcribe la ruta en vivo); si falta, el micrófono no cambia nada, porque
/// la pasada completa ya lo incluye.
pub(crate) fn decidir_cierre(cobertura_temporal: bool, falta_microfono: bool) -> PlanCierre {
    match (cobertura_temporal, falta_microfono) {
        (false, _) => PlanCierre::Completa,
        (true, true) => PlanCierre::SoloMicrofono,
        (true, false) => PlanCierre::Confirmar,
    }
}

/// Si, tras el texto en vivo, el micrófono aún necesita pasada por lotes.
///
/// Con el marcador que deja el transcriptor al cerrar limpio (motor incremental
/// activo en el micrófono toda la sesión) no hace falta ni leer el WAV: el
/// micrófono ya tiene sus frases en vivo. Sin marcador —sesiones viejas, modo
/// Whisper, motor caído a mitad— se conserva la comprobación de voz de siempre.
/// Si esa comprobación falla se asume que falta: es el lado seguro.
pub(crate) fn falta_microfono_tras_vivo(
    marcador_presente: bool,
    hay_voz_independiente: impl FnOnce() -> Result<bool>,
) -> bool {
    if marcador_presente {
        return false;
    }
    match hay_voz_independiente() {
        Ok(hay_voz) => hay_voz,
        Err(error) => {
            tracing::warn!(%error, "no se pudo comprobar la pista de micrófono; se conserva la pasada de respaldo");
            true
        }
    }
}

/// Pistas que decodifica el lote, en el orden en que se leen del disco.
pub(crate) fn pistas_del_lote(solo_pista: Option<Track>) -> Vec<Track> {
    [Track::Mic, Track::System]
        .into_iter()
        .filter(|t| solo_pista.is_none_or(|s| s == *t))
        .collect()
}

/// Resultado de procesar una sesión.
#[derive(Debug, Clone)]
pub struct Procesada {
    pub session_id: SessionId,
    pub frases: usize,
    pub avisos: usize,
    pub coste_usd: f64,
    pub proveedor: String,
}

impl Nucleo {
    /// Transcribe y genera las notas de una sesión ya grabada.
    ///
    /// `progreso` recibe eventos para la barra de la interfaz; puede ser un
    /// canal descartado si nadie mira.
    pub async fn procesar_sesion(
        &self,
        id: &SessionId,
        modelo: Modelo,
        ahora: EpochMs,
        progreso: Option<Sender<Evento>>,
    ) -> Result<Procesada> {
        let emitir = |e: Evento| {
            if let Some(tx) = &progreso {
                let _ = tx.send(e);
            }
        };

        let sesion = self.con_db(|db| db.sesion(id))?;

        // Lo primero: esperar a que el transcriptor en vivo vacíe lo que le
        // quedara en cola al detener. Detener vuelve al instante; la espera
        // ocurre aquí, que es donde hay una barra de progreso mirando.
        self.esperar_drenaje(id, &emitir).await;

        let vivas = self.con_db(|db| db.transcripcion(id))?;
        let fin_transcrito = vivas.iter().map(|u| u.end_ms).max().unwrap_or(0);

        if !vivas.is_empty() {
            let duracion = sesion.duracion_ms();
            let cobertura_temporal = duracion
                .map(|d| d > 0 && !falta_cobertura(Some(d), fin_transcrito))
                .unwrap_or(false);
            let dir_audio = self.dir_audio(id);
            let falta_microfono = falta_microfono_tras_vivo(
                dir_audio
                    .join(crate::transcriptor::MARCADOR_MIC_EN_VIVO)
                    .is_file(),
                || Self::hay_voz_independiente_en_microfono(&dir_audio),
            );
            let plan = decidir_cierre(cobertura_temporal, falta_microfono);

            if plan == PlanCierre::SoloMicrofono {
                // Sistema ya está cubierto en vivo; lo único pendiente son las
                // intervenciones propias, que en vivo no se transcriben cuando
                // hay loopback. Rehacer también el sistema con el modelo grande
                // costaría minutos para dejar el mismo texto.
                tracing::info!(
                    fin_transcrito,
                    duracion_ms = duracion.unwrap_or_default(),
                    "sistema cubierto en vivo; solo se transcribe por lotes el micrófono"
                );
                self.con_db(|db| db.cambiar_estado(id, SessionStatus::Transcribing))?;
                let mic =
                    self.transcribir_lote(id, &sesion, modelo, 0, Some(Track::Mic), &emitir)?;
                if mic.is_empty() {
                    self.con_db(|db| db.confirmar_vivas(id))?;
                } else {
                    self.guardar_pasada_de_microfono(id, &mic)?;
                }
            } else if plan == PlanCierre::Confirmar {
                // La hipótesis en vivo ya llegó hasta el final de la sesión.
                // Reprocesar todos los WAV con Turbo repetiría minutos de CPU
                // y volvería a sustituir los mismos tramos; se confirma el
                // texto existente y se continúa con las notas.
                let confirmadas = self.con_db(|db| db.confirmar_vivas(id))?;
                tracing::info!(
                    sesion = %id,
                    frases = confirmadas,
                    fin_transcrito,
                    duracion_ms = duracion.unwrap_or_default(),
                    mic_pendiente = falta_microfono,
                    "transcripción en vivo confirmada sin repetir la pasada completa"
                );
            } else {
                tracing::info!(
                    fin_transcrito,
                    duracion_ms = duracion.unwrap_or_default(),
                    mic_pendiente = falta_microfono,
                    "transcripción en vivo incompleta, fuera de rango o falta voz de micrófono; se ejecuta la pasada final de respaldo"
                );
                // Si falta más de un minuto al final, o no se conoce la
                // duración, la pasada de mayor calidad intenta recuperar la
                // cobertura. Este camino también conserva grabaciones cuyo
                // reconocedor en vivo dejó de funcionar.
                self.con_db(|db| db.cambiar_estado(id, SessionStatus::Transcribing))?;
                let definitivas = self.transcribir_lote(id, &sesion, modelo, 0, None, &emitir)?;
                if !definitivas.is_empty() {
                    self.guardar_pasada_completa(id, &definitivas)?;
                } else {
                    tracing::warn!(sesion = %id, "la pasada final no produjo frases; se confirman las frases disponibles");
                    self.con_db(|db| db.confirmar_vivas(id))?;
                }
            }
            self.diarizar_sesion(id, &sesion, &emitir);
            return self.solo_notas(id, &sesion, ahora, progreso).await;
        }

        // Sin transcripción previa: la pasada por lotes completa. Es el camino
        // de las grabaciones importadas, y el respaldo cuando el modelo no
        // pudo cargarse durante la clase.
        self.con_db(|db| db.cambiar_estado(id, SessionStatus::Transcribing))?;

        let frases = self.transcribir_lote(id, &sesion, modelo, 0, None, &emitir)?;
        if frases.is_empty() {
            self.con_db(|db| db.cambiar_estado(id, SessionStatus::Failed))?;
            return Err(ApiError::Config(
                "no hay audio con voz en esta sesión".into(),
            ));
        }

        self.guardar_pasada_completa(id, &frases)?;
        self.diarizar_sesion(id, &sesion, &emitir);
        self.solo_notas(id, &sesion, ahora, progreso).await
    }

    /// Vuelve a generar los apuntes de una sesión con la transcripción que ya
    /// tiene, por ejemplo tras renombrar a sus participantes.
    ///
    /// Llama **solo** a [`Self::solo_notas`]: no transcribe, no diariza y no toca
    /// las frases. Es lo que hace barato corregir una etiqueta: el único coste es
    /// el de una generación de notas.
    pub async fn regenerar_notas(
        &self,
        id: &SessionId,
        ahora: EpochMs,
        progreso: Option<Sender<Evento>>,
    ) -> Result<Procesada> {
        let sesion = self.con_db(|db| db.sesion(id))?;
        if self.con_db(|db| db.transcripcion(id))?.is_empty() {
            return Err(ApiError::Config(
                "la sesión no tiene transcripción de la que generar apuntes".into(),
            ));
        }
        self.solo_notas(id, &sesion, ahora, progreso).await
    }

    /// Guarda una pasada por lotes de las dos pistas, que sustituye a las frases.
    ///
    /// Es el único sitio que reescribe la transcripción entera, y por eso es
    /// también el que invalida la diarización: las frases nuevas nacen con las
    /// etiquetas por defecto, y un marcador viejo haría creer que ya están
    /// identificadas. El precio, declarado en el plan, es que se pierden los
    /// nombres que el usuario hubiera puesto a los participantes de esa sesión.
    pub(crate) fn guardar_pasada_completa(
        &self,
        id: &SessionId,
        frases: &[NewUtterance],
    ) -> Result<()> {
        self.con_db(|db| db.reemplazar_por_definitiva(id, frases))?;
        self.invalidar_diarizacion(id);
        Ok(())
    }

    /// Lo mismo para la pasada que solo rehace el micrófono: sus frases nuevas
    /// llevan «Yo» y la voz de cada persona de la sala hay que volver a buscarla.
    pub(crate) fn guardar_pasada_de_microfono(
        &self,
        id: &SessionId,
        frases: &[NewUtterance],
    ) -> Result<()> {
        self.con_db(|db| db.reemplazar_pista_por_definitiva(id, Track::Mic, frases))?;
        self.invalidar_diarizacion(id);
        Ok(())
    }

    /// Indica si el micrófono conserva voz que la ruta en vivo no haya reconocido.
    ///
    /// En una sesión con loopback, la captura en vivo prioriza sistema y puede
    /// omitir el micrófono para no duplicar por altavoces. Solo se permite cerrar
    /// rápido si el WAV no contiene voz o si, tras atenuar un retorno confirmado,
    /// no queda voz independiente. Ante una correlación ausente o un error se
    /// conserva la pasada completa para no descartar intervenciones del usuario.
    fn hay_voz_independiente_en_microfono(dir: &std::path::Path) -> Result<bool> {
        let mic = dir.join("mic.wav");
        let sistema = dir.join("system.wav");
        if !mic.is_file() || !sistema.is_file() {
            return Ok(false);
        }

        let (mut pcm_mic, _) = leer_wav(&mic)?;
        if !Self::tiene_voz(&pcm_mic) {
            return Ok(false);
        }

        let Some(retorno) = dictar_audio::detectar_retorno_en_wav(&mic, &sistema)? else {
            return Ok(true);
        };
        retorno.atenuar_microfono(&mut pcm_mic);
        Ok(Self::tiene_voz(&pcm_mic))
    }

    fn tiene_voz(pcm: &[f32]) -> bool {
        let mut segmentador = Segmentador::nuevo(Track::Mic);
        let mut segmentos = segmentador.empujar(pcm, 0);
        if let Some(ultimo) = segmentador.finalizar() {
            segmentos.push(ultimo);
        }
        segmentos.iter().any(|s| s.merece_transcribirse())
    }

    /// Espera a que el transcriptor en vivo termine lo que le quedó en cola.
    async fn esperar_drenaje(&self, id: &SessionId, emitir: &dyn Fn(Evento)) {
        let tomado = { self.drenaje.lock().unwrap().take() };
        let Some((sid, t)) = tomado else { return };

        if sid != *id {
            // El drenaje pendiente es de otra sesión: se devuelve a su sitio y
            // seguirá vaciándose en segundo plano.
            *self.drenaje.lock().unwrap() = Some((sid, t));
            return;
        }

        let mut ultimo = usize::MAX;
        let mut sin_avance = 0u32;

        while !t.esta_terminado() {
            let pendientes = t.pendientes();
            emitir(Evento::progreso(
                FaseProceso::Transcribiendo,
                0.0,
                format!("terminando {pendientes} tramo(s) de la clase"),
            ));

            // Cortafuegos: si el hilo murió sin marcar el fin, no se espera
            // eternamente. Dos minutos sin que baje la cola son señal de eso;
            // un solo segmento nunca tarda tanto.
            if pendientes == ultimo {
                sin_avance += 1;
                if sin_avance > 480 {
                    tracing::warn!("el drenaje no avanza; se sigue con lo que hay");
                    break;
                }
            } else {
                sin_avance = 0;
                ultimo = pendientes;
            }

            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }

        match Arc::try_unwrap(t) {
            Ok(t) => t.esperar(),
            // Si alguien más aún lo referencia, con soltarlo basta: el hilo ya
            // terminó su cola.
            Err(a) => drop(a),
        }
    }

    /// Transcribe por lotes el audio de la sesión, desde `desde_ms`.
    ///
    /// Con `desde_ms = 0` es la pasada completa. Con un valor mayor, solo el
    /// tramo final que la pasada en vivo dejó sin cubrir; los segmentos que
    /// cruzan la frontera pueden duplicar unas palabras, y es a propósito:
    /// el generador de notas ya funde duplicados, y perder la frase de la
    /// costura sería peor.
    ///
    /// Con `solo_pista` se decodifica únicamente esa pista; el retorno acústico
    /// se sigue calculando con las dos para poder atenuar el micrófono.
    fn transcribir_lote(
        &self,
        id: &SessionId,
        sesion: &dictar_domain::Session,
        modelo: Modelo,
        desde_ms: i64,
        solo_pista: Option<Track>,
        emitir: &dyn Fn(Evento),
    ) -> Result<Vec<NewUtterance>> {
        let glosario = match &sesion.topic_id {
            Some(t) => self.glosario(t).unwrap_or_default(),
            None => String::new(),
        };

        emitir(Evento::progreso(
            FaseProceso::Segmentando,
            0.0,
            "preparando el audio",
        ));

        let dir = self.dir_audio(id);
        let mut trabajos = Vec::new();
        let retorno = {
            let mic = dir.join("mic.wav");
            let sistema = dir.join("system.wav");
            if mic.is_file() && sistema.is_file() {
                match dictar_audio::detectar_retorno_en_wav(&mic, &sistema) {
                    Ok(retorno) => retorno,
                    Err(error) => {
                        tracing::warn!(%error, "no se pudo analizar el retorno acústico; se transcriben las dos pistas sin atenuar");
                        None
                    }
                }
            } else {
                None
            }
        };
        if let Some(retorno) = &retorno {
            tracing::info!(
                correlacion = retorno.correlacion,
                retardo_ms = retorno.retardo_ms,
                "retorno acústico detectado; se atenuará solo durante la transcripción"
            );
        }

        for track in pistas_del_lote(solo_pista) {
            let ruta = dir.join(format!("{}.wav", track.as_str()));
            if !ruta.is_file() {
                continue;
            }

            let (mut pcm, _sr) = leer_wav(&ruta)?;
            if track == Track::Mic {
                if let Some(retorno) = &retorno {
                    retorno.atenuar_microfono(&mut pcm);
                }
            }
            let mut seg = Segmentador::nuevo(track);
            let mut segmentos = seg.empujar(&pcm, 0);
            if let Some(u) = seg.finalizar() {
                segmentos.push(u);
            }

            // Los segmentos sin voz no llegan a Whisper: es donde alucina, y
            // además cuesta lo mismo transcribir silencio que habla.
            segmentos.retain(|s| s.merece_transcribirse());
            trabajos.extend(segmentos);
        }

        if desde_ms > 0 {
            trabajos.retain(|s| s.fin_ms > desde_ms);
        }
        if trabajos.is_empty() {
            return Ok(Vec::new());
        }

        // Orden cronológico entre pistas: es como se leerá la transcripción.
        trabajos.sort_by_key(|s| s.inicio_ms);

        emitir(Evento::progreso(
            FaseProceso::CargandoModelo,
            0.0,
            modelo.nombre_humano(),
        ));

        let ruta_modelo = modelos::localizar(modelo)?;
        let trans = Transcriptor::cargar(&ruta_modelo)?;

        let mut opciones = SttOpciones::definitiva();
        if !glosario.is_empty() {
            opciones.glosario = Some(glosario);
        }
        if let Some(l) = &sesion.language {
            opciones.idioma = Some(l.clone());
        }

        let quien_sistema = self.nombre_interlocutor(sesion);
        let total = trabajos.len();
        let mut frases: Vec<NewUtterance> = Vec::new();

        for (i, s) in trabajos.iter().enumerate() {
            emitir(Evento::progreso(
                FaseProceso::Transcribiendo,
                i as f32 / total as f32,
                format!("segmento {} de {total}", i + 1),
            ));

            // El contexto previo mejora la continuidad entre segmentos de la misma pista:
            // Whisper acierta más con los nombres propios si acaba de verlos.
            opciones.contexto_previo = frases
                .iter()
                .rev()
                .find(|u| u.track == s.track && !dictar_stt::limpieza::es_alucinacion(&u.text))
                .map(|u: &NewUtterance| {
                    u.text
                        .chars()
                        .rev()
                        .take(200)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect()
                });

            let fragmentos = trans.transcribir(&s.pcm, &opciones)?;

            // El decodificador puede engancharse y repetir la misma frase.
            let textos: Vec<&str> = fragmentos.iter().map(|f| f.texto.as_str()).collect();
            let (indices, en_bucle) = dictar_stt::limpieza::colapsar_bucles(&textos);
            if en_bucle > 0 {
                tracing::info!(descartados = en_bucle, "bucle del decodificador");
            }

            let mut frases_segmento = Vec::new();
            for idx in indices {
                let f = &fragmentos[idx];
                if dictar_stt::limpieza::es_alucinacion(&f.texto) {
                    continue;
                }

                let u = NewUtterance {
                    track: s.track,
                    speaker: Some(match s.track {
                        Track::Mic => "Yo".to_owned(),
                        Track::System => quien_sistema.clone(),
                    }),
                    start_ms: s.inicio_ms + f.inicio_ms,
                    end_ms: s.inicio_ms + f.fin_ms,
                    text: f.texto.clone(),
                    confidence: Some(f.confianza),
                    is_final: true,
                };

                frases_segmento.push(u);
            }

            let descartadas =
                deduplicar_solape(&frases, &mut frases_segmento, s.inicio_ms, SOLAPE_MS);
            if descartadas > 0 {
                tracing::info!(
                    descartadas,
                    "frases duplicadas en el solape entre segmentos"
                );
            }

            for frase in &frases_segmento {
                emitir(Evento::frase(
                    id,
                    s.track,
                    frase.start_ms,
                    frase.end_ms,
                    &frase.text,
                    true,
                ));
            }
            frases.extend(frases_segmento);
        }

        frases.sort_by_key(|u| u.start_ms);

        // Sin auriculares, el micrófono capta también al profesor por los
        // altavoces; sin este filtro, los apuntes atribuirían a «Yo» lo que
        // dijo el profesor.
        let descartadas = Self::suprimir_diafonia(&mut frases);
        if descartadas > 0 {
            tracing::info!(descartadas, "frases del micrófono que eran eco");
        }

        Ok(frases)
    }

    /// Genera las notas de una sesión cuya transcripción ya existe.
    ///
    /// Es el camino corto: cuando la clase se ha transcrito en vivo, detener
    /// solo cuesta el resumen. Y también lo usa el camino largo, para no tener
    /// dos versiones de lo mismo.
    async fn solo_notas(
        &self,
        id: &SessionId,
        sesion: &dictar_domain::Session,
        ahora: EpochMs,
        progreso: Option<Sender<Evento>>,
    ) -> Result<Procesada> {
        let emitir = |e: Evento| {
            if let Some(tx) = &progreso {
                let _ = tx.send(e);
            }
        };

        let glosario = match &sesion.topic_id {
            Some(t) => self.glosario(t).unwrap_or_default(),
            None => String::new(),
        };

        // -- Notas -----------------------------------------------------------
        self.con_db(|db| db.cambiar_estado(id, SessionStatus::Summarizing))?;
        emitir(Evento::progreso(
            FaseProceso::GenerandoNotas,
            0.0,
            "resumiendo",
        ));

        let utterances = self.transcripcion_sin_eco(id)?;

        let diapositivas = self.con_db(|db| db.diapositivas(id))?;
        let n_frases = utterances.len();

        let ctx = SessionContext {
            kind: sesion.kind.into(),
            contexto: self.contexto_de(sesion),
            duracion_ms: sesion
                .duracion_ms()
                .unwrap_or_else(|| utterances.last().map(|u| u.end_ms).unwrap_or(0)),
            fecha: fecha::iso(sesion.started_at),
            glosario,
            diapositivas,
        };

        let router = self.router()?;
        let generador = NotesGenerator::nuevo(&router);
        let generadas = generador.generar(&ctx, &utterances).await?;

        // -- Guardado --------------------------------------------------------
        emitir(Evento::progreso(FaseProceso::Guardando, 0.9, "guardando"));

        let payload = serde_json::to_string(&generadas.payload)
            .map_err(|e| ApiError::Config(e.to_string()))?;

        self.con_db(|db| {
            db.guardar_notas(
                id,
                &NewNotes {
                    template: self.plantilla_de(sesion),
                    kind: "final".to_owned(),
                    seq: None,
                    provider: generadas.proveedor.clone(),
                    model: generadas.proveedor.clone(),
                    prompt_ver: generadas.prompt_ver.to_owned(),
                    payload,
                    tokens_in: Some(generadas.usage.tokens_in as i64),
                    tokens_out: Some(generadas.usage.tokens_out as i64),
                    cost_usd: Some(generadas.usage.cost_usd),
                    created_at: ahora,
                },
            )
        })?;

        // Los avisos salen de las notas por código, no con otra llamada al
        // modelo: los datos ya vienen estructurados, y volver a preguntar
        // sería pagar dos veces y dar una oportunidad más de alucinar.
        let avisos = alert::extraer(&generadas.payload);
        let n_avisos = avisos.len();
        let topic = sesion.topic_id.clone();
        self.con_db(|db| db.guardar_avisos(id, topic.as_ref(), &avisos))?;

        // El vocabulario nuevo realimenta el glosario: es el ciclo que hace que
        // la clase siguiente se transcriba mejor que esta.
        self.realimentar_glosario(&sesion.topic_id, &generadas.payload);

        self.con_db(|db| db.cambiar_estado(id, SessionStatus::Ready))?;

        // Exportar es lo último y no puede tumbar el procesado: si la carpeta
        // configurada ya no existe —un disco externo desconectado, la nube sin
        // montar— se avisa, pero los apuntes ya están guardados en la base.
        match self.exportar_apuntes(id) {
            Ok(Some(ruta)) => tracing::info!(ruta = %ruta.display(), "apuntes exportados"),
            Ok(None) => {}
            Err(e) => tracing::warn!(error = %e, "no se pudieron exportar los apuntes"),
        }

        emitir(Evento::Terminada {
            session_id: id.0.clone(),
            coste_usd: generadas.usage.cost_usd,
            avisos: n_avisos,
        });

        tracing::info!(
            sesion = %id,
            frases = n_frases,
            avisos = n_avisos,
            coste = generadas.usage.cost_usd,
            "sesión procesada"
        );

        Ok(Procesada {
            session_id: id.clone(),
            frases: n_frases,
            avisos: n_avisos,
            coste_usd: generadas.usage.cost_usd,
            proveedor: generadas.proveedor,
        })
    }

    /// Transcripción de la sesión sin el eco del micrófono.
    ///
    /// La pasada en vivo transcribe cada pista por separado y no puede
    /// compararlas entre sí: si el usuario escucha por altavoces, el
    /// micrófono capta al profesor y cada frase aparece duplicada como
    /// «Yo». Se limpia aquí, una vez, reescribiendo la transcripción
    /// definitiva — unas notas que citan dos veces cada frase, con el
    /// hablante equivocado la mitad de ellas, no sirven.
    ///
    /// También lo llama la diarización antes de mirar el audio: si el eco
    /// siguiera ahí, cada frase de eco contaría como una voz más del micrófono
    /// y desaparecería después, dejando una numeración con huecos.
    fn transcripcion_sin_eco(&self, id: &SessionId) -> Result<Vec<dictar_domain::Utterance>> {
        let utterances = self.con_db(|db| db.transcripcion(id))?;
        let mut limpias: Vec<NewUtterance> = utterances
            .iter()
            .map(|u| NewUtterance {
                track: u.track,
                speaker: u.speaker.clone(),
                start_ms: u.start_ms,
                end_ms: u.end_ms,
                text: u.text.clone(),
                confidence: u.confidence,
                is_final: true,
            })
            .collect();
        let eco = Self::suprimir_diafonia(&mut limpias);
        if eco == 0 {
            return Ok(utterances);
        }
        tracing::info!(eco, "eco del micrófono eliminado de la transcripción");
        // Se reescribe sin pasar por `guardar_pasada_completa`: solo se quitan
        // frases y las etiquetas que quedan son las ya asignadas.
        self.con_db(|db| db.reemplazar_por_definitiva(id, &limpias))?;
        Ok(self.con_db(|db| db.transcripcion(id))?)
    }

    /// Cómo llamar a la otra parte en la transcripción.
    pub(crate) fn nombre_interlocutor(&self, sesion: &dictar_domain::Session) -> String {
        use dictar_domain::SessionKind;

        if let Some(t) = &sesion.topic_id {
            if let Ok(topic) = self.con_db(|db| db.topic(t)) {
                if let Some(p) = topic.person {
                    return p;
                }
            }
        }

        match sesion.kind {
            SessionKind::Class => "Profesor".to_owned(),
            SessionKind::ClientMeeting => "Cliente".to_owned(),
            _ => "Interlocutor".to_owned(),
        }
    }

    /// Contexto que se le da al modelo: asignatura y profesor, o cliente.
    fn contexto_de(&self, sesion: &dictar_domain::Session) -> String {
        if let Some(t) = &sesion.topic_id {
            if let Ok(topic) = self.con_db(|db| db.topic(t)) {
                return topic
                    .person
                    .map_or(topic.name.clone(), |p| format!("{} — {}", topic.name, p));
            }
        }
        sesion.title.clone().unwrap_or_default()
    }

    fn realimentar_glosario(&self, topic: &Option<dictar_domain::TopicId>, notas: &NotesPayload) {
        let Some(t) = topic else { return };
        let NotesPayload::Class(n) = notas else {
            return;
        };

        for termino in &n.glosario_nuevo {
            let _ = self.con_db(|db| {
                db.upsert_termino(
                    t,
                    &termino.termino,
                    termino.definicion.as_deref(),
                    GlossarySource::AutoRecurrent,
                )
            });
        }

        // También desde los conceptos extraídos. En la práctica el modelo
        // rellena «conceptos» de forma fiable y «glosario_nuevo» a veces lo
        // deja vacío, y sin términos el ciclo de mejora no arranca nunca: la
        // clase diez se transcribiría igual de mal que la primera.
        for c in &n.conceptos {
            if c.incierto {
                continue;
            }
            let _ = self.con_db(|db| {
                db.upsert_termino(
                    t,
                    &c.termino,
                    Some(&c.definicion),
                    GlossarySource::AutoRecurrent,
                )
            });
        }
    }

    /// Elimina las frases del micrófono que son eco de la pista del sistema.
    ///
    /// Devuelve cuántas se descartaron. Se conserva siempre la del sistema, que es
    /// la señal digital limpia; la del micro es la misma voz pasada por unos
    /// altavoces y una sala.
    fn suprimir_diafonia(frases: &mut Vec<NewUtterance>) -> usize {
        use dictar_stt::limpieza::similitud;

        // Margen temporal: el sonido tarda en llegar del altavoz al micrófono, y
        // los segmentos de cada pista no empiezan en el mismo instante.
        const MARGEN_MS: i64 = 4_000;
        const UMBRAL: f32 = 0.6;

        let sistema: Vec<(i64, i64, String)> = frases
            .iter()
            .filter(|u| u.track == Track::System)
            .map(|u| (u.start_ms, u.end_ms, u.text.clone()))
            .collect();

        if sistema.is_empty() {
            return 0;
        }

        let antes = frases.len();
        frases.retain(|u| {
            if u.track != Track::Mic {
                return true;
            }
            !sistema.iter().any(|(ini, fin, texto)| {
                let solapa = u.start_ms < fin + MARGEN_MS && u.end_ms + MARGEN_MS > *ini;
                solapa && similitud(&u.text, texto) >= UMBRAL
            })
        });

        antes - frases.len()
    }
}

// ---------------------------------------------------------------------------
// Diarización al cerrar una reunión
// ---------------------------------------------------------------------------

/// Archivo que deja la diarización en la carpeta de audio al terminar.
///
/// Vive junto al audio y no en la base porque es una marca de «este audio ya se
/// analizó» y se borra con la sesión sin migración alguna.
pub(crate) const MARCADOR_DIARIZACION: &str = "diarizacion.ok";

/// Motor de diarización ya cargado: recibe una pista mono a 16 kHz.
pub(crate) type MotorDiarizacion =
    Box<dyn Fn(&[f32]) -> std::result::Result<Vec<SegmentoHablante>, SttError>>;

/// Qué ocurrió con la diarización de una sesión. Nunca es un error de la
/// sesión: la diarización es un añadido y no puede tumbar el procesado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultadoDiarizacion {
    /// Solo las reuniones se diarizan; una clase o una nota de voz, no.
    NoAplica,
    /// Ya tenía etiquetas de una pasada anterior y no se repite.
    YaHecha,
    /// No hay frases que etiquetar.
    SinFrases,
    /// Faltan los modelos: la sesión sigue con las etiquetas de siempre.
    SinModelos,
    /// Falló el motor o la lectura del audio: igual, con las etiquetas de siempre.
    Fallo(String),
    /// Voces con frases en cada pista.
    Hecha {
        voces_sistema: usize,
        voces_microfono: usize,
    },
}

/// Si el tipo de sesión se diariza. Una clase tiene un profesor, y las notas de
/// voz una persona; el problema solo existe donde hablan varias.
pub(crate) fn diariza_este_tipo(tipo: SessionKind) -> bool {
    matches!(tipo, SessionKind::ClientMeeting | SessionKind::TeamMeeting)
}

fn abrir_motor_de_produccion(
    rutas: &RutasModelos,
) -> std::result::Result<MotorDiarizacion, SttError> {
    let diarizador = Diarizador::cargar(rutas, &OpcionesDiarizacion::produccion())?;
    Ok(Box::new(move |pcm| diarizador.diarizar(pcm)))
}

impl Nucleo {
    /// Identifica a los participantes de una reunión ya transcrita y reescribe
    /// el `speaker` de sus frases. Es lo que ejecuta el cierre; está público
    /// para el arnés de evaluación.
    pub fn diarizar_participantes(
        &self,
        id: &SessionId,
        progreso: Option<Sender<Evento>>,
    ) -> Result<ResultadoDiarizacion> {
        let sesion = self.con_db(|db| db.sesion(id))?;
        let emitir = |e: Evento| {
            if let Some(tx) = &progreso {
                let _ = tx.send(e);
            }
        };
        Ok(self.diarizar_sesion(id, &sesion, &emitir))
    }

    pub(crate) fn diarizar_sesion(
        &self,
        id: &SessionId,
        sesion: &dictar_domain::Session,
        emitir: &dyn Fn(Evento),
    ) -> ResultadoDiarizacion {
        self.diarizar_sesion_con(
            id,
            sesion,
            dictar_stt::diarizacion::rutas_modelos,
            abrir_motor_de_produccion,
            emitir,
        )
    }

    /// El cuerpo de la diarización, con la resolución de rutas y la carga del
    /// motor inyectables para poder probar los fallos sin modelos.
    pub(crate) fn diarizar_sesion_con(
        &self,
        id: &SessionId,
        sesion: &dictar_domain::Session,
        rutas: impl FnOnce() -> Option<RutasModelos>,
        abrir: impl FnOnce(&RutasModelos) -> std::result::Result<MotorDiarizacion, SttError>,
        emitir: &dyn Fn(Evento),
    ) -> ResultadoDiarizacion {
        let resultado = match self.intentar_diarizar(id, sesion, rutas, abrir, emitir) {
            Ok(r) => r,
            Err(error) => ResultadoDiarizacion::Fallo(error.to_string()),
        };
        // Un fallo no frena la sesión, pero tampoco se calla: la interfaz
        // muestra los eventos de error, y el usuario que esperaba ver a los
        // participantes separados tiene que saber por qué no los ve.
        match &resultado {
            ResultadoDiarizacion::SinModelos => {
                tracing::warn!(
                    sesion = %id,
                    "faltan los modelos de diarización; se conservan las etiquetas de siempre"
                );
                emitir(Evento::Error {
                    mensaje: "No se pudieron identificar los participantes: faltan los \
                              modelos de diarización. La reunión se procesa igualmente."
                        .to_owned(),
                    recuperable: true,
                });
            }
            ResultadoDiarizacion::Fallo(motivo) => {
                tracing::warn!(
                    sesion = %id,
                    motivo,
                    "la diarización falló; se conservan las etiquetas de siempre"
                );
                emitir(Evento::Error {
                    mensaje: format!(
                        "No se pudieron identificar los participantes: {motivo}. \
                         La reunión se procesa igualmente."
                    ),
                    recuperable: true,
                });
            }
            ResultadoDiarizacion::Hecha {
                voces_sistema,
                voces_microfono,
            } => tracing::info!(
                sesion = %id,
                voces_sistema,
                voces_microfono,
                "participantes identificados"
            ),
            _ => {}
        }
        resultado
    }

    fn intentar_diarizar(
        &self,
        id: &SessionId,
        sesion: &dictar_domain::Session,
        rutas: impl FnOnce() -> Option<RutasModelos>,
        abrir: impl FnOnce(&RutasModelos) -> std::result::Result<MotorDiarizacion, SttError>,
        emitir: &dyn Fn(Evento),
    ) -> Result<ResultadoDiarizacion> {
        if !diariza_este_tipo(sesion.kind) {
            return Ok(ResultadoDiarizacion::NoAplica);
        }
        let dir = self.dir_audio(id);
        if dir.join(MARCADOR_DIARIZACION).is_file() {
            return Ok(ResultadoDiarizacion::YaHecha);
        }
        let Some(rutas) = rutas() else {
            return Ok(ResultadoDiarizacion::SinModelos);
        };

        // Sin el eco: ver `transcripcion_sin_eco`.
        let frases = self.transcripcion_sin_eco(id)?;
        if frases.is_empty() {
            return Ok(ResultadoDiarizacion::SinFrases);
        }

        emitir(Evento::progreso(
            FaseProceso::Identificando,
            0.0,
            "cargando los modelos de voz",
        ));
        let motor = abrir(&rutas)?;

        let pistas = [
            (Track::System, self.nombre_interlocutor(sesion)),
            (Track::Mic, "Yo".to_owned()),
        ];
        let mut cambios: Vec<(i64, String)> = Vec::new();
        let mut voces = [0usize; 2];
        for (i, (pista, por_defecto)) in pistas.iter().enumerate() {
            let propias: Vec<_> = frases.iter().filter(|u| u.track == *pista).collect();
            // Un micrófono sin frases es un micrófono sin voz propia: no se
            // gasta tiempo en diarizarlo.
            if propias.is_empty() {
                continue;
            }
            // Con altavoces, el micrófono lleva también las voces del sistema.
            // Medido en una reunión de 57 min con retorno 0,709: 13 «Persona en
            // sala» con frases, con o sin atenuar. Mejor ninguna que inventadas:
            // todo lo del micrófono queda como «Yo».
            if *pista == Track::Mic && Self::microfono_con_altavoces(&dir) {
                tracing::info!(
                    "retorno acústico detectado: el micrófono no se diariza, todo queda como «Yo»"
                );
                voces[i] = 1;
                for frase in &propias {
                    if frase.speaker.as_deref() != Some(por_defecto.as_str()) {
                        cambios.push((frase.id, por_defecto.clone()));
                    }
                }
                continue;
            }
            let ruta = dir.join(format!("{}.wav", pista.as_str()));
            if !ruta.is_file() {
                tracing::warn!(
                    pista = pista.as_str(),
                    "no hay WAV de la pista; se deja sin diarizar"
                );
                continue;
            }
            emitir(Evento::progreso(
                FaseProceso::Identificando,
                i as f32 / pistas.len() as f32,
                format!("analizando las voces de la pista {}", pista.as_str()),
            ));

            let (pcm, _) = leer_wav(&ruta)?;
            let segmentos = motor(&pcm)?;
            drop(pcm);

            let tramos: Vec<_> = propias.iter().map(|u| (u.start_ms, u.end_ms)).collect();
            let etiquetado = etiquetar_frases(*pista, &tramos, &segmentos, por_defecto);
            voces[i] = etiquetado.voces;
            for (frase, etiqueta) in propias.iter().zip(etiquetado.etiquetas) {
                if frase.speaker.as_deref() != Some(etiqueta.as_str()) {
                    cambios.push((frase.id, etiqueta));
                }
            }
        }

        // Todo o nada: los cambios se aplican una vez analizadas las dos
        // pistas, para que un fallo en la segunda no deje la sesión a medias.
        self.con_db(|db| db.asignar_hablantes(&cambios))?;
        if let Err(error) = std::fs::write(dir.join(MARCADOR_DIARIZACION), b"ok\n") {
            // Sin marcador, la próxima vez se rehará: pierde tiempo, no datos.
            tracing::warn!(%error, "no se pudo escribir el marcador de diarización");
        }
        Ok(ResultadoDiarizacion::Hecha {
            voces_sistema: voces[0],
            voces_microfono: voces[1],
        })
    }

    /// Si el micrófono recoge a los altavoces, con el mismo detector que la
    /// transcripción por lotes. Es el punto de decisión de si se diariza el
    /// micrófono. Si no se puede analizar se diariza: sin evidencia de altavoces
    /// no hay motivo para renunciar a separar a las personas de la sala.
    pub(crate) fn microfono_con_altavoces(dir: &std::path::Path) -> bool {
        let mic = dir.join("mic.wav");
        let sistema = dir.join("system.wav");
        if !mic.is_file() || !sistema.is_file() {
            return false;
        }
        match dictar_audio::detectar_retorno_en_wav(&mic, &sistema) {
            Ok(retorno) => retorno.is_some(),
            Err(error) => {
                tracing::warn!(%error, "no se pudo analizar el retorno acústico");
                false
            }
        }
    }

    /// Borra el marcador de diarización. Ver [`Self::guardar_pasada_completa`].
    pub(crate) fn invalidar_diarizacion(&self, id: &SessionId) {
        let marcador = self.dir_audio(id).join(MARCADOR_DIARIZACION);
        match std::fs::remove_file(&marcador) {
            Ok(()) => {
                tracing::info!(sesion = %id, "diarización invalidada por una pasada completa")
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                %error,
                "no se pudo borrar el marcador de diarización; las etiquetas pueden desfasarse"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dictar_domain::{Session, SessionKind, SessionSource, Topic, TopicKind};

    fn nucleo() -> (Nucleo, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (Nucleo::abrir(dir.path()).unwrap(), dir)
    }

    #[test]
    fn el_interlocutor_toma_el_nombre_del_profesor() {
        let (n, _d) = nucleo();

        let mut t = Topic::nuevo(TopicKind::Course, "Cálculo II");
        t.person = Some("Dra. Pérez".into());
        n.crear_topic(&t).unwrap();

        let mut s = Session::nueva(SessionKind::Class, SessionSource::DesktopLoopback, 0);
        s.topic_id = Some(t.id.clone());

        assert_eq!(n.nombre_interlocutor(&s), "Dra. Pérez");
    }

    #[test]
    fn sin_topic_se_usa_un_nombre_segun_el_tipo_de_sesion() {
        let (n, _d) = nucleo();

        let clase = Session::nueva(SessionKind::Class, SessionSource::DesktopLoopback, 0);
        assert_eq!(n.nombre_interlocutor(&clase), "Profesor");

        let cliente = Session::nueva(
            SessionKind::ClientMeeting,
            SessionSource::DesktopLoopback,
            0,
        );
        assert_eq!(n.nombre_interlocutor(&cliente), "Cliente");
    }

    #[test]
    fn el_contexto_combina_asignatura_y_profesor() {
        let (n, _d) = nucleo();

        let mut t = Topic::nuevo(TopicKind::Course, "Cálculo II");
        t.person = Some("Dra. Pérez".into());
        n.crear_topic(&t).unwrap();

        let mut s = Session::nueva(SessionKind::Class, SessionSource::DesktopLoopback, 0);
        s.topic_id = Some(t.id.clone());

        assert_eq!(n.contexto_de(&s), "Cálculo II — Dra. Pérez");
    }

    #[tokio::test]
    async fn una_sesion_sin_audio_falla_con_un_mensaje_util() {
        let (n, _d) = nucleo();
        let s = Session::nueva(SessionKind::Class, SessionSource::DesktopLoopback, 0);
        n.con_db(|db| db.crear_sesion(&s)).unwrap();

        let r = n.procesar_sesion(&s.id, Modelo::Small, 0, None).await;

        assert!(r.is_err());
        // Y la sesión queda marcada como fallida, no colgada en «transcribing».
        assert_eq!(
            n.sesion(&s.id).unwrap().status,
            dictar_domain::SessionStatus::Failed
        );
    }

    #[test]
    fn el_glosario_nuevo_se_realimenta_al_topic() {
        use dictar_domain::notes::{ClassNotes, TerminoNuevo};

        let (n, _d) = nucleo();
        let t = Topic::nuevo(TopicKind::Course, "Cálculo II");
        n.crear_topic(&t).unwrap();

        let notas = NotesPayload::Class(ClassNotes {
            tldr: "x".into(),
            temas: vec![],
            conceptos: vec![],
            formulas: vec![],
            avisos: vec![],
            senales_examen: vec![],
            dudas: vec![],
            bibliografia: vec![],
            glosario_nuevo: vec![TerminoNuevo {
                termino: "región de convergencia".into(),
                definicion: Some("donde la integral converge".into()),
            }],
        });

        n.realimentar_glosario(&Some(t.id.clone()), &notas);

        // Es el ciclo que hace que la clase siguiente transcriba mejor.
        let g = n.glosario(&t.id).unwrap();
        assert!(g.contains("región de convergencia"), "glosario: {g}");
    }
}

#[cfg(test)]
mod tests_cobertura {
    use super::*;

    #[test]
    fn sistema_cubierto_con_voz_en_el_microfono_solo_retranscribe_el_microfono() {
        // Antes, cualquier voz propia en el micrófono rehacía toda la clase con
        // el modelo grande, pese a que el sistema ya estaba cubierto en vivo.
        let cobertura = !falta_cobertura(Some(600_000), 598_000);
        let plan = decidir_cierre(cobertura, true);
        assert_eq!(plan, PlanCierre::SoloMicrofono);
        assert_eq!(pistas_del_lote(Some(Track::Mic)), vec![Track::Mic]);
    }

    #[test]
    fn la_cobertura_insuficiente_sigue_usando_la_pasada_completa() {
        let cobertura = !falta_cobertura(Some(600_000), 300_000);
        assert_eq!(decidir_cierre(cobertura, true), PlanCierre::Completa);
        assert_eq!(decidir_cierre(cobertura, false), PlanCierre::Completa);
        // Duración desconocida: tampoco se da por cubierta.
        assert_eq!(decidir_cierre(false, false), PlanCierre::Completa);
        assert_eq!(pistas_del_lote(None), vec![Track::Mic, Track::System]);
    }

    #[test]
    fn con_marcador_de_microfono_en_vivo_solo_se_confirma_sin_leer_el_wav() {
        // Antes, el micrófono nunca tenía frases en vivo y el cierre lo
        // transcribía por lotes aunque el motor incremental lo hubiera cubierto.
        let falta = falta_microfono_tras_vivo(true, || {
            panic!("con marcador no debe comprobarse la voz del WAV")
        });
        assert!(!falta);
        assert_eq!(decidir_cierre(true, falta), PlanCierre::Confirmar);
    }

    #[test]
    fn sin_marcador_y_con_voz_en_el_microfono_sigue_el_plan_solo_microfono() {
        let falta = falta_microfono_tras_vivo(false, || Ok(true));
        assert!(falta);
        assert_eq!(decidir_cierre(true, falta), PlanCierre::SoloMicrofono);
    }

    #[test]
    fn sin_marcador_y_sin_poder_comprobar_el_microfono_se_asume_que_falta() {
        let falta = falta_microfono_tras_vivo(false, || Err(ApiError::Config("x".into())));
        assert!(falta);
    }

    #[test]
    fn sin_voz_pendiente_en_el_microfono_solo_se_confirma() {
        assert_eq!(decidir_cierre(true, false), PlanCierre::Confirmar);
    }

    #[test]
    fn el_silencio_de_la_despedida_no_obliga_a_retranscribir() {
        // Nadie habla mientras se despiden y cierran la llamada.
        assert!(!falta_cobertura(Some(7_200_000), 7_150_000));
    }

    #[test]
    fn media_clase_sin_cubrir_se_detecta() {
        // El modelo se cayó a mitad de clase: sin esto, las notas cubrirían
        // solo la primera hora y nadie se enteraría.
        assert!(falta_cobertura(Some(7_200_000), 3_600_000));
    }

    #[test]
    fn sin_duracion_conocida_no_se_rehace_nada() {
        assert!(!falta_cobertura(None, 0));
    }

    #[test]
    fn una_cobertura_completa_no_dispara_nada() {
        assert!(!falta_cobertura(Some(3_600_000), 3_600_000));
    }
}

#[cfg(test)]
mod tests_diarizacion {
    use super::*;
    use dictar_domain::{Session, SessionSource};
    use dictar_stt::diarizacion::MODELO_EMBEDDING;
    use std::cell::{Cell, RefCell};
    use std::path::Path;

    const SEG: usize = 16_000;

    fn nucleo() -> (Nucleo, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (Nucleo::abrir(dir.path()).unwrap(), dir)
    }

    /// WAV PCM de 16 bits, mono, 16 kHz, en silencio. Lo único que se mira de él
    /// es su duración: la voz la pone el motor de pruebas.
    fn escribir_wav(ruta: &Path, segundos: usize) {
        let datos = (segundos * SEG * 2) as u32;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + datos).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&16_000u32.to_le_bytes());
        b.extend_from_slice(&32_000u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&datos.to_le_bytes());
        b.resize(b.len() + datos as usize, 0);
        std::fs::write(ruta, b).unwrap();
    }

    fn frase(track: Track, ini: i64, fin: i64, texto: &str, quien: &str) -> NewUtterance {
        NewUtterance {
            track,
            speaker: Some(quien.to_owned()),
            start_ms: ini,
            end_ms: fin,
            text: texto.to_owned(),
            confidence: Some(0.9),
            is_final: true,
        }
    }

    fn seg(inicio_ms: i64, fin_ms: i64, hablante: usize) -> SegmentoHablante {
        SegmentoHablante {
            inicio_ms,
            fin_ms,
            hablante,
        }
    }

    /// Reunión con cliente con 40 s de sistema y 20 s de micrófono, y las frases
    /// que dejaría la transcripción: etiquetadas como hoy, «Cliente» y «Yo».
    fn reunion(tipo: SessionKind) -> (Nucleo, tempfile::TempDir, Session) {
        let (n, d) = nucleo();
        let s = Session::nueva(tipo, SessionSource::DesktopLoopback, 0);
        n.con_db(|db| db.crear_sesion(&s)).unwrap();
        let dir = n.dir_audio(&s.id);
        std::fs::create_dir_all(&dir).unwrap();
        escribir_wav(&dir.join("system.wav"), 40);
        escribir_wav(&dir.join("mic.wav"), 20);
        let defecto = n.nombre_interlocutor(&s);
        n.con_db(|db| {
            db.insertar_frases(
                &s.id,
                &[
                    frase(Track::System, 1_000, 5_000, "hola, buenos días", &defecto),
                    frase(Track::System, 12_000, 16_000, "vamos a empezar", &defecto),
                    frase(
                        Track::System,
                        22_000,
                        26_000,
                        "pasemos al presupuesto",
                        &defecto,
                    ),
                    frase(Track::Mic, 2_000, 6_000, "gracias por venir", "Yo"),
                    frase(Track::Mic, 8_000, 12_000, "empiezo yo con el resumen", "Yo"),
                ],
            )
        })
        .unwrap();
        (n, d, s)
    }

    /// Motor de pruebas: cuatro voces repartidas entre las dos pistas. Distingue
    /// la pista por su duración (el sistema dura 40 s y el micrófono 20 s).
    fn motor_con_dos_voces_por_pista() -> MotorDiarizacion {
        Box::new(|pcm: &[f32]| {
            Ok(if pcm.len() == 40 * SEG {
                vec![
                    seg(0, 10_000, 0),
                    seg(10_000, 20_000, 1),
                    seg(20_000, 30_000, 0),
                ]
            } else {
                vec![seg(0, 7_000, 3), seg(7_000, 19_000, 5)]
            })
        })
    }

    fn hablantes(n: &Nucleo, s: &Session) -> Vec<(Track, String)> {
        n.transcripcion(&s.id)
            .unwrap()
            .into_iter()
            .map(|u| (u.track, u.speaker.unwrap_or_default()))
            .collect()
    }

    fn con_rutas_falsas() -> Option<RutasModelos> {
        Some(RutasModelos {
            segmentacion: MODELO_EMBEDDING.into(),
            embedding: MODELO_EMBEDDING.into(),
        })
    }

    fn diarizar(
        n: &Nucleo,
        s: &Session,
        abrir: impl FnOnce(&RutasModelos) -> std::result::Result<MotorDiarizacion, SttError>,
    ) -> (ResultadoDiarizacion, Vec<Evento>) {
        let eventos = RefCell::new(Vec::new());
        let emitir = |e: Evento| eventos.borrow_mut().push(e);
        let r = n.diarizar_sesion_con(&s.id, s, con_rutas_falsas, abrir, &emitir);
        (r, eventos.into_inner())
    }

    #[test]
    fn varias_voces_por_canal_se_etiquetan_y_dejan_marcador() {
        let (n, _d, s) = reunion(SessionKind::ClientMeeting);

        let (r, eventos) = diarizar(&n, &s, |_| Ok(motor_con_dos_voces_por_pista()));

        assert_eq!(
            r,
            ResultadoDiarizacion::Hecha {
                voces_sistema: 2,
                voces_microfono: 2
            }
        );
        // Sistema: la voz 0 abre y cierra, la 1 está en medio. Micrófono: la
        // voz 5 habla 12 s y es «Yo»; la 3 (7 s) es una persona en sala.
        let esperado: Vec<(Track, &str)> = vec![
            (Track::System, "Participante 1"),
            (Track::Mic, "Persona en sala 1"),
            (Track::Mic, "Yo"),
            (Track::System, "Participante 2"),
            (Track::System, "Participante 1"),
        ];
        let obtenido = hablantes(&n, &s);
        let obtenido: Vec<(Track, &str)> = obtenido.iter().map(|(t, q)| (*t, q.as_str())).collect();
        assert_eq!(obtenido, esperado);
        assert!(n.dir_audio(&s.id).join(MARCADOR_DIARIZACION).is_file());
        // Y avisa de que está trabajando: sin fase propia la barra parecería colgada.
        assert!(eventos.iter().any(|e| matches!(
            e,
            Evento::Progreso {
                fase: FaseProceso::Identificando,
                ..
            }
        )));
    }

    #[test]
    fn una_voz_por_canal_conserva_las_etiquetas_de_siempre() {
        let (n, _d, s) = reunion(SessionKind::ClientMeeting);

        let (r, _) = diarizar(&n, &s, |_| {
            Ok(Box::new(|pcm: &[f32]| {
                Ok(vec![seg(0, (pcm.len() * 1000 / SEG) as i64, 0)])
            }))
        });

        assert_eq!(
            r,
            ResultadoDiarizacion::Hecha {
                voces_sistema: 1,
                voces_microfono: 1
            }
        );
        for (pista, quien) in hablantes(&n, &s) {
            let esperado = if pista == Track::Mic { "Yo" } else { "Cliente" };
            assert_eq!(quien, esperado);
        }
    }

    #[test]
    fn sin_modelos_la_sesion_sigue_con_las_etiquetas_de_hoy_y_se_avisa() {
        let (n, _d, s) = reunion(SessionKind::ClientMeeting);
        let antes = hablantes(&n, &s);

        let eventos = RefCell::new(Vec::new());
        let emitir = |e: Evento| eventos.borrow_mut().push(e);
        let r = n.diarizar_sesion_con(
            &s.id,
            &s,
            || None,
            |_| panic!("sin rutas no se debe abrir ningún motor"),
            &emitir,
        );

        assert_eq!(r, ResultadoDiarizacion::SinModelos);
        assert_eq!(hablantes(&n, &s), antes);
        assert!(!n.dir_audio(&s.id).join(MARCADOR_DIARIZACION).exists());
        // El fallo se dice: antes de este aviso un instalador sin modelos
        // habría dado reuniones sin separar y ninguna pista de por qué.
        assert!(eventos.borrow().iter().any(|e| matches!(
            e,
            Evento::Error {
                recuperable: true,
                ..
            }
        )));
    }

    #[test]
    fn un_motor_que_no_carga_deja_las_etiquetas_de_hoy() {
        let (n, _d, s) = reunion(SessionKind::TeamMeeting);
        let antes = hablantes(&n, &s);

        let (r, eventos) = diarizar(&n, &s, |_| Err(SttError::Carga("modelo corrupto".into())));

        assert!(matches!(&r, ResultadoDiarizacion::Fallo(m) if m.contains("modelo corrupto")));
        assert_eq!(hablantes(&n, &s), antes);
        assert!(eventos.iter().any(|e| matches!(e, Evento::Error { .. })));
    }

    #[test]
    fn un_fallo_en_el_microfono_no_deja_la_sesion_a_medias() {
        // El sistema se diariza bien y el micrófono falla: no debe quedar
        // «Participante 1» en una pista y «Yo» en la otra con un marcador que
        // diga que está hecho.
        let (n, _d, s) = reunion(SessionKind::ClientMeeting);
        let antes = hablantes(&n, &s);

        let (r, _) = diarizar(&n, &s, |_| {
            Ok(Box::new(|pcm: &[f32]| {
                if pcm.len() == 40 * SEG {
                    Ok(vec![seg(0, 10_000, 0), seg(10_000, 20_000, 1)])
                } else {
                    Err(SttError::Transcripcion("sin memoria".into()))
                }
            }))
        });

        assert!(matches!(r, ResultadoDiarizacion::Fallo(_)));
        assert_eq!(hablantes(&n, &s), antes);
        assert!(!n.dir_audio(&s.id).join(MARCADOR_DIARIZACION).exists());
    }

    #[test]
    fn una_clase_o_una_nota_de_voz_no_se_diarizan() {
        for tipo in [SessionKind::Class, SessionKind::VoiceNote] {
            let (n, _d, s) = reunion(tipo);
            let antes = hablantes(&n, &s);

            let (r, _) = diarizar(&n, &s, |_| panic!("no debe cargarse el motor"));

            assert_eq!(r, ResultadoDiarizacion::NoAplica);
            assert_eq!(hablantes(&n, &s), antes);
        }
    }

    #[test]
    fn una_sesion_ya_diarizada_no_se_rehace() {
        let (n, _d, s) = reunion(SessionKind::ClientMeeting);
        diarizar(&n, &s, |_| Ok(motor_con_dos_voces_por_pista()));
        // El usuario renombra a un participante.
        n.renombrar_hablante(&s.id, Track::System, "Participante 2", "Carlos (socio)")
            .unwrap();
        let antes = hablantes(&n, &s);

        let cargas = Cell::new(0);
        let (r, _) = diarizar(&n, &s, |_| {
            cargas.set(cargas.get() + 1);
            Ok(motor_con_dos_voces_por_pista())
        });

        // Rehacerla borraría el nombre que puso.
        assert_eq!(r, ResultadoDiarizacion::YaHecha);
        assert_eq!(cargas.get(), 0);
        assert_eq!(hablantes(&n, &s), antes);
        assert!(antes.iter().any(|(_, q)| q == "Carlos (socio)"));
    }

    #[test]
    fn una_pasada_completa_invalida_el_marcador_y_la_siguiente_diarizacion_rehace() {
        let (n, _d, s) = reunion(SessionKind::ClientMeeting);
        diarizar(&n, &s, |_| Ok(motor_con_dos_voces_por_pista()));
        let marcador = n.dir_audio(&s.id).join(MARCADOR_DIARIZACION);
        assert!(marcador.is_file());

        // La pasada de respaldo reescribe todas las frases, con las etiquetas
        // por defecto. Si el marcador sobreviviera, la reunión se quedaría para
        // siempre con «Cliente» y «Yo» creyendo que ya está identificada.
        n.guardar_pasada_completa(
            &s.id,
            &[
                frase(Track::System, 1_000, 5_000, "hola de nuevo", "Cliente"),
                frase(Track::System, 12_000, 16_000, "seguimos", "Cliente"),
            ],
        )
        .unwrap();
        assert!(!marcador.exists());

        let (r, _) = diarizar(&n, &s, |_| Ok(motor_con_dos_voces_por_pista()));
        assert!(matches!(r, ResultadoDiarizacion::Hecha { .. }));
        let h = hablantes(&n, &s);
        assert_eq!(h[0].1, "Participante 1");
        assert_eq!(h[1].1, "Participante 2");
        assert!(marcador.is_file());
    }

    #[test]
    fn la_pasada_que_solo_rehace_el_microfono_tambien_invalida_el_marcador() {
        let (n, _d, s) = reunion(SessionKind::ClientMeeting);
        diarizar(&n, &s, |_| Ok(motor_con_dos_voces_por_pista()));
        let marcador = n.dir_audio(&s.id).join(MARCADOR_DIARIZACION);

        n.guardar_pasada_de_microfono(
            &s.id,
            &[frase(Track::Mic, 2_000, 6_000, "gracias de nuevo", "Yo")],
        )
        .unwrap();

        assert!(!marcador.exists());
    }

    #[test]
    fn el_eco_del_microfono_no_cuenta_como_una_voz_mas() {
        // El micrófono capta por los altavoces la misma frase que el sistema.
        // Si se diarizara antes de quitar ese eco, la frase repetida pondría una
        // «Persona en sala» que luego desaparece del texto.
        let (n, _d, s) = reunion(SessionKind::ClientMeeting);
        n.con_db(|db| {
            db.insertar_frases(
                &s.id,
                &[frase(Track::Mic, 12_500, 16_500, "vamos a empezar", "Yo")],
            )
        })
        .unwrap();

        let (r, _) = diarizar(&n, &s, |_| {
            Ok(Box::new(|pcm: &[f32]| {
                Ok(if pcm.len() == 40 * SEG {
                    vec![seg(0, 40_000, 0)]
                } else {
                    // La voz 7 solo existe en el tramo del eco.
                    vec![seg(0, 12_000, 2), seg(12_000, 20_000, 7)]
                })
            }))
        });

        assert_eq!(
            r,
            ResultadoDiarizacion::Hecha {
                voces_sistema: 1,
                voces_microfono: 1
            }
        );
        let h = hablantes(&n, &s);
        assert!(h.iter().all(|(_, q)| !q.starts_with("Persona en sala")));
        assert_eq!(h.len(), 5, "la frase de eco ya no está");
    }

    /// WAV cuya envolvente (un valor constante por bloque de 20 ms) sigue
    /// `amplitudes`. El detector de retorno solo mira envolventes RMS.
    fn escribir_envolvente(ruta: &Path, amplitudes: &[f32]) {
        let mut b = Vec::new();
        let datos = (amplitudes.len() * 320 * 2) as u32;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + datos).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&16_000u32.to_le_bytes());
        b.extend_from_slice(&32_000u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&datos.to_le_bytes());
        for a in amplitudes {
            let muestra = (a * 32_000.0) as i16;
            for _ in 0..320 {
                b.extend_from_slice(&muestra.to_le_bytes());
            }
        }
        std::fs::write(ruta, b).unwrap();
    }

    /// Secuencia pseudoaleatoria fija de amplitudes entre 0,05 y 0,5.
    fn amplitudes(semilla: u32, n: usize) -> Vec<f32> {
        let mut x = semilla;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                0.05 + (x >> 16) as f32 / 65_536.0 * 0.45
            })
            .collect()
    }

    /// Reunión con 20 s de sistema y un micrófono que es, o no, copia retrasada
    /// 120 ms del sistema. Las frases del micrófono llevan una etiqueta que no
    /// es «Yo» para ver si se reescribe.
    fn reunion_con_microfono(copia_del_sistema: bool) -> (Nucleo, tempfile::TempDir, Session) {
        let (n, d) = nucleo();
        let s = Session::nueva(
            SessionKind::ClientMeeting,
            SessionSource::DesktopLoopback,
            0,
        );
        n.con_db(|db| db.crear_sesion(&s)).unwrap();
        let dir = n.dir_audio(&s.id);
        std::fs::create_dir_all(&dir).unwrap();
        let sistema = amplitudes(7, 1000);
        let mic: Vec<f32> = if copia_del_sistema {
            (0..1000)
                .map(|i| if i >= 6 { sistema[i - 6] * 0.5 } else { 0.0 })
                .collect()
        } else {
            amplitudes(99, 1000)
        };
        escribir_envolvente(&dir.join("system.wav"), &sistema);
        escribir_envolvente(&dir.join("mic.wav"), &mic);
        n.con_db(|db| {
            db.insertar_frases(
                &s.id,
                &[
                    frase(Track::System, 1_000, 5_000, "hola, buenos días", "Cliente"),
                    frase(
                        Track::Mic,
                        2_000,
                        6_000,
                        "gracias por venir",
                        "Persona rara",
                    ),
                    frase(Track::Mic, 8_000, 12_000, "empiezo yo con el resumen", "Yo"),
                ],
            )
        })
        .unwrap();
        (n, d, s)
    }

    /// Motor de pruebas que ve dos voces en cualquier pista (las dos duran 20 s):
    /// la 1 con 7 s y la 2 con 12 s.
    fn motor_que_separa_dos_voces_del_micro() -> MotorDiarizacion {
        Box::new(|_: &[f32]| Ok(vec![seg(0, 7_000, 1), seg(7_000, 19_000, 2)]))
    }

    #[test]
    fn con_altavoces_el_microfono_no_se_diariza_y_todo_queda_como_yo() {
        // Medido: con retorno 0,709 aparecían 13 «Persona en sala» en un
        // micrófono que solo recogía los altavoces y a una persona.
        let (n, _d, s) = reunion_con_microfono(true);
        assert!(Nucleo::microfono_con_altavoces(&n.dir_audio(&s.id)));

        let (r, _) = diarizar(&n, &s, |_| Ok(motor_que_separa_dos_voces_del_micro()));

        assert_eq!(
            r,
            ResultadoDiarizacion::Hecha {
                voces_sistema: 1,
                voces_microfono: 1
            }
        );
        let h = hablantes(&n, &s);
        assert!(h
            .iter()
            .filter(|(t, _)| *t == Track::Mic)
            .all(|(_, q)| q == "Yo"));
    }

    #[test]
    fn sin_altavoces_el_microfono_se_diariza_como_siempre() {
        let (n, _d, s) = reunion_con_microfono(false);
        assert!(!Nucleo::microfono_con_altavoces(&n.dir_audio(&s.id)));

        let (r, _) = diarizar(&n, &s, |_| Ok(motor_que_separa_dos_voces_del_micro()));

        assert_eq!(
            r,
            ResultadoDiarizacion::Hecha {
                voces_sistema: 1,
                voces_microfono: 2
            }
        );
        let mic: Vec<String> = hablantes(&n, &s)
            .into_iter()
            .filter(|(t, _)| *t == Track::Mic)
            .map(|(_, q)| q)
            .collect();
        assert_eq!(mic, ["Persona en sala 1", "Yo"]);
    }

    #[test]
    fn diariza_este_tipo_solo_acepta_reuniones() {
        assert!(diariza_este_tipo(SessionKind::ClientMeeting));
        assert!(diariza_este_tipo(SessionKind::TeamMeeting));
        assert!(!diariza_este_tipo(SessionKind::Class));
        assert!(!diariza_este_tipo(SessionKind::VoiceNote));
    }
}

#[cfg(test)]
mod tests_regenerar {
    use super::*;
    use dictar_domain::notes::ClassNotes;
    use dictar_domain::{NoteTemplate, Session, SessionSource};
    use dictar_providers::config::ProvidersFile;
    use dictar_storage::notes_store::NewNotes;
    use std::sync::mpsc;

    fn frase(track: Track, ini: i64, texto: &str, quien: &str) -> NewUtterance {
        NewUtterance {
            track,
            speaker: Some(quien.to_owned()),
            start_ms: ini,
            end_ms: ini + 3_000,
            text: texto.to_owned(),
            confidence: Some(0.9),
            is_final: true,
        }
    }

    /// Un núcleo cuyo único proveedor apunta a un puerto donde no escucha nadie:
    /// la generación falla enseguida y, sobre todo, la prueba no puede gastar
    /// dinero aunque la máquina tenga claves de API configuradas.
    fn nucleo_sin_modelo_de_lenguaje() -> (Nucleo, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut n = Nucleo::abrir(dir.path()).unwrap();
        n.config = ProvidersFile::desde_toml(
            r#"
[[provider]]
id = "sin_servidor"
kind = "openai_compat"
base_url = "http://127.0.0.1:9/v1"
model = "ninguno"
"#,
        )
        .unwrap();
        (n, dir)
    }

    fn notas(tldr: &str, creada: i64) -> NewNotes {
        let payload = NotesPayload::Class(ClassNotes {
            tldr: tldr.to_owned(),
            temas: vec![],
            conceptos: vec![],
            formulas: vec![],
            avisos: vec![],
            senales_examen: vec![],
            dudas: vec![],
            bibliografia: vec![],
            glosario_nuevo: vec![],
        });
        NewNotes {
            template: NoteTemplate::Class,
            kind: "final".to_owned(),
            seq: None,
            provider: "p".to_owned(),
            model: "m".to_owned(),
            prompt_ver: "v".to_owned(),
            payload: serde_json::to_string(&payload).unwrap(),
            tokens_in: None,
            tokens_out: None,
            cost_usd: Some(0.01),
            created_at: creada,
        }
    }

    #[tokio::test]
    async fn regenerar_las_notas_no_transcribe_no_diariza_y_no_toca_las_frases() {
        let (n, _d) = nucleo_sin_modelo_de_lenguaje();
        let s = Session::nueva(SessionKind::TeamMeeting, SessionSource::DesktopLoopback, 0);
        n.con_db(|db| db.crear_sesion(&s)).unwrap();
        n.con_db(|db| {
            db.insertar_frases(
                &s.id,
                &[
                    frase(Track::System, 0, "buenos días", "Carlos (socio)"),
                    frase(Track::Mic, 4_000, "empezamos", "Yo"),
                ],
            )
        })
        .unwrap();
        let antes: Vec<_> = n
            .transcripcion(&s.id)
            .unwrap()
            .into_iter()
            .map(|u| (u.text, u.speaker))
            .collect();

        let (tx, rx) = mpsc::channel();
        let r = n.regenerar_notas(&s.id, 1_000, Some(tx)).await;

        // Sin modelo de lenguaje la generación falla; lo que importa es lo que
        // hizo antes de fallar, que es exactamente nada más que ir a las notas.
        assert!(r.is_err());
        let fases: Vec<FaseProceso> = rx
            .try_iter()
            .filter_map(|e| match e {
                Evento::Progreso { fase, .. } => Some(fase),
                _ => None,
            })
            .collect();
        assert_eq!(fases, [FaseProceso::GenerandoNotas]);
        let despues: Vec<_> = n
            .transcripcion(&s.id)
            .unwrap()
            .into_iter()
            .map(|u| (u.text, u.speaker))
            .collect();
        assert_eq!(despues, antes, "los nombres puestos a mano no se tocan");
        assert!(!n.dir_audio(&s.id).join(MARCADOR_DIARIZACION).exists());
    }

    #[tokio::test]
    async fn regenerar_sin_transcripcion_dice_por_que_no_puede() {
        let (n, _d) = nucleo_sin_modelo_de_lenguaje();
        let s = Session::nueva(SessionKind::TeamMeeting, SessionSource::DesktopLoopback, 0);
        n.con_db(|db| db.crear_sesion(&s)).unwrap();

        let e = n.regenerar_notas(&s.id, 1_000, None).await.unwrap_err();

        assert!(e.to_string().contains("no tiene transcripción"), "{e}");
    }

    #[test]
    fn las_notas_regeneradas_sustituyen_a_las_anteriores_al_leerlas() {
        let (n, _d) = nucleo_sin_modelo_de_lenguaje();
        let s = Session::nueva(SessionKind::Class, SessionSource::DesktopLoopback, 0);
        n.con_db(|db| db.crear_sesion(&s)).unwrap();

        n.con_db(|db| db.guardar_notas(&s.id, &notas("RESUMEN VIEJO", 1_000)))
            .unwrap();
        n.con_db(|db| db.guardar_notas(&s.id, &notas("RESUMEN NUEVO", 2_000)))
            .unwrap();

        let md = n.notas_markdown(&s.id).unwrap().unwrap();
        assert!(md.contains("RESUMEN NUEVO"));
        assert!(!md.contains("RESUMEN VIEJO"));
        // Las dos generaciones cuestan dinero y las dos se cuentan.
        assert_eq!(n.coste_sesion(&s.id).unwrap(), 0.02);
    }
}
