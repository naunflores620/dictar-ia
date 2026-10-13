import 'package:dictar_ia/modelos/dominio.dart';
import 'package:dictar_ia/modelos/transcripcion_en_vivo.dart';
import 'package:flutter_test/flutter_test.dart';

Frase _frase({
  required int inicio,
  required int fin,
  required String texto,
  Pista pista = Pista.sistema,
  bool definitiva = false,
  int estableHasta = 0,
}) => Frase(
  inicioMs: inicio,
  finMs: fin,
  texto: texto,
  pista: pista,
  definitiva: definitiva,
  estableHasta: estableHasta,
);

void main() {
  test('una hipótesis nueva reemplaza el borrador de la misma frase', () {
    final transcripcion = TranscripcionEnVivo();

    transcripcion.aplicar(_frase(inicio: 1000, fin: 1800, texto: 'la clase'));
    transcripcion.aplicar(
      _frase(
        inicio: 1000,
        fin: 2200,
        texto: 'la clase empieza',
        estableHasta: 9,
      ),
    );

    expect(transcripcion.frases, hasLength(1));
    expect(transcripcion.frases.single.texto, 'la clase empieza');
    expect(transcripcion.frases.single.estableHasta, 9);
  });

  test(
    'una correccion del prefijo estable sustituye la frase sin duplicarla',
    () {
      final transcripcion = TranscripcionEnVivo();

      transcripcion.aplicar(
        _frase(
          inicio: 1000,
          fin: 2200,
          texto: 'el profesor explica',
          estableHasta: 'el profesor'.length,
        ),
      );
      transcripcion.aplicar(
        _frase(
          inicio: 1000,
          fin: 2500,
          texto: 'la profesora explica',
          estableHasta: 0,
        ),
      );

      expect(transcripcion.frases, hasLength(1));
      expect(transcripcion.frases.single.texto, 'la profesora explica');
      expect(transcripcion.frases.single.estableHasta, 0);
    },
  );

  test('la frase cerrada reemplaza los borradores que cubre', () {
    final transcripcion = TranscripcionEnVivo();

    transcripcion.aplicar(
      _frase(inicio: 1000, fin: 2400, texto: 'Internet de las cosas'),
    );
    transcripcion.aplicar(
      _frase(
        inicio: 1000,
        fin: 2800,
        texto: 'Internet of Things',
        definitiva: true,
      ),
    );

    expect(transcripcion.frases, hasLength(1));
    expect(transcripcion.frases.single.texto, 'Internet of Things');
    expect(transcripcion.frases.single.definitiva, isTrue);
  });

  test('un parcial retrasado no reabre una frase definitiva', () {
    final transcripcion = TranscripcionEnVivo();

    transcripcion.aplicar(
      _frase(
        inicio: 1000,
        fin: 2800,
        texto: 'texto corregido',
        definitiva: true,
      ),
    );
    transcripcion.aplicar(
      _frase(inicio: 1000, fin: 2200, texto: 'texto anterior'),
    );

    expect(transcripcion.frases, hasLength(1));
    expect(transcripcion.frases.single.texto, 'texto corregido');
    expect(transcripcion.frases.single.definitiva, isTrue);
  });

  test('ordena cronológicamente y separa pistas con el mismo inicio', () {
    final transcripcion = TranscripcionEnVivo();

    transcripcion.aplicar(_frase(inicio: 3000, fin: 3800, texto: 'tercera'));
    transcripcion.aplicar(
      _frase(inicio: 1000, fin: 1800, texto: 'micrófono', pista: Pista.micro),
    );
    transcripcion.aplicar(
      _frase(inicio: 1000, fin: 1800, texto: 'sistema', pista: Pista.sistema),
    );

    expect(transcripcion.frases.map((f) => (f.inicioMs, f.pista)), [
      (1000, Pista.micro),
      (1000, Pista.sistema),
      (3000, Pista.sistema),
    ]);
  });

  test('un marcador vacío retira solo el borrador de su pista', () {
    final transcripcion = TranscripcionEnVivo();

    transcripcion.aplicar(
      _frase(inicio: 1000, fin: 1800, texto: 'voz del profesor'),
    );
    transcripcion.aplicar(
      _frase(
        inicio: 1000,
        fin: 1800,
        texto: 'ruido de fondo',
        pista: Pista.micro,
      ),
    );
    transcripcion.aplicar(
      _frase(
        inicio: 1000,
        fin: 1800,
        texto: '',
        pista: Pista.micro,
        definitiva: true,
      ),
    );

    expect(transcripcion.frases, hasLength(1));
    expect(transcripcion.frases.single.texto, 'voz del profesor');
  });
}
