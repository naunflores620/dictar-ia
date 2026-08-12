//! Procesado de una sesión: de los WAV a los apuntes.
//!
//! Se ejecuta en segundo plano y por lotes, no durante la clase. Con 4,6×
//! tiempo real medido, diez horas de clase son unas dos horas de trabajo: se
//! lanza al cerrar el portátil y por la mañana están los apuntes.

use crate::eventos::{Evento, FaseProceso};
use crate::{ApiError, Nucleo, Result};
use dictar_audio::wav::leer_wav;
use dictar_domain::alert;
use dictar_domain::notes::NotesPayload;
use dictar_domain::transcript::GlossarySource;
use dictar_domain::{EpochMs, SessionId, SessionStatus, Track};
use dictar_notes::{fecha, NotesGenerator, SessionContext};
use dictar_storage::notes_store::NewNotes;
use dictar_storage::transcript::NewUtterance;
use dictar_stt::modelos::{self, Modelo};
use dictar_stt::whisper::Transcriptor;
use dictar_stt::SttOpciones;
use dictar_vad::segmentador::Segmentador;
use std::sync::mpsc::Sender;
use std::sync::Arc;

/// Cola sin transcribir que se tolera antes de rehacer el tramo final.
///
/// Un segmento dura como mucho 45 s, así que si la transcripción en vivo se
/// quedó a más de un minuto del final es que algo se la comió: un cierre a
/// medias, el modelo caído a mitad de clase. Menos que eso suele ser solo el
/// silencio de la despedida, y no merece rehacerse.
pub(crate) const COLA_SIN_CUBRIR_MS: i64 = 60_000;

/// Si la transcripción en vivo dejó un tramo final sin cubrir.
pub(crate) fn falta_cobertura(duracion_ms: Option<i64>, fin_transcrito_ms: i64) -> bool {
    duracion_ms
        .map(|d| d.saturating_sub(fin_transcrito_ms) > COLA_SIN_CUBRIR_MS)
        .unwrap_or(false)
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
            if falta_cobertura(sesion.duracion_ms(), fin_transcrito) {
                // La transcripción en vivo no llegó al final: un cierre a
                // medias, o el modelo se cayó a mitad de clase. Se transcribe
                // solo el tramo que falta, sin tirar lo ya hecho — unas notas
                // que cubrieran media clase en silencio serían peores que
                // esperar un poco más.
                tracing::info!(fin_transcrito, "completando el tramo final");
                self.con_db(|db| db.cambiar_estado(id, SessionStatus::Transcribing))?;

                let nuevas = self.transcribir_lote(id, &sesion, modelo, fin_transcrito, &emitir)?;
                if !nuevas.is_empty() {
                    self.con_db(|db| db.insertar_frases(id, &nuevas))?;
                }
            } else {
                tracing::info!(frases = vivas.len(), "transcripción ya hecha en vivo");
            }
            return self.solo_notas(id, &sesion, ahora, progreso).await;
        }

        // Sin transcripción previa: la pasada por lotes completa. Es el camino
        // de las grabaciones importadas, y el respaldo cuando el modelo no
        // pudo cargarse durante la clase.
        self.con_db(|db| db.cambiar_estado(id, SessionStatus::Transcribing))?;

        let frases = self.transcribir_lote(id, &sesion, modelo, 0, &emitir)?;
        if frases.is_empty() {
            self.con_db(|db| db.cambiar_estado(id, SessionStatus::Failed))?;
            return Err(ApiError::Config(
                "no hay audio con voz en esta sesión".into(),
            ));
        }

        self.con_db(|db| db.reemplazar_por_definitiva(id, &frases))?;
        self.solo_notas(id, &sesion, ahora, progreso).await
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
    fn transcribir_lote(
        &self,
        id: &SessionId,
        sesion: &dictar_domain::Session,
        modelo: Modelo,
        desde_ms: i64,
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

        for track in [Track::Mic, Track::System] {
            let ruta = dir.join(format!("{}.wav", track.as_str()));
            if !ruta.is_file() {
                continue;
            }

            let (pcm, _sr) = leer_wav(&ruta)?;
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
        let mut frases = Vec::new();

        for (i, s) in trabajos.iter().enumerate() {
            emitir(Evento::progreso(
                FaseProceso::Transcribiendo,
                i as f32 / total as f32,
                format!("segmento {} de {total}", i + 1),
            ));

            // El contexto previo mejora la continuidad entre segmentos: Whisper
            // acierta más con los nombres propios si acaba de verlos.
            opciones.contexto_previo = frases.last().map(|u: &NewUtterance| {
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

                emitir(Evento::frase(
                    id, s.track, u.start_ms, u.end_ms, &u.text, true,
                ));
                frases.push(u);
            }
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

        let utterances = self.con_db(|db| db.transcripcion(id))?;

        // La pasada en vivo transcribe cada pista por separado y no puede
        // compararlas entre sí: si el usuario escucha por altavoces, el
        // micrófono capta al profesor y cada frase aparece duplicada como
        // «Yo». Se limpia aquí, una vez, reescribiendo la transcripción
        // definitiva — unas notas que citan dos veces cada frase, con el
        // hablante equivocado la mitad de ellas, no sirven.
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
        let utterances = if eco > 0 {
            tracing::info!(eco, "eco del micrófono eliminado de la transcripción");
            self.con_db(|db| db.reemplazar_por_definitiva(id, &limpias))?;
            self.con_db(|db| db.transcripcion(id))?
        } else {
            utterances
        };

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
