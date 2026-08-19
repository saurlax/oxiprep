use crate::document::{Document, Selection};
use crate::viewport::{Camera, DisplayOptions, FOV_Y};
use cadrum::DVec3;
use eframe::egui::{self, Color32, TextureId};
use eframe::egui_wgpu::RenderState;
use eframe::egui_wgpu::wgpu;
use eframe::egui_wgpu::wgpu::util::DeviceExt;
use std::num::NonZeroU64;

const HIGHLIGHT: [u8; 3] = [0xE8, 0xA3, 0x3C];
const HIGHLIGHT_LINE: [f32; 4] = [0.91, 0.64, 0.24, 1.0];
const EDGE: [f32; 4] = [0.12, 0.12, 0.12, 1.0];
const MESH_EDGE: [f32; 4] = [0.38, 0.38, 0.38, 1.0];
const VERTEX: [f32; 4] = [0.15, 0.15, 0.15, 1.0];
const VERTEX_SEL: [f32; 4] = [0.91, 0.64, 0.24, 1.0];
const GRID: [f32; 4] = [0.45, 0.45, 0.45, 0.35];
const AXIS_X: [f32; 4] = [0.70, 0.28, 0.28, 0.7];
const AXIS_Y: [f32; 4] = [0.28, 0.55, 0.28, 0.7];

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuVertex {
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuUniforms {
    view_proj: [[f32; 4]; 4],
    light_dir: [f32; 4],
    clip_plane: [f32; 4],
    clip_enabled: f32,
    _pad: [f32; 3],
}

pub struct GpuRenderer {
    render_state: RenderState,
    pipeline_fill: wgpu::RenderPipeline,
    pipeline_line: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    fill_buf: wgpu::Buffer,
    fill_count: u32,
    line_buf: wgpu::Buffer,
    line_count: u32,
    grid_buf: wgpu::Buffer,
    grid_count: u32,
    color: Option<wgpu::Texture>,
    color_view: Option<wgpu::TextureView>,
    depth: Option<wgpu::Texture>,
    depth_view: Option<wgpu::TextureView>,
    texture_id: Option<TextureId>,
    size: [u32; 2],
    scene_key: SceneKey,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SceneKey {
    models: usize,
    selection: Vec<Selection>,
    tris: usize,
    faces: bool,
    edges: bool,
    mesh: bool,
    vertices: bool,
    zoom: i32,
}

impl GpuRenderer {
    pub fn new(render_state: RenderState) -> Self {
        let device = &render_state.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport"),
            source: wgpu::ShaderSource::Wgsl(include_str!("viewport.wgsl").into()),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(std::mem::size_of::<GpuUniforms>() as u64),
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        const VERTEX_ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
            0 => Float32x3,
            1 => Float32x3,
            2 => Float32x4,
        ];
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GpuVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VERTEX_ATTRS,
        };
        let color_target = wgpu::ColorTargetState {
            format: wgpu::TextureFormat::Rgba8Unorm,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        };
        let pipeline_fill = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewport fill"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GpuVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &VERTEX_ATTRS,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_shaded"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(color_target.clone())],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(depth_state()),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let pipeline_line = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewport line"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_line"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(vertex_layout)],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_flat"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(color_target)],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: Some(depth_state()),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport uniforms"),
            size: std::mem::size_of::<GpuUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport uniforms"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });
        let fill_buf = empty_vertex_buf(device, "fill");
        let line_buf = empty_vertex_buf(device, "line");
        let grid_buf = empty_vertex_buf(device, "grid");

        Self {
            render_state,
            pipeline_fill,
            pipeline_line,
            uniform_buf,
            bind_group,
            fill_buf,
            fill_count: 0,
            line_buf,
            line_count: 0,
            grid_buf,
            grid_count: 0,
            color: None,
            color_view: None,
            depth: None,
            depth_view: None,
            texture_id: None,
            size: [0, 0],
            scene_key: SceneKey {
                models: usize::MAX,
                selection: Vec::new(),
                tris: 0,
                faces: false,
                edges: false,
                mesh: false,
                vertices: false,
                zoom: i32::MIN,
            },
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        camera: &Camera,
        document: &Document,
        display: &DisplayOptions,
        clip: Option<[f32; 4]>,
        bg: Color32,
    ) {
        let ppp = ui.ctx().pixels_per_point();
        let w = (rect.width() * ppp).round().max(1.0) as u32;
        let h = (rect.height() * ppp).round().max(1.0) as u32;
        self.ensure_target(w, h);
        self.sync_scene(document, camera, display);
        self.draw(camera, w, h, clip, bg);

        if let Some(id) = self.texture_id {
            ui.painter().image(
                id,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    fn ensure_target(&mut self, w: u32, h: u32) {
        if self.size == [w, h] && self.color.is_some() {
            return;
        }
        let device = &self.render_state.device;
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport color"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport depth"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());

        {
            let mut renderer = self.render_state.renderer.write();
            match self.texture_id {
                Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                    device,
                    &color_view,
                    wgpu::FilterMode::Linear,
                    id,
                ),
                None => {
                    self.texture_id = Some(renderer.register_native_texture(
                        device,
                        &color_view,
                        wgpu::FilterMode::Linear,
                    ));
                }
            }
        }

        self.color = Some(color);
        self.color_view = Some(color_view);
        self.depth = Some(depth);
        self.depth_view = Some(depth_view);
        self.size = [w, h];
    }

    fn sync_scene(&mut self, document: &Document, camera: &Camera, display: &DisplayOptions) {
        let tri_count: usize = document
            .models
            .iter()
            .flat_map(|m| m.bodies.iter())
            .map(|b| b.display.triangles.len())
            .sum();
        let zoom = if display.vertices {
            (camera.distance.log2() * 8.0).round() as i32
        } else {
            0
        };
        let key = SceneKey {
            models: document.models.len(),
            selection: document.selection.clone(),
            tris: tri_count,
            faces: display.faces,
            edges: display.edges,
            mesh: display.mesh,
            vertices: display.vertices,
            zoom,
        };
        let rebuild_solids = key != self.scene_key;
        self.scene_key = key;

        if rebuild_solids {
            let (fill, lines) = pack_document(document, camera, display);
            self.fill_count = fill.len() as u32;
            self.line_count = lines.len() as u32;
            let device = &self.render_state.device;
            self.fill_buf = vertex_buf(device, "fill", &fill);
            self.line_buf = vertex_buf(device, "line", &lines);
        }
        let mut grid = Vec::new();
        pack_grid(camera, &mut grid);
        self.grid_count = grid.len() as u32;
        self.grid_buf = vertex_buf(&self.render_state.device, "grid", &grid);
    }

    fn draw(&mut self, camera: &Camera, w: u32, h: u32, clip: Option<[f32; 4]>, bg: Color32) {
        let aspect = w as f32 / h.max(1) as f32;
        let (view_proj, light) = camera.view_proj(aspect);
        let (clip_plane, clip_enabled) = match clip {
            Some(p) => (p, 1.0),
            None => ([0.0, 0.0, 1.0, 0.0], 0.0),
        };
        let uniforms = GpuUniforms {
            view_proj,
            light_dir: [light[0], light[1], light[2], 0.0],
            clip_plane,
            clip_enabled,
            _pad: [0.0; 3],
        };
        self.render_state
            .queue
            .write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&uniforms));

        let Some(color_view) = self.color_view.as_ref() else {
            return;
        };
        let Some(depth_view) = self.depth_view.as_ref() else {
            return;
        };

        let mut encoder =
            self.render_state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("viewport"),
                });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg.r() as f64 / 255.0,
                            g: bg.g() as f64 / 255.0,
                            b: bg.b() as f64 / 255.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            if self.grid_count > 0 {
                pass.set_pipeline(&self.pipeline_line);
                pass.set_vertex_buffer(0, self.grid_buf.slice(..));
                pass.draw(0..self.grid_count, 0..1);
            }
            if self.fill_count > 0 {
                pass.set_pipeline(&self.pipeline_fill);
                pass.set_vertex_buffer(0, self.fill_buf.slice(..));
                pass.draw(0..self.fill_count, 0..1);
            }
            if self.line_count > 0 {
                pass.set_pipeline(&self.pipeline_line);
                pass.set_vertex_buffer(0, self.line_buf.slice(..));
                pass.draw(0..self.line_count, 0..1);
            }
        }
        self.render_state
            .queue
            .submit(std::iter::once(encoder.finish()));
    }
}

