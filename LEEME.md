Objetivo del proyecto: una aplicación en rust que pueda ejecutarse tanto en Fedora como en Ubuntu con una interfaz gráfica. 

- En la interfaz gráfica quiero poder pasarle una imagen o un conjunto de imagenes (jpg, jpeg, png, etc) de un mismo personaje. Es muy probable que esas imagenes no estén aisladas del fondo.
- A partir de esas imagenes, construir un modelo 3D con sus texturas (sacándolas de las imagenes). 
- Guardar el proyecto significa guardar el modelo (en el formato que mejor te vaya) junto con las imagenes y las texturas extraidas de las imagenes
- Debe tener una opción para poder asignarle huesos para poder animarlos. Deberíamos tener un conjunto de animaciones por defecto para poder usar
- Entre esas animaciones imitar expresiones faciales Kawaii de las chicas japonesas
- Las caras deben poder animarse (moverse la boca, guiñar un ojo, parpadear, mofletes hinchados...)
- El pelo también se debe poder animar y debe poder tener física por defecto
- Debe tener opción para exportar a Blender y/o otros formatos comunes de 3D/Animación. En las opciones de exportación poder seleccionar si exportar todo (modelo, texturas y animaciones) o solo modelo y texturas
- Las iluminación del visualizador que sea luz natural pero podemos tener un set diferente de iluminaciones.
- El visualizador debe permitir zoom, pan, rotate como hace el proyecto doa.player que se encuentra en /home/pandi/Projects/doa.player
- El visualizador además puede tener diferentes presets de cámara: primer plano, plano general, etc
- Pelo por capas (flequillo, laterales, melena trasera, coletas, mechones...): cada capa se puede animar por separado, tiene su propia física y se puede ocultar o hacer transparente (también animando su opacidad). Las capas se exportan como mallas separadas.
- Muelles de pecho (bouncing) para personajes femeninos, con un control de intensidad. Usar los valores y cálculos medidos en doa.player (DOA3/DOA2U/DOAX/DOAX2).
- Paleta de capas al estilo Photoshop: cada parte del modelo (modelo base, cada capa de pelo, cada prenda, ojos, accesorios...) aparece como una capa que se puede mostrar/ocultar, para ver las partes por separado.
- Modos de visualización: alambre (wireframe), color sólido y texturizado.
- (Futuro, cuando lo básico funcione) Opción para generar una versión low poly del modelo, al estilo del vídeo de Grok de Nagatoro en /home/pandi/Projects/assets.3d.

- Último paso del proyecto, cuando todo lo anterior esté terminado: documentar en /home/pandi/Projects/doa.player la nueva interfaz (paleta de capas, modos de vista, iluminación, cámaras, etc.) para poder adaptarla allí.

La investigación de referencias y herramientas está en docs/investigacion.md

Encontré unos proyectos que pueden ayudar para ver cómo se hacen algunas cosas

- https://github.com/shinshin86/mesh-avatar-studio
- https://github.com/shinshin86/aituber-onair
- https://github.com/shinshin86/live2d-add-motion-sample-web-ui
- https://github.com/shinshin86/Anime2.5DRig

Y en este video explican cómo hacer ojos esféricos https://x.com/Topolazy/status/2121600365997260823?a=NVS9yN3YWbENi4jss8I&s=35
