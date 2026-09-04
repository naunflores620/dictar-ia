# T-15 — Revisión del trabajo que implementó el orquestador

No hay `PLAN.md` ni `HANDOFF.md`, y esa ausencia **es el problema que esta tarea repara**.

Estos tres cambios los implementó el orquestador bajo la regla vieja de `protocolo.md`, que le
permitía hacer tareas chicas. El PO derogó esa regla el 2026-09-03 (ver la bitácora), así que
este trabajo quedó fuera del mecanismo: nadie lo planificó, nadie lo contradijo y nadie lo
revisó.

| # | Qué | Archivos |
|---|---|---|
| a | `README.md:93` afirmaba que el `.exe` de Inno Setup «está escrito pero desactivado hasta que Windows pueda grabar». Era falso: `release.yml` lo compila en cada tag sin condición | `README.md` |
| b | **T-6.** `Ventana.esEscritorio` nuevo —distinto de `soportado`, que indica si el gestor de ventanas respondió—, y con él se condicionan la tarjeta «Área de la diapositiva» en Ajustes y, antes de grabar, el área y el interruptor «Capturar diapositivas». Además `_capturarPantalla` pasó de arrancar en `true` a arrancar según plataforma | `app/lib/ventana.dart`, `app/lib/pantallas/ajustes.dart`, `app/lib/pantallas/grabacion.dart` |
| c | **v3-1 y v3-2 de HU-01.** La prueba `con_las_dos_pistas_vivas_la_sesion_sigue` (la cuarta combinación, que faltaba) y el encadenamiento real en `una_sesion_de_una_sola_pista_termina_si_esa_pista_muere` | `core/audio-capture/src/sincronia.rs` |

Los informes van en esta carpeta: `REVIEW-codigo.md` y `REVIEW-pruebas.md`.
