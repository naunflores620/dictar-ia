import 'package:flutter/material.dart';

import '../datos/repositorio.dart';
import '../modelos/dominio.dart';
import 'proceso.dart';
import 'sesion.dart';

/// Listado de sesiones agrupadas por asignatura o cliente.
class PantallaInicio extends StatefulWidget {
  const PantallaInicio({super.key, required this.repo});

  final Repositorio repo;

  @override
  State<PantallaInicio> createState() => _PantallaInicioState();
}

class _PantallaInicioState extends State<PantallaInicio> {
  late Future<(List<Topic>, List<Sesion>)> _datos;

  @override
  void initState() {
    super.initState();
    _datos = _cargar();
  }

  Future<(List<Topic>, List<Sesion>)> _cargar() async {
    final t = await widget.repo.topics();
    final s = await widget.repo.sesiones();
    return (t, s);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Sesiones'),
        actions: [
          IconButton(
            icon: const Icon(Icons.file_upload_outlined),
            tooltip: 'Importar audio de una reunión presencial',
            onPressed: () => _avisar(context, 'Importación de audio'),
          ),
          IconButton(
            icon: const Icon(Icons.search),
            tooltip: 'Buscar en todas las sesiones',
            onPressed: () => _avisar(context, 'Búsqueda'),
          ),
        ],
      ),
      body: FutureBuilder<(List<Topic>, List<Sesion>)>(
        future: _datos,
        builder: (context, snap) {
          if (!snap.hasData) {
            return const Center(child: CircularProgressIndicator());
          }

          final (topics, sesiones) = snap.data!;
          if (sesiones.isEmpty) return const _SinSesiones();

          final porTopic = <String?, List<Sesion>>{};
          for (final s in sesiones) {
            porTopic.putIfAbsent(s.topicId, () => []).add(s);
          }

          return ListView(
            padding: const EdgeInsets.only(bottom: 88),
            children: [
              for (final t in topics)
                if (porTopic[t.id]?.isNotEmpty ?? false)
                  _GrupoTopic(
                    topic: t,
                    sesiones: porTopic[t.id]!,
                    repo: widget.repo,
                    onCambio: () => setState(() => _datos = _cargar()),
                  ),
              if (porTopic[null]?.isNotEmpty ?? false)
                _GrupoTopic(
                  topic: null,
                  sesiones: porTopic[null]!,
                  repo: widget.repo,
                  onCambio: () => setState(() => _datos = _cargar()),
                ),
            ],
          );
        },
      ),
    );
  }

  static void _avisar(BuildContext context, String que) {
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text('$que: pendiente de conectar con el núcleo')),
    );
  }
}

class _GrupoTopic extends StatelessWidget {
  const _GrupoTopic({
    required this.topic,
    required this.sesiones,
    required this.repo,
    required this.onCambio,
  });

  final Topic? topic;
  final List<Sesion> sesiones;
  final Repositorio repo;
  final VoidCallback onCambio;

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context);
    final esCliente = topic?.tipo == TipoTopic.cliente;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 20, 16, 8),
          child: Row(
            children: [
              Icon(
                esCliente ? Icons.business_center_outlined : Icons.school_outlined,
                size: 18,
                color: t.colorScheme.primary,
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  topic?.titulo ?? 'Sin clasificar',
                  style: t.textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
                ),
              ),
              Text('${sesiones.length}', style: t.textTheme.labelMedium),
            ],
          ),
        ),
        for (final s in sesiones)
          _FilaSesion(sesion: s, repo: repo, onCambio: onCambio),
      ],
    );
  }
}

class _FilaSesion extends StatelessWidget {
  const _FilaSesion({
    required this.sesion,
    required this.repo,
    required this.onCambio,
  });

  final Sesion sesion;
  final Repositorio repo;
  final VoidCallback onCambio;

