import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Lo que solo Android necesita: permisos en tiempo de ejecución y el servicio
/// en primer plano que sostiene la grabación.
///
/// El lado nativo está en `app/android/app/src/main/kotlin/.../MainActivity.kt`.
/// Se usa un `MethodChannel` propio en vez de un plugin del ecosistema porque
/// son cuarenta líneas de Kotlin, y cada dependencia nueva es un
/// `flutter pub get` más que puede bloquear — que en este proyecto ya ha
/// pasado.
///
/// **Todos los métodos son seguros de llamar en cualquier plataforma.** Fuera
/// de Android no hay nada al otro lado del canal, así que en vez de obligar a
/// cada pantalla a envolver la llamada en su propia comprobación —y a
/// equivocarse al repetirla, que es justo lo que pasó con `windowManager` y
/// obligó a crear `Ventana.soportado`— se comprueba una sola vez aquí.
class Android {
  Android._();

  static const _canal = MethodChannel('dictar/android');

  /// Si la plataforma es Android.
  ///
  /// `defaultTargetPlatform` y no `Platform.isAndroid` de `dart:io` para no
  /// romper la compilación en web ni las pruebas de widgets.
  static bool get esAndroid =>
      !kIsWeb && defaultTargetPlatform == TargetPlatform.android;

  /// Si ya está concedido el permiso de micrófono.
  ///
  /// Fuera de Android devuelve `true`: en el escritorio no hay permiso que
  /// pedir, y devolver `false` bloquearía la grabación donde hoy funciona.
  static Future<bool> tienePermisos() async {
    if (!esAndroid) return true;
    return await _invocar('tienePermisos') ?? false;
  }

  /// Pide el micrófono —y las notificaciones, en Android 13 o posterior—.
  ///
  /// Devuelve si quedó concedido el de micrófono. El de notificaciones no se
  /// exige: sin él la grabación es más frágil, pero grabar un rato es mejor
  /// que no dejar grabar.
  static Future<bool> pedirPermisos() async {
    if (!esAndroid) return true;
    return await _invocar('pedirPermisos') ?? false;
  }

  /// Arranca el servicio en primer plano.
  ///
  /// Sin él, Android corta la captura al apagar la pantalla y una reunión de
  /// una hora se queda en unos minutos.
  static Future<void> iniciarServicio() async {
    if (!esAndroid) return;
    await _invocar<void>('iniciarServicio');
  }

  static Future<void> detenerServicio() async {
    if (!esAndroid) return;
    await _invocar<void>('detenerServicio');
  }

  /// Traga los fallos del canal a propósito.
  ///
  /// Un `MissingPluginException` aquí significa que la aplicación corre sobre
  /// un Android al que no llegó este código nativo — una versión vieja
  /// instalada encima, o una prueba de widgets sin motor. Dejar que propague
  /// tumbaría la pantalla de grabación entera por no poder consultar un
  /// permiso, que es peor que seguir sin él y dejar que falle más adelante
  /// quien de verdad lo necesita.
  static Future<T?> _invocar<T>(String metodo) async {
    try {
      return await _canal.invokeMethod<T>(metodo);
    } on PlatformException catch (e) {
      debugPrint('canal de Android: $metodo falló (${e.code})');
      return null;
    } on MissingPluginException {
      debugPrint('canal de Android: $metodo no está implementado aquí');
      return null;
    }
  }
}
