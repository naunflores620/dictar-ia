import 'package:dictar_ia/ventana.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  // `debugDefaultTargetPlatformOverride` es un estado global de Flutter: si
  // un test lo deja puesto, el siguiente test del archivo (o de otro archivo
  // que comparta el mismo aislado) hereda una plataforma falsa sin haberla
  // pedido.
  tearDown(() => debugDefaultTargetPlatformOverride = null);

  group('Ventana.esEscritorio', () {
    // HU-11, criterios 1 y 4: la disposición amplia y las funciones que
    // dependen de una pantalla ajena (capturar diapositivas) solo tienen
    // sentido en escritorio. Antes de este archivo, ningún test llamaba a
    // este getter: ni invertir la negación (`!kIsWeb`) ni sumar
    // `TargetPlatform.android` a la lista lo habría puesto en rojo.
    for (final p in [
      TargetPlatform.linux,
      TargetPlatform.windows,
      TargetPlatform.macOS,
    ]) {
      test('$p cuenta como escritorio', () {
        debugDefaultTargetPlatformOverride = p;
        expect(Ventana.esEscritorio, isTrue);
      });
    }

    for (final p in [
      TargetPlatform.android,
      TargetPlatform.iOS,
      TargetPlatform.fuchsia,
    ]) {
      test('$p no cuenta como escritorio', () {
        // El caso real que esto evita: `ajustes.dart` y `grabacion.dart`
        // ofrecían "Área de la diapositiva" y "Capturar diapositivas" en
        // cualquier plataforma, un botón que en un móvil solo puede fallar
        // porque no hay pantalla ajena que capturar.
        debugDefaultTargetPlatformOverride = p;
        expect(Ventana.esEscritorio, isFalse);
      });
    }
  });
}
