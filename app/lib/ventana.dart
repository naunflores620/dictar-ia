import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:window_manager/window_manager.dart';

/// Control del tamaño de la ventana.
///
/// La aplicación tiene dos modos de uso muy distintos y conviene que la
/// ventana lo refleje sola:
///
/// - **Consultando**: ventana normal, con el listado y los apuntes.
/// - **Grabando**: un panel pequeño en una esquina, encima de la
///   videollamada, durante la clase entera.
///
/// Pedirle al usuario que la encoja a mano cada vez sería pedirle que haga el
/// trabajo de la aplicación.
class Ventana {
  Ventana._();

  /// Tamaño del panel durante la grabación. Cabe en una esquina sin tapar la
  /// diapositiva del profesor.
  static const compacto = Size(380, 330);

  /// Tamaño normal, para consultar apuntes.
  static const normal = Size(1100, 720);

  static bool _soportado = false;
  static Size? _antesDeGrabar;

  /// Prepara la ventana al arrancar. Se llama antes de `runApp`.
  static Future<void> preparar() async {
    // Solo escritorio: en Android no hay ventana que gestionar.
    if (kIsWeb) return;
    if (!(defaultTargetPlatform == TargetPlatform.linux ||
        defaultTargetPlatform == TargetPlatform.windows ||
        defaultTargetPlatform == TargetPlatform.macOS)) {
      return;
    }

    try {
      await windowManager.ensureInitialized();
      await windowManager.waitUntilReadyToShow(
        const WindowOptions(
          size: normal,
          minimumSize: Size(300, 280),
          title: 'dictar_ia',
        ),
        () async {
          await windowManager.show();
        },
      );
      _soportado = true;
    } catch (e) {
      // Sin gestor de ventanas la aplicación funciona igual, solo que sin
      // encogerse sola. No es motivo para no arrancar.
      debugPrint('gestor de ventanas no disponible: $e');
    }
  }

  /// Encoge la ventana y la deja encima del resto, para grabar.
  ///
  /// El «siempre encima» es lo que hace que sirva: durante la clase, la
  /// ventana de Meet está en primer plano, y un panel que se esconde detrás no
  /// permite pulsar «capturar diapositiva» cuando hace falta.
  static Future<void> modoGrabacion() async {
    if (!_soportado) return;

    try {
      _antesDeGrabar = await windowManager.getSize();
      await windowManager.setSize(compacto);
      await windowManager.setAlwaysOnTop(true);
      await windowManager.setAlignment(Alignment.topRight);
    } catch (e) {
      debugPrint('no se pudo encoger la ventana: $e');
    }
  }

  /// Devuelve la ventana a su tamaño anterior.
  static Future<void> modoNormal() async {
    if (!_soportado) return;

    try {
      await windowManager.setAlwaysOnTop(false);
      await windowManager.setSize(_antesDeGrabar ?? normal);
      await windowManager.center();
      _antesDeGrabar = null;
    } catch (e) {
      debugPrint('no se pudo restaurar la ventana: $e');
    }
  }
}
