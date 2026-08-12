import 'package:flutter/material.dart';

import '../datos/repositorio.dart';
import '../datos/repositorio_rust.dart';

/// Configuración de los proveedores de IA.
///
/// Existe para que las claves no haya que ponerlas editando un archivo a mano.
/// Con el botón de probar se hace una petición real: una clave con la forma
/// correcta pero revocada, o un modelo que ya no existe, solo se detectan
/// preguntando — y descubrirlo aquí, no a mitad de una clase, es la diferencia.
class PantallaAjustes extends StatefulWidget {
  const PantallaAjustes({super.key, required this.repo});

  final Repositorio repo;

  @override
  State<PantallaAjustes> createState() => _PantallaAjustesState();
}

class _PantallaAjustesState extends State<PantallaAjustes> {
  late Future<List<ProveedorInfo>> _proveedores;

  @override
  void initState() {
    super.initState();
    _proveedores = _cargar();
  }

  Future<List<ProveedorInfo>> _cargar() async {
    final r = widget.repo;
    if (r is! RepositorioRust) return const [];
    return r.proveedoresInfo();
  }

  void _recargar() => setState(() => _proveedores = _cargar());

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Proveedores de IA')),
      body: FutureBuilder<List<ProveedorInfo>>(
        future: _proveedores,
        builder: (context, snap) {
          if (!snap.hasData) {
            return const Center(child: CircularProgressIndicator());
          }

          final l = snap.data!;
          if (l.isEmpty) {
            return const _SinNucleo();
          }

          return ListView(
            padding: const EdgeInsets.only(bottom: 32),
            children: [
              _CarpetaApuntes(repo: widget.repo),
              const Divider(height: 32),
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 4),
                child: Text(
                  'Proveedores de IA',
                  style: Theme.of(context)
                      .textTheme
                      .titleMedium
                      ?.copyWith(fontWeight: FontWeight.w700),
                ),
              ),
              const _Explicacion(),
              for (final p in l)
                _FilaProveedor(
                  info: p,
                  repo: widget.repo,
                  onCambio: _recargar,
                ),
            ],
          );
        },
      ),
    );
  }
}

class _Explicacion extends StatelessWidget {
  const _Explicacion();

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context);
    return Container(
      margin: const EdgeInsets.fromLTRB(16, 16, 16, 8),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: t.colorScheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(10),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'No hace falta poner todas',
            style: t.textTheme.titleSmall?.copyWith(fontWeight: FontWeight.w700),
          ),
          const SizedBox(height: 6),
          Text(
            'Con una basta. Los proveedores sin clave se omiten y se pasa al '
            'siguiente de la lista.\n\n'
            'Los apuntes de una sesión cuestan unas dos décimas de céntimo, así '
            'que un semestre entero sale por menos de un café. La '
            'transcripción es local y gratuita.',
            style: t.textTheme.bodySmall
                ?.copyWith(color: t.colorScheme.outline, height: 1.5),
          ),
        ],
      ),
    );
  }
}

class _FilaProveedor extends StatefulWidget {
  const _FilaProveedor({
    required this.info,
    required this.repo,
    required this.onCambio,
  });

  final ProveedorInfo info;
  final Repositorio repo;
  final VoidCallback onCambio;

  @override
  State<_FilaProveedor> createState() => _FilaProveedorState();
}

class _FilaProveedorState extends State<_FilaProveedor> {
  final _campo = TextEditingController();
  bool _editando = false;
  bool _probando = false;
  String? _resultado;
  bool _ok = false;

  @override
  void dispose() {
    _campo.dispose();
    super.dispose();
  }

  Future<void> _guardar() async {
    final r = widget.repo;
    if (r is! RepositorioRust) return;

    final ruta = await r.guardarClave(widget.info.id, _campo.text);
    if (!mounted) return;

    setState(() {
      _editando = false;
      _campo.clear();
    });
    widget.onCambio();

    ScaffoldMessenger.of(context).showSnackBar(
      // Se dice el archivo: si algún día algo no cuadra, saber dónde está la
      // clave ahorra la búsqueda.
      SnackBar(content: Text('Guardada en $ruta')),
    );
  }

  Future<void> _probar() async {
    final r = widget.repo;
    if (r is! RepositorioRust) return;

    setState(() {
      _probando = true;
      _resultado = null;
    });

    try {
      final respuesta = await r.probarProveedor(widget.info.id);
      if (mounted) {
        setState(() {
          _ok = true;
          _resultado = 'Responde: «$respuesta»';
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _ok = false;
          _resultado = '$e';
        });
      }
    } finally {
      if (mounted) setState(() => _probando = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context);
    final p = widget.info;
    final tieneClave = p.origenClave != null;

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
      child: Padding(
        padding: const EdgeInsets.all(14),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(
                  p.esLocal
                      ? Icons.computer
                      : tieneClave
                          ? Icons.check_circle
                          : Icons.key_off_outlined,
                  size: 20,
                  color: p.esLocal || tieneClave
                      ? t.colorScheme.primary
                      : t.colorScheme.outline,
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(p.id, style: t.textTheme.titleMedium),
                      Text(p.modelo, style: t.textTheme.bodySmall),
                    ],
                  ),
                ),
                if (p.esLocal || tieneClave)
                  TextButton.icon(
                    onPressed: _probando ? null : _probar,
                    icon: _probando
                        ? const SizedBox(
                            width: 14,
                            height: 14,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Icon(Icons.bolt, size: 16),
                    label: const Text('Probar'),
                  ),
              ],
            ),
            const SizedBox(height: 4),

