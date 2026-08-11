# 03 — Capa de proveedores de IA y generación de notas

Requisito: **debe funcionar con cualquier proveedor** — Gemini, DeepSeek, OpenAI o modelos
locales — sin tocar el código. Solo configuración.

---

## 1. La observación que simplifica todo

DeepSeek, Groq, OpenRouter, Together, vLLM, LM Studio y Ollama exponen **la misma API que
OpenAI** (`/v1/chat/completions`). Gemini también ofrece un endpoint compatible además del suyo
nativo.

Por tanto no hacen falta cuatro adaptadores, sino **dos**:

1. **`OpenAiCompatProvider`** — parametrizado por `base_url`, `api_key` y `model`. Cubre OpenAI,
   DeepSeek, Groq, OpenRouter, Ollama, LM Studio, vLLM y cualquier proveedor futuro que siga el
   estándar de facto.
2. **`GeminiProvider`** — API nativa, para aprovechar contexto muy largo, `responseSchema` y
   entrada de audio e imagen directa (útil para el OCR de fotos de pizarra).

Añadir un proveedor compatible pasa a ser **una entrada en un TOML**, no una release.

## 2. Contrato

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn id(&self) -> &str;
    fn capabilities(&self) -> Capabilities;

    /// Completado con salida estructurada obligatoria contra un JSON Schema.
    async fn complete_structured(
        &self,
        req: CompletionRequest,
        schema: &JsonSchema,
    ) -> Result<(serde_json::Value, Usage), ProviderError>;

    /// Streaming de texto libre (chat con la asignatura o la reunión).
    async fn complete_stream(
        &self,
        req: CompletionRequest,
    ) -> Result<BoxStream<'static, Result<Delta, ProviderError>>, ProviderError>;
}

pub struct Capabilities {
    pub context_window: usize,
    pub max_output: usize,
    pub structured_output: StructuredMode,  // Native | JsonMode | PromptOnly
    pub accepts_audio: bool,
    pub accepts_images: bool,
    pub streaming: bool,
    pub price_in_per_mtok: f64,
    pub price_out_per_mtok: f64,
}
```

`StructuredMode` existe porque la salida estructurada **no es uniforme entre proveedores**:

| Modo | Cómo se pide | Quién lo soporta |
|---|---|---|
| `Native` | Esquema JSON estricto validado por el proveedor | OpenAI (*structured outputs*), Gemini (`responseSchema`) |
| `JsonMode` | "Devuelve JSON válido", sin garantía de esquema | DeepSeek y la mayoría de compatibles |
| `PromptOnly` | Solo instrucciones en el prompt | Modelos locales pequeños |

El adaptador **normaliza hacia arriba**: siempre se valida la respuesta contra el esquema en el
cliente y, si falla, se reintenta inyectando el error de validación en el prompt (hasta 2 veces).
Así el resto del sistema asume que la salida cumple el esquema sea cual sea el proveedor. Esta
es la pieza que hace real el "funciona con cualquiera".

## 3. Configuración — `config/providers.toml`

```toml
[[provider]]
id            = "gemini-flash"
kind          = "gemini"
model         = "gemini-2.5-flash"      # verificar el ID vigente en la doc de Google
api_key_ref   = "keyring:gemini"
context       = 1_000_000
structured    = "native"
accepts_audio = true
accepts_images = true
price_in      = 0.30
price_out     = 2.50

[[provider]]
id          = "deepseek"
kind        = "openai_compat"
base_url    = "https://api.deepseek.com/v1"
model       = "deepseek-chat"
api_key_ref = "keyring:deepseek"
context     = 128_000
structured  = "json_mode"
price_in    = 0.27
price_out   = 1.10

[[provider]]
id          = "openai"
kind        = "openai_compat"
base_url    = "https://api.openai.com/v1"
model       = "gpt-5"                    # ajustar al modelo contratado
api_key_ref = "keyring:openai"
context     = 400_000
structured  = "native"
price_in    = 1.25
price_out   = 10.00

[[provider]]
id         = "local"
kind       = "openai_compat"
base_url   = "http://localhost:11434/v1"
model      = "qwen3:14b"
context    = 32_768
structured = "json_mode"
price_in   = 0.0
price_out  = 0.0

