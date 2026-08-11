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
                    child: const Text('Quitar'),
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
