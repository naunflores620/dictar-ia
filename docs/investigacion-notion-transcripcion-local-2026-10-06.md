# Investigación: experiencia de reuniones de Notion reproducida localmente

**Proyecto:** dictar_ia  
**Fecha:** 6 de octubre de 2026  
**Estado:** investigación de documentación pública y código local; no se modificó código de producto.  
**Objetivo:** acordar el comportamiento que queremos igualar de Notion AI Meeting Notes y cómo medirlo en clases de video, manteniendo captura e inferencia en el equipo local.

## 1. Alcance y calidad de la evidencia

Se revisó la documentación pública de Notion y las rutas de captura, segmentación, STT, eventos, UI, almacenamiento y notas del repositorio actual. Se distingue:

- **Verificado en documentación pública:** características que Notion explica en su centro de ayuda.
- **Verificado en código:** comportamiento que se desprende de las rutas indicadas abajo. La inspección estática no demuestra calidad de audio en una máquina Windows concreta.
- **Inferencia/propuesta:** comportamiento local recomendado para igualar el resultado visible; sus umbrales deben comprobarse con el corpus y el hardware objetivo.
- **No publicado:** internals que la documentación de Notion no permite conocer. No se atribuyen a Notion modelos, frecuencias de fragmentación ni algoritmos que no haya documentado.

## 2. Cómo se comporta Notion AI Meeting Notes

### Captura

