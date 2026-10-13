# Pruebas del PO: instalador de la tanda «audio en vivo»

**Instalador:** `dist/dictar_ia-0.1.0-windows-setup.exe` (13/10, 04:18). SHA-256 `b5a8717601b12d83116331b65fc7bd7cc6cbc4fa5d98dd0ce56094f7e040d92a`. Compilado desde la rama `audio-vivo` en `51f929f`.

El instalador anterior, del 08/10, quedó en `dist/dictar_ia-0.1.0-windows-setup.anterior-2026-10-08.exe`, por si hay que comparar.

> Si Smart App Control bloquea el instalador (T-17, no está firmado), avisa: no hay que desactivarlo.

## Qué trae de nuevo

- **AV-2, avisos durante la grabación:**
  - no llega audio del sistema;
  - el micrófono dejó de enviar audio;
  - volumen de la clase muy bajo;
  - saturación;
  - eco (el micrófono recoge los altavoces).
- **AV-1, la grabación sigue al dispositivo activo.** Si cambias de salida o de micrófono, o se desconecta uno, la grabación se reabre sola y **no termina**. Cada bloque de audio lleva la hora en que se capturó.
- **AV-3: no cambia nada por defecto.** La nivelación existe pero va apagada (ver la prueba 8, opcional).

## Pruebas

Marca cada una con ✅, ❌ o «no pude», y anota lo raro. Graba con la ventana amplia visible.

| # | Cómo | Qué debería pasar |
|---|---|---|
| 1 | **Clase normal con auriculares**, 3 min o más, con el vídeo en el navegador | Ningún aviso. La transcripción en vivo avanza como antes |
| 2 | Empezar a grabar **sin reproducir nada** durante 40 s | A los ~30 s: «No llega audio del sistema…». Desaparece al reproducir algo |
| 3 | Reproducir un vídeo con voz **por los altavoces**, con el micrófono abierto | Antes de ~40 s: aviso de eco, «Usa auriculares…» |
| 4 | Con un vídeo sonando, **bajar el volumen de esa aplicación** en el Mezclador de volumen de Windows (no el volumen general) | Aviso de volumen muy bajo. Anota también si al bajar el volumen **general** el aviso aparece o no |
| 5 | Con un vídeo sonando, **cambiar la salida** (por ejemplo de altavoces a auriculares Bluetooth o USB) a mitad de grabación, y volver | La grabación **no se corta**. Al escuchar la sesión después, el audio del sistema sigue tras cada cambio, sin quedar mudo ni desfasado más de unos 3 s |
| 6 | **Desconectar** unos auriculares USB o Bluetooth durante la grabación (y volver a conectarlos) | No se corta. Si eran también el micrófono: «El micrófono dejó de enviar audio» a los ~10 s, que desaparece al volver |
| 7 | **Detener** cualquier grabación | La banda de avisos se vacía y el procesado de apuntes sigue como siempre |
| 8 | *(Opcional)* Probar la nivelación: crear la variable de usuario `DICTAR_STT_NIVELAR=1` (Configuración → Variables de entorno), abrir la app y grabar una clase en **Español** | Comparar a ojo si la transcripción en vivo sale mejor o peor que en la prueba 1. Después borrar la variable |

## Qué devolver

- El resultado de cada prueba.
- Si algo falla: la carpeta de la sesión, `%APPDATA%\dictar_ia\audio\ses_…` (`mic.wav`, `system.wav` y `stt_metrics.jsonl`).
- **Para la próxima tanda (AV-4 a AV-7):** 10 minutos de una clase transcritos a mano.
