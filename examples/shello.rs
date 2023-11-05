use std::any::Any;

#[cfg(feature = "accesskit")]
use accesskit::TreeUpdate;
use parley::FontContext;
use tracing_subscriber::EnvFilter;
use vello::util::{RenderContext, RenderSurface};
use vello::Renderer;
use vello::{
    kurbo::{Affine, PathEl, Point, Rect},
    peniko::{Brush, Color, Fill, Mix, Stroke},
    RenderParams, RendererOptions, Scene, SceneBuilder,
};

use glazier::kurbo::Size;
use glazier::{
    Application, Cursor, FileDialogToken, FileInfo, IdleToken, KeyEvent, PointerEvent, Region,
    Scalable, TimerToken, WinHandler, WindowHandle,
};

mod common;
use common::text::{self, ParleyBrush};

const WIDTH: usize = 2048;
const HEIGHT: usize = 1536;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let app = Application::new().unwrap();
    let window = glazier::WindowBuilder::new(app.clone())
        .size((WIDTH as f64 / 2., HEIGHT as f64 / 2.).into())
        .handler(Box::new(WindowState::new()))
        .build()
        .unwrap();
    window.show();
    app.run(None);
}

struct WindowState {
    handle: WindowHandle,
    renderer: Option<Renderer>,
    render: RenderContext,
    surface: Option<RenderSurface>,
    scene: Scene,
    size: Size,
    font_context: FontContext,
    counter: u64,
}

impl WindowState {
    pub fn new() -> Self {
        let render = RenderContext::new().unwrap();
        Self {
            handle: Default::default(),
            surface: None,
            renderer: None,
            render,
            scene: Default::default(),
            font_context: FontContext::new(),
            counter: 0,
            size: Size::new(800.0, 600.0),
        }
    }

    fn schedule_render(&self) {
        self.handle.invalidate();
    }

    fn surface_size(&self) -> (u32, u32) {
        let handle = &self.handle;
        let scale = handle.get_scale().unwrap_or_default();
        let insets = handle.content_insets().to_px(scale);
        let mut size = handle.get_size().to_px(scale);
        size.width -= insets.x_value();
        size.height -= insets.y_value();
        (size.width as u32, size.height as u32)
    }

    fn render(&mut self) {
        let (width, height) = self.surface_size();
        if self.surface.is_none() {
            self.surface = Some(
                pollster::block_on(self.render.create_surface(&self.handle, width, height))
                    .expect("failed to create surface"),
            );
        }

        render_anim_frame(&mut self.scene, &mut self.font_context, self.counter);
        self.counter += 1;

        if let Some(surface) = self.surface.as_mut() {
            if surface.config.width != width || surface.config.height != height {
                self.render.resize_surface(surface, width, height);
            }
            let surface_texture = surface.surface.get_current_texture().unwrap();
            let dev_id = surface.dev_id;
            let device = &self.render.devices[dev_id].device;
            let queue = &self.render.devices[dev_id].queue;
            let renderer_options = RendererOptions {
                surface_format: Some(surface.format),
                timestamp_period: queue.get_timestamp_period(),
            };
            let render_params = RenderParams {
                base_color: Color::BLACK,
                width,
                height,
            };
            self.renderer
                .get_or_insert_with(|| Renderer::new(device, &renderer_options).unwrap())
                .render_to_surface(device, queue, &self.scene, &surface_texture, &render_params)
                .unwrap();
            surface_texture.present();
        }
    }
}

impl WinHandler for WindowState {
    fn connect(&mut self, handle: &WindowHandle) {
        self.handle = handle.clone();
        self.schedule_render();
    }

    fn prepare_paint(&mut self) {}

    fn paint(&mut self, _: &Region) {
        self.render();
        self.schedule_render();
    }

    #[cfg(feature = "accesskit")]
    fn accesskit_tree(&mut self) -> TreeUpdate {
        // TODO: Construct a real TreeUpdate
        use accesskit::{NodeBuilder, NodeClassSet, NodeId, Role, Tree};
        let builder = NodeBuilder::new(Role::Window);
        let mut node_classes = NodeClassSet::new();
        let node = builder.build(&mut node_classes);
        const WINDOW_ID: NodeId = NodeId(0);
        TreeUpdate {
            nodes: vec![(WINDOW_ID, node)],
            tree: Some(Tree::new(WINDOW_ID)),
            focus: WINDOW_ID,
        }
    }

