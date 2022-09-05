use std::{borrow::Cow, collections::HashMap};

use crate::world;
use bytemuck::{Pod, Zeroable};
use cgmath::prelude::*;
use rand::RngCore;
use wgpu::util::DeviceExt;

fn material_to_raw(mat: &world::Material) -> RawMaterial {
    let rotation = cgmath::Quaternion::from_arc(mat.rotation.from, mat.rotation.to, None);
    RawMaterial {
        color: [
            mat.color.x as f32,
            mat.color.y as f32,
            mat.color.z as f32,
            mat.translation.x as f32,
        ],
        emitance: [
            mat.emitance.x as f32,
            mat.emitance.y as f32,
            mat.emitance.z as f32,
            mat.translation.y as f32,
        ],
        mrpx: [
            mat.metalness as f32,
            mat.roughness as f32,
            mat.is_portal as i32 as f32,
            mat.translation.z as f32,
        ],
        rotate_around: [
            mat.rotate_around.x as f32,
            mat.rotate_around.y as f32,
            mat.rotate_around.z as f32,
            0.0,
        ],
        rotation: [
            rotation.v.x as f32,
            rotation.v.y as f32,
            rotation.v.z as f32,
            rotation.s as f32,
        ],
    }
}

fn object_to_raw(
    obj: &world::Object,
    material_map: &HashMap<String, u32>,
    is_rendered: bool,
    is_refered_to: bool,
    current_refer_count: u32,
) -> (Vec<RawObject>, u32) {
    match obj {
        world::Object::Sphere {
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
        world::Object::Box {
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
        world::Object::PosModulo(_, _) => (
            // this one can't be implemented on the gpu just yet
            vec![RawObject {
                mrrt: [0, 0, 0, 2],
                args1: [0.0, 0.0, 0.0, 0.0],
                args2: [0.0, 0.0, 0.0, 0.0],
            }],
            0,
        ),
        world::Object::Inv(inverted) => {
            let (mut inner, used_refers) =
                object_to_raw(inverted, material_map, false, true, current_refer_count);
            inner.push(RawObject {
                mrrt: [0, is_refered_to as _, is_rendered as _, 3],
                args1: [(current_refer_count + used_refers) as f32, 0.0, 0.0, 0.0],
                args2: [0.0, 0.0, 0.0, 0.0],
            });
            (inner, used_refers + 1)
        }
        world::Object::Min(a, b) => {
            let (mut a_inner, used_refers) =
                object_to_raw(a, material_map, false, true, current_refer_count);
            let current_refer_count = current_refer_count + used_refers + 1;
            let (b_inner, used_refers_b) =
                object_to_raw(b, material_map, false, true, current_refer_count);
            a_inner.extend(b_inner.into_iter());
            let total_used_refers = used_refers + used_refers_b;
            a_inner.push(RawObject {
                mrrt: [0, is_refered_to as _, is_rendered as _, 5],
                args1: [
                    (current_refer_count - 1) as _,
                    (current_refer_count + used_refers_b) as _,
                    0.0,
                    0.0,
                ],
                args2: [0.0, 0.0, 0.0, 0.0],
            });
            (a_inner, total_used_refers + 2)
        }
        world::Object::Max(a, b) => {
            let (mut a_inner, used_refers) =
                object_to_raw(a, material_map, false, true, current_refer_count);
            let current_refer_count = current_refer_count + used_refers + 1;
            let (b_inner, used_refers_b) =
                object_to_raw(b, material_map, false, true, current_refer_count);
            a_inner.extend(b_inner.into_iter());
            let total_used_refers = used_refers + used_refers_b;
            a_inner.push(RawObject {
                mrrt: [0, is_refered_to as _, is_rendered as _, 4],
                args1: [
                    (current_refer_count - 1) as _,
                    (current_refer_count + used_refers_b) as _,
                    0.0,
                    0.0,
                ],
                args2: [0.0, 0.0, 0.0, 0.0],
            });
            (a_inner, total_used_refers + 2)
        }
        world::Object::Torus {
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

fn world_to_raw(world: &world::World) -> (Vec<RawObject>, Vec<RawMaterial>) {
    let mut materials = vec![];
    let mut material_map = HashMap::new();

    for (name, material) in &world.materials {
        materials.push(material_to_raw(material));
        material_map.insert(name.clone(), (materials.len() - 1) as _);
    }

    let mut objects = vec![];
    let mut ref_count = 0;
    for object in &world.objects {
        let (obj_raw, used_refs) = object_to_raw(object, &material_map, true, false, ref_count);
        ref_count += used_refs;
        objects.extend(obj_raw.into_iter());
    }
    (objects, materials)
}

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
struct RawObject {
    mrrt: [u32; 4],
    args1: [f32; 4],
    args2: [f32; 4],
}

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
struct RawMaterial {
    // portal translation is packed into the last element of each of the first three elements lol
    color: [f32; 4],
    emitance: [f32; 4],
    mrpx: [f32; 4],
    rotate_around: [f32; 4],
    rotation: [f32; 4],
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
    pub fn new(
        world: &world::World,
        screen_size: (u32, u32),
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Self {
        let render_depth = world.max_ray_depth as usize;

        let marcher_shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("marcher shader"),
            source: wgpu::ShaderSource::Glsl {
                shader: Cow::Borrowed(include_str!("marcher.comp")),
                stage: naga::ShaderStage::Compute,
                defines: HashMap::default(),
            },
        });
        let painter_shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("painter shader"),
            source: wgpu::ShaderSource::Glsl {
                shader: Cow::Borrowed(include_str!("painter.comp")),
                stage: naga::ShaderStage::Compute,
                defines: HashMap::default(),
            },
        });

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

        let (objects, materials) = world_to_raw(world);
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

        let mut initial_ray_buffer = vec![0.0f32; 8 * total_pixel_count as usize];
        for pixel_idx in 0..total_pixel_count {
            let pixel_pos = (
                pixel_idx % screen_size.0 as u64,
                pixel_idx / screen_size.0 as u64,
            );
            let pixel_pos = (
                (pixel_pos.0 as f32 / screen_size.0 as f32 - 0.5) * 2.0,
                -(pixel_pos.1 as f32 / screen_size.1 as f32 - 0.5) * 2.0,
            );
            let final_vec = cgmath::vec3(pixel_pos.0, pixel_pos.1, 1.0).normalize();
            initial_ray_buffer[pixel_idx as usize * 8 + 4] = final_vec.x;
            initial_ray_buffer[pixel_idx as usize * 8 + 5] = final_vec.y;
            initial_ray_buffer[pixel_idx as usize * 8 + 6] = final_vec.z;
        }
        queue.write_buffer(
            &ray_buffers[0],
            0,
            bytemuck::cast_slice(&initial_ray_buffer[..]),
        );

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
        let mut random_data_buffers = vec![];
        let mut random_data_gen = rand::thread_rng();
        let mut random_data = vec![0u8; 4 * total_pixel_count as usize];
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
                -0.5,
                -0.5,
                0.5,
                0.0,
                0.0,
                0.5,
                -0.5,
                0.5,
                screen_size.0 as f32,
                0.0,
                -0.5,
                0.5,
                0.5,
                0.0,
                screen_size.1 as f32,
                0.5,
                0.5,
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
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("collector vertex shader"),
                source: wgpu::ShaderSource::Glsl {
                    shader: Cow::Borrowed(include_str!("collector.vert")),
                    stage: naga::ShaderStage::Vertex,
                    defines: HashMap::default(),
                },
            });
        let collector_fragment_shader_module =
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("collector fragmene shader"),
                source: wgpu::ShaderSource::Glsl {
                    shader: Cow::Borrowed(include_str!("collector.frag")),
                    stage: naga::ShaderStage::Fragment,
                    defines: HashMap::default(),
                },
            });

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
                    format: wgpu::TextureFormat::Bgra8UnormSrgb,
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

    pub fn render(
        &mut self,
        render_to: &wgpu::TextureView,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        exposure: f32,
    ) {
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
        let r: u32 = rand::random();
        queue.write_buffer(&self.single_random_value, 0, &r.to_le_bytes());
        let mut march_recorders = vec![];
        for index in 0..(self.render_depth + 1) {
            let mut recorder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("march encoder"),
            });
            {
                let mut pass =
                    recorder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None });
                pass.set_pipeline(&self.marcher_pipeline);
                pass.set_bind_group(0, &self.marcher_painter_bind_groups[index], &[]);
                pass.dispatch_workgroups(self.screen_size.0, self.screen_size.1, 1);
            }
            march_recorders.push(recorder.finish());
        }
        let mut color_recorders = vec![];
        for index in (0..(self.render_depth + 1)).rev() {
            let mut recorder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("color encoder"),
            });
            {
                let mut pass =
                    recorder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None });
                pass.set_pipeline(&self.painter_pipeline);
                pass.set_bind_group(0, &self.marcher_painter_bind_groups[index], &[]);
                pass.dispatch_workgroups(self.screen_size.0, self.screen_size.1, 1);
            }
            color_recorders.push(recorder.finish());
        }
        let mut recorder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("collector encoder"),
        });
        {
            let mut pass = recorder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
        queue.submit(
            march_recorders
                .into_iter()
                .chain(color_recorders.into_iter())
                .chain([render_thing].into_iter()),
        );
        device.poll(wgpu::Maintain::Wait);
        device.poll(wgpu::Maintain::Wait);
        device.poll(wgpu::Maintain::Wait);
    }
}

#[repr(C)]
#[derive(bytemuck::Zeroable, bytemuck::Pod, Clone, Copy)]
struct CollectorUniform {
    render_count: u32,
    frame_width: u32,
    exposure: f32,
}
