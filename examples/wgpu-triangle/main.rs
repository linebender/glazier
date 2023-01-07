use glazier::kurbo::Size;
use glazier::{Application, IdleToken, Region, Scalable, WinHandler, WindowHandle};
use std::any::Any;

const WIDTH: usize = 2048;
const HEIGHT: usize = 1536;

use std::borrow::Cow;

struct InnerWindowState {
    window: WindowHandle,
    device: wgpu::Device,
    config: wgpu::SurfaceConfiguration,
    surface: wgpu::Surface,
    queue: wgpu::Queue,
    render_pipeline: wgpu::RenderPipeline,
}

fn surface_size(handle: &WindowHandle) -> (u32, u32) {
    let scale = handle.get_scale().unwrap_or_default();
    let insets = handle.content_insets().to_px(scale);
    let mut size = handle.get_size().to_px(scale);
    size.width -= insets.x_value();
    size.height -= insets.y_value();
    (size.width as u32, size.height as u32)
}

impl InnerWindowState {
    fn create(window: WindowHandle) -> Self {
        let size = surface_size(&window);
        let instance = wgpu::Instance::new(wgpu::Backends::all());
        let surface = unsafe { instance.create_surface(&window) };
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            // Request an adapter which can render to our surface
            compatible_surface: Some(&surface),
        }))
        .expect("Failed to find an appropriate adapter");

        // Create the logical device and command queue
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                features: wgpu::Features::empty(),
                // Make sure we use the texture resolution limits from the adapter, so we can support images the size of the swapchain.
                limits:
                    wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            },
            None,
        ))
        .expect("Failed to create device");

        // Load the shaders from disk
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let swapchain_format = surface.get_supported_formats(&adapter)[0];

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(swapchain_format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: swapchain_format,
            width: size.0,
            height: size.1,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface.get_supported_alpha_modes(&adapter)[0],
        };

        surface.configure(&device, &config);

        Self {
            config,
            surface,
            window,
            device,
            render_pipeline,
            queue,
        }
    }

    #[cfg(target_os = "macos")]
    fn schedule_render(&self) {
        self.window
            .get_idle_handle()
            .unwrap()
            .schedule_idle(IdleToken::new(0));
    }

    #[cfg(not(target_os = "macos"))]
    fn schedule_render(&self) {
        self.window.invalidate();
    }

    fn draw(&mut self) {
        let frame = self
            .surface
            .get_current_texture()
            .expect("Failed to acquire next swap chain texture");
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });
            rpass.set_pipeline(&self.render_pipeline);
            rpass.draw(0..3, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
    }
}

fn main() {
    let app = Application::new().unwrap();
    let mut window_builder = glazier::WindowBuilder::new(app.clone());
    window_builder.resizable(true);
    window_builder.set_size((WIDTH as f64 / 2., HEIGHT as f64 / 2.).into());
    window_builder.set_handler(Box::new(WindowState::new()));
    let window_handle = window_builder.build().unwrap();
    window_handle.show();
    app.run(None);
}

struct WindowState {
    inner: Option<InnerWindowState>,
}

impl WindowState {
    fn new() -> Self {
        Self { inner: None }
    }
}

impl WinHandler for WindowState {
    fn connect(&mut self, handle: &WindowHandle) {
        let inner = InnerWindowState::create(handle.clone());
        inner.schedule_render();
        self.inner = Some(inner);
    }

    fn prepare_paint(&mut self) {}

    fn paint(&mut self, _: &Region) {
        let inner = self.inner.as_mut().unwrap();
        inner.draw();
        inner.schedule_render();
    }

    fn idle(&mut self, _: IdleToken) {
        let inner = self.inner.as_mut().unwrap();
        inner.draw();
        inner.schedule_render();
    }

    fn size(&mut self, _: Size) {
        let inner = self.inner.as_mut().unwrap();
        let size = surface_size(&inner.window);
        inner.config.width = size.0;
        inner.config.height = size.1;
        inner.surface.configure(&inner.device, &inner.config);
    }

    fn request_close(&mut self) {
        self.inner.as_ref().unwrap().window.close();
    }

    fn destroy(&mut self) {
        Application::global().quit()
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}
