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

  /// Si hay un gestor de ventanas con el que hablar.
  ///
  /// Lo consulta cualquiera que vaya a llamar a `windowManager` por su cuenta:
  /// en Android no hay implementación y cada llamada lanza
  /// `MissingPluginException`, así que preguntar aquí evita repetir la
  /// comprobación de plataforma —y equivocarse al repetirla— en cada pantalla.
  ///
  /// Distinto de [esEscritorio]: esto es si el gestor **respondió**, no si la
  /// plataforma lo tiene. En un Linux sin sesión gráfica sería `false` aunque
  /// siga siendo escritorio.
  static bool get soportado => _soportado;

  /// Si la plataforma es de escritorio.
  ///
  /// Es la pregunta que hay que hacer para decidir si ofrecer una función que
  /// depende de que exista una pantalla ajena: capturar diapositivas solo
  /// tiene sentido donde el profesor comparte pantalla, y eso es el portátil,
  /// no el móvil. Se comprueba la plataforma y no [soportado] a propósito: la
  /// captura de pantalla la hace `core/screen-capture`, que no depende del
  /// gestor de ventanas, y usar `soportado` escondería la función en un
  /// escritorio donde el gestor falló por otro motivo.
  static bool get esEscritorio =>
      !kIsWeb &&
      (defaultTargetPlatform == TargetPlatform.linux ||
          defaultTargetPlatform == TargetPlatform.windows ||
          defaultTargetPlatform == TargetPlatform.macOS);

  /// Prepara la ventana al arrancar. Se llama antes de `runApp`.
  static Future<void> preparar() async {
    // Solo escritorio: en Android no hay ventana que gestionar.
    if (!esEscritorio) return;

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
