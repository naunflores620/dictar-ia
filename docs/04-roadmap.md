# 04 — Roadmap

Orden pensado para que **cada fase sea utilizable por sí sola**. Si el proyecto se detiene en la
fase 1, lo construido ya te sirve todos los días.

Con las clases online como caso principal, el proyecto es bastante más corto de lo que parecía:
no hay app móvil en el camino crítico y el problema acústico deja de ser el riesgo dominante.

---

## Fase 0 — Validación técnica (3–5 días)

No se construye producto. Se comprueba que lo difícil funciona **en tus máquinas**, antes de
invertir en arquitectura. Cinco binarios de línea de comandos, uno por prueba.

| # | Prueba | Criterio de éxito |
|---|---|---|
| 1 | Loopback + micrófono en dos pistas sincronizadas → WAV, en **Windows** | 30 min de grabación sin deriva perceptible entre pistas |
| 2 | Lo mismo con **PipeWire en tu distro concreta**, incluida una sesión de Meet real | Se capta el audio de Meet, no solo el micrófono |
| 3 | `whisper.cpp large-v3-turbo` sobre 30 min de una clase tuya real | Medir **tiempo y precisión reales en tu equipo**, no los del README |
| 4 | Captura de pantalla + dHash sobre una clase con diapositivas | Detecta los cambios de lámina sin dispararse con la webcam |
| 5 | Mismo JSON Schema contra Gemini, DeepSeek y OpenAI | Los tres devuelven salida que valida |
| 6 | **Cadena de empaquetado completa, con la app vacía** | Un `.deb` y un `.exe` que instalan y arrancan |

