# Investigación: transcripción local en tiempo real para equipos modestos

**Proyecto:** dictar_ia  
**Fecha:** 6 de octubre de 2026  
**Estado:** revisión de soluciones/repositorios públicos; no se instaló ni integró ningún motor.  
**Objetivo:** encontrar enfoques que mantengan la transcripción avanzando durante una clase en español, sin exigir GPU dedicada ni un modelo grande.

## Conclusión ejecutiva

Hay dos rutas prácticas que vale la pena medir en el equipo objetivo:

1. **Vosk con modelo español pequeño**: mejor candidato para empezar si el requisito principal es bajo consumo y salida parcial realmente incremental. El modelo publicado de español pesa 39 MB y la API ofrece parciales durante la entrada de audio. El riesgo es que las cifras publicadas no predicen reconocimiento de clases universitarias, acentos salvadoreños, términos técnicos ni video con sonido mezclado.
2. **Mantener Whisper, pero hacer incremental el flujo**: el modelo `tiny` multilingüe es pequeño comparado con `small`; `whisper.cpp` ha mostrado modo en tiempo real en Raspberry Pi 4 usando `tiny.en`. Esto demuestra viabilidad de la familia bajo condiciones limitadas, pero el ejemplo citado es inglés y no garantiza español, dos pistas simultáneas, ni el hardware del usuario. La técnica de ventanas solapadas vuelve a decodificar audio anterior, así que consume más que un motor diseñado como flujo stateful.

**Recomendación de investigación:** ejecutar primero un benchmark A/B local entre Vosk small español y el Whisper actual configurado con el menor modelo español útil, midiendo simultáneamente latencia, WER, consumo y atraso de cola en audio idéntico. No seleccionar por tamaño del archivo o demostraciones ajenas al caso. Si Vosk no alcanza la calidad mínima, probar ventanas solapadas Whisper con acuerdo de hipótesis; la pantalla debe recibir parches del mismo tramo, sin insertar cada hipótesis como frase nueva.

## Qué significa “en tiempo real” aquí

No basta con que una grabación de cinco minutos se transcriba en menos de cinco minutos. Para la experiencia pedida, el sistema debe:

- aceptar muestras pequeñas continuamente, sin esperar a que el VAD cierre bloques largos;
- mostrar texto parcial mientras la persona sigue hablando;
- permitir que el sufijo provisional cambie y marcarlo como provisional;
- confirmar texto estable y reconciliarlo al final sin duplicarlo;
- mantener tiempo de procesamiento menor que el tiempo de audio entrante (RTF < 1) y una cola que no crezca durante una clase larga.

Los motores de streaming stateful conservan estado acústico/lingüístico entre fragmentos. Las soluciones de “streaming Whisper” suelen volver a ejecutar un reconocedor offline sobre ventanas recientes y confirmar el prefijo común. La segunda opción es práctica y robusta, pero puede hacer varias veces trabajo sobre las mismas muestras.

## Soluciones comparadas

