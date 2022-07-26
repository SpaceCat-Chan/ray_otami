use std::{
    collections::HashMap,
    ops::DerefMut,
    sync::{Arc, Mutex},
    thread::spawn,
};

use bytemuck::{Pod, Zeroable};
use cgmath::prelude::*;
use rand::RngCore;
use rand_distr::Distribution;
use rayon::iter::{ParallelBridge, ParallelIterator};
use serde::{Deserialize, Serialize};
use wgpu::util::DeviceExt;

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct Material {
    pub color: cgmath::Vector3<f64>,
    pub emitance: cgmath::Vector3<f64>,
    pub metalness: f64,
    pub roughness: f64,
}

impl Material {
    fn to_raw(&self) -> RawMaterial {
        RawMaterial {
            color: [
                self.color.x as f32,
                self.color.y as f32,
                self.color.z as f32,
                0.0,
            ],
            emitance: [
                self.emitance.x as f32,
                self.emitance.y as f32,
                self.emitance.z as f32,
                0.0,
            ],
            mrxx: [self.metalness as f32, self.roughness as f32, 0.0, 0.0],
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum Object {
    Sphere {
        center: cgmath::Point3<f64>,
        radius: f64,
        material: String,
    },
    Box {
        lower_corner: cgmath::Point3<f64>,
        upper_corner: cgmath::Point3<f64>,
        material: String,
    },
    PosModulo(Box<Object>, f64),
    Inv(Box<Object>),
    Min(Box<Object>, Box<Object>),
    Max(Box<Object>, Box<Object>),
    Torus {
        major_radius: f64,
        minor_radius: f64,
        center: cgmath::Point3<f64>,
        material: String,
    },
}

impl Object {
    fn to_raw(
        &self,
        material_map: &HashMap<String, u32>,
        is_rendered: bool,
        is_refered_to: bool,
        current_refer_count: u32,
    ) -> (Vec<RawObject>, u32) {
        match self {
            Object::Sphere {
                center,
                radius,
                material,
            } => (
                vec![RawObject {
                    mrrt: [
                        material_map[material],
                        is_refered_to as _,
                        is_rendered as _,
                        0,
                    ],
                    args1: [
                        center.x as f32,
                        center.y as f32,
                        center.z as f32,
                        *radius as f32,
                    ],
                    args2: [0.0, 0.0, 0.0, 0.0],
                }],
                0,
            ),
            Object::Box {
                lower_corner,
                upper_corner,
                material,
            } => (
                vec![RawObject {
                    mrrt: [
                        material_map[material],
                        is_refered_to as _,
                        is_rendered as _,
                        1,
                    ],
                    args1: [
                        lower_corner.x as f32,
                        lower_corner.y as f32,
                        lower_corner.z as f32,
                        0.0,
                    ],
                    args2: [
                        upper_corner.x as f32,
                        upper_corner.y as f32,
                        upper_corner.z as f32,
                        0.0,
                    ],
                }],
                0,
            ),
            Object::PosModulo(_, _) => (
                // this one can't be implemented on the gpu just yet
                vec![RawObject {
                    mrrt: [0, 0, 0, 2],
                    args1: [0.0, 0.0, 0.0, 0.0],
                    args2: [0.0, 0.0, 0.0, 0.0],
                }],
                0,
            ),
            Object::Inv(inverted) => {
                let (mut inner, used_refers) =
                    inverted.to_raw(material_map, false, true, current_refer_count);
                inner.push(RawObject {
                    mrrt: [0, is_refered_to as _, is_rendered as _, 3],
                    args1: [(current_refer_count + used_refers) as f32, 0.0, 0.0, 0.0],
                    args2: [0.0, 0.0, 0.0, 0.0],
                });
                (inner, used_refers + 1)
            }
            Object::Min(a, b) => {
                let (mut a_inner, used_refers) =
                    a.to_raw(material_map, false, true, current_refer_count);
                let current_refer_count = current_refer_count + used_refers;
                let (b_inner, used_refers_b) =
                    a.to_raw(material_map, false, true, current_refer_count);
                a_inner.extend(b_inner.into_iter());
                let total_used_refers = used_refers + used_refers_b;
                a_inner.push(RawObject {
                    mrrt: [0, is_refered_to as _, is_rendered as _, 5],
                    args1: [
                        (current_refer_count) as _,
                        (current_refer_count + used_refers_b) as _,
                        0.0,
                        0.0,
                    ],
                    args2: [0.0, 0.0, 0.0, 0.0],
                });
                (a_inner, total_used_refers + 2)
            }
            Object::Max(a, b) => {
                let (mut a_inner, used_refers) =
                    a.to_raw(material_map, false, true, current_refer_count);
                let current_refer_count = current_refer_count + used_refers;
                let (b_inner, used_refers_b) =
                    a.to_raw(material_map, false, true, current_refer_count);
                a_inner.extend(b_inner.into_iter());
                let total_used_refers = used_refers + used_refers_b;
                a_inner.push(RawObject {
                    mrrt: [0, is_refered_to as _, is_rendered as _, 4],
                    args1: [
                        (current_refer_count) as _,
                        (current_refer_count + used_refers_b) as _,
                        0.0,
                        0.0,
                    ],
                    args2: [0.0, 0.0, 0.0, 0.0],
                });
                (a_inner, total_used_refers + 2)
            }
            Object::Torus {
                major_radius,
                minor_radius,
                center,
                material,
            } => (
                vec![RawObject {
                    mrrt: [
                        material_map[material],
                        is_refered_to as _,
                        is_rendered as _,
                        6,
                    ],
                    args1: [
                        center.x as _,
                        center.y as _,
                        center.z as _,
                        *major_radius as _,
                    ],
                    args2: [*minor_radius as _, 0.0, 0.0, 0.0],
                }],
                0,
            ),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct World {
    pub max_ray_depth: u32,
    pub sky_color: cgmath::Vector3<f64>,
    pub objects: Vec<Object>,
    pub materials: HashMap<String, Material>,
}

impl World {
    fn to_raw(&self) -> (Vec<RawObject>, Vec<RawMaterial>) {
        let mut materials = vec![];
        let mut material_map = HashMap::new();

        for (name, material) in self.materials {
            materials.push(material.to_raw());
            material_map.insert(name, (materials.len() - 1) as _);
        }

        let mut objects = vec![];
        let mut ref_count = 0;
        for object in &self.objects {
            let (obj_raw, used_refs) = object.to_raw(&material_map, true, false, ref_count);
            ref_count += used_refs;
            objects.extend(obj_raw.into_iter());
        }
        (objects, materials)
    }
}

#[derive(Clone, Copy, Zeroable, Pod)]
#[repr(C)]
struct RawObject {
    mrrt: [u32; 4],
    args1: [f32; 4],
    args2: [f32; 4],
}

#[derive(Clone, Copy, Zeroable, Pod)]
#[repr(C)]
struct RawMaterial {
    color: [f32; 4],
    emitance: [f32; 4],
    mrxx: [f32; 4],
}

// TODO(SpaceCat~Chan): move World to other file
pub struct PixelRenderer {
    objects_buffer: wgpu::Buffer,
    materials_buffer: wgpu::Buffer,

    render_depth: usize,
    screen_size: (u32, u32),
    ray_buffers: Vec<wgpu::Buffer>,
    color_buffers: Vec<wgpu::Buffer>,
    hit_result_buffers: Vec<wgpu::Buffer>,
    random_data_buffers: Vec<wgpu::Buffer>,
    single_random_value: wgpu::Buffer,

    marcher_painter_bind_layout: wgpu::BindGroupLayout,
    marcher_pipeline: wgpu::ComputePipeline,
    painter_pipeline: wgpu::ComputePipeline,
    marcher_painter_bind_groups: Vec<wgpu::BindGroup>,

    render_count: u32,

    accumulate_buffer: wgpu::Buffer,

    collector_vertex_input: wgpu::Buffer,
    collector_state_uniform: wgpu::Buffer,
    collector_bind_layout: wgpu::BindGroupLayout,
    collector_pipeline_layout: wgpu::PipelineLayout,
    collector_pipeline: wgpu::RenderPipeline,
    collector_bind_group: wgpu::BindGroup,
}

impl PixelRenderer {
    fn new(
        world: &World,
        render_depth: usize,
        screen_size: (u32, u32),
        device: &mut wgpu::Device,
        queue: &mut wgpu::Queue,
    ) -> Self {
        let marcher_shader_module =
            device.create_shader_module(wgpu::include_spirv!("marcher.comp.spv"));
        let painter_shader_module =
            device.create_shader_module(wgpu::include_spirv!("painter.comp.spv"));

        let marcher_painter_bind_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("marcher and painter bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 6,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 7,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 8,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 9,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let total_pixel_count = screen_size.0 as u64 * screen_size.1 as u64;

        let (objects, materials) = world.to_raw();
        // TODO(SpaceCat~Chan): use create_buffer_init to fill these
        // with the actual data from "world" immediatly
        let objects_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("object buffer"),
            // 12 floats,
            // vec4 mrrt
            // vec4 args1
            // vec4 args2
            contents: bytemuck::cast_slice(&objects[..]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
        });
        let materials_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("material buffer"),
            // 12 floats,
            // vec4 color
            // vec4 emitance
            // vec4 mrxx
            contents: bytemuck::cast_slice(&materials[..]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
        });

        let marcher_painter_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("marcher/painter pipeline layout"),
                bind_group_layouts: &[&marcher_painter_bind_layout],
                push_constant_ranges: &[],
            });

        let marcher_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&marcher_painter_pipeline_layout),
            module: &marcher_shader_module,
            entry_point: "main",
        });
        let painter_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&marcher_painter_pipeline_layout),
            module: &painter_shader_module,
            entry_point: "main",
        });

        let mut ray_buffers = vec![];
        let mut color_buffers = vec![];
        for _ in 0..(render_depth + 2) {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                // 4 bytes per float, 4+4 floats per pixel
                size: 4 * 8 * total_pixel_count,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            ray_buffers.push(buffer);
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                // 4 bytes per float, 4 floats per pixel
                size: 4 * 4 * total_pixel_count,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            color_buffers.push(buffer);
        }

        let mut hit_result_buffers = vec![];
        for _ in 0..(render_depth + 1) {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                // 1 uint + 4 floats + 3 empty, each is 4 bytes
                size: 4 * 8 * total_pixel_count,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            hit_result_buffers.push(buffer);
        }
        let random_data_buffers = vec![];
        let mut random_data_gen = rand::thread_rng();
        let random_data = vec![0u8; 4 * total_pixel_count as usize];
        for _ in 0..(render_depth + 1) {
            random_data_gen.fill_bytes(&mut random_data[..]);
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("buffer with random data"),
                contents: &random_data[..],
                usage: wgpu::BufferUsages::STORAGE,
            });
            random_data_buffers.push(buffer);
        }
        let single_random_value = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("a single random u32 value"),
            contents: &random_data_gen.next_u32().to_le_bytes(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
        });

        let mut marcher_painter_bind_groups = vec![];
        for index in 0..(render_depth + 1) {
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &marcher_painter_bind_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &objects_buffer,
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &materials_buffer,
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &ray_buffers[index],
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &ray_buffers[index + 1],
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &hit_result_buffers[index],
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &random_data_buffers[index],
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &single_random_value,
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &color_buffers[index],
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 9,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &color_buffers[index + 1],
                            offset: 0,
                            size: None,
                        }),
                    },
                ],
            });
            marcher_painter_bind_groups.push(bind_group);
        }

        let accumulate_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("accumulate buffer: buffer used for acumulating results"),
            // just a vec4
            size: 4 * 4 * total_pixel_count,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        let collector_state_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("collector uniform buffer"),
            // just 2 uints and a float
            size: 4 * 3,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        });

        let collector_vertex_input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vertex input for collector"),
            contents: bytemuck::bytes_of(&[
                -1.0,
                -1.0,
                0.5,
                0.0,
                0.0,
                1.0,
                -1.0,
                0.5,
                screen_size.0 as f32,
                0.0,
                -1.0,
                1.0,
                0.5,
                0.0,
                screen_size.1 as f32,
                1.0,
                1.0,
                0.5,
                screen_size.0 as f32,
                screen_size.1 as f32,
            ]),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let collector_bind_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("bind group layout for collector"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let collector_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("collector pipeline layout"),
                bind_group_layouts: &[&collector_bind_layout],
                push_constant_ranges: &[],
            });

        let collector_vertex_shader_module =
            device.create_shader_module(wgpu::include_spirv!("collector.vert.spv"));
        let collector_fragment_shader_module =
            device.create_shader_module(wgpu::include_spirv!("collector.frag.spv"));

        let collector_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("collector pipeline"),
            layout: Some(&collector_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &collector_vertex_shader_module,
                entry_point: "main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 4 * 5,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2],
                }],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &collector_fragment_shader_module,
                entry_point: "main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
        });

        let collector_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bind group for collector"),
            layout: &collector_bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &color_buffers[0],
                        offset: 0,
                        size: None,
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &accumulate_buffer,
                        offset: 0,
                        size: None,
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &collector_state_uniform,
                        offset: 0,
                        size: None,
                    }),
                },
            ],
        });

        Self {
            objects_buffer,
            materials_buffer,
            render_depth,
            screen_size,
            ray_buffers,
            color_buffers,
            hit_result_buffers,
            random_data_buffers,
            single_random_value,
            marcher_painter_bind_layout,
            marcher_pipeline,
            painter_pipeline,
            marcher_painter_bind_groups,
            render_count: 0,
            accumulate_buffer,
            collector_vertex_input,
            collector_state_uniform,
            collector_bind_layout,
            collector_pipeline_layout,
            collector_pipeline,
            collector_bind_group,
        }
    }

    fn render(
        &mut self,
        render_to: &wgpu::TextureView,
        device: &mut wgpu::Device,
        queue: &mut wgpu::Queue,
        exposure: f32,
    ) {
        let recorder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("raymarch encoder"),
        });
        for index in 0..(self.render_depth + 1) {
            let pass = recorder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None });
            pass.set_pipeline(&self.marcher_pipeline);
            pass.set_bind_group(0, &self.marcher_painter_bind_groups[index], &[]);
            pass.dispatch_workgroups(self.screen_size.0 * self.screen_size.1, 1, 1);
        }
        for index in (0..(self.render_depth + 1)).rev() {
            let pass = recorder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None });
            pass.set_pipeline(&self.painter_pipeline);
            pass.set_bind_group(0, &self.marcher_painter_bind_groups[index], &[]);
            pass.dispatch_workgroups(self.screen_size.0 * self.screen_size.1, 1, 1);
        }
        self.render_count += 1;
        queue.write_buffer(
            &self.collector_state_uniform,
            0,
            bytemuck::bytes_of(&CollectorUniform {
                render_count: self.render_count,
                frame_width: self.screen_size.0,
                exposure,
            }),
        );
        {
            let pass = recorder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("submitting rendered frame to be collected and shown on screen"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: render_to,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });
            pass.set_pipeline(&self.collector_pipeline);
            pass.set_bind_group(0, &self.collector_bind_group, &[]);
            pass.set_vertex_buffer(0, self.collector_vertex_input.slice(..));
            pass.draw(0..4, 0..1);
        }
        let render_thing = recorder.finish();
        queue.submit([render_thing].into_iter());
    }
}

#[repr(C)]
#[derive(bytemuck::Zeroable, bytemuck::Pod, Clone, Copy)]
struct CollectorUniform {
    render_count: u32,
    frame_width: u32,
    exposure: f32,
}
