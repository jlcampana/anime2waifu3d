# Hoja de ruta V2: el 3D lo genera una IA

Rama: `ruta-v2`. Fecha: 2026-11-19. Motivo: [FRACASO.md](./fracaso/FRACASO.md).

## Por qué falló la ruta desde imagen

- **La ruta da por hecho una sola pose.** El ajuste de Base_Female (`fit.rs`) necesita una imagen de cuerpo entero, de pie y de frente. Busca pies, cintura, hombros y barbilla, y proyecta la textura de frente.
  - Si la imagen está tumbada, inclinada o cortada a medio muslo, los puntos clave salen mal. El cuerpo se deforma: cabeza enorme, cuello largo y un brazo levantado sin motivo.
  - La ruta no avisa de que la imagen no le sirve.
- **La plantilla es poco flexible.** Base_Female solo se estira por tramos, así que nunca se parecerá al dibujo como se parece una malla generada. El pelo y la ropa son añadidos geométricos sencillos.
- **La ruta que sí funciona es la de mallas de IA.** Con las mallas de SupaVoxel, `from_ai_mesh` saca capas, rig, mechones con física, párpados y boca, y las poses se mueven bien.

**Conclusión:** el cuerpo base no es lo único que falla. El enfoque de plantilla tiene un techo bajo. Lo lógico es que una IA genere la malla y que nuestra app haga todo lo demás, que es lo que ya funciona.

```
imagen(es) ──► [1. normalizar pose] ──► [2. imagen → malla 3D] ──► from_ai_mesh ──► [low poly opcional]
                 opcional, IA de imagen      servicio externo           capas, rig,       meshopt + sombreado
                 (de pie, de frente,         (Tripo, Meshy,             pelo, cara        plano
                  vista de espaldas)          Hunyuan3D…)               (ya hecho)
```

## Cambio de reglas que implica

- **«Todo en local» pasa a «todo en local salvo la generación de la malla»**, y solo si el usuario elige un servicio. La app sigue funcionando sin conexión: visor, rig, física, exportación y la ruta de mallas importadas.
- **Antes de enviar nada, la app avisa** de qué imágenes salen, a qué servicio y cuánto cuesta.
- **Imágenes de prueba:** las imágenes que se suben a servicios externos solo pueden ser de personajes **adultos**. Nada de contenido sexual de personajes menores, tampoco en pruebas. Las de `fracaso/origen.png` son de Marin, igual que las que ya se rechazaron: no se usan ni se suben. Además, casi todos los servicios prohíben el contenido explícito en sus condiciones.
- **Grok:** que yo sepa, Grok (xAI) genera imágenes y vídeos, pero no mallas 3D. El vídeo *low poly* de Nagatoro es un vídeo, no un modelo (ver `docs/investigacion.md` §2). Grok puede servir en el paso 1 (normalizar la pose), pero no en el paso 2. Lo verifico antes de que pagues nada.

## Fase 0: preparación (poco trabajo)

1. **Diálogo de archivos en la carpeta correcta.** Ahora `rfd::FileDialog` (`src/main.rs:349`) se abre en la carpeta por defecto del portal. Propuesta, en este orden:
   1. la carpeta del último archivo usado con ese tipo de diálogo (imágenes, mallas, proyectos, exportaciones), guardada en `~/.config/anime2waifu3d/`;
   2. si no hay, la carpeta del proyecto o modelo abierto;
   3. si no hay, la carpeta de la app (`assets/resultados/`).

   Se usa `set_directory`; en el diálogo de guardar, también `set_file_name`.
2. **Avisar de las imágenes no aptas** en la ruta actual: si la imagen no es de cuerpo entero o la persona no está de pie y de frente, se avisa en la ventana «Proceso» en vez de generar un cuerpo deforme.
3. **Juego de imágenes de prueba legales:** 3 o 4 personajes adultos, con al menos uno de cuerpo entero de frente y uno de espaldas. Tarea del usuario en `humano.md`.

## Fase 1: probar los servicios a mano (sin código)

La app ya tiene «Preparar malla de IA…». Así que se puede probar todo sin programar nada: subir la imagen a la web del servicio, descargar el GLB y pasarlo por la app.