# Enrutado por tarea, con cadena de respaldo ordenada
[routing]
class_notes    = ["gemini-flash", "deepseek", "local"]
client_minutes = ["gemini-flash", "openai", "deepseek"]  # lo que enseñas a un cliente
chat           = ["deepseek", "gemini-flash", "local"]
ocr_slides     = ["gemini-flash"]                        # requiere visión
fallback_policy = "next_on_error"        # 429, 5xx y timeout → siguiente de la lista
```

Si el primero falla o devuelve 429, se pasa automáticamente al siguiente. Los apuntes de una
clase de dos horas no se pierden porque un proveedor tuvo un mal minuto.

> **Sobre los IDs de modelo:** cambian con frecuencia y cada proveedor usa su nomenclatura. Los
> valores de arriba son plantillas; hay que confirmar el ID exacto en la documentación vigente
> al configurarlo. Por eso viven en un TOML y no en el código. La app debe validar el ID al
> guardar la configuración con una petición de prueba y mostrar un error claro, en vez de fallar
> al procesar.

---

## 4. Coste real con tu volumen

Una sesión de 1,5 h ≈ 13.500 palabras ≈ **19.000 tokens** de transcripción.

| Concepto | Tokens ent. | Tokens sal. | Gemini Flash | DeepSeek | Ollama local |
|---|---|---|---|---|---|
| Map (6 bloques) | ~22.000 | ~5.000 | ~$0.019 | ~$0.011 | $0 |
| Reduce | ~7.000 | ~2.000 | ~$0.007 | ~$0.004 | $0 |
| **Por sesión** | | | **~$0.026** | **~$0.015** | **$0** |
| **Semestre (~100 sesiones)** | | | **~$2.60** | **~$1.50** | **$0** |

**Los apuntes de un semestre entero cuestan menos que un café.** Ese no es el problema.

El coste real está en la transcripción:

| Transcripción | 150 h / semestre |
|---|---|
| Whisper local | **$0** |
| Deepgram Nova | ~$39 |
| AssemblyAI | ~$55 |

Por eso Whisper local es el modo por defecto y no una opción. Al proveedor cloud solo va
**texto ya transcrito**, que es barato y mucho menos sensible que el audio — especialmente
relevante en reuniones con clientes.

---

## 5. Plantillas de notas

Unos apuntes de clase y un acta comercial no se parecen en nada. El sistema tiene un **registro
de plantillas**, y cada `session.kind` selecciona la suya:

| `kind` | Plantilla | Salida |
|---|---|---|
| `class` | Apuntes de clase | Conceptos, fórmulas, avisos, señales de examen |
| `client_meeting` | Acta comercial | Necesidades, objeciones, compromisos, próximos pasos |
| `team_meeting`, `tutoring` | Acta simple | Temas, decisiones, tareas |

Cada plantilla = un prompt versionado + un JSON Schema + un renderizador en la UI. Añadir una
plantilla nueva no toca el núcleo. Los prompts viven en `config/prompts/`, versionados, y el
número de versión se guarda junto a cada nota generada para poder reproducir y comparar
resultados.

### 5.1 Prompt de map (común, parametrizado por plantilla)

```
Analiza este fragmento de la transcripción y extrae únicamente información
presente de forma literal o claramente inferible del texto.

TIPO DE SESIÓN: {plantilla}
CONTEXTO: {titulo}, {fecha}, {asignatura_o_cliente}
GLOSARIO CONOCIDO: {terminos}

FRAGMENTO ({t_inicio} – {t_fin}):
{transcripcion}

Devuelve JSON conforme al esquema. Cada elemento incluye "ts_ms": la marca de
tiempo en milisegundos donde se dijo, para poder verificarlo en el audio.
Si un campo no tiene contenido en este fragmento, devuelve lista vacía.
La transcripción es automática y puede tener errores: si una palabra parece
mal transcrita pero el sentido es claro, corrígela usando el glosario. Si no
está claro, transcríbela tal cual y márcala con "incierto": true.
```

Esa última regla importa: la transcripción de un aula ruidosa tendrá errores, y el modelo puede
corregir muchos por contexto. Pero debe distinguir entre corregir y adivinar.

### 5.2 Reduce — apuntes de clase

```
Tienes los extractos estructurados de todos los bloques de una clase de
{duracion} de {asignatura}, impartida por {profesor}.

EXTRACTOS: {extractos_json}
DIAPOSITIVAS CAPTURADAS (ts_ms + texto OCR): {diapositivas}

Produce los apuntes finales en JSON conforme al esquema.

El texto OCR de las diapositivas es la fuente MÁS FIABLE de la terminología
exacta: si la transcripción dice "transformada de la place" y la diapositiva
en ese momento dice "Transformada de Laplace", usa la de la diapositiva.

Criterios:
- "tldr": 2–3 frases. De qué fue la clase. Lo que leerías si faltaste.
- "temas": agrupa por tema, no en orden cronológico. Cada uno con su rango
  de tiempo.