**La prueba 6 se hace el primer día, no el último.** Un "hola mundo" de Flutter que llame a una
función trivial de una cdylib de Rust vía `flutter_rust_bridge`, empaquetado como `.deb` y como
`.exe` desde GitHub Actions, e instalado en un contenedor `debian:12` limpio y en una VM de
Windows. Valida de golpe el puente FFI, la colocación de la `.so`/`.dll` y la compilación
cruzada por CI —que **no existe**: el `.exe` no se puede construir desde Linux
([05-empaquetado.md §6](05-empaquetado.md#6-compilación-cruzada-no-existe)).

Montar esto al final, con 20.000 líneas encima, convierte un problema de una tarde en una
semana de depuración a ciegas.

**La prueba 3 es la que decide el proyecto.** Si en tu equipo `large-v3-turbo` va a 3× tiempo
real, 10 h de clase son 3,5 h de procesado nocturno y todo funciona. Si va a 0,5×, hay que bajar
a `medium` o replantear. Mejor saberlo el día tres que el día sesenta.

---

## Fase 1 — MVP de escritorio (3–4 semanas)

El producto entero, en su versión simple. **Al terminar esta fase ya lo usas a diario.**

**Alcance**
- Grabación de dos pistas con detección de la app de videollamada activa.
- **Importar archivo de audio** → cubre las reuniones presenciales sin escribir una línea de
  código móvil, con la cadena de denoise activable.
- Transcripción diferida con `large-v3-turbo`, en cola y en segundo plano.
- Diarización por pista: "Yo" / "Profesor" o "Cliente".
- Notas finales con las **dos plantillas**: apuntes de clase y acta comercial.
- Organización por `topic` (asignatura o cliente) e historial de sesiones.
- Reproductor sincronizado con la transcripción: clic en una frase → salta al audio.
- Búsqueda de texto completo. Exportación a Markdown.
- Configuración de proveedor y clave de API en el llavero del SO.
- Recuperación ante crash.

**Fuera de alcance:** diapositivas, tiempo real, chat, búsqueda semántica, Android.

**Criterio de aceptación:** grabar una clase real de 2 h y una reunión con un cliente, y que
los apuntes y el acta sean lo bastante buenos como para **no tener que volver a escuchar el
audio**. Esa es la prueba de fuego; todo lo demás es acabado.

---

## Fase 2 — Diapositivas y tiempo real (2–3 semanas)

Las dos capacidades que solo existen porque las clases son online.

- **Captura automática de diapositivas**: muestreo, dHash, deduplicación, anclaje a `ts_ms`.
- **OCR** de las capturas → texto indexable y alimento del glosario.
- Apuntes con las diapositivas intercaladas en su posición correcta.
- **Transcripción en vivo** (`whisper small` + LocalAgreement-2), con interruptor para apagarla
  si el equipo va justo.
- **Resumen rodante** cada 10 minutos, visible durante la clase.
- **Glosario por asignatura**, alimentado por el OCR y los términos recurrentes, inyectado en el
  `initial_prompt` de Whisper.
- Marcadores manuales durante la sesión.

**Criterio de aceptación:** la transcripción en vivo no "baila" de forma molesta, y en una clase
de 60 diapositivas se capturan entre 55 y 65 — no 300 por culpa de la webcam del profesor.

El glosario merece atención especial: es lo que hace que la asignatura se transcriba cada vez
mejor a lo largo del semestre. Mide la precisión en la clase 1 y en la clase 10 de la misma
asignatura; si no mejora, algo está mal conectado.

---

## Fase 3 — Conocimiento acumulado (2–3 semanas)

Aquí aparece el valor que ninguna grabadora tiene: no la sesión suelta, sino el semestre entero.

- **Búsqueda semántica transversal** con `sqlite-vec`: "¿qué dijo sobre transformadas de
  Laplace?" a lo largo de las 15 clases.
- **Chat con la asignatura** (RAG) con citas y enlaces al audio y a la diapositiva.
- **Panel de pendientes** desde la tabla `alert`: fechas de examen, entregas y compromisos con
  clientes, cruzando todos los `topic`.
- **Preparación de examen**: recopilar todas las `senales_examen` de la asignatura y generar un
  banco de preguntas o tarjetas de repaso a partir de ellas.
- Exportación a PDF con formato decente, para enviar un acta a un cliente.

**Criterio de aceptación:** poder responder "¿qué entra en el examen?" y "¿qué le prometí a este
cliente?" sin abrir una sola grabación.

---

## Fase 4 — Android y opcionales

Solo la app Android está planificada (+2–3 semanas: la UI Flutter y el núcleo Rust ya existen,
el trabajo real son los permisos, el *foreground service* y la transferencia por LAN). El resto
se activa por una necesidad real, no por completitud:

| Se construye… | …cuando |
|---|---|
| **App Android** | El escritorio esté terminado. Está planificada (misma UI Flutter, mismo núcleo Rust), pero cubre el 5 % de las sesiones: no debe adelantar a nada de las fases 1–3 |
| Diarización real (`pyannote`) | Haya reuniones presenciales de grupo con varias voces |
| Integraciones (Notion, Obsidian, Calendar) | Ya exista el hábito de uso y la exportación manual estorbe |
| Backend y sincronización | Quieras usarlo desde dos equipos, o convertirlo en producto para otros |
| iOS | Nunca, salvo cambio de circunstancias |

---

## Orden de riesgo

| # | Riesgo | Se resuelve en |
|---|---|---|
| 1 | Loopback fiable en tu Linux con Wayland | Fase 0 |
| 2 | Velocidad de Whisper en tu equipo | Fase 0 |
| 3 | Cadena de empaquetado y puente FFI | Fase 0 |
| 4 | **Que las notas sean realmente buenas y no genéricas** | Fase 1 — es la propuesta de valor entera |
| 5 | Falsos positivos en la detección de diapositivas | Fase 2 |

El riesgo 4 es el único que no se resuelve con ingeniería. Es cuestión de prompt, de troceado y
de criterio, y solo se afina iterando contra grabaciones reales. Por eso el corpus de referencia
de [03-proveedores-ia.md §6](03-proveedores-ia.md#6-calidad-y-control-de-regresiones) debería
empezar a construirse ya: **graba tus clases desde esta semana**, aunque sea con OBS o con la
grabadora del sistema. Cuando llegues a la fase 1 tendrás material real con el que ajustar, en
lugar de esperar a la siguiente clase para cada prueba.

---

## Estimación

| Fase | Duración | Acumulado |
|---|---|---|
| 0 — Validación | 3–5 días | 1 sem |
| 1 — MVP escritorio | 3–4 sem | 5 sem |
| 2 — Diapositivas y vivo | 2–3 sem | 8 sem |
| 3 — Conocimiento acumulado | 2–3 sem | 11 sem |

**~2,5 meses** para el producto completo. **Utilizable a diario en ~5 semanas.**

Estimación para una persona trabajando de forma sostenida, con experiencia en el stack. Si
Rust es nuevo para ti, la fase 1 se puede ir a 6–8 semanas: la curva está en el manejo de
`tokio` y del hilo de audio, no en el resto.

---

## Qué NO hacer primero

Tres errores de orden que costarían semanas:

1. **Empezar por la transcripción en vivo.** Es lo más vistoso y lo menos importante. Los
   apuntes finales no dependen de ella. Va en la fase 2 por un motivo.
2. **Empezar por la app Android.** Cubre el 5 % de las sesiones y cuesta más que el resto junto.
3. **Pulir la UI antes de validar la calidad de las notas.** Si el resumen no es bueno, no hay
   producto por muy bonito que se vea. La fase 1 puede tener una interfaz fea.
</content>
</invoke>
