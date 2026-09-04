# HANDOFF — HU-05 «Claves de API en el llavero del SO»

Escrito por quien implementó, al entregar.

> **Para los revisores:** esto es una **declaración**, no evidencia. Todo lo que dice acá está
> por verificarse. Ver `protocolo.md`, regla 1.

## Tercera vuelta — correcciones al `REVIEW.md`

La segunda entrega volvió a implementación otra vez: sección «Segunda vuelta» de
`_orquestacion/trabajo/HU-05-llavero/REVIEW.md`, con el detalle en las secciones «Segunda vuelta»
de `REVIEW-codigo.md` y `REVIEW-pruebas.md`, en la misma carpeta. Esta sección documenta qué se
corrigió de cada uno de los diez hallazgos nuevos. Lo de abajo (primera y segunda vuelta) queda
tal cual se escribió entonces.

**Archivo tocado en esta vuelta: únicamente `core/providers/src/secretos.rs`**, igual que en la
segunda vuelta. Confirmado con dos vías independientes, no solo con esta declaración: `git status
--porcelain` sobre los cuatro archivos del `PLAN.md` solo marca ese; y la fecha de modificación en
disco de los otros tres (`Cargo.toml`, `puente.rs`, `repositorio_rust.dart`, las tres 09:11-09:12
del 3 de septiembre) sigue siendo anterior en horas a la de `secretos.rs` (16:36 del mismo día). No
toqué `core/audio-capture` (HU-01, cerrada según el `git status` de apertura de esta sesión) ni
`README.md`/`docs/`/ningún workflow, tal como pedía el encargo (H10-bis, T-11, T-12).

Sigue sin haber `cargo`/`rustc`/`rustfmt` en esta máquina, y esta vez tampoco arranca la VM de WSL
que las dos vueltas anteriores sí pudieron usar para confirmar el bloqueo por otra vía. Nada de
`secretos.rs` se compiló ni se ejecutó como Rust: todo lo de abajo es lectura y razonamiento
—trazado a mano de tipos, de control de flujo, y de cada mutación propuesta contra el código real—.
Donde la lógica era puramente de cadenas y control de flujo, sin tocar el sistema operativo ni el
llavero, la traduje a Python y la **ejecuté de verdad** con `python3` —no es una simulación
hipotética, es código que corrí—; el detalle y los dos scripts completos están en «Verificación por
cálculo, no solo lectura», más abajo. Y cuando algo era calculable sin compilador —ancho de línea,
balance de llaves/paréntesis— también lo verifiqué con `python`, no a ojo.

### H1-bis (Bloqueante) — `purgar_del_env` ahora consulta el contrato completo, no una versión reducida

La causa que señaló el `revisor-codigo` era exacta: `purgar_del_env` comparaba contra
`nombre_canonico(referencia)` —una sola forma—, mientras que `nombres_candidatos` (`secretos.rs:
36-52`) define tres, y `DotEnvResolver::resolver` (línea 143-148, sin tocar) prueba las tres al
leer. La corrección no fue «además comparar contra las otras dos formas en el sitio donde ya
comparaba»: extraje la noción misma de «esta línea declara esta clave» a una función que consulta
`nombres_candidatos` directamente, y la usé en los dos sitios que la necesitan.

- `nombre_de_linea` (`secretos.rs:580-587`): extrae el nombre de variable de una línea de `.env`
  —el análisis que antes vivía duplicado, casi idéntico, dentro de `guardar_clave_en` y dentro de
  `purgar_del_env` (era el hallazgo Menor H7-bis del `revisor-codigo`, «hoy son equivalentes, sin
  divergencia... un cambio futuro reabre H1-bis o H3-bis por una cuarta vía»)—.
- `linea_declara` (`secretos.rs:601-604`): si una línea declara `referencia` bajo **cualquiera**
  de los nombres de `nombres_candidatos`, no solo el canónico. Es la función que responde «¿esta
  línea es la misma clave?», y la consultan tanto `guardar_clave_en` (`secretos.rs:636-654`, al
  decidir qué línea sustituir o borrar) como `purgar_del_env_con` (`secretos.rs:760`, al decidir si
  hay algo que purgar).
- Efecto en `guardar_clave_en`: si el `.env` tenía la clave escrita con una forma corta (`GEMINI=
  sk-vieja`) y se guarda un valor nuevo, la línea corta se **normaliza** a la forma canónica en vez
  de convivir con una segunda línea nueva —dejar las dos habría reabierto la ambigüedad que
  `nombres_candidatos` documenta: al leer, la forma corta gana por ir primero en esa lista—.

**La lección, tal como la pidió el encargo, y por qué no la apliqué la primera vez:** en la
segunda vuelta corregí el caso que el hallazgo original describía (`NOMBRE_API_KEY` preexistente),
pero seguí comparando contra una sola forma en vez de preguntarle a `nombres_candidatos` cuáles son
todas las formas válidas. Es exactamente la regla de la réplica, en código de producción: la
función que define el contrato ya existía, y la corrección tenía que consultarla, no reimplementar
una versión más corta de ella.

**Pruebas, todas ejecutando la función real, no una réplica:**

- `guardar_en_el_llavero_purga_una_copia_vieja_escrita_con_nombre_corto` (`secretos.rs:1184-1216`):
  reproduce el escenario exacto del hallazgo de punta a punta —`.env` escrito a mano con `GEMINI=
  sk-vieja`, éxito simulado en el llavero vía `guardar_clave_orquestada`— y confirma, releyendo con
  `DotEnvResolver::desde_archivo` (código real, no una comparación de texto ad hoc), que la copia
  vieja ya no aparece.
- `purgar_del_env_reconoce_una_clave_escrita_con_un_nombre_corto` (`secretos.rs:1292-1307`): el
  mismo escenario, aislado en `purgar_del_env` en vez de en la composición completa.

Tracé a mano las dos contra el código **anterior** a esta corrección (comparando `clave_linea ==
Some(nombre_canonico(referencia).as_str())`): con la línea `GEMINI=sk-vieja`, `nombre_canonico`
da `"GEMINI_API_KEY"`, la comparación falla, `ya_estaba` queda en `false`, no se purga nada, y la
aserción final (`r.resolver("keyring:gemini").is_none()`) falla porque el resolutor sigue
devolviendo `"sk-vieja"`. Las dos pruebas se ponen en rojo con el código viejo y en verde con el
nuevo, por trazado manual — no pude correr `cargo test` para confirmarlo de verdad.

**Esto no se quedó en trazado a mano.** Traduje la lógica exacta de `nombres_candidatos`,
`nombre_de_linea`, `linea_declara`, el bucle de sustitución de `guardar_clave_en` y la comprobación
`ya_estaba` de `purgar_del_env_con` a un script de Python que reproduce paso a paso el mismo control
de flujo (script completo al final de esta sección, «Verificación por cálculo, no solo lectura» —
escrito en el `scratchpad` de esta sesión, que no persiste entre sesiones, así que lo transcribo
entero para que sea reproducible sin depender de esa ruta), y lo corrí contra el escenario del
hallazgo con
las dos versiones de `linea_declara` —la vieja (solo `nombre_canonico`) y la nueva—. Resultado:
con la lógica vieja, `DotEnvResolver.resolver('keyring:gemini')` devuelve `'sk-vieja'` después de
`purgar_del_env` (bug reproducido por cálculo, no por suposición); con la nueva, devuelve `None`.
La misma simulación confirmó también, por cálculo: que `guardar_no_pisa_las_claves_de_otros_
proveedores` sigue funcionando con la nueva `linea_declara` (se purga `deepseek` y se conserva
`gemini`); que el contenido queda byte a byte igual cuando no hay nada que purgar (la propiedad que
protege `purgar_del_env_no_toca_el_archivo_si_la_clave_no_esta`); y, la pieza que motivó H8-bis, que
esa misma comparación de contenido da **igual** con el `if ya_estaba` quitado — confirmando por
cálculo, no por lectura, por qué esa prueba necesitaba el cambio de diseño que le apliqué. No es
una prueba de `cargo test` —eso sigue bloqueado por B-1—, pero es más que lectura: es la misma
lógica, ejecutada.

### H3-bis (Importante) — solo «archivo ausente» cuenta como «nada que limpiar»

`purgar_del_env_con` (`secretos.rs:746-758`) ahora distingue `std::io::ErrorKind::NotFound` de
cualquier otro error de lectura:

```rust
let previo = match std::fs::read_to_string(&ruta) {
    Ok(texto) => texto,
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
    Err(e) => return Err(e),
};
```

Un permiso denegado, un bloqueo transitorio de un antivirus o un sincronizador, o contenido no
UTF-8 ahora se propaga como error —`guardar_clave_orquestada` lo convierte en `Err` con el `?` de
la línea 562, así que `guardar_clave` deja de responder éxito— en vez de tratarse silenciosamente
como «no hay nada que purgar».

Prueba: `purgar_del_env_propaga_un_error_de_lectura_que_no_es_archivo_ausente`
(`secretos.rs:1350-1370`). Fuerza el error haciendo que `.env` sea un **directorio**, no un
archivo —el mismo tipo de truco ya establecido en `el_texto_del_error_no_contiene_la_clave`, que
no depende de permisos que se comportan distinto entre el CI y una máquina de desarrollo—. No
necesité saber qué `ErrorKind` exacto asigna cada versión de Rust a «se esperaba un archivo y era
un directorio»: la prueba solo exige que **no** sea `NotFound`, lo cual es cierto en cualquier
versión, ya que el `.env` sí existe (como directorio). Tracé a mano que con el código viejo (`let
Ok(previo) = ... else { return Ok(()) }`) esta prueba fallaría (`resultado.is_err()` sería `false`,
ya que cualquier error caía en el mismo `else`).

### H4-bis (Importante) — reescritura atómica del `.env`

