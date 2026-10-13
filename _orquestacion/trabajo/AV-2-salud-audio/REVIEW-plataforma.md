# REVIEW-plataforma: AV-2 (salud de audio)

> Lo transcribe el orquestador desde el informe que el `auditor-plataforma` devolvió por mensaje. Sus instrucciones le impedían escribir el archivo. El contenido es del auditor; solo se reformateó.

## Veredicto

**Sin hallazgos bloqueantes por lectura.** `salud_audio.rs` y su uso en `grabacion.rs` no introducen `cfg`, dependencias ni código de plataforma.

La compilación de `dictar-api` completo en Windows quedó **NO VERIFICABLE** con un `target/` limpio, porque falta `cmake` para `whisper-rs-sys`. El código nuevo se compiló aislado en tres targets y salió limpio.

## Matriz

| Plataforma | Compila | Qué lo impediría |
|---|---|---|
| Windows (msvc) | Sí el código nuevo, aislado. `dictar-api` entero: NO VERIFICABLE | Nada del diff. Con un `target/` limpio falla `whisper-rs-sys` por falta de `cmake` |
| Linux | Sí por lectura. El CI lo cubre | Nada del diff |
| Android (aarch64) | Sí el código nuevo, aislado. `cargo ndk` sin correr | Nada del diff |
| FreeBSD | Sí el código nuevo, aislado | El check del CI (`ci.yml:97`) solo cubre `dictar-audio`, no `dictar-api` |

## Hallazgos (todos bajos)

- `app/lib/src/rust/puente.dart:279` (generado): el doc-comment de `tipo` no menciona `"salud"`. Se corrige al regenerar el puente.
- `.github/workflows/ci.yml:97`: el check de FreeBSD solo cubre `dictar-audio`. Es preexistente.

## Premisas atacadas

- **D5 (reutilizar campos de `EventoDto`): se sostiene.** `git diff --stat f45b274 refs/revision/av-2 -- core/api/src/frb_generated.rs app/lib/src` vacío. El struct no cambia, solo hay una rama nueva en `From<Evento>`. `flutter_rust_bridge.yaml` solo apunta a `crate::puente`.
- **Una app vieja ignora `tipo: "salud"`: se sostiene.** El `switch` de sentencia sobre `String` sin `default` no es exhaustivo. App y librería además se empaquetan juntas.
- **Los `match` sobre `Evento` fuera del núcleo no se rompen.** `cli/src/sesion.rs` tiene brazo `_`.
- **Dart sin guardas de plataforma: se sostiene.** El diff no añade `Platform`, `dart:io` ni `esEscritorio`. La disposición compacta es por ancho.
- **El ejemplo `salud_audio_eval.rs` no rompe otras plataformas.** Resuelve `%APPDATA%` en tiempo de ejecución, con caída a `$HOME/.local/share`.

## Qué verificó y no marcó

- **`cfg`:** solo `#[cfg(test)]` en `salud_audio.rs`.
- **Dependencias:** ningún `Cargo.toml` ni `Cargo.lock` cambia.
- **Compilación aislada** (`git archive refs/revision/av-2`, `CARGO_TARGET_DIR` aparte):
  - `cargo check --all-targets` en Windows msvc, FreeBSD y aarch64-linux-android: `Finished`;
  - `clippy -D warnings` sin avisos;
  - `cargo test` en Windows: 31 pasan;
  - `SaludAudio: Send` compila en Android.
- **No compilado:** el cuerpo de `bucle_escritura`, porque está dentro de `dictar-api`. La integración se revisó por lectura.

## Para verificarlo de verdad

1. `cmake` en el `PATH` y `LIBCLANG_PATH=C:\Users\naunf\LLVM18\bin`, y después `cargo check -p dictar-api --all-targets`.
2. `cargo ndk -t arm64-v8a --platform 26 build --release -p dictar-api`.
3. `flutter analyze` y `flutter test`.
4. Prueba en un dispositivo: que el aviso aparezca y desaparezca.
