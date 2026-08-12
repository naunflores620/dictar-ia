import 'dart:async';

import '../modelos/dominio.dart';

/// Contrato de datos de la aplicación.
///
/// Existe para que la interfaz no dependa de cómo se obtienen los datos. Hoy la
/// implementa [RepositorioDemo]; mañana lo hará el puente a Rust
/// (`flutter_rust_bridge`) sin tocar una sola pantalla.
abstract class Repositorio {
  Future<List<Topic>> topics();
  Future<List<Sesion>> sesiones({String? topicId});
  Future<Sesion> sesion(String id);

  Future<List<Frase>> transcripcion(String sesionId);
  Future<Notas?> notas(String sesionId);
  Future<List<Aviso>> pendientes();

  /// Sesiones que quedaron a medias porque la aplicación murió grabando. El
  /// audio está en disco: al arrancar hay que ofrecer recuperarlas, nunca
  /// descartarlas.
  Future<List<Sesion>> interrumpidas();

  Future<void> descartarAviso(int id);

  // -- Grabación ------------------------------------------------------------

  /// Emite la transcripción según llega. La pasada en vivo llega con
  /// `definitiva == false` y se sustituye entera al terminar.
  Stream<Frase> get fraseEnVivo;

  /// Progreso de la sesión en curso: duración y diapositivas capturadas.
  Stream<EstadoGrabacion> get estadoGrabacion;

  Future<String> iniciarGrabacion({
    required TipoSesion tipo,
    String? topicId,
    bool capturarPantalla,
  });

  Future<void> detenerGrabacion();

  /// Transcribe la sesión y genera las notas.
  ///
  /// Tarda minutos: con 4,6x tiempo real medido, una clase de dos horas son
  /// unos 26 minutos. La interfaz enseña [progreso] mientras tanto.
  Future<ResultadoProceso> procesar(String sesionId);

  /// Avance del procesado, para la barra.
  Stream<Progreso> get progreso;

  /// Guarda una captura de la pantalla en la sesión en curso.
  ///
  /// Es la vía manual: el usuario decide el momento, así que la lámina es la
  /// correcta por definición. La detección automática nunca acertará siempre.
  Future<String> capturarDiapositiva();

  // -- Reproducción ---------------------------------------------------------

  /// Empieza a reproducir la sesión. Devuelve la duración total en ms.
  Future<int> reproducir(String sesionId, int desdeMs);
  Future<void> pausarReproduccion(bool pausar);
  Future<void> saltarReproduccion(int ms);
  Future<void> detenerReproduccion();
  Future<EstadoReproduccion?> estadoReproduccion();

  /// Borra la sesión con su audio. Los apuntes exportados no se tocan.
  Future<void> borrarSesion(String sesionId);
}

/// Instantánea del procesado en curso.
class Progreso {
  const Progreso({
    required this.fase,
    required this.fraccion,
    this.detalle = '',
  });

  final String fase;
  /// De 0 a 1.
  final double fraccion;
  final String detalle;
}

class ResultadoProceso {
  const ResultadoProceso({
    required this.frases,
    required this.avisos,
    required this.costeUsd,
    required this.proveedor,
  });

  final int frases;
  final int avisos;
  final double costeUsd;
  final String proveedor;
}

/// Estado del audio que está sonando.
class EstadoReproduccion {
  const EstadoReproduccion({
    required this.posicionMs,
    required this.duracionMs,
    required this.pausado,
    required this.terminado,
  });

  final int posicionMs;
  final int duracionMs;
  final bool pausado;
  final bool terminado;
}

/// Instantánea de la sesión en curso.
class EstadoGrabacion {
  const EstadoGrabacion({
    required this.grabando,
    this.transcurrido = Duration.zero,
    this.diapositivas = 0,
    this.hablandoMicro = false,
    this.hablandoSistema = false,
  });

  final bool grabando;
  final Duration transcurrido;
  final int diapositivas;

  /// Indicadores de voz, del VAD. Sin ellos el usuario no sabe si de verdad se
  /// está captando el audio del profesor hasta que es tarde.
  final bool hablandoMicro;
  final bool hablandoSistema;

  static const parado = EstadoGrabacion(grabando: false);
}

// ---------------------------------------------------------------------------
// Implementación de demostración
// ---------------------------------------------------------------------------

