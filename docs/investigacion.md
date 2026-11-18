# Investigación: referencias, herramientas y decisiones

Fecha: 2026-11-17. Licencias y estrellas, consultadas en GitHub ese día.

## 1. Los repos de referencia del LEEME

| Repo | Qué es | Qué nos sirve |
| --- | --- | --- |
| [shinshin86/mesh-avatar-studio](https://github.com/shinshin86/mesh-avatar-studio) (MIT, TypeScript, ~2.4k ★) | Una ilustración → avatar **2D** con malla que parpadea, habla, gira la cabeza, respira y mueve el pelo. Un agente (Claude Code/Codex) corta la imagen en capas siguiendo `docs/agent-guide.md`; luego se ajusta en un editor web. | El formato de proyecto (`docs/format.md`): `base.png` con las partes móviles quitadas y rellenadas (*inpaint*), `hairmask.png`, capas de ojo (`eye0_ball`, párpados con curvas de 24 puntos), *sprites* de boca y ojos cerrados, *overlays* (gafas…) y orden de dibujo: ropa trasera → base → ojos/boca → ropa delantera → accesorios → mano. Es una buena plantilla para guardar las capas 2D dentro de nuestro proyecto. |
| [shinshin86/aituber-onair](https://github.com/shinshin86/aituber-onair) (MIT, ~340 ★) | Kit para VTubers con IA: chat, voz (TTS), memoria y avatares PNG/VRM/Live2D/Inochi2D/PSD. | Ideas de *runtime*: lip‑sync a partir del volumen de audio, parpadeo y mirada en reposo, reacciones según la emoción, física de pelo en el avatar "PuruPuru". Confirma que **VRM** es el formato estándar para avatares 3D de anime. |
| [shinshin86/live2d-add-motion-sample-web-ui](https://github.com/shinshin86/live2d-add-motion-sample-web-ui) (MIT) | Añade motions a un modelo Live2D editando solo JSON (curvas de *keyframes* por parámetro). | **La lista de expresiones kawaii por defecto y cómo construirlas a partir de parámetros**: Happy (EyeSmile + forma de boca + rubor + rebote), Wink, Nod, Thinking, Surprised, Shy (rubor + mirar abajo + párpados bajos), Head shake. Las motions salen de un generador, se comprueban con un validador y luego en un navegador sin interfaz. Adoptaremos el mismo sistema para nuestras animaciones: definidas como datos y validadas. |
| [Anime2.5DRig](https://github.com/shinshin86/Anime2.5DRig) (MIT; el código original es de 852wa) | Sueltas un PSD dividido en partes y se autorriga en 2.5D, en el navegador. | Lo más útil para el **pelo por capas**: detecta automáticamente los **mechones** (picos en el contorno de las puntas, hasta 6 por capa), cada capa `front hair_N` / `back hair_N` es un grupo de física independiente, y tiene **doble muelle por mechón (raíz rígida, punta suave)**. También asigna profundidad por capa (paralaje), separa automáticamente ojo izquierdo y derecho, detecta los puntos de referencia (párpado, iris, boca, pivote del cuello) y genera ojos y boca cerrados cuando faltan. Su entrada es la salida de **See-through** (abajo). |

## 2. El vídeo de Grok (Nagatoro *low poly*)

6 s, 432×688, una vuelta completa del personaje con fondo blanco. Lo que se ve:
- Malla de pocos polígonos con **sombreado plano** (caras visibles), pero la cara, los ojos y la sonrisa están **pintados en la textura**, sin geometría. Por eso el estilo anime sigue leyéndose bien.
- El pelo es un casco con mechones grandes; la falda, un cono de pliegues.
- El otro vídeo (`2120486698396135454_1.mp4`) muestra pelo rojo largo con muchos mechones finos y brillo anime. Es la referencia del pelo detallado y por capas que queremos.

**Feature futura "low poly"**: tras obtener el modelo detallado, simplificar la malla con `meshopt` (crate Rust) conservando los bordes de la UV, sombrear plano y volver a hornear (*bake*) la textura. Es un filtro posterior, no otro método de reconstrucción.

## 3. Twitter/X

No puedo navegar por X directamente, pero sí leer tweets sueltos a través de `api.fxtwitter.com` (texto y enlace al vídeo). No puedo buscar dentro de X. El tweet del LEEME:

- **@Topolazy, 2026-11-14**: *"I just want spherical eyes on my anime-style characters for ray tracing at any cost"* (#b3d, Blender, 10k likes). Es una respuesta a un tweet de @EL_K_8187 que compara ojos planos con esféricos.
- Idea que nos sirve: el ojo como **esfera real** (blanco del ojo + iris/pupila en una esfera con la textura del ojo) **dentro de** un hueco de la cara. El ojo gira con un hueso (mirada) y los párpados son *blend shapes*. Así el ojo funciona desde cualquier ángulo y con cualquier luz, y no se "despega" como un ojo pintado en plano. Habrá que dibujar el reflejo del ojo (*highlight*) en el *shader*, no en la textura, para que siga a la luz.

## 4. Otros proyectos y productos relevantes

### Imagen → 3D (reconstrucción)
| Proyecto | Licencia | Notas |
| --- | --- | --- |
| [StdGEN](https://github.com/hyz317/StdGEN) (CVPR 2025) | Apache‑2.0 | **El más alineado**: personaje anime 3D desde 1 imagen, con **cuerpo, ropa y pelo separados** en mallas distintas, en ~3 min. Dataset Anime3D++. |
| [CharacterGen](https://github.com/zjp-shadow/CharacterGen) | Apache‑2.0 | Personaje anime en pose A a partir de 1 imagen; StdGEN es su sucesor. |
| [TRELLIS.2](https://github.com/microsoft/TRELLIS.2) (Microsoft) | MIT | Genérico, alta calidad y PBR; según su autor, maneja bien las superficies finas y abiertas (mechones). |
| Hunyuan3D 2.1 (Tencent) | Tencent Community License (restrictiva) | Muy bueno en texturas; la licencia complica su uso. |
| [AssetFurnace / image-to-3dlab](https://github.com/Bingeljell/image-to-3dlab) | Apache‑2.0 | Encadena TRELLIS.2/Hunyuan3D/SF3D + autorrigging + 21 animaciones. Sirve de referencia de pipeline. **No verificado.** |
| Tripo, Meshy, Neural4D AnimeArt, VTubeMe | Comerciales (API/SaaS) | Neural4D promete VRM con 55 huesos, 52 *blend shapes* y *spring bones*. |
| [Charakuru](https://github.com/nanocle/Charakuru) | *Source-available*, sin redistribución | Ilustración → 8 vistas (Codex) → Tripo → Blender → **VRM 1.0** con *spring bones*. Sin expresiones ni movimiento de ojos. Su pipeline se parece mucho al nuestro, pero no se puede reutilizar código. |

### Segmentación y capas 2D
| Proyecto | Licencia | Notas |
| --- | --- | --- |
| [See-through](https://github.com/shitagaki-lab/see-through) (SIGGRAPH 2026) | Apache‑2.0 | 1 ilustración → **hasta 23 capas semánticas con las partes ocultas rellenadas** (pelo delantero/trasero, cara, ojos, ropa…) + profundidad + orden de dibujo, en PSD. Base ideal para el **pelo por capas** y para separar las texturas. Necesita entre 8 y 16 GB de VRAM. |
| [BiRefNet ToonOut](https://huggingface.co/sprited/birefnet-toonout-onnx) | MIT | Quitar fondo en anime (~99.5 %). **Existe en ONNX**, así que se ejecuta desde Rust con el crate `ort`. |
| [SkyTNT anime-segmentation](https://github.com/SkyTNT/anime-segmentation) (`isnet-anime`) | Apache‑2.0 | Alternativa más ligera; el crate `anime-seg-rs` ya la usa en Rust. |

### Rigging automático
| Proyecto | Licencia | Notas |
| --- | --- | --- |
| [UniRig](https://github.com/VAST-AI-Research/UniRig) (SIGGRAPH 2025) | MIT | Predice esqueleto + pesos de *skinning*, **incluidos los *spring bones* de pelo y falda con su rigidez y gravedad**. Entrenado también con 2 061 modelos de VRoid. |
| Make-It-Animatable (CVPR 2025) | — | Riggea un humanoide en <1 s; no cubre *spring bones*. |

### Formato y herramientas
- **VRM 1.0** (glTF 2.0 + extensiones): humanoide estándar, `VRMC_expressions` (parpadeo, aa/ih/ou/ee/oh, joy/angry/sorrow/fun/surprised, expresiones propias), `VRMC_springBone` (física de pelo), `VRMC_materials_mtoon` (*toon shader*). Blender lo importa con el **VRM Add-on for Blender** (open source). En Rust, el crate `gltf` lee y escribe glTF; las extensiones VRM son JSON que podemos escribir nosotros.
- **VRoid Studio**: el referente de "personaje anime con pelo por capas editable"; sus mechones son grupos de mallas independientes.

## 5. Conclusión sobre el hardware (importante)

Este equipo tiene una **AMD RX 6400/6500 XT con 4 GB de VRAM** y 16 GB de RAM. StdGEN, TRELLIS.2, See-through y UniRig piden entre 8 y 24 GB y casi todos CUDA. **No pueden ejecutarse aquí.** Opciones:

1. **Backend remoto intercambiable** (recomendado): la app de Rust hace todo lo interactivo (visor, capas, rig, animación, física, exportación) y delega en un "proveedor" la generación pesada: un Space de Hugging Face, un servidor propio en una GPU alquilada o una API comercial (Tripo/Meshy). Se define como una interfaz (*trait*) para poder cambiar de proveedor.
2. **En local solo lo ligero**: quitar fondo (ToonOut/isnet en ONNX con `ort`, en CPU o con DirectML/ROCm), simplificar a *low poly*, la física y el *rendering*.
3. **Modelos en la RAM, ejecutados en la CPU**: válido para los modelos pequeños, como quitar el fondo (BiRefNet, ~220 M parámetros, segundos), Depth Anything V2 o la segmentación. Los grandes son otra historia: TRELLIS.2 (4 B, unos 8 GB en bf16), StdGEN y See-through (que se basa en SDXL) usan *kernels* que solo existen en CUDA (nvdiffrast, convoluciones dispersas, xformers) y en CPU tardarían de decenas de minutos a horas, con 16 GB muy justos. Descargar a la RAM parte del modelo (*offload*) desde una GPU de 4 GB es posible pero lento, porque la 6500 XT va por PCIe x4. Además, ROCm no da soporte oficial a esa GPU (gfx1034); solo funciona forzándolo con `HSA_OVERRIDE_GFX_VERSION=10.3.0`. Merece la pena probar en CPU TripoSR o SF3D, que son pequeños, aunque en anime su calidad es baja.
4. **Reconstrucción sin IA 3D** como alternativa *offline*: el enfoque 2.5D de Anime2.5DRig, con capas extruidas y deformadas. La calidad es menor, pero funciona siempre.

### Decisión (2026-11-17): todo en local, sin servicios externos

Enfoque propuesto, que funciona en CPU y con 4 GB de VRAM: **ajustar una plantilla y proyectar las texturas**, en lugar de generar el 3D con IA.

1. **Recorte**: BiRefNet ToonOut o isnet-anime en ONNX (`ort`, CPU) quita el fondo.
2. **Análisis 2D**: segmentación de partes (pelo, cara, ojos, piel, ropa), detección de puntos clave de cara y cuerpo y profundidad (Depth Anything V2 small en ONNX). Son modelos pequeños, que tardan segundos en CPU. Se pueden corregir a mano en la interfaz, como en mesh-avatar-studio.
3. **Plantilla base propia**: un cuerpo anime ya riggeado (humanoide VRM), una cabeza con *blend shapes* de expresiones kawaii y **ojos esféricos**, que se deforma con parámetros (altura, proporciones, cara, pecho…) para que coincida con las siluetas y los puntos clave de una o varias imágenes. El rig, las expresiones y las animaciones por defecto vienen de serie con la plantilla, así que no hace falta riggear automáticamente.
4. **Pelo por capas generado**: cada máscara de pelo (flequillo, laterales, melena trasera…) se convierte en una malla de tiras o mechones con el contorno de la silueta y el volumen de la profundidad, con sus cadenas de física.
5. **Ropa**: capas sobre el cuerpo (desplazar la superficie e inflar desde la silueta). Las faldas, como un cono de pliegues con física de tela.
6. **Texturas**: se proyectan las imágenes sobre la UV de la plantilla; lo que no se ve se rellena por simetría o con el color de la región.

Más adelante, como experimento opcional y siempre en local: See-through en CPU, como un paso lento que se deja trabajando de fondo, si la RAM aguanta. Y si algún día cambias de GPU, la interfaz de "proveedor de reconstrucción" permite enchufar StdGEN o TRELLIS.2 en local.

## 6. Pelo por capas: diseño propuesto (lo aprendido en doa.player)

Lo que doa.player ya resolvió para DOA3 y DOA2U y podemos reutilizar:
- **Cadenas de partículas** (`doa3_chains.rs`): por segmento, `fuerza += (D − segmento)·rigidez + viento·factor`, con límite `max_force`, gravedad, *pull* y amortiguación; la raíz va anclada a un hueso. Es lo mismo que un *spring bone* VRM, así que lo podemos portar casi tal cual.
- **Viento** (`doa3_wind.rs`): un anillo de 32 ráfagas con una fase distinta por registro, para que no se muevan todos los mechones a la vez.
- **Rejillas de tela** (`doax2_cloth.rs`): cadenas enlazadas de lado, anillo cerrado u hoja abierta. Valen para las capas de pelo anchas (flequillo, melena trasera) y no solo para mechones sueltos.
- **Capas translúcidas** (modo 2 de DOA3): "una segunda capa de cabeza y pelo con texturas de alfa parcial". Se dibujan **al final, con mezcla alfa y sin escribir profundidad**. Es justo lo que pide la transparencia por capa.
- **Cara trasera por material**: los forros y las capas pegadas necesitan ocultar las caras traseras, o la capa interior tapa a la exterior.
- **Vértices anclados**: los vértices que no mueve ninguna partícula deben quedarse en el ancla de su cadena; si no, se estiran en líneas.

Modelo de datos:
- `HairLayer { nombre, malla, material, orden, opacidad, visible, cadenas: Vec<SpringChain>, colisionadores }`. Capas típicas: flequillo, laterales izquierdo y derecho, melena trasera, coletas, mechones sueltos ("antenas"). Su origen son las capas de pelo de See-through o la segmentación de StdGEN.
- Cada capa es un **nodo/malla propio en glTF** con su material (`alphaMode: BLEND` cuando la opacidad es < 1) y su grupo de *spring bones* VRM. Así se puede animar, ocultar o volver transparente por separado en Blender y en otros motores.
- **Opacidad animable**: en nuestra app, como pista de animación; al exportar, `KHR_animation_pointer` sobre `baseColorFactor.a` (Blender lo importa de forma parcial, así que hay que avisar).
- **Orden de dibujo**: primero lo opaco; después las capas con alfa, de atrás adelante, por `orden`. Si hace falta, pasada previa de profundidad o mezcla ponderada (*weighted blended OIT*) para cuando las capas se crucen al moverse.
- **Física por capa**: rigidez, gravedad y amortiguación por capa, con doble muelle (raíz rígida, punta suave, como en Anime2.5DRig) y colisionadores de cabeza, cuello y hombros.


### Estado (2026-11-18): mechones con cadenas y física, primera versión

Implementado en `src/hair.rs`. Lo ejecuta `rig` al final, y también `anime2waifu3d hair ENTRADA.glb SALIDA.glb` sobre un modelo ya riggeado.

- **Mechones.**
  - La raíz está a la altura de las sienes: la altura del hueso de la cabeza más el 45 % de lo que mide de ahí a la coronilla.
  - El pelo que cuelga por debajo de la raíz se reparte en 12 sectores de 30° alrededor del eje de la cabeza. Cada sector con pelo suficiente es un mechón, que recibe su nombre por su lado: "mechón detrás izq", etc.
- **Cadenas.**
  - Cada mechón tiene una cadena `J_Sec_Hair{n}_{k}`, al estilo de VRoid, hija de la cabeza.
  - Tiene entre 2 y 8 segmentos de unos 0.3 cabezas de largo. Cada articulación es el centro del mechón en su nivel de altura.
- **Pesos.**
  - Por altura: la cabeza por encima de la raíz; después, mezcla lineal entre segmentos consecutivos.
  - Por ángulo: mezcla entre los dos mechones más cercanos, para que la malla no se rompa entre mechones.
  - Los vértices de pelo de objetos marcados con `--prop` no se tocan.
- **Capas.** Cada capa de pelo pasa a ser un grupo con un hijo por mechón, más "coronilla", "flequillo" y "resto". Así se pueden ocultar o volver transparentes por separado en la paleta, y se exportan como mallas separadas.
- **Física (`HairSim`).**
  - Puntas con integración Verlet a 60 Hz, cada segmento con su longitud fija, como en las cadenas de DOA3.
  - Rigidez hacia la forma de reposo. Se usa una dirección de reposo por segmento sacada de la malla, en lugar de la dirección única con `bend` de DOA3.
  - "Gravedad" como fracción de la dirección de reposo en el mundo que se mantiene cuando gira la cabeza.
  - Ángulo máximo respecto al reposo.
  - Viento racheado.
- **Colisiones.**
  - Cápsulas de tronco, cuello y brazos, y una esfera de cabeza.
  - El radio de cada una es un percentil bajo de la distancia de los vértices del cuerpo a su hueso.
  - En cada cadena, ese radio se reduce para que sus puntas en reposo queden fuera.
- **Visor.**
  - En "Pelo": casilla de física, juego de valores ("normal", "suelto", "rígido") y deslizador de viento.
  - Por línea de comandos, `--wind 0..1` y `--no-hair`. Las capturas asientan el pelo primero, con 120 pasos.

- **Flequillo.**
  - La raíz sube por delante desde las sienes hasta la frente (80 % de la cabeza), con una transición suave según el ángulo.
  - Los mechones de delante tienen sus propias cadenas cortas, con nombre "flequillo …".
  - El pelo que está a menos de 0.02 a 0.08 cabezas de la piel de la cabeza se queda con la cabeza, con transición suave. Así no se arrancan trozos de cara que la segmentación metió en el pelo.
- **`VRMC_springBone`.**
  - `export_glb` escribe una cadena por mechón y los colisionadores (cápsulas y la esfera de la cabeza).
  - Cada colisionador va en su propio grupo, y cada cadena solo usa los que no la tocan en reposo.
  - Los valores son los habituales de VRoid: rigidez 1, arrastre 0.4 y gravedad 0.1.
  - Blender importa el GLB con sus huesos. La física no está comprobada en un visor VRM.

- **Distancia por la malla** (2026-11-18).
  - Los mechones ya no se parametrizan por altura, sino por la distancia a la raíz por la superficie del pelo.
  - Se calcula con Dijkstra sobre los vértices soldados por posición, partiendo de todo lo que queda por encima de la raíz.
  - Los niveles de las articulaciones y los pesos usan esa distancia, así que sirve para coletas y mechones que salen de lado o se curvan.
  - Las piezas sueltas, que no llegan a la raíz, siguen usando la altura.
- **Choque entre mechones.**
  - Las puntas de mechones distintos que en reposo están a menos de un segmento no se acercan más del 70 % de esa distancia.
  - Después, cada cadena recupera la longitud de sus segmentos desde la raíz.
- **Física por capa.**
  - Cada cadena sabe qué capa mueve: la que tiene más vértices con ella.
  - En la paleta, al elegir un mechón o un grupo, se puede poner "general", "normal", "suelto", "rígido" o "fijo".
  - Por línea de comandos: `--hair-layer "mechón detrás=fijo"`.

Pendiente:
- Comprobar la física exportada en un visor VRM o en Blender con el VRM Add-on.
- Probar con un modelo con coletas: no hay ninguno entre los de prueba.
- Guardar la física de cada capa en el proyecto y exportarla (hoy solo vive en el visor).
- Opacidad animable.

### Faldas y abrigos con física (2026-11-18)

Usa el mismo constructor de cadenas que el pelo (`build_chains` en `src/hair.rs`), con `rig_skirt`. Lo ejecutan `rig` y `hair`.

- **Qué entra:** las capas de ropa de cintura para abajo, sin las de pies. Las cadenas `J_Sec_Skirt{n}_{k}` cuelgan de la cadera, por sectores de 30° alrededor de su eje, y la raíz está a la altura del hueso de la cadera.
- **Ceñido o suelto.** Las mallas de SupaVoxel son una sola piel, así que el abrigo y la media están unidos y hay que separarlos.
  1. Por cada hueso de la pierna, en tramos a lo largo de él y en sectores de 30° alrededor, se toma la superficie más interior de todo lo que no es pelo. Lo que sobresale de ella menos del 3.5 % de la pierna es ceñido; lo que sobresale más del 7 %, suelto. Se mide desde la superficie y no desde el hueso porque el hueso no tiene por qué ir centrado en la pierna.
  2. Lo que está por debajo del tobillo es siempre ceñido (calzado).
  3. Filtro de mayoría sobre la malla, para que la frontera quede limpia.
  4. Una zona ceñida solo sigue con la pierna si da la vuelta a su propio centro (8 de 12 sectores en algún tramo), como unas medias. Si no la da, es un trozo de abrigo que roza el muslo, y cuelga.
  5. Voto por capa: si el 60 % de una capa está de un lado, la capa entera pasa a ese lado.
- **Pesos:** lo ceñido conserva los pesos del rig y lo suelto pasa a sus cadenas.
- **Cortes:** los triángulos entre lo ceñido y lo suelto se quedan del lado de la mayoría, con copias de los vértices del otro lado. Si no, se estirarían en láminas al separarse.
- **Física:** la misma que el pelo, con cápsulas de muslos y espinillas, y un juego de valores propio ("Ropa:" en el panel).
- **Exportación:** van a `VRMC_springBone` junto con el pelo.

Resultado en la patada:
- La morena: el abrigo cuelga y la pierna patea entera, con su media.
- La rubia: el panel dorado de la sobrefalda se levanta sobre el muslo como una tabla. No se estira, pero queda rígido.
- La de pelo turquesa: no tiene falda, así que no salen cadenas.

Pendiente:
- Que la tela se doble sobre la pierna en vez de levantarse rígida: haría falta una rejilla con enlaces laterales, como las telas de DOAX2 (`doax2_cloth.rs`).
- En la de la motosierra, una pieza rígida de la pierna derecha (un objeto del rig) recibe cadenas de falda y se mueve con el viento.

## 6c. Cara: párpados de geometría (2026-11-18)

Decisión del usuario: geometría de párpados, no cambiar la textura. Implementado en `src/face.rs` (`rig_face`); lo ejecutan `rig` y `anime2waifu3d face ENTRADA SALIDA [--debug cara.png]`.

- **Ojos.** Los huesos de ojo heredados de Base_Female no sirven: tras el ajuste caen en la boca o en el cuello. Los ojos se buscan en la cara proyectada de frente:
  1. Primero, la profundidad de la piel. Después, solo lo que está pegado a ella: así quedan fuera la melena de detrás y el flequillo que flota delante. El pelo que está prácticamente sobre la piel cuenta como pintado, porque a veces el iris tiene el color del pelo y la segmentación lo mete en esa capa.
  2. Manchas lejanas al color de piel (ΔE > 30 en Lab), separadas donde solo las une una línea fina (erosión y crecimiento).
  3. Se elige la mejor pareja de manchas: una a cada lado del centro de la piel, a la misma altura, en la franja de los ojos, con una separación razonable. Cada una debe tener forma de ojo (ni raya vertical ni ceja plana), estar en buena parte rodeada de piel y tener color (el iris).
  4. Los dos ojos toman el tamaño del menor.
- **Párpados.**
  - Por ojo, un párpado superior y uno inferior: rejillas de 14×7 pegadas a la cara un poco por delante, con bordes redondeados y normales según la curva de la cara.
  - Material color piel (pasado a lineal, como pide glTF) y una fila de pestañas del color más oscuro del contorno del ojo.
  - Abiertos están plegados en su borde, sin área.
  - Formas (morph targets): `blink`, `blinkLeft`, `blinkRight` (el párpado superior baja y deja un arco ‿) y `happy` (los dos párpados se encuentran en un arco ^^).
- **Formato.**
  - El modelo carga y guarda morph targets, con nombres en `extras.targetNames`.
  - `VRMC_vrm.expressions.preset` enlaza `blink`, `blinkLeft`, `blinkRight` y `happy`.
  - Blender los importa como shape keys.
- **Visor.**
  - Sección "Cara" con expresión (neutral, parpadeo, guiño izq/der, feliz) y parpadeo automático (0.18 s cada 3.7 s).
  - Por línea de comandos, `--expression`.
- **Resultado.**
  - Bien en la rubia, la de pelo turquesa y la morena.
  - La de la motosierra tiene la cabeza girada: no se encuentran dos ojos y se queda sin párpados, con un aviso.

Pendiente:
- Boca (abrir y sonreír) para las expresiones y para hablar (`aa`, `ih`, `ou`, `ee`, `oh`).
- Cejas.
- Caras de perfil o giradas.
- Un ojo solo.

## 6b. Muelles de pecho (bouncing), de doa.player

doa.player tiene medidos y verificados contra el juego tres modelos, de menos a más rico:

| Fuente | Modelo | Parámetros (rigidez, amortiguación, impulso, reposo) | Deformación |
| --- | --- | --- | --- |
| DOA3 y DOAX (`docs/doa3-internals.md`, `docs/doax-notes.md`) | 1 eje (vertical), a 60 Hz: `target = r - r·chest.up.y`; `v = d·v - k·(x - target) - impulso·drive`; `x = clamp(x+v, -1, 1)`. `drive` es el cambio en y de un punto (0, 0.2, 0.3) del pecho. | 0.128, 0.85, 20 (·0.5 de pie), 0.25 | *Shape* positiva o negativa: `base + offset·|x|`. En DOAX el peso es `(1-k)|v| + k·sqrt|v|`, con k = 0.3. |
| DOA2U (`docs/doa2u-notes.md`) | La misma forma | 0.075, 0.9, 30, 0.3 (se usa la mitad) | *Shapes* absolutas |
| DOAX2 (`docs/doax2-notes.md`) | **3 ejes**: lateral, adelante y vertical; amortiguación (0.9, 0.97, 0.5), velocidad limitada a ±0.2 | Ver la tabla de doax2 | 6 *shapes* (lateral ±, adelante ±, vertical ±) |

Propuesta:
- **Dentro de la app**: el modelo de 3 ejes de DOAX2, con los parámetros de DOA3/DOA2U como ajustes predefinidos ("suave", "normal", "exagerado") y un deslizador de intensidad de 0 a 3, como el *Bounce* de doa.player.
- **Cómo se deforma**: el modelo base incluye **huesos de pecho izquierdo y derecho** y, además, *shape keys* de deformación. El muelle mueve los huesos; las *shapes* corrigen la forma.
- **Al exportar**: los huesos de pecho van como `VRMC_springBone` (así la física funciona en Blender/Unity/VRChat). Opcionalmente, el movimiento de la animación se hornea en pistas de hueso o de *shape*.
- Se activa por personaje (por defecto, en personajes femeninos), no por una lista fija como en DOA.
- Nota legal: los **números y fórmulas** son conocimiento medido y se pueden reutilizar. Las mallas y *shapes* de DOA no.

**Estado (2026-11-18): primera versión hecha.** `src/physics.rs` implementa el muelle de 1 eje de DOA3 con tres juegos de valores ("normal (DOA3)", "suave (DOA2U)", "exagerado"). En el visor, con una animación de prueba ("botar", saltitos del 4 % de la altura a 1,5 Hz), cada muelle gira su hueso `J_Sec_*_Bust1` hasta ±0,35 rad × intensidad. El deslizador "Rebote del pecho" va de 0 a 3. Desde la línea de órdenes: `--anim botar --frames N --fps F --bounce X` saca N capturas seguidas.
El rig añade **pesos de pecho por zona** (caída suave alrededor de cada hueso, solo por delante), porque el pecho de Base_Female es pequeño y la piel del objetivo heredaba casi todo del torso.
Pendiente: los 3 ejes de DOAX2 (lateral y adelante-atrás) y la exportación como `VRMC_springBone`.

## 7. Paleta de capas: referencia PhotoCraft

[storytold/photocraft](https://github.com/storytold/photocraft) (Apache-2.0/MIT, Rust): un clon de Photoshop hecho en Rust, con **wgpu + egui**, el mismo stack que doa.player. Usa un fork propio de egui 0.37; doa.player usa egui 0.29.

- **Su panel de capas** (`crates/ui-egui/src/panels/layers*.rs`): el panel genérico vive en su *framework* `craft::layers::ui` (un `LayersPanel` sobre un *trait* `LayerTree`: `children`, `parent`, `info`, `selection`, `accepts_drop`…). Ese módulo no está en la cabeza pública de `craft-libs`, y la pieza está muy atada a su *framework* y a su fork de egui. **No conviene usarlo como dependencia; sí copiar el diseño.**
- **Comportamientos de Photoshop que adoptaremos** en nuestra paleta:
  - el ojo para mostrar u ocultar;
  - **Alt+clic en el ojo = solo esta capa** (otra vez, restaurar);
  - **arrastrar sobre los ojos** para cambiar varias capas de una pasada;
  - grupos con flecha para plegar y desplegar (ocultar un grupo oculta sus hijas);
  - opacidad por capa y por grupo (se multiplican);
  - bloqueo de capa;
  - **miniatura de cada capa** (en 3D, un pequeño render de la capa sola);
  - selección múltiple (Mayús/Ctrl);
  - reordenar arrastrando, que en 3D cambia el orden de dibujo de las capas translúcidas;
  - menú contextual;
  - colores de etiqueta;
  - filtro por tipo (pelo, ropa, cuerpo, accesorios).
- **Cómo organizar el trabajo**: un *trait* tipo `LayerTree` separa la vista del modelo de datos. Lo copiaremos para que la paleta no dependa del formato del modelo.
- **Piezas reutilizables de [craft-libs](https://github.com/storytold/craft-libs)** (Apache-2.0):
  - `craft-psd`: lectura y escritura fiel de PSD/PSB, sin dependencias de UI. **Útil para importar PSD por capas** (la salida de See-through, o un PSD que el usuario haya separado a mano, que es lo que lee Anime2.5DRig).
  - `craft-mesh`: OBJ, STL, PLY, FBX, 3MF y lectura/escritura de GLB. Útil para exportar a formatos que no sean glTF.
  - `craft-ui/docking`: paneles acoplables, si algún día los queremos.

### El panel de capas sí existe: `craft-layers`

En la versión de `craft-libs` que fija PhotoCraft (`a6b9d9c986f7c4c399632d72d9a4a8c8ea0b86e9`) están los crates `crates/layers` (**craft-layers**, Apache-2.0) y `crates/craft` (la fachada). En la cabeza de `craft-libs` ya no están: se quitaron o se movieron.
- **craft-layers** es "el panel de capas para las Craft apps sobre el árbol de capas de la app". La app implementa `ui::tree::LayerTree` (una vista de solo lectura de sus capas) y lo muestra con `ui::panel::LayersPanel`. Este devuelve lo que hizo el usuario como **eventos**, y la app los aplica con sus propios comandos y su deshacer. Incluye miniaturas, filas de mezcla y bloqueo, barra de filtro, opciones del panel, arrastrar para reordenar y menús.
- Depende de `craft-ui`, `craft-behavior`, `craft-i18n`, `craft-keymap`, `craft-theme` y del **fork de egui 0.37** de storytold.
- Se puede usar como dependencia git fijada a esa versión. El riesgo es que ya no se mantiene en público.

### Otros repos de storytold relevantes
- **[TextureCraft](https://github.com/storytold/texturecraft)** (Apache-2.0): pintura de texturas 3D por capas, con máscaras y *bake* de mapas de malla (AO, curvatura, grosor) en GPU. Tiene visor 3D, vista UV 2D y cámara orbital (`crates/engine/src/orb_camera.rs`). Nos interesa para **retocar a mano las texturas proyectadas** y para el *bake*. Stack: fork de egui 0.37 + wgpu 30 + fork de winit.
- **[AniCraft](https://github.com/storytold/anicraft)**: animación 2D con línea de tiempo, *keyframes* y *tweens*. Es referencia para nuestra línea de tiempo de animaciones.
- **[MaterialCraft](https://github.com/storytold/materialcraft)**: materiales por nodos con vista 3D. Por ahora no lo necesitamos.
- **ArtCraft**: su recorte de fondo usa la API de fal.ai, un servicio externo, así que **no nos sirve**.

### Decisión (2026-11-17): stack de doa.player

Usamos egui 0.29 + wgpu 22 + winit 0.30 + glam, como doa.player. Del ecosistema storytold solo tomamos crates que no dependen de la UI (`craft-psd`, `craft-mesh`) y el diseño de `craft-layers`.

Comparativa que llevó a la decisión:
| | Stack de doa.player (egui 0.29, wgpu 22) | Stack de storytold (fork de egui 0.37, wgpu 30, craft-libs) |
| --- | --- | --- |
| Paleta de capas | La escribimos nosotros, con el diseño de craft-layers | `craft-layers` hecho y probado |
| PSD, OBJ, FBX… | `craft-psd` y `craft-mesh` funcionan igual: no dependen de la UI | Igual |
| Riesgo | Bajo: crates publicados en crates.io | Forks fijados por *commit*, muchas dependencias; craft-layers ya no está en la cabeza |
| Código de doa.player (cámara, física) | Se copia tal cual | Hay que adaptarlo a wgpu 30 y egui 0.37; la física no cambia |

### Diseño de la interfaz: FigCraft

[storytold/figcraft](https://github.com/storytold/figcraft) (Apache-2.0, Rust): un clon de Figma que funciona sin conexión. **Ya está instalado en este PC** (`/usr/bin/figcraft`, `figcraft-cli` 0.18.1, desde el rpm).
- Sirve para hacer **maquetas de la interfaz durante el discovery**, antes de escribir código: ventana principal, paleta de capas, panel de vista y cámara, asistente de importación, diálogo de exportación…
- Todo se puede hacer por comandos: `figcraft-cli run guion.json --out ui.figcraft` crea el archivo sin abrir ventana y `figcraft-cli render ui.figcraft -o ui.png` lo exporta a imagen. También tiene un servidor MCP (`figcraft-cli mcp`). Así puedo construir yo las maquetas y tú las abres y editas en FigCraft.
- Los archivos `.figcraft` se guardan en el repo (por ejemplo, `docs/ui/`) y quedan como especificación visual.
- Su generación de código (CSS/React/SwiftUI…) no produce egui, así que **solo nos sirve para diseñar**, no para generar la interfaz.
- Su propia interfaz es otra referencia de diseño: barra de herramientas, panel de capas a la izquierda, panel de propiedades a la derecha.

## 8. Candidatas a modelo base

Requisitos: licencia que permita incluirlo en la app y modificarlo (CC0 idealmente), topología limpia en quads con UVs, y estilo anime o fácil de estilizar.

| Candidata | Licencia | Estilo | Rig | Notas |
| --- | --- | --- | --- | --- |
| [MakeHuman / MPFB2](https://static.makehumancommunity.org/mpfb/about.html) (extensión de Blender) | Código GPLv3, **assets exportados CC0** | Realista | Sí (estándar, Rigify, game engine) | Cientos de *targets* de proporción (altura, cabeza, pecho, cintura…), ojos, dientes y lengua aparte. Lo más sólido técnicamente. Hay que estilizar la cabeza a anime. |
| [Blender Studio Human Base Meshes](https://www.cgchannel.com/2023/06/download-blender-studios-free-human-base-meshes/) | CC0 | Realista y **estilizado** | No | 17 mallas: cuerpos completos, cabezas, mandíbulas, ojos, manos y pies. Quads limpios con UVs. Muy buena calidad. |
| [Base Female Model with Anime Face](https://www.blendswap.com/blend/22080) (BlendSwap #22080) | CC0 | **Anime** | No (el autor quitó el rig IK) | Cuerpo con cara anime, para Blender 2.8. Hay que verificar la calidad. |
| [Female Anime Base Mesh](https://www.blendswap.com/blend/13616) (BlendSwap #13616) | CC0 | **Anime** | No | De 2014 (Blender 2.7x). Seguramente es simple. |
| [Anime female body](https://opengameart.org/content/anime-female-body) (OpenGameArt, adrix89) | CC0 | Anime, low poly | No | De 2010; hay versión 2 y versión masculina. Podría servir como base del modo *low poly*. |
| [Feminine Humanoid base mesh](https://opengameart.org/content/feminine-humanoid-base-mesh) (OpenGameArt, 2025) | CC0 | Estilizado/anime | ? | Sin cabeza. |
| [Stylized Low Poly Female Base Mesh](https://opengameart.org/content/stylized-low-poly-female-base-mesh-wip) (OpenGameArt) | CC0 | Estilizado | ? | Trabajo en curso. |

Descartadas: VRoid (los modelos de ejemplo no se pueden redistribuir y no hay versión para Linux) y los packs anime de CGTrader, Gumroad o Superhive (de pago y sin licencia CC0).

Propuesta: cuerpo de MPFB2 (rig + proporciones), con la cabeza a partir de una de las cabezas anime CC0 o de la cabeza estilizada de Blender Studio, ajustada a anime, con ojos esféricos y *shape keys* de expresiones hechos por nosotros. Se monta con un script de Blender reproducible. Antes hay que descargar las candidatas y evaluarlas en Blender.

### Evaluación (2026-11-17)

Descargadas en `assets/candidatas/` (ignorado por git), abiertas en un Blender 5.2 aislado (`BLENDER_USER_RESOURCES` en el scratchpad, sin tocar la configuración del usuario) y renderizadas en nuestro visor. Hoja comparativa: `assets/candidatas/comparativa.png`.

| Candidata | Malla | Rig | Veredicto |
| --- | --- | --- | --- |
| **MPFB2 2.0.18** + system assets CC0 | 19 158 vértices (cuerpo), 100 % quads, UV, 206 grupos de vértices, *targets* de proporción. Ojos, cejas y pestañas aparte. | **55 huesos** con el rig `game_engine_with_breast`: dedos y **`breast_l`/`breast_r`**. También hay rigs `default`, `mixamo`, `openpose` y Rigify. | **Cuerpo y rig: sí.** Proporciones realistas, que se pueden llevar a anime con sus macros (altura, proporciones, cabeza) y *targets* propios. |
| Blender Studio Human Base Meshes 1.4.1 | Cuerpo femenino estilizado: 12 502 vértices, 100 % quads, UV, ojos aparte. Cabeza estilizada: 4 118 vértices. | No | Muy buena topología, pero estilo de animación occidental (nariz y labios grandes). Sirve de referencia, no como base. |
| OGA "Anime female body" v2 | 1 313 vértices, 89 % quads, UV, 11 grupos de vértices | Pesos sencillos | **Proporciones anime de verdad** (cabeza, cuello y piernas). Demasiado *low poly* para la base detallada: **referencia de proporciones y posible base del modo *low poly*.** |
| OGA "Feminine humanoid" | 623 vértices + subdivisión | No | Sin cabeza y con proporciones exageradas. Descartada. |
| OGA "Stylized low poly" | Varias piezas sueltas | No | Descartada. |
| BlendSwap #22080 / #13616 (cara anime) | — | — | Hay que iniciar sesión para descargar: pendiente de que el usuario las baje. |

**Conclusión**: cuerpo y rig de **MPFB2**, reproporcionado a anime tomando como guía la OGA "Anime female body". **La cabeza anime es el punto débil**: no hay ninguna CC0 de calidad. Opciones:
(a) la cara anime de BlendSwap #22080, si su calidad sirve;
(b) que el usuario encuentre una cabeza anime CC0;
(c) construirla nosotros en Blender por script, partiendo de la topología de cabeza de MPFB (para conservar los pesos), con ojos esféricos, e iterando con revisión visual.

### Hallazgo: modelos VRoid CC0 (2026-11-17)

[OpenGameArt: VRoid Studio CC0 models](https://opengameart.org/content/vroid-studio-cc0-models): modelos de las versiones alfa de VRoid Studio que pixiv publicó como CC0. Descargados en `assets/candidatas/vroid-cc0/`.

| Modelo | Licencia en el propio VRM | Notas |
| --- | --- | --- |
| **Base_Female** | **CC0** | **Base elegida.** VRM 0.x humanoide con 54 *humanBones* (87 nodos). **56 *morph targets* en la cara**, 15 expresiones (A/I/U/E/O, parpadeo y guiño izquierdo/derecho, Angry/Fun/Joy/Sorrow/Surprised…). Huesos **`J_Sec_L/R_Bust1-2`** (pecho) y **`J_Sec_*_Skirt*`** (falda). *Spring bones*. Cara anime auténtica. |
| **HairSample_Female** | **CC0** | El mismo cuerpo con **pelo VRoid por mechones** (coletas, orejas de gato). Referencia de pelo por capas. |
| AvatarSample_D–G, Sendagaya Shino, Sakurada Fumiriya | "Other" (enlace a VRoid Hub); el que los subió dice que son CC0 | Personajes completos con ropa y pelo. Los usamos como **referencia y prueba en local**; no los incluimos en la app mientras no se confirme la licencia oficial (la página de ayuda de VRoid devuelve 403 a nuestras peticiones). |

Consecuencias:
- **La cabeza anime está resuelta**: la cara de Base_Female ya trae expresiones. Encima añadiremos los ojos esféricos.
- **¿Cuerpo de VRoid o de MPFB2?** VRoid ya tiene proporciones anime, huesos de pecho y falda, y es VRM nativo. MPFB2 aporta *targets* de proporción mucho más ricos (altura, pecho, cintura…) y dedos con más detalle. Propuesta: **Base_Female como base completa**, y reproporcionarla con nuestras propias *shape keys*; MPFB2 queda como alternativa.
- Los VRM 0.x miran hacia −Z: el visor los gira media vuelta al cargarlos.

### Sketchfab (búsqueda "waifu")
La API pública (`api.sketchfab.com/v3/search?tags=waifu&downloadable=true&license=…`) no devuelve ningún CC0, pero sí unos 24 CC-BY. Muchos son *fan art* de personajes con copyright (Rem, Miku, Xenoblade…): no sirven como base. Hay algunos originales (p. ej. "Anime Girl Casual Outfit" de AoG.01 o "Alice" de kaisamafab) que valen como referencia o prueba, citando al autor. Descargar requiere iniciar sesión.
- "Pink-Haired Anime Girl – SupaVoxel" (CC-BY): **generado por IA** a partir de una imagen; 1,4 M triángulos, una sola malla, sin rig. Sirve para comparar "IA de imagen a 3D" con nuestro enfoque, no como base.

### Prueba con mallas de IA de Sketchfab (SupaVoxel, CC-BY)

Descargadas por el usuario en `assets/candidatas/sketchfab/`:

| Modelo | Triángulos | Vértices | Textura | Estructura |
| --- | --- | --- | --- | --- |
| Dark-Haired Anime Heroine (`waifu020.glb`) | 1 378 446 | 745 286 | 1 PNG de 2048² (solo color base) | 1 malla, 1 material, sin rig ni *morphs* |
| Anime Dark Lady Figure (`120.glb`) | 1 925 480 | 1 041 579 | 1 JPEG de 2048² | Igual |

- **Ambas están cortadas a mitad de muslo** (son "figuras" para imprimir): no hay piernas ni pies.
- Los brazos están separados del cuerpo, algo que facilita ponerles el rig. El pelo está fusionado con hombros y espalda.
- El visor (release) abre cada una en ~1 s.
- **Paso 1, reducción: funciona.** Blender, *collapse* respetando las costuras de UV: 1 378 446 → **60 000 triángulos en 30 s**, sin pérdida visible, porque el detalle está en la textura. Script: `tools/blender/decimate.py`.
- **Teal-Haired Anime Girl (`waifu026.glb`)**: **cuerpo entero**, con piernas y botas; 647 166 triángulos y textura de 2048². Brazos caídos y separados del cuerpo, piernas abiertas: una buena pose para ponerle el rig. Reducida a 60 000 triángulos en 14 s, sin pérdida visible. **Es el modelo de prueba principal de esta ruta.**
- **Anime Chainsaw Girl (`waifu039.glb`)**: cuerpo entero, 856 636 triángulos y textura de 2048². Pelo corto, chaqueta abierta (buena prueba de ropa por capas, porque la chaqueta queda separada del cuerpo), medias altas y tacones. El brazo derecho está levantado sujetando una motosierra fusionada a la mano: es una buena prueba para separar un objeto como accesorio, pero esa pose complica el rig del brazo. Es el segundo modelo de prueba.
- **Dark-Skinned Anime Ninja (`waifu006.glb`)**: cuerpo entero, 714 840 triángulos. Abrigo largo abierto por delante (buena prueba de **tela larga con física**), cuello alto, pelo largo, piernas asimétricas (una con bota alta). Pese al nombre, **la IA le puso piel clara**: la textura es clara, no es un fallo del visor.
- **Red-Haired Anime Lady (`waifu016.glb`)**: cuerpo entero, 1 156 096 triángulos. Pese al nombre, el pelo es **rubio dorado** (lo rojo es la sobrefalda). El pelo es muy largo y voluminoso, con muchos mechones y un lazo: **la mejor prueba de pelo por capas**. Lleva una sobrefalda abierta (tela con física), una espada a la cintura (accesorio) y botas; los brazos caídos.
- **"Chica anime con lentes de sol" (`base_basic_shaded.glb`)**: probablemente generado por **Tripo AI** (por el nombre de archivo), 499 844 triángulos. **Sin ropa**: sirve de referencia de la forma del cuerpo bajo la ropa. Tiene accesorios fusionados (orejas de conejo, gafas, puños, collar, pelo) y los pies modelados con tacón. Licencia pendiente de confirmar: solo en local.
- **Mai Shiranui (`mai shiranui fan art hi3d.glb`)**: *fan art* generado con **hi3d** y exportado con three.js; 2 000 000 triángulos y 2 texturas (color y canales empaquetados). Estilo **semirrealista** (más de videojuego que anime plano). **Pose de acción** (brazo en la frente, mano en la cadera, piernas abiertas): la prueba más difícil para quitar la pose (llevarlo a la pose de reposo) y poner el rig. Personaje de SNK: **solo para pruebas en local**.
- **UV de SupaVoxel**: la textura es un atlas troceado en cientos de islas pequeñas (desplegado automático). Al separar las capas por color basta con mirar el píxel de cada cara, pero retocar la textura a mano es inviable: al separar en capas **habrá que volver a desplegar las UV y hornear (*bake*) cada capa** en una textura limpia.
- **Paso 2, separar en capas: funciona (primera versión, 2026-11-17).** `anime2waifu3d segment ENTRADA SALIDA.glb` (`src/segment.rs`) tarda menos de 1 s con 60–80k triángulos:
  1. Color de cada cara (4 muestras de la textura) en Lab.
  2. k-means ponderado por área (8 grupos).
  3. Filtro de mayoría entre caras vecinas (soldando los vértices de las costuras de UV) y absorción de trozos de menos del 0,2 % del área.
  4. Cada trozo conexo es una región. Las vecinas se unen **solo si son un degradado**: su frontera es suave (ΔE < 12) y además mucho más suave que la diferencia entre sus medias. Así el pelo que oscurece hacia las puntas queda en una sola capa, y el pelo negro y el abrigo azul marino siguen separados.
  5. Las piezas diminutas aisladas pasan a la región más cercana.
  6. Clase por pistas (piel por el tono; pelo, la región que llega más arriba más las de su color en la parte alta; calzado abajo; accesorios si son pequeños; el resto, ropa) y zona por la posición (cabeza, torso, brazo izquierdo/derecho, cintura, pierna, pie).
  7. Se escribe un GLB (`src/export_glb.rs`) con un grupo por clase y una capa por región.

  Resultados: turquesa en 21 capas, rubia en 23, motosierra en 31, ninja en 17; en las cuatro el pelo, la piel, la ropa y el calzado quedan correctos a simple vista. El visor tiene el modo **"Colores por capa"** (`--mode layers`) para comprobarlo.
  Limitaciones conocidas:
  - **Debajo de la ropa no hay cuerpo** (la IA solo modeló la superficie visible): al ocultar la ropa se ven agujeros. Hay que rellenarlos con el cuerpo de Base_Female ajustado.
  - Los ojos, las cejas y la boca quedan dentro de "piel cabeza" (paso 4).
  - Las manos quedan sueltas cuando una muñequera corta el brazo.
  - Muchos accesorios pequeños: habrá que poder fusionar capas a mano en la paleta.
- **Paso 3, rig: funciona (2026-11-18).** `anime2waifu3d rig CAPAS.glb Base_Female.vrm SALIDA.glb [--prop "capa,capa=hueso"]` (`src/rig.rs`), unos 75 s por modelo con 4 hilos:
  1. Escala y centra Base_Female sobre el objetivo, **sin la falda del uniforme**: ocupa el sitio de las manos y les daba pesos de pierna.
  2. Busca la dirección de brazos y piernas, hueso a hueso: primero toda la esfera (encuentra un brazo levantado), después afina, minimizando el chamfer en los dos sentidos. Los puntos de la extremidad que quedan **dentro** del objetivo cuentan triple, para que el brazo no se esconda en el torso.
  3. **Cadenas** (torso, brazo izquierdo/derecho, pierna izquierda/derecha) por votos de los vecinos del modelo base con normal parecida, limpiadas sobre la malla: filtro de mayoría, el trozo conexo más grande de cada extremidad, y las piezas sueltas por votación de la pieza entera.
  4. **Ropa de cintura para abajo** (por el nombre de la capa): nunca con huesos de brazo.
  5. **Geometría extra** (lejos del cuerpo base): rígida con su ancla. Las **regiones objeto** van enteras con la parte del cuerpo que más las toca. Los **objetos marcados a mano** (`--prop`) van rígidos con un hueso humanoide; en la interfaz será un menú de la paleta ("Objeto → mano derecha").
  6. Pesos por cercanía, solo de huesos de la cadena del vértice (más el torso cerca de la raíz de la extremidad), **suavizados sobre la malla** dentro de cada cadena.
  7. **Corte de contactos fusionados**: la IA cose la mano a la cadera; esos vértices se duplican, una copia por cadena (de 0 a 900 por modelo).
  8. Vuelta a la pose en T de Base_Female y exportación con huesos, skin y VRMC_vrm humanoide.

  Resultado con las 6 poses del visor (`--pose`): **turquesa, rubia y ninja se mueven bien**. La motosierra necesita `--prop` para el arma, porque la segmentación clasificó partes del metal como pelo. Quedan escamas pequeñas donde se cortaron contactos.
  Pendiente del rig: ajustar también la longitud de brazos y piernas; huesos de falda y física para la ropa larga (ahora sigue a cadera y pierna, y en una patada se estira); acelerar más.
- Pendiente: paso 4 (cara animable) y **completar las piernas** de los modelos cortados.

### MMD (PMX/VMD): visor de Genshin/HSR/ZZZ y modelos de DeviantArt

- **[phoshco/model-viewer](https://github.com/phoshco/model-viewer)** (código MIT, TypeScript): visor web de los modelos **MMD oficiales** que HoYoverse publica (Genshin, Honkai Star Rail, ZZZ, Wuthering Waves…), con **Babylon.js + [babylon-mmd](https://github.com/noname0310/babylon-mmd)**. Carga PMX y motions VMD/BVMD con física. Los modelos se descargan de fuera del repo; en el repo hay unos 9 PMX.
  Lo que nos aporta:
  - **Referencia visual** de render toon de calidad: sombreado celular, contornos, sombras de cara.
  - **PMX es el formato anime de facto** y trae justo lo que buscamos: huesos estándar MMD, *morphs* de cara (parpadeo, guiño, vocales あいうえお, expresiones), **física de pelo, falda y pecho** con cuerpos rígidos y *joints* (Bullet), y orden de dibujo por material (capas translúcidas).
  - **babylon-mmd** (MIT) es una implementación de referencia de física MMD, IK y *morphs*.
  - Las **motions VMD** (bailes, gestos) son una posible fuente de animaciones de prueba; el mapeo de huesos MMD → humanoide VRM es conocido.
  - **Propuesta**: añadir **importación de PMX (+ VMD)** a la app. Daría modelos de prueba con rig, cara y física de verdad, y animaciones para probar el sistema.
  - Licencias: los modelos oficiales de HoYoverse tienen sus propios términos (uso no comercial de fans, con condiciones en el readme de cada modelo). **Solo en local, como referencia**; nunca se incluyen en la app.
- **[DeviantArt oz-sys: MMD Snowbreak y más](https://www.deviantart.com/oz-sys/gallery/93514823/mmd---snowbreak---models)**: modelos MMD de Snowbreak, Wuthering Waves, ZZZ, Honkai Star Rail, Genshin, Tower of Fantasy… Algunos con descarga ("+ DL"). No indica licencia ni de dónde salen (seguramente extraídos de los juegos). **Mismo trato que DOA: solo pruebas en local**, nunca en el repo ni en la app.

## 9. Siguiente paso propuesto

1. Decidir el proveedor de la generación pesada (sección 5).
2. Esqueleto de la app en Rust con el mismo stack que doa.player: wgpu + winit + egui + glam. El visor reutiliza su cámara (`renderer/camera.rs`) y añade *presets* de cámara e iluminación.
3. Formato de proyecto: carpeta o zip con `source/` (imágenes), `layers/` (capas 2D y máscaras), `model.vrm` (o `.glb`) y `project.json`.
