mod app;
mod export_glb;
mod hair;
mod lighting;
mod model;
mod physics;
mod pose;
mod renderer;
mod rig;
mod segment;

use app::{App, Options};
use renderer::camera::Shot;
use renderer::scene::ViewMode;
use renderer::RenderContext;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

struct Handler {
    options: Options,
    window: Option<Arc<Window>>,
    app: Option<App>,
    mouse_left_down: bool,
    mouse_right_down: bool,
    last_pos: (f64, f64),
    /// Píxeles que tapan los paneles a izquierda y derecha, del último fotograma.
    insets: (f32, f32),
    selected_layer: Option<usize>,
    last_frame: std::time::Instant,
}

impl ApplicationHandler for Handler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.app.is_some() {
            return;
        }
        let attributes = winit::platform::wayland::WindowAttributesExtWayland::with_name(Window::default_attributes(), "anime2waifu3d", "anime2waifu3d");
        let attributes = winit::platform::x11::WindowAttributesExtX11::with_name(attributes, "anime2waifu3d", "anime2waifu3d");
        let window = Arc::new(
            event_loop
                .create_window(
                    attributes
                        .with_title("anime2waifu3d")
                        // Una captura desde la línea de órdenes no necesita ventana visible.
                        .with_visible(self.options.shot.is_none())
                        .with_inner_size(winit::dpi::LogicalSize::new(1440u32, 900u32)),
                )
                .expect("no se pudo crear la ventana"),
        );
        let ctx = pollster::block_on(RenderContext::new(window.clone())).expect("no se pudo iniciar la GPU");
        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(egui_ctx.clone(), egui::ViewportId::ROOT, &*window, Some(window.scale_factor() as f32), None, Some(2048));
        let mut app = App::new(ctx, egui_ctx, egui_state);

        if let Some(path) = self.options.shot.clone() {
            let (w, h) = self.options.size;
            app.camera.set_aspect(w as f32 / h as f32);
            app.camera.set_insets(0.0, 0.0, w as f32, h as f32);
            self.options.apply(&mut app);
            // Con --frames N, N capturas seguidas (archivo_00.png…) avanzando la animación.
            let result = if self.options.frames > 1 {
                let stem = path.with_extension("");
                (0..self.options.frames).try_for_each(|k| {
                    if k > 0 {
                        app.tick(1.0 / self.options.fps);
                    }
                    app.screenshot(&std::path::PathBuf::from(format!("{}_{k:02}.png", stem.display())), w, h)
                })
            } else {
                app.screenshot(&path, w, h)
            };
            match result {
                Ok(()) => println!("{} -> {}", app.status, path.display()),
                Err(error) => {
                    eprintln!("Error en la captura: {error:#}");
                    std::process::exit(1);
                }
            }
            if app.status.starts_with("Error") {
                eprintln!("{}", app.status);
                std::process::exit(1);
            }
            event_loop.exit();
            return;
        }
        self.options.apply(&mut app);
        self.window = Some(window);
        self.app = Some(app);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(app), Some(window)) = (&mut self.app, &self.window) else { return };
        let consumed = app.egui_state.on_window_event(window, &event).consumed;
        match &event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => app.resize(size.width, size.height),
            WindowEvent::DroppedFile(path) => app.open(path),
            WindowEvent::MouseInput { button, state, .. } => {
                let down = *state == ElementState::Pressed && !consumed;
                match button {
                    MouseButton::Left => self.mouse_left_down = down,
                    MouseButton::Right | MouseButton::Middle => self.mouse_right_down = down,
                    _ => {}
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let delta = glam::Vec2::new((position.x - self.last_pos.0) as f32, (position.y - self.last_pos.1) as f32);
                if self.mouse_left_down {
                    app.camera.orbit(delta);
                }
                if self.mouse_right_down {
                    app.camera.pan(delta);
                }
                self.last_pos = (position.x, position.y);
            }
            WindowEvent::MouseWheel { delta, .. } if !consumed => {
                let scroll = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 * 0.01,
                };
                app.camera.zoom(scroll);
            }
            WindowEvent::RedrawRequested => {
                let now = std::time::Instant::now();
                app.tick(now.duration_since(self.last_frame).as_secs_f32());
                self.last_frame = now;
                render_frame(app, window, &mut self.insets, &mut self.selected_layer);
                window.request_redraw();
            }
            _ => {}
        }
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.app = None;
        self.window = None;
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