/// Datos falsos para desarrollar la interfaz sin el núcleo compilado.
///
/// No es un juguete: reproduce el comportamiento que importa —transcripción que
/// llega poco a poco, sesiones en distintos estados, avisos con y sin fecha—
/// para que la interfaz se construya contra casos reales y no contra el caso
/// bonito.
class RepositorioDemo implements Repositorio {
  RepositorioDemo({DateTime? ahora}) : _ahora = ahora ?? DateTime.now();

  final DateTime _ahora;
  final _frases = StreamController<Frase>.broadcast();
  final _estado = StreamController<EstadoGrabacion>.broadcast();
  Timer? _reloj;
  DateTime? _inicio;
  int _diapositivas = 0;

  late final List<Topic> _topics = [
    Topic(
      id: 'top_calculo',
      tipo: TipoTopic.asignatura,
      nombre: 'Cálculo II',
      persona: 'Dra. Pérez',
      periodo: '2026-1',
      consentimiento: true,
    ),
    const Topic(
      id: 'top_redes',
      tipo: TipoTopic.asignatura,
      nombre: 'Redes de Computadoras',
      persona: 'Ing. Molina',
      periodo: '2026-1',
    ),
    const Topic(
      id: 'top_acme',
      tipo: TipoTopic.cliente,
      nombre: 'Acme S.A.',
      persona: 'Juan Ruiz',
      consentimiento: true,
    ),
  ];

  late final List<Sesion> _sesiones = [
    Sesion(
      id: 'ses_1',
      topicId: 'top_calculo',
      tipo: TipoSesion.clase,
      estado: EstadoSesion.lista,
      inicio: _ahora.subtract(const Duration(days: 1, hours: 3)),
      fin: _ahora.subtract(const Duration(days: 1, hours: 1, minutes: 30)),
      titulo: 'Transformada de Laplace',
      appCapturada: 'meet',
      numDiapositivas: 47,
      costeUsd: 0.026,
    ),
    Sesion(
      id: 'ses_2',
      topicId: 'top_acme',
      tipo: TipoSesion.cliente,
      estado: EstadoSesion.lista,
      inicio: _ahora.subtract(const Duration(days: 2)),
      fin: _ahora.subtract(const Duration(days: 2)).add(const Duration(minutes: 52)),
      titulo: 'Alcance de la integración',
      appCapturada: 'zoom',
      costeUsd: 0.019,
    ),
    Sesion(
      id: 'ses_3',
      topicId: 'top_redes',
      tipo: TipoSesion.clase,
      estado: EstadoSesion.transcribiendo,
      inicio: _ahora.subtract(const Duration(hours: 4)),
      fin: _ahora.subtract(const Duration(hours: 2, minutes: 15)),
      titulo: 'Capa de enlace',
      appCapturada: 'zoom',
      numDiapositivas: 31,
    ),
  ];

  @override
  Future<List<Topic>> topics() async => List.unmodifiable(_topics);

  @override
  Future<List<Sesion>> sesiones({String? topicId}) async {
    final l = topicId == null
        ? _sesiones
        : _sesiones.where((s) => s.topicId == topicId).toList();
    return List.unmodifiable(l..sort((a, b) => b.inicio.compareTo(a.inicio)));
  }

  @override
  Future<Sesion> sesion(String id) async =>
      _sesiones.firstWhere((s) => s.id == id);

  @override
  Future<List<Sesion>> interrumpidas() async =>
      _sesiones.where((s) => s.estado == EstadoSesion.grabando).toList();

  @override
  Future<List<Frase>> transcripcion(String sesionId) async => const [
        Frase(
          inicioMs: 12000,
          finMs: 19000,
          texto: 'Buenos días. Hoy vamos a ver la transformada de Laplace.',
          pista: Pista.sistema,
          hablante: 'Profesor',
        ),
        Frase(
          inicioMs: 100000,
          finMs: 118000,
          texto:
              'La definimos como la integral de cero a infinito de f(t) por e '
              'elevado a menos st, dt.',
          pista: Pista.sistema,
          hablante: 'Profesor',
        ),
        Frase(
          inicioMs: 255000,
          finMs: 264000,
          texto:
              'Esto es importante: la región de convergencia siempre cae en el examen.',
          pista: Pista.sistema,
          hablante: 'Profesor',
        ),
        Frase(
          inicioMs: 570000,
          finMs: 576000,
          texto: '¿La región de convergencia depende del signo de sigma?',
          pista: Pista.micro,
        ),
        Frase(
          inicioMs: 595000,
          finMs: 604000,
          texto: 'Exacto, depende de la parte real de s.',
          pista: Pista.sistema,
          hablante: 'Profesor',
        ),
      ];

