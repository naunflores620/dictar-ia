//! Adaptación del JSON Schema al modo estricto de los proveedores.
//!
//! `schemars` genera un esquema correcto, pero no el que exigen las APIs de
//! salida estructurada. Concretamente OpenAI pide:
//!
//! - `additionalProperties: false` en **todos** los objetos.
//! - **todas** las propiedades en `required` (los campos opcionales se expresan
//!   como anulables, no como ausentes).
//! - las definiciones en `$defs`, no en `definitions`.
//!
//! Esta transformación se hace una vez aquí y sirve para todos los adaptadores,
//! en lugar de repetirla —y equivocarse— en cada uno.

use serde_json::{json, Map, Value};

/// Convierte un esquema de `schemars` en uno válido para el modo estricto.
pub fn a_estricto(mut esquema: Value) -> Value {
    // `schemars` emite `definitions`; las APIs esperan `$defs`.
    if let Some(obj) = esquema.as_object_mut() {
        if let Some(defs) = obj.remove("definitions") {
            obj.insert("$defs".to_owned(), defs);
        }
        // Ruido que algunos proveedores rechazan.
        obj.remove("$schema");
        obj.remove("title");
    }

    reescribir_refs(&mut esquema);
    endurecer(&mut esquema);
    esquema
}

/// `#/definitions/X` → `#/$defs/X`
fn reescribir_refs(v: &mut Value) {
    match v {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get_mut("$ref") {
                if let Some(resto) = r.strip_prefix("#/definitions/") {
                    *r = format!("#/$defs/{resto}");
                }
            }
            for (_, hijo) in map.iter_mut() {
                reescribir_refs(hijo);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(reescribir_refs),
        _ => {}
    }
}

/// Aplica recursivamente `additionalProperties: false` y `required` completo.
fn endurecer(v: &mut Value) {
    match v {
        Value::Object(map) => {
            let es_objeto = map
                .get("type")
                .map(|t| t == "object" || tipo_incluye(t, "object"))
                .unwrap_or(false)
                || map.contains_key("properties");

            if es_objeto {
                if let Some(Value::Object(props)) = map.get("properties") {
                    let nombres: Vec<Value> =
                        props.keys().map(|k| Value::String(k.clone())).collect();
                    map.insert("required".to_owned(), Value::Array(nombres));
                }
                map.insert("additionalProperties".to_owned(), Value::Bool(false));
            }

            for (_, hijo) in map.iter_mut() {
                endurecer(hijo);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(endurecer),
        _ => {}
    }
}

fn tipo_incluye(t: &Value, buscado: &str) -> bool {
    match t {
        Value::String(s) => s == buscado,
        Value::Array(a) => a.iter().any(|x| x.as_str() == Some(buscado)),
        _ => false,
    }
}

/// Envoltorio `response_format` de la API de OpenAI.
pub fn response_format_openai(nombre: &str, esquema: Value) -> Value {
    json!({
        "type": "json_schema",
        "json_schema": {
            "name": nombre,
            "strict": true,
            "schema": esquema,
        }
    })
}

/// Gemini usa `responseSchema` y un subconjunto de JSON Schema: no admite
/// `$defs` ni `$ref`, así que hay que aplanar las referencias antes de enviar.
pub fn response_schema_gemini(esquema: Value) -> Value {
    let defs = esquema
        .get("$defs")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));

    let mut plano = esquema;
    if let Some(obj) = plano.as_object_mut() {
        obj.remove("$defs");
    }
    // Profundidad máxima como cortafuegos ante una referencia cíclica: sin
    // ella, un esquema recursivo colgaría el proceso en lugar de fallar.
    aplanar(&mut plano, &defs, 0);
    limpiar_para_gemini(&mut plano);
    plano
}

fn aplanar(v: &mut Value, defs: &Value, profundidad: u32) {
    const MAX: u32 = 24;
    if profundidad > MAX {
        return;
    }

    match v {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref").cloned() {
                if let Some(nombre) = r.strip_prefix("#/$defs/") {
                    if let Some(destino) = defs.get(nombre) {
                        let mut copia = destino.clone();
                        aplanar(&mut copia, defs, profundidad + 1);
                        *v = copia;
                        return;
                    }
                }
            }
            for (_, hijo) in map.iter_mut() {
                aplanar(hijo, defs, profundidad + 1);
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|i| aplanar(i, defs, profundidad + 1)),
        _ => {}
    }
}

