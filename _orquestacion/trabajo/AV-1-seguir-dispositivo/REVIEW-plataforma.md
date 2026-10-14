# REVIEW-plataforma: AV-1 (seguir el dispositivo)

> Lo transcribe el orquestador desde el informe que el `auditor-plataforma` devolvió por mensaje. Sus instrucciones le impedían escribir el archivo. El contenido es del auditor; solo se reformateó. Revisión `refs/revision/av-1` (`f6d4cc6`) contra `b7f93eb`.

## Veredicto

**APTO en lo estático y en lo que se pudo compilar aquí.** No hay rotura multiplataforma. Quedan sin verificar:

- Linux (PipeWire): no se pudo compilar por infraestructura.
- `dictar-api` y el ejemplo: faltan libclang y cmake.
- La ejecución real de WASAPI.

## Matriz

| Plataforma | Compila | Evidencia |
|---|---|---|
| Windows | Sí, `dictar-audio` | `check --all-targets` sin avisos; `cargo test -p dictar-audio` 80/80, también con `--test-threads=1`, con `CARGO_TARGET_DIR` aislado |
| Android (aarch64 y armv7) | Sí, `dictar-audio` | `check --all-targets` sin avisos; `aaudio_src.rs` compila contra el `sincronia.rs` nuevo |
| NoSoportada (FreeBSD) | Sí | `check --all-targets` sin avisos |
| Linux (PipeWire) | NO VERIFICABLE | Sin `annotate-snippets` offline, sin pkg-config ni libclang. Revisado por lectura (B-1) |
| `dictar-api`, CLI y ejemplo | NO VERIFICABLE | `whisper-rs-sys` necesita libclang. El ejemplo se revisó por lectura (B-2) |

Clippy da un único aviso: `unnecessary_cast` en `core/audio-capture/src/lib.rs:304`. Es preexistente y ajeno a AV-1 (deuda T-20).

## Hallazgos

### Medio

**M-1. El `target/` compartido contamina los resultados de las pruebas.**

- Con `CARGO_TARGET_DIR=D:\dev_v1\dictar_ia\target` salieron 8 fallos deterministas en `sincronia::tests::*`. Un `println!` de diagnóstico añadido al código ni siquiera apareció en la salida.
- Con un `target/` propio y el mismo código: 80/80.
- Causa: las copias con la misma estructura y los mismos nombres de crate (`-av1-mut`, donde se estaba mutando a la vez) se pisan el binario de pruebas. Cargo calcula el hash relativo a la raíz del workspace.
- **Ninguna cifra sacada del `target/` compartido sirve como evidencia de AV-1.** Afecta al protocolo de verificación, no al código.

### Bajos

- **B-1.** Nada fuera de Windows dependía de `alguna_pista_sigue_viva` ni de `debe_terminar`:
  - `git grep` sin usos;
  - la única función `pub` eliminada es `alguna_pista_sigue_viva`;
  - solo `wasapi_src.rs` y `aaudio_src.rs` usan `sincronia::`;
  - `aaudio_src.rs` solo cambia un comentario.
- **B-2.** Los `#[cfg]` del ejemplo `wasapi_loopback_eval.rs` son correctos: `main` de reemplazo fuera de Windows y todos los ítems de primer nivel bajo `cfg(windows)`. El `Subscriber` hecho a mano implementa los 7 métodos obligatorios. Detalle cosmético: `new_span` devuelve siempre `Id 1`.
- **B-3.** No se tocó ningún `Cargo.toml` ni `Cargo.lock`, y no hacen falta features nuevas del crate `windows`.
- **B-4.** `sincronia.rs` es portable (solo usa `Duration`, `String` y `Option`). `PistaCapturada::nueva` delega en `continuar(…, 0)` con la misma firma. Hay una truncación teórica preexistente en armv7 tras días de sesión.
- **B-5.** El cambio de política (la sesión ya no termina sola) no rompe contratos:
  - solo cambia en Windows;
  - `grabacion.rs` sale por `parar` o por `Disconnected`, con `Niveles` congelado como único efecto, que es el aviso de AV-2;
  - `cli/src/grabar.rs` mejora: desaparece el caso de giro al 100 % de CPU de T-18 en Windows;
  - `detener()` envía, espera al hilo, y el relleno final llega antes de cerrar el canal.

## Premisas cuestionadas

1. «Las comprobaciones pasan con el `target/` compartido»: no es fiable (M-1).
2. Los tests de `sincronia` no cubren los `cfg` de `lib.rs`. Aquí no hace falta.
3. `cargo check -p dictar-audio --all-targets` **no** compila `core/api/examples`. El ejemplo solo se compila con `dictar-api`, y aquí no se pudo.
4. El 4551 del ejemplo declarado en HANDOFF no se reprodujo: el ejemplo no se ejecutó.

## Para verificarlo de verdad

1. libclang (`LIBCLANG_PATH`) para compilar `dictar-api` y el ejemplo en Windows.
2. Linux con `libpipewire-0.3-dev` y `libclang-dev`, o el CI.
3. Un `CARGO_TARGET_DIR` por worktree, o serializar las compilaciones (M-1).
4. Prueba manual con dos dispositivos de salida y dos de entrada y una cuenta de administrador.
5. `cargo ndk build` para confirmar el enlazado de `aaudio_src.rs`.
6. Corregir `lib.rs:304` (T-20).
