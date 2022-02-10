mod error_extra;
mod pixel_drawer;

use std::{
    ops::Add,
    sync::{Arc, Mutex},
};

use error_extra::*;
use wgpu::util::DeviceExt;

fn main() {
    match runner() {
        Ok(()) => {}
        Err(e) => panic!("{}", e),
    }
}

fn runner() -> color_eyre::Result<()> {
    env_logger::init();
    let event_loop = winit::event_loop::EventLoop::new();
    let window = winit::window::WindowBuilder::new()
        .with_inner_size(winit::dpi::LogicalSize::new(960.0, 960.0))
        .with_title("hi there")
        .build(&event_loop)?;

    let instance = wgpu::Instance::new(wgpu::Backends::GL);
    let surface = unsafe { instance.create_surface(&window) };
    let adaptor =
        futures::executor::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .ok_or("unable to get adaptor")
        .wrap_error()?;
    let (device, queue) = futures::executor::block_on(adaptor.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("the gpu, dumbass"),
            features: wgpu::Features::empty(),
            limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))?;

    let winit::dpi::PhysicalSize { width, height } = window.inner_size();
    let preffered_surface_format = surface
        .get_preferred_format(&adaptor)
        .ok_or("failed to get preffered_surface_format")
        .wrap_error()?;
    let surface_config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: preffered_surface_format,
        width,
        height,
        present_mode: wgpu::PresentMode::Mailbox,
    };
    surface.configure(&device, &surface_config);

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("bind group layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler {
                    filtering: true,
                    comparison: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("pipeline layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    let shader = device.create_shader_module(&wgpu::include_wgsl!("shader.wgsl"));

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("pipeline"),
        layout: Some(&pipeline_layout),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Cw,
            cull_mode: None,
            clamp_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vert_main",
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "frag_main",
            targets: &[wgpu::ColorTargetState {
                format: preffered_surface_format,
                blend: None,
                write_mask: wgpu::ColorWrites::all(),
            }],
        }),
    });

    let world = pixel_drawer::World {
        max_ray_depth: 4,
        ray_reflections: 1,
        sky_color: cgmath::vec3(0.529, 0.808, 0.98),
        objects: vec![
            pixel_drawer::Object::Sphere {
                center: cgmath::point3(0.0, 0.0, 2.0),
                radius: 0.5,
                metadata: pixel_drawer::Metadata {
                    color: cgmath::vec3(1.0, 1.0, 0.0),
                    emitance: cgmath::vec3(0.0, 0.0, 0.0),
                    metalness: 0.0,
                    roughness: 0.1,
                },
            },
            pixel_drawer::Object::Box {
                lower_corner: cgmath::point3(-5.0, -5.0, 5.0),
                upper_corner: cgmath::point3(5.0, 5.0, 5.5),
                metadata: pixel_drawer::Metadata {
                    color: cgmath::vec3(0.0, 1.0, 0.0),
                    emitance: cgmath::vec3(0.0, 0.0, 0.0),
                    metalness: 0.0,
                    roughness: 0.7,
                },
            },
            pixel_drawer::Object::Box {
                lower_corner: cgmath::point3(-5.0, 0.5, 0.0),
                upper_corner: cgmath::point3(5.0, 1.5, 5.5),
                metadata: pixel_drawer::Metadata {
                    color: cgmath::vec3(1.0, 1.0, 1.0),
                    emitance: cgmath::vec3(0.0, 0.0, 0.0),
                    metalness: 1.0,
                    roughness: 0.02,
                },
            },
            pixel_drawer::Object::Sphere {
                center: cgmath::point3(0.5, 0.0, 1.0),
                radius: 0.25,
                metadata: pixel_drawer::Metadata {
                    color: cgmath::vec3(0.0, 0.0, 0.0),
                    emitance: cgmath::vec3(100.0, 100.0, 100.0),
                    metalness: 0.0,
                    roughness: 0.0,
                },
            },
        ],
    };

    let transfer_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("transfer texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
    });

    let transfer_texture_view = transfer_texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("transfer texture view"),
        format: None,
        dimension: None,
        aspect: wgpu::TextureAspect::All,
        base_mip_level: 0,
        mip_level_count: None,
        base_array_layer: 0,
        array_layer_count: None,
    });

    let transfer_texture_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("transfer texture sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        lod_min_clamp: 0.0,
        lod_max_clamp: 1.0,
        compare: None,
        anisotropy_clamp: None,
        border_color: None,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("finished render bind group"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&transfer_texture_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&transfer_texture_view),
            },
        ],
    });

    let buffer_contents = Arc::new(Mutex::new(vec![0; (width * height * 4) as _]));
    let that_one = buffer_contents.clone();
    std::thread::spawn(move || pixel_drawer::render_to_buffer(that_one, (width, height), &world));
    event_loop.run(move |event, _, control| {
        if let winit::event::Event::WindowEvent {
            event: winit::event::WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control = winit::event_loop::ControlFlow::Exit;
        } else {
            *control = winit::event_loop::ControlFlow::WaitUntil(
                std::time::Instant::now().add(std::time::Duration::from_secs_f64(0.0166666)),
            )
        }
        let texture = surface.get_current_texture().unwrap();
        let texture_view = texture.texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("render texture view"),
            format: Some(preffered_surface_format),
            dimension: Some(wgpu::TextureViewDimension::D2),
            aspect: wgpu::TextureAspect::All,
            base_mip_level: 0,
            mip_level_count: None,
            base_array_layer: 0,
            array_layer_count: None,
        });
        let buffer_contents = buffer_contents.lock().unwrap();
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Temp Buffer"),
            contents: &buffer_contents,
            usage: wgpu::BufferUsages::COPY_SRC,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("texture_buffer_copy_encoder"),
        });

        encoder.copy_buffer_to_texture(
            wgpu::ImageCopyBuffer {
                buffer: &buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some((4 * width).try_into().unwrap()),
                    rows_per_image: Some(height.try_into().unwrap()),
                },
            },
            transfer_texture.as_image_copy(),
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render pass"),
                color_attachments: &[wgpu::RenderPassColorAttachment {
                    view: &texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: true,
                    },
                }],
                depth_stencil_attachment: None,
            });
            rpass.set_pipeline(&pipeline);
            rpass.set_bind_group(0, &bind_group, &[]);
            rpass.draw(0..6, 0..1);
        }
        queue.submit(std::iter::once(encoder.finish()));
        texture.present();
    });
}