            if (p.esLocal)
              Text(
                'En local: no necesita clave. Es el respaldo que funciona sin '
                'conexión.',
                style: t.textTheme.bodySmall
                    ?.copyWith(color: t.colorScheme.outline),
              )
            else if (tieneClave && !_editando)
              Row(
                children: [
                  Expanded(
                    child: Text(
                      'Clave configurada · ${p.origenClave}',
                      style: t.textTheme.bodySmall
                          ?.copyWith(color: t.colorScheme.outline),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  TextButton(
                    onPressed: () => setState(() => _editando = true),
                    child: const Text('Cambiar'),
                  ),
                  TextButton(
                    onPressed: () async {
                      final r = widget.repo;
                      if (r is RepositorioRust) {
                        await r.guardarClave(p.id, '');
                        widget.onCambio();
                      }
                    },
                    child: const Text('Volver a la de Documentos'),
                  ),
                ],
              )
            else
              Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  TextField(
                    controller: _campo,
                    // La clave se oculta al escribirla: alguien puede estar
                    // viendo la pantalla, que es justo lo que pasa en una
                    // clase compartida.
                    obscureText: true,
                    autocorrect: false,
                    enableSuggestions: false,
                    decoration: InputDecoration(
                      labelText: 'Clave de API',
                      hintText: _pista(p.id),
                      border: const OutlineInputBorder(),
                      isDense: true,
                    ),
                    onSubmitted: (_) => _guardar(),
                  ),
                  const SizedBox(height: 8),
                  Row(
                    children: [
                      FilledButton(
                        onPressed: _guardar,
                        child: const Text('Guardar'),
                      ),
                      const SizedBox(width: 8),
                      if (_editando)
                        TextButton(
                          onPressed: () => setState(() => _editando = false),
                          child: const Text('Cancelar'),
                        ),
                      const Spacer(),
                      Text(_donde(p.id), style: t.textTheme.labelSmall),
                    ],
                  ),
                ],
              ),

            if (_resultado != null) ...[
              const SizedBox(height: 10),
              Container(
                width: double.infinity,
                padding: const EdgeInsets.all(10),
                decoration: BoxDecoration(
                  color: _ok
                      ? t.colorScheme.primaryContainer
                      : t.colorScheme.errorContainer,
                  borderRadius: BorderRadius.circular(8),
                ),
                child: Text(_resultado!, style: t.textTheme.bodySmall),
              ),
            ],
          ],
        ),
      ),
    );
  }

  static String _pista(String id) => switch (id) {
        'gemini-flash' => 'AIza…',
        'deepseek' => 'sk-…',
        'openai' => 'sk-proj-…',
        _ => '',
      };

  /// Dónde se saca la clave. Sin esto, configurar el primer proveedor implica
  /// buscar en Google dónde está la página de claves de cada uno.
  static String _donde(String id) => switch (id) {
        'gemini-flash' => 'aistudio.google.com/apikey',
        'deepseek' => 'platform.deepseek.com/api_keys',
        'openai' => 'platform.openai.com/api-keys',
        _ => '',
      };
}

class _SinNucleo extends StatelessWidget {
  const _SinNucleo();

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Text(
          'El núcleo no está disponible, así que no se pueden configurar los '
          'proveedores. La aplicación está funcionando con datos de '
          'demostración.',
          textAlign: TextAlign.center,
          style: Theme.of(context).textTheme.bodyMedium,
        ),
      ),
    );
  }
}

/// Carpeta donde se guardan los apuntes exportados.
///
/// Se ofrecen las carpetas de sincronización detectadas en vez de un selector
/// de archivos: apuntando a OneDrive, Drive o Nextcloud, los apuntes acaban en
/// el móvil solos, sin que la aplicación tenga que hablar con la API de ninguna
/// nube —menos código, ningún token que caduque, y funciona con el servicio que
/// ya uses—.
class _CarpetaApuntes extends StatefulWidget {
  const _CarpetaApuntes({required this.repo});

  final Repositorio repo;

  @override
  State<_CarpetaApuntes> createState() => _CarpetaApuntesState();
}

