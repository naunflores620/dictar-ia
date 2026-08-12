#!/usr/bin/env bash
# Construye el paquete .deb a partir del bundle de Flutter.
#
#   ./packaging/linux/build_deb.sh [version]
#
# El fallo número uno de un .deb es una dependencia no declarada: funciona en la
# máquina de desarrollo, que tiene medio sistema instalado por otras vías, y
# falla en la del usuario. Por eso este script genera la lista de dependencias
# a partir del binario real en vez de confiar en una escrita a mano, y por eso
# existe `verify_deb.sh`, que lo prueba en un contenedor limpio.

set -euo pipefail

VERSION="${1:-0.1.0}"
PAQUETE="dictar-ia"
ARCH="amd64"
RAIZ="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BUNDLE="$RAIZ/app/build/linux/x64/release/bundle"
STAGE="$RAIZ/dist/${PAQUETE}_${VERSION}_${ARCH}"

if [[ ! -d "$BUNDLE" ]]; then
  echo "error: no existe $BUNDLE" >&2
  echo "ejecuta antes:  cd app && flutter build linux --release" >&2
  exit 1
fi

echo "→ preparando árbol en $STAGE"
rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN" \
         "$STAGE/usr/bin" \
         "$STAGE/usr/lib/$PAQUETE" \
         "$STAGE/usr/share/applications" \
         "$STAGE/usr/share/icons/hicolor" \
         "$STAGE/usr/share/$PAQUETE"

cp -r "$BUNDLE/." "$STAGE/usr/lib/$PAQUETE/"

# Iconos en todos los tamaños del tema. Sin esto el menú de aplicaciones
# muestra el engranaje genérico, que es lo que pasaba antes.
if [[ -d "$RAIZ/packaging/iconos/hicolor" ]]; then
  cp -r "$RAIZ/packaging/iconos/hicolor/." "$STAGE/usr/share/icons/hicolor/"
  echo "  incluidos $(find "$RAIZ/packaging/iconos/hicolor" -name '*.png' | wc -l) iconos"
else
  echo "  aviso: no hay iconos; genéralos con python3 packaging/icono.py" >&2
fi
cp "$RAIZ/config/providers.toml" "$STAGE/usr/share/$PAQUETE/providers.toml"

# La CLI va también en el paquete: es lo que permite procesar por lotes por la
# noche, o recuperar una sesión sin abrir la interfaz.
if [[ -x "$RAIZ/target/release/dictar" ]]; then
  cp "$RAIZ/target/release/dictar" "$STAGE/usr/bin/dictar"
  chmod 755 "$STAGE/usr/bin/dictar"
  echo "  incluida la CLI \`dictar\`"
fi

# El binario real vive en /usr/lib porque necesita su carpeta lib/ y data/ al
# lado; en /usr/bin va solo un lanzador.
cat > "$STAGE/usr/bin/$PAQUETE" <<'LANZADOR'
#!/bin/sh
exec /usr/lib/dictar-ia/dictar_ia "$@"
LANZADOR
chmod 755 "$STAGE/usr/bin/$PAQUETE"

# El archivo se llama como el APPLICATION_ID de la aplicación, no como el
# paquete. GNOME empareja la ventana con su lanzador por ese nombre: si no
# coinciden, la barra de tareas muestra el icono genérico aunque el .desktop
# tenga el correcto. Era justo lo que pasaba.
APP_ID="com.dictaria.dictar_ia"
cat > "$STAGE/usr/share/applications/$APP_ID.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=dictar_ia
Comment=Grabación de clases y reuniones con apuntes generados por IA
Exec=/usr/bin/$PAQUETE
Icon=$PAQUETE
Categories=Education;AudioVideo;Office;
Terminal=false
StartupWMClass=$APP_ID
DESKTOP

# -- Dependencias: se deducen del binario, no se escriben a mano --------------
echo "→ deduciendo dependencias con ldd"
# El `|| true` no es descuido: `dpkg -S` devuelve error si alguna de las
# librerías no pertenece a ningún paquete —las propias del bundle de Flutter,
# por ejemplo—, y con `pipefail` eso mataría el script justo aquí.
DEPS=$(
  {
    ldd "$BUNDLE/dictar_ia" 2>/dev/null \
      | awk '/=>/ {print $3}' \
      | grep -v '^$' \
      | xargs -r dpkg -S 2>/dev/null \
      | cut -d: -f1 \
      | sort -u \
      | tr '\n' ',' \
      | sed 's/,$//; s/,/, /g'
  } || true
)
# Respaldo por si dpkg -S no resuelve nada (contenedor sin la base de datos).
if [[ -z "$DEPS" ]]; then
  echo "  aviso: ldd no resolvió paquetes; usando la lista mínima conocida" >&2
  DEPS="libgtk-3-0, libglib2.0-0, libstdc++6, libpipewire-0.3-0, libsecret-1-0"
fi
echo "  $DEPS"

INSTALADO=$(du -sk "$STAGE/usr" | cut -f1)

# Refrescar el caché de iconos al instalar: sin esto, GNOME sigue enseñando el
# icono anterior hasta que se reinicia la sesión.
cat > "$STAGE/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -f -t /usr/share/icons/hicolor || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
exit 0
POSTINST
chmod 755 "$STAGE/DEBIAN/postinst"

cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: $PAQUETE
Version: $VERSION
Section: education
Priority: optional
Architecture: $ARCH
Depends: $DEPS
Installed-Size: $INSTALADO
Maintainer: dictar_ia <noreply@example.com>
Description: Grabación de clases y reuniones con apuntes generados por IA
 Captura clases online y reuniones desde Meet, Zoom o Teams en dos pistas
 separadas, transcribe con Whisper en local y genera apuntes o actas
 estructuradas con marcas de tiempo verificables.
 .
 Funciona sin servidor: los datos son un archivo SQLite en el equipo.
CONTROL

echo "→ construyendo el paquete"
# dpkg-deb deja el .deb junto al árbol, con el mismo nombre y extensión: no hay
# que moverlo a ningún sitio.
dpkg-deb --build --root-owner-group "$STAGE" >/dev/null
DEB="${STAGE}.deb"
rm -rf "$STAGE"

echo "✔ $DEB ($(du -h "$DEB" | cut -f1))"
echo
echo "Pruébalo en un contenedor limpio antes de distribuirlo:"
echo "  ./packaging/linux/verify_deb.sh $DEB"
