//! Dibuja el modelo por capas en tres modos: texturizado, color sólido y alambre.
//!
//! Orden, como en doa.player con las capas translúcidas de DOA3: primero lo opaco, después lo
//! que tiene transparencia (material en modo BLEND u opacidad de capa menor que 1), en el orden
//! de la paleta, con mezcla alfa y sin escribir profundidad.

use crate::lighting::LightRig;
use crate::model::{AlphaMode, Model, Vertex};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    Textured,
    Solid,
    Wireframe,
    /// Cada capa de un color distinto, para ver cómo está separado el modelo.
    LayerColors,
}

pub struct ViewSettings {
    pub mode: ViewMode,
    /// Color del modo sólido, en sRGB.
    pub solid_color: [f32; 3],
    /// Dibuja también el alambre encima en los modos texturizado y sólido.
    pub wire_overlay: bool,
    pub wire_color: [f32; 3],
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            mode: ViewMode::Textured,
            solid_color: [0.85, 0.85, 0.85],
            wire_overlay: false,
            wire_color: [0.1, 0.1, 0.1],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
    key_dir: [f32; 4],
    key_color: [f32; 4],
    fill_dir: [f32; 4],
    fill_color: [f32; 4],
    rim_color: [f32; 4],
    sky: [f32; 4],
    ground: [f32; 4],
    solid_color: [f32; 4],
    wire_color: [f32; 4],
    /// x: 1 en modo sólido, y: 1 con sombreado toon.
    params: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialUniform {
    base_color: [f32; 4],
    layer_color: [f32; 4],
    /// x: opacidad de la capa, y: corte alfa (negativo si no hay), z: 1 si el material es opaco.
    params: [f32; 4],
}

struct GpuPrimitive {
    layer: usize,
    material: usize,
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
    line_buf: wgpu::Buffer,
    line_count: u32,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

pub struct SceneRenderer {
    globals_buf: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    material_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white: wgpu::TextureView,
    /// Índices: [opaco, translúcido] x [dos caras, solo frontal].
    pipelines: [[wgpu::RenderPipeline; 2]; 2],
    wire_pipeline: wgpu::RenderPipeline,
    textures: Vec<wgpu::TextureView>,
    primitives: Vec<GpuPrimitive>,
}

impl SceneRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[uniform_entry(0)],
        });
        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() }],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[
                uniform_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = upload_texture(device, queue, 1, 1, &[255, 255, 255, 255]);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene"),
            source: wgpu::ShaderSource::Wgsl(SCENE_WGSL.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[&globals_layout, &material_layout],
            push_constant_ranges: &[],
        });
        let make = |blend: bool, cull: bool, lines: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("scene"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: if lines { "vs_wire" } else { "vs_main" },
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: if lines { "fs_wire" } else { "fs_main" },
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: blend.then_some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: if lines { wgpu::PrimitiveTopology::LineList } else { wgpu::PrimitiveTopology::TriangleList },
                    cull_mode: cull.then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: !blend,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let pipelines = [[make(false, false, false), make(false, true, false)], [make(true, false, false), make(true, true, false)]];
        let wire_pipeline = make(false, false, true);

        Self {
            globals_buf,
            globals_bind,
            material_layout,
            sampler,
            white,
            pipelines,
            wire_pipeline,
            textures: Vec::new(),
            primitives: Vec::new(),
        }
    }

    /// Sube a la GPU un modelo recién cargado, sustituyendo al anterior.
    pub fn set_model(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, model: &Model) {
        self.textures = model
            .textures
            .iter()
            .map(|t| upload_texture(device, queue, t.width, t.height, &t.rgba))
            .collect();
        self.primitives.clear();
        for (layer_index, layer) in model.layers.iter().enumerate() {
            for primitive in &layer.primitives {
                let material = &model.materials[primitive.material];
                let view = material.texture.and_then(|t| self.textures.get(t)).unwrap_or(&self.white);
                let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("material"),
                    size: std::mem::size_of::<MaterialUniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("material"),
                    layout: &self.material_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: uniform_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    ],
                });
                let lines = edge_lines(&primitive.indices);
                self.primitives.push(GpuPrimitive {
                    layer: layer_index,
                    material: primitive.material,
                    vertex_buf: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("vertices"),
                        contents: bytemuck::cast_slice(&primitive.vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
                    index_buf: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("indices"),
                        contents: bytemuck::cast_slice(&primitive.indices),
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                    index_count: primitive.indices.len() as u32,
                    line_buf: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("lines"),
                        contents: bytemuck::cast_slice(&lines),
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                    line_count: lines.len() as u32,
                    uniform_buf,
                    bind_group,
                });
            }
        }
    }

    pub fn update(&self, queue: &wgpu::Queue, view_proj: Mat4, camera_pos: Vec3, light: &LightRig, view: &ViewSettings, model: Option<&Model>) {
        let v4 = |v: Vec3| [v.x, v.y, v.z, 0.0];
        let srgb = |c: [f32; 3]| [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]), 1.0];
        let globals = Globals {
            view_proj: view_proj.to_cols_array_2d(),
            camera_pos: [camera_pos.x, camera_pos.y, camera_pos.z, 1.0],
            key_dir: v4(light.key_dir.normalize()),
            key_color: v4(light.key_color),
            fill_dir: v4(light.fill_dir.normalize()),
            fill_color: v4(light.fill_color),
            rim_color: v4(light.rim_color),
            sky: v4(light.sky),
            ground: v4(light.ground),
            solid_color: srgb(view.solid_color),
            wire_color: srgb(view.wire_color),
            params: [match view.mode { ViewMode::Solid => 1.0, ViewMode::LayerColors => 2.0, _ => 0.0 }, light.toon as u32 as f32, 0.0, 0.0],
        };
        queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));
        let Some(model) = model else { return };
        for p in &self.primitives {
            let material = &model.materials[p.material];
            let cutoff = match material.alpha {
                AlphaMode::Mask(c) => c,
                AlphaMode::Blend => 0.004,
                AlphaMode::Opaque => -1.0,
            };
            let uniform = MaterialUniform {
                base_color: material.base_color,
                layer_color: layer_color(p.layer),
                params: [model.effective_opacity(p.layer), cutoff, (material.alpha == AlphaMode::Opaque) as u32 as f32, 0.0],
            };
            queue.write_buffer(&p.uniform_buf, 0, bytemuck::bytes_of(&uniform));
        }
    }

    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, view: &ViewSettings, model: &Model) {
        pass.set_bind_group(0, &self.globals_bind, &[]);
        let visible: Vec<&GpuPrimitive> = self.primitives.iter().filter(|p| model.effective_visible(p.layer)).collect();
        if view.mode != ViewMode::Wireframe {
            let translucent = |p: &GpuPrimitive| {
                model.materials[p.material].alpha == AlphaMode::Blend || model.effective_opacity(p.layer) < 0.999
            };
            // Primero lo opaco, después lo translúcido en el orden de la paleta.
            for pass_translucent in [false, true] {
                for p in visible.iter().filter(|p| translucent(p) == pass_translucent) {
                    let cull = !model.materials[p.material].double_sided;
                    pass.set_pipeline(&self.pipelines[pass_translucent as usize][cull as usize]);
                    pass.set_bind_group(1, &p.bind_group, &[]);
                    pass.set_vertex_buffer(0, p.vertex_buf.slice(..));
                    pass.set_index_buffer(p.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..p.index_count, 0, 0..1);
                }
            }
        }
        if view.mode == ViewMode::Wireframe || view.wire_overlay {
            pass.set_pipeline(&self.wire_pipeline);
            for p in &visible {
                pass.set_bind_group(1, &p.bind_group, &[]);
                pass.set_vertex_buffer(0, p.vertex_buf.slice(..));
                pass.set_index_buffer(p.line_buf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..p.line_count, 0, 0..1);
            }
        }
    }
}

