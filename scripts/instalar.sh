#!/usr/bin/env bash
# Instala todo lo que necesita anime2waifu3d en Fedora o en Ubuntu/Debian y lo compila.
#
#   scripts/instalar.sh                  todo menos Blender
#   scripts/instalar.sh --con-blender    también Blender (para exportar a FBX)
#   scripts/instalar.sh --sin-modelos    sin descargar los modelos (unos 1,1 GB)
#   scripts/instalar.sh --simular        enseña lo que haría, sin hacerlo
#
# Qué instala:
#   - Rust (el de la distribución en Fedora; rustup en Ubuntu, que suele traer uno antiguo);
#   - compilador, Vulkan y las bibliotecas de ventana (wgpu, winit);
#   - ONNX Runtime 1.26 (paquete `onnxruntime` en Fedora; en Ubuntu, la biblioteca del paquete
#     oficial de PyPI, en ~/.local/lib/anime2waifu3d);
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
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
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
FAMILIA="${ID_LIKE:-} $ID"
echo "== Sistema: $PRETTY_NAME"

# 1. Paquetes del sistema y Rust.
if [[ "$FAMILIA" == *fedora* ]]; then
  PAQUETES=(rust cargo gcc pkgconf-pkg-config curl unzip vulkan-loader mesa-vulkan-drivers libxkbcommon libX11 libXcursor libXrandr libXi wayland-devel onnxruntime)
  [ "$CON_BLENDER" = 1 ] && PAQUETES+=(blender)
  run sudo dnf install -y "${PAQUETES[@]}"
elif [[ "$FAMILIA" == *debian* || "$FAMILIA" == *ubuntu* ]]; then
  PAQUETES=(build-essential pkg-config curl unzip python3 libvulkan1 mesa-vulkan-drivers libxkbcommon0 libx11-6 libxcursor1 libxrandr2 libxi6 libwayland-client0)
  [ "$CON_BLENDER" = 1 ] && PAQUETES+=(blender)
  run sudo apt-get update
  run sudo apt-get install -y "${PAQUETES[@]}"
  # Rust reciente (el proyecto necesita 1.92 o más).
  if ! command -v cargo >/dev/null 2>&1; then
    run sh -c "curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal"
    export PATH="$HOME/.cargo/bin:$PATH"
  fi
  # ONNX Runtime 1.26: la biblioteca del paquete oficial de PyPI.
  ORT_DIR="$HOME/.local/lib/anime2waifu3d"
  if [ ! -e "$ORT_DIR/libonnxruntime.so.1" ]; then
    TMP="$(mktemp -d)"
    # La URL real del wheel, preguntada a PyPI.
    WHEEL="$(curl -fsSL https://pypi.org/pypi/onnxruntime/1.26.0/json | python3 -c "import json,sys; print(next(f['url'] for f in json.load(sys.stdin)['urls'] if 'manylinux' in f['filename'] and 'x86_64' in f['filename']))")"
    run curl -fL -o "$TMP/ort.whl" "$WHEEL"
    run unzip -q -o "$TMP/ort.whl" 'onnxruntime/capi/libonnxruntime.so*' -d "$TMP"
    run mkdir -p "$ORT_DIR"
    run sh -c "cp $TMP/onnxruntime/capi/libonnxruntime.so* '$ORT_DIR'/ && cd '$ORT_DIR' && ln -sf \$(ls libonnxruntime.so.1.* | head -1) libonnxruntime.so.1"
    run rm -rf "$TMP"
  else
    echo "= ya está: $ORT_DIR/libonnxruntime.so.1"
  fi
else
  echo "Distribución no reconocida ($ID). Instala a mano: Rust 1.92+, Vulkan, onnxruntime 1.26 y curl." >&2
  exit 1
fi

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
echo "Listo. Prueba:"
echo "  target/release/anime2waifu3d imagen TU_IMAGEN.png personaje.glb"
echo "  target/release/anime2waifu3d personaje.glb"
