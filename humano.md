# Tareas para el humano

Lo que tienes que hacer tú: decisiones, pruebas en tu PC, material que solo tú puedes conseguir y permisos. Marca con `[x]` lo que vayas cerrando. Lo técnico lo voy haciendo yo; el registro está en `docs/investigacion.md`.

Última actualización: 2026-11-18.

## 1. Probar lo que ya está hecho

- [ ] Abre la app con un modelo terminado:
  ```bash
  cargo run --release -- assets/resultados/waifu016_80k.glb
  ```
  Los cuatro modelos de prueba, ya riggeados, con pelo, falda, párpados y boca, están en `assets/resultados/`:
  - `waifu026_60k`: la de pelo turquesa.
  - `waifu016_80k`: la rubia de melena larga.
  - `waifu006_80k`: la morena del abrigo largo.
  - `waifu039_80k`: la de la motosierra.

  Esta carpeta no se sube a git.
- [ ] En el panel izquierdo prueba:
  - pose;
  - animación "botar";
  - "Rebote del pecho";
  - "Física del pelo y la ropa", sus juegos de valores y "Viento";
  - en "Cara", las expresiones y el parpadeo automático;
  - los planos de cámara.
- [ ] En la paleta de capas (derecha):
  - abre el grupo de pelo;
  - oculta mechones (ojo, y Alt+clic para ver solo uno);
  - cambia su opacidad;
  - con un mechón seleccionado, cambia "Física:".
- [ ] Mira las imágenes de resultados en `assets/resultados/`:
  - `all_poses.png`;
  - `v2.png`;
  - `hair.gif`;
  - `flequillo.gif`;
  - `falda_patada.png`;
  - `falda_viento.png`;
  - `expresiones.png`;
  - `hablar.gif`;
  - `bocas.png`.
- [ ] Prueba a guardar un proyecto (Ctrl+S), cerrarlo y volver a abrirlo. También a exportar con "Exportar…" y abrir el resultado en Blender.
- [ ] Dime lo que veas mal o raro, con el modelo, la pose o la animación, y la zona. Si puedes, añade una captura.

### Ruta desde imagen (rama `ruta-imagen`)

- [ ] Prueba a crear un personaje desde la interfaz:
  ```bash
  git checkout ruta-imagen
  cargo run --release
  ```
  - En el panel izquierdo, «Personaje desde imagen…»: eliges una o **varias ilustraciones del mismo personaje** (unos 25 s cada una) y el personaje aparece en el visor.
    - La mejor de frente y de cuerpo entero da la figura y el frente.
    - Las proporciones son la media de todas las de frente.
    - Una de espaldas pinta la espalda y el pelo de detrás. La ventana «Proceso» enseña el avance, el recorte y el análisis; si sale raro, ahí se ve por qué.
  - «Preparar malla de IA…» hace lo mismo con una malla de una pieza (SupaVoxel): unos 2 minutos.
  - Después lo guardas con «Guardar proyecto…» o lo exportas con «Exportar…».
  - Por línea de comandos sigue funcionando todo (`imagen`, `preparar`, `export`…).

  Mejor resultado con:
  - un personaje de cuerpo entero, de pie y de frente;
  - con los brazos algo separados del cuerpo;
  - con fondo sencillo y en color;
  - **no sexual**.

  El ejemplo es `assets/resultados/imagen_a_3d.png` (Nagatoro).
- [ ] **Imágenes de prueba:** necesito varias ilustraciones de cuerpo entero así, para ajustar el proceso, y si puede ser **un mismo personaje de frente y de espaldas** (hojas de personaje o *turnarounds*), para probar la espalda. Ahora solo tengo una (Nagatoro 1).
  - Las de `assets/prueba` (Marin) no las he usado: son sexuales y el personaje es menor en la obra.
  - Valen personajes adultos o imágenes no sexuales, por ejemplo hojas de personaje (*character sheets*) o imágenes oficiales de cuerpo entero.