fn depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: wgpu::TextureFormat::Depth32Float,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

fn empty_vertex_buf(device: &wgpu::Device, label: &str) -> wgpu::Buffer {
    vertex_buf(
        device,
        label,
        &[GpuVertex {
            position: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
            color: [0.0; 4],
        }],
    )
}

fn vertex_buf(device: &wgpu::Device, label: &str, verts: &[GpuVertex]) -> wgpu::Buffer {
    let data: &[u8] = if verts.is_empty() {
        &[0u8; 40]
    } else {
        bytemuck::cast_slice(verts)
    };
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: data,
        usage: wgpu::BufferUsages::VERTEX,
    })
}

fn pack_document(
    document: &Document,
    camera: &Camera,
    display: &DisplayOptions,
) -> (Vec<GpuVertex>, Vec<GpuVertex>) {
    let mut fill = Vec::new();
    let mut lines = Vec::new();
    let (u_axis, v_axis) = camera_uv(camera);
    let mark = (camera.distance * 0.008).clamp(1e-4, 1e6) as f32;
    for (mi, model) in document.models.iter().enumerate() {
        for (bi, body) in model.bodies.iter().enumerate() {
            let mesh = &body.display;
            let body_hl = document.highlights_body(mi, bi);
            for (ti, (tri, rgb)) in mesh
                .triangles
                .iter()
                .zip(mesh.triangle_colors.iter())
                .enumerate()
            {
                let face_id = mesh.triangle_face_ids.get(ti).copied().unwrap_or(0);
                let selected = body_hl
                    || document.is_face_selected(mi, bi, face_id)
                    || document.is_cell_selected(mi, bi, ti as u32);
                if !display.faces && !selected {
                    continue;
                }
                let mut color = *rgb;
                if selected {
                    color = [
                        ((color[0] as u16 + HIGHLIGHT[0] as u16) / 2) as u8,
                        ((color[1] as u16 + HIGHLIGHT[1] as u16) / 2) as u8,
                        ((color[2] as u16 + HIGHLIGHT[2] as u16) / 2) as u8,
                    ];
                }
                let color = [
                    color[0] as f32 / 255.0,
                    color[1] as f32 / 255.0,
                    color[2] as f32 / 255.0,
                    1.0,
                ];
                for &idx in tri {
                    let i = idx as usize;
                    let normal = mesh.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]);
                    fill.push(GpuVertex {
                        position: mesh.positions[i],
                        normal,
                        color,
                    });
                }
            }
            if display.edges {
                if !mesh.cad_edges.is_empty() {
                    for edge in &mesh.cad_edges {
                        let color = if document.is_edge_selected(mi, bi, edge.id) {
                            HIGHLIGHT_LINE
                        } else {
                            EDGE
                        };
                        pack_polyline(&mut lines, &edge.points, color);
                    }
                } else if mesh.edges.iter().any(|p| !p[0].is_nan()) {
                    pack_cad_edges(&mut lines, &mesh.edges);
                } else if !display.mesh {
                    pack_triangle_edges(&mut lines, mesh, MESH_EDGE);
                }
            }
            if display.mesh {
                pack_triangle_edges(&mut lines, mesh, MESH_EDGE);
            }
            overlay_selected_edges(document, mi, bi, mesh, &mut lines);
            let point_only = mesh.triangles.is_empty() && mesh.cad_edges.is_empty();
            if display.vertices || point_only {
                for (index, p) in mesh.cad_vertices.iter().enumerate() {
                    let selected = body_hl || document.is_vertex_selected(mi, bi, index as u32);
                    let color = if selected { VERTEX_SEL } else { VERTEX };
                    push_cross(&mut lines, *p, u_axis, v_axis, mark, color);
                }
                if mesh.cad_vertices.is_empty() {
                    for (index, p) in mesh.positions.iter().enumerate() {
                        let selected = body_hl || document.is_node_selected(mi, bi, index as u32);
                        let color = if selected { VERTEX_SEL } else { VERTEX };
                        push_cross(&mut lines, *p, u_axis, v_axis, mark, color);
                    }
                }
            }
            overlay_selected_vertices(document, mi, bi, mesh, u_axis, v_axis, mark, &mut lines);
        }
    }
    (fill, lines)
}

