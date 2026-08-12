import 'dart:async';

import 'package:flutter/material.dart';

import '../datos/repositorio.dart';
import '../datos/repositorio_rust.dart';
import '../modelos/dominio.dart';
import 'proceso.dart';

/// Pantalla de grabación en curso.
///
/// Muestra la transcripción según llega. Ese texto es desechable —lo produce el
/// modelo pequeño— y se sustituye entero al terminar por la pasada de
/// `large-v3-turbo`. Sirve para poder releer lo que se dijo hace diez minutos
/// sin parar la clase, no como resultado final.
class PantallaGrabacion extends StatefulWidget {
  const PantallaGrabacion({super.key, required this.repo});

  final Repositorio repo;

  @override
  State<PantallaGrabacion> createState() => _PantallaGrabacionState();
}

class _PantallaGrabacionState extends State<PantallaGrabacion> {
  final _frases = <Frase>[];
  final _scroll = ScrollController();
  StreamSubscription<Frase>? _subFrases;
  StreamSubscription<EstadoGrabacion>? _subEstado;

  EstadoGrabacion _estado = EstadoGrabacion.parado;
  TipoSesion _tipo = TipoSesion.clase;
  String? _topicId;
  bool _capturarPantalla = true;
  bool _iniciando = false;

  /// Sesión creada al empezar a grabar; hace falta para procesarla después.
  String? _sesionId;

  /// Se incrementa al crear una asignatura, para releer el desplegable.
  int _recargaTopics = 0;

  static const _valorCrear = '__crear__';

  @override
  void dispose() {
    _subFrases?.cancel();
    _subEstado?.cancel();
    _scroll.dispose();
    super.dispose();
  }

  Future<void> _iniciar() async {
    // Grabar a un tercero sin avisar es un problema legal, no una preferencia:
    // las reuniones con cliente piden confirmación explícita.
    if (_tipo.requiereConsentimiento && !await _confirmarConsentimiento()) {
      return;
    }

    setState(() => _iniciando = true);

    _subFrases = widget.repo.fraseEnVivo.listen((f) {
      if (!mounted) return;
      setState(() => _frases.add(f));
      _bajarAlFinal();
    });

    _subEstado = widget.repo.estadoGrabacion.listen((e) {
      if (mounted) setState(() => _estado = e);
    });

    _sesionId = await widget.repo.iniciarGrabacion(
      tipo: _tipo,
      topicId: _topicId,
      capturarPantalla: _capturarPantalla,
    );

    if (mounted) setState(() => _iniciando = false);
  }