fn render_frame(app: &mut App, window: &Window, insets: &mut (f32, f32), selected: &mut Option<usize>) {
    let raw_input = app.egui_state.take_egui_input(window);
    let egui_ctx = app.egui_ctx.clone();
    let mut open_dialog = false;
    let mut frame_shot = None;
    let output = egui_ctx.run(raw_input, |ctx| {
        let left = egui::SidePanel::left("vista").default_width(230.0).show(ctx, |ui| {
            view_panel(ui, app, &mut open_dialog, &mut frame_shot);
        });
        let right = egui::SidePanel::right("capas").default_width(260.0).show(ctx, |ui| {
            layers_panel(ui, app, selected);
        });
        egui::TopBottomPanel::bottom("estado").show(ctx, |ui| ui.label(&app.status));
        let ppp = ctx.pixels_per_point();
        *insets = (left.response.rect.width() * ppp, right.response.rect.width() * ppp);
    });
    if open_dialog {
        if let Some(path) = rfd::FileDialog::new().add_filter("Modelos 3D", &["glb", "gltf", "vrm"]).pick_file() {
            app.open(&path);
        }
    }
    let size = window.inner_size();
    app.camera.set_insets(insets.0, insets.1, size.width as f32, size.height as f32);
    if let Some(shot) = frame_shot {
        app.frame(shot);
    }

    let surface_texture = match app.ctx.surface.get_current_texture() {
        Ok(t) => t,
        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
            let (w, h) = (app.ctx.surface_config.width, app.ctx.surface_config.height);
            app.ctx.resize(w, h);
            return;
        }
        Err(e) => {
            log::error!("superficie: {e}");
            return;
        }
    };
    let view = surface_texture.texture.create_view(&Default::default());
    let mut encoder = app.ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("fotograma") });
    app.draw_3d(&mut encoder, &view, &app.ctx.depth_view);

    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [app.ctx.surface_config.width, app.ctx.surface_config.height],
        pixels_per_point: window.scale_factor() as f32,
    };
    for (id, delta) in &output.textures_delta.set {
        app.egui_renderer.update_texture(&app.ctx.device, &app.ctx.queue, *id, delta);
    }
    let tris = app.egui_ctx.tessellate(output.shapes, output.pixels_per_point);
    app.egui_renderer.update_buffers(&app.ctx.device, &app.ctx.queue, &mut encoder, &tris, &screen);
    {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let mut pass = pass.forget_lifetime();
        app.egui_renderer.render(&mut pass, &tris, &screen);
    }
    for id in &output.textures_delta.free {
        app.egui_renderer.free_texture(id);
    }
    app.egui_state.handle_platform_output(window, output.platform_output);
    app.ctx.queue.submit([encoder.finish()]);
    surface_texture.present();
}

