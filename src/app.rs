//! Estado de la aplicación: el modelo cargado, la cámara y los ajustes del visor, y el dibujo de
//! un fotograma, en la ventana o en una imagen.

use crate::lighting::{LightRig, PRESETS};
use crate::model::Model;
use crate::renderer::camera::{Camera, Shot};
use crate::renderer::grid::GridRenderer;
use crate::renderer::scene::{SceneRenderer, ViewMode, ViewSettings};
use crate::renderer::RenderContext;
use anyhow::Result;
use std::path::Path;

/// Expresiones de la cara en el visor.
pub const EXPRESSIONS: &[&str] = &["neutral", "parpadeo", "guiño izq", "guiño der", "feliz", "sonrisa", "contenta", "gatito", "puchero", "sorpresa", "hablar"];

pub struct App {
    pub ctx: RenderContext,
    pub egui_ctx: egui::Context,
    pub egui_state: egui_winit::State,
    pub egui_renderer: egui_wgpu::Renderer,
    pub camera: Camera,
    pub grid: GridRenderer,
    pub scene: SceneRenderer,
    pub model: Option<Model>,
    /// Archivo abierto o guardado por última vez.
    pub path: Option<std::path::PathBuf>,
    pub view: ViewSettings,
    pub light: usize,
    pub show_grid: bool,
    /// Pose predefinida (ver `pose::PRESETS`), si el modelo tiene esqueleto.
    pub pose: String,
    /// Animación en curso (ver `ANIMATIONS`) y su tiempo.
    pub animation: String,
    pub anim_time: f32,
    /// Intensidad del rebote del pecho, de 0 a 3 (1 es el valor del juego, como en doa.player).
    pub bounce: f32,
    /// Juego de valores del muelle (ver `physics::PRESETS`).
    pub spring_preset: usize,
    springs: [crate::physics::BustSpring; 2],
    /// Física del pelo (si el modelo tiene cadenas `J_Sec_Hair*`), su juego de valores y el
    /// viento, de 0 a 1.
    hair: Option<crate::hair::HairSim>,
    pub hair_physics: bool,
    pub hair_preset: usize,
    /// Juego de valores de la ropa (faldas y abrigos).
    pub cloth_preset: usize,
    pub wind: f32,
    /// Expresión de la cara (ver `EXPRESSIONS`) y parpadeo automático.
    pub expression: String,
    pub auto_blink: bool,
    sim_time: f32,
    sim_accum: f32,
    pub status: String,
}

impl App {
    pub fn new(ctx: RenderContext, egui_ctx: egui::Context, egui_state: egui_winit::State) -> Self {
        let format = ctx.surface_format();
        let egui_renderer = egui_wgpu::Renderer::new(&ctx.device, format, None, 1, false);
        let grid = GridRenderer::new(&ctx.device, format);
        let scene = SceneRenderer::new(&ctx.device, &ctx.queue, format);
        let (w, h) = (ctx.surface_config.width.max(1), ctx.surface_config.height.max(1));
        let mut camera = Camera::new(w as f32 / h as f32);
        camera.frame_character(1.6);
        Self {
            ctx,
            egui_ctx,
            egui_state,
            egui_renderer,
            camera,
            grid,
            scene,
            model: None,
            path: None,
            view: ViewSettings::default(),
            light: 0,
            show_grid: true,
            pose: "reposo".into(),
            animation: "ninguna".into(),
            anim_time: 0.0,
            bounce: 1.0,
            spring_preset: 0,
            springs: Default::default(),
            hair: None,
            hair_physics: true,
            hair_preset: 0,
            cloth_preset: 0,
            wind: 0.0,
            expression: "neutral".into(),
            auto_blink: true,
            sim_time: 0.0,
            sim_accum: 0.0,
            status: "Arrastra un modelo .glb, .gltf o .vrm a la ventana, o usa Abrir modelo".into(),
        }
    }

    pub fn light_rig(&self) -> &LightRig {
        &PRESETS[self.light.min(PRESETS.len() - 1)]
    }

    pub fn open(&mut self, path: &Path) {
        match self.try_open(path) {
            Ok(()) => {}
            Err(error) => self.status = format!("Error al abrir {}: {error:#}", path.display()),
        }
    }