- "conceptos": término y la definición TAL COMO la dio el profesor, no la
  definición de manual. Si dio un ejemplo, inclúyelo.
- "formulas": expresión y qué significa cada símbolo, si lo explicó.
  Referencia la diapositiva visible en ese rango de tiempo mediante su
  slide_id: para una fórmula, la imagen vale más que su transcripción.
- "avisos": fechas de examen, entregas, cambios de aula, cambios de horario.
  Este campo es el más importante del documento.
- "senales_examen": todo lo que el profesor marcó como importante — "esto
  entra", "esto suele caer", "prestad atención aquí", "esto lo pregunto
  siempre". Cita sus palabras exactas.
- "dudas": preguntas del público y la respuesta que dio el profesor.
- "bibliografia": libros, artículos, capítulos o enlaces mencionados.
- "glosario_nuevo": términos técnicos que aparecen y no estaban en el
  glosario de la asignatura.
- Conserva "ts_ms" en todos los elementos.
- No añadas conocimiento externo. Si el profesor se equivocó, transcribes lo
  que dijo; no lo corriges con lo que sabes.
- Idioma: el de la transcripción.
```

La última regla es deliberada. Unos apuntes que "mejoran" lo que dijo el profesor son
inservibles para un examen que corrige ese profesor.

### 5.3 Reduce — acta de reunión con cliente

```
Tienes los extractos estructurados de una reunión de {duracion} con
{cliente}. Pista "mic" = tú. Pista "system" = el cliente.

EXTRACTOS: {extractos_json}

Produce el acta en JSON conforme al esquema.

Criterios:
- "tldr": 2–3 frases. En qué quedó la reunión y qué pasa ahora.
- "contexto_cliente": empresa, sector, tamaño, herramientas actuales —
  únicamente lo que se dijo en la reunión.
- "necesidades": problemas y dolores que expresó el cliente, con sus
  palabras siempre que sea posible. Es la información más valiosa del acta.
- "objeciones": reparos que planteó y cómo se respondieron. Marca las que
  quedaron sin respuesta satisfactoria.
- "presupuesto": cifras, rangos o restricciones económicas mencionadas.
  null si no se habló de dinero. No estimes.
- "acuerdos": lo acordado de forma explícita por ambas partes.
- "compromisos": separados en "mios" y "del_cliente", cada uno con fecha si
  se dijo. NUNCA inventes un compromiso ni una fecha: si no se dijo, null.
- "proximo_paso": la siguiente acción concreta y cuándo.
- "senales": indicios de interés o de riesgo, citando la frase que los
  sustenta.
- Conserva "ts_ms" en todo. No añadas recomendaciones comerciales propias:
  documentas la reunión, no la asesoras.