- **Servicios candidatos.** Todos tienen nivel gratuito o créditos de prueba. Los precios y las condiciones hay que comprobarlos el día de la prueba.

  | Servicio | Lo interesante |
  | --- | --- |
  | **Tripo** | Varias vistas, opción de *low poly* o retopología y rig automático. Probablemente el mismo tipo de malla que SupaVoxel. |
  | **Meshy** | Modo *low poly*, varias imágenes y API. |
  | **Hunyuan3D** (Tencent) | Web gratuita y textura muy buena. Licencia restrictiva para la malla. |
  | **TRELLIS / TRELLIS.2** (Space de Hugging Face) | Gratis y MIT. Cola de espera. |
  | **Rodin (Hyper3D)** | Alta calidad y opción de *low poly*. |

- **Procedimiento:** las mismas 2 o 3 imágenes en cada servicio, con y sin normalizar antes la pose (paso 1, con Grok, Gemini o ChatGPT: «este personaje de pie, de frente, en pose A, cuerpo entero, fondo blanco» y otra de espaldas).
- **Qué se mide:**
  - parecido con el dibujo (cara, pelo, ropa);
  - si sale de cuerpo entero;
  - triángulos y texturas;
  - si separa partes;
  - cómo funciona después nuestro `preparar`: capas, rig, poses y cara;
  - tiempo y coste por personaje;
  - licencia del resultado y política de contenido.
- **Resultado:** una tabla en `docs/investigacion.md` y la decisión de qué servicio integrar. Si ninguno convence en anime, se para aquí y se replantea.

## Fase 2: integrar el servicio en la app

1. **Interfaz de proveedor** (`trait Proveedor`) en un módulo nuevo `src/proveedor.rs`: recibe imágenes y opciones (*low poly* sí o no, número de caras) y devuelve un GLB. Corre como trabajo en segundo plano (`start_job`), con avance y cancelación.
2. **Proveedor «manual»** (el primero, sin API): abre la web del servicio en el navegador y vigila la carpeta de Descargas. Cuando aparece un GLB nuevo, lo pasa por `from_ai_mesh`. Sirve con cualquier servicio, incluidos los que no tienen API.
3. **Proveedor con API** del servicio elegido en la fase 1:
   - la clave se guarda en `~/.config/anime2waifu3d/` con permisos 600 y nunca en el proyecto ni en git;
   - antes de enviar, la app pide confirmación.
4. **Botón «Personaje desde imagen…»**: pregunta qué ruta usar, servicio o plantilla local. La línea de comandos tendrá lo mismo: `imagen --proveedor tripo A.png salida.glb`.
5. **Proyecto:** guardar qué servicio y qué opciones se usaron, y la malla original, para poder rehacer el rig sin pagar otra generación.

## Fase 3: adaptar `from_ai_mesh` a lo que devuelvan los servicios

- Mallas de pocos polígonos: comprobar que la segmentación por color, el corte de contactos y los pesos siguen bien con 5 000–20 000 triángulos.
- Cuadriláteros, varias texturas o materiales PBR. Si el servicio da partes separadas o un rig propio, aprovecharlos o descartarlos.
- Piernas cortadas o poses raras: usar el paso 1 para pedir siempre cuerpo entero en pose A.

## Fase 4: modo *low poly* local

Un filtro posterior, como estaba previsto (`docs/investigacion.md` §2):
- reducción con `meshopt` respetando las costuras de UV;
- sombreado plano en el visor y en la exportación;
- volver a hornear la textura.

**¿Sale más barato?**
- *Al generarlo:* en los servicios cuesta lo mismo o casi.
- *En el PC:* sí. Con 5 000–10 000 triángulos, el rig pasaría de unos 75 s a pocos segundos, el visor y la física van más ligeros y los archivos ocupan menos.

Es una buena opción por defecto para anime.

## Fase 5: qué hacer con la ruta de plantilla

Recomendación: **congelarla, sin borrarla**, como opción sin conexión. Ya no se invierte en afinarla (punto 1 y punto 4 de los pasos del `CLAUDE.md`). Algunas piezas se reutilizan: el recorte (`cutout.rs`) para limpiar la imagen antes de enviarla y el análisis (`analyze.rs`) para avisar de las imágenes no aptas.

## Lo que no cambia

La paleta de capas al estilo Photoshop, la línea de tiempo de animación, la tela de verdad y la documentación para doa.player siguen igual. No dependen de cómo se genere la malla y se retoman después de la fase 2.

## Decisiones pendientes del usuario

- [ ] ¿Aceptas el cambio de regla («local salvo la generación de la malla, a elección del usuario»)?
- [ ] ¿Qué servicios pruebas en la fase 1? Hace falta crear cuenta en cada uno.
- [ ] Imágenes de prueba de personajes adultos.
- [ ] ¿Te vale el orden de carpetas del diálogo de archivos de la fase 0?