    fn try_open(&mut self, path: &Path) -> Result<()> {
        let model = Model::load(path)?;
        self.scene.set_model(&self.ctx.device, &self.ctx.queue, &model);
        self.status = format!(
            "{}: {} capas, {} triángulos, {} texturas",
            model.name,
            model.layers.len(),
            model.triangle_count(),
            model.textures.len()
        );
        if let Some(skeleton) = &model.skeleton {
            self.status += &format!(", {} huesos ({} humanoides)", skeleton.bones.len(), skeleton.humanoid.len());
        }
        self.hair = crate::hair::HairSim::new(&model);
        let project = crate::project::read(&model);
        self.model = Some(model);
        self.path = Some(path.to_path_buf());
        self.frame(Shot::Full);
        match project {
            Some(project) => {
                self.apply_project(&project);
                self.status += " (proyecto)";
            }
            None => self.settle(),
        }
        Ok(())
    }

    /// El estado de la app, para guardarlo con el proyecto.
    pub fn project_state(&self) -> crate::project::Project {
        use crate::project::{CameraState, LayerState, Project, VERSION};
        let layers = self.model.as_ref().map_or(Vec::new(), |m| {
            m.layers
                .iter()
                .enumerate()
                .map(|(i, l)| LayerState {
                    name: l.name.clone(),
                    visible: l.visible,
                    opacity: l.opacity,
                    expanded: l.expanded,
                    physics: self.hair_layer_preset(i).flatten().map(|p| crate::hair::PRESETS[p].0.to_string()),
                })
                .collect()
        });
        Project {
            version: VERSION,
            view_mode: match self.view.mode {
                ViewMode::Textured => "tex",
                ViewMode::Solid => "solid",
                ViewMode::Wireframe => "wire",
                ViewMode::LayerColors => "layers",
            }
            .into(),
            wire_overlay: self.view.wire_overlay,
            show_grid: self.show_grid,
            light: self.light,
            pose: self.pose.clone(),
            animation: self.animation.clone(),
            bounce: self.bounce,
            spring_preset: self.spring_preset,
            hair_physics: self.hair_physics,
            hair_preset: self.hair_preset,
            cloth_preset: self.cloth_preset,
            wind: self.wind,
            expression: self.expression.clone(),
            auto_blink: self.auto_blink,
            camera: Some(CameraState {
                target: self.camera.target.to_array(),
                distance: self.camera.distance,
                yaw: self.camera.yaw,
                pitch: self.camera.pitch,
            }),
            layers,
        }
    }

    /// Restaura el estado de un proyecto. Las capas se emparejan por índice si el nombre
    /// coincide; si no, por nombre.
    pub fn apply_project(&mut self, project: &crate::project::Project) {
        self.view.mode = match project.view_mode.as_str() {
            "solid" => ViewMode::Solid,
            "wire" => ViewMode::Wireframe,
            "layers" => ViewMode::LayerColors,
            _ => ViewMode::Textured,
        };
        self.view.wire_overlay = project.wire_overlay;
        self.show_grid = project.show_grid;
        self.light = project.light.min(PRESETS.len() - 1);
        if !project.pose.is_empty() {
            self.pose = project.pose.clone();
        }
        if !project.animation.is_empty() {
            self.animation = project.animation.clone();
        }
        self.bounce = project.bounce;
        self.spring_preset = project.spring_preset.min(crate::physics::PRESETS.len() - 1);
        self.hair_physics = project.hair_physics;
        self.hair_preset = project.hair_preset.min(crate::hair::PRESETS.len() - 1);
        self.cloth_preset = project.cloth_preset.min(crate::hair::PRESETS.len() - 1);
        self.wind = project.wind;
        if !project.expression.is_empty() {
            self.expression = project.expression.clone();
        }
        self.auto_blink = project.auto_blink;
        if let Some(camera) = &project.camera {
            self.camera.target = glam::Vec3::from(camera.target);
            self.camera.distance = camera.distance;
            self.camera.yaw = camera.yaw;
            self.camera.pitch = camera.pitch;
        }
        let mut physics: Vec<(usize, Option<usize>)> = Vec::new();
        if let Some(model) = &mut self.model {
            for (i, state) in project.layers.iter().enumerate() {
                let index = if model.layers.get(i).is_some_and(|l| l.name == state.name) {
                    Some(i)
                } else {
                    model.layers.iter().position(|l| l.name == state.name)
                };
                let Some(index) = index else { continue };
                let layer = &mut model.layers[index];
                layer.visible = state.visible;
                layer.opacity = state.opacity.clamp(0.0, 1.0);
                layer.expanded = state.expanded;
                let preset = state.physics.as_ref().and_then(|name| crate::hair::PRESETS.iter().position(|(n, _)| n == name));
                physics.push((index, preset));
            }
        }
        // En orden de capas: un grupo primero y luego lo propio de cada hija.
        for (index, preset) in physics {
            if self.hair_layer_preset(index).is_some() {
                self.set_hair_layer_preset(index, preset);
            }
        }
        self.settle();
    }