  @override
  Future<Notas?> notas(String sesionId) async {
    if (sesionId != 'ses_1' && sesionId != 'ses_2') return null;

    if (sesionId == 'ses_2') {
      return const Notas(
        markdown: '''
# Acme S.A. — Juan Ruiz

Revisamos el alcance de la integración. Enviaré propuesta con dos opciones de
precio antes del viernes; ellos confirman el volumen real de facturas.

## Necesidades detectadas

- Procesan unas 4.000 facturas al mes a mano _(alta)_ _[00:06:20]_
  > «se nos va medio equipo tres días al mes en esto»

## Objeciones

- ❗ El precio de la opción con soporte parece alto _[00:31:45]_

## Compromisos

### Míos

- [ ] Enviar propuesta con dos opciones — **2026-08-14** _[00:44:10]_

### Del cliente

- [ ] Confirmar el volumen real de facturas _[00:46:02]_

## Próximo paso

**Llamada de seguimiento** (2026-08-18)
''',
        tldr: 'Revisamos el alcance. Propuesta antes del viernes.',
        plantilla: 'client',
        proveedor: 'gemini-flash',
        modelo: 'gemini-2.5-flash',
        costeUsd: 0.019,
      );
    }

    return const Notas(
      markdown: '''
# Cálculo II — Dra. Pérez

Introducción a la transformada de Laplace: definición, región de convergencia y
propiedades básicas de linealidad.

## ⚠️ Avisos

- [00:22:00] Parcial del tema 3 — **2026-09-15**
- [00:31:10] Leer el capítulo 4 de Oppenheim para la semana que viene

## 📌 Marcado como importante

- **Región de convergencia** — «esto siempre cae en el examen» _[00:04:15]_

## Conceptos

- **Transformada de Laplace**: integral de cero a infinito de f(t)·e^(-st)dt _[00:01:40]_
- **Región de convergencia**: valores de s para los que la integral converge _[00:04:15]_

## Fórmulas

\$\$
F(s)=\\int_0^\\infty f(t)e^{-st}\\,dt
\$\$
Definición de la transformada. _Diapositiva #7_ · _[00:01:40]_

## Preguntas en clase

- **P:** ¿La región de convergencia depende del signo de sigma? _[00:09:30]_
  **R:** Sí, depende de la parte real de s.
''',
      tldr: 'Introducción a la transformada de Laplace.',
      plantilla: 'class',
      proveedor: 'gemini-flash',
      modelo: 'gemini-2.5-flash',
      costeUsd: 0.026,
    );
  }

  @override
  Future<List<Aviso>> pendientes() async {
    final l = <Aviso>[
      Aviso(
        id: 1,
        sesionId: 'ses_1',
        topicId: 'top_calculo',
        tipo: TipoAviso.fechaExamen,
        texto: 'Parcial del tema 3',
        fecha: _iso(_ahora.add(const Duration(days: 35))),
        tsMs: 1320000,
      ),
      Aviso(
        id: 2,
        sesionId: 'ses_2',
        topicId: 'top_acme',
        tipo: TipoAviso.compromisoMio,
        texto: 'Enviar propuesta con dos opciones de precio',
        fecha: _iso(_ahora.add(const Duration(days: 3))),
        tsMs: 2650000,
      ),
      Aviso(
        id: 3,
        sesionId: 'ses_2',
        topicId: 'top_acme',
        tipo: TipoAviso.compromisoSuyo,
        texto: 'Confirmar el volumen real de facturas',
        tsMs: 2762000,
      ),
      Aviso(
        id: 4,
        sesionId: 'ses_1',
        topicId: 'top_calculo',
        tipo: TipoAviso.pistaExamen,
        texto: 'Región de convergencia — «esto siempre cae en el examen»',
        tsMs: 255000,
      ),
    ];
    return l.where((a) => !_descartados.contains(a.id)).toList();
  }

  final _descartados = <int>{};

  @override
  Future<void> descartarAviso(int id) async => _descartados.add(id);

  @override
  Stream<Frase> get fraseEnVivo => _frases.stream;

  @override
  Stream<EstadoGrabacion> get estadoGrabacion => _estado.stream;