/// Gemini rechaza la petición entera si encuentra palabras clave que no
/// soporta, así que se eliminan en lugar de dejar que falle.
fn limpiar_para_gemini(v: &mut Value) {
    const NO_SOPORTADAS: &[&str] = &[
        "additionalProperties",
        "$schema",
        "definitions",
        "$defs",
        "title",
        "default",
        "examples",
        "const",
    ];

    match v {
        Value::Object(map) => {
            for k in NO_SOPORTADAS {
                map.remove(*k);
            }
            // `type: ["string","null"]` no se admite: se traduce a `nullable`.
            if let Some(Value::Array(tipos)) = map.get("type").cloned() {
                let anulable = tipos.iter().any(|t| t.as_str() == Some("null"));
                if let Some(primero) = tipos.iter().find(|t| t.as_str() != Some("null")) {
                    map.insert("type".to_owned(), primero.clone());
                    if anulable {
                        map.insert("nullable".to_owned(), Value::Bool(true));
                    }
                }
            }
            for (_, hijo) in map.iter_mut() {
                limpiar_para_gemini(hijo);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(limpiar_para_gemini),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dictar_domain::{ClassNotes, ClientMinutes};

    fn esquema_de<T: schemars::JsonSchema>() -> Value {
        serde_json::to_value(schemars::schema_for!(T)).unwrap()
    }

    #[test]
    fn el_modo_estricto_cierra_los_objetos_y_exige_todos_los_campos() {
        let e = a_estricto(esquema_de::<ClassNotes>());

        assert_eq!(e["additionalProperties"], Value::Bool(false));

        let requeridos: Vec<&str> = e["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        // En modo estricto TODOS los campos van en `required`, incluidos los
        // que en Rust son opcionales: la opcionalidad se expresa anulando.
        for campo in [
            "tldr",
            "temas",
            "conceptos",
            "avisos",
            "senales_examen",
            "formulas",
        ] {
            assert!(requeridos.contains(&campo), "falta {campo} en required");
        }
    }

    #[test]
    fn las_definiciones_pasan_a_defs_y_las_referencias_se_reescriben() {
        let e = a_estricto(esquema_de::<ClassNotes>());
        assert!(e.get("$defs").is_some());
        assert!(e.get("definitions").is_none());

        let texto = serde_json::to_string(&e).unwrap();
        assert!(
            !texto.contains("#/definitions/"),
            "quedan refs sin reescribir"
        );
        assert!(texto.contains("#/$defs/"));
    }

    #[test]
    fn los_objetos_anidados_tambien_quedan_cerrados() {
        let e = a_estricto(esquema_de::<ClassNotes>());
        let aviso = &e["$defs"]["Aviso"];
        assert_eq!(aviso["additionalProperties"], Value::Bool(false));
        assert!(aviso["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "ts_ms"));
    }

    #[test]
    fn gemini_recibe_el_esquema_aplanado_y_sin_refs() {
        let e = response_schema_gemini(a_estricto(esquema_de::<ClientMinutes>()));
        let texto = serde_json::to_string(&e).unwrap();

        // Gemini no entiende referencias: si queda una, rechaza la petición.
        assert!(!texto.contains("$ref"), "quedan referencias sin aplanar");
        assert!(!texto.contains("$defs"));
        assert!(!texto.contains("additionalProperties"));

        // Y la estructura debe seguir siendo la correcta tras aplanar.
        let compromisos = &e["properties"]["compromisos"];
        assert!(compromisos["properties"]["mios"].is_object());
        assert!(compromisos["properties"]["del_cliente"].is_object());
    }

    #[test]
    fn los_campos_opcionales_se_traducen_a_nullable_para_gemini() {
        let e = response_schema_gemini(a_estricto(esquema_de::<ClassNotes>()));
        let fecha = &e["properties"]["avisos"]["items"]["properties"]["fecha"];
        assert_eq!(fecha["type"], "string");
        assert_eq!(fecha["nullable"], Value::Bool(true));
    }

    #[test]
    fn el_envoltorio_de_openai_activa_el_modo_estricto() {
        let rf = response_format_openai("apuntes", json!({"type": "object"}));
        assert_eq!(rf["type"], "json_schema");
        assert_eq!(rf["json_schema"]["strict"], Value::Bool(true));
        assert_eq!(rf["json_schema"]["name"], "apuntes");
    }

    #[test]
    fn una_referencia_ciclica_no_cuelga_el_proceso() {
        // Un esquema autorreferente no debe provocar recursión infinita:
        // es preferible un esquema incompleto a un proceso colgado.
        let ciclico = json!({
            "type": "object",
            "properties": { "hijo": { "$ref": "#/$defs/Nodo" } },
            "$defs": { "Nodo": {
                "type": "object",
                "properties": { "hijo": { "$ref": "#/$defs/Nodo" } }
            }}
        });
        let _ = response_schema_gemini(ciclico); // no debe colgarse
    }
}