    /// Guarda el proyecto (modelo entero y estado) en un GLB.
    pub fn save_project(&mut self, path: &Path) {
        let Some(model) = &self.model else { return };
        let state = self.project_state();
        self.status = match crate::project::save(model, &state, path) {
            Ok(()) => {
                self.path = Some(path.to_path_buf());
                format!("Proyecto guardado en {}", path.display())
            }
            Err(error) => format!("Error al guardar {}: {error:#}", path.display()),
        };
    }

    pub fn export(&mut self, path: &Path, format: crate::project::Format, only_visible: bool) {
        let Some(model) = &self.model else { return };
        self.status = match crate::project::export(model, path, format, only_visible) {
            Ok(()) => format!("Exportado a {}", path.display()),
            Err(error) => format!("Error al exportar {}: {error:#}", path.display()),
        };
    }

    /// Juego de valores de la física de una capa de pelo: `None` si no mueve ninguna cadena,
    /// `Some(None)` si usa el general.
    pub fn hair_layer_preset(&self, layer: usize) -> Option<Option<usize>> {
        self.hair.as_ref()?.layer_preset(&self.model.as_ref()?.layers, layer)
    }

    pub fn set_hair_layer_preset(&mut self, layer: usize, preset: Option<usize>) {
        if let (Some(hair), Some(model)) = (&mut self.hair, &self.model) {
            hair.set_layer_preset(&model.layers, layer, preset);
        }
    }

    /// Si el modelo tiene párpados (la forma `blink`).
    pub fn has_face(&self) -> bool {
        self.model.as_ref().is_some_and(|m| m.layers.iter().any(|l| l.primitives.iter().any(|p| p.morphs.iter().any(|m| m.name == "blink"))))
    }

    /// Pesos de las formas de la cara: la expresión elegida y, en neutral, el parpadeo.
    fn morph_weights(&self) -> Vec<(&'static str, f32)> {
        let mut weights = match self.expression.as_str() {
            "parpadeo" => vec![("blink", 1.0)],
            "guiño izq" => vec![("blinkLeft", 1.0)],
            "guiño der" => vec![("blinkRight", 1.0)],
            "feliz" => vec![("happy", 1.0), ("aa", 0.45)],
            "sorpresa" => vec![("oh", 1.0)],
            // Bocas dibujadas en la textura.
            "sonrisa" => vec![("sonrisa", 1.0)],
            "contenta" => vec![("happy", 1.0), ("sonrisa", 1.0)],
            "gatito" => vec![("gatito", 1.0)],
            "puchero" => vec![("puchero", 1.0)],
            "hablar" => {
                // Vocales seguidas, de 0.14 s cada una, con un hueco cerrado de vez en cuando.
                const VOWELS: [&str; 7] = ["aa", "ee", "oh", "", "ih", "ou", "aa"];
                let phase = self.sim_time / 0.14;
                let vowel = VOWELS[phase as usize % VOWELS.len()];
                let open = (phase.fract() * std::f32::consts::PI).sin();
                if vowel.is_empty() { Vec::new() } else { vec![(vowel, 0.9 * open)] }
            }
            _ => Vec::new(),
        };
        let eyes_busy = weights.iter().any(|(n, _)| n.starts_with("blink") || *n == "happy");
        if self.auto_blink && !eyes_busy {
            // Un parpadeo de 0.18 s cada 3.7 s.
            let t = self.sim_time % 3.7;
            if t < 0.18 {
                weights.push(("blink", 1.0 - (t - 0.09).abs() / 0.09));
            }
        }
        weights
    }

    pub fn has_hair(&self) -> bool {
        self.hair.is_some()
    }

    /// Rotaciones locales de la pose elegida más el pecho, sin el pelo.
    fn body_pose(&self) -> Option<Vec<glam::Quat>> {
        let skeleton = self.model.as_ref()?.skeleton.as_ref()?;
        let mut local = crate::pose::preset(skeleton, &self.pose)?;
        // Pecho: cada muelle gira su hueso hacia abajo o hacia arriba (eje X: el personaje mira
        // hacia +Z). x negativo es quedarse atrás al subir, es decir, hacia abajo.
        for (side, spring) in ["J_Sec_L_Bust1", "J_Sec_R_Bust1"].iter().zip(&self.springs) {
            if let Some(b) = skeleton.find(side) {
                local[b] = glam::Quat::from_rotation_x(-spring.x * 0.35 * self.bounce) * local[b];
            }
        }
        Some(local)
    }