  @override
  Future<String> iniciarGrabacion({
    required TipoSesion tipo,
    String? topicId,
    bool capturarPantalla = true,
  }) async {
    _inicio = DateTime.now();
    _diapositivas = 0;

    const guion = [
      'Buenos días, vamos a continuar con la clase de hoy.',
      'Recordad que el parcial es el quince de septiembre.',
      'La transformada convierte una ecuación diferencial en una algebraica.',
      'Esto es importante: entra en el examen.',
      'Fijaos en la región de convergencia de la diapositiva.',
    ];
    var i = 0;

    _reloj = Timer.periodic(const Duration(seconds: 1), (t) {
      final transcurrido = DateTime.now().difference(_inicio!);

      // Cada 12 s cambia de diapositiva; cada 4 s llega una frase nueva.
      if (t.tick % 12 == 0) _diapositivas++;
      if (t.tick % 4 == 0 && i < guion.length) {
        final ms = transcurrido.inMilliseconds;
        _frases.add(Frase(
          inicioMs: ms,
          finMs: ms + 4000,
          texto: guion[i],
          pista: Pista.sistema,
          hablante: 'Profesor',
          // Llega como provisional: es la pasada en vivo.
          definitiva: false,
        ));
        i++;
      }

      _estado.add(EstadoGrabacion(
        grabando: true,
        transcurrido: transcurrido,
        diapositivas: _diapositivas,
        hablandoSistema: t.tick % 4 < 3,
      ));
    });

    return 'ses_demo';
  }

  @override
  Future<void> detenerGrabacion() async {
    _reloj?.cancel();
    _reloj = null;
    _inicio = null;
    _estado.add(EstadoGrabacion.parado);
  }

  final _progreso = StreamController<Progreso>.broadcast();

  @override
  Stream<Progreso> get progreso => _progreso.stream;

  @override
  Future<String> capturarDiapositiva() async => '/demostración/lámina.png';

  DateTime? _playDesde;
  int _playBase = 0;
  int _playDur = 0;
  bool _playPausa = false;

  @override
  Future<int> reproducir(String sesionId, int desdeMs) async {
    _playDur = 90 * 60 * 1000;
    _playBase = desdeMs;
    _playDesde = DateTime.now();
    _playPausa = false;
    return _playDur;
  }

  @override
  Future<void> pausarReproduccion(bool pausar) async {
    if (pausar && _playDesde != null) {
      _playBase += DateTime.now().difference(_playDesde!).inMilliseconds;
      _playDesde = null;
    } else if (!pausar) {
      _playDesde = DateTime.now();
    }
    _playPausa = pausar;
  }

  @override
  Future<void> saltarReproduccion(int ms) async {
    _playBase = ms;
    if (!_playPausa) _playDesde = DateTime.now();
  }

  @override
  Future<void> detenerReproduccion() async => _playDesde = null;

  @override
  Future<EstadoReproduccion?> estadoReproduccion() async {
    final extra = _playDesde == null
        ? 0
        : DateTime.now().difference(_playDesde!).inMilliseconds;
    final pos = (_playBase + extra).clamp(0, _playDur);
    return EstadoReproduccion(
      posicionMs: pos,
      duracionMs: _playDur,
      pausado: _playPausa,
      terminado: pos >= _playDur,
    );
  }

  @override
  Future<void> borrarSesion(String sesionId) async {
    _sesiones.removeWhere((s) => s.id == sesionId);
  }

  @override
  Future<ResultadoProceso> procesar(String sesionId) async {
    const fases = [
      'Preparando el audio',
      'Cargando el modelo',
      'Transcribiendo',
      'Generando notas',
      'Guardando',
    ];
    for (var i = 0; i < fases.length; i++) {
      _progreso.add(Progreso(fase: fases[i], fraccion: (i + 1) / fases.length));
      await Future<void>.delayed(const Duration(milliseconds: 700));
    }
    return const ResultadoProceso(
      frases: 42,
      avisos: 2,
      costeUsd: 0.0024,
      proveedor: 'demostración',
    );
  }

  void dispose() {
    _reloj?.cancel();
    _frases.close();
    _estado.close();
    _progreso.close();
  }

  static String _iso(DateTime d) =>
      '${d.year}-${d.month.toString().padLeft(2, '0')}-${d.day.toString().padLeft(2, '0')}';
}
