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
        .with_resizable(false)
        .with_inner_size(winit::dpi::LogicalSize::new(960.0, 960.0))
        .with_title("hi there")
        .build(&event_loop)?;

    let instance = wgpu::Instance::new(wgpu::Backends::PRIMARY);
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
        usage: wgpu::TextureUsages::COPY_DST,
        format: preffered_surface_format,
        width,
        height,
        present_mode: wgpu::PresentMode::Mailbox,
    };
    surface.configure(&device, &surface_config);

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
                    color: cgmath::vec3(1.0, 0.0, 0.0),
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
                    roughness: 0.01,
                },
            },
            pixel_drawer::Object::Torus {
                major_radius: 0.5,
                minor_radius: 0.1,
                center: cgmath::point3(0.0, -1.0, 2.5),
                metadata: pixel_drawer::Metadata {
                    color: cgmath::vec3(1.0, 0.0, 1.0),
                    emitance: cgmath::vec3(0.0, 0.0, 0.0),
                    metalness: 0.0,
                    roughness: 0.75,
                },
            },
        ],
    };

    let buffer_contents = Arc::new(Mutex::new(vec![0; (width * height * 4) as _]));
    let that_one = buffer_contents.clone();
    std::thread::spawn(move || pixel_drawer::render_to_buffer(that_one, (width, height), &world));
    event_loop.run(move |event, _, control| match event {
        winit::event::Event::WindowEvent {
            event: winit::event::WindowEvent::CloseRequested,
            ..
        } => {
            *control = winit::event_loop::ControlFlow::Exit;
        }
        winit::event::Event::MainEventsCleared => {
            *control = winit::event_loop::ControlFlow::WaitUntil(
                std::time::Instant::now().add(std::time::Duration::from_secs_f64(0.0166666)),
            );
            let texture = surface.get_current_texture().unwrap();
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
                wgpu::ImageCopyTexture {
                    texture: &texture.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit(std::iter::once(encoder.finish()));
            texture.present();
        }
        _ => {}
    });
}