fn view_panel(ui: &mut egui::Ui, app: &mut App, open_dialog: &mut bool, frame_shot: &mut Option<Shot>) {
    if ui.button("Abrir modelo…").clicked() {
        *open_dialog = true;
    }
    ui.separator();
    ui.heading("Vista");
    ui.horizontal(|ui| {
        ui.selectable_value(&mut app.view.mode, ViewMode::Textured, "Texturizado");
        ui.selectable_value(&mut app.view.mode, ViewMode::Solid, "Sólido");
        ui.selectable_value(&mut app.view.mode, ViewMode::Wireframe, "Alambre");
    });
    ui.selectable_value(&mut app.view.mode, ViewMode::LayerColors, "Colores por capa");
    if app.view.mode == ViewMode::Solid {
        ui.horizontal(|ui| {
            ui.label("Color");
            ui.color_edit_button_rgb(&mut app.view.solid_color);
        });
    }
    if app.view.mode != ViewMode::Wireframe {
        ui.checkbox(&mut app.view.wire_overlay, "Alambre encima");
    }
    ui.checkbox(&mut app.show_grid, "Rejilla");
    ui.separator();
    ui.heading("Iluminación");
    for (i, preset) in lighting::PRESETS.iter().enumerate() {
        ui.radio_value(&mut app.light, i, preset.name);
    }
    if app.model.as_ref().is_some_and(|m| m.skeleton.is_some()) {
        ui.separator();
        ui.heading("Pose");
        let before = app.pose.clone();
        egui::ComboBox::from_id_salt("pose").selected_text(app.pose.as_str()).show_ui(ui, |ui| {
            for name in pose::PRESETS {
                ui.selectable_value(&mut app.pose, name.to_string(), *name);
            }
        });
        if app.pose != before {
            app.apply_pose();
        }
        if app.has_hair() {
            ui.heading("Pelo y ropa");
            if ui.checkbox(&mut app.hair_physics, "Física del pelo y la ropa").changed() {
                app.apply_pose();
            }
            let name = hair::PRESETS[app.hair_preset].0;
            egui::ComboBox::from_id_salt("pelo").selected_text(format!("Pelo: {name}")).show_ui(ui, |ui| {
                for (i, (name, _)) in hair::PRESETS.iter().enumerate() {
                    ui.selectable_value(&mut app.hair_preset, i, *name);
                }
            });
            let name = hair::PRESETS[app.cloth_preset].0;
            egui::ComboBox::from_id_salt("ropa").selected_text(format!("Ropa: {name}")).show_ui(ui, |ui| {
                for (i, (name, _)) in hair::PRESETS.iter().enumerate() {
                    ui.selectable_value(&mut app.cloth_preset, i, *name);
                }
            });
            ui.add(egui::Slider::new(&mut app.wind, 0.0..=1.0).text("Viento"));
        }
        ui.heading("Animación");
        egui::ComboBox::from_id_salt("animacion").selected_text(app.animation.as_str()).show_ui(ui, |ui| {
            for name in ["ninguna", "botar"] {
                ui.selectable_value(&mut app.animation, name.to_string(), name);
            }
        });
        ui.add(egui::Slider::new(&mut app.bounce, 0.0..=3.0).text("Rebote del pecho"));
        let preset_name = physics::PRESETS[app.spring_preset].0;
        egui::ComboBox::from_id_salt("muelle").selected_text(preset_name).show_ui(ui, |ui| {
            for (i, (name, _)) in physics::PRESETS.iter().enumerate() {
                ui.selectable_value(&mut app.spring_preset, i, *name);
            }
        });
    }
    ui.separator();
    ui.heading("Cámara");
    for shot in Shot::ALL {
        if ui.button(shot.name()).clicked() {
            *frame_shot = Some(shot);
        }
    }
}