  @override
  Widget build(BuildContext context) {
    // `transcribiendo` significa aquí «grabada y a la espera»: el procesado se
    // lanza a mano, no arranca solo. Se ofrece como acción en vez de dejar la
    // fila muerta con un indicador giratorio que no gira.
    final pendiente = sesion.estado == EstadoSesion.transcribiendo ||
        sesion.estado == EstadoSesion.resumiendo;

    return ListTile(
      leading: _IconoApp(app: sesion.appCapturada, tipo: sesion.tipo),
      title: Text(sesion.tituloMostrado),
      subtitle: Row(
        children: [
          Text(_fecha(sesion.inicio)),
          if (sesion.duracion != null) ...[
            const Text(' · '),
            Text(_duracion(sesion.duracion!)),
          ],
          if (sesion.numDiapositivas > 0) ...[
            const Text(' · '),
            const Icon(Icons.slideshow_outlined, size: 13),
            Text(' ${sesion.numDiapositivas}'),
          ],
        ],
      ),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (pendiente)
            FilledButton.tonalIcon(
              onPressed: () => _procesar(context),
              icon: const Icon(Icons.auto_awesome, size: 16),
              label: const Text('Generar apuntes'),
            ),
          // El listado se llena de pruebas y falsos comienzos; sin poder
          // borrarlos, encontrar la clase de verdad entre veinte «0 min» es
          // buscar una aguja.
          PopupMenuButton<String>(
            icon: const Icon(Icons.more_vert),
            onSelected: (v) {
              if (v == 'borrar') _confirmarBorrado(context);
            },
            itemBuilder: (_) => const [
              PopupMenuItem(
                value: 'borrar',
                child: ListTile(
                  leading: Icon(Icons.delete_outline),
                  title: Text('Eliminar'),
                  contentPadding: EdgeInsets.zero,
                ),
              ),
            ],
          ),
        ],
      ),
      onTap: pendiente
          ? () => _procesar(context)
          : () => Navigator.of(context).push(
                MaterialPageRoute(
                  builder: (_) => PantallaSesion(repo: repo, sesionId: sesion.id),
                ),
              ),
    );
  }

  Future<void> _confirmarBorrado(BuildContext context) async {
    final seguro = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('¿Eliminar esta sesión?'),
        content: const Text(
          'Se borra la grabación, la transcripción y los apuntes de la '
          'aplicación. Los apuntes ya exportados a tu carpeta de Documentos '
          'no se tocan.\n\nEsto no se puede deshacer.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: const Text('Cancelar'),
          ),
          FilledButton(
            style: FilledButton.styleFrom(
              backgroundColor: Theme.of(ctx).colorScheme.error,
              foregroundColor: Theme.of(ctx).colorScheme.onError,
            ),
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Eliminar'),
          ),
        ],
      ),
    );

    if (seguro == true) {
      await repo.borrarSesion(sesion.id);
      onCambio();
    }
  }

  void _procesar(BuildContext context) {
    Navigator.of(context).push(
      MaterialPageRoute(
        builder: (_) => PantallaProceso(repo: repo, sesionId: sesion.id),
      ),
    );
  }

  static String _fecha(DateTime d) {
    const meses = [
      'ene', 'feb', 'mar', 'abr', 'may', 'jun',
      'jul', 'ago', 'sep', 'oct', 'nov', 'dic',
    ];
    return '${d.day} ${meses[d.month - 1]} · '
        '${d.hour.toString().padLeft(2, '0')}:${d.minute.toString().padLeft(2, '0')}';
  }

  static String _duracion(Duration d) =>
      d.inHours > 0 ? '${d.inHours}h ${d.inMinutes % 60}min' : '${d.inMinutes} min';
}

class _IconoApp extends StatelessWidget {
  const _IconoApp({required this.app, required this.tipo});

  final String? app;
  final TipoSesion tipo;

  @override
  Widget build(BuildContext context) {
    final c = Theme.of(context).colorScheme;
    final icono = switch (app) {
      'meet' || 'zoom' || 'teams' => Icons.videocam_outlined,
      _ => tipo == TipoSesion.cliente ? Icons.groups_outlined : Icons.mic_none,
    };

    return CircleAvatar(
      backgroundColor: c.primaryContainer,
      foregroundColor: c.onPrimaryContainer,
      child: Icon(icono, size: 20),
    );
  }
}

class _SinSesiones extends StatelessWidget {
  const _SinSesiones();

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context);
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(Icons.mic_none, size: 56, color: t.colorScheme.outline),
            const SizedBox(height: 16),
            Text('Todavía no hay grabaciones', style: t.textTheme.titleMedium),
            const SizedBox(height: 8),
            Text(
              'Pulsa el botón rojo antes de que empiece tu clase o reunión. '
              'Se graban dos pistas por separado: tu micrófono y el audio del '
              'sistema.',
              textAlign: TextAlign.center,
              style: t.textTheme.bodyMedium?.copyWith(color: t.colorScheme.outline),
            ),
          ],
        ),
      ),
    );
  }
}