/// Un color lineal distinto para cada capa: tonos repartidos por la razón áurea.
fn layer_color(index: usize) -> [f32; 4] {
    let hue = (index as f32 * 0.618_034).fract() * 6.0;
    let x = 1.0 - (hue % 2.0 - 1.0).abs();
    let (r, g, b) = match hue as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    let mix = |c: f32| srgb_to_linear(0.25 + 0.7 * c);
    [mix(r), mix(g), mix(b), 1.0]
}

/// Cada arista una sola vez, como pares de índices.
fn edge_lines(indices: &[u32]) -> Vec<u32> {
    let mut edges = std::collections::HashSet::new();
    let mut lines = Vec::new();
    for tri in indices.chunks_exact(3) {
        for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            if edges.insert((a.min(b), a.max(b))) {
                lines.extend([a, b]);
            }
        }
    }
    lines
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn upload_texture(device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32, rgba: &[u8]) -> wgpu::TextureView {
    let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::ImageCopyTexture { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        rgba,
        wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(4 * width), rows_per_image: Some(height) },
        size,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

const SCENE_WGSL: &str = r#"
struct Globals {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    key_dir: vec4<f32>,
    key_color: vec4<f32>,
    fill_dir: vec4<f32>,
    fill_color: vec4<f32>,
    rim_color: vec4<f32>,
    sky: vec4<f32>,
    ground: vec4<f32>,
    solid_color: vec4<f32>,
    wire_color: vec4<f32>,
    params: vec4<f32>,
}
struct Material {
    base_color: vec4<f32>,
    layer_color: vec4<f32>,
    params: vec4<f32>,
}
@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var<uniform> m: Material;
@group(1) @binding(1) var tex: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;

struct VertIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}
struct VertOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

