/// Espejo en Dart de los tipos de `core/domain`.
///
/// Cuando entre `flutter_rust_bridge`, este archivo lo generará la herramienta
/// a partir del núcleo Rust y dejará de mantenerse a mano. Hasta entonces
/// define el contrato para que la interfaz se pueda construir y probar sin
/// esperar al puente.
library;

enum TipoSesion {
  clase('class', 'Clase'),
  cliente('client_meeting', 'Reunión con cliente'),
  equipo('team_meeting', 'Reunión de trabajo'),
  nota('voice_note', 'Nota de voz');

  const TipoSesion(this.clave, this.etiqueta);
  final String clave;
  final String etiqueta;

  /// Grabar a un tercero sin avisar es un problema legal, no una preferencia:
  /// las reuniones con clientes exigen confirmación antes de empezar.
  bool get requiereConsentimiento => this == TipoSesion.cliente;

  static TipoSesion desdeClave(String c) =>
      TipoSesion.values.firstWhere((t) => t.clave == c, orElse: () => TipoSesion.equipo);
}

enum EstadoSesion {
  grabando('recording', 'Grabando'),
  transcribiendo('transcribing', 'Transcribiendo'),
  resumiendo('summarizing', 'Generando notas'),
  lista('ready', 'Lista'),
  fallida('failed', 'Falló');

  const EstadoSesion(this.clave, this.etiqueta);
  final String clave;
  final String etiqueta;

  bool get enProceso =>
      this == grabando || this == transcribiendo || this == resumiendo;

  static EstadoSesion desdeClave(String c) =>
      EstadoSesion.values.firstWhere((e) => e.clave == c, orElse: () => EstadoSesion.fallida);
}

enum Pista {
  micro('mic', 'Yo'),
  sistema('system', 'Interlocutor');

  const Pista(this.clave, this.etiquetaPorDefecto);
  final String clave;
  final String etiquetaPorDefecto;
}

enum TipoTopic { asignatura, cliente }

class Topic {
  const Topic({
    required this.id,
    required this.tipo,
    required this.nombre,
    this.persona,
    this.periodo,
    this.consentimiento = false,
  });

  final String id;
  final TipoTopic tipo;
  final String nombre;
  final String? persona;
  final String? periodo;
  final bool consentimiento;

  /// Lo que se muestra como contexto al modelo y en la cabecera de las notas.
  String get titulo => persona == null ? nombre : '$nombre — $persona';
}

class Sesion {
  const Sesion({
    required this.id,
    required this.tipo,
    required this.estado,
    required this.inicio,
    this.topicId,
    this.titulo,
    this.fin,
    this.appCapturada,
    this.numDiapositivas = 0,
    this.costeUsd = 0,
  });

  final String id;
  final String? topicId;
  final TipoSesion tipo;
  final EstadoSesion estado;
  final DateTime inicio;
  final DateTime? fin;
  final String? titulo;
  final String? appCapturada;
  final int numDiapositivas;
  final double costeUsd;

  Duration? get duracion => fin?.difference(inicio);

  String get tituloMostrado =>
      titulo?.trim().isNotEmpty == true ? titulo! : tipo.etiqueta;
}

class Frase {
  const Frase({
    required this.inicioMs,
    required this.finMs,
    required this.texto,
    required this.pista,
    this.hablante,
    this.definitiva = true,
  });

  final int inicioMs;
  final int finMs;
  final String texto;
  final Pista pista;
  final String? hablante;

  /// `false` mientras es la pasada en vivo del modelo pequeño. La interfaz la
  /// atenúa: es desechable y se sustituye entera al terminar.
  final bool definitiva;

  String get quien => hablante ?? pista.etiquetaPorDefecto;
}

enum TipoAviso {
  fechaExamen('exam_date', 'Examen'),
  entrega('deadline', 'Entrega'),
  pistaExamen('exam_hint', 'Entra en el examen'),
  cambioAula('room_change', 'Cambio de aula'),
  compromisoMio('commitment_mine', 'Me comprometí'),
  compromisoSuyo('commitment_theirs', 'Se comprometió');

  const TipoAviso(this.clave, this.etiqueta);
  final String clave;
  final String etiqueta;

  /// Una pista de examen no tiene fecha límite: informa, no urge.
  bool get esAccionable => this != pistaExamen && this != cambioAula;

  static TipoAviso desdeClave(String c) =>
      TipoAviso.values.firstWhere((t) => t.clave == c, orElse: () => TipoAviso.entrega);
}

class Aviso {
  const Aviso({
    required this.id,
    required this.sesionId,
    required this.tipo,
    required this.texto,
    required this.tsMs,
    this.topicId,
    this.fecha,
    this.descartado = false,
  });

  final int id;
  final String sesionId;
  final String? topicId;
  final TipoAviso tipo;
  final String texto;
  final String? fecha;
  final int tsMs;
  final bool descartado;

  /// Días que faltan, o `null` si no tiene fecha o no se puede interpretar.
  int? diasRestantes(DateTime ahora) {
    if (fecha == null) return null;
    final d = DateTime.tryParse(fecha!);
    if (d == null) return null;
    return DateTime(d.year, d.month, d.day)
        .difference(DateTime(ahora.year, ahora.month, ahora.day))
        .inDays;
  }

  bool esUrgente(DateTime ahora) {
    final d = diasRestantes(ahora);
    return d != null && d >= 0 && d <= 7;
  }

  bool estaVencido(DateTime ahora) {
    final d = diasRestantes(ahora);
    return d != null && d < 0;
  }
}

/// Notas ya generadas. Se guardan como JSON con la forma del esquema del
/// núcleo; la interfaz solo necesita el Markdown ya renderizado más los datos
/// que hacen falta para navegar.
class Notas {
  const Notas({
    required this.markdown,
    required this.tldr,
    required this.plantilla,
    required this.proveedor,
    required this.modelo,
    this.costeUsd = 0,
  });

  final String markdown;
  final String tldr;
  final String plantilla;
  final String proveedor;
  final String modelo;
  final double costeUsd;
}

/// Convierte milisegundos a `h:mm:ss`, o `mm:ss` si dura menos de una hora.
String formatearTs(int ms) {
  final s = (ms < 0 ? 0 : ms) ~/ 1000;
  final h = s ~/ 3600;
  final m = (s % 3600) ~/ 60;
  final seg = s % 60;
  final dosDigitos = seg.toString().padLeft(2, '0');
  if (h == 0) return '$m:$dosDigitos';
  return '$h:${m.toString().padLeft(2, '0')}:$dosDigitos';
}