/// Paleta de capas mínima: ojo, nombre, Alt+clic en el ojo para ver solo esa capa, y opacidad de la
/// capa seleccionada. La paleta completa al estilo Photoshop llegará con la interfaz.
fn layers_panel(ui: &mut egui::Ui, app: &mut App, selected: &mut Option<usize>) {
    ui.heading("Capas");
    let Some(model) = &mut app.model else {
        ui.label("Sin modelo");
        return;
    };
    ui.horizontal(|ui| {
        if ui.button("Mostrar todas").clicked() {
            model.layers.iter_mut().for_each(|l| l.visible = true);
        }
        if ui.button("Ocultar todas").clicked() {
            model.layers.iter_mut().for_each(|l| l.visible = false);
        }
    });
    let alt = ui.input(|i| i.modifiers.alt);
    let mut solo = None;
    egui::ScrollArea::vertical().max_height(ui.available_height() - 60.0).show(ui, |ui| {
        let mut hidden_parent_depth: Option<usize> = None;
        for index in 0..model.layers.len() {
            let depth = model.layers[index].depth;
            // Las hijas de un grupo plegado no se listan.
            if let Some(hidden) = hidden_parent_depth {
                if depth > hidden {
                    continue;
                }
                hidden_parent_depth = None;
            }
            let has_children = model.layers.get(index + 1).is_some_and(|next| next.parent == Some(index));
            ui.horizontal(|ui| {
                ui.add_space(depth as f32 * 12.0);
                let layer = &mut model.layers[index];
                let eye = if layer.visible { "👁" } else { "  " };
                if ui.add(egui::Button::new(eye).min_size(egui::vec2(22.0, 0.0))).on_hover_text("Alt+clic: solo esta capa").clicked() {
                    if alt {
                        solo = Some(index);
                    } else {
                        layer.visible = !layer.visible;
                    }
                }
                if has_children {
                    let arrow = if layer.expanded { "⏷" } else { "⏵" };
                    if ui.small_button(arrow).clicked() {
                        layer.expanded = !layer.expanded;
                    }
                }
                if ui.selectable_label(*selected == Some(index), &layer.name).clicked() {
                    *selected = Some(index);
                }
                if !layer.expanded && has_children {
                    hidden_parent_depth = Some(layer.depth);
                }
            });
        }
    });
    if let Some(index) = solo {
        // Como en Photoshop: Alt+clic deja visible solo esa capa (y sus antecesoras y
        // descendientes); otro Alt+clic sobre la misma lo deshace.
        let already_solo = model.layers.iter().enumerate().all(|(i, l)| {
            l.visible == (i == index || model.is_ancestor(i, index) || model.is_ancestor(index, i))
        });
        let keep: Vec<bool> = (0..model.layers.len())
            .map(|i| already_solo || i == index || model.is_ancestor(i, index) || model.is_ancestor(index, i))
            .collect();
        for (layer, keep) in model.layers.iter_mut().zip(keep) {
            layer.visible = keep;
        }
    }
    if let Some(layer) = selected.and_then(|i| model.layers.get_mut(i)) {
        ui.separator();
        ui.label(&layer.name);
        ui.add(egui::Slider::new(&mut layer.opacity, 0.0..=1.0).text("Opacidad"));
    }
    // Física propia de un mechón (o de todos los de un grupo).
    if let Some(index) = *selected {
        if let Some(current) = app.hair_layer_preset(index) {
            let mut choice = current;
            egui::ComboBox::from_id_salt("fisica-capa")
                .selected_text(format!("Física: {}", choice.map_or("general", |i| hair::PRESETS[i].0)))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut choice, None, "general");
                    for (i, (name, _)) in hair::PRESETS.iter().enumerate() {
                        ui.selectable_value(&mut choice, Some(i), *name);
                    }
                });
            if choice != current {
                app.set_hair_layer_preset(index, choice);
            }
        }
    }
}

/// `anime2waifu3d segment ENTRADA SALIDA.glb [--clusters N]`: separa una malla de una pieza en
/// capas por color y escribe un GLB con una capa por grupo.
fn run_segment(args: &[String]) -> anyhow::Result<()> {
    let (mut input, mut output, mut options) = (None, None, segment::Options::default());
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--clusters" => options.clusters = it.next().ok_or_else(|| anyhow::anyhow!("falta N"))?.parse()?,
            "--smooth" => options.smooth_passes = it.next().ok_or_else(|| anyhow::anyhow!("falta N"))?.parse()?,
            "--merge" => options.merge_delta = it.next().ok_or_else(|| anyhow::anyhow!("falta ΔE"))?.parse()?,
            "--min-component" => options.min_component = it.next().ok_or_else(|| anyhow::anyhow!("falta F"))?.parse()?,
            _ if input.is_none() => input = Some(std::path::PathBuf::from(arg)),
            _ if output.is_none() => output = Some(std::path::PathBuf::from(arg)),
            _ => anyhow::bail!("argumento desconocido: {arg}"),
        }
    }
    let (input, output) = (input.ok_or_else(|| anyhow::anyhow!("falta la entrada"))?, output.ok_or_else(|| anyhow::anyhow!("falta la salida"))?);
    let start = std::time::Instant::now();
    let model = model::Model::load(&input)?;
    let segments = segment::segment(&model, &options);
    for s in &segments {
        println!("{:<11} {:<24} {:5.1} %  #{:02x}{:02x}{:02x}", s.class, s.name, s.area_fraction * 100.0, s.color[0], s.color[1], s.color[2]);
    }
    let result = segment::into_model(model, &segments);
    export_glb::write(&result, &output)?;
    println!("{} capas -> {} en {:.1} s", segments.len(), output.display(), start.elapsed().as_secs_f32());
    Ok(())
}

