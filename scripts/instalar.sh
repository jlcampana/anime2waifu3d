#!/usr/bin/env bash
# Instala todo lo que necesita anime2waifu3d en Fedora y lo compila.
#
#   scripts/instalar.sh                  todo menos Blender
#   scripts/instalar.sh --con-blender    también Blender (exportar a FBX y reducir mallas de IA)
#   scripts/instalar.sh --sin-modelos    sin descargar los modelos (unos 1,1 GB)
#   scripts/instalar.sh --simular        enseña lo que haría, sin hacerlo
#
# Qué instala:
#   - con dnf: Rust, compilador, Vulkan, las bibliotecas de ventana (wgpu, winit) y ONNX Runtime;
#   - los modelos, en assets/modelos/: BiRefNet ToonOut (quitar el fondo, MIT, 974 MB) y Depth
#     Anything V2 small (profundidad, Apache-2.0, 99 MB), de Hugging Face;
#   - el cuerpo base Base_Female de VRoid (CC0, OpenGameArt), en assets/candidatas/vroid-cc0/.
# Todo se ejecuta después en local: la app no usa servicios externos.
set -euo pipefail
cd "$(dirname "$0")/.."

CON_BLENDER=0
SIN_MODELOS=0
SIMULAR=0
for arg in "$@"; do
  case "$arg" in
    --con-blender) CON_BLENDER=1 ;;
    --sin-modelos) SIN_MODELOS=1 ;;
    --simular) SIMULAR=1 ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "Opción desconocida: $arg" >&2; exit 2 ;;
  esac
done

run() {
  echo "+ $*"
  if [ "$SIMULAR" = 0 ]; then "$@"; fi
}

descargar() { # URL DESTINO
  if [ -s "$2" ]; then
    echo "= ya está: $2"
    return
  fi
  run mkdir -p "$(dirname "$2")"
  run curl -fL --retry 3 -o "$2.part" "$1"
  run mv "$2.part" "$2"
}

. /etc/os-release
if [[ "${ID_LIKE:-} $ID" != *fedora* ]]; then
  echo "Este script es para Fedora (este sistema es $PRETTY_NAME)." >&2
  exit 1
fi
echo "== Sistema: $PRETTY_NAME"

# 1. Paquetes del sistema.
PAQUETES=(rust cargo gcc pkgconf-pkg-config curl unzip vulkan-loader mesa-vulkan-drivers libxkbcommon libX11 libXcursor libXrandr libXi wayland-devel onnxruntime)
[ "$CON_BLENDER" = 1 ] && PAQUETES+=(blender)
run sudo dnf install -y "${PAQUETES[@]}"

# 2. Modelos y cuerpo base.
if [ "$SIN_MODELOS" = 0 ]; then
  descargar "https://huggingface.co/sprited/birefnet-toonout-onnx/resolve/main/birefnet-toonout.onnx" assets/modelos/birefnet-toonout.onnx
  descargar "https://huggingface.co/onnx-community/depth-anything-v2-small/resolve/main/onnx/model.onnx" assets/modelos/depth-anything-v2-small.onnx
fi
BASE=assets/candidatas/vroid-cc0/base_female
if [ ! -s "$BASE/Base_Female.vrm" ]; then
  descargar "https://opengameart.org/sites/default/files/base_female.zip" "$BASE/base_female.zip"
  run unzip -q -o "$BASE/base_female.zip" -d "$BASE"
else
  echo "= ya está: $BASE/Base_Female.vrm"
fi

# 3. Compilar.
run cargo build --release
echo
echo "Listo. Abre la app y usa «Personaje desde imagen…»:"
echo "  target/release/anime2waifu3d"