```

La regla sobre compromisos inventados es la más importante del documento. Un acta que atribuye
al cliente una promesa que nunca hizo no es un error de calidad: es un problema de negocio.

### 5.4 Esquema — apuntes de clase

```json
{
  "type": "object",
  "required": ["tldr", "temas", "conceptos", "avisos", "senales_examen"],
  "additionalProperties": false,
  "properties": {
    "tldr": { "type": "string", "maxLength": 600 },
    "temas": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["titulo", "puntos", "inicio_ms", "fin_ms"],
        "properties": {
          "titulo":    { "type": "string" },
          "puntos":    { "type": "array", "items": { "type": "string" } },
          "inicio_ms": { "type": "integer" },
          "fin_ms":    { "type": "integer" }
        }
      }
    },
    "conceptos": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["termino", "definicion", "ts_ms"],
        "properties": {
          "termino":    { "type": "string" },
          "definicion": { "type": "string" },
          "ejemplo":    { "type": ["string", "null"] },
          "incierto":   { "type": "boolean" },
          "ts_ms":      { "type": "integer" }
        }
      }
    },
    "formulas": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["expresion", "ts_ms"],
        "properties": {
          "expresion":   { "type": "string", "description": "LaTeX si es posible" },
          "significado": { "type": ["string", "null"] },
          "slide_id":    { "type": ["integer", "null"], "description": "diapositiva visible" },
          "ts_ms":       { "type": "integer" }
        }
      }
    },
    "avisos": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["tipo", "texto", "ts_ms"],
        "properties": {
          "tipo":  { "enum": ["examen", "entrega", "cambio_aula", "cambio_horario", "otro"] },
          "texto": { "type": "string" },
          "fecha": { "type": ["string", "null"], "description": "ISO-8601" },
          "ts_ms": { "type": "integer" }
        }
      }
    },
    "senales_examen": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["tema", "cita", "ts_ms"],
        "properties": {
          "tema":  { "type": "string" },
          "cita":  { "type": "string", "description": "palabras exactas del profesor" },
          "ts_ms": { "type": "integer" }
        }
      }
    },
    "dudas": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["pregunta", "ts_ms"],
        "properties": {
          "pregunta":  { "type": "string" },
          "respuesta": { "type": ["string", "null"] },
          "ts_ms":     { "type": "integer" }
        }
      }
    },
    "bibliografia": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["referencia", "ts_ms"],
        "properties": {
          "referencia": { "type": "string" },
          "ts_ms":      { "type": "integer" }
        }
      }
    },
    "glosario_nuevo": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["termino"],
        "properties": {
          "termino":    { "type": "string" },
          "definicion": { "type": ["string", "null"] }
        }
      }
    }
  }
}
```

### 5.5 Esquema — acta comercial

```json
{
  "type": "object",
  "required": ["tldr", "necesidades", "acuerdos", "compromisos", "proximo_paso"],
  "additionalProperties": false,
  "properties": {
    "tldr": { "type": "string", "maxLength": 600 },
    "contexto_cliente": {
      "type": "object",
      "properties": {
        "empresa":     { "type": ["string", "null"] },
        "sector":      { "type": ["string", "null"] },
        "tamano":      { "type": ["string", "null"] },
        "herramientas":{ "type": "array", "items": { "type": "string" } }
      }
    },
    "necesidades": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["texto", "ts_ms"],
        "properties": {
          "texto":     { "type": "string" },
          "cita":      { "type": ["string", "null"] },
          "prioridad": { "enum": ["alta", "media", "baja", "desconocida"] },
          "ts_ms":     { "type": "integer" }
        }
      }
    },
    "objeciones": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["objecion", "ts_ms"],
        "properties": {
          "objecion":  { "type": "string" },
          "respuesta": { "type": ["string", "null"] },
          "resuelta":  { "type": "boolean" },
          "ts_ms":     { "type": "integer" }
        }
      }
    },
    "presupuesto": {
      "type": ["object", "null"],
      "properties": {
        "texto":  { "type": "string" },
        "cifra":  { "type": ["string", "null"] },
        "ts_ms":  { "type": "integer" }
      }
    },
    "acuerdos": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["texto", "ts_ms"],
        "properties": {
          "texto": { "type": "string" },
          "ts_ms": { "type": "integer" }
        }
      }
    },
    "compromisos": {
      "type": "object",
      "required": ["mios", "del_cliente"],
      "properties": {
        "mios": {
          "type": "array",
          "items": {
            "type": "object",
            "required": ["texto", "ts_ms"],
            "properties": {
              "texto": { "type": "string" },
              "fecha": { "type": ["string", "null"] },
              "ts_ms": { "type": "integer" }
            }
          }
        },
        "del_cliente": {
          "type": "array",
          "items": {
            "type": "object",
            "required": ["texto", "ts_ms"],
            "properties": {
              "texto": { "type": "string" },
              "fecha": { "type": ["string", "null"] },
              "ts_ms": { "type": "integer" }
            }
          }
        }
      }
    },
    "proximo_paso": {
      "type": ["object", "null"],
      "properties": {
        "texto": { "type": "string" },
        "fecha": { "type": ["string", "null"] },
        "ts_ms": { "type": "integer" }
      }
    },
    "senales": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["tipo", "cita", "ts_ms"],
        "properties": {
          "tipo":  { "enum": ["interes", "riesgo"] },
          "cita":  { "type": "string" },
          "ts_ms": { "type": "integer" }
        }
      }
    }
  }
}
```

---

## 6. Calidad y control de regresiones

Un cambio de proveedor o de prompt puede degradar el resultado sin que nadie lo note:

- **Corpus de referencia**: 10–15 grabaciones reales tuyas con apuntes y actas escritos a mano.
  Debe incluir la clase con peor audio que tengas — es el caso que hay que defender.
- **Métrica de cobertura**: qué porcentaje de los avisos, fechas y compromisos del documento de
  referencia aparecen en el generado. Los falsos negativos aquí son lo que duele.
- **Métrica de alucinación**: proporción de afirmaciones cuyo `ts_ms` no está respaldado por el
  texto real de la transcripción en esa ventana. Es implementable de forma automática y muy
  reveladora.
- **Tests de contrato por adaptador**: mismo prompt y esquema contra todos los proveedores
  configurados; verificar que la salida valida y que `Usage` se extrae bien. Bajo demanda, no en
  cada CI, porque cuesta dinero.
- **Registro de coste por sesión** visible en la UI. Ver "esta clase costó $0.02" genera
  confianza y detecta configuraciones caras por accidente.
</content>
</invoke>