/// `anime2waifu3d rig OBJETIVO BASE.vrm SALIDA.glb`: pone el esqueleto del modelo base al
/// objetivo (una malla separada en capas con `segment`) y lo deja en la pose de reposo del base.
fn run_rig(args: &[String]) -> anyhow::Result<()> {
    // --prop "capa 1,capa 2=rightHand": esas capas son un objeto rígido con ese hueso humanoide.
    let mut props = Vec::new();
    let mut files = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg == "--prop" {
            let value = it.next().ok_or_else(|| anyhow::anyhow!("falta el valor de --prop"))?;
            let (layers, bone) = value.rsplit_once('=').ok_or_else(|| anyhow::anyhow!("--prop es \"capa,capa=hueso\""))?;
            props.push(rig::Prop { layers: layers.split(',').map(|s| s.trim().to_string()).collect(), bone: bone.trim().to_string() });
        } else {
            files.push(arg.clone());
        }
    }
    let [target, base, output] = files.as_slice() else { anyhow::bail!("uso: rig OBJETIVO BASE.vrm SALIDA.glb [--prop \"capas=hueso\"]...") };
    let start = std::time::Instant::now();
    let mut model = model::Model::load(std::path::Path::new(target))?;
    let base = model::Model::load(std::path::Path::new(base))?;
    let report = rig::rig(&mut model, &base, &props)?;
    println!("escala {:.3}", report.scale);
    for (bone, before, after) in &report.limbs {
        println!("{bone:<15} encaje {:.4} -> {:.4}", before, after);
    }
    print_hair(&hair::rig_hair(&mut model)?);
    print_hair(&hair::rig_skirt(&mut model)?);
    export_glb::write(&model, std::path::Path::new(output))?;
    println!("-> {output} en {:.1} s", start.elapsed().as_secs_f32());
    Ok(())
}

fn print_hair(report: &hair::Report) {
    for strand in &report.strands {
        println!("{:<22} {} huesos, {} vértices", strand.name, strand.bones, strand.vertices);
    }
}

/// `anime2waifu3d hair ENTRADA.glb SALIDA.glb`: pone cadenas de pelo y de falda a un modelo ya
/// riggeado (lo hace también `rig`).
fn run_hair(args: &[String]) -> anyhow::Result<()> {
    let [input, output] = args else { anyhow::bail!("uso: hair ENTRADA.glb SALIDA.glb") };
    let mut model = model::Model::load(std::path::Path::new(input))?;
    print_hair(&hair::rig_hair(&mut model)?);
    print_hair(&hair::rig_skirt(&mut model)?);
    export_glb::write(&model, std::path::Path::new(output))?;
    println!("-> {output}");
    Ok(())
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("rig") {
        if let Err(error) = run_rig(&args[1..]) {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return;
    }
    if args.first().map(String::as_str) == Some("hair") {
        if let Err(error) = run_hair(&args[1..]) {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return;
    }
    if args.first().map(String::as_str) == Some("segment") {
        if let Err(error) = run_segment(&args[1..]) {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return;
    }
    let options = match Options::parse(args.into_iter()) {
        Ok(o) => o,
        Err(error) => {
            eprintln!("{error:#}");
            eprintln!(
                "Uso: anime2waifu3d [modelo.glb|.gltf|.vrm] [--shot salida.png] [--size 1024x1024] \
                 [--mode tex|solid|wire|layers] [--light N] [--camera N] [--hide nombre]... [--pose nombre] [--anim botar] [--frames N] [--fps F] [--bounce 0..3] [--wind 0..1] [--no-hair] [--hair-layer capa=valores] [--wire-overlay] [--no-grid]"
            );
            std::process::exit(2);
        }
    };
    let event_loop = EventLoop::new().expect("no se pudo crear el bucle de eventos");
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    let mut handler = Handler {
        options,
        window: None,
        app: None,
        mouse_left_down: false,
        mouse_right_down: false,
        last_pos: (0.0, 0.0),
        insets: (0.0, 0.0),
        selected_layer: None,
        last_frame: std::time::Instant::now(),
    };
    event_loop.run_app(&mut handler).expect("fallo en el bucle de eventos");
}