Notion recomienda su aplicación de escritorio para videollamadas: captura micrófono y audio del sistema, y solicita permisos de audio del sistema/grabación de pantalla. El navegador captura solo el micrófono, por lo que puede perder las voces remotas si el usuario lleva auriculares. En móvil también captura el micrófono, no el audio que reproduce el propio teléfono. ([Ayuda de Notion: captura y permisos](https://www.notion.com/help/ai-meeting-notes))

### Transcripción durante la sesión

El producto presenta una transcripción de la sesión con cambios de hablante en líneas nuevas. La documentación de Notion describe “real-time transcription”, pero no especifica tamaño de ventanas, intervalos de refresco, modelo STT ni política de revisión/confirmación de palabras. Por tanto, esos mecanismos internos no son verificables públicamente. ([Ayuda de Notion: transcripción y hablantes](https://www.notion.com/help/ai-meeting-notes))

En escritorio Notion puede etiquetar hablantes, con mejores resultados en reuniones virtuales uno a uno donde obtiene por separado micrófono y audio del sistema. Reconoce límites en reuniones grupales, varias personas ante un solo micrófono y uso desde navegador/móvil; su etiquetado de hablantes actualmente está disponible en inglés. Las notas pueden citar líneas de la transcripción y llevar al momento correspondiente. ([Ayuda de Notion: hablantes y citas](https://www.notion.com/help/ai-meeting-notes))

### Notas y resumen

El usuario puede preparar agenda/contexto antes de iniciar. Al detener la transcripción, Notion inicia automáticamente la generación del resumen; el resumen considera tanto la transcripción como las notas previas. Notion indica un mínimo de 300 caracteres transcritos (aproximadamente un minuto hablado). Hay formatos/instrucciones de resumen configurables y opción de regenerarlo. ([Ayuda de Notion: flujo y resumen](https://www.notion.com/help/ai-meeting-notes))

### Límites y privacidad relevantes para una versión local

Notion confirma que el audio de su app/escritorio se envía a subprocesadores para transcripción en tiempo real; guarda una copia temporal local como respaldo y puede subirla si falla el procesamiento. La ayuda indica que AI Meeting Notes no funciona offline. El almacenamiento local de audio es una opción separada y desactivada por defecto. ([Ayuda de Notion: tratamiento de audio y disponibilidad offline](https://www.notion.com/help/ai-meeting-notes))

La documentación también admite que una transcripción puede contener errores o palabras inventadas y recomienda revisar calidad de audio. Por ello, la paridad debe medirse en audio real, no inferirse solo de que el texto aparezca rápidamente. ([Ayuda de Notion: troubleshooting de transcripciones](https://www.notion.com/help/ai-meeting-notes))

## 3. Qué hace hoy dictar_ia

### Captura y audio

- **Verificado:** Windows usa captura WASAPI (`core/audio-capture/src/wasapi_src.rs:87,180`); los valores predeterminados habilitan captura de sistema (`core/audio-capture/src/lib.rs:114-135`). La grabación configura micrófono y sistema en `core/api/src/grabacion.rs:105`.
- **Verificado:** separar pistas permite distinguir voz propia de audio reproducido, pero no garantiza que el micrófono no vuelva a captar físicamente los altavoces. El eco de la evidencia no puede adjudicarse con la imagen: hay que escuchar/medir `mic.wav`, `system.wav` y la mezcla.

### Transcripción en vivo

- **Verificado:** el VAD apunta a segmentos de 9 s, los cierra tras 500 ms de pausa una vez alcanzado ese mínimo y los fuerza a cerrar a los 20 s (`core/vad/src/segmentador.rs:36,49,52,182-183`). Esto es espera por segmento, no decodificación continua.
- **Verificado:** `encolar` recibe segmentos ya cerrados (`core/api/src/transcriptor.rs:106-110`); el bucle de trabajo los consume e infiere uno por uno (`core/api/src/transcriptor.rs:200-227`). La inferencia activa usa `Small` por defecto (`core/api/src/lib.rs:356-376`).
- **Verificado:** se ejecuta limpieza de bucles y se emite el resultado de esa inferencia con `is_final: true` (`core/api/src/transcriptor.rs:229-260`). Aunque el DTO y la UI conocen texto no definitivo, esta ruta no emite una hipótesis provisional revisable antes del cierre.
- **Verificado:** hay una implementación `AcuerdoLocal` diseñada para confirmar prefijos repetidos entre hipótesis y conservar una cola provisional (`core/stt/src/acuerdo.rs:17-24`). La búsqueda de referencias muestra su definición/pruebas, pero no una llamada desde el transcriptor de producción. Es capacidad existente aún desconectada.
- **Verificado:** la UI recibe eventos de frase y puede mostrar estado no definitivo (`app/lib/datos/repositorio_rust.dart:185`; `app/lib/pantallas/grabacion.dart:88-...`, estilo por `definitiva` en `grabacion.dart:783`). El polling/event stream de UI no reduce por sí mismo el tiempo hasta que el backend publica una frase.
- **Verificado:** la pasada definitiva posterior sustituye la transcripción provisional (`core/storage/src/transcript.rs:74-100`; llamada desde `core/api/src/proceso.rs:110`). Por tanto, el dominio ya distingue transcripción live y definitiva, pero su flujo live actual no explota esa diferencia de forma incremental.

### Notas

- **Verificado:** hay generación final de notas tras procesar la sesión (`core/api/src/proceso.rs:388-398`). Esto se parece al resumen al detener de Notion.
- **Verificado:** existe `NotesGenerator::rodante` y modelo `RollingSummary` (`core/notes/src/lib.rs:325-358`; `core/domain/src/notes.rs:36-40`) y existe almacenamiento de último resumen (`core/storage/src/notes_store.rs:73-74`). No se encontró llamada desde el ciclo de grabación/API; es decir, las notas rodantes no están conectadas a la sesión activa.
- **Verificado:** la app enlaza notas/conceptos con tiempos de audio en la presentación final; la calidad del ejemplo compartido debe evaluarse contra audio y lista de hechos esperados, no solo contra la existencia de esquema estructurado.

## 4. Diagnóstico de la diferencia observada

La principal brecha con “se va trabajando” está en la ruta de entrega: la app puede mostrar eventos sin esperar al cierre de la sesión, pero el backend solo produce texto después de que VAD cierra bloques de habla y marca cada resultado como final. Los límites actuales implican espera perceptible de varios segundos y, en habla continua, potencialmente hasta 20 s antes de una nueva inferencia/publicación. El mecanismo `AcuerdoLocal` que podría dar texto provisional y confirmar texto estable no participa hoy en esa ruta.

La transcripción repetitiva y el eco son problemas relacionados, pero diferentes:

1. **Eco audible:** localizar si ya existe en una de las pistas o aparece al mezclar/reproducir; inspección de código no determina cuál ocurrió en la máquina reportada.
2. **Repeticiones en texto:** separar repetición audible real, ASR en bucle y duplicación/revisión en eventos. Hay limpieza anti-bucle y supresión de diafonía al generar notas/finalizar (`core/api/src/proceso.rs:302-308,344-369`), pero la emisión de frases live (`core/api/src/transcriptor.rs:229-260`) ocurre antes de esa limpieza entre pistas.
3. **Resumen corto:** medir primero si la transcripción contiene los temas y hechos de la clase; la generación final actual se alimenta del texto procesado. No hay evidencia suficiente para atribuir el resumen escueto solo al modelo de notas.

## 5. Resultado local que proponemos igualar

La paridad debe definirse por el resultado comprobable para el usuario, no por copiar la implementación privada de Notion:

1. **Mientras habla el profesor**, el texto empieza a aparecer en segundos, recibe revisiones mientras la frase sigue abierta y distingue visualmente borrador de texto confirmado.
2. **Al estabilizarse una frase**, la UI conserva una sola versión: sin duplicados al actualizar borrador, confirmar prefijo o reemplazar por la pasada final. Cada tramo mantiene tiempo y pista.
3. **Al detener**, el sistema reconcilia la pasada final con lo mostrado en vivo, genera apuntes de clase y enlaza cada tema/concepto a su momento de audio. Un resumen con transcripción insuficiente debe advertirlo.
4. **En clases de video en Windows**, permite usar audio de sistema y micrófono como pistas separadas; una clase reproducida no debe reaparecer como “Yo” por fuga acústica sin detectar.
5. **En local/offline**, los modelos, audio, inferencia, transcripción y notas se procesan y persisten localmente. La app debe identificar qué modelos faltan y no degradarse silenciosamente a una dependencia de red.
6. **Español**, incluyendo nombres y términos de la materia. Notion soporta español para transcribir, pero su ayuda actualmente limita las etiquetas de hablante a inglés; el proyecto local puede priorizar etiquetas fiables de pistas `Yo`/`Sistema` en clases individuales.

El artículo Whisper-Streaming describe una estrategia viable para convertir Whisper, que no fue diseñado para streaming, en transcripción incremental: ventanas solapadas y política LocalAgreement con latencia adaptativa. Reporta 3.3 s en su conjunto de evaluación; esa cifra **no** predice rendimiento en esta computadora/modelo. Debe usarse como referencia de diseño y medir latencia y WER en el hardware del usuario. ([Artículo técnico](https://arxiv.org/abs/2307.14743)) El ejemplo oficial `whisper.cpp` también muestra inferencia periódica y ventanas deslizantes con VAD, aunque se describe como ejemplo básico, no como garantía de calidad de producto. ([Ejemplo whisper.cpp de audio en tiempo real](https://github.com/ggml-org/whisper.cpp/blob/master/examples/stream/README.md))

## 6. Plan técnico derivado, sin implementar todavía

1. **Medir la referencia:** conservar una muestra autorizada de la clase reportada, anotar a mano texto/timestamps/hablante/hechos clave, capturar y escuchar micrófono, sistema y mezcla. Esto resuelve el eco antes de tocar STT.
2. **Hacer que el texto avance:** usar audio reciente con ventanas solapadas y procesar cada pista separadamente. Reutilizar/terminar de integrar `AcuerdoLocal` para emitir cambios provisionales y solo confirmar palabras cuando coincidan en inferencias consecutivas. La ventana/intervalo final se selecciona mediante benchmark, no se asume idéntico a Notion.
3. **Dar semántica de actualización a eventos/UI:** identificar cada segmento lógico con pista y rango temporal; un evento debe poder crearlo, actualizar su cola provisional y confirmarlo. La UI debe reemplazar el borrador correspondiente, no añadir cada revisión como otra frase.
4. **No dejar que la inferencia se atrase:** medir tiempo de inferencia/audio acumulado por pista y mantener cola acotada; si el equipo no procesa en tiempo real, mostrar latencia y un estado recuperable. No descartar silenciosamente audio ni publicar resultados viejos como si fueran actuales.
5. **Unificar la reconciliación final:** al detener, pasar la inferencia de calidad alta, reemplazar/conciliar elementos live por identidad temporal y conservar marcas de tiempo/pistas sin doble inserción.
6. **Limpiar el origen de eco:** comparar formas de onda, correlación y escucha por pista. Si el profesor está en `mic.wav`, reducir fuga acústica mediante configuración/orientación de dispositivos o cancelación validada; si solo está duplicado en la mezcla, corregir mezcla/reproducción; si no está en audio y sí en texto, corregir ASR/eventos.
7. **Apuntes de clase:** tras una transcripción aceptada, evaluar cobertura de hechos, fidelidad, organización por tema y enlaces a momentos. Notas rodantes durante la clase pueden ser una HU opcional posterior; Notion documenta principalmente generación automática al detener, así que no son requisito de paridad mínima.
8. **Validar local/offline y privacidad:** ejecutar sin red después de preparar modelos, verificar que no se realicen llamadas externas durante captura/STT/resumen, y documentar ubicación/borrado de audios y transcripciones.

## 7. Métricas de aceptación sugeridas

Estos son objetivos iniciales propuestos para validar, no prestaciones actuales:

| Dimensión | Objetivo inicial |
|---|---|
| Primera hipótesis visible | p95 ≤ 4 s desde el habla reconocible, medida por pista y con referencia de hardware registrada |
| Revisión visual | el borrador se actualiza sobre el mismo tramo; nunca añade una copia por revisión |
| Confirmación | el prefijo confirmado no cambia por revisiones posteriores, salvo corrección explícita de la pasada final identificada como tal |
| Atraso | en clip de 30 min, cola no crece sostenidamente y p95 de latencia se mantiene dentro del objetivo |
| Transcripción final | WER ≤ 25 % sobre audio inteligible del corpus acordado; medir español, por pista y conjunto |
| Bucle alucinatorio | el patrón artificial “de la Tierra” no sobrevive como repetición si no está en el audio, pero sí se conserva una repetición intencional de control |
| Hablantes/pistas | voz del video queda en Sistema; intervención propia queda en Yo; fuga por altavoces se contabiliza y no se etiqueta como una segunda intervención sin control |
| Notas | ≥80 % de hechos clave anotados por persona; cada afirmación contrastable está respaldada por audio y con enlace temporal |
| Operación local | captura, transcripción y resumen completan sin acceso de red después de descargar los modelos |

Los umbrales de WER/cobertura ya aparecen en el plan de corrección previo; deben confirmarse contra la grabación real. Latencia p95 ≤4 s es una meta de producto propuesta y debe revisarse tras benchmark, no una conclusión tomada de Notion ni una garantía para cualquier PC.

## 8. Preguntas abiertas que el código no resuelve

1. ¿El usuario quiere conservar siempre la voz propia durante una clase en video, o poder escoger “solo profesor” para ese modo?
2. ¿Puede proporcionar los WAV separados (`mic.wav` y `system.wav`) de la sesión que produjo el eco, o una reproducción controlada equivalente? Sin esto no se puede localizar causalmente la duplicación.
3. ¿Qué hardware Windows usó (CPU/GPU/RAM), qué modelo se descargó realmente, y si escuchaba por altavoces o auriculares?
4. ¿El comportamiento aceptable es que el texto provisional cambie mientras habla, aunque algunas palabras desaparezcan, o se prefiere mostrar solo fragmentos confirmados con algo más de latencia?
5. ¿Debe “100 % local” incluir también modelos descargados de antemano y cero conexión durante el uso, y qué política de conservación/borrado de audio se espera?
6. Para las notas, ¿qué estructura es imprescindible para clases (tema, conceptos, ejemplos, preguntas, fórmulas, tareas, timestamps) y quién define el conjunto de hechos con el que se medirá cobertura?

## 9. Fuentes

- [Notion Help Center — Take AI Meeting Notes in Notion](https://www.notion.com/help/ai-meeting-notes) (captura, flujo, transcripción, hablantes, citas, resumen, audio, offline y límites conocidos).
- [Macháček, Dabre y Bojar — Turning Whisper into Real-Time Transcription System](https://arxiv.org/abs/2307.14743) (LocalAgreement y transcripción incremental basada en Whisper).
- [ggml-org/whisper.cpp — ejemplo `stream`](https://github.com/ggml-org/whisper.cpp/blob/master/examples/stream/README.md) (inferencia periódica, ventanas deslizantes y VAD como ejemplo básico).