  /// Crea una asignatura o cliente sin salir de la pantalla.
  ///
  /// Se pide también la persona porque va al contexto que recibe el modelo, y
  /// unos apuntes que dicen «la Dra. Pérez anunció» se leen mucho mejor que
  /// unos que dicen «el interlocutor anunció».
  Future<void> _crearTopic(bool esCliente) async {
    final nombre = TextEditingController();
    final persona = TextEditingController();

    final creado = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(esCliente ? 'Nuevo cliente' : 'Nueva asignatura'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: nombre,
              autofocus: true,
              decoration: InputDecoration(
                labelText: esCliente ? 'Empresa' : 'Asignatura',
                hintText: esCliente ? 'Acme S.A.' : 'Cálculo II',
                border: const OutlineInputBorder(),
              ),
            ),
            const SizedBox(height: 12),
            TextField(
              controller: persona,
              decoration: InputDecoration(
                labelText: esCliente ? 'Contacto (opcional)' : 'Profesor (opcional)',
                hintText: esCliente ? 'Juan Ruiz' : 'Dra. Pérez',
                border: const OutlineInputBorder(),
              ),
              onSubmitted: (_) => Navigator.pop(ctx, true),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancelar'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Crear'),
          ),
        ],
      ),
    );

    if (creado != true || nombre.text.trim().isEmpty) return;

    final r = widget.repo;
    if (r is! RepositorioRust) return;

    final id = await r.crearTopic(
      nombre: nombre.text.trim(),
      persona: persona.text.trim().isEmpty ? null : persona.text.trim(),
      esCliente: esCliente,
    );

    if (!mounted) return;
    setState(() {
      _topicId = id;
      _recargaTopics++;
    });
  }

  Future<bool> _confirmarConsentimiento() async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        icon: const Icon(Icons.record_voice_over_outlined),
        title: const Text('Avisa antes de grabar'),
        content: const Text(
          'Vas a grabar a otra persona. En España y en gran parte de '
          'Latinoamérica hace falta el consentimiento de todas las partes.\n\n'
          'Dilo en voz alta al empezar la reunión.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancelar'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Ya he avisado'),
          ),
        ],
      ),
    );
    return ok ?? false;
  }

  /// Detiene la grabación y encadena el procesado.
  ///
  /// Encadenarlo en vez de volver al listado es deliberado: si el usuario
  /// tuviera que acordarse de pulsar «procesar» después, acabaría con clases
  /// grabadas y sin apuntes, que es tener el trabajo hecho a medias.
  Future<void> _detener() async {
    await widget.repo.detenerGrabacion();
    await _subFrases?.cancel();
    await _subEstado?.cancel();
    if (!mounted) return;

    final id = _sesionId;
    if (id == null) {
      Navigator.of(context).pop();
      return;
    }

    // Se encadena el procesado en vez de volver al listado: si el usuario
    // tuviera que acordarse de pulsar «procesar» después, acabaría con clases
    // grabadas y sin apuntes, que es tener el trabajo hecho a medias.
    Navigator.of(context).pushReplacement(
      MaterialPageRoute(
        builder: (_) => PantallaProceso(repo: widget.repo, sesionId: id),
      ),
    );
  }

  void _bajarAlFinal() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scroll.hasClients) {
        _scroll.animateTo(
          _scroll.position.maxScrollExtent,
          duration: const Duration(milliseconds: 250),
          curve: Curves.easeOut,
        );
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: Text(_estado.grabando ? 'Grabando' : 'Nueva sesión'),
        leading: IconButton(
          icon: const Icon(Icons.close),
          onPressed: _estado.grabando ? _detener : () => Navigator.pop(context),
        ),
      ),
      body: _estado.grabando ? _enCurso() : _configuracion(),
    );
  }



  // -- Antes de empezar -------------------------------------------------------

  Widget _configuracion() {
    return ListView(
      padding: const EdgeInsets.all(20),
      children: [
        Text('¿Qué vas a grabar?', style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 12),
        SegmentedButton<TipoSesion>(
          segments: const [
            ButtonSegment(
              value: TipoSesion.clase,
              icon: Icon(Icons.school_outlined),
              label: Text('Clase'),
            ),
            ButtonSegment(
              value: TipoSesion.cliente,
              icon: Icon(Icons.business_center_outlined),
              label: Text('Cliente'),
            ),
            ButtonSegment(
              value: TipoSesion.equipo,
              icon: Icon(Icons.groups_outlined),
              label: Text('Equipo'),
            ),
          ],
          selected: {_tipo},
          onSelectionChanged: (s) => setState(() => _tipo = s.first),
        ),
        const SizedBox(height: 24),
        FutureBuilder<List<Topic>>(
          key: ValueKey(_recargaTopics),
          future: widget.repo.topics(),
          builder: (context, snap) {
            final topics = snap.data ?? const <Topic>[];
            final esperado = _tipo == TipoSesion.cliente
                ? TipoTopic.cliente
                : TipoTopic.asignatura;
            final opciones = topics.where((t) => t.tipo == esperado).toList();
            final esCliente = _tipo == TipoSesion.cliente;

            return DropdownButtonFormField<String>(
              initialValue:
                  opciones.any((o) => o.id == _topicId) ? _topicId : null,
              decoration: InputDecoration(
                labelText: esCliente ? 'Cliente' : 'Asignatura',
                helperText: 'Su glosario mejora la transcripción de esta sesión',
                border: const OutlineInputBorder(),
              ),
              items: [
                for (final o in opciones)
                  DropdownMenuItem(value: o.id, child: Text(o.titulo)),
                // La opción de crear va dentro del propio desplegable: es
                // donde el usuario está mirando cuando descubre que su
                // asignatura no está en la lista.
                DropdownMenuItem(
                  value: _valorCrear,
                  child: Row(
                    children: [
                      const Icon(Icons.add, size: 18),
                      const SizedBox(width: 8),
                      Text(esCliente ? 'Nuevo cliente…' : 'Nueva asignatura…'),
                    ],
                  ),
                ),
              ],
              onChanged: (v) {
                if (v == _valorCrear) {
                  _crearTopic(esCliente);
                } else {
                  setState(() => _topicId = v);
                }
              },
            );
          },
        ),
        const SizedBox(height: 12),
        SwitchListTile(
          value: _capturarPantalla,
          onChanged: (v) => setState(() => _capturarPantalla = v),
          title: const Text('Capturar diapositivas'),
          subtitle: const Text(
            'Guarda una imagen cada vez que cambia la pantalla compartida, '
            'anclada al segundo exacto',
          ),
          secondary: const Icon(Icons.slideshow_outlined),
          contentPadding: EdgeInsets.zero,
        ),
        const SizedBox(height: 24),
        FilledButton.icon(
          onPressed: _iniciando ? null : _iniciar,
          icon: const Icon(Icons.fiber_manual_record),
          label: const Text('Empezar a grabar'),
          style: FilledButton.styleFrom(
            minimumSize: const Size.fromHeight(52),
            backgroundColor: Theme.of(context).colorScheme.error,
            foregroundColor: Theme.of(context).colorScheme.onError,
          ),
        ),
        const SizedBox(height: 12),
        Text(
          'Se graban dos pistas por separado: tu micrófono y el audio del '
          'sistema. Así se sabe quién dijo cada cosa sin ningún modelo extra.',
          style: Theme.of(context).textTheme.bodySmall?.copyWith(
                color: Theme.of(context).colorScheme.outline,
              ),
        ),
      ],
    );
  }

  // -- Durante la grabación ---------------------------------------------------

  Widget _enCurso() {
    return Column(
      children: [
        _Cabecera(estado: _estado),
        const Divider(height: 1),
        Expanded(
          child: _frases.isEmpty
              ? const Center(child: Text('Escuchando…'))
              : ListView.builder(
                  controller: _scroll,
                  padding: const EdgeInsets.all(16),
                  itemCount: _frases.length,
                  itemBuilder: (context, i) => _LineaEnVivo(frase: _frases[i]),
                ),
        ),
        SafeArea(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: FilledButton.icon(
              onPressed: _detener,
              icon: const Icon(Icons.stop),
              label: const Text('Detener y generar notas'),
              style: FilledButton.styleFrom(minimumSize: const Size.fromHeight(52)),
            ),
          ),
        ),
      ],
    );
  }
}

