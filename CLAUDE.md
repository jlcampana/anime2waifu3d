# anime2waifu3d

App en Rust para Fedora que convierte ilustraciones anime (o mallas de IA de una pieza) en personajes 3D riggeados y animables. Los requisitos del usuario están en `LEEME.md`. Usa el stack de doa.player: winit 0.30, wgpu 22, egui 0.29 y glam.

## Reglas del usuario

- **Todo en local, sin servicios externos ni APIs.** Solo se descargan modelos o herramientas con su permiso. Equipo: AMD RX 6400/6500 XT con 4 GB de VRAM y 16 GB de RAM.
- **Solo Fedora.** No hace falta pensar en Ubuntu.
- Hablar al usuario en español. Los comentarios del código y la documentación también van en español.
- Todo debe poder hacerse desde la interfaz gráfica, además de por línea de comandos.
- **Ningún proceso largo ni diálogo de archivo en el hilo de la ventana.** Si se bloquea, GNOME muestra «no responde». Ver `Dialog` en `src/main.rs` y `start_job` en `src/app.rs`.
- **Lo que necesite del usuario se apunta en `humano.md`** (decisiones, pruebas, material, permisos). Mantenerlo al día.
- **Licencias:** los modelos de DOA, los ripeados, Mai Shiranui y los MMD de HoYoverse son solo para pruebas locales. `assets/` no se sube a git. Base_Female (VRoid) es CC0; SupaVoxel, CC-BY.
- **No usar ni procesar contenido sexual de personajes menores.** Las imágenes de Marin de `assets/prueba` se rechazaron por eso.
- Commits: el usuario pide comitear, subir, abrir PR y unir a `main`. Trabajar en rama y unir con PR.

## Dónde está cada cosa

- `src/main.rs`: interfaz (paneles, diálogos en otro hilo, paneles en su propia ventana `PanelWindow`) y subcomandos.
- `src/app.rs`: estado del visor, poses, física, expresiones, proyecto y trabajos en segundo plano.
- `src/pipeline.rs`: las dos rutas completas, `from_images` y `from_ai_mesh`.
- **Ruta desde imagen:**
  - `cutout.rs`: recorte con BiRefNet;
  - `depth.rs`: profundidad;
  - `analyze.rs`: partes y puntos clave;
  - `fit.rs`: ajuste de Base_Female y proyección, también de la espalda;
  - `dress.rs`: pelo con volumen y falda;
  - `onnx.rs`: ONNX Runtime del sistema.
- **Ruta de mallas de IA:**
  - `segment.rs`: capas por color;
  - `rig.rs`: rig con Base_Female;
  - `hair.rs`: cadenas y física de pelo y falda;
  - `face.rs`: párpados, boca y bocas por textura.
- `export_glb.rs` (GLB/VRM con springBone y expresiones) y `project.rs` (proyecto `.proyecto.glb` y exportaciones).
- `docs/investigacion.md`: el registro de todo lo hecho y decidido, por secciones con fecha. Leerlo antes de tocar algo.
- `humano.md`: las tareas del usuario.
- `assets/modelos/`: los modelos ONNX. `assets/candidatas/vroid-cc0/base_female/`: el cuerpo base. `assets/resultados/`: modelos e imágenes de ejemplo.
- `scripts/instalar.sh`: instalación en Fedora.

## Comandos

```bash
cargo build --release && cargo test --release
target/release/anime2waifu3d                      # la app
target/release/anime2waifu3d imagen A.png [B.png…] salida.glb
target/release/anime2waifu3d preparar malla_ia.glb salida.glb [--prop "capas=hueso"]
target/release/anime2waifu3d modelo.glb --shot captura.png --size 300x600 --pose saludo --expression feliz --camera 0 --yaw 180 --no-grid
target/release/anime2waifu3d modelo.glb --desde-imagen A.png --shot c.png   # el mismo proceso que el botón, sin ventana
```

Las capturas sin ventana (`--shot`, `--frames`) sirven para comprobar resultados a ojo; montarlas con `magick montage`. Tarda: unos 25 s por imagen, unos 2 minutos por malla de IA y unos 4 minutos de compilación.

## Estado (2026-11-19)

En `main`, con los PR #1 y #2 unidos. Funciona:
- **Visor:** capas, poses, muelles de pecho, pelo y falda con física, cara con expresiones, proyecto y exportación a GLB, VRM, OBJ y FBX. El VRM está validado con el VRM Add-on.
- **Ruta de mallas de IA**, desde la malla original.
- **Ruta desde imagen**, con una o varias imágenes.
- **Interfaz:** creación en segundo plano, diálogos sin bloqueo y paneles que salen a su propia ventana. El usuario comprobó los paneles y los diálogos.

## Siguientes pasos, por orden

1. **Afinar la ruta desde imagen con imágenes reales del usuario.** Está pendiente de que las pase (ver `humano.md`). Solo se ha probado con una imagen de cuerpo entero (Nagatoro), y la espalda nunca con una imagen de espaldas real.
   - El brazo en jarra no se reproduce: se prueban ángulos de brazo recto; faltaría doblar el codo.
   - Los mechones finos de los lados se clasifican mal como pelo.
   - En blanco y negro no se distingue la piel.
2. **Paleta de capas completa al estilo Photoshop:**
   - arrastrar para reordenar o reagrupar;
   - miniaturas y bloquear;
   - marcar una capa como objeto rígido con un hueso (hoy `--prop`; arreglaría la motosierra de waifu039).
   - Guardar todo en el proyecto.
3. **Línea de tiempo de animación:** animaciones por defecto, expresiones kawaii animadas, opacidad animable de capas, exportar animaciones e importar VMD.
4. **Ruta desde imagen, calidad:**
   - usar la profundidad (calculada pero sin usar) para el volumen del pelo y la ropa;
   - mechones a partir de los picos del contorno de la máscara de pelo;
   - texturas sin estiramientos a los lados (proyección desde varias direcciones o relleno).
5. **Pendientes del rig:** ajustar la longitud de brazos y piernas, tela de verdad (rejilla con enlaces laterales como `doax2_cloth.rs` de doa.player) y caras giradas sin ojos detectados.
6. **Modo low poly**: una función opcional del producto final.
7. **Último paso del proyecto:** documentar la interfaz nueva en `/home/pandi/Projects/doa.player`. Ya está como primer punto de su `CLAUDE.md`, pero sin comitear allí, porque el usuario tenía otros cambios sin comitear.