fn transform(v: VertIn) -> VertOut {
    var out: VertOut;
    out.clip = g.view_proj * vec4<f32>(v.pos, 1.0);
    out.world = v.pos;
    out.normal = v.normal;
    out.uv = v.uv;
    return out;
}

@vertex
fn vs_main(v: VertIn) -> VertOut {
    return transform(v);
}

// El alambre se acerca un poco a la cámara para no pelearse con las caras que tiene debajo.
@vertex
fn vs_wire(v: VertIn) -> VertOut {
    var out = transform(v);
    out.clip.z = out.clip.z - 0.0004 * out.clip.w;
    return out;
}

@fragment
fn fs_wire(v: VertOut) -> @location(0) vec4<f32> {
    return vec4<f32>(g.wire_color.rgb, 1.0);
}

fn diffuse(ndl: f32, toon: bool) -> f32 {
    if toon {
        return smoothstep(0.18, 0.24, ndl);
    }
    return max(ndl, 0.0);
}

@fragment
fn fs_main(v: VertOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let sampled = textureSample(tex, samp, v.uv);
    var albedo = m.base_color * sampled;
    if g.params.x > 1.5 {
        albedo = vec4<f32>(m.layer_color.rgb, albedo.a);
    } else if g.params.x > 0.5 {
        // Modo sólido: un solo color, pero se conserva el alfa para los recortes (pelo, pestañas).
        albedo = vec4<f32>(g.solid_color.rgb, albedo.a);
    }
    if m.params.z > 0.5 {
        albedo.a = 1.0;
    }
    if albedo.a < m.params.y {
        discard;
    }

    var n = normalize(v.normal);
    if !front {
        n = -n;
    }
    let to_eye = normalize(g.camera_pos.xyz - v.world);
    let toon = g.params.y > 0.5;
    let hemi = mix(g.ground.rgb, g.sky.rgb, n.y * 0.5 + 0.5);
    let key = g.key_color.rgb * diffuse(dot(n, g.key_dir.xyz), toon);
    let fill = g.fill_color.rgb * max(dot(n, g.fill_dir.xyz), 0.0);
    var rim_amount = pow(1.0 - max(dot(n, to_eye), 0.0), 4.0);
    if toon {
        rim_amount = step(0.5, rim_amount);
    }
    let lit = albedo.rgb * (hemi + key + fill) + g.rim_color.rgb * rim_amount;
    return vec4<f32>(lit, albedo.a * m.params.x);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quad_has_five_edges() {
        assert_eq!(edge_lines(&[0, 1, 2, 0, 2, 3]).len(), 10);
    }
}
