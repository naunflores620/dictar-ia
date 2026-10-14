import '../modelos/dominio.dart';

/// Una voz de la transcripción: quién, por qué canal y cuánto habló.
///
/// No se guarda en ningún sitio: se deriva de las frases. El nombre vive en
/// `Frase.hablante`, así que renombrar a alguien es reescribir esas frases en
/// el núcleo y volver a derivar la lista.
class Participante {
  const Participante({
    required this.pista,
    required this.nombre,
    required this.tiempoHablado,
    required this.frases,
  });

  final Pista pista;
  final String nombre;
  final Duration tiempoHablado;
  final int frases;
}

/// Agrupa las frases por (canal, nombre). Primero el sistema y luego el
/// micrófono; dentro de cada canal, quien más habla arriba.
///
/// El canal forma parte de la clave porque «Yo» en el micrófono y un
/// «Participante 1» del sistema nunca son la misma voz, aunque alguien las
/// llamara igual: el núcleo renombra por canal y la interfaz debe hablar igual.
List<Participante> participantesDe(List<Frase> frases) {
  final tiempo = <(Pista, String), int>{};
  final cuantas = <(Pista, String), int>{};
  for (final f in frases) {
    final clave = (f.pista, f.quien);
    final dura = f.finMs > f.inicioMs ? f.finMs - f.inicioMs : 0;
    tiempo.update(clave, (v) => v + dura, ifAbsent: () => dura);
    cuantas.update(clave, (v) => v + 1, ifAbsent: () => 1);
  }
  final lista = [
    for (final e in tiempo.entries)
      Participante(
        pista: e.key.$1,
        nombre: e.key.$2,
        tiempoHablado: Duration(milliseconds: e.value),
        frases: cuantas[e.key]!,
      ),
  ];
  lista.sort((a, b) {
    if (a.pista != b.pista) {
      return a.pista == Pista.sistema ? -1 : 1;
    }
    return b.tiempoHablado.compareTo(a.tiempoHablado);
  });
  return lista;
}