fn pack_polyline(lines: &mut Vec<GpuVertex>, points: &[[f32; 3]], color: [f32; 4]) {
    for w in points.windows(2) {
        push_line(lines, w[0], w[1], color);
    }
}

fn overlay_selected_edges(
    document: &Document,
    mi: usize,
    bi: usize,
    mesh: &crate::document::DisplayMesh,
    lines: &mut Vec<GpuVertex>,
) {
    for edge in &mesh.cad_edges {
        if document.is_edge_selected(mi, bi, edge.id) {
            pack_polyline(lines, &edge.points, HIGHLIGHT_LINE);
        }
    }
    for s in &document.selection {
        if let Selection::MeshEdge { model, body, a, b } = *s {
            if model == mi && body == bi {
                let Some(pa) = mesh.positions.get(a as usize) else {
                    continue;
                };
                let Some(pb) = mesh.positions.get(b as usize) else {
                    continue;
                };
                push_line(lines, *pa, *pb, HIGHLIGHT_LINE);
            }
        }
    }
}

fn overlay_selected_vertices(
    document: &Document,
    mi: usize,
    bi: usize,
    mesh: &crate::document::DisplayMesh,
    u_axis: [f32; 3],
    v_axis: [f32; 3],
    mark: f32,
    lines: &mut Vec<GpuVertex>,
) {
    for (index, p) in mesh.cad_vertices.iter().enumerate() {
        if document.is_vertex_selected(mi, bi, index as u32) {
            push_cross(lines, *p, u_axis, v_axis, mark, VERTEX_SEL);
        }
    }
    for (index, p) in mesh.positions.iter().enumerate() {
        if document.is_node_selected(mi, bi, index as u32) {
            push_cross(lines, *p, u_axis, v_axis, mark, VERTEX_SEL);
        }
    }
}