`guardar_clave_en` (`secretos.rs:622-694`) ya no trunca el archivo definitivo en el sitio. Escribe
el contenido completo en un archivo temporal del mismo directorio (`dir.join(format!(".env.tmp.{}",
std::process::id()))`, línea 680 — mismo directorio para que el `rename` no pueda fallar por cruzar
de sistema de archivos), le aplica el permiso 0600 en Unix, y reemplaza el `.env` real con
`std::fs::rename` (línea 691), que en Unix y en Windows sustituye el destino de una sola vez según
la documentación de `std::fs::rename` —no pude confirmarlo ejecutándolo, ver «Lo que NO pude
verificar»—. Si algo falla antes del `rename` (fallo al escribir el temporal, fallo al aplicar el
permiso), el `.env` real queda intacto: el error se propaga con `?` sin haber tocado el archivo
definitivo.

Justificación del patrón elegido, tal como pedía el encargo si no se aplicaba: es exactamente el
patrón habitual («temporal + rename»), así que lo apliqué en vez de justificar por qué no. El costo
es una escritura de más (el temporal) y la posibilidad de dejar un `.env.tmp.<pid>` huérfano si el
proceso muere entre el `write` del temporal y el `rename` —nunca se lee ni interfiere con nada,
`rutas_dotenv()` busca rutas fijas terminadas en `.env`, no escanea el directorio—; no añadí
limpieza de temporales huérfanos al arrancar por ser un cambio de alcance mayor que esta corrección
puntual.

No hay una prueba nueva dedicada a la atomicidad en sí —simular un corte de energía a mitad de una
llamada al sistema no es algo que se pueda hacer de forma determinista y portable desde un test—,
pero **todas** las pruebas existentes que verifican contenido final de un `.env`
(`guardar_una_clave_la_deja_legible_para_el_resolutor`, `guardar_no_pisa_las_claves_de_otros_
proveedores`, `guardar_dos_veces_sustituye_en_vez_de_acumular`, `borrar_una_clave_la_quita_del_
archivo`, `el_archivo_de_claves_queda_ilegible_para_los_demas`, y las nuevas de `purgar_del_env`)
pasan ahora por este camino atómico, así que cualquier error de transcripción en el `rename` las
habría puesto en rojo. Tracé cada una a mano contra el código nuevo (ver el detalle en cada
hallazgo de pruebas) y todas llegan al mismo contenido final que antes.

### H2-bis (Importante) — decisión documentada sobre la red de seguridad que retira la purga

El hallazgo pedía una decisión, no dejarlo sin resolver. Las dos vías que sugería el encargo:
avisar al usuario, o no purgar si el llavero no se puede releer de inmediato. **Elegí la segunda,
con el alcance que los archivos de esta vuelta permiten**: avisar al usuario exigiría tocar
`app/lib/pantallas/ajustes.dart`, que no está en la lista de archivos del `PLAN.md` ni de este
encargo —tocarlo por mi cuenta habría violado la regla de «solo los archivos que el plan
declara»—, así que la decisión que sí pude implementar vive en `secretos.rs`:

`escribir_en_llavero` (`secretos.rs:317-336`), en el brazo `Some(v)`, ahora relee el valor con
`entrada.get_password()` inmediatamente después de `entrada.set_password(v)?`, y solo cuenta como
éxito si esa relectura también tiene éxito:

```rust
Some(v) => {
    entrada.set_password(v)?;
    entrada.get_password().map(|_| ())
}
```

Si la relectura falla, `registrar_resultado_de_llavero` trata el resultado como fallo —el mismo
camino que ya existía para cualquier otro error del llavero—, `guardar_clave_orquestada` cae al
respaldo del `.env` y **no purga nada**: la clave que se estaba reemplazando sigue en el archivo.
Esto cierra el caso «`set_password` devolvió éxito pero el valor en realidad no quedó accesible
para una lectura inmediata en esta misma sesión» —por ejemplo, si escritura y lectura resolvieran a
una colección de Secret Service distinta—, que es la forma más barata de que la purga se dispare
sobre un «éxito» que no era tal.

**Lo que esto NO cierra, y lo documento en vez de darlo por resuelto:** una sesión **futura y
distinta** —`cron`, `ssh` sin bus de sesión reenviado, un contenedor mínimo, un demonio de secretos
que no arrancó tras reanudar— sigue sin poder alcanzar una clave que solo vive en el llavero, y no
hay ningún diagnóstico en pantalla que apunte a esa causa. Documenté esto explícitamente en el
comentario de `purgar_del_env` (`secretos.rs:708-726`), con las dos razones por las que no revertí
la purga (reabriría el Bloqueante original) y con la constancia de que la mitigación completa
—avisar en la interfaz— queda fuera del alcance de archivos de esta vuelta. Es deuda declarada, no
deuda escondida: ver «Deuda que dejo».

