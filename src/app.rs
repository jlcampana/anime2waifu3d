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

pub struct App {
    pub ctx: RenderContext,
    pub egui_ctx: egui::Context,
    pub egui_state: egui_winit::State,
    pub egui_renderer: egui_wgpu::Renderer,
    pub camera: Camera,
    pub grid: GridRenderer,
    pub scene: SceneRenderer,
    pub model: Option<Model>,
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
    pub wind: f32,
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
            wind: 0.0,
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
        self.model = Some(model);
        self.settle();
        self.frame(Shot::Full);
        Ok(())
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
        let mut vertices = crate::pose::skin_model(model, &local);
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
        let wind = glam::Vec3::new(0.6, 0.0, -1.0).normalize() * self.wind * gust * height * 0.004;
        let params = crate::hair::PRESETS[self.hair_preset.min(crate::hair::PRESETS.len() - 1)].1;
        let (Some(model), Some(hair)) = (&self.model, &mut self.hair) else { return };
        let Some(skeleton) = &model.skeleton else { return };
        let skin = crate::pose::skin_matrices(skeleton, &local);
        hair.step(skeleton, &skin, offset, &params, wind);
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
        if !animated && !hair_active && self.springs.iter().all(|s| s.x.abs() < 1e-4) {
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
        if animated || moving {
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
    }
}