fn push_cross(
    lines: &mut Vec<GpuVertex>,
    p: [f32; 3],
    u_axis: [f32; 3],
    v_axis: [f32; 3],
    mark: f32,
    color: [f32; 4],
) {
    let u = [u_axis[0] * mark, u_axis[1] * mark, u_axis[2] * mark];
    let v = [v_axis[0] * mark, v_axis[1] * mark, v_axis[2] * mark];
    push_line(lines, sub_f(p, u), add_f(p, u), color);
    push_line(lines, sub_f(p, v), add_f(p, v), color);
}

fn pack_cad_edges(lines: &mut Vec<GpuVertex>, edges: &[[f32; 3]]) {
    let mut prev: Option<[f32; 3]> = None;
    for p in edges {
        if p[0].is_nan() {
            prev = None;
            continue;
        }
        if let Some(a) = prev {
            push_line(lines, a, *p, EDGE);
        }
        prev = Some(*p);
    }
}

fn pack_triangle_edges(
    lines: &mut Vec<GpuVertex>,
    mesh: &crate::document::DisplayMesh,
    color: [f32; 4],
) {
    for tri in &mesh.triangles {
        let p0 = mesh.positions[tri[0] as usize];
        let p1 = mesh.positions[tri[1] as usize];
        let p2 = mesh.positions[tri[2] as usize];
        push_line(lines, p0, p1, color);
        push_line(lines, p1, p2, color);
        push_line(lines, p2, p0, color);
    }
}

fn camera_uv(camera: &Camera) -> ([f32; 3], [f32; 3]) {
    let dir = camera.view_dir();
    let v = (DVec3::Z - dir * DVec3::Z.dot(dir))
        .try_normalize()
        .unwrap_or(DVec3::Y);
    let u = v.cross(dir);
    (dvec(u), dvec(v))
}

fn add_f(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub_f(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn pack_grid(camera: &Camera, lines: &mut Vec<GpuVertex>) {
    let span = camera.distance * (FOV_Y * 0.5).tan() * 4.0;
    let step = nice_step(span / 10.0);
    if step <= 0.0 {
        return;
    }
    let origin = DVec3::new(
        (camera.target.x / step).round() * step,
        (camera.target.y / step).round() * step,
        0.0,
    );
    let n = 12i32;
    for i in -n..=n {
        let o = i as f64 * step;
        let color = if (origin.y + o).abs() < step * 0.25 {
            AXIS_X
        } else {
            GRID
        };
        let a = origin + DVec3::new(-span, o, 0.0);
        let b = origin + DVec3::new(span, o, 0.0);
        push_line(lines, dvec(a), dvec(b), color);
        let color = if (origin.x + o).abs() < step * 0.25 {
            AXIS_Y
        } else {
            GRID
        };
        let a = origin + DVec3::new(o, -span, 0.0);
        let b = origin + DVec3::new(o, span, 0.0);
        push_line(lines, dvec(a), dvec(b), color);
    }
}

fn dvec(v: DVec3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}

fn push_line(out: &mut Vec<GpuVertex>, a: [f32; 3], b: [f32; 3], color: [f32; 4]) {
    let n = [0.0, 0.0, 1.0];
    out.push(GpuVertex {
        position: a,
        normal: n,
        color,
    });
    out.push(GpuVertex {
        position: b,
        normal: n,
        color,
    });
}

fn nice_step(target: f64) -> f64 {
    if !target.is_finite() || target <= 0.0 {
        return 1.0;
    }
    let exp = target.log10().floor() as i32;
    let pow = 10f64.powi(exp);
    let m = target / pow;
    let nice = if m < 2.0 {
        1.0
    } else if m < 5.0 {
        2.0
    } else {
        5.0
    };
    nice * pow
}