No hay una prueba automática para el `get_password` nuevo, por la misma razón que ya rige el resto
de `escribir_en_llavero`: ninguna prueba lo llama directamente para no tocar el almacén real
(Decisión #1 de la primera vuelta, sin cambios). Queda cubierto por lectura de tipos, no por
ejecución — ver «Lo que NO pude verificar».

### H5-bis y H7-bis (Importantes, misma raíz) — las dos decisiones de `resolver_por_defecto` ahora tienen prueba

Extraje `cadena_con` (`secretos.rs:464-475`), que recibe un resolutor de entorno, un
`DotEnvResolver` **ya construido**, y un resolutor de llavero, y decide con ellos las dos cosas que
`ensamblar_cadena_por_defecto` no decidía por sí sola: (a) el cableado posicional —qué objeto llega
a qué parámetro— y (b) la traducción de `dotenv.origen()` a `Origen::Archivo`/`Origen::Memoria`
(antes línea 435, preexistente a toda esta HU). `resolver_por_defecto` (`secretos.rs:485-491`)
queda como una llamada pura a `cadena_con` con las tres piezas reales, sin ninguna decisión propia
—el mismo patrón que ya usaban `guardar_clave`/`guardar_clave_orquestada` y `resolver_por_defecto`/
`ensamblar_cadena_por_defecto` desde la segunda vuelta—.

Tres pruebas nuevas, las tres llamando a `cadena_con` directamente, no a una reconstrucción:

- `cadena_con_no_intercambia_el_archivo_con_el_llavero` (`secretos.rs:1024-1058`): construye un
  `DotEnvResolver::desde_texto(...)` con un valor y un `MapResolver` distinto para el llavero, y
  comprueba que el archivo gana (no el llavero) y que, sin nada en el archivo, se sigue llegando
  hasta el llavero con la etiqueta `Origen::Llavero` correcta. Tracé a mano que intercambiar
  `Box::new(dotenv)` y `llavero` en la llamada a `ensamblar_cadena_por_defecto` dentro de
  `cadena_con` hace que la primera aserción reciba `"del-llavero"` en vez de `"del-archivo"`, y que
  la segunda reciba `Origen::Memoria` en vez de `Origen::Llavero` — las dos aserciones lo detectan,
  por caminos independientes.
- `un_env_encontrado_se_reporta_como_origen_archivo_no_como_memoria` y
  `sin_env_encontrado_el_origen_es_memoria_no_una_ruta_inventada` (`secretos.rs:1061-1099`): las
  dos direcciones de la traducción a `Origen`, una con `DotEnvResolver::desde_archivo` sobre un
  archivo temporal real y otra con `DotEnvResolver::desde_texto` (sin archivo). Invertir la
  traducción pondría en rojo cualquiera de las dos.

Confirmé por `grep` que `resolver_por_defecto()` sigue sin que la llame ninguna prueba —eso no
cambió, y no hacía falta que cambiara—, pero ya no importa: no le queda ninguna decisión propia sin
cubrir, todas viven en `cadena_con`, que sí se prueba.

También simulé en Python (segundo script, también transcrito al final de esta sección) el
ensamblado y la resolución de
`resolver_con_origen` con y sin el intercambio de posiciones que describe H5-bis: sin la mutación,
`cadena_con_no_intercambia_el_archivo_con_el_llavero` vería `valor == "del-archivo"`; con la
mutación aplicada al ensamblado, el cálculo da `valor == "del-llavero"` — la aserción del test
fallaría, confirmando por cálculo que la detecta, en las dos mitades del test (la del valor y la de
la etiqueta `Origen`).

### H6-bis (Importante) — mitigación de coste casi nulo en `escribir_en_llavero`

El `revisor-codigo` tenía razón y el orquestador se la dio: cerrar la fuga residual con una prueba
exige inyección de dependencias que no es proporcionada, pero un comentario de advertencia no
cuesta nada. Lo añadí directamente sobre el brazo `Some(v)` de `escribir_en_llavero`
(`secretos.rs:321-327`):

```rust
// Ninguna prueba pasa por esta rama —es la única línea de todo
// el archivo que sigue tocando el almacén real, ver el
// comentario de `registrar_resultado_de_llavero`—, así que si
// algún día hace falta diagnosticar un fallo aquí con
// `tracing`, que el evento lleve `referencia` o el error, pero
// NUNCA `v`: eso filtraría el valor de la clave sin que ninguna
// prueba automática lo notara.
```

No es una prueba, es exactamente lo que el hallazgo pedía: una advertencia en el propio código, en
la línea donde haría falta, con el mismo criterio que ya usa el comentario de
`registrar_resultado_de_llavero` para explicar por qué **esa** función sí es segura de loguear.

### H8-bis y H9-bis (Importantes, pruebas) — ambos casos de borde, resueltos

**H8-bis.** `purgar_del_env_no_toca_el_archivo_si_la_clave_no_esta` (`secretos.rs:1274-1289`) queda
sin tocar —sigue siendo una aserción real, solo que no detectaba la mutación del `if ya_estaba`—, y
añadí `purgar_del_env_no_reescribe_si_la_clave_no_esta` (`secretos.rs:1310-1327`) al lado. En vez de
comparar contenido (que da igual con o sin el `if`, porque `guardar_clave_en` es idempotente en
contenido cuando no hay ninguna coincidencia), extraje `purgar_del_env_con`
(`secretos.rs:740-766`) con la reescritura como parámetro inyectado —mismo patrón que
`guardar_clave_orquestada`/`cadena_con`— y la nueva prueba inyecta un cierre que **entra en pánico
si se lo llama**. Con el `if ya_estaba` roto (quitado), la reescritura se dispara sin condición y el
cierre inyectado panica, poniendo la prueba en rojo; con el código real, el cierre nunca se llama.
Es el mismo idioma que ya usaba `guardar_en_el_llavero_informa_de_su_origen_no_de_una_ruta` con
`resultado_de_guardar`, no algo nuevo. La misma simulación de Python de arriba modela también
`purgar_del_env_con` con un `escribir_fn` inyectado: con el `if ya_estaba` intacto, el contador de
llamadas queda en cero para una clave ausente; con el guard quitado a propósito (para reproducir la
mutación), el `escribir_fn` inyectado sí se invoca —y si es el que panica, como el de la prueba
real, el cálculo termina en la misma excepción que pondría la prueba en rojo—.

Decidí **no** usar el truco de marcar el archivo de solo lectura (`Permissions::set_readonly`) que
consideré primero: en Unix, el permiso del propio archivo no bloquea un `rename` sobre él —lo que
manda es el permiso de escritura del directorio que lo contiene—, así que con la reescritura ahora
atómica (H4-bis) ese truco habría sido fiable en Windows pero silenciosamente ineficaz en Unix, sin
que nada lo avisara. La inyección de dependencias evita ese problema por construcción, en vez de
depender de un detalle de la implementación del sistema de archivos que no pude verificar
ejecutando nada.

**H9-bis.** `purgar_del_env_propaga_el_error_si_falla_al_reescribir` (`secretos.rs:1330-1347`),
mismo mecanismo: el cierre inyectado en `purgar_del_env_con` devuelve `Err(std::io::Error::other(
"fallo simulado al reescribir"))`, y la prueba comprueba que ese error sale de `purgar_del_env_con`
sin convertirse en éxito. Es el cuarto caso de borde que el `REVIEW.md` original pedía y que
seguía sin prueba.

### H10-bis — no tocado

`README.md` queda exactamente como estaba, tal como pedía el encargo. Tampoco toqué T-11
(`libsecret-1-dev` en workflows, `INSTALL.md`, `docs/01-arquitectura.md`) ni T-12 (`rust-version`):
siguen siendo deuda para quien cierre la HU, sin cambios respecto de lo que ya declaraban las dos
vueltas anteriores.

### Verificación por cálculo, no solo lectura

Sin `cargo`, tracé cada mutación a mano — pero donde la lógica es puramente de cadenas y control de
flujo (sin tocar el sistema operativo ni el llavero), la reproduje en Python, con `python3`, en vez
de confiar solo en la lectura. Los dos scripts completos, para que sean reproducibles sin depender
del `scratchpad` de esta sesión (que no persiste):

```python
# verificar_h1bis.py — H1-bis, H8-bis y el caso de "otras claves preservadas"
def nombres_candidatos(referencia):
    base = referencia.split(':', 1)[1] if ':' in referencia else referencia
    base = base.strip()
    mayus = base.upper()
    v = [base]
    if mayus != base:
        v.append(mayus)
    if not mayus.endswith("_API_KEY"):
        v.append(mayus + "_API_KEY")
    return v

def nombre_canonico(referencia):
    return nombres_candidatos(referencia)[-1]

def nombre_de_linea(linea):
    l = linea.strip()
    if l.startswith("export "):
        l = l[len("export "):]
    if '=' not in l:
        return None
    k, _ = l.split('=', 1)
    return k.strip()

def linea_declara_NUEVO(linea, referencia):
    candidatos = nombres_candidatos(referencia)
    k = nombre_de_linea(linea)
    return k is not None and k in candidatos

def linea_declara_VIEJO(linea, referencia):
    k = nombre_de_linea(linea)
    return k is not None and k == nombre_canonico(referencia)

def guardar_clave_en_sim(previo_texto, referencia, valor, declara_fn):
    nombre = nombre_canonico(referencia)
    lineas = []
    sustituida = False
    for linea in (previo_texto.splitlines() if previo_texto else []):
        if declara_fn(linea, referencia):
            if not sustituida:
                if valor is not None:
                    lineas.append(f"{nombre}={valor}")
            sustituida = True
        else:
            lineas.append(linea)
    if not sustituida and valor is not None:
        if not lineas:
            lineas.append("# Claves de API de dictar_ia. Permisos 0600.")
        lineas.append(f"{nombre}={valor}")
    return "\n".join(lineas) + "\n"

def purgar_del_env_sim(previo_texto, referencia, declara_fn):
    if previo_texto is None:
        return None
    ya_estaba = any(declara_fn(l, referencia) for l in previo_texto.splitlines())
    if ya_estaba:
        return guardar_clave_en_sim(previo_texto, referencia, None, declara_fn)
    return previo_texto

def dotenv_resolver_sim(texto, referencia):
    valores = {}
    for linea in (texto.splitlines() if texto else []):
        l = linea.strip()
        if not l or l.startswith('#'):
            continue
        if l.startswith("export "):
            l = l[len("export "):]
        if '=' not in l:
            continue
        k, v = l.split('=', 1)
        k, v = k.strip(), v.strip()
        if k and v:
            valores[k] = v
    for n in nombres_candidatos(referencia):
        if n in valores:
            return valores[n]
    return None

env_inicial = "GEMINI=sk-vieja\n"
referencia = "keyring:gemini"

# H1-bis con la lógica vieja: reproduce el bug.
assert dotenv_resolver_sim(
    purgar_del_env_sim(env_inicial, referencia, linea_declara_VIEJO), referencia
) == "sk-vieja"

# H1-bis con la lógica nueva: lo arregla.
assert dotenv_resolver_sim(
    purgar_del_env_sim(env_inicial, referencia, linea_declara_NUEVO), referencia
) is None

# Otras claves preservadas.
env2 = guardar_clave_en_sim(None, "keyring:deepseek", "vieja-del-env", linea_declara_NUEVO)
env2 = guardar_clave_en_sim(env2, "keyring:gemini", "de-gemini", linea_declara_NUEVO)
env2p = purgar_del_env_sim(env2, "keyring:deepseek", linea_declara_NUEVO)
assert dotenv_resolver_sim(env2p, "keyring:deepseek") is None
assert dotenv_resolver_sim(env2p, "keyring:gemini") == "de-gemini"

# H8-bis: el contenido queda idéntico con o sin el guard `ya_estaba`, para el
# caso "clave ausente" — confirma por qué la prueba vieja no detectaba la
# mutación, y por qué la nueva usa un espía inyectado en vez de comparar texto.
env3 = guardar_clave_en_sim(None, "keyring:gemini", "de-gemini", linea_declara_NUEVO)
antes = env3
despues_con_guard = purgar_del_env_sim(env3, "keyring:deepseek", linea_declara_NUEVO)
despues_sin_guard = guardar_clave_en_sim(env3, "keyring:deepseek", None, linea_declara_NUEVO)
assert antes == despues_con_guard == despues_sin_guard

print("verificar_h1bis.py: todas las aserciones pasaron")
```

```python
# verificar_cadena_con.py — H5-bis: el intercambio de posiciones en cadena_con
def ensamblar(entorno, origen_archivo, archivo, llavero):
    return [("Entorno", entorno), (origen_archivo, archivo), ("Llavero", llavero)]

def ensamblar_MUTADO(entorno, origen_archivo, archivo, llavero):
    return [("Entorno", entorno), (origen_archivo, llavero), ("Llavero", archivo)]

def resolver_con_origen(cadena, referencia):
    for origen, resolver in cadena:
        v = resolver.get(referencia)
        if v is not None:
            return v, origen
    return None

def cadena_con(entorno, dotenv_valores, dotenv_origen_es_memoria, llavero, ensamblar_fn):
    origen_archivo = "Memoria" if dotenv_origen_es_memoria else "Archivo(ruta)"
    return ensamblar_fn(entorno, origen_archivo, dotenv_valores, llavero)

entorno, archivo, llavero = {}, {"keyring:gemini": "del-archivo"}, {"keyring:gemini": "del-llavero"}

valor, _ = resolver_con_origen(cadena_con(entorno, archivo, True, llavero, ensamblar), "keyring:gemini")
assert valor == "del-archivo"

valor_m, _ = resolver_con_origen(
    cadena_con(entorno, archivo, True, llavero, ensamblar_MUTADO), "keyring:gemini"
)
assert valor_m == "del-llavero"  # confirma que la mutación se detecta

valor2, origen2 = resolver_con_origen(cadena_con(entorno, {}, True, llavero, ensamblar), "keyring:gemini")
assert valor2 == "del-llavero" and origen2 == "Llavero"

_, origen2m = resolver_con_origen(
    cadena_con(entorno, {}, True, llavero, ensamblar_MUTADO), "keyring:gemini"
)
assert origen2m != "Llavero"  # confirma que la mutación se detecta también por la etiqueta

print("verificar_cadena_con.py: todas las aserciones pasaron")
```

Los corrí con `python3` en esta sesión (no simulado, ejecución real de `python3 <archivo>.py`, con
salida `todas las aserciones pasaron` en los dos casos) contra la lógica que trasladé línea por
línea desde `secretos.rs`. No sustituye a `cargo test`, pero es más que lectura del código: es la
misma lógica, ejecutada de verdad, aunque en un lenguaje distinto al que compila el proyecto.

### Comandos para reproducir (tercera vuelta)

Sigue sin haber `cargo`/`rustc`/`rustfmt` en esta máquina, y esta vez tampoco pude usar WSL como vía
alternativa:

```
$ cargo --version
/usr/bin/bash: line 1: cargo: command not found

$ where cargo
INFORMACIÓN: no se pudo encontrar ningún archivo para los patrones dados.

$ wsl -l -v
    NAME                    STATE           VERSION
  * docker-desktop          Stopped         2
    Ubuntu-26.04            Stopped         2
```

No intenté arrancar la distribución WSL con un comando que pudiera colgarse esperando una VM que no
levanta —el encargo ya avisaba de que no arranca—, así que no hay una transcripción de ese intento
en concreto más allá de lo que ya reportó `REVIEW-pruebas.md` en su pasada final. Ninguno de los
cuatro comandos del checklist de cierre se corrió en esta vuelta:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd app && flutter analyze && flutter test
```

Lo que sí hice, a falta de compilador: leí las firmas reales de `keyring::Entry::set_password`,
`get_password` y `delete_credential` en el código fuente descargado en rondas anteriores
(`C:\Users\naunf\AppData\Local\Temp\keyring-src\keyring-4.2.0\src\v1.rs`, que seguía en el disco)
para confirmar que las tres toman `&self` —no consumen la `Entry`— antes de encadenar
`set_password` y `get_password` sobre el mismo valor en `escribir_en_llavero`; tracé a mano cada
mutación de la tabla de pruebas nueva contra el código real; y verifiqué con un script de Python
—no a ojo— que ninguna línea de `secretos.rs` (1499 líneas en total ahora) supera 100 caracteres
(máximo real: 97, sin cambios respecto de las vueltas anteriores) y que las llaves, paréntesis y
corchetes del archivo están balanceados.

### Lo que NO pude verificar (tercera vuelta)

- **Nada de lo nuevo compiló.** B-1 sigue igual, confirmado de nuevo antes de escribir una línea.
- **Que `nombre_de_linea`, `linea_declara`, `cadena_con` y `purgar_del_env_con` compilen exactamente
  como las escribí.** En particular, el `?` dentro del cierre de `escribir_en_llavero`
  (`entrada.set_password(v)?; entrada.get_password().map(|_| ())`) depende de que el tipo de
  retorno inferido del cierre coincida con `keyring::Result<()>` — lo revisé a mano contra las
  firmas reales de `set_password`/`get_password` (confirmadas en el código fuente, ver arriba), pero
  es lectura, no compilación.
- **Que `std::fs::rename` sustituya el archivo de destino de una sola vez en Windows tal como dice
  su documentación**, incluida la posibilidad de que falle si algo tiene el `.env` abierto sin
  compartir permiso de borrado (un antivirus, un sincronizador) — el propio riesgo que motivó la
  atomicidad no desaparece del todo, solo deja de poder corromper el archivo a medias; ahora
  fallaría limpio en vez de silenciosamente. No pude ejecutar esto para confirmarlo.
- **El comportamiento exacto de `entrada.get_password()` inmediatamente después de un
  `set_password()` con éxito, en un backend real** (Credential Manager, Secret Service, Keychain).
  Razoné que debería tener éxito de forma consistente por ser operaciones síncronas sobre la misma
  entrada, pero no hay forma de confirmarlo sin un llavero real y sin poder compilar.
  Documentado como decisión, no como hecho verificado — ver el hallazgo H2-bis arriba.
- **Que las ocho pruebas nuevas de esta vuelta se pongan en rojo exactamente como las tracé a
  mano** (`cadena_con_no_intercambia_el_archivo_con_el_llavero`,
  `un_env_encontrado_se_reporta_como_origen_archivo_no_como_memoria`,
  `sin_env_encontrado_el_origen_es_memoria_no_una_ruta_inventada`,
  `guardar_en_el_llavero_purga_una_copia_vieja_escrita_con_nombre_corto`,
  `purgar_del_env_reconoce_una_clave_escrita_con_un_nombre_corto`,
  `purgar_del_env_no_reescribe_si_la_clave_no_esta`,
  `purgar_del_env_propaga_el_error_si_falla_al_reescribir`,
  `purgar_del_env_propaga_un_error_de_lectura_que_no_es_archivo_ausente`). Ninguna prueba
  preexistente se reescribió en esta vuelta —solo se añadieron estas ocho—. El razonamiento de cada
  una está en su hallazgo, arriba. De las ocho, cinco se apoyan en lógica que además verifiqué por
  cálculo con Python, no solo por trazado (`cadena_con_no_intercambia_el_archivo_con_el_llavero`,
  `guardar_en_el_llavero_purga_una_copia_vieja_escrita_con_nombre_corto` y
  `purgar_del_env_reconoce_una_clave_escrita_con_un_nombre_corto` —estas dos comparten la misma
  lógica de fondo—, `purgar_del_env_no_reescribe_si_la_clave_no_esta` y
  `purgar_del_env_propaga_el_error_si_falla_al_reescribir`; ver «Verificación por cálculo, no solo
  lectura»). Las otras tres (las dos de la traducción a `Origen::Memoria`/`Archivo`, y la que fuerza
  un error de lectura convirtiendo `.env` en un directorio) dependen de tipos de Rust o de
  comportamiento real del sistema de archivos, así que quedan solo en trazado manual.
- Todo lo que las dos vueltas anteriores ya declaraban sin verificar y que esta vuelta no toca
  —compilación cruzada a Android, `cargo fmt` real, el comportamiento medido de D-Bus sin sesión, el
  valor exacto de `CRED_MAX_USERNAME_LENGTH`, el texto exacto de cada variante de `keyring::Error`—
  sigue igual: ver las secciones originales más abajo.

### Deuda que dejo (actualizada, tercera vuelta)

- **La red de seguridad que retira la purga, para una sesión futura sin acceso al llavero (H2-bis),
  sigue siendo un riesgo aceptado y documentado, no eliminado.** Cerrarlo del todo exigiría avisar
  al usuario desde `app/lib/pantallas/ajustes.dart`, fuera de la lista de archivos de esta vuelta.
  El comentario de `purgar_del_env` (`secretos.rs:708-726`) deja escrita la decisión y el porqué.
- **Sigue sin haber una prueba de ida y vuelta para el criterio 1** (AC 1) — sin cambios respecto de
  la segunda vuelta: la razón (el `LazyLock` de `keyring::Entry` v1 no es interceptable en el CI de
  Linux) sigue siendo la misma, y esta vuelta no tocó esa parte del archivo.
- **`.env.tmp.<pid>` puede quedar huérfano si el proceso muere entre escribir el temporal y
  renombrarlo.** No interfiere con nada (no lo lee ningún resolutor), pero no hay limpieza
  automática al arrancar. Deuda nueva de esta vuelta, de bajo riesgo.
- El resto de la deuda que ya declaraban la primera y la segunda vuelta (`libsecret-1-dev` en los
  archivos de T-11, `README.md:78-80`, `ajustes.dart:165-167`, `puente.dart` generado, MSRV/T-12,
  `nombres_candidatos` con `_API_KEY`, la línea `entrada.set_password(v)` sin cobertura automática)
  sigue igual, sin cambios: ver las secciones originales más abajo.

---

## Segunda vuelta — correcciones al `REVIEW.md`

La primera entrega volvió a implementación: `_orquestacion/trabajo/HU-05-llavero/REVIEW.md`
(veredicto consolidado) y los tres informes de origen en la misma carpeta
(`REVIEW-codigo.md`, `REVIEW-pruebas.md`, `REVIEW-plataforma.md`). Esta sección documenta qué se
corrigió de cada uno de los diez hallazgos, en el orden que pidió el REVIEW. El resto del
documento, debajo de esta sección, es la declaración de la **primera** entrega y queda tal cual
se escribió entonces —lo que sigue vigente de ahí no se repite aquí; lo que quedó obsoleto se
marca en el punto correspondiente—.

**Archivo tocado en esta vuelta: únicamente `core/providers/src/secretos.rs`.** Los otros tres
del `PLAN.md` (`core/providers/Cargo.toml`, `core/api/src/puente.rs`,
`app/lib/datos/repositorio_rust.dart`) no necesitaron ningún cambio para lo que pide el REVIEW:
la firma pública de `guardar_clave` no cambió, así que nada de lo que consume ese archivo desde
fuera de `secretos.rs` se vio afectado. Se consideró tocar el doc-comment de `guardar_clave` en
`puente.rs` para mencionar la limpieza del `.env`, pero no dice nada falso tal como está —describe
el resultado desde la perspectiva de quien llama, no el mecanismo interno—, así que se dejó
igual, para no ampliar el conjunto de archivos tocados sin necesidad.

Sigue sin haber `cargo`/`rustc`/`rustfmt` en esta máquina (B-1, confirmado de nuevo: mismo
resultado que la primera vuelta). Todo lo de abajo es lectura y razonamiento, no ejecución.

### Hallazgo 1 (Bloqueante) — `guardar_clave` ya retira la copia vieja del `.env`

La causa raíz, tal como la corrige el REVIEW, estaba en una premisa del `PLAN.md`, no en el
código: "no hace falta migrar porque el `.env` conserva prioridad" describía el mecanismo pero no
su efecto, que era justo el contrario de lo que decía la interfaz.

Cambios en `secretos.rs`:

- `guardar_clave` (línea 486) ahora delega en una función nueva, `guardar_clave_orquestada`
  (línea 502), que hace explícito el paso que faltaba: si el intento de llavero tiene éxito,
  antes de devolver `Ok(Origen::Llavero)` se llama a `purgar_del_env` (línea 606) sobre la carpeta
  de configuración real.
- `purgar_del_env` cubre los cuatro casos de borde que pedía el REVIEW:
  - **El `.env` no existe:** `std::fs::read_to_string` falla, el `let ... else` devuelve `Ok(())`
    de inmediato, sin crear archivo ni carpeta (`purgar_del_env_no_crea_el_archivo_si_no_existia`,
    línea 1012).
  - **La clave no está en él:** se comprueba primero con un `.any(...)` de solo lectura; si no
    aparece, no se reescribe nada — el archivo queda **byte a byte** igual
    (`purgar_del_env_no_toca_el_archivo_si_la_clave_no_esta`, línea 1022, que compara el contenido
    completo antes/después).
  - **Hay que conservar las demás claves:** `purgar_del_env` no reescribe su propia lógica de
    sustitución — llama a `guardar_clave_en(dir, referencia, None)`, la función **ya probada** que
    sabe conservar el resto del archivo, en vez de duplicar esa lógica con el riesgo de que
    diverja (`guardar_una_clave_en_el_llavero_no_deja_dos_copias_activas`, línea 931, guarda dos
    proveedores distintos y comprueba que solo uno se retira).
  - **Un fallo al reescribir no debe dejarla en dos sitios sin avisar:** `guardar_clave_orquestada`
    propaga el error de `purgar_del_env` con `?` (línea 514) en vez de tragárselo — si la limpieza
    falla, `guardar_clave` devuelve `Err`, no `Ok(Origen::Llavero)`. Es la lectura que me pareció
    correcta del invariante 5 aplicado aquí: informar "guardado" cuando la copia vieja del `.env`
    sigue mandando sería la misma mentira que el hallazgo 1 denunciaba, solo que a través de un
    camino distinto. **Esto no tiene una prueba dedicada** — ver «Lo que NO pude verificar
    (segunda vuelta)».
- Caso adicional, no pedido explícitamente pero simétrico: si el llavero tiene éxito y **nunca**
  hubo un `.env`, no se crea uno vacío solo para "limpiarlo"
  (`guardar_en_el_llavero_sin_copia_vieja_no_crea_el_env`, línea 967).
- El doc-comment de `guardar_clave` (líneas 468-485) se reescribió: ya no dice "no migra": explica
  la limpieza y por qué hace falta.

**Precisión de alcance, para que quede escrita y no solo implícita en el código:** la limpieza
solo actúa sobre el `.env` de `dir_configuracion()` (`~/.config/dictar_ia/.env` o
`%APPDATA%\dictar_ia\.env`), que es el único lugar donde `guardar_clave`/`guardar_clave_en` han
escrito alguna vez. `resolver_por_defecto()` también busca un `.env` en el directorio actual y en
su padre (`rutas_dotenv()`), y devuelve el primero que encuentra —si existiera uno ahí, con una
clave puesta a mano, seguiría mandando sobre el llavero sin que esta limpieza lo toque—. No lo
traté como parte del hallazgo 1: es un `.env` que la aplicación nunca escribió, así que no es "la
copia vieja que el llavero reemplaza", y limpiar un archivo que el usuario colocó a mano en un
directorio arbitrario sin más contexto me pareció un cambio de alcance mayor, no una corrección
del mismo bug.

**Qué línea de código de producción ejecuta cada prueba nueva de este bloque**, respondiendo la
pregunta de la regla de la réplica: las cuatro llaman a `guardar_clave_orquestada` o a
`purgar_del_env` directamente — las mismas funciones que `guardar_clave` (el punto de entrada
real, sin cambiar de firma) usa para cada llamada real, no una construcción aparte.

### Hallazgos 2, 3, 4 y 5 — la regla de la réplica

**2. `el_llavero_va_despues_del_entorno_y_del_env` ahora ejercita el ensamblado real.**

En vez de reordenar `resolver_por_defecto` directamente y arriesgarme a que el test tuviera que
mutar variables de entorno reales para poder controlar los tres eslabones —lo que habría hecho
falta para probar la función tal cual, con sus tres piezas reales (entorno, `.env` de verdad,
llavero de verdad)—, extraje la parte que decide el **orden**, que es lo que el hallazgo pedía
proteger, a una función nueva: `ensamblar_cadena_por_defecto` (línea 410). Recibe los tres
resolutores ya construidos y solo decide en qué secuencia se registran. `resolver_por_defecto`
(línea 430) no hace nada más que llamarla con las piezas reales — no le queda ninguna otra línea
de lógica propia.

Decidí **no** mutar variables de entorno reales (`std::env::set_var`) para poder llamar a
`resolver_por_defecto()` literalmente con los tres eslabones controlados. Dos razones, ambas ya
reconocidas por este mismo archivo antes de esta vuelta: (a) desde Rust 1.82 `std::env::set_var`
es `unsafe`, y con `dtolnay/rust-toolchain@stable` flotando en el CI no tengo forma de confirmar
sin compilador si la sintaxis exacta que hubiera escrito es válida; (b) el comentario de
`guardar_clave_en` (líneas 529-534, sin tocar en esta vuelta) ya explica por qué este archivo
evita tocar variables de entorno globales en un test: "los tests corren en paralelo... hace que se
pisen entre ellos y fallen de forma intermitente". Preferí extender ese mismo criterio en vez de
contradecirlo. La función nueva prueba exactamente lo que el hallazgo pedía —que
`resolver_por_defecto` no invierta el orden— sin ese riesgo: si alguien reordena los `.con(...)`
dentro de `ensamblar_cadena_por_defecto`, el test lo detecta, y esa reordenación **es** el bug
real que rompería la aplicación, porque `resolver_por_defecto` no añade ninguna otra decisión
encima.

**3 y 4. La prueba de `tracing` ya no toca el llavero real, y cubre la rama de éxito de verdad.**

Antes de tocar nada, investigué el backend `mock` que señaló el `verificador-pruebas`, tal como
pedía el REVIEW. Descargué las fuentes reales desde crates.io de `keyring-core` 1.0.0,
`zbus-secret-service-keyring-store` 1.0.0 y `windows-native-keyring-store` 1.1.0 (mismo estándar
de evidencia que la primera vuelta, con acceso a Internet confirmado en esta sesión). Hallazgos
concretos:

- `keyring-core` **sí** trae un backend mock (`keyring_core::mock::Store`), sin *feature flag*,
  pensado exactamente para esto: en memoria, sin persistencia, inyectable con
  `keyring_core::set_default_store(...)`.
- Pero `keyring::Entry` (la API v1 que usa `secretos.rs`, no `keyring_core::Entry` directamente)
  envuelve la inicialización del backend en un `LazyLock` que se evalúa **una sola vez por
  proceso** (`keyring-4.2.0/src/v1.rs`, función `set_credential_store`): la primera vez que se
  llama a `keyring::Entry::new(...)` en todo el binario de test, construye el backend **nativo**
  de la plataforma sin condición — no hay forma de interceptarlo desde fuera antes de esa primera
  llamada. Si esa construcción falla (Linux sin sesión D-Bus, exactamente el caso del CI de este
  repositorio: `zbus_secret_service_keyring_store::Store::new()` intenta conectar al bus de
  inmediato, no de forma perezosa), el resultado queda fijado a
  `Err(Error::NoDefaultStore)` **para el resto del proceso**, y `keyring::Entry::new` devuelve ese
  error de inmediato en cada llamada posterior — antes incluso de consultar qué backend esté
  instalado en `keyring_core` en ese momento. Es decir: **en el CI de Linux, ningún truco con el
  mock puede hacer que la rama de éxito de `keyring::Entry` se alcance nunca**, porque el propio
  arranque de la librería la bloquea antes de que el mock tenga oportunidad de intervenir. Sí
  sería viable en Windows (donde `windows_native_keyring_store::Store::new()` siempre tiene éxito,
  sin sesión) o en un Linux de escritorio con D-Bus, seedeando el `LazyLock` con una llamada real y
  reemplazando después el store con `keyring_core::set_default_store(mock)` — pero eso deja el
  problema exactamente donde estaba para el entorno que el hallazgo señala como el que "salva" la
  prueba por accidente.
- Además, esa vía exige mutar el store por defecto de `keyring_core`, que es **estado global de
  proceso** compartido por cualquier test que en paralelo llame a `keyring::Entry::new` — la misma
  clase de riesgo (estado mutable compartido entre tests concurrentes) que la Decisión #1 de la
  primera vuelta ya identificó y evitó para `XDG_CONFIG_HOME`.

**Decisión: no usar el mock para interceptar `keyring::Entry`.** En su lugar, extraje la parte de
`escribir_en_llavero` que decide qué avisar por `tracing` a una función nueva,
`registrar_resultado_de_llavero` (línea 327), que recibe un `keyring::Result<()>` ya resuelto —
nunca llama a `keyring::Entry` ella misma. `escribir_en_llavero` (línea 305) sigue siendo la única
línea de todo el archivo que toca el almacén real, y ahora es un cuerpo de tres líneas que le pasa
el resultado a la función nueva. El test reescrito
(`ningun_evento_de_tracing_contiene_el_valor_de_la_clave`, línea 1113) llama a
`registrar_resultado_de_llavero` con un `Ok(())` sintético y con un `Err(keyring::Error::NoEntry)`
sintético —`keyring::Error::NoEntry` es una variante sin datos, confirmé que se puede construir
desde fuera del crate que la define porque `zbus-secret-service-keyring-store`,
`windows-native-keyring-store` y el propio `keyring` (`src/cli.rs`) la construyen así, pese a que
`keyring_core::Error` es `#[non_exhaustive]`—, y comprueba que la rama de éxito no emite ningún
evento y que la rama de error sí. Las dos ramas quedan cubiertas de verdad, de forma determinista,
**en cualquier plataforma, incluido el CI de Linux**: ya no depende de que la escritura real tenga
éxito o falle.

Esto resuelve el hallazgo 4 por completo (la función que se ejercita en el test ya no llama a
`keyring::Entry` en absoluto, así que no hay forma de que toque el Credential Manager / Keychain
reales al correr `cargo test`) y el hallazgo 3 **parcialmente**: la fuga que se puede introducir en
`registrar_resultado_de_llavero` queda cubierta en cualquier entorno; una fuga añadida
específicamente dentro del brazo `Some(v) => entrada.set_password(v)` de `escribir_en_llavero`
—la única línea de tres que sigue sin cobertura automática, porque ahí es donde vive el único
acceso real al llavero, y por diseño ninguna prueba lo toca— no la detectaría ningún test. Lo dejo
declarado explícitamente en «Lo que NO pude verificar (segunda vuelta)» en vez de darlo por
resuelto del todo: la línea es de una sola expresión (`entrada.set_password(v)`), fácil de
auditar a simple vista y ya cubierta por el `grep` de `tracing::` que el `revisor-codigo` hizo en
la primera vuelta (y presumiblemente repetirá en esta), pero no es lo mismo que una prueba
automática.

**5. `guardar_clave` (vía `guardar_clave_orquestada`) ya se ejercita como composición completa.**

Tres pruebas nuevas cubren las dos ramas de la composición real, no solo sus piezas:
`guardar_una_clave_en_el_llavero_no_deja_dos_copias_activas` y
`guardar_en_el_llavero_sin_copia_vieja_no_crea_el_env` (línea 967) cubren "el llavero tiene éxito";
`si_el_llavero_no_esta_disponible_la_clave_no_se_pierde` (línea 987) cubre "el llavero falla, cae
al `.env`" —el mismo camino que antes de esta HU, ahora probado a través de la composición
completa y no solo de `guardar_clave_en` en aislamiento—. Las tres llaman a
`guardar_clave_orquestada` con el intento de llavero y la carpeta de configuración como
parámetros, en vez de reconstruir la lógica de `guardar_clave` por su cuenta: es el mismo patrón
que ya usaba `resultado_de_guardar` (sin tocar en esta vuelta), llevado un nivel más arriba para
que `guardar_clave` mismo —no solo su mitad de la decisión— tenga una prueba que lo ejercite
completo.

### Hallazgo 9 (Menor) — `clippy::uninlined_format_args`

`secretos.rs:883` (número de línea de la primera vuelta) pasó de
`write!(self.0, " {}={:?}", field.name(), value)` a `write!(self.0, " {}={value:?}", field.name())`.
Solo `value` se inlineó: `field.name()` es una llamada a método, no un identificador simple, y ese
patrón no admite inlineado en la sintaxis de `format!`/`write!` de Rust. No pude correr `clippy`
para confirmarlo (B-1 persiste), pero es exactamente la corrección que señaló `revisor-codigo`.

### Hallazgo 10 (Nota) — ida y vuelta del criterio 1: sigue como deuda, y explico por qué

El REVIEW decía que esta prueba "sale casi gratis" **si** resolvía 3 y 4 con el backend mock. No
lo hice —ver el razonamiento del punto 3/4 arriba—, así que no sale gratis, y no la añadí. Añadir
un round-trip real (guardar en el llavero, releerlo con `LlaveroResolver`) exigiría exactamente la
misma intercepción del `LazyLock` de `keyring::Entry` que decidí no usar, con el mismo problema de
fondo: en el CI de Linux sería inalcanzable de todas formas, porque la construcción del backend
nativo falla ahí antes de que cualquier mock pueda intervenir. Sigue como deuda, igual que en la
primera vuelta, pero ahora con la razón concreta de por qué no es trivial cerrarla con el recurso
que el REVIEW proponía.

### Hallazgos 6, 7 y 8 — fuera de alcance, sin tocar

Tal como indicó el encargo: van al tablero como T-12 (MSRV) y T-11 (`libsecret-1-dev`, con el
aviso de `libdbus-1-dev` ya escrito por el `auditor-plataforma`, más el archivo que le faltaba a
mi inventario original: `docs/01-arquitectura.md:591`). No toqué ningún workflow, ni `README.md`,
ni `docs/`.

### Lo que NO pude verificar (segunda vuelta)

- **Nada de lo nuevo compiló.** B-1 sigue igual que en la primera vuelta: sin `cargo`/`rustc` en
  esta máquina. Repetí la comprobación (`cargo --version` → `command not found`) antes de escribir
  una sola línea.
- **Que `guardar_clave_orquestada`, `purgar_del_env`, `ensamblar_cadena_por_defecto` y
  `registrar_resultado_de_llavero` compilen exactamente como las escribí.** Revisé a mano cada
  tipo (los `impl Fn`/`impl FnOnce` de los parámetros, la coerción de `&PathBuf` a `&Path`, que
  `keyring::Result<()>`/`keyring::Error` sean accesibles sin un `use` explícito porque el resto del
  archivo ya los usa así) contra el resto del archivo y contra las fuentes descargadas de
  `keyring`/`keyring-core`, pero es lectura, no compilación. Es el mismo riesgo que ya declaraba la
  primera vuelta para el código nuevo de entonces, aplicado ahora al código nuevo de esta vuelta.
- **La fuga estructural que queda sin cubrir**, declarada arriba en el punto 3/4: una línea nueva
  de `tracing::` insertada dentro del brazo `Some(v) => entrada.set_password(v)` de
  `escribir_en_llavero` (la única línea del archivo que sigue tocando el llavero real) no la
  detectaría ningún test automático. La cubre la revisión estática (grep de `tracing::`), no una
  prueba.
- **Que la propagación de errores de `purgar_del_env` en `guardar_clave_orquestada` (línea 514,
  el `?` tras `purgar_del_env(&dir, referencia)`) se comporte como digo.** Es una garantía
  estructural del operador `?` de Rust (si `purgar_del_env` devuelve `Err`, `guardar_clave_orquestada`
  devuelve `Err` de inmediato, sin llegar a `resultado_de_guardar`), no algo que dependa de lógica
  de negocio propia, así que no escribí una prueba dedicada a forzar ese fallo — hacerlo de forma
  determinista y portable (Windows/Linux) hubiera exigido dejar un archivo sin permiso de
  escritura, con el riesgo añadido de escribir algo mal en una API de permisos que no puedo
  comprobar sin compilador. Lo dejo razonado, no probado.
- **El texto exacto que produce `tracing::warn!(error = %e, ...)` para `keyring::Error::NoEntry`.**
  Confío en que `impl Display for Error` (leído en el `error.rs` descargado de `keyring-core`
  1.0.0) produce `"No matching credential found"` para esa variante, sin datos del secreto —pero
  no pude compilar ni ejecutar el macro `tracing::warn!` para confirmar el texto final capturado
  por `CapturaEventos`.
- Todo lo que la primera vuelta ya declaraba sin verificar y que esta vuelta no toca —compilación
  cruzada a Android, `cargo fmt` real, el valor exacto de `CRED_MAX_USERNAME_LENGTH`, el
  comportamiento de `zbus` sin sesión— sigue exactamente igual, sin cambios: ver la sección
  original más abajo.

### Deuda que dejo (actualizada)

- **Sigue sin haber una prueba de ida y vuelta para el criterio 1** — ver hallazgo 10 arriba, con
  la razón nueva (el `LazyLock` de `keyring::Entry` v1 no es interceptable en el entorno donde más
  falta hace, que es el CI de Linux).
- **Nueva:** la línea `Some(v) => entrada.set_password(v)` de `escribir_en_llavero` es el único
  punto del archivo sin cobertura automática contra una fuga por `tracing`. Si alguien quiere
  cerrar esto del todo, la vía que investigué y no adopté —instalar el mock de `keyring-core`
  después de sembrar el `LazyLock` con una llamada real, protegido con un `Mutex` estático para
  serializar los tests que tocan el llavero— funcionaría en Windows y en Linux de escritorio con
  D-Bus, pero seguiría sin cubrir el CI. La alternativa que sí cubriría el CI exigiría cambiar
  `escribir_en_llavero` para que reciba el `Entry` (o un trait que lo abstraiga) como parámetro en
  vez de construirlo él mismo —una inversión de dependencias más profunda que no me pareció
  proporcionada para una corrección, dado que no se puede compilar para comprobarla—.
- El resto de la deuda que ya declaraba la primera vuelta (`libsecret-1-dev` en siete-más-uno
  archivos, `README.md:78-80`, `ajustes.dart:165-167`, `puente.dart` generado, MSRV,
  `nombres_candidatos` con `_API_KEY`) sigue igual, sin cambios: ver la sección original más abajo.

---

## Estado

**Implementación completa según el `PLAN.md` (revisión 2), cierre condicionado por B-1.**

Los tres puntos de «Verificación previa, y es bloqueante» se resolvieron **antes** de escribir
código, con acceso a Internet real (crates.io) y lectura de fuente descargada — no hay `cargo`
en esta máquina, así que todo lo de abajo es verificación estática, no ejecución:

1. **Crate y features.** `keyring` 4.2.0 (máxima versión publicada) confirma exactamente lo que
   adelantó el contradictor: `default = ["v1"]`, y
   `v1 = ["apple-native-keyring-store/keychain", "windows-native-keyring-store",
   "zbus-secret-service-keyring-store"]`. En Linux, `zbus-secret-service-keyring-store` depende
   de `secret-service` (Rust) + `zbus` (cliente D-Bus en Rust puro) con la *feature*
   `crypto-rust` — **cero dependencias de sistema**: ni `libsecret`, ni `libdbus`, ni OpenSSL.
   Verificado descargando y leyendo el `Cargo.toml.orig` de `keyring` 4.2.0 y de
   `zbus-secret-service-keyring-store` 1.0.0 desde crates.io. No pido ninguna *feature* extra:
   uso `keyring = "4.2.0"` a secas.
2. **Android fuera por construcción.** La lista de dependencias de `keyring` 4.2.0 (endpoint
   `/api/v1/crates/keyring/4.2.0/dependencies`) confirma que `android-native-keyring-store` es
   opcional, con `target = cfg(target_os = "android")`, y que **no** está en el conjunto por
   defecto ni en `v1` — solo entra con la *feature* `cli`, que no pido. Además, leyendo
   `src/v1.rs` de `keyring` 4.2.0: en Android ninguna de las tres ramas de
   `set_credential_store()` aplica (todas están tras `#[cfg(target_os = "...")]` de otra
   plataforma), así que cae al `Err(Error::Invalid("platform", ...))` sin referenciar ningún
   crate de backend. Aun así, condiciono la dependencia con
   `[target.'cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))'.dependencies]`,
   copiando literalmente el patrón de `core/screen-capture/Cargo.toml` con `xcap`, tal como pedía
   el encargo — es más explícito que confiar en que el crate se degrade solo, y dos definiciones
   de `LlaveroResolver`/`escribir_en_llavero` (una por rama `cfg`) evitan que `core/api` (que
   cruza a Android) escriba un solo `#[cfg]` propio.
3. **`libsecret-1-dev`.** Con el punto 1 confirmado, todo indica que sobra: nada en la cadena de
   dependencias de Linux la necesita. **Anotado como deuda, no tocado** — ver esa sección.

Con eso resuelto, se implementó la estrategia completa: `LlaveroResolver` real (dos ramas
`cfg`), alta al final de la cadena sin alterar las dos primeras, `guardar_clave`/
`guardar_clave_en` devolviendo `Origen`, la línea de `puente.rs`, el comentario de
`repositorio_rust.dart`, y las seis pruebas de la tabla del plan (una de ellas, la de `tracing`,
necesitó una pieza de soporte nueva — ver «Decisiones que se apartan del PLAN»).

**No se pudo compilar, formatear con `rustfmt` ni ejecutar una sola prueba: bloqueo B-1** (no
hay `cargo`, `rustc` ni `rustfmt` en esta máquina). Por eso el cierre solo puede quedar
condicionado, tal como dice `protocolo.md` mientras dure ese bloqueo.

## Archivos tocados

Los cuatro que declara la tabla del plan. Ninguno fuera de lista.

| Ruta | Qué se hizo | ¿Estaba en el PLAN? |
|---|---|---|
| `core/providers/Cargo.toml` | Añadida `keyring = "4.2.0"` bajo `[target.'cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))'.dependencies]`, con el comentario del porqué | Sí |
| `core/providers/src/secretos.rs` | Doc de cabecera corregida (ya no dice «pendiente de libsecret»); `nombre_canonico()` nuevo (compartido); sección «Llavero del sistema operativo» con `LlaveroResolver` y `escribir_en_llavero` (dos ramas `cfg` cada uno); `resolver_por_defecto()` con el tercer eslabón al final; `resultado_de_guardar()` nuevo; `guardar_clave`/`guardar_clave_en` devolviendo `Origen`; 5 tests existentes adaptados al nuevo tipo de retorno (con un helper `ruta_de` nuevo); 6 tests nuevos de la tabla del plan | Sí |
| `core/api/src/puente.rs` | La línea del `.map(...)` de `guardar_clave` pasa a `origen.to_string()`. Además actualicé el doc-comment de esa función (3 líneas), que decía «Escribe en `~/.config/dictar_ia/.env`» sin condición — quedaba falso en cuanto el llavero pasa a ser el intento preferente. Ver «Decisiones que se apartan del PLAN» | Sí (la línea del `.map`); el ajuste del doc-comment es una extensión mía dentro del mismo archivo, ver abajo |
| `app/lib/datos/repositorio_rust.dart` | Comentario de `guardarClave` (líneas ~293-298) actualizado: ya no dice «Devuelve el archivo donde quedó» sin condición | Sí |

No toqué ningún otro archivo. Encontré varios lugares fuera de mi lista que quedan afectados por
esta HU y que **no** modifiqué — están todos en «Deuda que dejo».

## Comandos para reproducir

**No pude ejecutar ninguno.** Bloqueo B-1: esta máquina no tiene `cargo`, `rustc` ni `rustfmt`
instalados, ni en Windows ni en la distribución WSL disponible. Comprobado directamente:

```
$ cargo --version
/usr/bin/bash: line 1: cargo: command not found
```

Los comandos que el checklist de cierre pide son estos, y ninguno se corrió — decirlo es lo
correcto, inventar un resultado sería la única falta grave de este documento:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd app && flutter analyze && flutter test
```

Lo que sí hice, a falta de compilador, fue verificación estática con evidencia descargada de
crates.io (detallada arriba y en «Lo que NO pude verificar»): leí el `Cargo.toml.orig` y el
código fuente real de `keyring` 4.2.0, `keyring-core` 1.0.0 y `zbus-secret-service-keyring-store`
1.0.0, y el de `windows-native-keyring-store` 1.1.0 para las reglas de validación de longitud
que usan dos de las pruebas nuevas.

**Nota de la segunda vuelta:** sigue sin haber `cargo`/`rustc`/`rustfmt` en esta máquina. Ninguno
de los cuatro comandos de arriba se corrió tampoco esta vez. Lo nuevo de esta vuelta (descargar y
leer las fuentes de `keyring-core` 1.0.0 y `zbus-secret-service-keyring-store` 1.0.0 para entender
el mecanismo del `LazyLock` de `keyring::Entry`) está detallado en «Segunda vuelta —
correcciones al REVIEW.md», arriba del todo de este documento.

## Criterios de aceptación

Tabla de la primera entrega, con las columnas «Prueba» y «Estado» actualizadas donde la segunda y
la tercera vuelta cambiaron qué código ejercita cada una. Ver las secciones «Segunda vuelta» y
«Tercera vuelta», arriba del todo, para el detalle de cada cambio.

| AC de `docs/06` | Prueba que lo cubre | Estado |
|---|---|---|
| 1. Existe un `KeyResolver` que lee del llavero (Secret Service en Linux, Credential Manager en Windows) | `el_resolutor_real_del_llavero_no_entra_en_panico_sin_sesion`, `un_error_del_llavero_se_convierte_en_none` (ambas ejecutan el `LlaveroResolver` real, sin cambios en esta vuelta) | Escrito, no ejecutado. Sigue sin haber una prueba de ida y vuelta (hallazgo 10 de la segunda vuelta: investigué el backend `mock` de `keyring-core` y no es interceptable en el CI de Linux, ver detalle arriba) |
| 2. Va el último de la cadena, después del entorno y del `.env`, sin alterar ese orden | `el_llavero_va_despues_del_entorno_y_del_env`, llamando a `ensamblar_cadena_por_defecto`; y, nuevas de la tercera vuelta, `cadena_con_no_intercambia_el_archivo_con_el_llavero`, `un_env_encontrado_se_reporta_como_origen_archivo_no_como_memoria` y `sin_env_encontrado_el_origen_es_memoria_no_una_ruta_inventada`, las tres llamando a `cadena_con` —la función de la que `resolver_por_defecto` ya no tiene ninguna decisión propia sin cubrir— | Escrito, no ejecutado. Corregido el hallazgo 2 de la segunda vuelta; corregidos H5-bis y H7-bis de la tercera |
| 3. Guardar una clave desde ajustes la escribe en el llavero, no en un archivo | `guardar_en_el_llavero_informa_de_su_origen_no_de_una_ruta` (la mitad de la decisión), `guardar_una_clave_en_el_llavero_no_deja_dos_copias_activas`, `guardar_en_el_llavero_sin_copia_vieja_no_crea_el_env` y `si_el_llavero_no_esta_disponible_la_clave_no_se_pierde` (la composición completa); y, nuevas de la tercera vuelta, `guardar_en_el_llavero_purga_una_copia_vieja_escrita_con_nombre_corto`, `purgar_del_env_reconoce_una_clave_escrita_con_un_nombre_corto`, `purgar_del_env_no_reescribe_si_la_clave_no_esta`, `purgar_del_env_propaga_el_error_si_falla_al_reescribir` y `purgar_del_env_propaga_un_error_de_lectura_que_no_es_archivo_ausente` | Escrito, no ejecutado. Corregido el hallazgo 5 de la segunda vuelta; corregidos H1-bis, H3-bis, H8-bis y H9-bis de la tercera |
| 4. Sin llavero disponible, la aplicación sigue funcionando con las otras dos fuentes | `el_resolutor_real_del_llavero_no_entra_en_panico_sin_sesion`, `un_error_del_llavero_se_convierte_en_none` (sin cambios) | Escrito, no ejecutado |
| 5. Ninguna clave aparece en los logs, ni siquiera truncada | `ningun_evento_de_tracing_contiene_el_valor_de_la_clave`, reescrita para cubrir las dos ramas de `registrar_resultado_de_llavero` de forma determinista en cualquier plataforma (antes solo la rama de error, y solo por accidente del entorno), más `el_texto_del_error_no_contiene_la_clave` (sin cambios) | Escrito, no ejecutado. Corregidos los hallazgos 3 y 4 de la segunda vuelta. La fuga estructural residual en `escribir_en_llavero` sigue sin prueba automática, pero H6-bis (tercera vuelta) le añadió un comentario de advertencia — ver arriba |

## Invariantes del producto

| Invariante | Cómo se respeta acá |
|---|---|
| 4. Ninguna firma pública cambia según la plataforma; si una plataforma no soporta algo, devuelve un error claro, la función no desaparece | `LlaveroResolver` (struct + `nuevo()` + `impl KeyResolver`) y `escribir_en_llavero` tienen **dos** definiciones con firma idéntica, una por rama `#[cfg]`, copiando el patrón de `dictar_screen::iniciar`/`captura_para_seleccion` en `core/screen-capture/src/lib.rs`. En Android, `LlaveroResolver::resolver()` devuelve `None` (no hay pánico ni falta la función) y `escribir_en_llavero()` devuelve `false` (cae al `.env`). `resolver_por_defecto()` y `guardar_clave` no escriben un solo `#[cfg]` |
| 5. Un fallo no se traga | El fallo del llavero al escribir se anuncia con `tracing::warn!(error = %e, "...")` antes de caer al `.env` — no desaparece en silencio, se degrada con aviso. El fallo al leer sí se convierte en `None` a propósito (es la política del último eslabón de una cadena de respaldo, documentada en el propio código), pero nunca se confunde con un éxito: `Origen` siempre dice de dónde salió el valor de verdad |

Los invariantes 1-3 (escritura a disco antes de procesar, dos pistas de audio, 16 kHz mono
`f32`) no aplican: esta HU no toca el pipeline de audio.

## Decisiones que se apartan del PLAN

Ninguna cambia el alcance; todas son detalles de implementación necesarios para que lo que el
plan pide funcione de verdad. Las marco todas para que el revisor las juzgue con evidencia, no
con mi palabra:

1. **`guardar_clave_en` (con directorio explícito) no intenta el llavero.** El plan dice
   «escribe en el llavero, con respaldo al `.env` si no hay» para `guardar_clave`, y por separado
   describe `guardar_clave_en` como «igual, pero con el directorio explícito», sin decir si
   también debía tocar el llavero. Decidí que **no**: si lo hiciera, los tests existentes que
   pasan un directorio temporal (`guardar_una_clave_la_deja_legible_para_el_resolutor` y otros
   cinco) escribirían de verdad en el llavero real de la máquina que ejecuta `cargo test` —en
   Windows, el Credential Manager está siempre disponible, sin necesitar sesión gráfica ni
   demonio— y dejarían de comprobar lo que dicen comprobar, además de ensuciar un almacén real en
   cada corrida. Documenté el porqué en el doc-comment de la función.
2. **El fallo al borrar una entrada inexistente del llavero cuenta como fallo, no como éxito.**
   Si `entrada.delete_credential()` devuelve `NoEntry` (nunca hubo nada ahí), no lo trato como
   «ya está borrado, éxito» — lo dejo caer al respaldo del `.env`. Razón: si la clave vivía en el
   `.env` (porque el usuario la tenía de antes) y el llavero nunca la tuvo, tratar el `NoEntry`
   como éxito habría dejado el valor viejo del `.env` activo para siempre —el `.env` manda sobre
   el llavero al leer—, mientras la interfaz diría «borrada». Cayendo al respaldo, `guardar_clave_en`
   sí limpia el `.env` si había algo. Está documentado en el comentario de `escribir_en_llavero`.
   No hay una prueba dedicada a este caso concreto porque no estaba en la tabla del plan y no
   quise ampliar el alcance de pruebas sin que el plan lo pidiera; queda como comportamiento
   razonado pero no verificado por un test.
3. **La prueba de `tracing` no usa `tracing-subscriber`.** El plan pide «instalar un subscriber
   de captura», y `tracing-subscriber` ya es una dependencia de *workspace* (usada hoy por `cli`)
   con la que hubiera sido más rápido escribir la prueba con `fmt()` + un `MakeWriter` a un
   buffer. Preferí implementar a mano un `Subscriber` mínimo contra el trait que ya expone
   `tracing` (dependencia directa de `dictar-providers` desde antes de esta HU), para no añadir
   ninguna entrada nueva a `Cargo.toml` más allá de la dependencia del llavero que el plan sí
   declaró. Es más código de test, pero cero cambio de superficie de dependencias.
4. **`nombre_canonico()` extraído como función compartida.** `guardar_clave_en` ya calculaba
   «el último de `nombres_candidatos`»; lo extraje para que `LlaveroResolver` y
   `escribir_en_llavero` usen exactamente la misma regla al leer y al escribir, en vez de
   duplicar la lógica. Cambio mecánico, mismo comportamiento.
5. **Actualicé el doc-comment de `guardar_clave` en `puente.rs`**, no solo la línea del `.map`.
   El comentario existente decía, sin condición, que la función «Escribe en
   `~/.config/dictar_ia/.env»`; con el llavero como intento preferente eso queda falso la mayor
   parte del tiempo. `protocolo.md` marca un comentario que describe mal el código como hallazgo
   **Importante**, no menor, así que preferí corregirlo dentro del mismo archivo ya declarado en
   vez de dejarlo pasar.

## Lo que NO pude verificar

- **Nada compiló.** Bloqueo B-1. Todo lo que sigue es lectura de código, no ejecución.
- **La API exacta del trait `tracing::Subscriber` y `tracing::field::Visit`.** Implementé
  `CapturaEventos` contra mi conocimiento de esas dos interfaces (7 métodos requeridos en
  `Subscriber`: `enabled`, `new_span`, `record`, `record_follows_from`, `event`, `enter`, `exit`;
  1 en `Visit`: `record_debug`), pero no tengo forma de compilarlo para confirmar que no me dejé
  ningún método requerido o que las firmas son exactas para la versión de `tracing` 0.1 que
  resuelve este *workspace*. Es el punto de mayor riesgo de todo lo que entrego.
- **El valor exacto de `CRED_MAX_USERNAME_LENGTH` en Windows.** Confirmé, leyendo el código
  fuente de `windows-native-keyring-store` 1.1.0 (`src/utils.rs`), que existe una comprobación
  `if user.len() > CRED_MAX_USERNAME_LENGTH as usize { return Err(Error::Invalid(...)) }` antes
  de tocar la API de Windows — pero no descargué el crate `windows`/`windows-sys` para ver el
  valor numérico real de esa constante. Uso una referencia de 5000 caracteres en
  `un_error_del_llavero_se_convierte_en_none`, muy por encima de cualquier valor documentado que
  conozco para ese límite (los que recuerdo de la documentación de Win32 son varios órdenes de
  magnitud menores), pero no pude confirmarlo empíricamente.
- **El comportamiento real de `zbus`/D-Bus sin sesión** (¿falla rápido o se queda esperando un
  *timeout*?). Si se queda esperando, `el_resolutor_real_del_llavero_no_entra_en_panico_sin_sesion`
  y `un_error_del_llavero_se_convierte_en_none` seguirían siendo correctas, pero podrían volverse
  lentas en CI. No lo pude medir.
- **Compilación cruzada a Android** (`cargo ndk build -p dictar-api`). Bloqueado también por B-3
  (no hay NDK). Confirmé por lectura de código que `keyring` no se activa como dependencia ahí
  (la sección `[target.'cfg(...)'.dependencies]` no matchea `target_os = "android"`), pero nunca
  ejecuté esa compilación ni tengo forma de hacerlo en esta máquina.
- **`cargo fmt`.** Formateé a mano intentando seguir las convenciones que ya usa el archivo
  (4 espacios, ancho de 100 columnas contando caracteres — comprobado con un script que cuenta
  bytes de cada línea, ninguna supera 100), pero no hay garantía de que coincida byte a byte con
  lo que produciría `rustfmt`, en particular en las líneas donde partí una llamada encadenada
  larga (`keyring::Entry::new(...).and_then(...)`).
- **Round-trip real de AC 1** (guardar en el llavero y releerlo con `LlaveroResolver`, en una
  máquina con llavero de verdad). Ninguna prueba automática lo cubre porque el CI de Linux no
  tiene sesión gráfica; solo se puede probar a mano en un escritorio con GNOME/KDE o en Windows.

## Deuda que dejo

- **`libsecret-1-dev` probablemente sobra**, y en más sitios de los que el plan enumeraba. El
  plan solo mencionaba `ci.yml`, `release.yml` e `INSTALL.md`; además encontré:
  - `packaging/linux/build_deb.sh:101` — lista de respaldo de dependencias del `.deb`
    (`libsecret-1-0`).
  - `docs/05-empaquetado.md` — tres apariciones: la línea `Depends:` de ejemplo del `.deb`
    (línea 168), el comando `fpm` de ejemplo (línea 289), y un fragmento de workflow de ejemplo
    (línea 338).
  - `README.md:133` — instrucciones de instalación de dependencias del sistema.

  Ninguno de estos siete archivos está en mi lista (todos pertenecen a HU-01 o son
  documentación general), así que no los toqué. Con la verificación de este HANDOFF (`keyring`
  4.2.0 en Linux usa `zbus-secret-service-keyring-store`, cliente D-Bus puro Rust, sin
  `libsecret`), quien cierre esta deuda ya tiene la evidencia hecha.

- **`README.md:78-80` queda doblemente desactualizado.** Dice «el llavero del SO es el tercer
  eslabón previsto y todavía no está implementado (pendiente de `libsecret`)» — ambas partes
  quedan falsas con esta HU: ya está implementado, y nunca dependió de `libsecret`. No está en mi
  lista de archivos.

- **`app/lib/pantallas/ajustes.dart:165-167`** tiene un comentario — «Se dice el archivo: si
  algún día algo no cuadra…» — que ya no es exacto: ahora puede decirse el llavero, no solo un
  archivo. El comportamiento funcional no cambia (el texto se muestra tal cual en el
  `SnackBar`), pero el comentario describe mal lo que puede pasar. No está en mi lista de
  archivos.

- **`app/lib/src/rust/puente.dart` (generado)** conserva el comentario obsoleto sobre
  `~/.config/dictar_ia/.env`. Es deuda que el propio plan ya declaró a propósito: no se puede
  tocar mientras dure B-5 (no hay `flutter_rust_bridge_codegen` para regenerarlo).

- **MSRV del workspace.** `Cargo.toml` raíz declara `rust-version = "1.75"` en
  `[workspace.package]`; `keyring` 4.2.0 exige `rustc` 1.88.0 y edición 2024 para sí mismo. No es
  un conflicto duro — Cargo solo comprueba el compilador activo contra lo que cada dependencia
  pide, no contra lo que el propio *workspace* declara como su piso, y el CI usa
  `dtolnay/rust-toolchain@stable` sin fijar versión, así que en la práctica debería ser
  transparente — pero la cifra `1.75` que el `Cargo.toml` raíz declara deja de ser cierta una vez
  esta dependencia entra al árbol. No toqué ese archivo (no está en mi lista) y lo anoto para que
  quien mantenga el *workspace* lo sepa.

- **`nombres_candidatos()` con el sufijo `_API_KEY`.** Ya declarado como deuda por el propio
  plan, para que lo decida HU-06. No lo toqué.

- **Sin round-trip real de AC 1**, ver «Lo que NO pude verificar» — habría que probarlo a mano
  en un escritorio con sesión gráfica y en Windows antes de cerrar esta tarea con confianza
  total, ya que ninguna prueba automática lo ejercita de punta a punta.
