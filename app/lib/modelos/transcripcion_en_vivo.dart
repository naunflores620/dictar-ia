import 'dart:collection';

import 'dominio.dart';

/// Mantiene una sola versión visible por pista e inicio de frase.
///
/// Las hipótesis parciales reemplazan su borrador. Cuando llega una frase
/// cerrada, esta retira los borradores que cubre antes de insertarse en orden
/// cronológico. La vista pública no permite modificar la lista desde la UI.
class TranscripcionEnVivo {
  final _frases = <Frase>[];
  late final List<Frase> frases = UnmodifiableListView(_frases);

  /// Aplica un evento y devuelve `true` si cambió la estructura o llegó un
  /// cierre, útil para decidir cuándo seguir el final de la lista.
  bool aplicar(Frase nueva) {
    var cambioEstructural = nueva.definitiva;
    if (nueva.definitiva || nueva.texto.trim().isEmpty) {
      final cantidadAntes = _frases.length;
      _frases.removeWhere(
        (anterior) =>
            !anterior.definitiva &&
            anterior.pista == nueva.pista &&
            anterior.inicioMs <= nueva.finMs &&
            anterior.finMs >= nueva.inicioMs,
      );
      cambioEstructural |= _frases.length != cantidadAntes;
    }

    if (nueva.texto.trim().isEmpty) return cambioEstructural;

    final indice = _buscar(nueva);
    if (indice < _frases.length && _mismaClave(_frases[indice], nueva)) {
      // Un evento parcial retrasado no debe reabrir una frase ya cerrada.
      if (_frases[indice].definitiva && !nueva.definitiva) {
        return cambioEstructural;
      }
      _frases[indice] = nueva;
      return cambioEstructural;
    }

    _frases.insert(indice, nueva);
    return true;
  }

  int _buscar(Frase frase) {
    var bajo = 0;
    var alto = _frases.length;
    while (bajo < alto) {
      final medio = bajo + ((alto - bajo) >> 1);
      if (_comparar(_frases[medio], frase) < 0) {
        bajo = medio + 1;
      } else {
        alto = medio;
      }
    }
    return bajo;
  }

  bool _mismaClave(Frase a, Frase b) =>
      a.inicioMs == b.inicioMs && a.pista == b.pista;

  int _comparar(Frase a, Frase b) {
    final porTiempo = a.inicioMs.compareTo(b.inicioMs);
    if (porTiempo != 0) return porTiempo;
    return a.pista.index.compareTo(b.pista.index);
  }
}