class _CarpetaApuntesState extends State<_CarpetaApuntes> {
  final _campo = TextEditingController();
  List<String> _sugeridas = const [];
  String? _error;
  bool _guardado = false;
  String? _efectiva;
  bool _esPorDefecto = true;

  @override
  void initState() {
    super.initState();
    _cargar();
  }

  @override
  void dispose() {
    _campo.dispose();
    super.dispose();
  }

  Future<void> _cargar() async {
    final r = widget.repo;
    if (r is! RepositorioRust) return;

    final a = await r.ajustes();
    final s = await r.carpetasSugeridas();
    if (!mounted) return;

    setState(() {
      _campo.text = a.carpetaApuntes ?? '';
      _efectiva = a.carpetaEfectiva;
      _esPorDefecto = a.carpetaApuntes == null || a.carpetaApuntes!.trim().isEmpty;
      _sugeridas = s;
    });
  }

  Future<void> _guardar(String ruta) async {
    final r = widget.repo;
    if (r is! RepositorioRust) return;

    setState(() {
      _error = null;
      _guardado = false;
    });

    try {
      final a = await r.ajustes();
      await r.guardarAjustes(AjustesApp(
        carpetaApuntes: ruta,
        modelo: a.modelo,
        capturarDiapositivas: a.capturarDiapositivas,
      ));
      if (mounted) {
        setState(() => _guardado = true);
        await _cargar();
      }
    } catch (e) {
      // El núcleo comprueba que existe y que se puede escribir antes de
      // guardar: descubrir que la ruta estaba mal al acabar una clase, con los
      // apuntes sin exportar, sería el peor momento.
      if (mounted) setState(() => _error = '$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context);

    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 16, 16, 0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            'Carpeta de apuntes',
            style: t.textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700),
          ),
          const SizedBox(height: 6),
          Text(
            'Al terminar cada sesión se guarda ahí el Markdown, en una '
            'subcarpeta por asignatura. Si eliges una carpeta de OneDrive o '
            'Drive, los apuntes te llegan al móvil solos.',
            style: t.textTheme.bodySmall
                ?.copyWith(color: t.colorScheme.outline, height: 1.45),
          ),
          const SizedBox(height: 12),

          // Se enseña la carpeta que se está usando de verdad, no solo la
          // configurada: si nunca se tocó nada, el usuario tiene que saber
          // dónde están apareciendo sus apuntes.
          if (_efectiva != null)
            Container(
              width: double.infinity,
              padding: const EdgeInsets.all(12),
              margin: const EdgeInsets.only(bottom: 12),
              decoration: BoxDecoration(
                color: t.colorScheme.surfaceContainerHighest,
                borderRadius: BorderRadius.circular(8),
              ),
              child: Row(
                children: [
                  Icon(Icons.folder_open, size: 18, color: t.colorScheme.primary),
                  const SizedBox(width: 10),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          _esPorDefecto ? 'Guardando en (por defecto)' : 'Guardando en',
                          style: t.textTheme.labelSmall
                              ?.copyWith(color: t.colorScheme.outline),
                        ),
                        Text(_efectiva!, style: t.textTheme.bodyMedium),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          TextField(
            controller: _campo,
            decoration: InputDecoration(
              labelText: 'Ruta',
              hintText: 'Déjalo vacío para usar la de Documentos',
              border: const OutlineInputBorder(),
              isDense: true,
              errorText: _error,
              suffixIcon: _guardado
                  ? Icon(Icons.check, color: t.colorScheme.primary)
                  : null,
            ),
            onSubmitted: _guardar,
            onChanged: (_) => setState(() => _guardado = false),
          ),
          const SizedBox(height: 8),
          Row(
            children: [
              FilledButton(
                onPressed: () => _guardar(_campo.text),
                child: const Text('Guardar'),
              ),
              const SizedBox(width: 8),
              if (_campo.text.isNotEmpty)
                TextButton(
                  onPressed: () async {
                    final r = widget.repo;
                    if (r is! RepositorioRust) return;
                    final a = await r.ajustes();
                    await r.guardarAjustes(AjustesApp(
                      carpetaApuntes: null,
                      modelo: a.modelo,
                      capturarDiapositivas: a.capturarDiapositivas,
                    ));
                    if (mounted) {
                      _campo.clear();
                      await _cargar();
                    }
                  },
                  child: const Text('Volver a la de Documentos'),
                ),
            ],
          ),
          if (_sugeridas.isNotEmpty) ...[
            const SizedBox(height: 12),
            Text('Detectadas en tu equipo:', style: t.textTheme.labelMedium),
            const SizedBox(height: 6),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                for (final c in _sugeridas)
                  ActionChip(
                    avatar: const Icon(Icons.folder_outlined, size: 16),
                    label: Text(c.split('/').last),
                    tooltip: c,
                    onPressed: () => _guardar('$c/Apuntes'),
                  ),
              ],
            ),
          ],
        ],
      ),
    );
  }
}
