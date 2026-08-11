import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';

import '../modelos/dominio.dart';
import '../src/rust/frb_generated.dart';
import '../src/rust/puente.dart' as rust;
import 'repositorio.dart';

/// Repositorio respaldado por el núcleo Rust.
///
/// Traduce entre los tipos planos que cruzan el FFI (`*Dto`) y los del dominio
/// de la interfaz. Esa capa de traducción parece burocracia, pero es lo que
/// permite que las pantallas no sepan nada del puente: si mañana cambia la
/// forma de un DTO, se arregla aquí y no en cinco widgets.
class RepositorioRust implements Repositorio {
  RepositorioRust._();

  /// Abre el núcleo y devuelve el repositorio listo para usar.
  ///
  /// [dirDatos] en `null` usa la carpeta estándar del sistema.
  static Future<RepositorioRust> abrir({String? dirDatos}) async {
    await Nucleo.init();

    // `DICTAR_DATOS` permite apuntar a otro almacén sin recompilar. Es lo que
    // hace posible probar contra grabaciones reales durante el desarrollo sin
    // ensuciar los datos de uso diario.
    final dir = dirDatos ?? Platform.environment['DICTAR_DATOS'];
    final ruta = await rust.inicializar(dirDatos: dir);

    final repo = RepositorioRust._();
    final n = (await rust.listarSesiones(limite: 500)).length;
    debugPrint('núcleo abierto en $ruta · $n sesión(es)');
    return repo;
  }

  final _frases = StreamController<Frase>.broadcast();
  final _estado = StreamController<EstadoGrabacion>.broadcast();
  final _progreso = StreamController<Progreso>.broadcast();
  Timer? _sondeo;

  // -- Consulta -------------------------------------------------------------

  @override
  Future<List<Topic>> topics() async {
    final l = await rust.listarTopics();
    return l
        .map((t) => Topic(
              id: t.id,
              tipo: t.kind == 'client' ? TipoTopic.cliente : TipoTopic.asignatura,
              nombre: t.name,
              persona: t.person,
              periodo: t.term,
              consentimiento: t.consentAck,
            ))
        .toList();
  }

  @override
  Future<List<Sesion>> sesiones({String? topicId}) async {
    final l = await rust.listarSesiones(limite: 200);
    final s = l.map(_aSesion).toList();
    if (topicId == null) return s;
    return s.where((x) => x.topicId == topicId).toList();
  }

  @override
  Future<Sesion> sesion(String id) async {
    final todas = await sesiones();
    return todas.firstWhere((s) => s.id == id);
  }

  @override
  Future<List<Sesion>> interrumpidas() async =>
      (await rust.sesionesInterrumpidas()).map(_aSesion).toList();

  @override
  Future<List<Frase>> transcripcion(String sesionId) async {
    final l = await rust.obtenerTranscripcion(sessionId: sesionId);
    return l.map(_aFrase).toList();
  }

  @override
  Future<Notas?> notas(String sesionId) async {
    final md = await rust.obtenerNotas(sessionId: sesionId);
    if (md == null) return null;

    // El núcleo devuelve el Markdown ya renderizado. El resumen de una línea
    // se toma del primer párrafo, que por construcción es el «tldr».
    final tldr = md
        .split('\n')
        .firstWhere((l) => l.trim().isNotEmpty && !l.startsWith('#'),
            orElse: () => '');

    return Notas(
      markdown: md,
      tldr: tldr,
      plantilla: '',
      proveedor: '',
      modelo: '',
    );
  }

  @override
  Future<List<Aviso>> pendientes() async {
    final l = await rust.listarPendientes();
    return l
        .map((a) => Aviso(
              id: a.id.toInt(),
              sesionId: a.sessionId,
              topicId: a.topicId,
              tipo: TipoAviso.desdeClave(a.kind),
              texto: a.texto,
              fecha: a.fecha,
              tsMs: a.tsMs.toInt(),
            ))
        .toList();
  }

  @override
  Future<void> descartarAviso(int id) => rust.descartarAviso(id: id);

  Future<List<Frase>> buscar(String consulta) async =>
      (await rust.buscar(consulta: consulta)).map(_aFrase).toList();