| Solución pública | ¿Parciales en vivo? | Coste publicado/relevante | Español | Evaluación para dictar_ia |
|---|---|---|---|---|
| **Vosk API + `vosk-model-small-es-0.42`** | Sí. API recibe audio progresivamente y expone `PartialResult`; confirma resultados al endpoint. | Modelo español 39 MB. Proyecto indica que la familia small ronda 50 MB y ~300 MB de memoria de ejecución; dice que estos modelos apuntan a móviles/Raspberry Pi y desktop. | Español explícito. Lista oficial publica WER 16.02 Common Voice, 16.72 MTEDx, 11.21 MLS para el modelo small. | Primer benchmark recomendado para CPU/modesta. La cifra no es comparable con el clip de clase; verificar nombres, jerga, eco y acento. La licencia del modelo indicado es Apache 2.0. |
| **`whisper.cpp` + Whisper tiny/base multilingüe** | El ejemplo `stream` reevalúa audio periódicamente; ofrece rolling segment o bloques con timestamp/VAD. | Tiny 75 MiB en disco; base 142 MiB. En benchmark comunitario del proyecto se muestra ejemplo tiny.en tiempo real en Raspberry Pi 4 con tres hilos. Memoria/RAM y velocidad real dependen del build, CPU y longitud de ventana. | Whisper tiny/base son multilingües, pero tiny puede degradar palabras poco frecuentes; hay que medirlo en la clase. | Menor riesgo de cambiar de familia respecto al motor actual, pero es streaming simulado con ventanas superpuestas, no un reconocedor causal stateful. El ejemplo Pi es inglés y no basta para afirmar RTF español. |
| **Whisper-Streaming / SimulStreaming (UFAL)** | Sí. Usa LocalAgreement para confirmar prefijos que coinciden entre actualizaciones; SimulStreaming también describe AlignAtt y procesamiento incremental. | El README actual de SimulStreaming recomienda GPU de 10 GB VRAM para su mejor configuración y avisa que corre en CPU pero sería demasiado lento en tiempo real. El código de Whisper-Streaming documenta CPU int8, probado aproximadamente 10× más lento que CUDA FP16 en el hardware del autor. | Basado en Whisper multilingüe. | Referencia útil de algoritmo de revisión/confirmación, no primera elección como motor ligero en CPU. Se puede reutilizar la idea LocalAgreement dentro del pipeline nativo, evitando añadir su pila Python pesada. |
| **sherpa-onnx online streaming** | Sí. API online recibe waveform por chunks, decodifica cuando está lista, y consulta resultados; puede manejar varios streams. | Runtime local y configuraciones CPU; existen modelos cuantizados int8. Coste real depende de modelo y tamaño del chunk. | El runtime admite varios modelos, pero no se confirmó un modelo español streaming oficial generalista equivalente al pequeño Vosk. Hay un Zipformer comunitario español streaming que predice fonemas, entrenado con Common Voice/SLR72; no equivale sin más a una transcripción de palabras lista para UI. | Motor técnicamente interesante para una fase posterior si aparece un checkpoint español adecuado. No lo tomaría como candidato inmediato sin entrenar/evaluar el modelo y resolver decodificación de fonemas. |
| **Moonshine Streaming** | Sí; arquitectura publicada procesa audio incrementalmente y ofrece tiny/small/medium. | Enfoque on-device y tamaños pequeños/medios; documentación presenta streaming de baja latencia. | Los checkpoints oficiales consultados se muestran como inglés; no se encontró checkpoint oficial español streaming. | No cumple el idioma requerido con evidencia comprobada; descartarlo como candidato de esta fase, aunque la arquitectura sea atractiva. |

## Evidencia y límites por solución

### 1. Vosk: streaming nativo y modelo pequeño en español