## 2. Decisiones que necesito

- [x] **Siguiente bloque grande.** Lo que has decidido:
  - [x] **Falda y abrigo con física de tela:** hecho.
  - [x] **Cara animable:** elegiste geometría de párpados. Hecho: parpadeo, guiño, ojos felices (^^) y boca con vocales.
  - [x] **Formato de proyecto:** hecho. "Guardar proyecto…" (Ctrl+S) y "Exportar…" a GLB, VRM, GLB solo modelo, OBJ y FBX.
  - [x] **Ruta desde imagen:** empezada el 2026-11-18, en la rama `ruta-imagen`. Paso 1 (quitar el fondo) hecho, con ONNX Runtime de Fedora y BiRefNet fp32, que elegiste tú.
- [x] **VRM Add-on de Blender.** Me diste permiso. Está instalado en la carpeta aislada (versión 4.10.5) y el VRM exportado se ha validado con él. Si quieres verlo tú, instálalo en tu Blender desde Edit > Preferences > Get Extensions (busca "VRM").
- [x] **Unir la rama a `main`:** PR #1 unido el 2026-11-18.
- [ ] **El modelo de la motosierra:**
  - necesita marcar la motosierra a mano (`--prop`), porque la segmentación confunde el metal con pelo;
  - tiene la cabeza girada y se queda sin párpados ni boca;
  - una pieza rígida de su pierna derecha recibe física de falda.

  ¿Lo dejamos así hasta tener la paleta completa, donde se marcará con un clic, o es prioritario?
- [x] **Sonreír sin abrir la boca:** elegiste cambiar la textura. Hecho: sonrisa ‿, gatito ω y puchero ︵. Pruébalas en "Cara" y dime si quieres otras bocas.

## 3. Material que necesito

- [ ] **Un modelo con coletas o twintails**, al estilo SupaVoxel de Sketchfab si puede ser. El pelo ya debería soportarlas, pero no lo he podido probar.
  - Descárgalo en GLB o glTF.
  - Déjalo en `assets/candidatas/sketchfab/<nombre>/`.
  - Avísame de la licencia: que permita descargarlo, y si es CC-BY, el autor.
- [ ] **Un modelo con falda corta o de vuelo**, para la física de tela. La rubia y la morena tienen sobrefalda y abrigo largo; viene bien variedad.
- [ ] Opcional: **referencias de expresiones kawaii** que te gusten (imágenes, gifs o vídeos) para la cara animable.
- [ ] Opcional: más imágenes 2D de personajes en `/home/pandi/Projects/assets.2d` para la ruta desde imagen.

## 4. Licencias (revísalo tú)

- [ ] Los modelos SupaVoxel de Sketchfab son CC-BY: si alguno se distribuye o se usa en material público, hay que dar crédito al autor. De momento solo están en `assets/`, que no se sube a git.
- [ ] Los modelos de DOA, los ripeados de DeviantArt, Mai Shiranui y los MMD de HoYoverse son **solo para pruebas locales**: nunca se suben ni se incluyen en la app. Confírmalo si tu idea es distinta.
- [x] Base_Female y HairSample_Female de VRoid son CC0 (según sus metadatos VRM). Conviene que lo confirmes en la página oficial de VRoid antes de distribuir algo que los incluya.

## 5. Al final del proyecto

- [ ] **Script de instalación (hecho, solo Fedora):** `scripts/instalar.sh` instala con dnf lo necesario y compila:
  - Rust;
  - Vulkan;
  - ONNX Runtime;
  - los modelos y Base_Female.

  Opciones: `--con-blender` (FBX y reducir mallas de IA densas), `--sin-modelos` y `--simular`. Lo he comprobado en modo simulación.
- [ ] Revisar la documentación de la nueva interfaz que escribiré en `/home/pandi/Projects/doa.player`, para adaptarla allí. Es el último paso.
- [ ] Decidir si quieres el modo low poly (estilo del vídeo de Grok de Nagatoro) como opción del producto final.
