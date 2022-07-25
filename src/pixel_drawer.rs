use std::{
    collections::HashMap,
    ops::DerefMut,
    sync::{Arc, Mutex},
    thread::spawn,
};

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
    Smooth {
        alpha: f64,
        objects: Vec<Object>,
    },
}

fn smooth(values: &[f64], alpha: f64) -> (f64, Vec<f64>) {
    let exp_terms: Vec<_> = values.iter().map(|d| (d * alpha).exp()).collect();

    let top_sum: f64 = exp_terms
        .iter()
        .zip(values.iter())
        .map(|(a, b)| a * b)
        .sum();
    let bottom_sum: f64 = exp_terms.iter().sum();

    (top_sum / bottom_sum, exp_terms)
}

fn for_single<T>(vals: Vec<T>, bottom_sum: f64, distances: &[f64]) -> T
where
    T: std::ops::Mul<f64, Output = T> + std::ops::Div<f64, Output = T> + std::iter::Sum,
    f64: std::ops::Mul<T, Output = T>,
{
    let top_sum: T = distances.iter().zip(vals).map(|(&b, c)| c / b).sum();
    top_sum / bottom_sum
}

impl Object {
    fn estimate_distance(&self, point: cgmath::Point3<f64>) -> f64 {
        match self {
            Self::Sphere { center, radius, .. } => point.distance(*center) - radius,
            Self::Box {
                lower_corner,
                upper_corner,
                ..
            } => {
                let center = lower_corner.midpoint(*upper_corner);
                let b = center - lower_corner;

                let q = (point - center).map(|x| x.abs()) - b;
                q.map(|x| x.max(0.0)).distance(cgmath::vec3(0.0, 0.0, 0.0))
                    + q.x.max(q.y.max(q.z)).min(0.0)
            }
            Self::PosModulo(o, period) => o.estimate_distance(point.map(|x| x.rem_euclid(*period))),
            Self::Inv(o) => -o.estimate_distance(point),
            Self::Max(a, b) => a.estimate_distance(point).max(b.estimate_distance(point)),
            Self::Min(a, b) => a.estimate_distance(point).min(b.estimate_distance(point)),
            Self::Torus {
                major_radius,
                minor_radius,
                center,
                ..
            } => {
                let mut point = center - point;
                let mut move_by = point;
                move_by.y = 0.0;
                //if move_by == cgmath::vec3(0.0, 0.0, 0.0) {
                //    move_by = cgmath::vec3(1.0, 0.0, 1.0);
                //}
                let move_by = move_by.normalize_to(*major_radius);
                point -= move_by;

                point.magnitude() - minor_radius
            }
            Self::Smooth { alpha, objects } => {
                let distances: Vec<_> =
                    objects.iter().map(|o| o.estimate_distance(point)).collect();
                smooth(&distances, *alpha).0
            }
        }
    }