  // -- Grabación ------------------------------------------------------------

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
    final id = await rust.iniciarGrabacion(
      tipo: tipo.clave,
      topicId: topicId,
      titulo: null,
      capturarSistema: true,
      soloLocal: false,
      ahoraMs: DateTime.now().millisecondsSinceEpoch,
    );

    _arrancarSondeo();
    return id;
  }

  @override
  Future<void> detenerGrabacion() async {
    _sondeo?.cancel();
    _sondeo = null;
    await rust.detenerGrabacion(ahoraMs: DateTime.now().millisecondsSinceEpoch);
    _estado.add(EstadoGrabacion.parado);
  }

  @override
  Stream<Progreso> get progreso => _progreso.stream;

  /// Procesa una sesión ya grabada: transcribe y genera las notas.
  ///
  /// El generador ejecuta las funciones de Rust en su propio hilo, así que
  /// esta llamada no bloquea la interfaz aunque tarde minutos. El sondeo
  /// sigue corriendo en paralelo para alimentar la barra de progreso.
  @override
  Future<ResultadoProceso> procesar(String sesionId) async {
    _arrancarSondeo();
    try {
      final r = await rust.procesarSesion(
        sessionId: sesionId,
        ahoraMs: DateTime.now().millisecondsSinceEpoch,
      );
      return ResultadoProceso(
        frases: r.frases.toInt(),
        avisos: r.avisos.toInt(),
        costeUsd: r.costeUsd,
        proveedor: r.proveedor,
      );
    } finally {
      _sondeo?.cancel();
      _sondeo = null;
    }
  }

  Future<List<rust.ProveedorDto>> proveedores() => rust.listarProveedores();

  /// Vacía la cola de eventos del núcleo hacia los flujos de la interfaz.
  ///
  /// Se sondea en lugar de usar un `StreamSink` a propósito: el lado Rust
  /// espera hasta 200 ms dentro de cada llamada, así que no hay espera activa
  /// —el hilo se bloquea en el canal, no gira—, y a cambio el puente se queda
  /// sin gestión de ciclo de vida de flujos a través del FFI, que es donde
  /// suelen aparecer las fugas.
  void _arrancarSondeo() {
    _sondeo?.cancel();
    _sondeo = Timer.periodic(const Duration(milliseconds: 120), (_) async {
      final e = await rust.siguienteEvento(esperaMs: 200);
      if (e == null) return;

      switch (e.tipo) {
        case 'niveles':
          _estado.add(EstadoGrabacion(
            grabando: true,
            transcurrido: Duration(milliseconds: e.transcurridoMs.toInt()),
            hablandoMicro: e.nivelMic > 0.01,
            hablandoSistema: e.nivelSistema > 0.01,
          ));
        case 'frase':
          _frases.add(Frase(
            inicioMs: e.inicioMs.toInt(),
            finMs: e.finMs.toInt(),
            texto: e.texto ?? '',
            pista: e.track == 'mic' ? Pista.micro : Pista.sistema,
            definitiva: e.definitiva,
          ));
        case 'error':
              debugPrint('núcleo: ${e.mensaje}');
      }
    });
  }

  void dispose() {
    _sondeo?.cancel();
    _frases.close();
    _estado.close();
    _progreso.close();
  }

  // -- Traducción -----------------------------------------------------------

  static Sesion _aSesion(rust.SesionDto s) => Sesion(
        id: s.id,
        topicId: s.topicId,
        tipo: TipoSesion.desdeClave(s.kind),
        estado: EstadoSesion.desdeClave(s.status),
        inicio: DateTime.fromMillisecondsSinceEpoch(s.startedAt.toInt()),
        fin: s.endedAt == null
            ? null
            : DateTime.fromMillisecondsSinceEpoch(s.endedAt!.toInt()),
        titulo: s.title,
        appCapturada: s.appCaptured,
        numDiapositivas: s.numDiapositivas.toInt(),
        costeUsd: s.costeUsd,
      );

  static Frase _aFrase(rust.FraseDto f) => Frase(
        inicioMs: f.inicioMs.toInt(),
        finMs: f.finMs.toInt(),
        texto: f.texto,
        pista: f.track == 'mic' ? Pista.micro : Pista.sistema,
        hablante: f.speaker,
        definitiva: f.definitiva,
      );
}
