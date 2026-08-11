#!/usr/bin/env bash
# Actualiza dictar_ia a la última versión del repositorio.
#
#   ./actualizar.sh
#
# Trae los cambios, recompila y reinstala. Es la vía sencilla mientras el
# proyecto se está puliendo: no hace falta montar un repositorio APT ni un
# actualizador dentro de la aplicación para dos personas.
#
# Lo que NUNCA toca: tus datos. Las grabaciones, la base y las claves viven en
# ~/.local/share/dictar_ia y ~/.config/dictar_ia, fuera del paquete, así que
# sobreviven a cualquier reinstalación.

set -euo pipefail

RAIZ="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$RAIZ"

# rustup y Flutter viven en $HOME y no siempre están en el PATH.
export PATH="$HOME/.cargo/bin:$HOME/flutter/bin:$PATH"

azul() { printf '\033[1;34m%s\033[0m\n' "$1"; }
verde() { printf '\033[1;32m%s\033[0m\n' "$1"; }

# --- 1. Traer los cambios ---------------------------------------------------
if [[ -d .git ]]; then
  azul "→ Trayendo cambios del repositorio"
  antes=$(git rev-parse --short HEAD)
  git pull --rebase --autostash
  despues=$(git rev-parse --short HEAD)

  if [[ "$antes" == "$despues" ]]; then
    echo "  ya estabas en la última versión ($antes)"
  else
    echo "  $antes → $despues"
    git log --oneline "$antes..$despues" | sed 's/^/    /'
  fi
fi

# --- 2. Comprobar que sigue todo en pie -------------------------------------
# Se ejecutan antes de instalar, no después: instalar una versión rota y
# descubrirlo en mitad de una clase sería el peor momento posible.
azul "→ Ejecutando las pruebas"
cargo test --workspace --quiet 2>&1 | tail -3

# --- 3. Compilar ------------------------------------------------------------
azul "→ Compilando el núcleo"
cargo build --release --workspace

azul "→ Compilando la interfaz"
(cd app && flutter build linux --release)

# --- 4. Empaquetar e instalar -----------------------------------------------
azul "→ Empaquetando"
VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
bash packaging/linux/build_deb.sh "$VERSION" | tail -1

DEB="dist/dictar-ia_${VERSION}_amd64.deb"
azul "→ Instalando (hace falta tu contraseña)"
sudo apt install -y --reinstall "./$DEB"

verde "✔ dictar_ia $VERSION instalado"
echo
echo "  Interfaz:  dictar-ia          (o desde el menú de aplicaciones)"
echo "  Terminal:  dictar sesion --tipo clase --contexto \"Cálculo II\""
echo
echo "  Tus datos siguen intactos en ~/.local/share/dictar_ia"
