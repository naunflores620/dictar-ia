# PLAN — DEUDA «Transcripción incremental local para clases»

Origen: [plan de corrección, HU-18](../../../docs/plan-correccion-calidad-windows.md#hu-18--ver-cómo-la-transcripción-se-actualiza-mientras-ocurre-la-clase) y [comparación de motores para equipos modestos](../../../docs/investigacion-stt-tiempo-real-equipos-modestos-2026-10-06.md).

**Estado:** planificación inicial; sin cambios de producto en esta tarea.  
**Objetivo:** durante una clase en Windows, mostrar texto español provisional que avance con baja latencia, actualizarlo sin duplicados y cerrar con una transcripción consolidada, ejecutando el reconocimiento localmente y sin requerir GPU dedicada.  
**Límite:** no se presupone que el defecto sea exclusivamente Whisper. Se separan entrada de audio, inferencia, estado/eventos, UI, reconciliación final y generación de notas.

## Evidencia de partida

- En la ruta actual, `TranscriptorVivo::encolar` recibe un `Segmento` ya cerrado (`core/api/src/transcriptor.rs:106-110`); el worker llama una vez a `trans.transcribir` por segmento (`core/api/src/transcriptor.rs:200-227`).
- La segmentación apunta a 9 s, cierra por pausa de 500 ms una vez llegado al objetivo, o fuerza corte a 20 s (`core/vad/src/segmentador.rs:36,49,52,182-183`). Así la UI no puede mostrar palabras de esa inferencia hasta el cierre y el tiempo de decodificación.
- La ruta live elimina algunos bucles/alucinaciones y después emite cada fragmento como definitivo (`core/api/src/transcriptor.rs:229-260`). La limpieza de pistas cruzadas se ejecuta después en el procesamiento de sesión/notas (`core/api/src/proceso.rs:302-308,344-369`), demasiado tarde para evitar una duplicación visible en vivo.
- El modelo live por defecto es `Small`; el puente solicita `LargeV3Turbo` al procesar la sesión (`core/api/src/lib.rs:371-376`, `core/api/src/puente.rs:698`). No son una única inferencia ni generan la misma transcripción.
- `AcuerdoLocal` existe (`core/stt/src/acuerdo.rs:17-24`), pero no está conectado a `TranscriptorVivo`. Por sí solo no produce hipótesis nuevas: requiere alimentarlo con revisiones de audio/hypothesis.
- La pantalla de grabación y el repositorio ya tienen cambios sin confirmar en este checkout. La inspección inicial muestra errores visibles de modelo y separación de contexto por pista añadidos; deben preservarse y releerse antes de cualquier integración.
- El eco audible no está diagnosticado: el código habilita captura de sistema y micrófono por defecto (`core/audio-capture/src/lib.rs:124-131`), pero faltan los archivos de audio separados de la sesión que falló.

## Alcance de esta tarea

### Incluye

1. Medir una fuente de clase en español limpia y la captura real por pista; registrar CPU/RAM/GPU, modelos/versiones, configuración de salida y duración.
2. Comparar dos caminos locales con el mismo WAV de referencia: el Whisper que ya está en el producto (incluyendo una estrategia de ventana/revisión solo si hace RTF viable) y Vosk `small-es-0.42` como referencia streaming de bajo consumo.
3. Seleccionar un camino usando calidad y latencia medidas; no sustituir Whisper por Vosk solo por tamaño, ni conservar Whisper solo por continuidad tecnológica.
4. Implementar el ciclo de hipótesis provisional → actualización del mismo tramo → confirmación de prefijo → conciliación con pasada final.
5. Evitar que UI, eventos o almacenamiento acumulen snapshots de hipótesis como frases independientes. Mantener pista, timestamps y enlace al audio.
6. Mostrar atraso/backlog si el dispositivo no puede procesar el audio a tiempo real.
7. Preservar modo offline después de descargar los recursos de modelo.

### No incluye

- Diagnosticar/corregir mezcla acústica de la clase reportada sin sus WAV (`mic.wav` y `system.wav`); eso requiere evidencia del PO y corresponde al trabajo de HU-14/HU-15.
- Mejorar resumen/apuntes en esta tarea. Las notas usan otra ruta (`core/api/src/proceso.rs:387-399`) y deben medirse después de aceptar la transcripción.
- Reescribir arquitectura de la app o sustituir proveedores cloud de notas como parte del arreglo live.
- Comprometer una latencia universal sin registrar el hardware objetivo.

## Archivos candidatos

La lista final debe confirmarse al leer el árbol justo antes de implementar. Los siguientes archivos ya tienen cambios ajenos o pendientes en el checkout, por lo que el implementador debe incorporar el estado actual y nunca restaurarlos desde HEAD a ciegas.

| Archivo/área | Propósito probable |
|---|---|
| `core/stt/src/whisper.rs`, `modelos.rs`, `acuerdo.rs`, `core/stt/src/lib.rs` | Medición/invocación incremental y estabilización de hipótesis; revisar APIs y carga por modelo. |
| `core/vad/src/segmentador.rs` | Definir ventana/solape y límites de buffer con base en benchmark; evitar que VAD cierre sea el único disparador. |
| `core/api/src/transcriptor.rs`, `eventos.rs`, `puente.rs` | Mantener decoder/estado por pista, publicar operaciones de texto y progreso/atraso; comprobar si los DTO existentes bastan antes de tocar código generado. |
| `core/storage/src/transcript.rs`, `migrations.rs` | Identidad/reemplazo/idempotencia de tramos provisionales y definitivos, si la persistencia debe guardar hipótesis live. |
| `app/lib/datos/repositorio.dart`, `repositorio_rust.dart`, `app/lib/pantallas/grabacion.dart` | Reemplazar el tramo provisional en vez de añadir snapshots; mostrar texto provisional, confirmación y atraso. |
| `core/api/src/proceso.rs` | Reconciliación final con Large V3 Turbo y tratamiento de diafonía antes de notas; solo si es necesario para preservar el flujo actual. |

**Solapamientos activos conocidos:** `core/api/src/transcriptor.rs`, `core/vad/src/segmentador.rs`, `core/api/src/lib.rs`, `core/api/src/proceso.rs`, `core/audio-capture/src/lib.rs`, `app/lib/datos/repositorio_rust.dart`, `app/lib/pantallas/grabacion.dart` y otras rutas están modificadas/untracked en el árbol de trabajo al iniciar esta planificación. No se asume autoría ni estado completo de esos cambios.

## Secuencia de trabajo

1. **Congelar baseline medible.** Registrar hardware y obtener/crear clip consentido de referencia con verdad-terreno; escuchar `system.wav`, `mic.wav` y mezcla por separado. Si no se obtiene WAV del PO, distinguir la muestra controlada del incidente original.
2. **Benchmark A/B de motor.** Procesar la misma voz del profesor con Whisper local actual y Vosk small español. Medir p50/p95 de primera hipótesis y confirmación, WER, términos de clase, CPU/RAM pico/promedio y RTF. Después repetir con las dos pistas concurrentes.
3. **Decidir el recognizer con evidencia.** Un candidato solo avanza si RTF se mantiene <1 y el backlog no crece en una sesión de 30 min; entre los viables, priorizar WER/terminología y latencia de confirmación. Si no hay hardware/clip, no se cambia la opción por conjetura.
4. **Diseñar el contrato de actualización.** Definir tramo estable (sesión, pista y rango de tiempo) y operaciones/eventos para crear, actualizar provisional, confirmar y reconciliar. Confirmar compatibilidad con bridge/storage actuales antes de alterar DTOs/migraciones.
5. **Conectar inferencia incremental.** Si Whisper gana, usar ventanas solapadas acotadas y alimentar revisiones a `AcuerdoLocal`; si Vosk gana, traducir su PartialResult/Result al mismo contrato. Mantener el decodificador/estado independiente por pista para no contaminar contexto.
6. **Actualizar UI y persistencia.** Una revisión reemplaza el texto provisional del mismo tramo. La UI distingue borrador de confirmado; almacenamiento y búsqueda no deben contar múltiples snapshots como varias frases. Mostrar backlog/atraso si se incumple el objetivo.
7. **Cerrar/reconciliar.** Al detener, vaciar el último buffer, obtener la pasada final de calidad alta donde esté habilitada, reemplazar por identidad temporal/pista y verificar que no queden copias ni saltos.
8. **Validar.** Capturar sesión de 30 min, probar sobre el caso «de la Tierra» y un control de repetición pronunciada, comparar salida por pista, medir latencia/RTF/recursos y confirmar que la misma operación funciona sin red con modelos ya descargados. Informar como no verificable cualquier criterio que exija hardware/audio no disponible.

## Criterios para cerrar la tarea

- La p95 de la primera hipótesis visible es ≤4 s desde habla reconocible en hardware registrado, o se deja documentado el resultado y su causa si el objetivo no se logra.
- Se mide por separado latencia de borrador y de confirmación; revisión de hipótesis no genera filas repetidas ni se congela como definitiva antes de cumplir la política acordada.
- En un clip de 30 min, RTF <1 y la cola no crece de forma sostenida por pista.
- La transcripción final cumple WER inicial ≤25 % en el conjunto limpio acordado, con cifras por pista y por clip; términos académicos clave se reportan por separado.
- La secuencia artificial de repetición que no existe en la referencia se rechaza, mientras el control realmente repetido permanece.
- Cada tramo final tiene pista, hablante y timestamps válidos; la pasada final no duplica los tramos live.
- La prueba offline se completa después de preparar modelos, sin solicitudes de red durante captura, reconocimiento y guardado de transcripción.
- La causa del eco del incidente original solo se declara identificada si se comparan sus pistas separadas; de otro modo queda abierta y no bloquea afirmar mejoras independientes del motor.

## Premisas cuestionadas antes de implementar

1. **«Whisper es la causa porque el texto está mal».** No queda probado. Whisper puede decodificar un audio degradado o esperar segmentos largos; hay que observar entrada, hipótesis, filtros y salida.
2. **«Un motor más pequeño hará lo mismo en menos tiempo».** No demostrado. Vosk small se publica con métricas en otros corpus y Whisper tiny tiene una demostración inglesa de hardware específico; ninguno predice una clase en español con dos pistas.
3. **«Bajar el límite VAD convierte el flujo en vivo».** No basta. Solo reduce la demora entre inferencias por bloque; para texto que se corrige mientras la frase sigue abierta, hay que publicar/reemplazar hipótesis provisionales.
4. **«`AcuerdoLocal` resuelve streaming con conectarlo».** No basta. Hace falta volver a generar hipótesis sobre audio nuevo, acotando CPU y manteniendo una identidad estable de tramo.
5. **«El resumen corto confirma que falla el LLM».** No. Primero hay que comprobar que la transcripción contiene los temas y hechos de la clase; generación de notas es otra tarea.

## Dependencias y cuestiones abiertas

- WAV de micrófono y sistema del incidente, o una reproducción consentida equivalente.
- CPU/RAM/GPU del equipo Windows donde debe funcionar y modo de salida usado (altavoces/auriculares).
- Confirmar prioridad de producto: ¿clase de video = transcribir sobre todo al profesor, conservando siempre también la voz del usuario?
- Verificar disponibilidad de build/bridge tools antes de decidir cambios de DTO. El tablero menciona B-2/B-5, pero su estado actual debe confirmarse antes de considerar un bloqueo vigente.
- Este documento propone planificación y evidencia; no implica selección de modelo ni garantiza rendimiento hasta el benchmark.

## Desafío del plan

La arquitectura de dos pasadas actual separa baja latencia y calidad final. Este plan podría elevar significativamente el coste si redecodifica ventanas de Whisper para dos pistas en paralelo. Por eso el benchmark precede al cableado de ventana rodante y conserva Vosk como candidato pequeño. Si ningún motor reúne calidad y RTF requerido, la siguiente decisión debe ser explícita sobre el compromiso de UX, no esconder retraso o error de transcripción.
