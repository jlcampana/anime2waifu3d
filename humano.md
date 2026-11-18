# Tareas para el humano

Lo que tienes que hacer tú: decisiones, pruebas en tu PC, material que solo tú puedes conseguir y permisos. Marca con `[x]` lo que vayas cerrando. Lo técnico lo voy haciendo yo; el registro está en `docs/investigacion.md`.

Última actualización: 2026-11-18.

## 1. Probar lo que ya está hecho

- [ ] Abre la app con un modelo terminado:
  ```bash
  cargo run --release -- assets/resultados/waifu016_80k.glb
  ```
  Los cuatro modelos de prueba, ya riggeados, con pelo, falda y párpados, están en `assets/resultados/`:
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
- [ ] Mira las imágenes de resultados en `assets/resultados/`: `all_poses.png`, `v2.png`, `hair.gif` y `flequillo.gif`.
- [ ] Dime lo que veas mal o raro, con el modelo, la pose o la animación, y la zona. Si puedes, añade una captura.

## 2. Decisiones que necesito

- [ ] **Siguiente bloque grande.** Elige el orden:
  - **Falda y abrigo con física de tela:** en la patada el faldón sigue a la pierna. Es mi recomendación y lo empiezo ya.
  - [x] **Cara animable:** elegiste geometría de párpados. Ya están el parpadeo, el guiño y los ojos felices (^^). Queda la boca.
  - **Formato de proyecto:** guardar y abrir con capas, visibilidad, opacidad y física por capa.
  - **Ruta desde imagen:** imagen de anime → modelo, con quitar el fondo y ajustar Base_Female a la silueta.
- [ ] **VRM Add-on de Blender.** Para comprobar que la física del pelo exportada (`VRMC_springBone`) funciona fuera de la app hace falta instalar el add-on oficial "VRM Add-on for Blender", que es gratuito. Tienes dos opciones:
  - darme permiso para descargarlo e instalarlo en la carpeta aislada de Blender, sin tocar tu configuración;
  - instalarlo tú en tu Blender e importar un `.glb` de `assets/resultados/`.
- [ ] **Unir la rama a `main`.** Todo está en la rama `visor-y-segmentacion` (GitHub `jlcampana/anime2waifu3d`). ¿Abro un PR y lo unes cuando lo hayas probado?
- [ ] **El modelo de la motosierra** necesita marcar la motosierra a mano (`--prop`), porque la segmentación confunde el metal con pelo. ¿Lo dejamos así hasta tener la paleta completa, donde se marcará con un clic, o es prioritario?

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
- [ ] Base_Female y HairSample_Female de VRoid son CC0 (según sus metadatos VRM). Conviene que lo confirmes en la página oficial de VRoid antes de distribuir algo que los incluya.

## 5. Al final del proyecto

- [ ] Revisar la documentación de la nueva interfaz que escribiré en `/home/pandi/Projects/doa.player`, para adaptarla allí. Es el último paso.
- [ ] Decidir si quieres el modo low poly (estilo del vídeo de Grok de Nagatoro) como opción del producto final.