    /// Deforma el modelo con la pose elegida (más la animación, el pecho y el pelo) y sube los
    /// vértices a la GPU.
    pub fn apply_pose(&mut self) {
        let Some(model) = &self.model else { return };
        let Some(skeleton) = &model.skeleton else { return };
        let Some(mut local) = self.body_pose() else {
            self.status = format!("Pose desconocida: {}", self.pose);
            return;
        };
        if let Some(hair) = self.hair.as_ref().filter(|_| self.hair_physics) {
            hair.apply(skeleton, &mut local);
        }
        let morphs = self.morph_weights();
        let mut vertices = crate::pose::skin_model(model, &local, &morphs);
        let lift = self.body_offset();
        if lift != 0.0 {
            for layer in &mut vertices {
                for primitive in layer {
                    for v in primitive {
                        v.position[1] += lift;
                    }
                }
            }
        }
        self.scene.set_vertices(&self.ctx.queue, &vertices);
    }

    /// Un paso de 1/60 s del pelo con la pose actual.
    fn step_hair(&mut self) {
        let Some(local) = self.body_pose() else { return };
        let offset = glam::Vec3::Y * self.body_offset();
        // Viento racheado de lado y desde delante.
        let t = self.sim_time;
        let gust = 0.6 + 0.4 * (t * 1.3).sin() * (t * 0.47 + 1.0).sin();
        let height = self.model_height();
        let wind = glam::Vec3::new(0.6, 0.0, -1.0).normalize() * self.wind * gust * height * 0.0025;
        let params = crate::hair::PRESETS[self.hair_preset.min(crate::hair::PRESETS.len() - 1)].1;
        let cloth = crate::hair::PRESETS[self.cloth_preset.min(crate::hair::PRESETS.len() - 1)].1;
        let (Some(model), Some(hair)) = (&self.model, &mut self.hair) else { return };
        let Some(skeleton) = &model.skeleton else { return };
        let skin = crate::pose::skin_matrices(skeleton, &local);
        hair.step(skeleton, &skin, offset, &params, &cloth, wind);
    }

    /// Deja el pelo asentado en la pose actual (para cambiar de pose o hacer una captura).
    pub fn settle(&mut self) {
        if let Some(hair) = &mut self.hair {
            hair.reset();
        }
        for _ in 0..120 {
            self.step_hair();
        }
        self.apply_pose();
    }

    /// Altura del modelo, para escalar las animaciones.
    fn model_height(&self) -> f32 {
        self.model.as_ref().and_then(|m| m.bounds(false)).map_or(1.6, |(lo, hi)| hi.y - lo.y)
    }

    /// Desplazamiento vertical del cuerpo entero en la animación actual.
    fn body_offset(&self) -> f32 {
        match self.animation.as_str() {
            // Saltitos de 1.5 por segundo, de un 4 % de la altura.
            "botar" => self.model_height() * 0.04 * (std::f32::consts::PI * 1.5 * self.anim_time).sin().abs(),
            _ => 0.0,
        }
    }

    /// Avanza la animación y la física `dt` segundos, en pasos fijos de 1/60 s.
    pub fn tick(&mut self, dt: f32) {
        let animated = self.animation != "ninguna";
        let hair_active = self.hair_physics && self.hair.is_some();
        let blinking = (self.auto_blink || self.expression == "hablar") && self.has_face();
        if !animated && !hair_active && !blinking && self.springs.iter().all(|s| s.x.abs() < 1e-4) {
            return;
        }
        let Some(chest_y) = self.model.as_ref().and_then(|m| m.skeleton.as_ref()).and_then(|s| s.bone("chest").map(|b| s.rest_position(b).y)) else {
            return;
        };
        let params = crate::physics::PRESETS[self.spring_preset.min(crate::physics::PRESETS.len() - 1)].1;
        self.sim_accum += dt.min(0.1);
        while self.sim_accum >= 1.0 / 60.0 {
            self.sim_accum -= 1.0 / 60.0;
            if animated {
                self.anim_time += 1.0 / 60.0;
            }
            self.sim_time += 1.0 / 60.0;
            let y = chest_y + self.body_offset();
            for spring in &mut self.springs {
                spring.step(&params, 1.0, y);
            }
            if hair_active {
                self.step_hair();
            }
        }
        // Sin animación, solo se vuelve a deformar la malla si algo se mueve.
        let moving = self.springs.iter().any(|s| s.x.abs() >= 1e-4) || self.hair.as_ref().is_some_and(|h| hair_active && h.motion > 1e-5);
        if animated || moving || blinking {
            self.apply_pose();
        }
    }