class _Cabecera extends StatelessWidget {
  const _Cabecera({required this.estado});

  final EstadoGrabacion estado;

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 12, 20, 16),
      child: Row(
        children: [
          Icon(Icons.fiber_manual_record, color: t.colorScheme.error, size: 14),
          const SizedBox(width: 10),
          Text(
            _reloj(estado.transcurrido),
            style: t.textTheme.headlineSmall?.copyWith(
              fontFeatures: const [FontFeature.tabularFigures()],
            ),
          ),
          const Spacer(),
          // Los indicadores de voz importan más de lo que parece: sin ellos el
          // usuario no descubre que no se estaba captando al profesor hasta
          // que la clase ha terminado.
          _Nivel(etiqueta: 'Tú', activo: estado.hablandoMicro),
          const SizedBox(width: 12),
          _Nivel(etiqueta: 'Sistema', activo: estado.hablandoSistema),
          if (estado.diapositivas > 0) ...[
            const SizedBox(width: 16),
            const Icon(Icons.slideshow_outlined, size: 16),
            const SizedBox(width: 4),
            Text('${estado.diapositivas}', style: t.textTheme.labelLarge),
          ],
        ],
      ),
    );
  }

  static String _reloj(Duration d) =>
      '${d.inHours.toString().padLeft(2, '0')}:'
      '${(d.inMinutes % 60).toString().padLeft(2, '0')}:'
      '${(d.inSeconds % 60).toString().padLeft(2, '0')}';
}

class _Nivel extends StatelessWidget {
  const _Nivel({required this.etiqueta, required this.activo});

  final String etiqueta;
  final bool activo;

  @override
  Widget build(BuildContext context) {
    final c = Theme.of(context).colorScheme;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        AnimatedContainer(
          duration: const Duration(milliseconds: 150),
          width: 8,
          height: 8,
          decoration: BoxDecoration(
            shape: BoxShape.circle,
            color: activo ? c.primary : c.outlineVariant,
          ),
        ),
        const SizedBox(width: 5),
        Text(etiqueta, style: Theme.of(context).textTheme.labelSmall),
      ],
    );
  }
}

class _LineaEnVivo extends StatelessWidget {
  const _LineaEnVivo({required this.frase});

  final Frase frase;

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context);
    final esMio = frase.pista == Pista.micro;

    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 58,
            child: Text(
              formatearTs(frase.inicioMs),
              style: t.textTheme.labelSmall?.copyWith(color: t.colorScheme.outline),
            ),
          ),
          Expanded(
            child: RichText(
              text: TextSpan(
                style: t.textTheme.bodyMedium?.copyWith(height: 1.4),
                children: [
                  TextSpan(
                    text: '${frase.quien}: ',
                    style: TextStyle(
                      fontWeight: FontWeight.w700,
                      color: esMio ? t.colorScheme.tertiary : t.colorScheme.primary,
                    ),
                  ),
                  TextSpan(
                    text: frase.texto,
                    // Todo lo de la pasada en vivo es provisional hasta que
                    // termine la sesión.
                    style: TextStyle(
                      color: frase.definitiva ? null : t.colorScheme.onSurfaceVariant,
                    ),
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}