    fn idle(&mut self, _: IdleToken) {}

    fn command(&mut self, _id: u32) {}

    fn open_file(&mut self, _token: FileDialogToken, file_info: Option<FileInfo>) {
        println!("open file result: {file_info:?}");
    }

    fn save_as(&mut self, _token: FileDialogToken, file: Option<FileInfo>) {
        println!("save file result: {file:?}");
    }

    fn key_down(&mut self, event: &KeyEvent) -> bool {
        println!("keydown: {event:?}");
        false
    }

    fn key_up(&mut self, event: &KeyEvent) {
        println!("keyup: {event:?}");
    }

    fn wheel(&mut self, event: &PointerEvent) {
        println!("wheel {event:?}");
    }

    fn pointer_move(&mut self, _event: &PointerEvent) {
        self.handle.set_cursor(&Cursor::Arrow);
        //println!("pointer_move {event:?}");
    }

    fn pointer_down(&mut self, event: &PointerEvent) {
        println!("pointer_down {event:?}");
    }

    fn pointer_up(&mut self, event: &PointerEvent) {
        println!("pointer_up {event:?}");
    }

    fn timer(&mut self, id: TimerToken) {
        println!("timer fired: {id:?}");
    }

    fn size(&mut self, size: Size) {
        self.size = size;
    }

    fn got_focus(&mut self) {
        println!("Got focus");
    }

    fn lost_focus(&mut self) {
        println!("Lost focus");
    }

    fn request_close(&mut self) {
        self.handle.close();
    }

    fn destroy(&mut self) {
        Application::global().quit()
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

pub fn render_anim_frame(scene: &mut Scene, fcx: &mut FontContext, i: u64) {
    let mut sb = SceneBuilder::for_scene(scene);
    let rect = Rect::from_origin_size(Point::new(0.0, 0.0), (1000.0, 1000.0));
    sb.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        &Brush::Solid(Color::rgb8(128, 128, 128)),
        None,
        &rect,
    );

    let scale = (i as f64 * 0.01).sin() * 0.5 + 1.5;
    let mut lcx = parley::LayoutContext::new();
    let mut layout_builder =
        lcx.ranged_builder(fcx, "Hello vello! ഹലോ ਸਤ ਸ੍ਰੀ ਅਕਾਲ مرحبا!", scale as f32);
    layout_builder.push_default(&parley::style::StyleProperty::FontSize(34.0));
    layout_builder.push(
        &parley::style::StyleProperty::Brush(ParleyBrush(Brush::Solid(Color::rgb8(255, 255, 0)))),
        6..10,
    );
    layout_builder.push(&parley::style::StyleProperty::FontSize(48.0), 6..12);
    layout_builder.push(
        &parley::style::StyleProperty::Brush(ParleyBrush(Color::YELLOW.into())),
        6..12,
    );
    layout_builder.push_default(&parley::style::StyleProperty::Brush(ParleyBrush(
        Brush::Solid(Color::rgb8(255, 255, 255)),
    )));
    let mut layout = layout_builder.build();
    layout.break_all_lines(None, parley::layout::Alignment::Start);
    text::render_text(&mut sb, Affine::translate((100.0, 400.0)), &layout);

    let th = (std::f64::consts::PI / 180.0) * (i as f64);
    let center = Point::new(500.0, 500.0);
    let mut p1 = center;
    p1.x += 400.0 * th.cos();
    p1.y += 400.0 * th.sin();
    sb.stroke(
        &Stroke::new(5.0),
        Affine::IDENTITY,
        &Brush::Solid(Color::rgb8(128, 0, 0)),
        None,
        &[PathEl::MoveTo(center), PathEl::LineTo(p1)],
    );
    sb.fill(
        Fill::NonZero,
        Affine::translate((150.0, 150.0)) * Affine::scale(0.2),
        Color::RED,
        None,
        &rect,
    );
    let alpha = (i as f64 * 0.03).sin() as f32 * 0.5 + 0.5;
    sb.push_layer(Mix::Normal, alpha, Affine::IDENTITY, &rect);
    sb.fill(
        Fill::NonZero,
        Affine::translate((100.0, 100.0)) * Affine::scale(0.2),
        Color::BLUE,
        None,
        &rect,
    );
    sb.fill(
        Fill::NonZero,
        Affine::translate((200.0, 200.0)) * Affine::scale(0.2),
        Color::GREEN,
        None,
        &rect,
    );
    sb.pop_layer();
}