    pub fn frame(&mut self, shot: Shot) {
        if let Some((low, high)) = self.model.as_ref().and_then(|m| m.bounds(true)) {
            self.camera.frame_shot(low, high, shot);
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.ctx.resize(width, height);
        if width > 0 && height > 0 {
            self.camera.set_aspect(width as f32 / height as f32);
        }
    }

    pub fn clear_color(&self) -> wgpu::Color {
        let [r, g, b] = self.light_rig().background;
        // El fondo está en sRGB y la superficie también lo es, así que se pasa a lineal.
        let lin = |c: f64| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        wgpu::Color { r: lin(r), g: lin(g), b: lin(b), a: 1.0 }
    }

    /// Prepara los uniformes y dibuja la escena 3D en `target`.
    pub fn draw_3d(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, depth: &wgpu::TextureView) {
        let view_proj = self.camera.view_proj();
        self.grid.update(&self.ctx.queue, &view_proj);
        self.scene.update(&self.ctx.queue, view_proj, self.camera.position(), self.light_rig(), &self.view, self.model.as_ref());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("3D"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(self.clear_color()), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if self.show_grid {
            self.grid.draw(&mut pass);
        }
        if let Some(model) = &self.model {
            self.scene.draw(&mut pass, &self.view, model);
        }
    }

    /// Dibuja la escena en una imagen de `width` x `height` y la guarda como PNG.
    pub fn screenshot(&mut self, path: &Path, width: u32, height: u32) -> Result<()> {
        let device = &self.ctx.device;
        let format = self.ctx.surface_format();
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("captura"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("captura profundidad"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let padded_row = (width * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("captura"),
            size: (padded_row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let aspect = self.camera.aspect;
        self.camera.set_aspect(width as f32 / height as f32);
        self.camera.set_insets(0.0, 0.0, width as f32, height as f32);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("captura") });
        self.draw_3d(
            &mut encoder,
            &color.create_view(&Default::default()),
            &depth.create_view(&Default::default()),
        );
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::ImageCopyBuffer {
                buffer: &buffer,
                layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(padded_row), rows_per_image: Some(height) },
            },
            size,
        );
        self.ctx.queue.submit([encoder.finish()]);
        self.camera.set_aspect(aspect);

        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::Maintain::Wait);
        let data = slice.get_mapped_range();
        let bgra = matches!(format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in data.chunks(padded_row as usize).take(height as usize) {
            for px in row[..(width * 4) as usize].chunks_exact(4) {
                if bgra {
                    pixels.extend([px[2], px[1], px[0], 255]);
                } else {
                    pixels.extend([px[0], px[1], px[2], 255]);
                }
            }
        }
        drop(data);
        buffer.unmap();
        image::save_buffer(path, &pixels, width, height, image::ExtendedColorType::Rgba8)?;
        Ok(())
    }
}

/// Lo que se pide por la línea de órdenes.
#[derive(Default)]
pub struct Options {
    pub model: Option<std::path::PathBuf>,
    pub shot: Option<std::path::PathBuf>,
    pub size: (u32, u32),
    pub mode: Option<ViewMode>,
    pub light: Option<usize>,
    pub camera: Option<Shot>,
    pub hide: Vec<String>,
    pub pose: Option<String>,
    pub animation: Option<String>,
    /// Número de fotogramas de la captura y fotogramas por segundo.
    pub frames: u32,
    pub fps: f32,
    pub bounce: Option<f32>,
    /// Giro de la cámara en grados tras el encuadre (90 es el perfil izquierdo).
    pub yaw: Option<f32>,
    pub wire_overlay: bool,
    pub no_grid: bool,
    /// Viento de 0 a 1 y física del pelo apagada.
    pub wind: Option<f32>,
    pub no_hair: bool,
    pub expression: Option<String>,
    /// Guarda el proyecto tras aplicar las opciones (para probar sin ventana).
    pub save_project: Option<std::path::PathBuf>,
    /// Física propia de capas de pelo: (parte del nombre, juego de valores).
    pub hair_layers: Vec<(String, String)>,
}

impl Options {
    pub fn parse(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut options = Options { size: (1024, 1024), frames: 1, fps: 12.0, ..Default::default() };
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            let mut value = || args.next().ok_or_else(|| anyhow::anyhow!("falta el valor de {arg}"));
            match arg.as_str() {
                "--shot" => options.shot = Some(value()?.into()),
                "--size" => {
                    let v = value()?;
                    let (w, h) = v.split_once('x').ok_or_else(|| anyhow::anyhow!("--size es ANCHOxALTO"))?;
                    options.size = (w.parse()?, h.parse()?);
                }
                "--mode" => {
                    options.mode = Some(match value()?.as_str() {
                        "tex" | "textured" => ViewMode::Textured,
                        "solid" => ViewMode::Solid,
                        "wire" => ViewMode::Wireframe,
                        "layers" => ViewMode::LayerColors,
                        other => anyhow::bail!("modo desconocido: {other} (tex, solid, wire, layers)"),
                    })
                }
                "--light" => options.light = Some(value()?.parse()?),
                "--camera" => {
                    let v = value()?;
                    let index: usize = v.parse().map_err(|_| anyhow::anyhow!("--camera es un número de 0 a {}", Shot::ALL.len() - 1))?;
                    options.camera = Some(*Shot::ALL.get(index).ok_or_else(|| anyhow::anyhow!("no hay cámara {index}"))?);
                }
                "--hide" => options.hide.push(value()?.to_lowercase()),
                "--pose" => options.pose = Some(value()?),
                "--anim" => options.animation = Some(value()?),
                "--frames" => options.frames = value()?.parse()?,
                "--fps" => options.fps = value()?.parse()?,
                "--bounce" => options.bounce = Some(value()?.parse()?),
                "--yaw" => options.yaw = Some(value()?.parse()?),
                "--wire-overlay" => options.wire_overlay = true,
                "--no-grid" => options.no_grid = true,
                "--wind" => options.wind = Some(value()?.parse()?),
                "--no-hair" => options.no_hair = true,
                "--expression" => options.expression = Some(value()?),
                "--save-project" => options.save_project = Some(value()?.into()),
                "--hair-layer" => {
                    let v = value()?;
                    let (layer, preset) = v.rsplit_once('=').ok_or_else(|| anyhow::anyhow!("--hair-layer es capa=valores"))?;
                    options.hair_layers.push((layer.to_lowercase(), preset.to_string()));
                }
                _ if !arg.starts_with("--") && options.model.is_none() => options.model = Some(arg.into()),
                _ => anyhow::bail!("argumento desconocido: {arg}"),
            }
        }
        Ok(options)
    }

    pub fn apply(&self, app: &mut App) {
        if let Some(path) = &self.model {
            app.open(path);
        }
        if let Some(mode) = self.mode {
            app.view.mode = mode;
        }
        if let Some(light) = self.light {
            app.light = light.min(PRESETS.len() - 1);
        }
        if let Some(wind) = self.wind {
            app.wind = wind;
        }
        app.hair_physics = !self.no_hair;
        // En las capturas, sin parpadeo automático: la expresión que se pida, o ojos abiertos.
        app.auto_blink = false;
        if let Some(expression) = &self.expression {
            app.expression = expression.clone();
        }
        for (name, preset) in &self.hair_layers {
            let preset = crate::hair::PRESETS.iter().position(|(n, _)| n == preset);
            let layers: Vec<usize> = app.model.as_ref().map_or(Vec::new(), |m| {
                m.layers.iter().enumerate().filter(|(_, l)| l.name.to_lowercase().contains(name.as_str())).map(|(i, _)| i).collect()
            });
            for layer in layers {
                app.set_hair_layer_preset(layer, preset);
            }
        }
        if let Some(pose) = &self.pose {
            app.pose = pose.clone();
        }
        app.settle();
        if let Some(animation) = &self.animation {
            app.animation = animation.clone();
        }
        if let Some(bounce) = self.bounce {
            app.bounce = bounce;
        }
        app.view.wire_overlay = self.wire_overlay;
        app.show_grid = !self.no_grid;
        if let Some(model) = &mut app.model {
            for layer in &mut model.layers {
                if self.hide.iter().any(|h| layer.name.to_lowercase().contains(h)) {
                    layer.visible = false;
                }
            }
        }
        if let Some(shot) = self.camera {
            app.frame(shot);
        }
        if let Some(yaw) = self.yaw {
            app.camera.yaw = yaw.to_radians();
        }
        if let Some(path) = &self.save_project {
            app.save_project(path);
        }
    }
}
