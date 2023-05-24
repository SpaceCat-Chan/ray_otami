mod error_extra;
mod pixel_drawer;
mod world;

use error_extra::*;
use winit::event::MouseScrollDelta;

fn main() {
    match runner() {
        Ok(()) => {}
        Err(e) => panic!("{}", e),
    }
}

fn runner() -> color_eyre::Result<()> {
    env_logger::init();

    let world_filename = std::env::args().nth(1);
    let world_filename = match &world_filename {
        Some(s) => s.as_str(),
        None => {
            println!("no filename given, assuming shapes.ron was meant");
            "shapes.ron"
        }
    };

    let world = ron::de::from_reader(
        std::fs::File::open(world_filename).expect("failed to open shapes file"),
    )
    .expect("failed to deserialize contents of shapes file");

    let event_loop = winit::event_loop::EventLoop::new();
    let window = winit::window::WindowBuilder::new()
        .with_resizable(false)
        .with_inner_size(winit::dpi::LogicalSize::new(960.0f64, 960.0f64))
        .with_title("hi there")
        .build(&event_loop)?;

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(), // was: wgpu::Backends::PRIMARY
        dx12_shader_compiler: wgpu::Dx12Compiler::Fxc,
    });
    let surface = unsafe { instance.create_surface(&window) }?;
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
            limits: wgpu::Limits {
                max_dynamic_storage_buffers_per_pipeline_layout: 4,
                max_storage_buffers_per_shader_stage: 8,
                ..wgpu::Limits::downlevel_defaults()
            },
        },
        None,
    ))?;

    let winit::dpi::PhysicalSize { width, height } = window.inner_size();

    let prefered_surface_format = *surface
        .get_capabilities(&adaptor)
        .formats
        .first()
        .ok_or("failed to get prefered_surface_format")
        .wrap_error()?;
    let surface_config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: prefered_surface_format,
        width,
        height,
        present_mode: wgpu::PresentMode::AutoNoVsync,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: vec![prefered_surface_format],
    };
    surface.configure(&device, &surface_config);

    let mut renderer = pixel_drawer::PixelRenderer::new(
        &world,
        (width, height),
        &device,
        &queue,
        prefered_surface_format,
    );

    let mut exposure = 1.0;

    let mut average_frame_times = 0.0;
    let mut last_time = std::time::Instant::now();

    event_loop.run(move |event, _, control| match event {
        winit::event::Event::WindowEvent {
            event: winit::event::WindowEvent::CloseRequested,
            ..
        }
        | winit::event::Event::WindowEvent {
            event:
                winit::event::WindowEvent::KeyboardInput {
                    input:
                        winit::event::KeyboardInput {
                            state: winit::event::ElementState::Pressed,
                            virtual_keycode: Some(winit::event::VirtualKeyCode::Escape),
                            ..
                        },
                    ..
                },
            ..
        } => {
            *control = winit::event_loop::ControlFlow::Exit;
        }
        winit::event::Event::WindowEvent {
            event:
                winit::event::WindowEvent::MouseWheel {
                    delta: MouseScrollDelta::LineDelta(_, y),
                    ..
                },
            ..
        } => {
            exposure *= 1.1f32.powf(y);
            println!("new exposure: {}", exposure)
        }
        winit::event::Event::MainEventsCleared => {
            control.set_poll();
            let texture = surface.get_current_texture().unwrap();
            renderer.render(
                &texture.texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("texture view for current frame"),
                    format: Some(prefered_surface_format),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    aspect: wgpu::TextureAspect::All,
                    base_mip_level: 0,
                    mip_level_count: None,
                    base_array_layer: 0,
                    array_layer_count: None,
                }),
                &device,
                &queue,
                exposure,
            );
            texture.present();
            let time = std::time::Instant::now();
            let this_time = (time - last_time).as_secs_f64();
            average_frame_times = this_time * 0.05 + average_frame_times * 0.95;
            last_time = time;
            println!(
                "frametime: {} ({} fps)\nthis frame: {} ({} fps)",
                average_frame_times,
                1.0 / average_frame_times,
                this_time,
                1.0 / this_time,
            )
        }
        _ => {}
    });
}