La documentación oficial de Vosk describe modelos pequeños pensados para móviles/Raspberry Pi y desktop: aproximadamente 50 MB de modelo y 300 MB de memoria en ejecución. La tabla oficial publica `vosk-model-small-es-0.42` en 39 MB, idioma español, licencia Apache 2.0 y resultados separados para Common Voice, MTEDx y MLS. El modelo grande español figura en 1.4 GB, con mejores métricas publicadas, por lo que hay una compensación concreta entre memoria/tamaño y error. ([Modelos Vosk](https://alphacephei.com/vosk/models))

La API oficial expone `AcceptWaveform`, `PartialResult` y `FinalResult`, y el wrapper Android ejemplifica cómo emitir parcial hasta que `AcceptWaveform` indica un endpoint. Eso es transcripción incremental utilizable en UI, no solo una pasada por bloques cerrados. ([API Python de Vosk](https://github.com/alphacep/vosk-api/blob/master/python/vosk/__init__.py), [servicio Android de streaming](https://github.com/alphacep/vosk-api/blob/master/android/lib/src/main/java/org/vosk/android/SpeechStreamService.java))

**Límite clave:** los WER de la tabla proceden de corpus y protocolos distintos a una clase grabada desde navegador. No hay garantía de que el modelo pequeño entienda términos de la clase, voz de profesor comprimida por videollamada, música, reverberación o acento regional. Se debe evaluar el WAV de usuario con referencia humana. El parser en vivo entrega texto provisional que puede cambiar; si la UI acumula todos los snapshots, producirá repeticiones visuales incluso con ASR correcto.

### 2. Whisper tiny con `whisper.cpp`: calidad multilingüe posible, inferencia repetida

El catálogo de `whisper.cpp` lista tiny (75 MiB) y base (142 MiB), y aclara que los modelos son multilingües salvo los sufijos `.en`. El ejemplo oficial de streaming toma audio cada medio segundo en modo periódico o usa una ventana deslizante con VAD; su propio README lo llama un ejemplo básico. ([Modelos `whisper.cpp`](https://github.com/ggml-org/whisper.cpp/blob/master/models/README.md), [ejemplo stream](https://github.com/ggml-org/whisper.cpp/blob/master/examples/stream/README.md))

El proyecto publicó una demostración de `tiny.en` en Raspberry Pi 4 con tres hilos y pasos de 4 o 7.5 segundos. Prueba que Whisper muy pequeño puede alcanzar comportamiento cercano a tiempo real en un caso concreto, pero no prueba español ni latencia subsegundo, ni dos canales. ([Demostración de Raspberry Pi 4](https://github.com/ggml-org/whisper.cpp/discussions/166))

El costo de recurrir a ventanas se conoce: para revisar el texto tentativo, el reconocedor debe volver a mirar audio reciente. Reducir frecuencia ahorra CPU, pero retrasa correcciones; agrandar ventana mejora contexto, pero consume más memoria/CPU y sube latencia. La arquitectura puede aprovechar el motor Whisper ya presente y `AcuerdoLocal`, pero necesita medir RTF cuando se ejecutan simultáneamente pistas micrófono/sistema.

### 3. Whisper-Streaming/SimulStreaming: buen patrón de UX, no necesariamente motor ligero

Whisper-Streaming define LocalAgreement-n: si consecutivas actualizaciones sobre audio nuevo coinciden en un prefijo, ese prefijo pasa a confirmado. Es justo la separación de texto estable y borrador que queremos en UI. ([Repositorio Whisper-Streaming](https://github.com/ufal/whisper_streaming))

El repositorio más reciente SimulStreaming integra estrategias de streaming para modelos offline, pero advierte explícitamente que el código funciona en CPU y sería demasiado lento para tiempo real; su hardware recomendado para el mejor modelo Whisper large-v3 es GPU de al menos 10 GB VRAM. El código previo Whisper-Streaming documenta un backend CPU int8, pero los autores reportaron alrededor de 10 veces más lento que CUDA FP16 en su prueba. Estas mediciones no son universales, pero hacen que desplegar toda esa implementación/modelo no sea la primera apuesta para equipos sin GPU. ([Repositorio SimulStreaming](https://github.com/ufal/SimulStreaming), [backend CPU int8 de Whisper-Streaming](https://github.com/ufal/whisper_streaming/blob/main/whisper_online.py))

**Uso recomendado en este proyecto:** reutilizar el principio LocalAgreement y mantener el runtime Rust existente como camino principal. El algoritmo de confirmación es barato frente a inferir el modelo; la carga grande está en cuánto audio y con qué modelo se vuelve a decodificar.

### 4. sherpa-onnx: streaming stateful eficiente, problema de checkpoint español

La API online de sherpa-onnx alimenta waveform en fragmentos (su ejemplo muestra 0.2 s a 16 kHz), decodifica pasos listos y obtiene snapshots de texto; documenta también decodificar varios streams en paralelo. El encabezado especifica ejecución local sin Internet. ([API C de streaming online](https://github.com/k2-fsa/sherpa-onnx/blob/master/sherpa-onnx/c-api/c-api.h))

Se encontró un checkpoint comunitario `Pruned Stateless Zipformer RNN-T Streaming Robust ES v0`, con licencia Apache 2.0, basado en Common Voice Spanish y SLR72. El model card dice explícitamente que el objetivo son fonemas IPA, no secuencias de palabras. Esto puede servir para investigación/acústica pero hace falta un decodificador léxico/textual y medir errores; no es enchufar el archivo ONNX y recibir texto español formateado. ([Model card comunitario español](https://huggingface.co/bookbot/sherpa-onnx-ort-zipformer-streaming-robust-es-v0))

**Conclusión:** el runtime es candidato técnico para dos pistas concurrentes si se selecciona un modelo apropiado; por ahora la disponibilidad/model card española reduce su utilidad inmediata. Mantenerlo como vigilancia/experimento, no prometerlo como ganador sin benchmark.

### 5. Moonshine Streaming: muy pequeño, pero sin evidencia española

La documentación oficial de Transformers describe Moonshine Streaming como una variante optimizada para baja latencia en dispositivo, con tamaños tiny, small y medium y proceso incremental de chunks. El checkpoint tiny mostrado es `UsefulSensors/moonshine-streaming-tiny`. La página del checkpoint consultada lo etiqueta como inglés; no se encontró evidencia oficial de modelo streaming en español. ([Documentación Moonshine Streaming](https://huggingface.co/docs/transformers/model_doc/moonshine_streaming), [checkpoint tiny](https://huggingface.co/UsefulSensors/moonshine-streaming-tiny))

**Conclusión:** no priorizar en este sistema hasta que exista soporte verificable de español para un checkpoint realmente streaming.

## Qué encaja con la base actual

Verificado en el repositorio, la app actualmente:

- usa Whisper local y un modelo `Small` en vivo por defecto (`core/api/src/lib.rs:356-376`);
- publica texto una vez cerrados segmentos VAD de 9 s como objetivo, 500 ms de pausa para cierre y tope forzoso de 20 s (`core/vad/src/segmentador.rs:36,49,52,182-183`);
- conecta `AcuerdoLocal` a las hipótesis Whisper en vivo para emitir prefijo estable y sufijo provisional (`core/api/src/transcriptor.rs:423-439`); la calidad depende de que las hipótesis sucesivas mantengan el mismo inicio temporal;
- captura pistas separadas de micrófono y audio de sistema. Por tanto, una prueba que mida solo una pista no responde a la clase de video completa.

La medición local posterior en `evidencia/audio-referencia-espanol/diseno-sistemas/system.wav` mostró Vosk small español a 0.112 RTF por lote y parciales frecuentes, con texto entendible pero errores léxicos; Whisper small a 2 hilos, forzado a español, produjo frases más completas pero solo 0.52× respecto al audio fuente. Son benchmarks de WAV, no captura real, y no existe transcripción humana para WER. La configuración de idioma se está haciendo seleccionable en Ajustes para que Whisper y la pasada final usen el idioma esperado. Sigue pendiente una comparación en flujo vivo con CPU/RAM y atraso de cola durante reproducción.

## Benchmark mínimo propuesto para decidir

Usar el caso reportado de 2:12 y, antes de seleccionar tecnología, una clase de 30 minutos. Congelar versiones de motor/modelos y hardware. No cambiar simultáneamente captura, algoritmo y UI: se quiere saber qué variable aporta la mejora.

### Entradas

1. Pista `system.wav` del video/clase (profesor remoto).
2. Pista `mic.wav` en la condición real de uso (auriculares o altavoces documentados).
3. Para ASR de contenido del profesor, probar también la mezcla solo si las pruebas de eco evidencian que sistema/micrófono aportan valor sin duplicación.
4. Referencia humana con palabras, tiempos aproximados y términos académicos relevantes.

### Candidatos

- A: Vosk `small-es-0.42`, feeding PCM continuamente, guardar tanto snapshots parciales como finales.
- B: motor Whisper local actual con tamaño mínimo que reconozca español; generar hipótesis con ventana deslizante y LocalAgreement. Comparar `tiny` vs el modelo live actual solo si el hardware permite mantener RTF estable.
- C, solo si A/B no llegan: sherpa-onnx online con checkpoint español y ruta de decodificación textual aprobada; documentar origen, licencia, benchmark y calidad del vocabulario.

### Métricas

| Métrica | Cómo decidir |
|---|---|
| Latencia primer texto | p50/p95 desde el inicio de voz hasta primer texto visible. Meta inicial: p95 ≤4 s, ajustable tras obtener el hardware real. |
| Latencia de confirmación | p50/p95 desde el habla hasta que las palabras dejan de cambiar. Mantener separado del primer borrador. |
| RTF y atraso | tiempo de inferencia/audio procesado; registrar backlog por pista cada segundo. RTF promedio y pico <1, backlog no crece sostenidamente. |
| Calidad de español | WER global y por segmentos limpios/ruidosos; además exactitud de términos clave de la clase. Comparar el mismo audio y normalización. |
| Repetición/alucinación | medir palabras repetidas que no existen en audio; incluir control donde sí se repite una frase. |
| Recursos | pico y promedio de RAM, CPU total, carga por núcleo, temperatura/limitación y tamaño de modelo en disco. Registrar build/runtime. |
| Dos pistas | probar `system` y `mic` concurrentes; validar que la cola siga acotada y que la atribución no cree al profesor como “Yo”. |
| UX de revisión | número de correcciones visibles, saltos/duplicados de texto y proporción de texto provisional finalmente confirmado. |

### Regla de selección

Descartar un modelo si no sostiene RTF <1 con ambas pistas o si el atraso crece a lo largo de la clase, aunque el promedio WER sea bueno. Entre opciones que cumplen eso, escoger la de mejor WER/terminología y menor latencia de confirmación. Si ninguna opción cubre ambos, presentar el trade-off como ajuste al usuario (modo ligero más veloz o modo más preciso con más demora), en vez de ocultar el atraso.

## Orden sugerido de investigación aplicada

1. **Baseline Vosk small español en WAV**, empezando con `system.wav`: bajo costo y streaming real, permite medir la calidad inferior/superior del caso sin tocar captura.
2. **Baseline Whisper actual con audio por ventanas cortas** y conexión de `AcuerdoLocal`: conservar la calidad del motor ya elegido y cuantificar costo del replanteo de ventanas.
3. **Comparar en directo con dos pistas** en CPU y hardware del usuario; el rendimiento de un solo canal no basta.
4. **Decidir motor con corpus de clase**, no con demo genérica. La transcripción debe acertar tanto velocidad como contenido útil.
5. **Solo después** evaluar sherpa-onnx/checkpoints especializados u otros modelos ligeros; pedir modelo con documentación clara en español, texto final decodificado, licencia de uso y evaluación reproducible.

## Riesgos y preguntas pendientes

- Sin CPU/RAM exactas no se puede afirmar qué latencia tendrá el equipo de prueba.
- Las métricas públicas de Vosk se calculan en otros conjuntos; WER no compara directamente Common Voice, MTEDx, MLS y nuestra clase.
- “39 MB” describe el tamaño de descarga del modelo Vosk small, no la memoria total del proceso ni calidad equivalente a modelos grandes.
- Dos pistas reconocidas independientemente aproximadamente duplican trabajo de inferencia, salvo soporte de batching o streams compartidos; hay que medirlo.
- Primero debe resolverse la fuga/eco de audio: cualquier motor recibirá una señal ya degradada y podría inventar/repetir texto.
- Hace falta conocer CPU, RAM, si hay GPU integrada y si la sesión problemática se escuchó en altavoces o audífonos.

## Fuentes públicas

- [Vosk — modelos y resultados por idioma](https://alphacephei.com/vosk/models)
- [Vosk — API streaming y parciales](https://github.com/alphacep/vosk-api/blob/master/python/vosk/__init__.py)
- [Vosk — servicio streaming Android](https://github.com/alphacep/vosk-api/blob/master/android/lib/src/main/java/org/vosk/android/SpeechStreamService.java)
- [whisper.cpp — tamaños de modelo](https://github.com/ggml-org/whisper.cpp/blob/master/models/README.md)
- [whisper.cpp — ejemplo de streaming](https://github.com/ggml-org/whisper.cpp/blob/master/examples/stream/README.md)
- [whisper.cpp — tiny.en en Raspberry Pi 4](https://github.com/ggml-org/whisper.cpp/discussions/166)
- [UFAL — Whisper-Streaming](https://github.com/ufal/whisper_streaming)
- [UFAL — SimulStreaming](https://github.com/ufal/SimulStreaming)
- [sherpa-onnx — API C online streaming](https://github.com/k2-fsa/sherpa-onnx/blob/master/sherpa-onnx/c-api/c-api.h)
- [Checkpoint español streaming de comunidad para sherpa-onnx](https://huggingface.co/bookbot/sherpa-onnx-ort-zipformer-streaming-robust-es-v0)
- [Transformers — Moonshine Streaming](https://huggingface.co/docs/transformers/model_doc/moonshine_streaming)
- [Checkpoint Moonshine Streaming tiny](https://huggingface.co/UsefulSensors/moonshine-streaming-tiny)