    fn get_metadata(
        &self,
        point: cgmath::Point3<f64>,
        material_lookup: &HashMap<String, Material>,
    ) -> (f64, Material) {
        match self {
            Self::Sphere { material, .. } => (
                self.estimate_distance(point),
                *material_lookup.get(material).unwrap_or(&BLACK_MATERIAL),
            ),
            Self::Box { material, .. } => (
                self.estimate_distance(point),
                *material_lookup.get(material).unwrap_or(&BLACK_MATERIAL),
            ),
            Self::PosModulo(o, period) => {
                o.get_metadata(point.map(|x| x.rem_euclid(*period)), material_lookup)
            }
            Self::Inv(o) => {
                let (dist, meta) = o.get_metadata(point, material_lookup);
                (-dist, meta)
            }
            Object::Min(a, b) => {
                let (a_dist, a_meta) = a.get_metadata(point, material_lookup);
                let (b_dist, b_meta) = b.get_metadata(point, material_lookup);
                if a_dist < b_dist {
                    (a_dist, a_meta)
                } else {
                    (b_dist, b_meta)
                }
            }
            Object::Max(a, b) => {
                let (a_dist, a_meta) = a.get_metadata(point, material_lookup);
                let (b_dist, b_meta) = b.get_metadata(point, material_lookup);
                if a_dist > b_dist {
                    (a_dist, a_meta)
                } else {
                    (b_dist, b_meta)
                }
            }
            Self::Torus { material, .. } => (
                self.estimate_distance(point),
                *material_lookup.get(material).unwrap_or(&BLACK_MATERIAL),
            ),
            Self::Smooth { alpha, objects } => {
                let materials: Vec<_> = objects
                    .iter()
                    .map(|o| o.get_metadata(point, material_lookup))
                    .collect();
                let distances: Vec<_> = materials.iter().map(|(d, _)| *d).collect();
                let (final_distance, mut exp_terms) = smooth(&distances, *alpha);

                if *alpha < 0.0 {
                    exp_terms = exp_terms.into_iter().map(|v| 1.0 / v).collect();
                }
                let bottom_sum = exp_terms.iter().sum();

                let roughness = for_single(
                    materials
                        .iter()
                        .map(|(_, m)| m.roughness)
                        .collect::<Vec<_>>(),
                    bottom_sum,
                    &exp_terms,
                )
                .clamp(0.0, 1.0);
                let metalness = for_single(
                    materials
                        .iter()
                        .map(|(_, m)| m.metalness)
                        .collect::<Vec<_>>(),
                    bottom_sum,
                    &exp_terms,
                )
                .clamp(0.0, 1.0);
                let color = for_single(
                    materials.iter().map(|(_, m)| m.color).collect::<Vec<_>>(),
                    bottom_sum,
                    &exp_terms,
                );
                let emitance = for_single(
                    materials
                        .iter()
                        .map(|(_, m)| m.emitance)
                        .collect::<Vec<_>>(),
                    bottom_sum,
                    &exp_terms,
                );

                (
                    final_distance,
                    Material {
                        color,
                        emitance,
                        metalness,
                        roughness,
                    },
                )
            }
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

static BLACK: cgmath::Vector3<f64> = cgmath::vec3(0.0, 0.0, 0.0);
static BLACK_MATERIAL: Material = Material {
    color: BLACK,
    emitance: BLACK,
    metalness: 0.0,
    roughness: 1.0,
};

impl World {
    fn estimate_distance(&self, point: cgmath::Point3<f64>) -> f64 {
        self.objects
            .iter()
            .map(|x| x.estimate_distance(point))
            .reduce(f64::min)
            .unwrap_or(0.0)
    }

    fn get_closest_metadata(&self, point: cgmath::Point3<f64>) -> Material {
        self.objects
            .iter()
            .map(|x| x.get_metadata(point, &self.materials))
            .reduce(|acc, x| if x.0 < acc.0 { x } else { acc })
            .map(|(_, mat)| mat)
            .unwrap_or(BLACK_MATERIAL)
    }

    fn get_distance_gradient(&self, point: cgmath::Point3<f64>) -> cgmath::Vector3<f64> {
        let x_neg = self.estimate_distance(point + cgmath::vec3(-0.005, 0.0, 0.0));
        let x_pos = self.estimate_distance(point + cgmath::vec3(0.005, 0.0, 0.0));
        let y_neg = self.estimate_distance(point + cgmath::vec3(0.0, -0.005, 0.0));
        let y_pos = self.estimate_distance(point + cgmath::vec3(0.0, 0.005, 0.0));
        let z_neg = self.estimate_distance(point + cgmath::vec3(0.0, 0.0, -0.005));
        let z_pos = self.estimate_distance(point + cgmath::vec3(0.0, 0.0, 0.005));
        cgmath::vec3(x_pos - x_neg, y_pos - y_neg, z_pos - z_neg)
    }
}

// TODO(SpaceCat~Chan): move World to other file
pub struct PixelRenderer {
    world: World,

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
        world: World,
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

        // TODO(SpaceCat~Chan): use create_buffer_init to fill these
        // with the actual data from "world" immediatly
        let objects_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("object buffer"),
            // 12 floats,
            // vec4 mrrt
            // vec4 args1
            // vec4 args2
            size: 4 * 12 * total_pixel_count,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let materials_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("material buffer"),
            // 12 floats,
            // vec4 color
            // vec4 emitance
            // vec4 mrxx
            size: 4 * 12 * 1,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
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
            world,
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
